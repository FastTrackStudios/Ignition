//! Environments, people, fixtures, groups and the patch.
//!
//! An **environment** is a venue directory (`data/venues/<name>/`): the
//! room (`room.json`: floor, walls, truss, risers, the audience area),
//! its props and screens, the fixtures hung in it with their addresses
//! (`fixtures.json`), its groups, palettes (focus points), the profile
//! binding (which fixtures play which role), where its universes go on
//! the wire (`venue.ig-venue`), and, added here, the people standing in
//! it (`people.json`) and where they stand per song and section
//! (`blocking.json`).
//!
//! Coordinates are metres: x stage right (−) to stage left (+), y
//! downstage (−) to upstage (+), z up, the playing area centred on the
//! origin.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ignition_show::profile::VenueProfile;
use ignition_show::{FixtureInfo, Group, Placement, Quat, Rig, Selection, Vec3};
use serde_json::{Map, Value, json};

use super::library::{dir_of, join, layers, show_path};
use super::song::{put_part, roles_of, selection};
use super::{Ctx, Res, arr, count_f, flag, list, merge, num, obj, opt_text, slug, text, whole};

/// The environment an action is about: `env` (a path, or a name under
/// `data/venues`), else the show's venue.
pub(crate) fn env_dir(cx: &Ctx, args: &Value) -> Result<PathBuf, String> {
    if let Some(env) = opt_text(args, "env") {
        return Ok(if env.contains('/') { cx.path(env) } else { cx.data().join("venues").join(env) });
    }
    let show = show_path(cx, args);
    let doc = cx.json(&show).map_err(|e| format!("`env` is required, or a show with a venue ({e})"))?;
    Ok(join(&dir_of(&show), text(&doc, "venue")?))
}

fn file(cx: &Ctx, dir: &Path, name: &str) -> Value {
    cx.json_or(&dir.join(name), json!([])).unwrap_or_else(|_| json!([]))
}

fn xyz(v: Option<&Value>) -> Option<Vec3> {
    let v = v?;
    Some(Vec3 { x: num(v, "x")?, y: num(v, "y")?, z: num(v, "z").unwrap_or(0.0) })
}

fn vec_json(v: Vec3) -> Value {
    json!({ "x": v.x, "y": v.y, "z": v.z })
}

/// The hang a set of eulers (degrees, applied Z then Y then X, as the
/// visualizer reads them) makes.
fn quat_of(e: Vec3) -> Value {
    let (hx, hy, hz) = (e.x.to_radians() / 2.0, e.y.to_radians() / 2.0, e.z.to_radians() / 2.0);
    let (sx, cx) = hx.sin_cos();
    let (sy, cy) = hy.sin_cos();
    let (sz, cz) = hz.sin_cos();
    json!({
        "w": (cz * cy).mul_add(cx, sz * sy * sx),
        "x": (cz * cy).mul_add(sx, -(sz * sy * cx)),
        "y": (cz * sy).mul_add(cx, sz * cy * sx),
        "z": (sz * cy).mul_add(cx, -(cz * sy * sx)),
    })
}

/// The hang that points a fixture's beam, at rest, from `from` at `at`:
/// the shortest turn from straight down (the beam of an unturned
/// fixture) to that direction.
fn quat_aim(from: Vec3, at: Vec3) -> Value {
    let (dx, dy, dz) = (at.x - from.x, at.y - from.y, at.z - from.z);
    let n = dz.mul_add(dz, dx.mul_add(dx, dy * dy)).sqrt();
    if n < 1e-9 {
        return json!({ "w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0 });
    }
    let (dx, dy, dz) = (dx / n, dy / n, dz / n);
    let dot = -dz;
    if dot > 0.999_999 {
        return json!({ "w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0 });
    }
    if dot < -0.999_999 {
        return json!({ "w": 0.0, "x": 1.0, "y": 0.0, "z": 0.0 });
    }
    // (0, 0, -1) × d
    let (cx, cy, w) = (dy, -dx, 1.0 + dot);
    let n = w.mul_add(w, cx.mul_add(cx, cy * cy)).sqrt();
    json!({ "w": w / n, "x": cx / n, "y": cy / n, "z": 0.0 })
}

fn chans_of(entries: &[Value]) -> Vec<u32> {
    let mut out = Vec::new();
    for e in entries {
        match e.as_str() {
            Some(s) => match s.split_once('-') {
                Some((a, b)) => {
                    if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                        out.extend(a..=b);
                    }
                }
                None => out.extend(s.trim().parse::<u32>().ok()),
            },
            None => out.extend(whole(e)),
        }
    }
    out
}

/// The room as the engine resolves selections against it.
pub(crate) struct Room {
    pub dir: PathBuf,
    pub fixtures: Value,
    pub rig: Rig,
    pub groups: Vec<Group>,
    pub binding: VenueProfile,
}

pub(crate) fn room(cx: &Ctx, args: &Value) -> Result<Room, String> {
    let dir = env_dir(cx, args)?;
    let fixtures = file(cx, &dir, "fixtures.json");
    let infos = fixtures
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let q = f.get("quat");
            Some(FixtureInfo {
                chan: f.get("chan").and_then(whole)?,
                placement: xyz(f.get("position")).map(|position| Placement {
                    position,
                    orientation: Quat {
                        w: q.and_then(|q| num(q, "w")).unwrap_or(1.0),
                        x: q.and_then(|q| num(q, "x")).unwrap_or(0.0),
                        y: q.and_then(|q| num(q, "y")).unwrap_or(0.0),
                        z: q.and_then(|q| num(q, "z")).unwrap_or(0.0),
                    },
                }),
                manufacturer: opt_text(f, "manufacturer").unwrap_or_default().to_owned(),
                model: opt_text(f, "model").unwrap_or_default().to_owned(),
                tags: list(f, "tags").iter().filter_map(|t| t.as_str().map(str::to_owned)).collect(),
            })
        })
        .collect();
    let groups = file(cx, &dir, "groups.json")
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|g| Some(Group { name: opt_text(g, "label")?.to_owned(), chans: chans_of(list(g, "channels")) }))
        .collect();
    let binding = cx
        .json(&dir.join("profile.json"))
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    Ok(Room { dir, fixtures, rig: Rig::new(infos), groups, binding })
}

impl Room {
    /// The channels a target names, in its order.
    pub(crate) fn resolve(&self, target: &Value) -> Result<Vec<u32>, String> {
        let roles: BTreeSet<String> = self.binding.groups.keys().cloned().collect();
        let sel: Selection = serde_json::from_value(selection(target, &roles)?).map_err(|e| format!("target: {e}"))?;
        Ok(ignition_show::selection::resolve_with(&sel, &self.groups, &self.rig, &self.binding))
    }
}

// ---------------------------------------------------------------------
// Environments
// ---------------------------------------------------------------------

