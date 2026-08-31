//! The priority stack, and the macros that drive it.
//!
//! The top of the domain: what the rig is actually outputting right now,
//! given everything that wants a say. Hand, masters, flashes, faders,
//! triggers, the cue player — [`playbacks`] is the order they merge in
//! and the rule that transient beats sustained, and it is the last word
//! before a value reaches the encoder. See `docs/spec/playback.md`.
//!
//! [`macros`] is the desk's own automation: the named sequences a page
//! fader or a key fires, written against the effect library and the
//! stack below.
//!
//! Nothing depends on this crate except an application. That is what
//! being the top of a layering means, and it is why the split is worth
//! having — the merge order can be reasoned about, and tested, without
//! the cue engine or the effect library in the way.

pub mod macros;
pub mod playbacks;

pub use macros::{HostRequest, MacroRunner};
pub use playbacks::{Class, Playback, Playbacks};

pub use ignition_effects::*;
