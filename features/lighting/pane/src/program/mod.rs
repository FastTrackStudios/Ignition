//! Programming a show: the actions a person, an agent or a Luau script
//! builds lighting with.
//!
//! Every action is `run(id, args) -> json`: it reads the files it needs,
//! edits them in memory, and writes them together when it has succeeded
//! (a failed action writes nothing). What it wrote is one step of the
//! history (`data/.history`), so `IGNITION_UNDO` takes it back; a script
//! that opens an edit (`IGNITION_EDIT_BEGIN` … `IGNITION_EDIT_COMMIT`)
//! or uses `IGNITION_BATCH` makes its whole run one step.
//!
//! Nothing here opens a window or moves the one that is open: an action
//! names the song, set or environment it works on in its arguments.
//!
//! The catalogue (`actions.json`, beside the manifest) is the one list of
//! ids, descriptions and arguments: the extension registers from it, and
//! `tools/program_actions.py` writes the manifest's `[[actions]]` and the
//! docs' table from it.
//!
//! - [`library`]: colours, palettes, splits, effects, looks, focus points,
//!   beam presets and trick presets, in three layers.
//! - [`song`]: cues, sections, markers, fills, and the same over a set.
//! - [`env`]: environments (rooms), people, fixtures, groups and the patch.
//! - [`tricks`]: units, grids and spreads (grandMA3's MAtricks), effects
//!   made from parameters, and previews.

// The arithmetic here is on room coordinates, bars and counts of
// fixtures, none of which approach a float's or an integer's limits.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::too_many_lines,
    clippy::missing_errors_doc,
    clippy::module_name_repetitions,
    clippy::option_if_let_else,
    clippy::cognitive_complexity,
    clippy::needless_pass_by_value,
    clippy::similar_names,
    clippy::implicit_hasher,
    // Style lints this module's JSON-walking code trips often; the base
    // of this crate is not clean under them either (session.rs, cues.rs).
    clippy::redundant_clone,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    clippy::collapsible_if,
    clippy::single_match_else,
    clippy::explicit_counter_loop,
    clippy::unnecessary_wraps,
    clippy::must_use_candidate,
    clippy::or_fun_call,
    clippy::too_long_first_doc_paragraph,
    clippy::case_sensitive_file_extension_comparisons
)]

pub mod env;
pub mod library;
pub mod song;
pub mod tricks;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde_json::{Map, Value, json};

/// What an action answers.
pub type Res = Result<Value, String>;

/// The catalogue: every action's id, name, description, effect and
/// arguments.
pub const CATALOGUE: &str = include_str!("../../actions.json");

/// The ids in the catalogue.
#[must_use]
pub fn ids() -> Vec<String> {
    serde_json::from_str::<Value>(CATALOGUE)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|a| a.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

// ---------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------

pub(crate) fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("`{key}` is required (text)"))
}

pub(crate) fn opt_text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

pub(crate) fn num(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(Value::as_f64)
}

pub(crate) fn flag(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A float narrowed for the engine's `f32` fields.
#[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
pub(crate) const fn f32_of(v: f64) -> f32 {
    v as f32
}

/// A count as a float.
#[allow(clippy::as_conversions, clippy::cast_precision_loss)]
pub(crate) const fn count_f(v: usize) -> f64 {
    v as f64
}

/// A whole, non-negative number out of JSON.
pub(crate) fn whole(v: &Value) -> Option<u32> {
    v.as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| {
            v.as_f64().filter(|f| *f >= 0.0 && f.fract() == 0.0).and_then(|f| {
                #[allow(
                    clippy::as_conversions,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                let n = f as u64;
                u32::try_from(n).ok()
            })
        })
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// `Drive To LA` → `drive-to-la`.
#[must_use]
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c == '\'' || c == '’' {
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

pub(crate) fn obj(v: &mut Value) -> Result<&mut Map<String, Value>, String> {
    v.as_object_mut().ok_or_else(|| "expected an object".to_owned())
}

pub(crate) fn arr<'a>(v: &'a mut Value, key: &str) -> Result<&'a mut Vec<Value>, String> {
    let map = obj(v)?;
    map.entry(key.to_owned())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| format!("`{key}` is not a list"))
}

pub(crate) fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// `patch` laid over `base`, key by key; a `null` removes the key.
pub(crate) fn merge(base: &mut Value, patch: &Value) {
    match (base.as_object_mut(), patch.as_object()) {
        (Some(base), Some(patch)) => {
            for (k, v) in patch {
                if v.is_null() {
                    base.remove(k);
                } else if v.is_object() && base.get(k).is_some_and(Value::is_object) {
                    if let Some(slot) = base.get_mut(k) {
                        merge(slot, v);
                    }
                } else {
                    base.insert(k.clone(), v.clone());
                }
            }
        }
        _ => *base = patch.clone(),
    }
}

// ---------------------------------------------------------------------
// Files and history
// ---------------------------------------------------------------------

/// One action's view of the files: reads see its own writes, and nothing
/// reaches the disk until [`Ctx::commit`].
pub struct Ctx {
    /// The lighting data root: the folder holding `data/`.
    pub root: PathBuf,
    writes: BTreeMap<PathBuf, (Option<String>, String)>,
}

