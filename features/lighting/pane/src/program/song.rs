//! Programming a song, and the same over a set.
//!
//! A song is its file (`data/songs/<set>/<song>.json`): `cues`,
//! `triggers`, and what this module adds beside them: `sections` (the
//! song's section map: name, first bar, length in bars), `markers`
//! (named points: drum hits, a stop, a pickup) and `library` (the song's
//! overrides and its palette in use). Every position an action takes
//! (`at`) is a bar (`12`), a bar and beat (`"12.3"`), a section
//! (`"Chorus 1"`, `"Chorus 1+2"` two bars in, `"Chorus 1#2"` its second
//! occurrence), a marker (`{"marker": "stop"}`) or the file's own form.

use std::collections::{BTreeMap, BTreeSet};

use ignition_show::library::TrickPreset;
use serde_json::{Map, Value, json};

use super::library::{Layers, dir_of, join, layers, names_in, profile_path, song_path};
use super::{Ctx, Res, arr, count_f, f32_of, flag, list, merge, num, obj, opt_text, slug, text, whole};

/// One section of a song.
#[derive(Debug, Clone)]
pub(crate) struct Sect {
    pub name: String,
    pub ordinal: usize,
    pub bar: u32,
    pub bars: f64,
}

/// The song's section map: its `sections`, or, for a file that has none,
/// what its cues say (each cue at the start of a section names it and
/// carries the bar it resolved to).
pub(crate) fn sections(doc: &Value) -> Vec<Sect> {
    let mut out: Vec<Sect> = Vec::new();
    let push = |name: &str, bar: u32, bars: f64, out: &mut Vec<Sect>| {
        let ordinal = out.iter().filter(|s| s.name == name).count();
        out.push(Sect { name: name.to_owned(), ordinal, bar, bars });
    };
    if let Some(given) = doc.get("sections").and_then(Value::as_array) {
        let mut next = 1;
        for s in given {
            let name = opt_text(s, "name").unwrap_or("?");
            let bars = num(s, "bars").unwrap_or(8.0);
            let bar = s.get("bar").and_then(whole).unwrap_or(next);
            #[allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let len = bars.ceil().max(1.0) as u32;
            next = bar + len;
            push(name, bar, bars, &mut out);
        }
        return out;
    }
    let mut starts: Vec<(String, u32)> = Vec::new();
    for cue in list(doc, "cues") {
        let at = cue.get("at").cloned().unwrap_or_default();
        let (Some(name), Some(bar)) = (
            opt_text(&at, "section"),
            cue.get("resolved").and_then(|r| r.get("bar")).and_then(whole),
        ) else {
            continue;
        };
        let into = at.get("bars").and_then(whole).unwrap_or(0);
        if at.get("last_bar").is_none() && !starts.iter().any(|(n, _)| n == name) {
            starts.push((name.to_owned(), bar.saturating_sub(into)));
        }
    }
    starts.sort_by_key(|(_, bar)| *bar);
    for (i, (name, bar)) in starts.iter().enumerate() {
        let bars = starts.get(i + 1).map_or(8.0, |(_, next)| f64::from(next.saturating_sub(*bar)));
        push(name, *bar, bars, &mut out);
    }
    out
}

fn find_section<'a>(sects: &'a [Sect], name: &str, ordinal: usize) -> Option<&'a Sect> {
    sects.iter().find(|s| s.name == name && s.ordinal == ordinal).or_else(|| {
        let want = name.to_lowercase();
        sects.iter().filter(|s| s.name.to_lowercase() == want).nth(ordinal)
    })
}

/// A position as the file stores it and the bar it resolves to.
pub(crate) fn place(doc: &Value, at: &Value) -> Result<(Value, Value), String> {
    let sects = sections(doc);
    let bar_beat = |bar: u32, beat: f64| json!({ "bar": bar, "beat": beat });
    let in_section = |name: &str, ordinal: usize, bars: u32, beat: f64| -> Result<(Value, Value), String> {
        let s = find_section(&sects, name, ordinal).ok_or_else(|| {
            let have: Vec<&str> = sects.iter().map(|s| s.name.as_str()).collect();
            format!("no section {name:?} (occurrence {}); the song has {have:?}", ordinal + 1)
        })?;
        Ok((
            json!({ "section": s.name, "ordinal": ordinal, "bars": bars, "beat": beat }),
            bar_beat(s.bar + bars, beat),
        ))
    };
    match at {
        Value::Number(_) => {
            let bar = whole(at).ok_or("a bar is a whole number, from 1")?;
            Ok((bar_beat(bar, 1.0), bar_beat(bar, 1.0)))
        }
        Value::String(s) => {
            let s = s.trim();
            let mut parts = s.splitn(2, '.');
            if let (Some(Ok(bar)), beat) = (parts.next().map(str::parse::<u32>), parts.next()) {
                let beat = beat.map_or(Ok(1.0), str::parse::<f64>).map_err(|_| format!("{s:?}: bar.beat"))?;
                return Ok((bar_beat(bar, beat), bar_beat(bar, beat)));
            }
            if let Some(m) = list(doc, "markers").iter().find(|m| opt_text(m, "name") == Some(s)) {
                return place(doc, &json!({ "marker": m.get("name") }));
            }
            let (rest, bars) = match s.rsplit_once('+') {
                Some((rest, n)) if n.trim().parse::<u32>().is_ok() => (rest.trim(), n.trim().parse().unwrap_or(0)),
                _ => (s, 0),
            };
            let (name, ordinal) = match rest.rsplit_once('#') {
                Some((name, n)) if n.trim().parse::<usize>().is_ok() => {
                    (name.trim(), n.trim().parse::<usize>().unwrap_or(1).saturating_sub(1))
                }
                _ => (rest, 0),
            };
            in_section(name, ordinal, bars, 1.0)
        }
        Value::Object(map) => {
            if let Some(name) = map.get("marker").and_then(Value::as_str) {
                let m = list(doc, "markers")
                    .iter()
                    .find(|m| opt_text(m, "name") == Some(name))
                    .ok_or_else(|| format!("no marker {name:?}"))?;
                let bar = m.get("bar").and_then(whole).ok_or("the marker has no bar")?;
                let beat = num(m, "beat").unwrap_or(1.0);
                return Ok((bar_beat(bar, beat), bar_beat(bar, beat)));
            }
            if let Some(name) = map.get("section").and_then(Value::as_str) {
                let ordinal = map.get("ordinal").and_then(whole).map_or(0, |o| usize::try_from(o).unwrap_or(0));
                if map.get("last_bar").and_then(Value::as_bool) == Some(true) {
                    let s = find_section(&sects, name, ordinal).ok_or_else(|| format!("no section {name:?}"))?;
                    #[allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    let last = s.bar + (s.bars.ceil().max(1.0) as u32) - 1;
                    return Ok((at.clone(), bar_beat(last, 1.0)));
                }
                let bars = map.get("bars").and_then(whole).unwrap_or(0);
                return in_section(name, ordinal, bars, num(at, "beat").unwrap_or(1.0));
            }
            let bar = map.get("bar").and_then(whole).ok_or("`at`: a bar, \"bar.beat\", a section, or {marker}")?;
            let beat = num(at, "beat").unwrap_or(1.0);
            Ok((bar_beat(bar, beat), bar_beat(bar, beat)))
        }
        _ => Err("`at`: a bar, \"bar.beat\", a section name, or {marker}".to_owned()),
    }
}

