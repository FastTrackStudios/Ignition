//! The library: list, get, create, update, duplicate and delete for
//! colours, palettes, splits, effects, looks, focus points, beam presets
//! and trick presets, over three layers (`ignition_show::library`):
//! the profile (read-only: duplicate an entry to edit it), the set's
//! library file, and a song's own overrides.

use std::path::{Path, PathBuf};

use ignition_show::library::{KINDS, Library};
use serde_json::{Map, Value, json};

use super::{Ctx, Res, flag, list, merge, obj, opt_text, slug, text};

/// The show the action is about: `show` (a path, or a name under
/// `data/shows`), else `IGNITION_SHOW`, else the recital.
pub(crate) fn show_path(cx: &Ctx, args: &Value) -> PathBuf {
    let given = opt_text(args, "show")
        .map(str::to_owned)
        .or_else(|| std::env::var("IGNITION_SHOW").ok())
        .unwrap_or_else(|| "data/shows/jt-recital.ig-show".to_owned());
    if given.contains('/') || given.ends_with(".ig-show") {
        cx.path(&given)
    } else {
        cx.data().join("shows").join(format!("{given}.ig-show"))
    }
}

pub(crate) fn dir_of(path: &Path) -> PathBuf {
    path.parent().map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// `base/rel` with `..` folded, so a file has one spelling in the history.
pub(crate) fn join(base: &Path, rel: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for part in Path::new(rel).components() {
        match part {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Where the set's library is: the show's `library` key, else
/// `../library/<show>.json` beside it.
pub(crate) fn library_path(show: &Path, doc: &Value) -> PathBuf {
    let rel = opt_text(doc, "library").map_or_else(
        || {
            let stem = show.file_stem().and_then(|s| s.to_str()).unwrap_or("set");
            format!("../library/{stem}.json")
        },
        str::to_owned,
    );
    join(&dir_of(show), &rel)
}

/// The profile's file: the show's `profile` (a path beside it, or a name
/// under `data/profiles`).
pub(crate) fn profile_path(cx: &Ctx, show: &Path, doc: &Value) -> PathBuf {
    let name = opt_text(doc, "profile").unwrap_or("ignition");
    let beside = join(&dir_of(show), name);
    if beside.is_file() {
        return beside;
    }
    cx.data()
        .join("profiles")
        .join(format!("{}.ig-profile", name.to_lowercase()))
}

/// One song of a show: `song` is a path to its file, or its title,
/// locator or slug. Its file and its entry in the show.
pub(crate) fn song_path(cx: &Ctx, args: &Value) -> Result<(PathBuf, Value), String> {
    let given = text(args, "song")?;
    if given.ends_with(".json") {
        let path = cx.path(given);
        if cx.exists(&path) {
            return Ok((path, json!({ "song": given })));
        }
    }
    let show = show_path(cx, args);
    let doc = cx.json(&show)?;
    let want = slug(given);
    for entry in list(&doc, "songs") {
        let file = opt_text(entry, "file").unwrap_or_default();
        let stem = Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        let names = [opt_text(entry, "song"), opt_text(entry, "locator"), Some(stem)];
        if names.iter().flatten().any(|n| slug(n) == want) {
            return Ok((join(&dir_of(&show), file), entry.clone()));
        }
    }
    let known: Vec<&str> = list(&doc, "songs").iter().filter_map(|s| opt_text(s, "song")).collect();
    Err(format!("{}: no song {given:?}; it has {known:?}", show.display()))
}

/// The profile as a library layer (the same shape as the set's file).
pub(crate) fn profile_layer(cx: &Ctx, path: &Path) -> Value {
    let Ok(p) = cx.json(path) else {
        return json!({});
    };
    let mut out = Map::new();
    let by_name = |key: &str| -> Map<String, Value> {
        list(&p, key)
            .iter()
            .filter_map(|c| {
                let name = opt_text(c, "name")?.to_owned();
                let mut c = c.clone();
                if let Some(m) = c.as_object_mut() {
                    m.remove("name");
                }
                Some((name, c))
            })
            .collect()
    };
    out.insert("colors".into(), Value::Object(by_name("colors")));
    out.insert("splits".into(), Value::Object(by_name("splits")));
    let notes = p.get("effect_notes").cloned().unwrap_or_else(|| json!({}));
    let effects: Map<String, Value> = p
        .get("effects")
        .and_then(Value::as_object)
        .map(|effects| {
            effects
                .iter()
                .map(|(name, recipe)| {
                    let note = notes.get(name);
                    (
                        name.clone(),
                        json!({
                            "recipe": recipe,
                            "family": note.and_then(|n| n.get("family")).cloned().unwrap_or_else(|| json!("")),
                            "about": note.and_then(|n| n.get("about")).cloned().unwrap_or_else(|| json!("")),
                        }),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    out.insert("effects".into(), Value::Object(effects));
    out.insert("looks".into(), p.get("looks").cloned().unwrap_or_else(|| json!({})));
    let tricks: Map<String, Value> = p
        .get("tricks")
        .and_then(Value::as_object)
        .map(|t| t.iter().map(|(k, v)| (k.clone(), json!({ "tricks": v }))).collect())
        .unwrap_or_default();
    out.insert("tricks".into(), Value::Object(tricks));
    Value::Object(out)
}

/// The three layers an action sees, lowest first.
pub(crate) struct Layers {
    pub show: PathBuf,
    pub show_doc: Value,
    pub profile: Value,
    pub set_path: PathBuf,
    pub set: Value,
    pub song: Option<(PathBuf, Value)>,
}

pub(crate) fn layers(cx: &Ctx, args: &Value) -> Result<Layers, String> {
    let show = show_path(cx, args);
    let show_doc = cx.json_or(&show, json!({}))?;
    let set_path = library_path(&show, &show_doc);
    let set = cx.json_or(&set_path, json!({ "version": 1 }))?;
    let song = if opt_text(args, "song").is_some() {
        let (path, _) = song_path(cx, args)?;
        let doc = cx.json(&path)?;
        Some((path, doc))
    } else {
        None
    };
    Ok(Layers {
        profile: profile_layer(cx, &profile_path(cx, &show, &show_doc)),
        show,
        show_doc,
        set_path,
        set,
        song,
    })
}

impl Layers {
    fn song_lib(&self) -> Value {
        self.song
            .as_ref()
            .and_then(|(_, doc)| doc.get("library").cloned())
            .unwrap_or_else(|| json!({}))
    }

    /// `(layer name, layer)`, highest first.
    pub(crate) fn stack(&self) -> Vec<(&'static str, Value)> {
        let mut out = Vec::new();
        if self.song.is_some() {
            out.push(("song", self.song_lib()));
        }
        out.push(("set", self.set.clone()));
        out.push(("profile", self.profile.clone()));
        out
    }

    /// An entry and the layer it comes from, highest first.
    pub(crate) fn find(&self, kind: &str, name: &str) -> Option<(&'static str, Value)> {
        self.stack()
            .into_iter()
            .find_map(|(layer, lib)| lib.get(kind)?.get(name).cloned().map(|e| (layer, e)))
    }

    /// Every name of a kind with its top layer.
    pub(crate) fn names(&self, kind: &str) -> Vec<(String, &'static str, Value)> {
        let mut seen: std::collections::BTreeMap<String, (&'static str, Value)> = std::collections::BTreeMap::new();
        for (layer, lib) in self.stack() {
            if let Some(entries) = lib.get(kind).and_then(Value::as_object) {
                for (name, entry) in entries {
                    seen.entry(name.clone()).or_insert_with(|| (layer, entry.clone()));
                }
            }
        }
        seen.into_iter().map(|(n, (l, e))| (n, l, e)).collect()
    }

    /// The palette in use and its slots, each resolved to RGB.
    pub(crate) fn palette(&self) -> (Option<String>, Vec<Value>) {
        let name = self
            .stack()
            .into_iter()
            .find_map(|(_, lib)| opt_text(&lib, "palette").map(str::to_owned));
        let slots = name
            .as_ref()
            .and_then(|n| self.find("palettes", n))
            .and_then(|(_, p)| p.get("slots").and_then(Value::as_object).cloned())
            .unwrap_or_default();
        let rgb = |slot: &str, value: &Value, stands_in: Option<&str>| {
            let c = value
                .as_str()
                .and_then(|colour| self.find("colors", colour).map(|(_, c)| c))
                .unwrap_or_else(|| value.clone());
            json!({ "slot": slot, "is": value, "stands_in": stands_in, "red": c.get("red"), "green": c.get("green"), "blue": c.get("blue") })
        };
        let mut resolved: Vec<Value> = slots.iter().map(|(slot, value)| rgb(slot, value, None)).collect();
        // A slot another palette has and this one lacks stands in from
        // this one (`ignition_show::library::stand_in`).
        if name.is_some() {
            let every: std::collections::BTreeSet<String> = self
                .names("palettes")
                .iter()
                .filter_map(|(_, _, p)| p.get("slots").and_then(Value::as_object).cloned())
                .flat_map(|m| m.into_iter().map(|(k, _)| k))
                .collect();
            for slot in every {
                if slots.contains_key(&slot) {
                    continue;
                }
                if let Some(from) = ignition_show::library::stand_in(&slot, |s| slots.contains_key(s)) {
                    if let Some(value) = slots.get(&from) {
                        resolved.push(rgb(&slot, value, Some(&from)));
                    }
                }
            }
        }
        (name, resolved)
    }

    /// The layer an edit goes to: `scope` (`set`, the default, or `song`).
    fn target<'a>(&'a mut self, args: &Value) -> Result<&'a mut Value, String> {
        match opt_text(args, "scope").unwrap_or("set") {
            "set" => Ok(&mut self.set),
            "song" => {
                let (_, doc) = self
                    .song
                    .as_mut()
                    .ok_or_else(|| "scope `song` needs `song`".to_owned())?;
                Ok(obj(doc)?.entry("library").or_insert_with(|| json!({})))
            }
            other => Err(format!("scope {other:?}: `set` or `song` (the profile is read-only: duplicate its entry)")),
        }
    }

    /// Checks the edited layers read as a library and stages them.
    pub(crate) fn save(&mut self, cx: &mut Ctx) -> Result<(), String> {
        let check = |v: &Value, what: &str| {
            serde_json::from_value::<Library>(v.clone())
                .map(drop)
                .map_err(|e| format!("{what} would not read as a library: {e}"))
        };
        check(&self.set, "the set's library")?;
        if let Some(map) = self.set.as_object_mut() {
            map.entry("version").or_insert(json!(1));
        }
        cx.write_json(&self.set_path, &self.set)?;
        // The show names its library, so the renderer finds it.
        if opt_text(&self.show_doc, "library").is_none() && self.show_doc.get("songs").is_some() {
            let dir = dir_of(&self.show);
            let rel = self
                .set_path
                .strip_prefix(dir.parent().unwrap_or(&dir))
                .map_or_else(|_| self.set_path.display().to_string(), |p| format!("../{}", p.display()));
            obj(&mut self.show_doc)?.insert("library".into(), json!(rel));
            cx.write_json(&self.show, &self.show_doc)?;
        }
        if let Some((path, doc)) = &self.song {
            check(&doc.get("library").cloned().unwrap_or_else(|| json!({})), "the song's library")?;
            cx.write_json(path, doc)?;
        }
        Ok(())
    }
}

fn kind_of(args: &Value) -> Result<&str, String> {
    let kind = text(args, "kind")?;
    if KINDS.contains(&kind) {
        Ok(kind)
    } else {
        Err(format!("kind {kind:?}: one of {KINDS:?}"))
    }
}

fn summary(kind: &str, entry: &Value) -> Value {
    match kind {
        "colors" => json!([entry.get("red"), entry.get("green"), entry.get("blue")]),
        "palettes" => entry.get("slots").cloned().unwrap_or_default(),
        "splits" => json!({ "colors": entry.get("colors"), "distribute": entry.get("distribute") }),
        "effects" => {
            let r = entry.get("recipe").cloned().unwrap_or_default();
            json!({
                "family": entry.get("family"),
                "target": r.get("target"),
                "steps": list(&r, "steps").len(),
                "beats": r.get("timing").and_then(|t| t.get("measure")),
                "tricks": r.get("tricks"),
            })
        }
        "looks" => json!({ "kind": entry.get("kind"), "parts": list(entry, "recipes").len() }),
        "focus" => entry.get("target").cloned().unwrap_or_default(),
        "beams" => entry.get("values").cloned().unwrap_or_default(),
        _ => {
            let mut e = entry.clone();
            if let Some(m) = e.as_object_mut() {
                for k in ["about", "tags", "folder"] {
                    m.remove(k);
                }
            }
            e
        }
    }
}

fn do_list(cx: &Ctx, args: &Value) -> Res {
    let layers = layers(cx, args)?;
    let kinds: Vec<&str> = match opt_text(args, "kind") {
        Some(_) => vec![kind_of(args)?],
        None => KINDS.to_vec(),
    };
    let tag = opt_text(args, "tag");
    let folder = opt_text(args, "folder");
    let family = opt_text(args, "family");
    let search = opt_text(args, "search").map(str::to_lowercase);
    let only = opt_text(args, "layer");
    let mut out = Map::new();
    for kind in kinds {
        let rows: Vec<Value> = layers
            .names(kind)
            .into_iter()
            .filter(|(name, layer, e)| {
                only.is_none_or(|l| l == *layer)
                    && tag.is_none_or(|t| list(e, "tags").iter().any(|x| x.as_str() == Some(t)))
                    && folder.is_none_or(|f| opt_text(e, "folder").is_some_and(|x| x == f || x.starts_with(&format!("{f}/"))))
                    && family.is_none_or(|f| opt_text(e, "family") == Some(f))
                    && search.as_ref().is_none_or(|s| {
                        name.to_lowercase().contains(s)
                            || opt_text(e, "about").is_some_and(|a| a.to_lowercase().contains(s))
                    })
            })
            .map(|(name, layer, e)| {
                json!({
                    "name": name, "layer": layer,
                    "about": e.get("about"), "tags": e.get("tags"), "folder": e.get("folder"),
                    "is": summary(kind, &e),
                })
            })
            .collect();
        out.insert(kind.to_owned(), Value::Array(rows));
    }
    let (palette, slots) = layers.palette();
    Ok(json!({
        "library": layers.set_path,
        "palette": palette, "slots": slots,
        "entries": out,
    }))
}

fn do_get(cx: &Ctx, args: &Value) -> Res {
    let layers = layers(cx, args)?;
    let (kind, name) = (kind_of(args)?, text(args, "name")?);
    let (layer, entry) = layers
        .find(kind, name)
        .ok_or_else(|| format!("no {kind} entry {name:?}"))?;
    let under: Vec<&str> = layers
        .stack()
        .into_iter()
        .filter(|(l, lib)| *l != layer && lib.get(kind).and_then(|k| k.get(name)).is_some())
        .map(|(l, _)| l)
        .collect();
    Ok(json!({ "kind": kind, "name": name, "layer": layer, "overrides": under, "entry": entry }))
}

fn put(cx: &mut Ctx, args: &Value, name: &str, entry: Value, replace: bool) -> Res {
    let kind = kind_of(args)?.to_owned();
    let mut layers = layers(cx, args)?;
    let shadows = layers.find(&kind, name).map(|(l, _)| l);
    let scope = opt_text(args, "scope").unwrap_or("set").to_owned();
    let target = layers.target(args)?;
    let pool = obj(target)?.entry(kind.clone()).or_insert_with(|| json!({}));
    let pool = obj(pool)?;
    if pool.contains_key(name) && !replace {
        return Err(format!("the {scope}'s library already has {kind} {name:?} (IGNITION_LIBRARY_UPDATE changes it)"));
    }
    pool.insert(name.to_owned(), entry.clone());
    layers.save(cx)?;
    Ok(json!({ "kind": kind, "name": name, "layer": scope, "overrides": shadows.filter(|l| *l != scope), "entry": entry }))
}

fn do_create(cx: &mut Ctx, args: &Value) -> Res {
    let entry = args.get("entry").cloned().ok_or("`entry` is required")?;
    put(cx, args, text(args, "name")?, entry, flag(args, "replace"))
}

fn do_update(cx: &mut Ctx, args: &Value) -> Res {
    let layers = layers(cx, args)?;
    let (kind, name) = (kind_of(args)?, text(args, "name")?);
    let (_, mut entry) = layers
        .find(kind, name)
        .ok_or_else(|| format!("no {kind} entry {name:?}"))?;
    merge(&mut entry, args.get("set").ok_or("`set` is required: the keys to change")?);
    put(cx, args, name, entry, true)
}

fn do_duplicate(cx: &mut Ctx, args: &Value) -> Res {
    let layers = layers(cx, args)?;
    let (kind, name) = (kind_of(args)?, text(args, "name")?);
    let (_, mut entry) = layers
        .find(kind, name)
        .ok_or_else(|| format!("no {kind} entry {name:?}"))?;
    if let Some(set) = args.get("set") {
        merge(&mut entry, set);
    }
    put(cx, args, text(args, "as")?, entry, false)
}

fn do_delete(cx: &mut Ctx, args: &Value) -> Res {
    let (kind, name) = (kind_of(args)?.to_owned(), text(args, "name")?.to_owned());
    let mut layers = layers(cx, args)?;
    let target = layers.target(args)?;
    let gone = target
        .get_mut(&kind)
        .and_then(Value::as_object_mut)
        .and_then(|pool| pool.remove(&name));
    if gone.is_none() {
        return Err(format!("that layer has no {kind} entry {name:?} (the profile's are read-only)"));
    }
    layers.save(cx)?;
    let left = layers.find(&kind, &name).map(|(l, _)| l);
    Ok(json!({ "deleted": name, "kind": kind, "now_from": left }))
}

/// Who uses an entry: the cues of every song of the show that name it.
fn do_uses(cx: &Ctx, args: &Value) -> Res {
    let layers = layers(cx, args)?;
    let name = text(args, "name")?;
    let mut out = Vec::new();
    for entry in list(&layers.show_doc, "songs") {
        let Some(file) = opt_text(entry, "file") else { continue };
        let Ok(doc) = cx.json(&join(&dir_of(&layers.show), file)) else { continue };
        let cues: Vec<Value> = list(&doc, "cues")
            .iter()
            .filter(|cue| names_in(cue).iter().any(|(_, n)| n == name))
            .filter_map(|c| c.get("name").cloned())
            .collect();
        if !cues.is_empty() {
            out.push(json!({ "song": entry.get("song"), "cues": cues }));
        }
    }
    Ok(json!({ "name": name, "used_by": out }))
}

/// Every library name a piece of a show refers to, as `(kind, name)`.
pub(crate) fn names_in(v: &Value) -> Vec<(&'static str, String)> {
    fn walk(v: &Value, out: &mut Vec<(&'static str, String)>) {
        match v {
            Value::Object(map) => {
                for (k, inner) in map {
                    let kind = match k.as_str() {
                        "Color" => Some("colors"),
                        "Split" => Some("splits"),
                        "effect" => Some("effects"),
                        "look" => Some("looks"),
                        "bundle" => Some("bundles"),
                        "FocusPoint" => Some("focus"),
                        "tricks_ref" => Some("tricks"),
                        "Role" => Some("roles"),
                        "Group" if inner.is_string() => Some("groups"),
                        _ => None,
                    };
                    match (kind, inner.as_str()) {
                        (Some(kind), Some(name)) => out.push((kind, name.to_owned())),
                        _ => walk(inner, out),
                    }
                    if k == "colors" || k == "Colors" {
                        for c in inner.as_array().into_iter().flatten().chain(list(inner, "colors")) {
                            if let Some(name) = c.as_str() {
                                out.push(("colors", name.to_owned()));
                            }
                        }
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|i| walk(i, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(v, &mut out);
    out
}

/// Tags and folders in use, so a picker can offer them.
fn do_tags(cx: &Ctx, args: &Value) -> Res {
    let layers = layers(cx, args)?;
    let (mut tags, mut folders) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    for kind in KINDS {
        for (_, _, e) in layers.names(kind) {
            tags.extend(list(&e, "tags").iter().filter_map(|t| t.as_str().map(str::to_owned)));
            folders.extend(opt_text(&e, "folder").map(str::to_owned));
        }
    }
    Ok(json!({ "tags": tags, "folders": folders }))
}

/// Names the palette in use: the set's, or one song's (`song`); an empty
/// `palette` on a song gives it back the set's.
fn do_palette_use(cx: &mut Ctx, args: &Value) -> Res {
    let mut layers = layers(cx, args)?;
    let name = args.get("palette").and_then(Value::as_str).unwrap_or_default().to_owned();
    if !name.is_empty() && layers.find("palettes", &name).is_none() {
        let have: Vec<String> = layers.names("palettes").into_iter().map(|(n, _, _)| n).collect();
        return Err(format!("no palette {name:?}; the library has {have:?}"));
    }
    let scope = if layers.song.is_some() { "song" } else { "set" };
    let target = layers.target(&json!({ "scope": scope }))?;
    let map = obj(target)?;
    if name.is_empty() {
        map.remove("palette");
    } else {
        map.insert("palette".into(), json!(name));
    }
    layers.save(cx)?;
    let (now, slots) = layers.palette();
    Ok(json!({ "scope": scope, "palette": now, "slots": slots }))
}

pub(crate) fn run(cx: &mut Ctx, id: &str, args: &Value) -> Option<Res> {
    Some(match id {
        "IGNITION_LIBRARY_LIST" => do_list(cx, args),
        "IGNITION_LIBRARY_GET" => do_get(cx, args),
        "IGNITION_LIBRARY_CREATE" => do_create(cx, args),
        "IGNITION_LIBRARY_UPDATE" => do_update(cx, args),
        "IGNITION_LIBRARY_DUPLICATE" => do_duplicate(cx, args),
        "IGNITION_LIBRARY_DELETE" => do_delete(cx, args),
        "IGNITION_LIBRARY_USES" => do_uses(cx, args),
        "IGNITION_LIBRARY_TAGS" => do_tags(cx, args),
        "IGNITION_PALETTE_USE" => do_palette_use(cx, args),
        _ => return None,
    })
}
