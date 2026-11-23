//! The catalogue against the code: every listed action is one `run`
//! knows, and the manifest lists them all.

use serde_json::json;

use super::{ids, run};

#[test]
fn every_action_of_the_catalogue_is_run_and_in_the_manifest() {
    let manifest = include_str!("../../extension.toml");
    let dir = std::env::temp_dir().join(format!("ignition-program-{}", std::process::id()));
    for id in ids() {
        assert!(
            manifest.contains(&format!("id = \"{id}\"")),
            "{id} is not in extension.toml: run tools/program_actions.py"
        );
        let said = run(&id, &json!({ "root": dir })).err().unwrap_or_default();
        assert!(!said.starts_with("no action"), "{id} is in the catalogue and nothing runs it");
    }
    assert!(run("IGNITION_NOPE", &json!({})).is_err());
}

/// An open edit is the process's (a script's run), so tests that write
/// take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn root(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("ignition-program-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fixtures = dir.join("data/fixtures");
    std::fs::create_dir_all(&fixtures).unwrap_or_default();
    std::fs::write(
        fixtures.join("par.json"),
        r#"{"console_name":"Test Par","modes":{"4ch":["r","g","b","dim"]}}"#,
    )
    .unwrap_or_default();
    dir
}

#[test]
fn a_room_is_made_patched_and_a_conflict_is_refused_whole() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = root("room");
    let a = |extra: serde_json::Value| {
        let mut v = json!({ "root": dir, "env": "hall" });
        super::merge(&mut v, &extra);
        v
    };
    assert!(run("IGNITION_ENV_CREATE", &json!({ "root": dir, "name": "Hall", "width": 10.0, "depth": 8.0 })).is_ok());
    let added = run(
        "IGNITION_FIXTURE_ADD",
        &a(json!({ "type": "Test Par", "count": 3, "position": { "x": 0.0, "y": 0.0, "z": 3.0 }, "groups": ["Pars"] })),
    )
    .unwrap_or_default();
    assert_eq!(added["added"][2]["address"], 9, "three four-channel fixtures pack end to end");
    // On top of fixture 1: refused, and nothing moved.
    assert!(run("IGNITION_PATCH_SET", &a(json!({ "chan": 2, "address": 3 }))).is_err());
    let patch = run("IGNITION_PATCH_GET", &a(json!({}))).unwrap_or_default();
    assert_eq!(patch["patch"][1]["address"], 5);
    assert_eq!(patch["ok"], true);
    // Moved clear of the others: taken, and undone as one step.
    assert!(run("IGNITION_PATCH_SET", &a(json!({ "chan": 2, "address": 100 }))).is_ok());
    assert!(run("IGNITION_UNDO", &json!({ "root": dir })).is_ok());
    let patch = run("IGNITION_PATCH_GET", &a(json!({}))).unwrap_or_default();
    assert_eq!(patch["patch"][1]["address"], 5);
    // A person is a focus point at head height, and fixtures aim at them.
    assert!(run("IGNITION_PERSON_SET", &a(json!({ "name": "Singer", "position": { "x": 1.0, "y": 0.0, "z": 0.0 } }))).is_ok());
    let aim = run("IGNITION_FOCUS_ON", &a(json!({ "target": "Pars", "person": "Singer" }))).unwrap_or_default();
    assert_eq!(aim["fixtures"].as_array().map(Vec::len), Some(3));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_song_is_programmed_by_section_and_reskinned_by_palette() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = root("song");
    let a = |extra: serde_json::Value| {
        let mut v = json!({ "root": dir, "show": "t", "song": "One" });
        super::merge(&mut v, &extra);
        v
    };
    let sections = json!([{ "name": "Verse", "bars": 8 }, { "name": "Chorus", "bars": 8 }, { "name": "Chorus", "bars": 4 }]);
    assert!(run("IGNITION_SONG_CREATE", &json!({ "root": dir, "show": "t", "title": "One", "sections": sections })).is_ok());
    let lib = |kind: &str, name: &str, entry: serde_json::Value| {
        run("IGNITION_LIBRARY_CREATE", &json!({ "root": dir, "show": "t", "kind": kind, "name": name, "entry": entry }))
    };
    assert!(lib("colors", "Ember", json!({ "red": 1.0, "green": 0.3, "blue": 0.0 })).is_ok());
    assert!(lib("colors", "Frost", json!({ "red": 0.3, "green": 0.7, "blue": 1.0 })).is_ok());
    assert!(lib("palettes", "Warm", json!({ "slots": { "Primary": "Ember" } })).is_ok());
    assert!(lib("palettes", "Cold", json!({ "slots": { "Primary": "Frost" } })).is_ok());
    assert!(run("IGNITION_PALETTE_USE", &json!({ "root": dir, "show": "t", "palette": "Warm" })).is_ok());
    let plan = run(
        "IGNITION_FILL_SECTIONS",
        &a(json!({ "looks": { "*": { "target": { "Group": "All" }, "dimmer": 0.5, "color": "Primary" } } })),
    )
    .unwrap_or_default();
    assert_eq!(plan["plan"].as_array().map(Vec::len), Some(3));
    // A cue by section, two bars in, and by the second occurrence.
    let cue = run("IGNITION_CUE_ADD", &a(json!({ "name": "lift", "at": "Chorus#2+2" }))).unwrap_or_default();
    assert_eq!(cue["cue"]["bar"], 19, "the second chorus opens at bar 17");
    let check = run("IGNITION_SET_CHECK", &json!({ "root": dir, "show": "t" })).unwrap_or_default();
    assert_eq!(check["problems"], 3, "the group of each section cue is in no room: {check}");
    // The re-skin: only the palette's name changes.
    let used = run("IGNITION_SET_EACH", &json!({ "root": dir, "show": "t", "action": "IGNITION_PALETTE_USE", "args": { "palette": "Cold" } })).unwrap_or_default();
    assert_eq!(used["results"][0]["result"]["slots"][0]["blue"], 1.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_measured_rooms_objects_move_and_its_architecture_stays() {
    let _turn = TURN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = root("measured");
    let env = dir.join("data/venues/lab");
    std::fs::create_dir_all(&env).unwrap_or_default();
    std::fs::write(env.join("fixtures.json"), "[]").unwrap_or_default();
    std::fs::write(
        env.join("environment.json"),
        r#"{"name":"Lab","model":"lab.glb","note_kept":7,
            "light_groups":[{"name":"house","label":"House","on":false}],
            "lights":[{"name":"L1","group":"house"}],
            "objects":[{"name":"Floor","kind":"architecture","position":{"x":0,"y":0,"z":0},"visible":true,"movable":false},
                       {"name":"Piano","kind":"gear","position":{"x":1,"y":2,"z":0},"visible":true,"movable":true}]}"#,
    )
    .unwrap_or_default();
    let a = |extra: serde_json::Value| {
        let mut v = json!({ "root": dir, "env": "lab", "in": "environment" });
        super::merge(&mut v, &extra);
        v
    };
    assert!(run("IGNITION_ENV_PIECE_SET", &a(json!({ "name": "Floor", "move": { "x": 1.0 } }))).is_err());
    assert!(run("IGNITION_ENV_PIECE_SET", &a(json!({ "name": "Sofa", "visible": false }))).is_err(), "not in the model");
    let moved = run("IGNITION_ENV_PIECE_SET", &a(json!({ "name": "Piano", "move": { "x": 0.5 }, "visible": false }))).unwrap_or_default();
    assert_eq!(moved["object"]["position"]["x"], 1.5);
    assert_eq!(moved["object"]["visible"], false);
    let lights = run("IGNITION_ENV_LIGHTS", &json!({ "root": dir, "env": "lab", "group": "house", "on": true })).unwrap_or_default();
    assert_eq!(lights["light_groups"][0]["on"], true);
    assert_eq!(lights["light_groups"][0]["lights"], 1);
    let got = run("IGNITION_ENV_GET", &json!({ "root": dir, "env": "lab" })).unwrap_or_default();
    assert_eq!(got["measured"]["objects"].as_array().map(Vec::len), Some(2));
    // The generated room is not written: the edits are beside it.
    let raw = std::fs::read_to_string(env.join("environment.json")).unwrap_or_default();
    assert!(raw.contains("note_kept") && raw.contains(r#""x":1,"y":2"#), "the room's own file is as it was");
    let edits = std::fs::read_to_string(env.join("environment.edits.json")).unwrap_or_default();
    assert!(edits.contains("Piano") && edits.contains("house"));
    let _ = std::fs::remove_dir_all(&dir);
}