/// An edit a script holds open across several actions: the files each
/// wrote, with what they held before the first.
struct Open {
    label: String,
    root: PathBuf,
    files: BTreeMap<PathBuf, (Option<String>, String)>,
}

static OPEN: Mutex<Option<Open>> = Mutex::new(None);

fn root_of(args: &Value) -> PathBuf {
    opt_text(args, "root")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("IGNITION_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Ctx {
    #[must_use]
    pub fn new(args: &Value) -> Self {
        Self {
            root: root_of(args),
            writes: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    /// A path as written, or under the root when it is relative.
    #[must_use]
    pub fn path(&self, given: &str) -> PathBuf {
        let p = Path::new(given);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    pub fn exists(&self, path: &Path) -> bool {
        self.writes.contains_key(path) || path.is_file()
    }

    pub fn read(&self, path: &Path) -> Result<String, String> {
        if let Some((_, now)) = self.writes.get(path) {
            return Ok(now.clone());
        }
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn json(&self, path: &Path) -> Res {
        serde_json::from_str(&self.read(path)?).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// The file as JSON, or `otherwise` when there is none.
    pub fn json_or(&self, path: &Path, otherwise: Value) -> Res {
        if self.exists(path) {
            self.json(path)
        } else {
            Ok(otherwise)
        }
    }

    pub fn write(&mut self, path: &Path, text: String) {
        let before = match self.writes.remove(path) {
            Some((before, _)) => before,
            None => std::fs::read_to_string(path).ok(),
        };
        self.writes.insert(path.to_path_buf(), (before, text));
    }

    pub fn write_json(&mut self, path: &Path, value: &Value) -> Result<(), String> {
        let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
        text.push('\n');
        self.write(path, text);
        Ok(())
    }

    /// Writes what the action changed and records it: one history step,
    /// or part of the edit a script holds open. The files written.
    pub fn commit(self, action: &str) -> Result<Vec<String>, String> {
        let changed: BTreeMap<PathBuf, (Option<String>, String)> = self
            .writes
            .into_iter()
            .filter(|(_, (before, now))| before.as_deref() != Some(now.as_str()))
            .collect();
        for (path, (_, now)) in &changed {
            write_atomically(path, now)?;
        }
        let names = changed.keys().map(|p| p.display().to_string()).collect();
        if changed.is_empty() {
            return Ok(names);
        }
        let mut open = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(open) = open.as_mut() {
            for (path, (before, now)) in changed {
                match open.files.get_mut(&path) {
                    Some(slot) => slot.1 = now,
                    None => {
                        open.files.insert(path, (before, now));
                    }
                }
            }
        } else {
            drop(open);
            record(&self.root, action, changed)?;
        }
        Ok(names)
    }
}

fn write_atomically(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".writing");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

fn history_dir(root: &Path) -> PathBuf {
    root.join("data/.history")
}

/// The step the files are at: 0 with no history.
fn head(root: &Path) -> u64 {
    std::fs::read_to_string(history_dir(root).join("head"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn set_head(root: &Path, at: u64) -> Result<(), String> {
    write_atomically(&history_dir(root).join("head"), &at.to_string())
}

fn step_path(root: &Path, n: u64) -> PathBuf {
    history_dir(root).join(format!("{n:06}.json"))
}

fn record(
    root: &Path,
    label: &str,
    files: BTreeMap<PathBuf, (Option<String>, String)>,
) -> Result<(), String> {
    let at = head(root);
    // A new edit after an undo drops what was undone.
    let mut n = at + 1;
    while step_path(root, n).is_file() {
        let _ = std::fs::remove_file(step_path(root, n));
        n += 1;
    }
    let files: Vec<Value> = files
        .into_iter()
        .map(|(path, (before, after))| json!({ "path": path, "before": before, "after": after }))
        .collect();
    let when = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let step = json!({ "step": at + 1, "label": label, "unix": when, "files": files });
    write_atomically(
        &step_path(root, at + 1),
        &serde_json::to_string(&step).map_err(|e| e.to_string())?,
    )?;
    set_head(root, at + 1)
}

fn step(root: &Path, n: u64) -> Res {
    let path = step_path(root, n);
    let raw = std::fs::read_to_string(&path).map_err(|_| format!("no history step {n}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", path.display()))
}

/// Puts a step's files to their `before` (undo) or `after` (redo).
fn restore(step: &Value, side: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for file in list(step, "files") {
        let Some(path) = file.get("path").and_then(Value::as_str) else {
            continue;
        };
        match file.get(side).and_then(Value::as_str) {
            Some(text) => write_atomically(Path::new(path), text)?,
            None => {
                let _ = std::fs::remove_file(path);
            }
        }
        out.push(path.to_owned());
    }
    Ok(out)
}

fn history(args: &Value) -> Res {
    let root = root_of(args);
    let at = head(&root);
    let mut steps = Vec::new();
    let mut n = 1;
    while let Ok(s) = step(&root, n) {
        steps.push(json!({
            "step": n,
            "label": s.get("label"),
            "unix": s.get("unix"),
            "files": list(&s, "files").iter().filter_map(|f| f.get("path").cloned()).collect::<Vec<_>>(),
            "applied": n <= at,
        }));
        n += 1;
    }
    let limit = num(args, "limit").map_or(50, |l| {
        #[allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let l = l.max(1.0) as usize;
        l
    });
    let skip = steps.len().saturating_sub(limit);
    Ok(json!({ "head": at, "steps": steps.into_iter().skip(skip).collect::<Vec<_>>() }))
}

fn undo(args: &Value) -> Res {
    let root = root_of(args);
    let at = head(&root);
    if at == 0 {
        return Err("nothing to undo".to_owned());
    }
    let s = step(&root, at)?;
    let files = restore(&s, "before")?;
    set_head(&root, at - 1)?;
    Ok(json!({ "undone": s.get("label"), "step": at, "files": files }))
}

fn redo(args: &Value) -> Res {
    let root = root_of(args);
    let at = head(&root);
    let s = step(&root, at + 1).map_err(|_| "nothing to redo".to_owned())?;
    let files = restore(&s, "after")?;
    set_head(&root, at + 1)?;
    Ok(json!({ "redone": s.get("label"), "step": at + 1, "files": files }))
}

fn edit_begin(args: &Value) -> Res {
    // An edit a script left open (it stopped on an error) is kept as its
    // own step, never folded into the next one.
    let stale = OPEN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|o| o.label.clone());
    if stale.is_some() {
        edit_end(true)?;
    }
    let label = opt_text(args, "label").unwrap_or("script").to_owned();
    *OPEN.lock().unwrap_or_else(PoisonError::into_inner) = Some(Open {
        label: label.clone(),
        root: root_of(args),
        files: BTreeMap::new(),
    });
    Ok(json!({ "open": label, "closed_left_open": stale }))
}

fn edit_end(keep: bool) -> Res {
    let open = OPEN.lock().unwrap_or_else(PoisonError::into_inner).take();
    let Some(open) = open else {
        return Err("no edit is open".to_owned());
    };
    let files: Vec<String> = open.files.keys().map(|p| p.display().to_string()).collect();
    if keep {
        if !open.files.is_empty() {
            record(&open.root, &open.label, open.files)?;
        }
        Ok(json!({ "committed": open.label, "files": files, "step": head(&open.root) }))
    } else {
        for (path, (before, _)) in &open.files {
            match before {
                Some(text) => write_atomically(path, text)?,
                None => {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
        Ok(json!({ "aborted": open.label, "files": files }))
    }
}

fn batch(args: &Value) -> Res {
    let label = opt_text(args, "label").unwrap_or("batch");
    let ours = OPEN.lock().unwrap_or_else(PoisonError::into_inner).is_none();
    if ours {
        edit_begin(&json!({ "label": label, "root": args.get("root") }))?;
    }
    let mut results = Vec::new();
    for (i, s) in list(args, "steps").iter().enumerate() {
        let id = s.get("action").and_then(Value::as_str).unwrap_or_default();
        let mut step_args = s.get("args").cloned().unwrap_or_else(|| json!({}));
        if let (Some(root), Some(map)) = (args.get("root"), step_args.as_object_mut()) {
            map.entry("root").or_insert_with(|| root.clone());
        }
        match run(id, &step_args) {
            Ok(v) => results.push(v),
            Err(e) => {
                if ours {
                    edit_end(false)?;
                }
                return Err(format!("step {} ({id}): {e}; nothing of the batch was kept", i + 1));
            }
        }
    }
    if ours {
        edit_end(true)?;
    }
    Ok(json!({ "label": label, "results": results }))
}

// ---------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------

/// Runs one action. An unknown id is refused with the ids there are.
pub fn run(id: &str, args: &Value) -> Res {
    match id {
        "IGNITION_HISTORY" => return history(args),
        "IGNITION_UNDO" => return undo(args),
        "IGNITION_REDO" => return redo(args),
        "IGNITION_EDIT_BEGIN" => return edit_begin(args),
        "IGNITION_EDIT_COMMIT" => return edit_end(true),
        "IGNITION_EDIT_ABORT" => return edit_end(false),
        "IGNITION_BATCH" => return batch(args),
        "IGNITION_SET_EACH" => return song::set_each(args),
        "IGNITION_ACTIONS" => {
            return serde_json::from_str(CATALOGUE).map_err(|e| e.to_string());
        }
        _ => {}
    }
    let mut cx = Ctx::new(args);
    let found = library::run(&mut cx, id, args)
        .or_else(|| song::run(&mut cx, id, args))
        .or_else(|| env::run(&mut cx, id, args))
        .or_else(|| tricks::run(&mut cx, id, args));
    let Some(result) = found else {
        return Err(format!("no action {id:?}; IGNITION_ACTIONS lists them"));
    };
    let mut out = result?;
    let wrote = cx.commit(id)?;
    if let Some(map) = out.as_object_mut() {
        if !wrote.is_empty() {
            map.insert("wrote".to_owned(), json!(wrote));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