fn env_list(cx: &Ctx, args: &Value) -> Res {
    let venues = cx.data().join("venues");
    let in_use = env_dir(cx, args).ok();
    let mut out = Vec::new();
    let mut names: Vec<PathBuf> = std::fs::read_dir(&venues)
        .map_err(|e| format!("{}: {e}", venues.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("fixtures.json").is_file() || p.join("venue.ig-venue").is_file())
        .collect();
    names.sort();
    for dir in names {
        let manifest = cx.json_or(&dir.join("venue.ig-venue"), json!({}))?;
        let fixtures = file(cx, &dir, "fixtures.json");
        let universes: BTreeSet<u32> = fixtures.as_array().into_iter().flatten().filter_map(|f| f.get("universe").and_then(whole)).collect();
        out.push(json!({
            "env": dir.file_name().and_then(|n| n.to_str()),
            "name": manifest.get("name"), "dir": dir,
            "fixtures": fixtures.as_array().map_or(0, Vec::len),
            "universes": universes,
            "people": file(cx, &dir, "people.json").as_array().map_or(0, Vec::len),
            "measured": dir.join(MEASURED).is_file(),
            "in_use": in_use.as_ref() == Some(&dir),
        }));
    }
    Ok(json!({ "environments": out }))
}

fn env_get(cx: &Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let palettes = cx.json_or(&r.dir.join("palettes.json"), json!({}))?;
    let manifest = cx.json_or(&r.dir.join("venue.ig-venue"), json!({}))?;
    // A measured room: what is drawn instead of `room.json`'s boxes.
    let measured = measured_room(cx, &r.dir).ok().map(|(m, _)| {
        json!({
            "name": m.get("name"), "model": m.get("model"), "dimensions": m.get("measured"),
            "light_groups": m.get("light_groups"), "lights": list(&m, "lights").len(),
            "objects": list(&m, "objects").iter().map(|o| json!({
                "name": o.get("name"), "kind": o.get("kind"), "position": o.get("position"),
                "turn_deg": o.get("eulers").and_then(|e| e.get("z")), "size": o.get("size"),
                "visible": o.get("visible"), "movable": o.get("movable"), "group": o.get("group"),
            })).collect::<Vec<_>>(),
        })
    });
    let bound: Map<String, Value> = r
        .binding
        .groups
        .iter()
        .map(|(role, sel)| {
            let chans = ignition_show::selection::resolve_with(sel, &r.groups, &r.rig, &r.binding);
            (role.clone(), json!({ "is": sel, "fixtures": chans.len() }))
        })
        .collect();
    Ok(json!({
        "env": r.dir.file_name().and_then(|n| n.to_str()), "dir": r.dir, "name": manifest.get("name"),
        "room": file(cx, &r.dir, "room.json"),
        "props": file(cx, &r.dir, "props.json"),
        "screens": file(cx, &r.dir, "screens.json"),
        "people": file(cx, &r.dir, "people.json"),
        "measured": measured,
        "fixtures": r.fixtures.as_array().map_or(0, Vec::len),
        "groups": r.groups.iter().map(|g| json!({ "name": g.name, "fixtures": g.chans.len() })).collect::<Vec<_>>(),
        "roles": bound, "focus_roles": r.binding.focus,
        "focus": palettes.get("focus"),
        "dmx": manifest.get("dmx"),
        "look": manifest.get("look"),
    }))
}

fn piece(name: &str, position: (f64, f64, f64), size: (f64, f64, f64), turn: f64, kind: &str) -> Value {
    json!({
        "name": name, "kind": kind,
        "position": { "x": position.0, "y": position.1, "z": position.2 },
        "eulers": { "x": 0, "y": 0, "z": turn },
        "size": { "x": size.0, "y": size.1, "z": size.2 },
    })
}

/// A room from its dimensions: a floor, four walls, and, when asked, a
/// stage riser, an audience area and trusses. `upstage` is how far the
/// upstage wall is behind the origin (the centre of the playing area).
fn env_create(cx: &mut Ctx, args: &Value) -> Res {
    let name = text(args, "name")?;
    let dir = cx.data().join("venues").join(slug(name));
    if cx.exists(&dir.join("venue.ig-venue")) || dir.join("fixtures.json").is_file() {
        return Err(format!("{} is already an environment (IGNITION_ENV_DUPLICATE copies one)", dir.display()));
    }
    let width = num(args, "width").ok_or("`width` (metres) is required")?;
    let depth = num(args, "depth").ok_or("`depth` (metres) is required")?;
    let height = num(args, "height").unwrap_or(4.0);
    let upstage = num(args, "upstage").unwrap_or(depth * 0.2);
    let house = upstage - depth;
    let mut pieces = vec![
        piece("Floor", (0.0, upstage - depth / 2.0, 0.0), (width, depth, 0.0), 0.0, "floor"),
        piece("Wall - Upstage", (0.0, upstage, 0.0), (width, 0.0, height), 0.0, "wall"),
        piece("Wall - House Back", (0.0, house, 0.0), (width, 0.0, height), -180.0, "wall"),
        piece("Wall - Stage Left", (width / 2.0, upstage - depth / 2.0, 0.0), (depth, 0.0, height), -90.0, "wall"),
        piece("Wall - Stage Right", (-width / 2.0, upstage - depth / 2.0, 0.0), (depth, 0.0, height), 90.0, "wall"),
    ];
    let mut props = Vec::new();
    if let Some(stage) = args.get("stage") {
        let (w, d, h) = (num(stage, "width").unwrap_or(width * 0.6), num(stage, "depth").unwrap_or(3.0), num(stage, "height").unwrap_or(0.3));
        props.push(piece("Stage", (0.0, num(stage, "y").unwrap_or(0.0), 0.0), (w, d, h), 0.0, "riser"));
    }
    if let Some(aud) = args.get("audience") {
        let d = num(aud, "depth").unwrap_or((depth - upstage - 2.0).max(1.0));
        let front = num(aud, "front").unwrap_or(-2.0);
        pieces.push(piece("Audience", (0.0, front - d / 2.0, 0.01), (num(aud, "width").unwrap_or(width * 0.8), d, 0.0), 0.0, "audience"));
    }
    for t in list(args, "truss") {
        let label = opt_text(t, "name").unwrap_or("Truss");
        props.push(piece(
            &format!("Truss - {label}"),
            (num(t, "x").unwrap_or(0.0), num(t, "y").unwrap_or(0.0), num(t, "z").unwrap_or(height - 0.5)),
            (num(t, "width").unwrap_or(width * 0.6), 0.3, 0.3),
            num(t, "turn").unwrap_or(0.0),
            "truss",
        ));
    }
    let focus = [
        ("Vocal Centre", 0.0, -0.9, 1.55), ("Stage Wide", 0.0, 0.0, 1.2), ("Upstage Centre", 0.0, upstage * 0.5, 1.55),
        ("Drums", 0.0, upstage * 0.5, 1.1), ("Band Wall", 0.0, upstage, height * 0.5),
        ("Audience Front", 0.0, -3.0, 1.2), ("Audience Back", 0.0, house + 1.0, 1.2),
    ];
    let points: Vec<Value> = focus.iter().map(|(n, x, y, z)| json!({ "name": n, "target": { "x": x, "y": y, "z": z } })).collect();
    let title = opt_text(args, "title").unwrap_or(name);
    let files: [(&str, Value); 9] = [
        ("venue.ig-venue", json!({ "version": 1, "name": title, "profile": "Ignition", "dmx": { "universes": { "1": { "sacn": { "priority": 100 }, "artnet": { "net": 0, "subnet": 0, "universe": 0 } } } } })),
        ("room.json", json!(pieces)),
        ("props.json", json!(props)),
        ("screens.json", json!([])),
        ("fixtures.json", json!([])),
        ("groups.json", json!([])),
        ("people.json", json!([])),
        ("palettes.json", json!({ "colors": [], "focus": points })),
        ("profile.json", json!({ "profile": "Ignition", "groups": {}, "focus": {
            "Vocal": "Vocal Centre", "Stage": "Stage Wide", "Band": "Upstage Centre", "Drums": "Drums",
            "Back Wall": "Band Wall", "Audience": "Audience Front", "House": "Audience Back" }, "canvases": {} })),
    ];
    for (file, value) in &files {
        cx.write_json(&dir.join(file), value)?;
    }
    cx.write_json(&dir.join("cameras.json"), &json!({ "presets": [
        { "name": "Wide", "eye": [0.0, house + 0.6, 1.7], "look": [0.0, 0.0, 1.2], "fov_deg": 65.0, "about": "The whole room from the back of the house." },
        { "name": "Top", "eye": [0.0, -0.5, height * 2.2], "look": [0.0, 0.0, 0.0], "fov_deg": 60.0, "about": "From above." },
    ] }))?;
    Ok(json!({ "env": slug(name), "dir": dir, "name": title, "room": pieces, "props": props }))
}

fn env_duplicate(cx: &mut Ctx, args: &Value) -> Res {
    let from = env_dir(cx, args)?;
    let name = text(args, "as")?;
    let to = cx.data().join("venues").join(slug(name));
    if to.exists() {
        return Err(format!("{} is already there", to.display()));
    }
    let mut copied = Vec::new();
    let mut skipped = Vec::new();
    for entry in std::fs::read_dir(&from).map_err(|e| format!("{}: {e}", from.display()))?.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else { continue };
        if !path.is_file() {
            skipped.push(file_name);
            continue;
        }
        match cx.read(&path) {
            Ok(mut text) => {
                if file_name == "venue.ig-venue" {
                    if let Ok(mut v) = serde_json::from_str::<Value>(&text) {
                        obj(&mut v)?.insert("name".into(), json!(opt_text(args, "title").unwrap_or(name)));
                        text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
                    }
                }
                cx.write(&to.join(&file_name), text);
                copied.push(file_name);
            }
            Err(_) => skipped.push(file_name),
        }
    }
    Ok(json!({ "env": slug(name), "dir": to, "copied": copied, "not_copied": skipped }))
}

