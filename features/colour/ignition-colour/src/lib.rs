//! Colour as intent, not as emitter levels.
//!
//! A leaf of the domain. What the operator means — "warm", "deep", the
//! third colour of a four-way split — and how that resolves against
//! whatever emitters a fixture actually has, whether that is RGB, RGBW,
//! CMY or a colour wheel. See `docs/spec/color.md`.
//!
//! Nothing here knows what a cue is, what a rig is, or what time it is.
//! That is what makes it a leaf, and what lets the layers above take a
//! colour without taking the whole domain with it.

pub mod color;
pub mod preset;

pub use preset::{
    ColorPreset, ColorSplit, Distribute, FocusPointPreset, Palettes, Ref, SplitProblem,
};

// Re-exported so `crate::Attribute` reads the same here as it does in
// every other crate of the domain — the proto types are the vocabulary
// all of them share.
pub use ignition_proto::{Attribute, ChanId, ColorChannel, PatchEntry, Placement, Quat, Vec3};
