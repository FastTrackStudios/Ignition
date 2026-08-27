//! The Live surface as a library: the same components on the studio's
//! desk and on an iPad in Safari.
//!
//! The split is at the transport. A component here never owns a
//! channel or a socket; it calls [`send`] and reads [`use_playhead`],
//! and the *host* — the studio binary or the web app — decides what
//! those are wired to. On the desktop that is the mpsc into the Blitz
//! widget and the tokio watch out of it; in the browser it is one
//! WebSocket carrying the same two types as JSON. Nothing in the
//! view knows which (`r[studio.touch.ipad]`, `r[studio.one-truth]`).

// r[impl studio.touch.ipad] - one set of components, native and wasm
// r[impl studio.one-truth] - the playhead is fed, never computed here

pub mod command;
pub mod desk;
pub mod faders;
pub mod library;
pub mod live;
pub mod operators;
pub mod program;

pub use command::{Command, PageMove, Playhead, SpeedKey};
use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

// ── The transport seam ───────────────────────────────────────────────

/// Where commands go. The host implements this once and installs it.
///
/// `Send + Sync` because the studio installs it before any window
/// exists and every window's components call it; the browser is one
/// thread and satisfies the bound with a `futures` channel sender.
pub trait Bridge: Send + Sync {
    fn send(&self, command: Command);
}

impl<F: Fn(Command) + Send + Sync> Bridge for F {
    fn send(&self, command: Command) {
        self(command)
    }
}

static BRIDGE: std::sync::OnceLock<Box<dyn Bridge>> = std::sync::OnceLock::new();

/// Install the host's transport. Once per process; a second call is
/// ignored, which is what a hot-reloaded root wants.
pub fn install(bridge: impl Bridge + 'static) {
    let _ = BRIDGE.set(Box::new(bridge));
}

/// A free function rather than a captured closure: every control needs
/// to send, closures in `rsx!` are `FnMut`, and a captured sender
/// cannot be moved out of one more than once. Before `install`, a
/// command is dropped — there is nothing to send it to.
pub fn send(command: Command) {
    if let Some(bridge) = BRIDGE.get() {
        bridge.send(command);
    }
}

/// The playhead the host feeds, as a signal in context.
///
/// The host owns the signal and writes it whenever the engine's state
/// moves — from a tokio watch on the desktop, from a WebSocket frame
/// in the browser. Components read it; the derived hooks below narrow
/// it so a fader column does not re-render on every tick of the song
/// clock.
#[derive(Clone, Copy)]
pub struct PlayheadFeed(pub Signal<Playhead>);

/// The whole playhead. Every read of this re-renders with the song
/// clock; prefer [`use_desk`] or [`use_current_cue`] for a control that
/// only cares about a slice.
pub fn use_playhead() -> Signal<Playhead> {
    use_context::<PlayheadFeed>().0
}

/// Only the cue the player is standing on. A memo, so the cue list —
/// a hundred-odd rows that only care which one is lit — diffs only
/// when the cue actually changes.
pub fn use_current_cue() -> Memo<Option<usize>> {
    let playhead = use_playhead();
    use_memo(move || playhead().cue)
}

/// Which hit is ringing, for the list.
pub fn use_ringing_hit() -> Memo<Option<usize>> {
    let playhead = use_playhead();
    use_memo(move || playhead().hit)
}

/// The desk's own state — page, latches, blind — narrowed from the
/// playhead for the same reason `use_current_cue` is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Desk {
    pub page: usize,
    pub pages: usize,
    pub latched: [bool; ignition_core::FADERS],
    pub toggled: [bool; ignition_core::FADERS],
    pub blind: bool,
    pub tap_bpm: f32,
    pub tap_multiplier: f32,
    pub sound: [f32; 3],
    pub parked: usize,
    pub paused: bool,
}

impl Desk {
    pub fn of(playhead: &Playhead) -> Self {
        Self {
            page: playhead.page,
            pages: playhead.pages.max(1),
            latched: playhead.latched,
            toggled: playhead.toggled,
            blind: playhead.blind,
            tap_bpm: playhead.tap_bpm,
            tap_multiplier: playhead.tap_multiplier,
            sound: playhead.sound,
            parked: playhead.parked,
            paused: playhead.paused,
        }
    }
}

pub fn use_desk() -> Memo<Desk> {
    let playhead = use_playhead();
    use_memo(move || Desk::of(&playhead()))
}

// ── What the surface is made of ──────────────────────────────────────