fn env_use(cx: &mut Ctx, args: &Value) -> Res {
    let show = show_path(cx, args);
    let mut doc = cx.json(&show)?;
    let env = text(args, "env")?;
    let dir = cx.data().join("venues").join(env);
    if !cx.exists(&dir.join("fixtures.json")) {
        return Err(format!("no environment {env:?} (IGNITION_ENV_LIST)"));
    }
    let was = doc.get("venue").cloned();
    obj(&mut doc)?.insert("venue".into(), json!(format!("../venues/{env}")));
    cx.write_json(&show, &doc)?;
    Ok(json!({ "show": show, "env": env, "was": was, "note": "a running renderer takes it when it starts again" }))
}

fn piece_file(args: &Value) -> Result<&'static str, String> {
    match opt_text(args, "in").unwrap_or("room") {
        "room" => Ok("room.json"),
        "props" | "prop" => Ok("props.json"),
        "screens" | "screen" => Ok("screens.json"),
        other => Err(format!("`in` {other:?}: room, props, screens, or environment (a measured room's objects)")),
    }
}

/// The measured room, when the environment has one: `environment.json`
/// (the visualizer's `environment.rs`): the model's objects by name, each
/// with where it stands, whether it is shown and whether it may move, and
/// the room's own lights in switchable groups.
const MEASURED: &str = "environment.json";

/// The edits these actions make to a measured room, beside it. The room's
/// own file is generated and is never written here: a rebuild of the room
/// keeps every edit, and the loader lays this over it
/// (`ignition_viz::environment::EDITS`).
const EDITS: &str = "environment.edits.json";

/// A measured room as it stands: its generated document with the edits
/// laid over it. Also the edits alone, to write back.
fn measured_room(cx: &Ctx, dir: &Path) -> Result<(Value, Value), String> {
    let mut doc = cx
        .json(&dir.join(MEASURED))
        .map_err(|_| "this environment has no measured model (environment.json)".to_owned())?;
    let edits = cx.json_or(&dir.join(EDITS), json!({}))?;
    for o in arr(&mut doc, "objects")? {
        if let Some(edit) = opt_text(o, "name").and_then(|n| edits.get("objects").and_then(|e| e.get(n))) {
            let edit = edit.clone();
            merge(o, &edit);
        }
    }
    for g in arr(&mut doc, "light_groups")? {
        if let Some(on) = opt_text(g, "name").and_then(|n| edits.get("light_groups").and_then(|e| e.get(n))) {
            let on = on.clone();
            obj(g)?.insert("on".into(), on);
        }
    }
    Ok((doc, edits))
}

/// Moves, turns, shows or hides an object of the measured room. Only what
/// the model has can be changed (an object is a node of the model), and
/// one marked not `movable` (the architecture) moves only with `force`.
/// The change is written to the edits file, not to the generated room.
fn object_set(cx: &mut Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let name = text(args, "name")?;
    let (doc, mut edits) = measured_room(cx, &dir)?;
    let objects = list(&doc, "objects");
    let slot = objects.iter().find(|o| opt_text(o, "name") == Some(name)).ok_or_else(|| {
        let names: Vec<&str> = objects.iter().filter_map(|o| opt_text(o, "name")).collect();
        format!("the model has no object {name:?}; it has {names:?}")
    })?;
    let moves = args.get("position").is_some() || args.get("eulers").is_some() || args.get("move").is_some();
    if moves && slot.get("movable").and_then(Value::as_bool) == Some(false) && !flag(args, "force") {
        return Err(format!("{name:?} is part of the room and does not move (`force: true` moves it anyway)"));
    }
    let mut set = Map::new();
    for key in ["position", "eulers", "visible"] {
        if let Some(v) = args.get(key) {
            set.insert(key.into(), v.clone());
        }
    }
    if let Some(by) = args.get("move") {
        let at = xyz(set.get("position").or_else(|| slot.get("position"))).unwrap_or(Vec3 { x: 0.0, y: 0.0, z: 0.0 });
        set.insert("position".into(), json!({ "x": at.x + num(by, "x").unwrap_or(0.0), "y": at.y + num(by, "y").unwrap_or(0.0), "z": at.z + num(by, "z").unwrap_or(0.0) }));
    }
    let mut now = slot.clone();
    merge(&mut now, &Value::Object(set.clone()));
    let pool = obj(&mut edits)?.entry("objects").or_insert_with(|| json!({}));
    let entry = obj(pool)?.entry(name).or_insert_with(|| json!({}));
    if flag(args, "reset") {
        obj(pool)?.remove(name);
    } else {
        merge(entry, &Value::Object(set));
    }
    cx.write_json(&dir.join(EDITS), &edits)?;
    Ok(json!({ "object": now, "note": "only eulers.z turns an object; `reset: true` drops this object's edits" }))
}

/// The room's own lights (the house lights, not the rig): the groups and
/// whether each starts on; `group` + `on` switches one (in the edits file).
fn env_lights(cx: &mut Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let (mut doc, mut edits) = measured_room(cx, &dir)?;
    if let Some(group) = opt_text(args, "group") {
        let on = args.get("on").and_then(Value::as_bool).ok_or("`on` (true or false) is required with `group`")?;
        let names: Vec<String> = list(&doc, "light_groups").iter().filter_map(|g| opt_text(g, "name").map(str::to_owned)).collect();
        if !names.iter().any(|n| n == group) {
            return Err(format!("no light group {group:?}; the room has {names:?}"));
        }
        let pool = obj(&mut edits)?.entry("light_groups").or_insert_with(|| json!({}));
        obj(pool)?.insert(group.into(), json!(on));
        cx.write_json(&dir.join(EDITS), &edits)?;
        for g in arr(&mut doc, "light_groups")? {
            if opt_text(g, "name") == Some(group) {
                obj(g)?.insert("on".into(), json!(on));
            }
        }
    }
    let groups: Vec<Value> = list(&doc, "light_groups")
        .iter()
        .map(|g| {
            let n = list(&doc, "lights").iter().filter(|l| l.get("group") == g.get("name")).count();
            json!({ "name": g.get("name"), "label": g.get("label"), "on": g.get("on"), "lights": n })
        })
        .collect();
    Ok(json!({ "light_groups": groups }))
}

/// How the room looks to the camera: `haze` (0 a clear room, 1 normally
/// hazed; a room with no hazer is near 0) and `exposure` (stops on the
/// stage exposure). The renderer and the stills read them.
fn env_look(cx: &mut Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let path = dir.join("venue.ig-venue");
    let mut doc = cx.json_or(&path, json!({ "version": 1, "profile": "Ignition" }))?;
    let look = obj(&mut doc)?.entry("look").or_insert_with(|| json!({}));
    for key in ["haze", "exposure"] {
        match args.get(key) {
            Some(Value::Null) => {
                obj(look)?.remove(key);
            }
            Some(v) if v.is_number() => {
                obj(look)?.insert(key.into(), v.clone());
            }
            Some(_) => return Err(format!("`{key}` is a number")),
            None => {}
        }
    }
    let now = look.clone();
    cx.write_json(&path, &doc)?;
    Ok(json!({ "look": now, "defaults": { "haze": 0.8, "exposure": 0.0 } }))
}

fn piece_set(cx: &mut Ctx, args: &Value) -> Res {
    if matches!(opt_text(args, "in"), Some("environment" | "model")) {
        return object_set(cx, args);
    }
    let dir = env_dir(cx, args)?;
    let name = text(args, "name")?;
    let path = dir.join(piece_file(args)?);
    let mut all = cx.json_or(&path, json!([]))?;
    let items = all.as_array_mut().ok_or("not a list")?;
    let mut set = args.get("set").cloned().unwrap_or_else(|| json!({}));
    for key in ["position", "size", "eulers", "kind", "canvas"] {
        if let Some(v) = args.get(key) {
            obj(&mut set)?.insert(key.into(), v.clone());
        }
    }
    let made = !items.iter().any(|p| opt_text(p, "name") == Some(name));
    if made {
        items.push(json!({ "name": name, "position": { "x": 0, "y": 0, "z": 0 }, "eulers": { "x": 0, "y": 0, "z": 0 }, "size": { "x": 1, "y": 1, "z": 1 } }));
    }
    let slot = items.iter_mut().find(|p| opt_text(p, "name") == Some(name)).ok_or("no such piece")?;
    merge(slot, &set);
    let now = slot.clone();
    cx.write_json(&path, &all)?;
    Ok(json!({ "piece": now, "created": made }))
}

