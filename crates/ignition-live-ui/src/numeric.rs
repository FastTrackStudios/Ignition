//! Small, audited numeric conversions shared across the panes.
//!
//! `as_conversions` is denied so a lossy cast cannot appear inline at
//! each call site; each of these is the one place a particular shape of
//! cast happens, with the reason the truncation is total written beside
//! it. See `docs/ops/clippy.md`.

/// A beat's tenths digit for display — "4.3" is `tenths(0.3)`.
///
/// Every caller passes an already-fractional value in `0.0..10.0` (a
/// beat's fractional part times ten), so the round never lands outside
/// a single digit and the `% 10` is a belt no caller has ever needed.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "callers pass an already-fractional tenths value; see the doc comment"
)]
pub const fn tenths(x: f64) -> u32 {
    x.round() as u32 % 10
}

/// A cue number or bar count as the nearest whole number, for display.
///
/// Cue counts and bar numbers in a show stay in the low thousands at
/// most, nowhere near where an `f32` and an `i64` round differently, so
/// the truncation this performs never disagrees with what is shown.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "cue and bar counts are small; see the doc comment"
)]
pub const fn rounded_i64(x: f32) -> i64 {
    x.round() as i64
}

/// A small whole count as a float — a percentage tile's `0..=100`, a
/// palette's wedge count.
///
/// Nowhere near the 2^24 where an `f32` stops counting integers exactly,
/// so the conversion is total in practice.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "callers pass small literal counts; see the doc comment"
)]
pub const fn small_f32(n: u32) -> f32 {
    n as f32
}

/// A row index as a float, for scroll-position arithmetic.
///
/// A pane's row count stays in the thousands at most, nowhere near
/// where an `f64` stops counting integers exactly.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "callers pass small row counts; see the doc comment"
)]
pub const fn small_f64(n: usize) -> f64 {
    n as f64
}

/// A non-negative scroll offset or row count, floored to a `usize`.
///
/// `#[cfg(test)]`: today's only caller is the visible-band test in
/// `panes.rs`; move the attribute the day a render path needs it too.
#[cfg(test)]
///
/// NaN and anything at or below zero become 0 — a scroll position never
/// goes negative in practice, but a panel's own arithmetic should not
/// panic if it ever briefly does.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to non-negative before the cast; see the doc comment"
)]
pub fn floor_usize(x: f64) -> usize {
    if x.is_nan() || x <= 0.0 {
        return 0;
    }
    x.floor() as usize
}
