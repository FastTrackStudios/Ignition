//! The lighting domain, composed.
//!
//! This crate holds no code of its own any more. It is the seam an
//! application reaches for: one import that assembles the domain's five
//! feature slices into the flat namespace they had when they were one
//! crate, so `ignition_core::Recipe` and `ignition_core::music::Bars`
//! mean exactly what they always meant.
//!
//! ```text
//!  ignition-playback   the priority stack, desk macros
//!         ▲
//!  ignition-effects    the shipped effect library
//!         ▲
//!  ignition-show       recipes, cues, tracking, programmer, show file
//!         ▲
//!  ignition-colour     ignition-rig     ignition-daw-proto
//!  what a value is     what it lands on   when it happens
//! ```
//!
//! The layering is not decoration. It was found rather than chosen: a
//! dependency analysis of the old crate's modules turned up exactly one
//! strongly-connected component — the nine modules now in
//! `ignition-show`, which are mutually recursive and stay together —
//! and the arrows above are the edges that were already one-way.
//! Splitting on anything else would have meant redesigning what a cue
//! means.
//!
//! What the split buys: colour and the rig can be taken without the cue
//! engine; the effect library can be reordered or replaced without the
//! cue engine noticing; and the merge order in `playbacks` can be
//! reasoned about with nothing above it. What it costs: one more `use`
//! line in each feature crate, and this file.
//!
//! Nothing in the domain opens a socket, reads a frame or touches Bevy —
//! `ignition-dmx` sends the bytes, `ignition-viz` draws them,
//! `ignition-daw` supplies the arrangement, and the studio composes all
//! of it.
//!
//! `no_std`-compatible is still the aim rather than the state: `std`
//! collections and `String` are used throughout, so the `alloc`-only
//! split has not been made yet.

// `ignition-playback` sits at the top and re-exports every layer under
// it, each of which re-exports the one under it in turn. Re-exporting
// the top is therefore the whole domain, in one line, with no list here
// to fall out of date the next time a type is added three crates down.
pub use ignition_playback::*;

// Named again rather than left to the glob: a glob import does not carry
// module re-exports through more than it must, and these are the paths
// the tree spells out — `ignition_core::music::Position`,
// `ignition_core::effects::library()`.
pub use ignition_colour::{color, preset};
pub use ignition_daw_proto as music;
pub use ignition_effects::effects;
pub use ignition_playback::{macros, playbacks};
pub use ignition_rig::{focus, group, selection, tricks};
pub use ignition_show::{bump, canvas, cue, profile, programmer, recipe, show_file, step, trigger};