fn piece_remove(cx: &mut Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let name = text(args, "name")?;
    let path = dir.join(piece_file(args)?);
    let mut all = cx.json_or(&path, json!([]))?;
    let items = all.as_array_mut().ok_or("not a list")?;
    let before = items.len();
    items.retain(|p| opt_text(p, "name") != Some(name));
    if items.len() == before {
        return Err(format!("no piece {name:?} there"));
    }
    cx.write_json(&path, &all)?;
    Ok(json!({ "removed": name }))
}

// ---------------------------------------------------------------------
// People
// ---------------------------------------------------------------------

fn head(p: &Value) -> Vec3 {
    let at = xyz(p.get("position")).unwrap_or(Vec3 { x: 0.0, y: 0.0, z: 0.0 });
    Vec3 { z: num(p, "height").unwrap_or(1.75).mul_add(0.93, at.z), ..at }
}

/// People as the room shows and aims at them: a prop each (`Person -
/// <name>`, what the visualizer draws) and a focus point at head height
/// under the person's name.
fn sync_people(cx: &mut Ctx, dir: &Path, people: &[Value]) -> Result<(), String> {
    let props_path = dir.join("props.json");
    let mut props = cx.json_or(&props_path, json!([]))?;
    let items = props.as_array_mut().ok_or("props.json is not a list")?;
    items.retain(|p| !(p.get("person").is_some() || opt_text(p, "name").is_some_and(|n| people.iter().any(|x| opt_text(x, "name").map(|x| format!("Person - {x}")).as_deref() == Some(n)))));
    for p in people {
        let name = opt_text(p, "name").unwrap_or("?");
        let h = num(p, "height").unwrap_or(1.75);
        items.push(json!({
            "name": format!("Person - {name}"), "person": name, "label": p.get("label"),
            "position": p.get("position"),
            "eulers": { "x": 0, "y": 0, "z": num(p, "facing_deg").unwrap_or(0.0) },
            "size": { "x": 0.6, "y": 0.3, "z": h },
        }));
    }
    cx.write_json(&props_path, &props)?;
    let pal_path = dir.join("palettes.json");
    let mut pal = cx.json_or(&pal_path, json!({ "colors": [], "focus": [] }))?;
    let focus = arr(&mut pal, "focus")?;
    for p in people {
        let name = opt_text(p, "name").unwrap_or("?");
        focus.retain(|f| opt_text(f, "name") != Some(name));
        focus.push(json!({ "name": name, "target": vec_json(head(p)), "person": true }));
    }
    cx.write_json(&pal_path, &pal)
}

fn person_set(cx: &mut Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let path = dir.join("people.json");
    let mut all = cx.json_or(&path, json!([]))?;
    let given: Vec<Value> = match args.get("people").and_then(Value::as_array) {
        Some(many) => many.clone(),
        None => vec![args.clone()],
    };
    let mut out = Vec::new();
    for g in &given {
        let name = text(g, "name")?;
        let items = all.as_array_mut().ok_or("people.json is not a list")?;
        if !items.iter().any(|p| opt_text(p, "name") == Some(name)) {
            items.push(json!({ "name": name, "position": { "x": 0.0, "y": 0.0, "z": 0.0 }, "facing_deg": 0.0, "height": 1.75, "stand_in": false }));
        }
        let slot = items.iter_mut().find(|p| opt_text(p, "name") == Some(name)).ok_or("no such person")?;
        let mut set = Map::new();
        for key in ["label", "position", "facing_deg", "height", "stand_in"] {
            if let Some(v) = g.get(key) {
                set.insert(key.into(), v.clone());
            }
        }
        merge(slot, &Value::Object(set));
        if let Some(by) = g.get("move") {
            let at = xyz(slot.get("position")).unwrap_or(Vec3 { x: 0.0, y: 0.0, z: 0.0 });
            let moved = Vec3 { x: at.x + num(by, "x").unwrap_or(0.0), y: at.y + num(by, "y").unwrap_or(0.0), z: at.z + num(by, "z").unwrap_or(0.0) };
            obj(slot)?.insert("position".into(), vec_json(moved));
        }
        out.push(slot.clone());
    }
    let people = all.as_array().cloned().unwrap_or_default();
    sync_people(cx, &dir, &people)?;
    cx.write_json(&path, &all)?;
    Ok(json!({ "people": out }))
}

fn person_remove(cx: &mut Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let name = text(args, "name")?;
    let path = dir.join("people.json");
    let mut all = cx.json_or(&path, json!([]))?;
    let items = all.as_array_mut().ok_or("people.json is not a list")?;
    let before = items.len();
    items.retain(|p| opt_text(p, "name") != Some(name));
    if items.len() == before {
        return Err(format!("no person {name:?}"));
    }
    let people = items.clone();
    cx.write_json(&path, &all)?;
    let props_path = dir.join("props.json");
    let mut props = cx.json_or(&props_path, json!([]))?;
    if let Some(items) = props.as_array_mut() {
        items.retain(|p| opt_text(p, "person") != Some(name));
    }
    cx.write_json(&props_path, &props)?;
    let pal_path = dir.join("palettes.json");
    let mut pal = cx.json_or(&pal_path, json!({}))?;
    if pal.get("focus").is_some() {
        arr(&mut pal, "focus")?.retain(|f| !(opt_text(f, "name") == Some(name) && f.get("person").is_some()));
        cx.write_json(&pal_path, &pal)?;
    }
    sync_people(cx, &dir, &people)?;
    Ok(json!({ "removed": name }))
}

/// Where everyone stands for a song and section: the people, with that
/// song's blocking laid over (the section's over the song's `*`).
fn standing(cx: &Ctx, dir: &Path, song: Option<&str>, section: Option<&str>) -> Vec<Value> {
    let mut people = file(cx, dir, "people.json").as_array().cloned().unwrap_or_default();
    let blocking = cx.json_or(&dir.join("blocking.json"), json!({})).unwrap_or_default();
    let Some(song) = song.map(slug) else { return people };
    for key in [Some("*"), section].into_iter().flatten() {
        let Some(positions) = blocking.get(&song).and_then(|s| s.get(key)).and_then(Value::as_object) else { continue };
        for p in &mut people {
            if let Some(over) = opt_text(p, "name").and_then(|n| positions.get(n)) {
                let mut at = p.get("position").cloned().unwrap_or_else(|| json!({}));
                merge(&mut at, &json!({ "x": over.get("x"), "y": over.get("y") }));
                if let Some(z) = over.get("z") {
                    merge(&mut at, &json!({ "z": z }));
                }
                merge(p, &json!({ "position": at, "from": format!("{song}/{key}") }));
                if let Some(f) = over.get("facing_deg") {
                    merge(p, &json!({ "facing_deg": f }));
                }
            }
        }
    }
    people
}

fn people_list(cx: &Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let people = standing(cx, &dir, opt_text(args, "song"), opt_text(args, "section"));
    let rows: Vec<Value> = people.iter().map(|p| { let at = vec_json(head(p)); let mut p = p.clone(); merge(&mut p, &json!({ "head": at })); p }).collect();
    Ok(json!({ "people": rows, "blocking": cx.json_or(&dir.join("blocking.json"), json!({}))? }))
}

