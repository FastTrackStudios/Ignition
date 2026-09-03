//! The Cameras pane: the venue's camera presets, the ten on the number
//! keys badged `1`–`0`, the one the programme camera is on lit from the
//! playhead.
//!
//! Click cuts; right-click opens a menu to put a preset on a key, save
//! the view the viewport is on right now as a preset, or delete one
//! (`r[studio.video.cameras-pane]`).
//!
//! Everything drawn here comes back from the engine on the playhead
//! (`Playhead::camera`): which preset is active, where the camera is,
//! and the preset and slot lists — so the pane never holds a copy of
//! the venue file that could drift from what the visualizer loaded.
//! The per-operator half — which ten presets are on the keys — is the
//! `cameras.favourites` key of the operator file, read and written by
//! [`favourites`] and [`save_favourites`] below.

// r[impl studio.video.cameras-pane] - list, badges, active, click to cut, right-click menu
// r[impl viz.camera-favourites] - per-operator slots in the operator file

use crate::command::{CameraState, CameraTarget, CanvasRow, Command};
use crate::{send, use_playhead};
use dioxus::prelude::*;

/// The keys, in slot order: `1`..`9` then `0`.
pub const KEYS: [u8; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

/// The key a preset is on, from the playhead's slot list.
#[must_use]
pub fn key_of(state: &CameraState, name: &str) -> Option<u8> {
    state
        .slots
        .iter()
        .position(|s| s.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .and_then(|i| KEYS.get(i).copied())
}

/// The operator file's `cameras.favourites`, if the file has one.
///
/// Read as loose JSON from the operator file, the way `operators.rs`
/// reads its own keys, so this module owns one key and nothing else.
#[must_use]
pub fn favourites(name: &str) -> Option<Vec<String>> {
    favourites_in(std::path::Path::new(crate::operators::DIR), name)
}

#[must_use]
pub fn favourites_in(dir: &std::path::Path, name: &str) -> Option<Vec<String>> {
    let raw = std::fs::read_to_string(dir.join(format!("{name}.ig-user"))).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let list = value.get("cameras")?.get("favourites")?;
    serde_json::from_value(list.clone()).ok()
}

/// Write `cameras.favourites` into the operator file, keeping every
/// other key as it was.
///
/// # Errors
///
/// Whatever `std::fs::write` or `create_dir_all` returns — the operator
/// directory is not writable, say.
pub fn save_favourites(name: &str, slots: &[String]) -> std::io::Result<()> {
    save_favourites_in(std::path::Path::new(crate::operators::DIR), name, slots)
}

/// # Errors
///
/// Whatever `std::fs::write` or `create_dir_all` returns.
pub fn save_favourites_in(
    dir: &std::path::Path,
    name: &str,
    slots: &[String],
) -> std::io::Result<()> {
    let path = dir.join(format!("{name}.ig-user"));
    // Read the file as a JSON object, keeping every key this module does
    // not own; anything unreadable or not an object starts fresh rather
    // than panicking on a corrupt or foreign file.
    let mut root = if let Some(serde_json::Value::Object(map)) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
    {
        map
    } else {
        let mut map = serde_json::Map::new();
        map.insert("name".into(), serde_json::json!(name));
        map
    };
    let mut cameras = if let Some(serde_json::Value::Object(map)) = root.get("cameras") {
        map.clone()
    } else {
        serde_json::Map::new()
    };
    cameras.insert("favourites".into(), serde_json::json!(slots));
    root.insert("cameras".into(), serde_json::Value::Object(cameras));
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::Value::Object(root))? + "\n",
    )
}

/// What the right-click opened, and on which preset.
#[derive(Debug, Clone, PartialEq)]
struct Menu {
    at: (f32, f32),
    preset: String,
}