fn bar_key(v: &Value) -> (u32, f64) {
    let r = v.get("resolved").or_else(|| v.get("at"));
    (
        r.and_then(|r| r.get("bar")).and_then(whole).unwrap_or(u32::MAX),
        r.and_then(|r| num(r, "beat")).unwrap_or(1.0),
    )
}

/// Timed cues in song order; the operator's own (no position) keep their
/// place at the end.
fn sort_cues(doc: &mut Value, key: &str) -> Result<(), String> {
    arr(doc, key)?.sort_by(|a, b| {
        let (a, b) = (bar_key(a), bar_key(b));
        a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
    });
    Ok(())
}

/// A cue by name, by number (`3`, `"3"`: its number, else its place) or
/// `{index}` from 0.
fn cue_index(doc: &Value, cue: &Value) -> Result<usize, String> {
    let cues = list(doc, "cues");
    if let Some(i) = cue.get("index").and_then(whole) {
        return usize::try_from(i).ok().filter(|i| *i < cues.len()).ok_or_else(|| format!("no cue at index {i}"));
    }
    if let Some(name) = cue.as_str() {
        if let Some(i) = cues.iter().position(|c| opt_text(c, "name") == Some(name)) {
            return Ok(i);
        }
    }
    let n = cue.as_f64().or_else(|| cue.as_str().and_then(|s| s.parse().ok()));
    if let Some(n) = n {
        if let Some(i) = cues.iter().position(|c| num(c, "number").is_some_and(|x| (x - n).abs() < 1e-6)) {
            return Ok(i);
        }
        #[allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let i = (n - 1.0).max(0.0) as usize;
        if n.fract() == 0.0 && i < cues.len() {
            return Ok(i);
        }
    }
    let names: Vec<&str> = cues.iter().filter_map(|c| opt_text(c, "name")).collect();
    Err(format!("no cue {cue}; the song has {names:?}"))
}

/// The cue that opens a section: the first at its first bar, by its
/// `at`, else by the bar it resolved to.
fn section_cue(doc: &Value, s: &Sect) -> Option<usize> {
    let cues = list(doc, "cues");
    cues.iter()
        .position(|c| {
            let at = c.get("at").cloned().unwrap_or_default();
            opt_text(&at, "section") == Some(s.name.as_str())
                && at.get("ordinal").and_then(whole).map_or(0, |o| usize::try_from(o).unwrap_or(0)) == s.ordinal
                && at.get("bars").and_then(whole).unwrap_or(0) == 0
                && at.get("last_bar").is_none()
                && num(&at, "beat").unwrap_or(1.0) <= 1.0
        })
        .or_else(|| cues.iter().position(|c| bar_key(c) == (s.bar, 1.0)))
}

/// A target as a selection: a selection as JSON, or a name (a role of
/// the profile when it has one by that name, else a group of the venue).
pub(crate) fn selection(target: &Value, roles: &BTreeSet<String>) -> Result<Value, String> {
    match target {
        Value::String(name) if roles.contains(name) => Ok(json!({ "Role": name })),
        Value::String(name) => Ok(json!({ "Group": name })),
        Value::Array(chans) => Ok(json!({ "Chans": chans })),
        Value::Object(_) => Ok(target.clone()),
        _ => Err("`target`: a role or group name, a list of channels, or a selection".to_owned()),
    }
}