/// Saves where people stand for a song (`section` `*`, the default: the
/// whole song) or one of its sections. `show: true` also moves the
/// room's people there, so the visualizer and the focus points follow.
fn blocking_set(cx: &mut Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let song = slug(text(args, "song")?);
    let section = opt_text(args, "section").unwrap_or("*");
    let path = dir.join("blocking.json");
    let mut all = cx.json_or(&path, json!({}))?;
    let known: BTreeSet<String> = file(cx, &dir, "people.json").as_array().into_iter().flatten().filter_map(|p| opt_text(p, "name").map(str::to_owned)).collect();
    if let Some(positions) = args.get("positions").and_then(Value::as_object) {
        if let Some(stranger) = positions.keys().find(|n| !known.contains(*n)) {
            return Err(format!("no person {stranger:?} in this environment; it has {known:?}"));
        }
        let s = obj(&mut all)?.entry(song.clone()).or_insert_with(|| json!({}));
        let slot = obj(s)?.entry(section).or_insert_with(|| json!({}));
        merge(slot, &Value::Object(positions.clone()));
    }
    cx.write_json(&path, &all)?;
    let now = standing(cx, &dir, Some(&song), Some(section));
    if flag(args, "show") {
        let plain: Vec<Value> = now.iter().map(|p| { let mut p = p.clone(); if let Some(m) = p.as_object_mut() { m.remove("from"); } p }).collect();
        cx.write_json(&dir.join("people.json"), &json!(plain))?;
        sync_people(cx, &dir, &plain)?;
    }
    Ok(json!({ "song": song, "section": section, "people": now }))
}

/// Aims fixtures at a person, a focus point or a point: the pan and tilt
/// each fixture needs from where it hangs, and, with `song`, the focus
/// put on that section's (or `cue`'s) part for the target.
fn focus_on(cx: &mut Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let chans = r.resolve(args.get("target").ok_or("`target` is required")?)?;
    if chans.is_empty() {
        return Err("the target names no fixtures in this environment".to_owned());
    }
    let song = opt_text(args, "song");
    let section = opt_text(args, "section");
    let (point, name, by_name) = if let Some(person) = opt_text(args, "person") {
        let people = standing(cx, &r.dir, song, section);
        let p = people.iter().find(|p| opt_text(p, "name") == Some(person)).ok_or_else(|| {
            format!("no person {person:?}; the environment has {:?}", people.iter().filter_map(|p| opt_text(p, "name")).collect::<Vec<_>>())
        })?;
        // A person who moves for this section is aimed at by value; one
        // who stands where the room has them, by name (so moving them
        // later moves the light).
        (head(p), person.to_owned(), p.get("from").is_none())
    } else if let Some(focus) = opt_text(args, "focus") {
        let pal = cx.json_or(&r.dir.join("palettes.json"), json!({}))?;
        let bound = r.binding.focus.get(focus).cloned().unwrap_or_else(|| focus.to_owned());
        let p = list(&pal, "focus").iter().find(|f| opt_text(f, "name") == Some(bound.as_str())).and_then(|f| xyz(f.get("target")))
            .ok_or_else(|| format!("no focus point {focus:?}"))?;
        (p, focus.to_owned(), true)
    } else {
        (xyz(args.get("point")).ok_or("`person`, `focus` or `point` {x,y,z} is required")?, "point".to_owned(), false)
    };
    let mut rows = Vec::new();
    for chan in &chans {
        let Some(place) = r.rig.get(*chan).and_then(|f| f.placement.clone()) else {
            rows.push(json!({ "chan": chan, "error": "no known position" }));
            continue;
        };
        let (pan, tilt) = ignition_show::pan_tilt_deg_to_point(place.position, place.orientation, point);
        let d = ((point.x - place.position.x).powi(2) + (point.y - place.position.y).powi(2) + (point.z - place.position.z).powi(2)).sqrt();
        rows.push(json!({ "chan": chan, "pan_deg": pan, "tilt_deg": tilt, "throw_m": d }));
    }
    let mut wrote = Value::Null;
    if song.is_some() {
        let l = layers(cx, args)?;
        let (path, mut doc) = l.song.clone().ok_or("`song` is required")?;
        let roles = roles_of(cx, &l);
        let at = match (args.get("cue"), section) {
            (Some(cue), _) => cue.clone(),
            (None, Some(s)) => json!(s),
            _ => return Err("with `song`, name the `section` or `cue` the focus goes on".to_owned()),
        };
        let cues = arr(&mut doc, "cues")?;
        let i = cues
            .iter()
            .position(|c| c.get("name") == Some(&at) || c.get("at").and_then(|a| a.get("section")) == Some(&at))
            .ok_or_else(|| format!("no cue or section {at} in the song"))?;
        let cue = cues.get_mut(i).ok_or("no such cue")?;
        let mut spec = json!({ "target": args.get("target"), "focus": if by_name { json!(name) } else { vec_json(point) } });
        if let Some(d) = args.get("dimmer") {
            merge(&mut spec, &json!({ "dimmer": d }));
        }
        wrote = put_part(cue, &spec, &l, &roles)?;
        merge(&mut wrote, &json!({ "cue": cue.get("name") }));
        cx.write_json(&path, &doc)?;
    }
    Ok(json!({ "aimed_at": name, "point": vec_json(point), "fixtures": rows, "part": wrote }))
}

fn focus_set(cx: &mut Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let name = text(args, "name")?;
    let path = dir.join("palettes.json");
    let mut pal = cx.json_or(&path, json!({ "colors": [], "focus": [] }))?;
    let focus = arr(&mut pal, "focus")?;
    if flag(args, "remove") {
        focus.retain(|f| opt_text(f, "name") != Some(name));
    } else {
        let target = xyz(args.get("target")).ok_or("`target` {x,y,z} is required")?;
        focus.retain(|f| opt_text(f, "name") != Some(name));
        focus.push(json!({ "name": name, "target": vec_json(target) }));
    }
    let all = focus.clone();
    cx.write_json(&path, &pal)?;
    Ok(json!({ "focus": all }))
}

// ---------------------------------------------------------------------
// Fixtures, groups, roles
// ---------------------------------------------------------------------

fn mode_len(mode: &Value) -> usize {
    mode.as_array().map(Vec::len).or_else(|| mode.get("channels").and_then(Value::as_array).map(Vec::len)).unwrap_or(0)
}

fn types(cx: &Ctx) -> Vec<Value> {
    let dir = cx.data().join("fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| {
            let d = cx.json(p).ok()?;
            let modes: Map<String, Value> = d.get("modes")?.as_object()?.iter().filter(|(_, m)| mode_len(m) > 0).map(|(k, m)| (k.clone(), json!(mode_len(m)))).collect();
            Some(json!({
                "type": d.get("console_name"), "manufacturer": d.get("manufacturer"), "model": d.get("model"),
                "modes": modes, "beam_angle_deg": d.get("optics").and_then(|o| o.get("beam_angle_deg")),
                "moves": d.get("movement").is_some_and(|m| !m.is_null()),
                "size_mm": d.get("physical"), "file": p.file_name().and_then(|n| n.to_str()),
            }))
        })
        .collect()
}

fn find_type(cx: &Ctx, name: &str) -> Result<Value, String> {
    let all = types(cx);
    let want = name.to_lowercase();
    all.iter()
        .find(|t| opt_text(t, "type").is_some_and(|n| n.to_lowercase() == want))
        .or_else(|| all.iter().find(|t| [opt_text(t, "type"), opt_text(t, "model"), opt_text(t, "file")].iter().flatten().any(|n| n.to_lowercase().contains(&want))))
        .cloned()
        .ok_or_else(|| format!("no fixture type {name:?} (IGNITION_FIXTURE_TYPES lists them)"))
}

/// A fixture's footprint: its type's mode (the fixture's `mode`, else the
/// type's first), else what the room's `patch.json` says, else 1.
fn footprint(cx: &Ctx, dir: &Path, f: &Value, all: &[Value]) -> (u32, Option<String>) {
    let ty = opt_text(f, "model").and_then(|m| {
        let m = m.to_lowercase();
        all.iter().find(|t| opt_text(t, "type").is_some_and(|n| n.to_lowercase() == m))
    });
    if let Some(modes) = ty.and_then(|t| t.get("modes")).and_then(Value::as_object) {
        let pick = opt_text(f, "mode").and_then(|m| modes.get_key_value(m)).or_else(|| modes.iter().next());
        if let Some((name, n)) = pick {
            return (whole(n).unwrap_or(1), Some(name.clone()));
        }
    }
    let n = file(cx, dir, "patch.json")
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p.get("chan") == f.get("chan"))
        .and_then(|p| p.get("footprint").and_then(whole))
        .unwrap_or(1);
    (n, opt_text(f, "mode").map(str::to_owned))
}

