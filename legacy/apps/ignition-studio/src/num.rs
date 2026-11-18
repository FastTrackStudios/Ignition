//! The audited numeric conversions the rest of the crate reaches for
//! instead of an inline `as`. `as_conversions` is denied so that a lossy
//! cast cannot appear unexamined in application code — see
//! `docs/ops/clippy.md`. Each helper here states exactly why its cast is
//! total (or as total as the existing behaviour ever was) for the domain
//! it is used in, once, so nothing downstream has to re-derive the
//! argument. Same shape as `ignition-viz`'s `crates/ignition-viz/src/
//! num.rs` — this is `ignition-studio`'s copy of that idiom, sized to
//! what the sound detector, dock layout and remote-input paths actually
//! cast; it cannot reuse that module's helpers because they are
//! `pub(crate)` to `ignition-viz`.
//!
//! Two families:
//!
//! - **Widening a count to a float** (`f32_of_*`): every call site is a
//!   hop length, a sample counter, a bin index or a dock-pane count.
//!   Most are nowhere near 2^24 (where `f32` stops counting integers
//!   exactly); the one exception (`f32_of_u64`, a running audio-sample
//!   counter) is documented at its own definition, because that one
//!   really can drift on a long capture session and this module says so
//!   rather than pretending otherwise.
//! - **Narrowing a float to an integer** (`usize_of_f32`, `u32_of_f32`,
//!   `byte_of_f32`): the value can be NaN, negative or out of range
//!   because it came from a duration times a sample rate, a UI pixel
//!   size or a colour channel — so the cast is preceded by a clamp that
//!   makes it total, the same shape as `ignition_proto::Curve`'s private
//!   `byte()`.
#![allow(
    dead_code,
    reason = "shared helper module; a helper with no caller in a given feature build is expected, not dead code left behind"
)]

/// A count — a hop length, a bin count, a pane index — as a float. Every
/// call site here is bounded well below the 2^24 where an `f32` stops
/// counting integers exactly, so the cast never actually loses anything.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the module doc comment"
)]
pub const fn f32_of_usize(n: usize) -> f32 {
    n as f32
}

/// The same audit as [`f32_of_usize`], for a `u32` count — a window
/// dimension or dock geometry value, still nowhere near 2^24.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the module doc comment"
)]
pub const fn f32_of_u32(n: u32) -> f32 {
    n as f32
}

/// The same audit as [`f32_of_usize`], for an `i32` count — a signed
/// loop index in a test fixture, still nowhere near 2^24.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the module doc comment"
)]
pub const fn f32_of_i32(n: i32) -> f32 {
    n as f32
}

/// A running `u64` sample counter, widened to `f32`. Unlike the other
/// `f32_of_*` helpers this one is *not* claiming the value stays under
/// 2^24 forever: a capture session past a few minutes at a typical
/// sample rate can exceed it, and at that point the returned second
/// count starts rounding to the nearest few samples instead of being
/// exact. That is the same behaviour the bare `as` cast this replaces
/// already had — this helper does not fix it, only names it, so a
/// future change to track elapsed time more precisely has one call site
/// to find instead of a search for `as f32`.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "matches the precision this crate's sample-counter cast has always had; see the module doc comment"
)]
pub const fn f32_of_u64(n: u64) -> f32 {
    n as f32
}

/// An `f64` narrowed to the `f32` the dock layout and window-size paths
/// work in — a widget's reported size, a pointer position. Every call
/// site here is far below where `f32`'s ~7 significant digits would
/// matter for a screen-space measurement.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "values here are far below where f32 precision would matter; see the module doc comment"
)]
pub const fn f32_of_f64(v: f64) -> f32 {
    v as f32
}

/// A clamped, non-negative float as an index or a sample count. NaN and
/// anything below zero become 0; the upper clamp sits at the largest
/// value an `f32` still represents exactly below `u32::MAX`, far larger
/// than any hop count, bin count or synthesised test buffer this crate
/// builds.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a non-negative, in-range value before the cast"
)]
pub fn usize_of_f32(value: f32) -> usize {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    value.min(4_294_967_040.0) as usize
}

/// A clamped, non-negative float as a `u32` — a window or pane pixel
/// dimension. Screen sizes in this crate are addressed in the
/// thousands at most, never near `u32::MAX`, so the clamp below is the
/// only bound the cast needs.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a non-negative, screen-sized value before the cast"
)]
pub fn u32_of_f32(value: f32) -> u32 {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    value.min(4_294_967_040.0) as u32
}

/// A float clamped to `0..=255` and rounded to the nearest byte — a
/// colour channel. NaN and anything at or below 0 land on 0, anything
/// at or above 255 lands on 255, so the cast that follows cannot lose a
/// sign or truncate anything the clamp has not already decided. Mirrors
/// `ignition_proto::Curve`'s private `byte()`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the clamp above makes this cast total; see the module doc comment"
)]
pub const fn byte_of_f32(value: f32) -> u8 {
    if value.is_nan() {
        return 0;
    }
    value.round().clamp(0.0, 255.0) as u8
}

/// An OSC `Long` (`i64`) argument, widened to the `f32` every other
/// numeric OSC type in `remote.rs` reports as. A control surface sends a
/// 64-bit integer for what is, in practice, a toggle (0/1) or a small
/// fader value — nowhere near where `f32` would round it — but unlike
/// the other `f32_of_*` helpers this one is not claiming a bound the
/// caller has already checked: an OSC message can carry any `i64` the
/// sender likes, so a value that genuinely was astronomical would round,
/// the same as the bare `as` cast this replaces already did.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "matches the precision this crate's OSC-value cast has always had; see the doc comment"
)]
pub const fn f32_of_i64(n: i64) -> f32 {
    n as f32
}

/// A `u32` count widened to `usize` — a light-swatch tally, a fixture
/// count read back off a venue. Every target this crate ships for has a
/// `usize` at least as wide as `u32`, so this is exact everywhere in
/// practice; the saturating fallback exists only so the conversion has
/// no `as` to audit, not because it is expected to trigger. Mirrors
/// `ignition_viz::num::usize_of_u32`.
#[must_use]
pub fn usize_of_u32(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}
