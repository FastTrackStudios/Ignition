//! What the UI says to the visualizer.
//!
//! Dioxus components and the Bevy app live on the same thread but in
//! different ownership worlds — the app is inside a Blitz widget the
//! components cannot reach. A channel is the seam: components send, the
//! widget drains before it steps. That also means every control is a
//! *message*, which is what a hardware surface or a remote will send
//! later without the UI having to change shape.

use ignition_core::{Fader, Selection};

#[derive(Debug, Clone)]
pub enum Command {
    /// Pick the fixtures the next palette hit lands on.
    Select(Selection),
    Deselect,
    /// A colour palette entry, by name.
    Color(String),
    /// A focus palette entry, by name.
    Focus(String),
    /// Intensity for the current selection.
    Dimmer(f32),
    /// Take the operator's hand off the current selection.
    Release,
    ClearValues,
    Fader(usize, Box<Fader>),
    Level(usize, f32),
    /// The master rate every fader's phaser is slaved to, in BPM.
    Rate(f32),
    /// How far every effect swings, 0..=1 — the control an operator
    /// holds for most of a night. Distinct from a master: size flattens
    /// the effect, a master dims the fixtures.
    Size(f32),
    /// A multiplier on every effect's rate against its speed master.
    EffectRate(f32),
    /// A role's intensity master, 0..=1.
    Master(String, f32),
    /// Play one role on its own; `None` clears.
    Solo(Option<String>),
    /// Fire a bump by hand — a flash key.
    ///
    /// Carries the same shape a charted hit fires, so the two are one
    /// gesture arriving from two places rather than two features that
    /// happen to look alike.
    Flash(String, ignition_core::BumpKind),
    Go,
    Cue(usize),
    /// Transport. The song is loaded once at startup; these move it.
    Play,
    Stop,
    /// Locate to a section by name — how a rehearsal loops one part.
    Section(String),
    /// Move the playhead to a fraction of the song, 0..=1 — what
    /// dragging the progress bar sends.
    Scrub(f32),
}

pub type Sender = std::sync::mpsc::Sender<Command>;
pub type Receiver = std::sync::mpsc::Receiver<Command>;

pub fn channel() -> (Sender, Receiver) {
    std::sync::mpsc::channel()
}

/// Where the show actually is — the widget's answer back to the UI.
///
/// The commands above are one-way, and that was fine while every change
/// came from a click: the sidebar could just remember what it had sent.
/// The moment the *song* drives the cues, that memory becomes a second
/// source of truth and starts lying — it went on highlighting the last
/// cue clicked while the transport had moved several cues past it. So
/// the player's own state comes back, and the UI renders that rather
/// than anything it believes about itself.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Playhead {
    /// Index of the cue the player is actually standing on.
    pub cue: Option<usize>,
    /// Seconds into the song, and how long it is — the progress bar.
    pub secs: f32,
    pub length: f32,
    pub playing: bool,
}

impl Playhead {
    /// How far through, 0..=1. A zero-length song reports 0 rather than
    /// NaN, which would otherwise reach a CSS percentage and lay the bar
    /// out at a width no one can explain.
    pub fn fraction(&self) -> f32 {
        if self.length > 0.0 {
            (self.secs / self.length).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

/// The latest playhead, written by the widget and read by the UI.
///
/// A watch channel rather than a queue: the UI wants the *current*
/// value, and a backlog of stale positions is worse than useless — it
/// would render the song's history one frame at a time, always behind.
pub type StateTx = tokio::sync::watch::Sender<Playhead>;
pub type StateRx = tokio::sync::watch::Receiver<Playhead>;

pub fn state_channel() -> (StateTx, StateRx) {
    tokio::sync::watch::channel(Playhead::default())
}