pub(crate) fn roles_of(cx: &Ctx, l: &Layers) -> BTreeSet<String> {
    cx.json(&profile_path(cx, &l.show, &l.show_doc))
        .map(|p| list(&p, "roles").iter().filter_map(|r| opt_text(r, "name").map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// A trick preset by name, or written in place.
pub(crate) fn preset_of(l: &Layers, tricks: &Value) -> Result<TrickPreset, String> {
    let value = match tricks {
        Value::String(name) => l.find("tricks", name).map(|(_, e)| e).ok_or_else(|| format!("no trick preset {name:?}"))?,
        Value::Array(chain) => json!({ "tricks": chain }),
        other => other.clone(),
    };
    serde_json::from_value(value).map_err(|e| format!("trick preset: {e}"))
}

/// Puts one part on a cue: a static part (`dimmer`, `color`, `split`,
/// `focus`, `beam`, `raw`) merged into the cue's part for that target,
/// an `effect` (with `bars`, `depth`, `tricks`), or a `look`. What it did.
pub(crate) fn put_part(cue: &mut Value, spec: &Value, l: &Layers, roles: &BTreeSet<String>) -> Res {
    if let Some(look) = opt_text(spec, "look") {
        if l.find("looks", look).is_none() {
            return Err(format!("no look {look:?}"));
        }
        let parts = arr(cue, "recipes")?;
        parts.retain(|p| p.get("look").is_none() || flag(spec, "keep_looks"));
        parts.insert(0, json!({ "look": look }));
        return Ok(json!({ "look": look }));
    }
    let target = selection(spec.get("target").ok_or("a part needs `target`")?, roles)?;
    if flag(spec, "clear") {
        arr(cue, "recipes")?.retain(|p| p.get("target") != Some(&target));
    }
    let mut did = Map::new();
    let mut applies: Vec<(String, Value)> = Vec::new();
    if let Some(d) = num(spec, "dimmer") {
        applies.push(("Dimmer".into(), json!(d)));
    }
    if let Some(c) = spec.get("color") {
        if let Some(name) = c.as_str() {
            let slot = l.palette().1.iter().any(|s| opt_text(s, "slot") == Some(name));
            if !slot && l.find("colors", name).is_none() {
                return Err(format!("no colour or palette slot {name:?}"));
            }
        }
        applies.push(("Color".into(), c.clone()));
    }
    if let Some(s) = opt_text(spec, "split") {
        if l.find("splits", s).is_none() {
            return Err(format!("no split {s:?}"));
        }
        applies.push(("Split".into(), json!(s)));
    }
    if let Some(f) = spec.get("focus") {
        applies.push(("FocusPoint".into(), f.clone()));
    }
    if let Some(b) = opt_text(spec, "beam") {
        let (_, beam) = l.find("beams", b).ok_or_else(|| format!("no beam preset {b:?}"))?;
        applies.push(("Raw".into(), beam.get("values").cloned().unwrap_or_default()));
    }
    if let Some(raw) = spec.get("raw") {
        applies.push(("Raw".into(), raw.clone()));
    }
    if !applies.is_empty() {
        let parts = arr(cue, "recipes")?;
        let at = parts.iter().position(|p| {
            p.get("target") == Some(&target) && p.get("effect").is_none() && list(p, "steps").len() == 1
        });
        let at = at.unwrap_or_else(|| {
            let first_effect = parts.iter().position(|p| p.get("effect").is_some() || list(p, "steps").len() > 1);
            let i = first_effect.unwrap_or(parts.len());
            parts.insert(i, json!({ "target": target, "steps": [{ "apply": [], "width": 1.0, "transition": 0.0, "ease": "Linear" }] }));
            i
        });
        let apply = parts
            .get_mut(at)
            .and_then(|p| p.get_mut("steps"))
            .and_then(|s| s.get_mut(0))
            .ok_or("the part has no step")?;
        let apply = arr(apply, "apply")?;
        for (key, value) in applies {
            apply.retain(|a| a.get(&key).is_none());
            apply.push(json!({ key.clone(): value }));
            did.insert(key.to_lowercase(), json!(true));
        }
    }
    if let Some(effect) = opt_text(spec, "effect") {
        let (_, entry) = l.find("effects", effect).ok_or_else(|| format!("no effect {effect:?}"))?;
        let label = opt_text(spec, "name").map_or_else(|| effect.to_owned(), str::to_owned);
        let mut params = Map::new();
        if let Some(d) = num(spec, "depth") {
            params.insert("depth".into(), json!(d));
        }
        let part = match spec.get("tricks") {
            // A preset that spreads phase or speed is more than a chain: the
            // effect is written in place with the preset applied.
            Some(tricks) => {
                let preset = preset_of(l, tricks)?;
                let mut recipe: ignition_show::Recipe =
                    serde_json::from_value(entry.get("recipe").cloned().unwrap_or_default())
                        .map_err(|e| format!("effect {effect:?}: {e}"))?;
                recipe.target = serde_json::from_value(target.clone()).map_err(|e| format!("target: {e}"))?;
                if let Some(bars) = num(spec, "bars") {
                    recipe.timing.measure = f32_of(bars * 4.0);
                }
                if let Some(d) = num(spec, "depth") {
                    recipe.depth = f32_of(d);
                }
                preset.apply(&mut recipe);
                recipe.name = Some(label.clone());
                recipe.note = Some(format!(
                    "effect {effect:?} with tricks {}",
                    tricks.as_str().map_or_else(|| "written in place".to_owned(), |n| format!("{n:?}"))
                ));
                serde_json::to_value(&recipe).map_err(|e| e.to_string())?
            }
            None => {
                let mut part = json!({ "effect": effect, "target": target });
                if let Some(m) = part.as_object_mut() {
                    if let Some(bars) = num(spec, "bars") {
                        m.insert("bars".into(), json!(bars));
                    }
                    if !params.is_empty() {
                        m.insert("params".into(), Value::Object(params));
                    }
                    if let Some(name) = opt_text(spec, "name") {
                        m.insert("name".into(), json!(name));
                    }
                }
                part
            }
        };
        let parts = arr(cue, "recipes")?;
        parts.retain(|p| {
            !(p.get("target") == Some(&target)
                && (opt_text(p, "effect") == Some(effect) || opt_text(p, "name") == Some(label.as_str())))
        });
        parts.push(part);
        did.insert("effect".into(), json!(effect));
    }
    if did.is_empty() {
        return Err("a part sets something: dimmer, color, split, focus, beam, raw, effect or look".to_owned());
    }
    did.insert("target".into(), target);
    Ok(Value::Object(did))
}

fn part_specs(v: &Value) -> Vec<Value> {
    match v {
        Value::Array(items) => items.clone(),
        Value::String(look) => vec![json!({ "look": look })],
        Value::Null => Vec::new(),
        other => vec![other.clone()],
    }
}

fn new_cue(name: &str, at: Value, resolved: Value) -> Value {
    json!({ "name": name, "fade_secs": 0.75, "values": [], "recipes": [], "block": false, "at": at, "resolved": resolved })
}

fn part_summary(p: &Value) -> Value {
    if let Some(look) = p.get("look") {
        return json!({ "look": look });
    }
    if let Some(bundle) = p.get("bundle") {
        return json!({ "bundle": bundle, "target": p.get("target") });
    }
    if let Some(effect) = p.get("effect") {
        return json!({ "effect": effect, "target": p.get("target"), "bars": p.get("bars"), "params": p.get("params") });
    }
    let steps = list(p, "steps");
    if steps.len() > 1 {
        return json!({ "effect": p.get("name"), "inline": true, "target": p.get("target"), "steps": steps.len(), "tricks": p.get("tricks") });
    }
    json!({ "target": p.get("target"), "apply": steps.first().and_then(|s| s.get("apply")) })
}

fn cue_summary(i: usize, c: &Value) -> Value {
    json!({
        "index": i,
        "number": c.get("number").cloned().unwrap_or_else(|| json!(i + 1)),
        "name": c.get("name"),
        "at": c.get("at"),
        "bar": c.get("resolved").and_then(|r| r.get("bar")),
        "beat": c.get("resolved").and_then(|r| r.get("beat")),
        "fade_secs": c.get("fade_secs"),
        "block": c.get("block"),
        "note": c.get("note"),
        "parts": list(c, "recipes").iter().map(part_summary).collect::<Vec<_>>(),
    })
}

fn sections_json(doc: &Value) -> Vec<Value> {
    sections(doc)
        .iter()
        .map(|s| json!({ "name": s.name, "ordinal": s.ordinal, "bar": s.bar, "bars": s.bars, "cue": section_cue(doc, s) }))
        .collect()
}

fn song_list(cx: &Ctx, args: &Value) -> Res {
    let l = layers(cx, args)?;
    let dir = dir_of(&l.show);
    let songs: Vec<Value> = list(&l.show_doc, "songs")
        .iter()
        .map(|e| {
            let file = opt_text(e, "file").unwrap_or_default();
            let doc = cx.json(&join(&dir, file)).unwrap_or_default();
            json!({
                "song": e.get("song"), "slug": opt_text(e, "song").map(slug), "set": e.get("set"),
                "lit": e.get("lit"), "locator": e.get("locator"), "file": join(&dir, file),
                "cues": list(&doc, "cues").len(), "triggers": list(&doc, "triggers").len(),
                "sections": sections(&doc).len(), "has_section_map": doc.get("sections").is_some(),
                "markers": list(&doc, "markers").len(),
                "palette": doc.get("library").and_then(|l| l.get("palette")),
            })
        })
        .collect();
    Ok(json!({
        "show": l.show, "name": l.show_doc.get("name"), "venue": l.show_doc.get("venue"),
        "library": l.set_path, "palette": l.set.get("palette"), "sets": l.show_doc.get("sets"),
        "songs": songs,
    }))
}

fn song_get(cx: &Ctx, args: &Value) -> Res {
    let l = layers(cx, args)?;
    let (path, doc) = l.song.clone().ok_or("`song` is required")?;
    let (palette, slots) = l.palette();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for m in list(&doc, "markers") {
        *kinds.entry(opt_text(m, "kind").unwrap_or("").to_owned()).or_default() += 1;
    }
    Ok(json!({
        "file": path, "name": doc.get("name"),
        "sections": sections_json(&doc), "has_section_map": doc.get("sections").is_some(),
        "markers": if flag(args, "markers") { json!(list(&doc, "markers")) } else { json!(kinds) },
        "cues": list(&doc, "cues").iter().enumerate().map(|(i, c)| cue_summary(i, c)).collect::<Vec<_>>(),
        "triggers": list(&doc, "triggers").len(),
        "palette": palette, "slots": slots,
        "overrides": doc.get("library").and_then(Value::as_object).map(|m| {
            m.iter().filter(|(_, v)| v.is_object()).map(|(k, v)| (k.clone(), json!(v.as_object().map(|o| o.keys().cloned().collect::<Vec<_>>())))).collect::<Map<_, _>>()
        }),
    }))
}

fn song_create(cx: &mut Ctx, args: &Value) -> Res {
    let title = text(args, "title")?;
    let mut l = layers(cx, args)?;
    let dir = dir_of(&l.show);
    let stem = l.show.file_stem().and_then(|s| s.to_str()).unwrap_or("set").to_owned();
    let rel = opt_text(args, "file").map_or_else(|| format!("../songs/{stem}/{}.json", slug(title)), str::to_owned);
    let path = join(&dir, &rel);
    if cx.exists(&path) {
        return Err(format!("{} is already there", path.display()));
    }
    if !l.show_doc.is_object() || l.show_doc.get("songs").is_none() {
        merge(&mut l.show_doc, &json!({ "version": 1, "name": stem, "profile": "../profiles/ignition.ig-profile", "venue": opt_text(args, "env").map_or_else(|| "../venues/room138-cbu".to_owned(), |e| format!("../venues/{e}")), "songs": [] }));
    }
    arr(&mut l.show_doc, "songs")?.push(json!({
        "set": opt_text(args, "set").unwrap_or(""), "song": title,
        "locator": opt_text(args, "locator").unwrap_or(title), "lit": true, "file": rel,
    }));
    let mut doc = json!({ "version": 1, "profile": "Ignition", "name": title, "cues": [] });
    if let Some(sections) = args.get("sections") {
        obj(&mut doc)?.insert("sections".into(), sections.clone());
    }
    cx.write_json(&path, &doc)?;
    cx.write_json(&l.show, &l.show_doc)?;
    Ok(json!({ "song": title, "file": path, "sections": sections_json(&doc) }))
}

/// Loads the song an action edits, runs `edit`, re-sorts and stages it.
fn edit_song(cx: &mut Ctx, args: &Value, edit: impl FnOnce(&mut Value, &Layers, &BTreeSet<String>) -> Res) -> Res {
    let l = layers(cx, args)?;
    let (path, mut doc) = l.song.clone().ok_or("`song` is required")?;
    let roles = roles_of(cx, &l);
    let out = edit(&mut doc, &l, &roles)?;
    sort_cues(&mut doc, "cues")?;
    if doc.get("triggers").is_some() {
        sort_cues(&mut doc, "triggers")?;
    }
    cx.write_json(&path, &doc)?;
    Ok(out)
}

fn cue_get(cx: &Ctx, args: &Value) -> Res {
    let (_, doc) = layers(cx, args)?.song.ok_or("`song` is required")?;
    let i = cue_index(&doc, args.get("cue").ok_or("`cue` is required")?)?;
    Ok(json!({ "index": i, "cue": list(&doc, "cues").get(i) }))
}

fn cue_add(cx: &mut Ctx, args: &Value) -> Res {
    edit_song(cx, args, |doc, l, roles| {
        let name = text(args, "name")?;
        let (at, resolved) = place(doc, args.get("at").ok_or("`at` is required")?)?;
        let mut cue = new_cue(name, at, resolved);
        if let Some(set) = args.get("set") {
            merge(&mut cue, set);
        }
        for key in ["fade_secs", "block", "note"] {
            if let Some(v) = args.get(key) {
                obj(&mut cue)?.insert(key.into(), v.clone());
            }
        }
        let mut parts = Vec::new();
        for spec in part_specs(args.get("parts").unwrap_or(&Value::Null)) {
            parts.push(put_part(&mut cue, &spec, l, roles)?);
        }
        let summary = cue_summary(0, &cue);
        arr(doc, "cues")?.push(cue);
        Ok(json!({ "added": name, "cue": summary, "parts": parts }))
    })
}

fn cue_update(cx: &mut Ctx, args: &Value) -> Res {
    edit_song(cx, args, |doc, l, roles| {
        let i = cue_index(doc, args.get("cue").ok_or("`cue` is required")?)?;
        let moved = match args.get("at") {
            Some(at) => Some(place(doc, at)?),
            None => None,
        };
        let cue = arr(doc, "cues")?.get_mut(i).ok_or("no such cue")?;
        if let Some(set) = args.get("set") {
            merge(cue, set);
        }
        if let Some((at, resolved)) = moved {
            let map = obj(cue)?;
            map.insert("at".into(), at);
            map.insert("resolved".into(), resolved);
        }
        let mut parts = Vec::new();
        for spec in part_specs(args.get("parts").unwrap_or(&Value::Null)) {
            parts.push(put_part(cue, &spec, l, roles)?);
        }
        if let Some(remove) = args.get("remove") {
            let target = match remove.get("target") {
                Some(t) => Some(selection(t, roles)?),
                None => None,
            };
            let effect = opt_text(remove, "effect");
            let look = opt_text(remove, "look");
            arr(cue, "recipes")?.retain(|p| {
                let hit = target.as_ref().is_none_or(|t| p.get("target") == Some(t))
                    && effect.is_none_or(|e| opt_text(p, "effect") == Some(e) || opt_text(p, "name") == Some(e))
                    && look.is_none_or(|k| opt_text(p, "look") == Some(k))
                    && (target.is_some() || effect.is_some() || look.is_some());
                !hit
            });
        }
        Ok(json!({ "cue": cue_summary(i, cue), "parts": parts }))
    })
}

fn cue_delete(cx: &mut Ctx, args: &Value) -> Res {
    edit_song(cx, args, |doc, _, _| {
        let i = cue_index(doc, args.get("cue").ok_or("`cue` is required")?)?;
        let gone = arr(doc, "cues")?.remove(i);
        Ok(json!({ "deleted": gone.get("name"), "cue": gone }))
    })
}

fn cue_duplicate(cx: &mut Ctx, args: &Value) -> Res {
    edit_song(cx, args, |doc, _, _| {
        let i = cue_index(doc, args.get("cue").ok_or("`cue` is required")?)?;
        let (at, resolved) = place(doc, args.get("at").ok_or("`at` is required")?)?;
        let mut cue = list(doc, "cues").get(i).cloned().ok_or("no such cue")?;
        let map = obj(&mut cue)?;
        map.insert("at".into(), at);
        map.insert("resolved".into(), resolved);
        map.remove("number");
        if let Some(name) = opt_text(args, "name") {
            map.insert("name".into(), json!(name));
        }
        let summary = cue_summary(0, &cue);
        arr(doc, "cues")?.push(cue);
        Ok(json!({ "cue": summary }))
    })
}

/// The sections an action names: `section` a name, `"*"`, a kind
/// (`"kind:chorus"`) or a list of those.
fn pick_sections(doc: &Value, which: &Value) -> Result<Vec<Sect>, String> {
    let all = sections(doc);
    let mut out: Vec<Sect> = Vec::new();
    let wanted: Vec<Value> = which.as_array().cloned().unwrap_or_else(|| vec![which.clone()]);
    for w in &wanted {
        let Some(w) = w.as_str() else {
            return Err("`section`: a name, \"*\", \"kind:chorus\", or a list".to_owned());
        };
        let hits: Vec<Sect> = if w == "*" {
            all.clone()
        } else if let Some(kind) = w.strip_prefix("kind:") {
            all.iter().filter(|s| kind_of(&s.name) == kind).cloned().collect()
        } else {
            let (name, ordinal) = match w.rsplit_once('#') {
                Some((n, o)) if o.parse::<usize>().is_ok() => (n, Some(o.parse::<usize>().unwrap_or(1).saturating_sub(1))),
                _ => (w, None),
            };
            all.iter()
                .filter(|s| s.name.eq_ignore_ascii_case(name) && ordinal.is_none_or(|o| s.ordinal == o))
                .cloned()
                .collect()
        };
        if hits.is_empty() {
            let have: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
            return Err(format!("no section {w:?}; the song has {have:?}"));
        }
        for h in hits {
            if !out.iter().any(|s| s.name == h.name && s.ordinal == h.ordinal) {
                out.push(h);
            }
        }
    }
    Ok(out)
}

/// What a section is, from its name: intro, verse, pre, chorus, bridge,
/// solo, break, outro or other.
#[must_use]
pub fn kind_of(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["pre"]) {
        "pre"
    } else if has(&["chorus", "ch ", "hook", "refrain"]) || n == "ch" {
        "chorus"
    } else if has(&["verse", "vs"]) {
        "verse"
    } else if has(&["intro", "count"]) {
        "intro"
    } else if has(&["bridge", "middle"]) {
        "bridge"
    } else if has(&["solo", "inst", "interlude", "riff"]) {
        "solo"
    } else if has(&["break", "drop", "down"]) {
        "break"
    } else if has(&["outro", "end", "tag", "coda"]) {
        "outro"
    } else {
        "other"
    }
}

/// The cue opening a section, made when there is none. Its index.
fn ensure_section_cue(doc: &mut Value, s: &Sect) -> Result<(usize, bool), String> {
    if let Some(i) = section_cue(doc, s) {
        return Ok((i, false));
    }
    let name = if s.ordinal == 0 { s.name.clone() } else { format!("{} ({})", s.name, s.ordinal + 1) };
    let cue = new_cue(
        &name,
        json!({ "section": s.name, "ordinal": s.ordinal, "bars": 0, "beat": 1.0 }),
        json!({ "bar": s.bar, "beat": 1.0 }),
    );
    let cues = arr(doc, "cues")?;
    cues.push(cue);
    Ok((cues.len() - 1, true))
}

fn section_apply(cx: &mut Ctx, args: &Value) -> Res {
    edit_song(cx, args, |doc, l, roles| {
        let picked = pick_sections(doc, args.get("section").ok_or("`section` is required")?)?;
        let specs = part_specs(args.get("parts").unwrap_or(args));
        let mut out = Vec::new();
        for s in &picked {
            let (i, made) = ensure_section_cue(doc, s)?;
            let cue = arr(doc, "cues")?.get_mut(i).ok_or("no such cue")?;
            let mut did = Vec::new();
            for spec in &specs {
                did.push(put_part(cue, spec, l, roles)?);
            }
            out.push(json!({ "section": s.name, "ordinal": s.ordinal, "bar": s.bar, "cue": cue.get("name"), "created": made, "parts": did }));
        }
        Ok(json!({ "applied": out }))
    })
}

const LOOK_KEYS: [&str; 8] = ["recipes", "values", "fade_secs", "block", "timing", "fan", "mib", "morph"];

fn section_copy(cx: &mut Ctx, args: &Value) -> Res {
    let l = layers(cx, args)?;
    let (from_path, from_doc) = l.song.clone().ok_or("`song` is required")?;
    let from = pick_sections(&from_doc, args.get("from").ok_or("`from` is required")?)?;
    let from = from.first().ok_or("no such section")?;
    let source = section_cue(&from_doc, from)
        .and_then(|i| list(&from_doc, "cues").get(i).cloned())
        .ok_or_else(|| format!("section {:?} has no cue to copy", from.name))?;
    let (to_path, mut to_doc) = match opt_text(args, "to_song") {
        Some(song) => {
            let mut a = args.clone();
            obj(&mut a)?.insert("song".into(), json!(song));
            let (p, _) = song_path(cx, &a)?;
            let d = cx.json(&p)?;
            (p, d)
        }
        None => (from_path, from_doc.clone()),
    };
    let targets = pick_sections(&to_doc, args.get("to").ok_or("`to` is required")?)?;
    let mut out = Vec::new();
    for s in &targets {
        let (i, made) = ensure_section_cue(&mut to_doc, s)?;
        let cue = arr(&mut to_doc, "cues")?.get_mut(i).ok_or("no such cue")?;
        let map = obj(cue)?;
        for key in LOOK_KEYS {
            match source.get(key) {
                Some(v) => map.insert(key.to_owned(), v.clone()),
                None => map.remove(key),
            };
        }
        out.push(json!({ "section": s.name, "ordinal": s.ordinal, "cue": map.get("name"), "created": made }));
    }
    sort_cues(&mut to_doc, "cues")?;
    cx.write_json(&to_path, &to_doc)?;
    Ok(json!({ "from": source.get("name"), "parts": list(&source, "recipes").len(), "to": out }))
}

/// A look per section from the section map: `looks` maps a section's
/// kind (`verse`, `chorus`… or `*`) or its name to the parts it gets.
/// `dry_run` answers the plan and writes nothing.
fn fill_sections(cx: &mut Ctx, args: &Value) -> Res {
    let dry = flag(args, "dry_run");
    let l = layers(cx, args)?;
    let (path, mut doc) = l.song.clone().ok_or("`song` is required")?;
    let roles = roles_of(cx, &l);
    let looks = args.get("looks").and_then(Value::as_object).ok_or("`looks` is required: {kind or section name: parts}")?;
    let mut plan = Vec::new();
    for s in sections(&doc) {
        let kind = kind_of(&s.name);
        let Some(parts) = looks.get(&s.name).or_else(|| looks.get(kind)).or_else(|| looks.get("*")) else {
            plan.push(json!({ "section": s.name, "kind": kind, "bar": s.bar, "skipped": "no look for it" }));
            continue;
        };
        let (i, made) = ensure_section_cue(&mut doc, &s)?;
        let cue = arr(&mut doc, "cues")?.get_mut(i).ok_or("no such cue")?;
        if flag(args, "replace") {
            obj(cue)?.insert("recipes".into(), json!([]));
        }
        if let Some(block) = args.get("block") {
            obj(cue)?.insert("block".into(), block.clone());
        }
        let mut did = Vec::new();
        for spec in part_specs(parts) {
            did.push(put_part(cue, &spec, &l, &roles)?);
        }
        plan.push(json!({
            "section": s.name, "kind": kind, "bar": s.bar, "bars": s.bars,
            "cue": cue.get("name"), "created": made, "parts": did,
        }));
    }
    if !dry {
        sort_cues(&mut doc, "cues")?;
        cx.write_json(&path, &doc)?;
    }
    Ok(json!({ "dry_run": dry, "plan": plan }))
}

fn bump_kind(name: &str) -> Result<ignition_show::BumpKind, String> {
    serde_json::from_value(json!(name)).map_err(|_| format!("bump {name:?}: Level, White, ColorBoost or Burst"))
}

/// Accents on hits: a trigger (a one-shot bump) at each marker of a
/// kind, or at each hit of a hits file. `dry_run` answers the plan.
fn fill_hits(cx: &mut Ctx, args: &Value) -> Res {
    let dry = flag(args, "dry_run");
    let l = layers(cx, args)?;
    let (path, mut doc) = l.song.clone().ok_or("`song` is required")?;
    let roles = roles_of(cx, &l);
    let target = selection(args.get("target").ok_or("`target` is required")?, &roles)?;
    let typed: ignition_show::Selection = serde_json::from_value(target.clone()).map_err(|e| format!("target: {e}"))?;
    let kind = opt_text(args, "kind");
    let band = opt_text(args, "band");
    let min = num(args, "min_strength").unwrap_or(0.0);
    let bump = opt_text(args, "bump").unwrap_or("Level");
    let depth = f32_of(num(args, "depth").unwrap_or(0.6));
    let label = opt_text(args, "name").map_or_else(|| kind.or(band).unwrap_or("hit").to_owned(), str::to_owned);
    let mut points: Vec<(u32, f64)> = Vec::new();
    if let Some(hits) = opt_text(args, "hits") {
        let file = cx.json(&cx.path(hits))?;
        for h in list(&file, "hits") {
            if band.is_none_or(|b| opt_text(h, "band").is_some_and(|x| x.eq_ignore_ascii_case(b)))
                && num(h, "strength").unwrap_or(1.0) >= min
            {
                if let Some(bar) = h.get("at").and_then(|a| a.get("bar")).and_then(whole) {
                    points.push((bar, h.get("at").and_then(|a| num(a, "beat")).unwrap_or(1.0)));
                }
            }
        }
    } else {
        for m in list(&doc, "markers") {
            if kind.is_none_or(|k| opt_text(m, "kind") == Some(k)) && num(m, "strength").unwrap_or(1.0) >= min {
                if let Some(bar) = m.get("bar").and_then(whole) {
                    points.push((bar, num(m, "beat").unwrap_or(1.0)));
                }
            }
        }
    }
    if let Some(only) = args.get("section") {
        let picked = pick_sections(&doc, only)?;
        points.retain(|(bar, _)| {
            picked.iter().any(|s| {
                #[allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let end = s.bar + s.bars.ceil() as u32;
                *bar >= s.bar && *bar < end
            })
        });
    }
    points.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    points.dedup_by(|a, b| a.0 == b.0 && (a.1 - b.1).abs() < 1e-6);
    if points.is_empty() {
        return Err("no hits: the song has no markers of that kind (IGNITION_SONG_MAP_SET puts them in) and no `hits` file was given".to_owned());
    }
    let recipe = serde_json::to_value(ignition_show::bump::bump(typed, bump_kind(bump)?, depth)).map_err(|e| e.to_string())?;
    let triggers = arr(&mut doc, "triggers")?;
    let before = triggers.len();
    if flag(args, "replace") {
        let prefix = format!("{label} ");
        triggers.retain(|t| !opt_text(t, "name").is_some_and(|n| n.starts_with(&prefix)));
    }
    let removed = before - triggers.len();
    for (bar, beat) in &points {
        triggers.push(json!({
            "at": { "bar": bar, "beat": beat }, "resolved": { "bar": bar, "beat": beat },
            "recipe": recipe, "name": format!("{label} {bar}.{beat}"), "hold": false,
        }));
    }
    if !dry {
        sort_cues(&mut doc, "triggers")?;
        cx.write_json(&path, &doc)?;
    }
    Ok(json!({
        "dry_run": dry, "target": target, "bump": bump, "depth": depth,
        "added": points.len(), "replaced": removed,
        "first": points.iter().take(8).map(|(b, t)| format!("{b}.{t}")).collect::<Vec<_>>(),
    }))
}

fn triggers(cx: &mut Ctx, args: &Value, clear: bool) -> Res {
    let l = layers(cx, args)?;
    let (path, mut doc) = l.song.clone().ok_or("`song` is required")?;
    let prefix = opt_text(args, "name");
    if clear {
        let all = arr(&mut doc, "triggers")?;
        let before = all.len();
        all.retain(|t| !prefix.is_none_or(|p| opt_text(t, "name").is_some_and(|n| n.starts_with(p))));
        let removed = before - all.len();
        cx.write_json(&path, &doc)?;
        return Ok(json!({ "removed": removed }));
    }
    let rows: Vec<Value> = list(&doc, "triggers")
        .iter()
        .filter(|t| prefix.is_none_or(|p| opt_text(t, "name").is_some_and(|n| n.starts_with(p))))
        .map(|t| json!({ "name": t.get("name"), "at": t.get("resolved").or_else(|| t.get("at")), "target": t.get("recipe").and_then(|r| r.get("target")), "hold": t.get("hold") }))
        .collect();
    Ok(json!({ "count": rows.len(), "triggers": rows }))
}

/// Sets a song's section map and markers, and re-resolves its cues'
/// bars against it. `sections`: `[{name, bars, bar?}]`; `markers`:
/// `[{name, kind, bar, beat, strength?}]` (`replace_markers` drops the
/// ones there, else they are added).
fn map_set(cx: &mut Ctx, args: &Value) -> Res {
    let l = layers(cx, args)?;
    let (path, mut doc) = l.song.clone().ok_or("`song` is required")?;
    if let Some(s) = args.get("sections") {
        obj(&mut doc)?.insert("sections".into(), s.clone());
    }
    if let Some(m) = args.get("markers").and_then(Value::as_array) {
        let all = arr(&mut doc, "markers")?;
        if flag(args, "replace_markers") {
            all.clear();
        }
        all.extend(m.iter().cloned());
    }
    let unresolved = re_resolve(&mut doc)?;
    cx.write_json(&path, &doc)?;
    Ok(json!({ "sections": sections_json(&doc), "markers": list(&doc, "markers").len(), "unresolved": unresolved }))
}

/// Every cue's and trigger's bar again from its `at` and the section map.
fn re_resolve(doc: &mut Value) -> Result<Vec<Value>, String> {
    let snapshot = doc.clone();
    let mut unresolved = Vec::new();
    for key in ["cues", "triggers"] {
        if doc.get(key).is_none() {
            continue;
        }
        for item in arr(doc, key)? {
            let Some(at) = item.get("at").cloned() else { continue };
            match place(&snapshot, &at) {
                Ok((_, resolved)) => {
                    obj(item)?.insert("resolved".into(), resolved);
                }
                Err(e) => unresolved.push(json!({ "name": item.get("name"), "why": e })),
            }
        }
        sort_cues(doc, key)?;
    }
    Ok(unresolved)
}

/// The section map of every song of the show from its Live set: each
/// song's locator range, its sections by bar. Cues are re-resolved.
fn map_import(cx: &mut Ctx, args: &Value) -> Res {
    let l = layers(cx, args)?;
    let set_path = opt_text(args, "live_set")
        .map(str::to_owned)
        .or_else(|| std::env::var("IGNITION_LIVE_SET").ok())
        .ok_or("`live_set` is required (or IGNITION_LIVE_SET)")?;
    let set = ignition_daw::ableton::LiveSet::read(&set_path).map_err(|e| e.to_string())?;
    let only = opt_text(args, "song").map(slug);
    let mut out = Vec::new();
    for entry in list(&l.show_doc, "songs") {
        let (Some(file), Some(locator)) = (opt_text(entry, "file"), opt_text(entry, "locator")) else { continue };
        if only.as_ref().is_some_and(|o| Some(o.clone()) != opt_text(entry, "song").map(slug)) {
            continue;
        }
        let Some(live) = set.song(locator) else {
            out.push(json!({ "song": entry.get("song"), "error": format!("the Live set has no song {locator:?}") }));
            continue;
        };
        let path = join(&dir_of(&l.show), file);
        let mut doc = cx.json(&path)?;
        let sects: Vec<Value> = live
            .song
            .sections
            .iter()
            .map(|s| json!({ "name": s.name, "bar": s.start.bar, "bars": s.bars }))
            .collect();
        let n = sects.len();
        obj(&mut doc)?.insert("sections".into(), json!(sects));
        let unresolved = re_resolve(&mut doc)?;
        cx.write_json(&path, &doc)?;
        out.push(json!({ "song": entry.get("song"), "sections": n, "unresolved": unresolved }));
    }
    Ok(json!({ "live_set": set_path, "songs": out }))
}

/// A song's literal colours made palette slots: each colour its cues
/// name becomes a slot (`White`, `Primary`, `Secondary`, `Tertiary`,
/// `Accent`, `Accent 2`…, by how much it is used), a palette of the
/// song's name holding them goes into the set's library, the cues name
/// the slots, and the song uses that palette. The look is the same;
/// from here another palette re-skins it.
fn song_slots(cx: &mut Ctx, args: &Value) -> Res {
    let mut l = layers(cx, args)?;
    let (path, mut doc) = l.song.clone().ok_or("`song` is required")?;
    let title = opt_text(&doc, "name").unwrap_or("song").to_owned();
    let existing: BTreeSet<String> = l.palette().1.iter().filter_map(|s| opt_text(s, "slot").map(str::to_owned)).collect();
    let mut weight: BTreeMap<String, usize> = BTreeMap::new();
    for key in ["cues", "triggers"] {
        for (kind, name) in names_in(&json!(list(&doc, key))) {
            if kind == "colors" && !existing.contains(&name) && l.find("colors", &name).is_some() {
                *weight.entry(name).or_default() += 1;
            }
        }
    }
    if weight.is_empty() {
        return Ok(json!({ "song": title, "slots": {}, "note": "its cues name no literal colours" }));
    }
    let mut ranked: Vec<(String, usize)> = weight.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let (whites, colours): (Vec<_>, Vec<_>) = ranked.into_iter().partition(|(n, _)| n.contains("White"));
    let mut slots: Map<String, Value> = Map::new();
    let mut rename: BTreeMap<String, String> = BTreeMap::new();
    for (i, (name, _)) in whites.iter().enumerate() {
        let slot = if i == 0 { "White".to_owned() } else { format!("White {}", i + 1) };
        slots.insert(slot.clone(), json!(name));
        rename.insert(name.clone(), slot);
    }
    for (i, (name, _)) in colours.iter().enumerate() {
        let slot = match i {
            0 => "Primary".to_owned(),
            1 => "Secondary".to_owned(),
            2 => "Tertiary".to_owned(),
            3 => "Accent".to_owned(),
            n => format!("Accent {}", n - 2),
        };
        slots.insert(slot.clone(), json!(name));
        rename.insert(name.clone(), slot);
    }
    fn rewrite(v: &mut Value, rename: &BTreeMap<String, String>) {
        match v {
            Value::Object(map) => {
                for (k, inner) in map.iter_mut() {
                    if k == "Color" {
                        if let Some(to) = inner.as_str().and_then(|n| rename.get(n)) {
                            *inner = json!(to);
                            continue;
                        }
                    }
                    if k == "colors" || k == "Colors" {
                        let items = if inner.is_array() { Some(inner) } else { inner.get_mut("colors") };
                        if let Some(items) = items.and_then(Value::as_array_mut) {
                            for c in items.iter_mut() {
                                if let Some(to) = c.as_str().and_then(|n| rename.get(n)) {
                                    *c = json!(to);
                                }
                            }
                        }
                        continue;
                    }
                    rewrite(inner, rename);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|i| rewrite(i, rename)),
            _ => {}
        }
    }
    for key in ["cues", "triggers"] {
        if let Some(v) = doc.get_mut(key) {
            rewrite(v, &rename);
        }
    }
    let lib = obj(&mut doc)?.entry("library").or_insert_with(|| json!({}));
    obj(lib)?.insert("palette".into(), json!(title));
    l.song = Some((path, doc));
    let pool = obj(&mut l.set)?.entry("palettes").or_insert_with(|| json!({}));
    obj(pool)?.insert(
        title.clone(),
        json!({ "slots": slots, "about": format!("{title} as it was programmed"), "tags": ["song"], "folder": "Songs" }),
    );
    l.save(cx)?;
    Ok(json!({ "song": title, "palette": title, "slots": slots }))
}

/// A song's cue list with nothing left to look up in the library: slot
/// and library colours written as values, the library's effects and
/// looks written in place, its focus points as points. For a player that
/// does not read the library (the `viz` tool, a session's lane, a copy
/// sent elsewhere). Written to `to`; the song's own file is not changed.
fn song_bake(cx: &mut Ctx, args: &Value) -> Res {
    let l = layers(cx, args)?;
    let (_, mut doc) = l.song.clone().ok_or("`song` is required")?;
    let (_, slots) = l.palette();
    let colour = |name: &str| -> Option<Value> {
        let rgb = slots
            .iter()
            .find(|s| opt_text(s, "slot") == Some(name))
            .cloned()
            .or_else(|| l.find("colors", name).filter(|(layer, _)| *layer != "profile").map(|(_, c)| c))?;
        Some(json!({ "name": name, "red": rgb.get("red")?, "green": rgb.get("green")?, "blue": rgb.get("blue")? }))
    };
    fn colours(v: &mut Value, colour: &dyn Fn(&str) -> Option<Value>, n: &mut usize) {
        match v {
            Value::Object(map) => {
                for (k, inner) in map.iter_mut() {
                    if k == "Color" {
                        if let Some(c) = inner.as_str().and_then(colour) {
                            *inner = c;
                            *n += 1;
                            continue;
                        }
                    }
                    if k == "colors" || k == "Colors" {
                        let items = if inner.is_array() { Some(&mut *inner) } else { inner.get_mut("colors") };
                        for c in items.and_then(Value::as_array_mut).into_iter().flatten() {
                            if let Some(to) = c.as_str().and_then(colour) {
                                *c = to;
                                *n += 1;
                            }
                        }
                        continue;
                    }
                    if k == "FocusPoint" {
                        continue;
                    }
                    colours(inner, colour, n);
                }
            }
            Value::Array(items) => items.iter_mut().for_each(|i| colours(i, colour, n)),
            _ => {}
        }
    }
    let (mut inlined, mut coloured) = (0, 0);
    for cue in arr(&mut doc, "cues")? {
        let parts = std::mem::take(arr(cue, "recipes")?);
        let mut out = Vec::new();
        for part in parts {
            if let Some((_, look)) = opt_text(&part, "look").and_then(|n| l.find("looks", n)).filter(|(layer, _)| *layer != "profile") {
                out.extend(list(&look, "recipes").iter().cloned());
                inlined += 1;
            } else if let Some((_, entry)) = opt_text(&part, "effect").and_then(|n| l.find("effects", n)).filter(|(layer, _)| *layer != "profile") {
                let mut recipe = entry.get("recipe").cloned().unwrap_or_default();
                let map = obj(&mut recipe)?;
                if let Some(t) = part.get("target") {
                    map.insert("target".into(), t.clone());
                }
                map.insert("name".into(), part.get("name").or_else(|| part.get("effect")).cloned().unwrap_or_default());
                if let Some(bars) = num(&part, "bars") {
                    let timing = map.entry("timing").or_insert_with(|| json!({}));
                    obj(timing)?.insert("measure".into(), json!(bars * 4.0));
                }
                out.push(recipe);
                inlined += 1;
            } else {
                out.push(part);
            }
        }
        for part in &mut out {
            if let Some(apply) = part.get_mut("steps") {
                for step in apply.as_array_mut().into_iter().flatten() {
                    for a in step.get_mut("apply").and_then(Value::as_array_mut).into_iter().flatten() {
                        if let Some((_, f)) = a.get("FocusPoint").and_then(Value::as_str).and_then(|n| l.find("focus", n)) {
                            *a = json!({ "FocusPoint": f.get("target") });
                        }
                    }
                }
            }
        }
        *arr(cue, "recipes")? = out;
    }
    for key in ["cues", "triggers"] {
        if let Some(v) = doc.get_mut(key) {
            colours(v, &colour, &mut coloured);
        }
    }
    if let Some(map) = doc.as_object_mut() {
        map.remove("library");
    }
    let to = cx.path(text(args, "to")?);
    cx.write_json(&to, &doc)?;
    Ok(json!({ "to": to, "colours_written": coloured, "parts_written_in_place": inlined, "cues": list(&doc, "cues").len() }))
}

/// What a set's songs refer to that is not there.
fn set_check(cx: &Ctx, args: &Value) -> Res {
    let base = layers(cx, args)?;
    let dir = dir_of(&base.show);
    let roles = roles_of(cx, &base);
    let profile = cx.json(&profile_path(cx, &base.show, &base.show_doc)).unwrap_or_default();
    let bundles: BTreeSet<String> = profile.get("bundles").and_then(Value::as_object).map(|b| b.keys().cloned().collect()).unwrap_or_default();
    let venue = join(&dir, opt_text(&base.show_doc, "venue").unwrap_or("."));
    let groups: BTreeSet<String> = cx
        .json(&venue.join("groups.json"))
        .map(|g| g.as_array().into_iter().flatten().filter_map(|g| opt_text(g, "label").map(str::to_owned)).collect())
        .unwrap_or_default();
    let binding = cx.json(&venue.join("profile.json")).unwrap_or_default();
    let palettes = cx.json(&venue.join("palettes.json")).unwrap_or_default();
    let mut focus: BTreeSet<String> = list(&palettes, "focus").iter().filter_map(|f| opt_text(f, "name").map(str::to_owned)).collect();
    focus.extend(binding.get("focus").and_then(Value::as_object).into_iter().flat_map(|m| m.keys().cloned()));
    let bound: BTreeSet<String> = binding.get("groups").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let mut songs = Vec::new();
    let mut total = 0;
    for entry in list(&base.show_doc, "songs") {
        let Some(title) = opt_text(entry, "song") else { continue };
        let mut a = args.clone();
        if let Some(m) = a.as_object_mut() {
            m.insert("song".into(), json!(title));
        }
        let mut problems: Vec<Value> = Vec::new();
        let l = match layers(cx, &a) {
            Ok(l) => l,
            Err(e) => {
                songs.push(json!({ "song": title, "problems": [{ "what": "file", "why": e }] }));
                total += 1;
                continue;
            }
        };
        let Some((_, doc)) = l.song.clone() else { continue };
        let (palette, slots) = l.palette();
        let slots: BTreeSet<String> = slots.iter().filter_map(|s| opt_text(s, "slot").map(str::to_owned)).collect();
        if let Some(p) = &palette {
            if l.find("palettes", p).is_none() {
                problems.push(json!({ "what": "palette", "name": p, "why": "the song's palette is not in the library" }));
            }
        }
        for (slot, _) in l.palette().1.iter().filter_map(|s| Some((opt_text(s, "slot")?, s.get("red")?.is_null()))).filter(|(_, missing)| *missing) {
            problems.push(json!({ "what": "slot", "name": slot, "why": "names a colour that is not there" }));
        }
        let sects = sections(&doc);
        for (key, items) in [("cue", list(&doc, "cues")), ("trigger", list(&doc, "triggers"))] {
            for item in items {
                let name = item.get("name").cloned().unwrap_or_default();
                let mut seen = BTreeSet::new();
                for (kind, n) in names_in(item) {
                    if !seen.insert((kind, n.clone())) {
                        continue;
                    }
                    let ok = match kind {
                        "colors" => slots.contains(&n) || l.find("colors", &n).is_some(),
                        "bundles" => bundles.contains(&n),
                        "roles" => roles.contains(&n),
                        "groups" => groups.contains(&n),
                        "focus" => focus.contains(&n) || l.find("focus", &n).is_some(),
                        other => l.find(other, &n).is_some(),
                    };
                    if !ok {
                        problems.push(json!({ "what": kind, "name": n, key: name, "why": "not in the library, the profile or the room" }));
                    } else if kind == "roles" && !bound.contains(&n) && !bound.is_empty() {
                        problems.push(json!({ "what": "role", "name": n, key: name, "why": "the room binds no fixtures to this role: the part lights nothing here" }));
                    }
                }
                if let Some(section) = item.get("at").and_then(|a| opt_text(a, "section")) {
                    if doc.get("sections").is_some() && !sects.iter().any(|s| s.name == section) {
                        problems.push(json!({ "what": "section", "name": section, key: name, "why": "the song's section map has no such section" }));
                    }
                }
            }
        }
        // The same role-warning for every cue is one finding, not forty.
        let mut once = BTreeSet::new();
        problems.retain(|p| p.get("what") != Some(&json!("role")) || once.insert(p.get("name").map(Value::to_string)));
        total += problems.len();
        songs.push(json!({ "song": title, "palette": palette, "cues": list(&doc, "cues").len(), "problems": problems }));
    }
    Ok(json!({ "show": base.show, "ok": total == 0, "problems": total, "songs": songs }))
}

/// One action over every lit song of a show (or `songs`), as one step.
pub(crate) fn set_each(args: &Value) -> Res {
    let cx = Ctx::new(args);
    let l = layers(&cx, args)?;
    let action = text(args, "action")?;
    let only: Option<BTreeSet<String>> = args.get("songs").and_then(Value::as_array).map(|s| s.iter().filter_map(|x| x.as_str().map(slug)).collect());
    let titles: Vec<String> = list(&l.show_doc, "songs")
        .iter()
        .filter(|e| e.get("lit").and_then(Value::as_bool) != Some(false) || flag(args, "unlit"))
        .filter_map(|e| opt_text(e, "song").map(str::to_owned))
        .filter(|t| only.as_ref().is_none_or(|o| o.contains(&slug(t))))
        .collect();
    let steps: Vec<Value> = titles
        .iter()
        .map(|t| {
            let mut a = args.get("args").cloned().unwrap_or_else(|| json!({}));
            if let Some(m) = a.as_object_mut() {
                m.insert("song".into(), json!(t));
                for key in ["show", "root"] {
                    if let Some(v) = args.get(key) {
                        m.entry(key).or_insert_with(|| v.clone());
                    }
                }
            }
            json!({ "action": action, "args": a })
        })
        .collect();
    let label = format!("{action} over {} songs", steps.len());
    let out = super::run("IGNITION_BATCH", &json!({ "label": label, "steps": steps, "root": args.get("root") }))?;
    let results: Vec<Value> = titles
        .iter()
        .zip(list(&out, "results"))
        .map(|(t, r)| json!({ "song": t, "result": r }))
        .collect();
    Ok(json!({ "action": action, "songs": count_f(results.len()), "results": results }))
}

pub(crate) fn run(cx: &mut Ctx, id: &str, args: &Value) -> Option<Res> {
    Some(match id {
        "IGNITION_SONG_LIST" => song_list(cx, args),
        "IGNITION_SONG_GET" => song_get(cx, args),
        "IGNITION_SONG_CREATE" => song_create(cx, args),
        "IGNITION_SONG_MAP_SET" => map_set(cx, args),
        "IGNITION_SONG_MAP_IMPORT" => map_import(cx, args),
        "IGNITION_SONG_SLOTS" => song_slots(cx, args),
        "IGNITION_SONG_BAKE" => song_bake(cx, args),
        "IGNITION_CUE_GET" => cue_get(cx, args),
        "IGNITION_CUE_ADD" => cue_add(cx, args),
        "IGNITION_CUE_UPDATE" => cue_update(cx, args),
        "IGNITION_CUE_DELETE" => cue_delete(cx, args),
        "IGNITION_CUE_DUPLICATE" => cue_duplicate(cx, args),
        "IGNITION_SECTION_APPLY" => section_apply(cx, args),
        "IGNITION_SECTION_COPY" => section_copy(cx, args),
        "IGNITION_FILL_SECTIONS" => fill_sections(cx, args),
        "IGNITION_FILL_HITS" => fill_hits(cx, args),
        "IGNITION_TRIGGER_LIST" => triggers(cx, args, false),
        "IGNITION_TRIGGER_CLEAR" => triggers(cx, args, true),
        "IGNITION_SET_CHECK" => set_check(cx, args),
        _ => return None,
    })
}