fn rows(cx: &Ctx, r: &Room) -> Vec<Value> {
    let all = types(cx);
    r.fixtures
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            let (n, mode) = footprint(cx, &r.dir, f, &all);
            let chan = f.get("chan").and_then(whole);
            let address = f.get("address").and_then(whole);
            json!({
                "chan": chan, "name": f.get("name"), "label": f.get("label"), "type": f.get("model"), "mode": mode,
                "position": f.get("position"), "eulers": f.get("eulers"), "tags": f.get("tags"),
                "patched": f.get("patched").and_then(Value::as_bool).unwrap_or(true),
                "universe": f.get("universe"), "address": address, "footprint": n,
                "last": address.map(|a| a + n - 1),
                "groups": r.groups.iter().filter(|g| chan.is_some_and(|c| g.chans.contains(&c))).map(|g| g.name.clone()).collect::<Vec<_>>(),
            })
        })
        .collect()
}

fn fixture_list(cx: &Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let mut out = rows(cx, &r);
    if let Some(target) = args.get("target") {
        let chans = r.resolve(target)?;
        out.retain(|f| f.get("chan").and_then(whole).is_some_and(|c| chans.contains(&c)));
        out.sort_by_key(|f| chans.iter().position(|c| Some(*c) == f.get("chan").and_then(whole)));
    }
    Ok(json!({ "count": out.len(), "fixtures": out }))
}

/// Every overlap, overrun and loose end in a patch.
fn problems(rows: &[Value], manifest: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let span = |f: &Value| Some((f.get("universe").and_then(whole)?, f.get("address").and_then(whole)?, f.get("last").and_then(whole)?));
    let live: Vec<&Value> = rows.iter().filter(|f| f.get("patched") == Some(&json!(true))).collect();
    for (i, a) in live.iter().enumerate() {
        let Some((ua, a0, a1)) = span(a) else {
            out.push(json!({ "what": "unaddressed", "chan": a.get("chan"), "name": a.get("name") }));
            continue;
        };
        if a0 == 0 || a1 > 512 {
            out.push(json!({ "what": "out of range", "chan": a.get("chan"), "universe": ua, "address": a0, "last": a1 }));
        }
        for b in live.iter().skip(i + 1) {
            if let Some((ub, b0, b1)) = span(b) {
                if ua == ub && a0 <= b1 && b0 <= a1 {
                    out.push(json!({ "what": "overlap", "universe": ua, "chans": [a.get("chan"), b.get("chan")], "addresses": [[a0, a1], [b0, b1]] }));
                }
            }
        }
    }
    let routed = manifest.get("dmx").and_then(|d| d.get("universes")).and_then(Value::as_object);
    let used: BTreeSet<u32> = live.iter().filter_map(|f| f.get("universe").and_then(whole)).collect();
    for u in used {
        if routed.is_some_and(|r| !r.contains_key(&u.to_string())) {
            out.push(json!({ "what": "unrouted", "universe": u, "why": "fixtures are patched to it and it goes to no output (IGNITION_PATCH_ROUTE)" }));
        }
    }
    out
}

fn next_free(rows: &[Value], universe: u32, need: u32, from: u32) -> Option<u32> {
    let mut taken: Vec<(u32, u32)> = rows
        .iter()
        .filter(|f| f.get("universe").and_then(whole) == Some(universe))
        .filter_map(|f| Some((f.get("address").and_then(whole)?, f.get("last").and_then(whole)?)))
        .collect();
    taken.sort_unstable();
    let mut at = from.max(1);
    for (a, b) in taken {
        if at + need - 1 < a {
            break;
        }
        at = at.max(b + 1);
    }
    (at + need - 1 <= 512).then_some(at)
}

fn save_fixtures(cx: &mut Ctx, r: &Room) -> Result<(), String> {
    cx.write_json(&r.dir.join("fixtures.json"), &r.fixtures)
}

fn fixture_add(cx: &mut Ctx, args: &Value) -> Res {
    let mut r = room(cx, args)?;
    let ty = find_type(cx, text(args, "type")?)?;
    let modes = ty.get("modes").and_then(Value::as_object).cloned().unwrap_or_default();
    let mode = match opt_text(args, "mode") {
        Some(m) if modes.contains_key(m) => m.to_owned(),
        Some(m) => return Err(format!("{:?} has no mode {m:?}; it has {:?}", ty.get("type"), modes.keys().collect::<Vec<_>>())),
        None => modes.keys().next().cloned().ok_or("the type has no modes")?,
    };
    let need = modes.get(&mode).and_then(whole).unwrap_or(1);
    let count = args.get("count").and_then(whole).unwrap_or(1);
    let at = xyz(args.get("position")).ok_or("`position` {x,y,z} is required")?;
    let step = xyz(args.get("step")).unwrap_or(Vec3 { x: 0.0, y: 0.0, z: 0.0 });
    let eulers = xyz(args.get("eulers")).unwrap_or(Vec3 { x: 0.0, y: 0.0, z: 0.0 });
    let universe = args.get("universe").and_then(whole).unwrap_or(1);
    let base = opt_text(args, "name").or_else(|| opt_text(&ty, "type")).unwrap_or("Fixture").to_owned();
    let mut next_chan = args.get("chan").and_then(whole).unwrap_or_else(|| r.fixtures.as_array().into_iter().flatten().filter_map(|f| f.get("chan").and_then(whole)).max().map_or(1, |m| m + 1));
    let mut from = args.get("address").and_then(whole).unwrap_or(1);
    let size = ty.get("size_mm").cloned().unwrap_or_default();
    let mm = |key: &str, fallback: f64| num(&size, key).map_or(fallback, |v| v / 1000.0);
    let mut added = Vec::new();
    for i in 0..count {
        let current = rows(cx, &r);
        if current.iter().any(|f| f.get("chan").and_then(whole) == Some(next_chan)) {
            return Err(format!("channel {next_chan} is taken"));
        }
        let address = next_free(&current, universe, need, from).ok_or_else(|| format!("universe {universe} has no room for {need} more channels"))?;
        let k = f64::from(i);
        let here = Vec3 { x: step.x.mul_add(k, at.x), y: step.y.mul_add(k, at.y), z: step.z.mul_add(k, at.z) };
        let hang = match (args.get("quat"), xyz(args.get("aim"))) {
            (Some(q), _) => q.clone(),
            (None, Some(aim)) => quat_aim(here, aim),
            (None, None) => quat_of(eulers),
        };
        let record = json!({
            "chan": next_chan,
            "name": if count > 1 { format!("{base} {}", i + 1) } else { base.clone() },
            "tags": args.get("tags").cloned().unwrap_or_else(|| json!([])),
            "patched": true, "manufacturer": "", "model": ty.get("type"), "mode": mode,
            "position": { "x": step.x.mul_add(k, at.x), "y": step.y.mul_add(k, at.y), "z": step.z.mul_add(k, at.z) },
            "eulers": vec_json(eulers), "quat": hang,
            "size": { "x": mm("width_mm", 0.2), "y": mm("length_mm", 0.2), "z": mm("height_mm", 0.2) },
            "beam_angle_deg": ty.get("beam_angle_deg"),
            "universe": universe, "address": address, "global_address": (universe - 1) * 512 + address,
        });
        r.fixtures.as_array_mut().ok_or("fixtures.json is not a list")?.push(record);
        added.push(json!({ "chan": next_chan, "universe": universe, "address": address, "last": address + need - 1 }));
        from = address + need;
        next_chan += 1;
    }
    save_fixtures(cx, &r)?;
    let chans: Vec<u32> = added.iter().filter_map(|a| a.get("chan").and_then(whole)).collect();
    for g in list(args, "groups") {
        if let Some(name) = g.as_str() {
            group_put(cx, &r.dir, name, &chans, true)?;
        }
    }
    Ok(json!({ "type": ty.get("type"), "mode": mode, "footprint": need, "added": added }))
}

/// The fixtures an action names: `chan`, `chans`, or a `target`.
fn picked(r: &Room, args: &Value) -> Result<Vec<u32>, String> {
    if let Some(c) = args.get("chan").and_then(whole) {
        return Ok(vec![c]);
    }
    if let Some(many) = args.get("chans").and_then(Value::as_array) {
        return Ok(many.iter().filter_map(whole).collect());
    }
    r.resolve(args.get("target").ok_or("name the fixtures: `chan`, `chans` or `target`")?)
}

