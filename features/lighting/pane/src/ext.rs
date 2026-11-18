//! The extension: one `linked::Extension`, linked into Studio Architect
//! (feature `pane`: its `show` view is the GPU process view) or served as
//! a process (`src/main.rs`: a development or installed copy). A process
//! copy's room is the manifest's `room` view (`kind = "process"`,
//! architect#838), which the app draws from the renderer in the package;
//! its own `show` view is the room's controls.

use std::sync::{Mutex, PoisonError};

use architect_extensions::hostcall::HostRequest;
use architect_extensions::linked::{Builder, Extension, Handle};
use dioxus::prelude::*;
use serde_json::{Value, json};

use crate::{Picks, SETTINGS};

/// The extension.
#[derive(Debug, Clone, Copy, Default)]
pub struct Lighting;

/// Its id and its views', as the manifest names them.
pub const EXTENSION: &str = "ignition";
pub const VIEW: &str = "show";
/// The room drawn by the renderer in the package (`kind = "process"`).
pub const ROOM: &str = "room";

/// Which way this copy runs: in the app, its view the GPU pane; or as a
/// process, its view a status pane.
pub const LINKED: bool = cfg!(feature = "pane");

static HANDLE: Mutex<Option<Handle>> = Mutex::new(None);

pub(crate) fn handle() -> Result<Handle, String> {
    HANDLE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .ok_or_else(|| "the Lighting extension is not running".to_owned())
}

impl Extension for Lighting {
    fn manifest(&self) -> &'static str {
        include_str!("../extension.toml")
    }

    fn build(&self, ext: &mut Builder) {
        *HANDLE.lock().unwrap_or_else(PoisonError::into_inner) = Some(ext.handle());
        ext.view(VIEW, ShowView)
            .action("IGNITION_SHOW_OPEN", |_| open())
            .action("IGNITION_SHOW_PICK", |a| async move {
                if let Some(set) = a.get("live_set").and_then(Value::as_str) {
                    set_setting("ignition.live_set", set).await?;
                }
                pick(&a, "show", "ignition.show").await
            })
            .action("IGNITION_CAMERA_PICK", |a| async move {
                pick(&a, "camera", "ignition.camera").await
            })
            .action("IGNITION_CLOCK_PICK", |a| async move {
                pick(&a, "clock", "ignition.clock").await
            })
            .action("IGNITION_POP_OUT", |a| async move {
                let display = a.get("display").and_then(Value::as_u64);
                let picks = picks().await?;
                pop_out(&picks, display).map(|pid| json!({ "popped_out": pid, "display": display }))
            })
            .action("IGNITION_POP_IN", |_| async move {
                Ok(json!({ "stopped": pop_in() }))
            })
            .action("IGNITION_SESSION_STORE", crate::session::store_song)
            .action("IGNITION_SESSION_CUES", crate::session::cues)
            .action("IGNITION_SESSION_FOLLOW", crate::session::follow)
            .action("IGNITION_SESSION_TAKEN", |_| async move {
                Ok(crate::session::report())
            })
            .action("IGNITION_STATE", |_| async move {
                let picks = picks().await?;
                Ok(json!({
                    "copy": if LINKED { "linked" } else { "process" },
                    "pid": std::process::id(),
                    "renderer": picks.program().display().to_string(),
                    "args": picks.args(LINKED),
                    "show": picks.show,
                    "camera": picks.camera,
                    "clock": picks.clock,
                }))
            });
    }
}

/// Opens the room: a linked copy's `show` view (its GPU pane), else the
/// `room` view the app draws from the package's renderer.
async fn open() -> Result<Value, String> {
    let view = if LINKED { VIEW } else { ROOM };
    open_view(view).await?;
    Ok(
        json!({ "opened": format!("{EXTENSION}/{view}"), "copy": if LINKED { "linked" } else { "process" } }),
    )
}