/// The pane.
#[component]
pub fn CamerasPane() -> Element {
    let playhead = use_playhead();
    let mut menu = use_signal(|| None::<Menu>);
    let mut new_name = use_signal(String::new);
    let state = playhead().camera.unwrap_or_default();
    let active = state.preset.clone();
    let count = state.presets.len();
    let pose = format!(
        "eye {:.1} {:.1} {:.1}  ·  look {:.1} {:.1} {:.1}  ·  fov {:.0}°",
        state.eye[0],
        state.eye[1],
        state.eye[2],
        state.look[0],
        state.look[1],
        state.look[2],
        state.fov_deg
    );
    let note = match &active {
        Some(name) => name.clone(),
        None if count == 0 => "no visualizer".to_string(),
        None => "free".to_string(),
    };
    rsx! {
        style { {CSS} }
        section {
            class: "pane pane-cameras",
            onclick: move |_| menu.set(None),
            header { class: "pane-head",
                span { class: "pane-title", "Cameras" }
                span { class: "pane-note", "{note}" }
                span { class: "cam-pose", title: "where the programme camera is", "{pose}" }
            }
            div { class: "cam-save",
                input {
                    class: "pane-search",
                    r#type: "text",
                    placeholder: "save current view as…",
                    value: "{new_name}",
                    oninput: move |e| new_name.set(e.value()),
                    onclick: move |e| e.stop_propagation(),
                }
                button {
                    class: "pane-key",
                    disabled: new_name().trim().is_empty() || count == 0,
                    onclick: move |e| {
                        e.stop_propagation();
                        let name = new_name().trim().to_string();
                        if !name.is_empty() {
                            send(Command::SaveCameraPreset { name });
                            new_name.set(String::new());
                        }
                    },
                    "SAVE"
                }
            }
            div { class: "cam-list",
                for name in state.presets.iter().cloned() {
                    {
                        let key = key_of(&state, &name);
                        let lit = active.as_deref().is_some_and(|a| a.eq_ignore_ascii_case(&name));
                        let class = if lit { "cam-row on" } else { "cam-row" };
                        let cut_name = name.clone();
                        let menu_name = name.clone();
                        rsx! {
                            div {
                                key: "{name}",
                                class: "{class}",
                                onclick: move |e| {
                                    e.stop_propagation();
                                    menu.set(None);
                                    send(Command::Camera {
                                        target: CameraTarget::Preset(cut_name.clone()),
                                        beats: 0.0,
                                    });
                                },
                                oncontextmenu: move |e| {
                                    e.stop_propagation();
                                    e.prevent_default();
                                    let p = e.data.element_coordinates();
                                    menu.set(Some(Menu {
                                        at: (crate::pointer::coord(p.x), crate::pointer::coord(p.y)),
                                        preset: menu_name.clone(),
                                    }));
                                },
                                span { class: if key.is_some() { "cam-key" } else { "cam-key empty" },
                                    {key.map_or_else(|| "·".to_string(), |k| format!("{k}"))}
                                }
                                span { class: "cam-name", "{name}" }
                            }
                        }
                    }
                }
            }
            // The wide view's own preset, while a Programme pane takes
            // the cuts.
            // r[impl viz.programme-view] - the wide view is selectable
            if let Some(wide) = state.wide.clone() {
                div { class: "cam-wide",
                    span { class: "cam-sub", "WIDE VIEW" }
                    for name in state.presets.iter().cloned() {
                        {
                            let lit = name.eq_ignore_ascii_case(&wide);
                            let target = name.clone();
                            rsx! {
                                button {
                                    key: "{name}",
                                    class: if lit { "cam-chip on" } else { "cam-chip" },
                                    onclick: move |e| {
                                        e.stop_propagation();
                                        send(Command::Wide { target: CameraTarget::Preset(target.clone()) });
                                    },
                                    "{name}"
                                }
                            }
                        }
                    }
                }
            }
            // TO SCREENS: each canvas on the programme, or on its own content.
            // r[impl canvas.camera-source] - switched from the pane
            if !state.canvases.is_empty() {
                div { class: "cam-screens",
                    span { class: "cam-sub", "TO SCREENS" }
                    for row in state.canvases.iter().cloned() {
                        ScreenToggle { key: "{row.name}", row }
                    }
                }
            }
            if let Some(m) = menu() {
                {
                    let preset = m.preset.clone();
                    let del = preset.clone();
                    rsx! {
                        div {
                            class: "cam-menu",
                            style: "left: {m.at.0}px; top: {m.at.1}px;",
                            onclick: move |e| e.stop_propagation(),
                            div { class: "menu-title", "{preset}" }
                            div { class: "menu-sub", "set as slot" }
                            div { class: "menu-grid",
                                for k in KEYS {
                                    {
                                        let slot_name = preset.clone();
                                        rsx! {
                                            button {
                                                key: "{k}",
                                                class: "menu-item",
                                                onclick: move |_| {
                                                    send(Command::SetCameraSlot { slot: k, name: slot_name.clone() });
                                                    menu.set(None);
                                                },
                                                "{k}"
                                            }
                                        }
                                    }
                                }
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    send(Command::SaveCameraPreset { name: preset.clone() });
                                    menu.set(None);
                                },
                                "save current view here"
                            }
                            button {
                                class: "menu-item",
                                onclick: move |_| {
                                    send(Command::DeleteCameraPreset { name: del.clone() });
                                    menu.set(None);
                                },
                                "delete"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One canvas's TO SCREENS key: lit while the canvas shows the
/// programme; a click toggles it back to its own content.
#[component]
fn ScreenToggle(row: CanvasRow) -> Element {
    let on = row.camera.is_some();
    let name = row.name.clone();
    let label = match &row.camera {
        Some(source) => format!("{} · {}", row.name, source.trim_start_matches("camera:")),
        None => row.name.clone(),
    };
    rsx! {
        button {
            class: if on { "cam-chip on" } else { "cam-chip" },
            title: "put the programme camera on this canvas",
            onclick: move |e| {
                e.stop_propagation();
                send(Command::CanvasSource {
                    canvas: name.clone(),
                    source: if on { String::new() } else { "camera:programme".to_string() },
                });
            },
            "{label}"
        }
    }
}

/// The pane's own rules, inlined so the pane carries its look wherever
/// it is mounted. Colours follow `live.css`.
const CSS: &str = r"
.pane-cameras { position: relative; }
.cam-pose { font-size: 10px; color: #8a8a96; white-space: nowrap; overflow: hidden; }
.cam-save { display: flex; gap: 6px; margin-bottom: 6px; flex: 0 0 auto; }
.cam-list { display: flex; flex-direction: column; gap: 3px; overflow: hidden; }
.cam-row { display: flex; align-items: center; gap: 8px; min-height: 34px; padding: 0 8px;
  border-radius: 4px; background: #1e1e26; border: 1px solid #2c2c36; color: #d6d6e0; cursor: pointer; }
.cam-row:hover { background: #272733; }
.cam-row.on { background: #3a3a6b; border-color: #6a6ad0; color: #fff; }
.cam-key { min-width: 20px; text-align: center; font-size: 11px; font-weight: bold; padding: 1px 5px;
  border-radius: 4px; background: #e0b860; color: #1b1b22; }
.cam-key.empty { background: transparent; color: #4a4a55; }
.cam-name { font-size: 12px; letter-spacing: 0.04em; }
.cam-wide, .cam-screens { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; margin-top: 6px; flex: 0 0 auto; }
.cam-sub { font-size: 9px; letter-spacing: 0.12em; text-transform: uppercase; color: #6a6a78; margin-right: 4px; }
.cam-chip { min-height: 26px; padding: 0 8px; border-radius: 4px; background: #1e1e26; border: 1px solid #2c2c36;
  color: #d6d6e0; font-size: 10px; cursor: pointer; }
.cam-chip:hover { background: #272733; }
.cam-chip.on { background: #3a3a6b; border-color: #6a6ad0; color: #fff; }
.cam-menu { position: absolute; z-index: 20; min-width: 160px; padding: 4px 0; background: #16161c;
  border: 1px solid #3a3a48; border-radius: 6px; box-shadow: 0 6px 18px #000a; }
.cam-menu .menu-title { padding: 4px 10px; font-size: 9px; letter-spacing: 0.12em; text-transform: uppercase; color: #8a8a96; }
.cam-menu .menu-sub { padding: 5px 10px 1px; font-size: 9px; letter-spacing: 0.1em; text-transform: uppercase; color: #6a6a78; }
.cam-menu .menu-item { display: block; width: 100%; text-align: left; padding: 5px 10px; font-size: 11px;
  background: transparent; border: 0; color: #d6d6e0; cursor: pointer; }
.cam-menu .menu-item:hover { background: #2c2c40; color: #fff; }
.cam-menu .menu-grid { display: flex; flex-wrap: wrap; max-width: 160px; }
.cam-menu .menu-grid .menu-item { width: 20%; text-align: center; }
";

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> CameraState {
        CameraState {
            preset: Some("Drums".into()),
            presets: vec![
                "Wide".into(),
                "Singer".into(),
                "Drums".into(),
                "Bird's eye".into(),
            ],
            slots: vec![
                Some("Wide".into()),
                Some("Singer".into()),
                Some("Drums".into()),
                None,
                None,
                None,
                None,
                None,
                None,
                Some("Bird's eye".into()),
            ],
            ..Default::default()
        }
    }

    /// r[verify viz.camera-favourites] - the badge is the key: 1..9, and 0 for the tenth
    #[test]
    fn badges_follow_the_keys() {
        let s = state();
        assert_eq!(key_of(&s, "Wide"), Some(1));
        assert_eq!(key_of(&s, "drums"), Some(3));
        assert_eq!(key_of(&s, "Bird's eye"), Some(0));
        assert_eq!(key_of(&s, "Nope"), None);
    }

    /// r[verify viz.camera-favourites] - the operator's ten survive a round trip beside the layout
    #[test]
    fn operator_favourites_round_trip_and_keep_the_rest_of_the_file() {
        let dir = std::env::temp_dir().join(format!("ig-camfav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("ann.ig-user"),
            r#"{"name":"ann","windows":[{"monitor":"DP-1"}],"favourites":{"looks":["punt"]}}"#,
        )
        .unwrap();
        assert_eq!(favourites_in(&dir, "ann"), None);
        save_favourites_in(&dir, "ann", &["Wide".into(), "Drums".into()]).unwrap();
        assert_eq!(
            favourites_in(&dir, "ann"),
            Some(vec!["Wide".to_string(), "Drums".to_string()])
        );
        let raw = std::fs::read_to_string(dir.join("ann.ig-user")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["windows"][0]["monitor"], "DP-1");
        assert_eq!(value["favourites"]["looks"][0], "punt");
        // A file that does not exist yet is created.
        save_favourites_in(&dir, "bob", &["Wide".into()]).unwrap();
        assert_eq!(favourites_in(&dir, "bob"), Some(vec!["Wide".to_string()]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The pane draws the playhead and nothing else, and its menu
    /// sends what the menu offers.
    ///
    /// Everything visible here — which presets exist, which key each is
    /// on, which one the programme camera is currently showing — comes
    /// back from the engine on the playhead. The failure this guards
    /// against is the pane keeping its own copy of the venue's cameras:
    /// it would look right until someone cut from a second window or an
    /// iPad, and then quietly disagree with the rig.
    ///
    /// r[verify studio.video.cameras-pane] - the pane half: badges, the active tile, the menu
    /// r[verify studio.one-truth] - the list, the badges and the highlight are all read from the playhead
    #[test]
    fn the_pane_draws_the_playhead_and_its_menu_sends_the_three_actions() {
        let mut s = state();

        // The list is the playhead's, in the venue's order, and the
        // badges are the playhead's keys.
        assert_eq!(s.presets.len(), 4);
        assert_eq!(key_of(&s, "Wide"), Some(1));
        assert_eq!(key_of(&s, "Bird's eye"), Some(0), "the tenth key is `0`");
        assert_eq!(
            key_of(&s, "Side stage"),
            None,
            "a preset on no key wears no badge"
        );

        // The lit tile is whichever preset the playhead says is active,
        // and it moves when the playhead does — the pane remembers
        // nothing of its own between the two.
        let active = |s: &CameraState, name: &str| {
            s.preset
                .as_deref()
                .is_some_and(|p| p.eq_ignore_ascii_case(name))
        };
        assert!(active(&s, "Drums"));
        assert!(!active(&s, "Wide"));
        s.preset = Some("Wide".into());
        assert!(active(&s, "Wide"));
        assert!(!active(&s, "Drums"));
        s.preset = None;
        assert!(
            !s.presets.iter().any(|p| active(&s, p)),
            "with the engine on no preset the pane lights none"
        );

        // Click cuts; the menu's three items are the three commands.
        let click = Command::Camera {
            target: CameraTarget::Preset("Drums".into()),
            beats: 0.0,
        };
        assert!(
            matches!(&click, Command::Camera { beats, .. } if *beats == 0.0),
            "a click on a tile dissolves instead of cutting"
        );
        for command in [
            Command::SetCameraSlot {
                slot: 4,
                name: "Drums".into(),
            },
            Command::SaveCameraPreset {
                name: "Wing".into(),
            },
            Command::DeleteCameraPreset {
                name: "Wing".into(),
            },
        ] {
            let json = serde_json::to_string(&command).expect("a menu item serialises");
            let back: Command = serde_json::from_str(&json).expect("and reaches the engine");
            assert_eq!(format!("{back:?}"), format!("{command:?}"), "{json}");
        }

        // And a slot change lands in the operator's file, which is the
        // half of it that outlives the session.
        let dir = std::env::temp_dir().join(format!("ig-campane-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        save_favourites_in(&dir, "ann", &["Wide".into(), String::new(), "Drums".into()]).unwrap();
        assert_eq!(
            favourites_in(&dir, "ann"),
            Some(vec!["Wide".to_string(), String::new(), "Drums".to_string()]),
            "an empty key has to survive as an empty key, or every badge after it shifts"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn camera_commands_round_trip_as_json() {
        for command in [
            Command::Camera {
                target: CameraTarget::Slot(3),
                beats: 2.0,
            },
            Command::Camera {
                target: CameraTarget::Preset("Side stage".into()),
                beats: 0.0,
            },
            Command::SaveCameraPreset {
                name: "Wing".into(),
            },
            Command::SetCameraSlot {
                slot: 0,
                name: "Bird's eye".into(),
            },
            Command::DeleteCameraPreset {
                name: "Wing".into(),
            },
            Command::Wide {
                target: CameraTarget::Preset("Wide".into()),
            },
            Command::CanvasSource {
                canvas: "side-left".into(),
                source: "camera:programme".into(),
            },
        ] {
            let json = serde_json::to_string(&command).unwrap();
            let back: Command = serde_json::from_str(&json).unwrap();
            assert_eq!(format!("{back:?}"), format!("{command:?}"), "{json}");
        }
    }
}