/// Moves, turns, renames or re-types fixtures. `position` places them,
/// `move` shifts them, `eulers` (degrees) or `quat` re-hangs them,
/// `mode` and `type` change what they are on the wire.
fn fixture_set(cx: &mut Ctx, args: &Value) -> Res {
    let mut r = room(cx, args)?;
    let chans = picked(&r, args)?;
    let ty = match opt_text(args, "type") {
        Some(t) => Some(find_type(cx, t)?),
        None => None,
    };
    let all_types = types(cx);
    let mut out = Vec::new();
    for f in r.fixtures.as_array_mut().ok_or("fixtures.json is not a list")? {
        let Some(chan) = f.get("chan").and_then(whole).filter(|c| chans.contains(c)) else { continue };
        let mut set = Map::new();
        for key in ["name", "label", "tags", "position", "quat", "beam_angle_deg", "gel"] {
            if let Some(v) = args.get(key) {
                set.insert(key.into(), v.clone());
            }
        }
        if let Some(e) = xyz(args.get("eulers")) {
            set.insert("eulers".into(), vec_json(e));
            set.insert("quat".into(), quat_of(e));
        }
        if let Some(by) = args.get("move") {
            let at = xyz(f.get("position")).unwrap_or(Vec3 { x: 0.0, y: 0.0, z: 0.0 });
            set.insert("position".into(), json!({ "x": at.x + num(by, "x").unwrap_or(0.0), "y": at.y + num(by, "y").unwrap_or(0.0), "z": at.z + num(by, "z").unwrap_or(0.0) }));
        }
        if let Some(aim) = xyz(args.get("aim")) {
            let from = xyz(set.get("position").or_else(|| f.get("position"))).unwrap_or(Vec3 { x: 0.0, y: 0.0, z: 0.0 });
            set.insert("quat".into(), quat_aim(from, aim));
        }
        if let Some(ty) = &ty {
            set.insert("model".into(), ty.get("type").cloned().unwrap_or_default());
            set.insert("mode".into(), Value::Null);
        }
        merge(f, &Value::Object(set));
        if let Some(mode) = opt_text(args, "mode") {
            let model = opt_text(f, "model").unwrap_or_default().to_lowercase();
            let modes = all_types.iter().find(|t| opt_text(t, "type").is_some_and(|n| n.to_lowercase() == model)).and_then(|t| t.get("modes")).and_then(Value::as_object);
            if !modes.is_some_and(|m| m.contains_key(mode)) {
                return Err(format!("channel {chan}'s type has no mode {mode:?}; it has {:?}", modes.map(|m| m.keys().collect::<Vec<_>>())));
            }
            obj(f)?.insert("mode".into(), json!(mode));
        }
        out.push(json!({ "chan": chan, "name": f.get("name"), "position": f.get("position"), "eulers": f.get("eulers"), "type": f.get("model"), "mode": f.get("mode") }));
    }
    if out.is_empty() {
        return Err("no such fixtures".to_owned());
    }
    save_fixtures(cx, &r)?;
    let manifest = cx.json_or(&r.dir.join("venue.ig-venue"), json!({}))?;
    let r = room(cx, args)?;
    Ok(json!({ "fixtures": out, "patch_problems": problems(&rows(cx, &r), &manifest) }))
}

fn fixture_remove(cx: &mut Ctx, args: &Value) -> Res {
    let mut r = room(cx, args)?;
    let chans = picked(&r, args)?;
    let items = r.fixtures.as_array_mut().ok_or("fixtures.json is not a list")?;
    let before = items.len();
    items.retain(|f| !f.get("chan").and_then(whole).is_some_and(|c| chans.contains(&c)));
    let removed = before - items.len();
    save_fixtures(cx, &r)?;
    let path = r.dir.join("groups.json");
    let mut groups = cx.json_or(&path, json!([]))?;
    for g in groups.as_array_mut().into_iter().flatten() {
        let kept: Vec<Value> = chans_of(list(g, "channels")).into_iter().filter(|c| !chans.contains(c)).map(|c| json!(c.to_string())).collect();
        obj(g)?.insert("channels".into(), json!(kept));
    }
    cx.write_json(&path, &groups)?;
    Ok(json!({ "removed": removed, "chans": chans }))
}

fn group_put(cx: &mut Ctx, dir: &Path, name: &str, chans: &[u32], add: bool) -> Result<Vec<u32>, String> {
    let path = dir.join("groups.json");
    let mut groups = cx.json_or(&path, json!([]))?;
    let items = groups.as_array_mut().ok_or("groups.json is not a list")?;
    if !items.iter().any(|g| opt_text(g, "label") == Some(name)) {
        let id = items.len() + 1;
        items.push(json!({ "target": id.to_string(), "label": name, "channels": [] }));
    }
    let g = items.iter_mut().find(|g| opt_text(g, "label") == Some(name)).ok_or("no such group")?;
    let mut now = if add { chans_of(list(g, "channels")) } else { Vec::new() };
    for c in chans {
        if !now.contains(c) {
            now.push(*c);
        }
    }
    obj(g)?.insert("channels".into(), json!(now.iter().map(u32::to_string).collect::<Vec<_>>()));
    cx.write_json(&path, &groups)?;
    Ok(now)
}

fn group_set(cx: &mut Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let name = text(args, "name")?;
    if flag(args, "delete") {
        let path = r.dir.join("groups.json");
        let mut groups = cx.json_or(&path, json!([]))?;
        if let Some(items) = groups.as_array_mut() {
            items.retain(|g| opt_text(g, "label") != Some(name));
        }
        cx.write_json(&path, &groups)?;
        return Ok(json!({ "deleted": name }));
    }
    let chans = picked(&r, args)?;
    let now = group_put(cx, &r.dir, name, &chans, flag(args, "add"))?;
    Ok(json!({ "group": name, "chans": now }))
}

fn group_list(cx: &Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let roles: Map<String, Value> = r.binding.groups.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    Ok(json!({
        "groups": r.groups.iter().map(|g| json!({ "name": g.name, "chans": g.chans })).collect::<Vec<_>>(),
        "roles": roles, "focus_roles": r.binding.focus,
    }))
}

/// Binds a role of the profile to fixtures of this room: `to` a group's
/// name or a selection (`null` unbinds); `focus` binds a focus role to a
/// focus point's name.
fn role_bind(cx: &mut Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let role = text(args, "role")?;
    let path = r.dir.join("profile.json");
    let mut doc = cx.json_or(&path, json!({ "profile": "Ignition", "groups": {}, "focus": {}, "canvases": {} }))?;
    if let Some(point) = args.get("focus") {
        let map = obj(&mut doc)?.entry("focus").or_insert_with(|| json!({}));
        if point.is_null() { obj(map)?.remove(role); } else { obj(map)?.insert(role.into(), point.clone()); }
    } else {
        let to = args.get("to").ok_or("`to` (a group or a selection) or `focus` is required")?;
        let map = obj(&mut doc)?.entry("groups").or_insert_with(|| json!({}));
        if to.is_null() {
            obj(map)?.remove(role);
        } else {
            let sel = match to.as_str() {
                Some(group) if r.groups.iter().any(|g| g.name == group) => json!({ "Group": group }),
                Some(group) => return Err(format!("no group {group:?} in this environment")),
                None => to.clone(),
            };
            serde_json::from_value::<Selection>(sel.clone()).map_err(|e| format!("`to`: {e}"))?;
            obj(map)?.insert(role.into(), sel);
        }
    }
    cx.write_json(&path, &doc)?;
    Ok(json!({ "role": role, "binding": doc }))
}

// ---------------------------------------------------------------------
// The patch
// ---------------------------------------------------------------------

