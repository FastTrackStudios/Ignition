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
    Go,
    Cue(usize),
    /// Transport. The song is loaded once at startup; these move it.
    Play,
    Stop,
    /// Locate to a section by name — how a rehearsal loops one part.
    Section(String),
}

pub type Sender = std::sync::mpsc::Sender<Command>;
pub type Receiver = std::sync::mpsc::Receiver<Command>;

pub fn channel() -> (Sender, Receiver) {
    std::sync::mpsc::channel()
}
