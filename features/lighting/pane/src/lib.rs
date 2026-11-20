//! Lighting, an Architect extension: the show in 3D as a pane.
//!
//! The pane is a process view (architect#641): the app runs the
//! `ignition` program with `--process-view`, and the program renders the
//! room, the rig and the cues following the clock it is given, sending
//! its frames over shared memory. Natively nothing of Bevy is in this
//! crate; a browser runs no programs, so there the room is Bevy in the
//! page (`web`).
//!
//! What the pane runs is the extension's settings ([`Picks`]): the show,
//! the Live set, the camera, the clock and the renderer — set on the
//! settings page or by its actions, read live. Linked in (feature `pane`)
//! the view is the GPU pane; run as a process (a development copy) it is
//! a status pane that opens the room in a window (see `ext`). Linked into
//! a browser build (`pane` on wasm32) the room is Ignition's web renderer
//! in the page.

// In a browser nothing is `Send`: the page's futures run on its one thread.
#![cfg_attr(target_arch = "wasm32", allow(clippy::future_not_send))]

use std::path::PathBuf;

/// The `cues` view: a session's cue list following its transport.
#[cfg(feature = "extension")]
pub mod cues;
#[cfg(feature = "extension")]
pub mod ext;
/// Programming a show: the library, a song's and a set's cues,
/// environments, people, fixtures, the patch, tricks.
#[cfg(feature = "extension")]
pub mod program;
/// The show's cue list in a session (the session primitive).
#[cfg(feature = "extension")]
pub mod session;
/// The room in a browser: Ignition's web renderer in a canvas, following
/// the session.
#[cfg(all(feature = "pane", target_arch = "wasm32"))]
pub mod web;

/// What the pane runs: the extension's settings, empty for "the default".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Picks {
    pub show: String,
    pub live_set: String,
    pub camera: String,
    pub clock: String,
    pub renderer: String,
    /// Drive the rig (sACN / Art-Net) from the pane's renderer.
    pub output: bool,
}

/// The settings [`Picks`] is read from.
pub const SETTINGS: [&str; 6] = [
    "ignition.show",
    "ignition.live_set",
    "ignition.camera",
    "ignition.clock",
    "ignition.renderer",
    "ignition.output",
];

impl Picks {
    /// The renderer program: the setting, else `IGNITION_BIN`, else
    /// `ignition` beside the app's executable, else on `PATH`.
    #[must_use]
    pub fn program(&self) -> PathBuf {
        non_empty(&self.renderer)
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("IGNITION_BIN").map(PathBuf::from))
            .or_else(|| {
                let beside = std::env::current_exe().ok()?.parent()?.join("ignition");
                beside.is_file().then_some(beside)
            })
            .unwrap_or_else(|| PathBuf::from("ignition"))
    }

    /// The program's arguments: a pane (`--process-view`) or a window.
    #[must_use]
    pub fn args(&self, pane: bool) -> Vec<String> {
        let mut args = Vec::new();
        if pane {
            args.push("--process-view".to_owned());
        }
        let clock = non_empty(&self.clock).unwrap_or("studio");
        args.extend(["--clock".to_owned(), clock.to_owned()]);
        let live_set = non_empty(&self.live_set)
            .map(str::to_owned)
            .or_else(|| std::env::var("IGNITION_LIVE_SET").ok());
        if let Some(set) = live_set {
            args.extend(["--set".to_owned(), set]);
        }
        if let Some(show) = non_empty(&self.show) {
            args.extend(["--show".to_owned(), show.to_owned()]);
        }
        if let Some(camera) = non_empty(&self.camera) {
            args.extend(["--camera".to_owned(), camera.to_owned()]);
        }
        // Only the pane drives the rig: a popped-out room is a picture.
        if pane && self.output {
            args.push("--output".to_owned());
        }
        args
    }

    /// One key per distinct thing to run: a change of any pick is another
    /// session (the renderer started again with it).
    #[must_use]
    pub fn session_key(&self) -> String {
        format!(
            "ignition.show|{}|{}|{}|{}|{}|{}",
            self.show, self.live_set, self.camera, self.clock, self.renderer, self.output
        )
    }
}

fn non_empty(s: &str) -> Option<&str> {
    let s = s.trim();
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pane_runs_the_process_view_with_its_picks() {
        let picks = Picks {
            show: "/s/night.ig-show".into(),
            live_set: "/s/set.als".into(),
            camera: "Wide".into(),
            clock: "internal".into(),
            renderer: "/bin/ignition-next".into(),
            output: true,
        };
        assert_eq!(picks.program(), PathBuf::from("/bin/ignition-next"));
        assert_eq!(
            picks.args(true),
            [
                "--process-view",
                "--clock",
                "internal",
                "--set",
                "/s/set.als",
                "--show",
                "/s/night.ig-show",
                "--camera",
                "Wide",
                "--output"
            ]
        );
        let window = picks.args(false);
        assert!(!window.contains(&"--process-view".to_owned()));
        assert!(
            !window.contains(&"--output".to_owned()),
            "a pop-out never drives the rig"
        );
    }

    #[test]
    fn empty_picks_follow_studio_and_another_pick_is_another_session() {
        let picks = Picks::default();
        assert_eq!(
            &picks.args(true)[..3],
            ["--process-view", "--clock", "studio"]
        );
        let camera = Picks {
            camera: "Drums".into(),
            ..Picks::default()
        };
        assert_ne!(picks.session_key(), camera.session_key());
    }
}
