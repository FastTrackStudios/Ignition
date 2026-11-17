//! The shipped effect library: a recipe with two or more steps.
//!
//! Above [`ignition_show`], because an effect is written in the show's
//! own vocabulary — it is a `Recipe` whose `Timing` carries more than
//! one `Step` — and below the playback stack, which is what decides
//! whether one is running. See `docs/spec/effects.md`.
//!
//! Its own layer rather than part of `ignition-show` because the
//! dependency genuinely runs one way: an effect knows what a recipe is,
//! and nothing in `recipe` or `cue` knows this library exists. That is
//! the seam the cycle analysis found, and it is a real one — the
//! library can grow, be reordered, or be replaced without the cue engine
//! noticing.

pub mod effects;

pub use ignition_show::*;