fn patch_get(cx: &Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let manifest = cx.json_or(&r.dir.join("venue.ig-venue"), json!({}))?;
    let all = rows(cx, &r);
    let mut universes: BTreeMap<u32, (u32, Vec<Value>)> = BTreeMap::new();
    for f in &all {
        if let (Some(u), Some(n)) = (f.get("universe").and_then(whole), f.get("footprint").and_then(whole)) {
            let slot = universes.entry(u).or_default();
            slot.0 += n;
            slot.1.push(json!([f.get("address"), f.get("last"), f.get("chan")]));
        }
    }
    let routes = manifest.get("dmx").and_then(|d| d.get("universes")).cloned().unwrap_or_else(|| json!({}));
    let unis: Vec<Value> = universes
        .iter()
        .map(|(u, (used, spans))| json!({ "universe": u, "used": used, "free": 512_u32.saturating_sub(*used), "next_free": next_free(&all, *u, 1, 1), "output": routes.get(u.to_string()), "spans": spans }))
        .collect();
    let patch: Vec<Value> = all.iter().map(|f| json!({ "chan": f.get("chan"), "name": f.get("name"), "type": f.get("type"), "mode": f.get("mode"), "patched": f.get("patched"), "universe": f.get("universe"), "address": f.get("address"), "footprint": f.get("footprint"), "last": f.get("last") })).collect();
    let found = problems(&all, &manifest);
    Ok(json!({ "ok": found.is_empty(), "patch": patch, "universes": unis, "routes": routes, "problems": found }))
}

/// Re-addresses fixtures: one (`chan`, `universe`, `address`) or several
/// (`moves`). An address that would overlap another fixture is refused
/// with the overlap, unless `force`. `patched: false` parks a fixture.
fn patch_set(cx: &mut Ctx, args: &Value) -> Res {
    let mut r = room(cx, args)?;
    let moves: Vec<Value> = match args.get("moves").and_then(Value::as_array) {
        Some(m) => m.clone(),
        None => vec![args.clone()],
    };
    let mut out = Vec::new();
    for m in &moves {
        let chan = m.get("chan").and_then(whole).ok_or("each move names a `chan`")?;
        let f = r.fixtures.as_array_mut().into_iter().flatten().find(|f| f.get("chan").and_then(whole) == Some(chan)).ok_or_else(|| format!("no fixture on channel {chan}"))?;
        let was = json!([f.get("universe"), f.get("address")]);
        let universe = m.get("universe").and_then(whole).or_else(|| f.get("universe").and_then(whole)).unwrap_or(1);
        let map = obj(f)?;
        if let Some(p) = m.get("patched").and_then(Value::as_bool) {
            map.insert("patched".into(), json!(p));
        }
        if let Some(address) = m.get("address").and_then(whole) {
            if address == 0 || address > 512 {
                return Err(format!("address {address}: 1 to 512"));
            }
            map.insert("universe".into(), json!(universe));
            map.insert("address".into(), json!(address));
            map.insert("global_address".into(), json!((universe.max(1) - 1) * 512 + address));
        }
        out.push(json!({ "chan": chan, "was": was, "now": [map.get("universe"), map.get("address")] }));
    }
    let manifest = cx.json_or(&r.dir.join("venue.ig-venue"), json!({}))?;
    let found = problems(&rows(cx, &r), &manifest);
    let blocking: Vec<&Value> = found.iter().filter(|p| matches!(opt_text(p, "what"), Some("overlap" | "out of range"))).collect();
    if !blocking.is_empty() && !flag(args, "force") {
        return Err(format!("the patch would conflict, so nothing was changed: {}", json!(blocking)));
    }
    save_fixtures(cx, &r)?;
    // The room's patch sheet, where it keeps one, says the same.
    let sheet = r.dir.join("patch.json");
    if cx.exists(&sheet) {
        let mut doc = cx.json(&sheet)?;
        for p in doc.as_array_mut().into_iter().flatten() {
            if let Some(f) = r.fixtures.as_array().into_iter().flatten().find(|f| f.get("chan") == p.get("chan")) {
                merge(p, &json!({ "universe": f.get("universe"), "address": f.get("address"), "patched": f.get("patched") }));
            }
        }
        cx.write_json(&sheet, &doc)?;
    }
    Ok(json!({ "moved": out, "problems": found }))
}

/// Packs fixtures end to end from `address` in `universe`, in the
/// target's order.
fn patch_pack(cx: &mut Ctx, args: &Value) -> Res {
    let r = room(cx, args)?;
    let chans = picked(&r, args)?;
    let universe = args.get("universe").and_then(whole).unwrap_or(1);
    let mut at = args.get("address").and_then(whole).unwrap_or(1);
    let all = rows(cx, &r);
    let mut moves = Vec::new();
    for c in &chans {
        let n = all.iter().find(|f| f.get("chan").and_then(whole) == Some(*c)).and_then(|f| f.get("footprint").and_then(whole)).unwrap_or(1);
        moves.push(json!({ "chan": c, "universe": universe, "address": at }));
        at += n;
    }
    let mut a = args.clone();
    obj(&mut a)?.insert("moves".into(), json!(moves));
    patch_set(cx, &a)
}

/// Where a universe goes on the wire: `sacn` (`{priority, universe?}`),
/// `artnet` (`{net, subnet, universe}`), `enabled`; `null` drops a
/// protocol, `remove: true` the universe's route.
fn patch_route(cx: &mut Ctx, args: &Value) -> Res {
    let dir = env_dir(cx, args)?;
    let universe = args.get("universe").and_then(whole).ok_or("`universe` is required")?;
    let path = dir.join("venue.ig-venue");
    let mut doc = cx.json_or(&path, json!({ "version": 1, "profile": "Ignition" }))?;
    let dmx = obj(&mut doc)?.entry("dmx").or_insert_with(|| json!({}));
    let unis = obj(dmx)?.entry("universes").or_insert_with(|| json!({}));
    let unis = obj(unis)?;
    if flag(args, "remove") {
        unis.remove(&universe.to_string());
    } else {
        let slot = unis.entry(universe.to_string()).or_insert_with(|| json!({}));
        let mut set = Map::new();
        for key in ["sacn", "artnet", "enabled", "max_hz", "keepalive_hz"] {
            if let Some(v) = args.get(key) {
                set.insert(key.into(), v.clone());
            }
        }
        merge(slot, &Value::Object(set));
    }
    let routes = Value::Object(unis.clone());
    cx.write_json(&path, &doc)?;
    Ok(json!({ "universe": universe, "routes": routes }))
}

pub(crate) fn run(cx: &mut Ctx, id: &str, args: &Value) -> Option<Res> {
    Some(match id {
        "IGNITION_ENV_LIST" => env_list(cx, args),
        "IGNITION_ENV_GET" => env_get(cx, args),
        "IGNITION_ENV_CREATE" => env_create(cx, args),
        "IGNITION_ENV_DUPLICATE" => env_duplicate(cx, args),
        "IGNITION_ENV_USE" => env_use(cx, args),
        "IGNITION_ENV_PIECE_SET" => piece_set(cx, args),
        "IGNITION_ENV_PIECE_REMOVE" => piece_remove(cx, args),
        "IGNITION_ENV_LIGHTS" => env_lights(cx, args),
        "IGNITION_ENV_LOOK" => env_look(cx, args),
        "IGNITION_PERSON_SET" => person_set(cx, args),
        "IGNITION_PERSON_REMOVE" => person_remove(cx, args),
        "IGNITION_PEOPLE_LIST" => people_list(cx, args),
        "IGNITION_BLOCKING_SET" => blocking_set(cx, args),
        "IGNITION_FOCUS_ON" => focus_on(cx, args),
        "IGNITION_FOCUS_SET" => focus_set(cx, args),
        "IGNITION_FIXTURE_TYPES" => {
            let search = opt_text(args, "search").map(str::to_lowercase);
            let all: Vec<Value> = types(cx).into_iter().filter(|t| search.as_ref().is_none_or(|s| t.to_string().to_lowercase().contains(s))).collect();
            Ok(json!({ "count": count_f(all.len()), "types": all }))
        }
        "IGNITION_FIXTURE_LIST" => fixture_list(cx, args),
        "IGNITION_FIXTURE_ADD" => fixture_add(cx, args),
        "IGNITION_FIXTURE_SET" => fixture_set(cx, args),
        "IGNITION_FIXTURE_REMOVE" => fixture_remove(cx, args),
        "IGNITION_GROUP_LIST" => group_list(cx, args),
        "IGNITION_GROUP_SET" => group_set(cx, args),
        "IGNITION_ROLE_BIND" => role_bind(cx, args),
        "IGNITION_PATCH_GET" => patch_get(cx, args),
        "IGNITION_PATCH_SET" => patch_set(cx, args),
        "IGNITION_PATCH_PACK" => patch_pack(cx, args),
        "IGNITION_PATCH_ROUTE" => patch_route(cx, args),
        _ => return None,
    })
}