/// A colour palette entry as the surface draws it: the name to send, and
/// the colour to show. A colour pool that does not show its colours is a
/// list of words, which is the whole reason to have a pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorChip {
    pub name: String,
    pub css: String,
}

/// The named things the surface offers, resolved once from the venue by
/// the host — and, for the browser, sent over once at connect.
#[derive(Debug, Clone, Default, Props, PartialEq, Serialize, Deserialize)]
pub struct Surface {
    pub groups: Vec<String>,
    pub colors: Vec<ColorChip>,
    /// Multi-colour palette entries, drawn as a split disc — the way a
    /// grandMA3 colour preset holding several colours shows in its
    /// picker.
    pub splits: Vec<ColorChip>,
    pub focus: Vec<String>,
    pub cues: Vec<Row>,
}

/// One line of the cue list: a cue the operator can GO, or a hit the
/// song fires. Hits are shown because an operator wants to see what is
/// coming and what just landed; they are not in the GO order — see
/// `docs/spec/triggers.md`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Row {
    Cue {
        index: usize,
        name: String,
    },
    Hit {
        index: usize,
        name: String,
        at: ignition_core::Bars,
    },
}

/// Everything a Live client needs to draw before the first playhead
/// arrives: the surface, the desk banks, whose favourites, and — for
/// the profile the library lists — the file profile the studio loaded,
/// since the browser has no disk to read it from. Also which URLs the
/// server is listening on, for the mode strip.
// r[impl studio.touch.ipad] - the browser is bootstrapped, not configured
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bootstrap {
    pub surface: Surface,
    pub banks: Vec<desk::Bank>,
    pub operator: operators::Operator,
    pub profile: Option<ignition_core::Profile>,
    #[serde(default)]
    pub lan: Vec<String>,
}

/// What the server says to a client. One enum so a client can match on
/// it; the playhead is by far the common case.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "snake_case")]
pub enum ServerMessage {
    Hello(Box<Bootstrap>),
    Playhead(Playhead),
}

// ── Shared controls ──────────────────────────────────────────────────

/// The horizontal slider's track width in CSS pixels, matching
/// `live.css` / `studio.css`. Blitz reports pointer positions relative
/// to the element but not the element's own width, so the maths needs
/// the number from somewhere.
pub const HSLIDER_WIDTH: f32 = 90.0;

/// A small horizontal slider, for the trims. Divs, not a range input,
/// so it draws the same on Blitz and in Safari and so the grab area is
/// the whole track.
#[component]
pub fn HSlider(initial: f32, on_change: EventHandler<f32>) -> Element {
    let mut level = use_signal(|| initial);
    let mut held = use_signal(|| false);
    let mut set_from = move |x: f64| {
        let v = (x as f32 / HSLIDER_WIDTH).clamp(0.0, 1.0);
        level.set(v);
        on_change.call(v);
    };
    rsx! {
        div {
            class: "hslider",
            onpointerdown: move |e| {
                held.set(true);
                set_from(e.data.element_coordinates().x);
            },
            onpointermove: move |e| {
                if held() {
                    set_from(e.data.element_coordinates().x);
                }
            },
            onpointerup: move |_| held.set(false),
            onpointerleave: move |_| held.set(false),
            div { class: "hfill", style: "width: {level() * 100.0}%" }
        }
    }
}