async fn open_view(view: &str) -> Result<(), String> {
    handle()?
        .host(HostRequest::Run {
            action: "EXT_VIEW_OPEN".to_owned(),
            args: json!({ "extension": EXTENSION, "view": view }),
            ticket: None,
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn pick(args: &Value, name: &str, key: &str) -> Result<Value, String> {
    let value = args
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("`{name}` is required"))?;
    set_setting(key, value).await?;
    Ok(json!({ key: value }))
}

async fn set_setting(key: &str, value: &str) -> Result<(), String> {
    handle()?
        .host(HostRequest::SetSetting {
            key: key.to_owned(),
            value: Value::String(value.to_owned()),
        })
        .await
        .map(drop)
        .map_err(|e| e.to_string())
}

/// The settings now.
async fn picks() -> Result<Picks, String> {
    let h = handle()?;
    let mut values = Vec::with_capacity(SETTINGS.len());
    for key in SETTINGS {
        let value = h
            .host(HostRequest::Setting {
                key: key.to_owned(),
            })
            .await
            .map_err(|e| e.to_string())?;
        values.push(value);
    }
    let mut v = values.into_iter();
    let mut next = || v.next().unwrap_or(Value::Null);
    let mut text = || next().as_str().unwrap_or_default().to_owned();
    Ok(Picks {
        show: text(),
        live_set: text(),
        camera: text(),
        clock: text(),
        renderer: text(),
        output: next().as_bool().unwrap_or(false),
    })
}

/// The renderer popped out to a display, if one is: Studio's to start
/// and stop, never a program of the person's own.
static POPPED: Mutex<Option<std::process::Child>> = Mutex::new(None);

/// The renderer, borderless fullscreen on `display` (an index; none: the
/// display Studio is on), with the pane's show, camera and clock. One at a
/// time: popping out again replaces it. Its pid.
fn pop_out(picks: &Picks, display: Option<u64>) -> Result<u32, String> {
    pop_in();
    let mut command = std::process::Command::new(picks.program());
    command
        .args(picks.args(false))
        .env("IGNITION_FULLSCREEN", "1")
        .stdin(std::process::Stdio::null());
    if let Some(display) = display {
        command.env("IGNITION_MONITOR", display.to_string());
    }
    if let Ok(root) = std::env::var("IGNITION_ROOT") {
        command.env("IGNITION_ROOT", root);
    }
    let child = command
        .spawn()
        .map_err(|e| format!("starting {}: {e}", picks.program().display()))?;
    let pid = child.id();
    *POPPED.lock().unwrap_or_else(PoisonError::into_inner) = Some(child);
    Ok(pid)
}

/// Stops the popped-out renderer (SIGTERM: it lets Studio's engine go
/// cleanly). Whether one was running.
fn pop_in() -> bool {
    let Some(mut child) = POPPED.lock().unwrap_or_else(PoisonError::into_inner).take() else {
        return false;
    };
    if child.try_wait().ok().flatten().is_some() {
        return false;
    }
    let _ = std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    true
}

/// The view: the picks, read again when a setting changes (once a
/// second), and what draws the room.
#[component]
fn ShowView() -> Element {
    let mut current = use_signal(|| None::<Picks>);
    use_future(move || async move {
        loop {
            if let Ok(now) = picks().await
                && current.peek().as_ref() != Some(&now)
            {
                current.set(Some(now));
            }
            futures_timer::Delay::new(std::time::Duration::from_secs(1)).await;
        }
    });
    let Some(picks) = current() else {
        return rsx! {
            div { style: "flex: 1; display: flex; align-items: center; justify-content: center; color: var(--muted-foreground); font-size: 13px;",
                "Lighting…"
            }
        };
    };
    rsx! { Room { picks } }
}

/// Linked in: the GPU pane, `ignition --process-view`, started again
/// whenever a pick changes (the previous renderer stopped).
#[cfg(feature = "pane")]
#[component]
fn Room(picks: Picks) -> Element {
    use architect_process_view::host::{ProcessViewPane, Program, sessions};
    let key = picks.session_key();
    // One renderer at a time: the sessions of earlier picks stop.
    {
        let mut all = sessions().lock().unwrap_or_else(PoisonError::into_inner);
        let stale: Vec<String> = all
            .keys()
            .filter(|k| k.starts_with("ignition.show|") && **k != key)
            .cloned()
            .collect();
        for k in stale {
            if let Some(session) = all.remove(&k) {
                session.stop();
            }
        }
    }
    let mut program = Program::new(picks.program());
    program.args = picks.args(true);
    // Shared memory: the renderer is on wgpu 29 (Bevy), the app on 30.
    let mut program = program.env("ARCHITECT_PROCESS_VIEW_TRANSPORT", "shm");
    if let Ok(root) = std::env::var("IGNITION_ROOT") {
        program = program.env("IGNITION_ROOT", root);
    }
    rsx! {
        div { style: "flex: 1; display: flex; min-height: 0; min-width: 0;",
            for k in std::iter::once(key) {
                div { key: "{k}", style: "flex: 1; display: flex; min-height: 0; min-width: 0;",
                    ProcessViewPane { session_key: k.clone(), program: program.clone() }
                }
            }
        }
    }
}

/// As a process (a development or installed copy): the room is the
/// `room` view, which the app draws from the package's renderer; this is
/// what it runs, a button that opens it, and one that pops it out to a
/// display of its own.
#[cfg(not(feature = "pane"))]
#[component]
fn Room(picks: Picks) -> Element {
    let mut said = use_signal(String::new);
    let program = picks.program().display().to_string();
    let args = picks.args(false).join(" ");
    rsx! {
        div { style: "flex: 1; display: flex; flex-direction: column; gap: 10px; padding: 20px; font-size: 13px;",
            div { style: "font-size: 15px; font-weight: 600;", "Lighting" }
            div { style: "color: var(--muted-foreground);",
                "The room is drawn by the renderer in this package, in its own pane (Lighting: room); the show, the camera and the clock are this extension's settings."
            }
            div { style: "font-family: monospace; font-size: 12px;", "{program} {args}" }
            div { style: "display: flex; gap: 8px;",
                button {
                    style: "padding: 6px 12px;",
                    onclick: move |_| {
                        spawn(async move {
                            if let Err(e) = open_view(ROOM).await {
                                said.set(e);
                            }
                        });
                    },
                    "Open the room"
                }
                button {
                    style: "padding: 6px 12px;",
                    onclick: move |_| {
                        let picks = picks.clone();
                        said.set(match pop_out(&picks, None) {
                            Ok(pid) => format!("popped out (pid {pid})"),
                            Err(e) => e,
                        });
                    },
                    "Pop out to a display"
                }
            }
            div { "{said}" }
        }
    }
}