/// The cue stack. Underneath the busking layer, not beside it: a cue
/// fills in whatever the operator is not currently holding.
#[component]
pub fn CueList(cues: Vec<Row>) -> Element {
    // What the player is actually standing on, not what was last
    // clicked. A click still fires the cue; it just no longer decides
    // what the list *shows*.
    let current = use_current_cue();
    let ringing = use_ringing_hit();

    rsx! {
        aside { class: "cues",
            header {
                span { "Cue List" }
                button {
                    class: "go",
                    onclick: move |_| send(Command::Go),
                    "GO"
                }
                // GO on the look list — the list beneath the song's that
                // the operator steps by hand.
                button {
                    class: "go look",
                    onclick: move |_| send(Command::LookGo),
                    "LOOK"
                }
            }
            ol {
                for (i, row) in cues.iter().enumerate() {
                    match row {
                        Row::Cue { index, name } => {
                            let index = *index;
                            rsx! {
                                li {
                                    key: "c{i}",
                                    class: if current() == Some(index) { "cue on" } else { "cue" },
                                    onclick: move |_| send(Command::Cue(index)),
                                    span { class: "num", "{index}" }
                                    span { class: "name", "{name}" }
                                }
                            }
                        }
                        Row::Hit { index, name, at } => {
                            let (index, at) = (*index, *at);
                            rsx! {
                                li {
                                    key: "h{i}",
                                    class: if ringing() == Some(index) { "cue hit on" } else { "cue hit" },
                                    onclick: move |_| send(Command::Locate(at)),
                                    span { class: "num", "♪" }
                                    span { class: "name", "{name}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire is JSON of the desktop's own types; nothing is lost on
    /// the way to the iPad and back.
    // r[impl studio.touch.ipad] - the contract round-trips
    #[test]
    fn commands_round_trip_as_json() {
        let commands = vec![
            Command::Select(ignition_core::Selection::Group("Washers".into())),
            Command::Level(3, 0.5),
            Command::Key {
                index: 1,
                action: ignition_core::KeyAction::Flash,
                down: true,
            },
            Command::Page(PageMove::Set(2)),
            Command::Speed(SpeedKey::Half),
            Command::Flash(
                ignition_core::Selection::Group("All".into()),
                ignition_core::BumpKind::White,
            ),
            Command::Hold(None),
            Command::Locate(ignition_core::Bars::new(3, 1.0)),
            Command::PlaybackMaster(ignition_core::Class::Song, 0.25),
            Command::Param {
                index: 0,
                name: "depth".into(),
                value: 0.7,
            },
            Command::Look(Some("punt".into())),
            Command::StoreCue {
                index: 4,
                mode: ignition_core::cue::StoreMode::Track,
            },
            Command::StoreLook {
                name: "verse two".into(),
                kind: ignition_core::profile::LookKind::Bed,
            },
        ];
        for command in commands {
            let json = serde_json::to_string(&command).unwrap();
            let back: Command = serde_json::from_str(&json).unwrap();
            assert_eq!(format!("{back:?}"), format!("{command:?}"), "{json}");
        }
    }

    #[test]
    fn playhead_round_trips_as_json() {
        let mut playhead = Playhead {
            cue: Some(7),
            secs: 12.5,
            length: 200.0,
            playing: true,
            page: 1,
            pages: 4,
            grand: 0.8,
            held_look: Some("punt".into()),
            effects_playing: vec!["strobe".into()],
            protected: vec!["Drummer".into()],
            ..Default::default()
        };
        playhead.levels[2] = 0.4;
        playhead.latched[5] = true;
        let json = serde_json::to_string(&ServerMessage::Playhead(playhead.clone())).unwrap();
        let back: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ServerMessage::Playhead(playhead));
        // An older server's playhead without a newer field still parses.
        let sparse: Playhead = serde_json::from_str(r#"{"cue":1,"secs":1.0}"#).unwrap();
        assert_eq!(sparse.cue, Some(1));
    }

    #[test]
    fn bootstrap_round_trips_as_json() {
        let boot = Bootstrap {
            surface: Surface {
                groups: vec!["All".into()],
                colors: vec![ColorChip {
                    name: "Red".into(),
                    css: "rgb(255 0 0)".into(),
                }],
                cues: vec![Row::Hit {
                    index: 0,
                    name: "stab".into(),
                    at: ignition_core::Bars::new(2, 0.0),
                }],
                ..Default::default()
            },
            banks: vec![desk::Bank {
                name: "Warm".into(),
                scenes: vec![desk::Scene {
                    index: 3,
                    name: "Amber".into(),
                }],
            }],
            operator: operators::Operator::starter("cody"),
            profile: None,
            lan: vec!["http://10.0.0.2:8420".into()],
        };
        let json = serde_json::to_string(&ServerMessage::Hello(Box::new(boot.clone()))).unwrap();
        let back: ServerMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ServerMessage::Hello(Box::new(boot)));

        // The profile too — compared by what the library lists rather
        // than bit for bit, because serde_json's f32 path can move a
        // focus delta by one ulp and nothing on a surface can tell.
        let profile = faders::profile().clone();
        let json = serde_json::to_string(&Some(profile.clone())).unwrap();
        let back: Option<ignition_core::Profile> = serde_json::from_str(&json).unwrap();
        let back = back.expect("a profile");
        assert_eq!(back.pages.len(), profile.pages.len());
        assert_eq!(
            back.looks.keys().collect::<Vec<_>>(),
            profile.looks.keys().collect::<Vec<_>>()
        );
        assert_eq!(
            back.macros.keys().collect::<Vec<_>>(),
            profile.macros.keys().collect::<Vec<_>>()
        );
        assert_eq!(back.roles, profile.roles);
    }
}
