//! The audited numeric conversions the rest of the crate reaches for
//! instead of an inline `as`. `as_conversions` is denied so that a lossy
//! cast cannot appear unexamined in domain code — see
//! `docs/ops/clippy.md`. Each helper here states exactly why its cast is
//! total for the domain it is used in, once, so nothing downstream has to
//! re-derive the argument. Same shape as `ignition-proto`'s `byte()` /
//! `float_of()` / `usize_of()` and `features/show/ignition-show/src/
//! num.rs` — this module is `ignition-viz`'s copy of that idiom, sized to
//! what this crate's renderer, GDTF import and DMX paths actually cast.
//!
//! Two families:
//!
//! - **Widening a count to a float** (`f32_of_*`, `f64_of_*`): every call
//!   site is a frame index, a pixel/grid coordinate, a channel count or a
//!   universe count — nowhere near the point (2^24 for `f32`, 2^52 for
//!   `f64`) where the cast would stop being exact. `clippy::
//!   cast_precision_loss` fires anyway because the compiler cannot see
//!   the bound, so the reason is written once here instead of at every
//!   call site.
//! - **Narrowing a float to an integer** (`byte_of_*`, `usize_of_*`,
//!   `u32_of_*`, `u16_of_*`, `i32_of_*`): the value can be NaN, negative
//!   or out of range because it came from a DMX byte's float form, a
//!   render-space coordinate or a UI slider — so the cast is preceded by
//!   a clamp that makes it total, the same shape as `ignition_proto::
//!   Curve`'s private `byte()`.
//! - **Narrowing an integer to a smaller integer** (`u32_of_usize`,
//!   `u16_of_usize`, `u8_of_usize`, `u16_of_i32`, `u8_of_u16`,
//!   `usize_of_u64`): done with `TryFrom` and a saturating fallback
//!   rather than `as`, so there is no lossy cast to audit at all — the
//!   value either fits, or is clamped to the target type's extreme,
//!   never wrapped.
//!
//! This module lands ahead of most of its call sites: the crate's other
//! files are being cleaned onto the lint gate in parallel, one at a
//! time, and each cast site migrates to one of these helpers as its own
//! file is done. A helper with no caller yet is expected, not dead code
//! left behind — hence the blanket below rather than a per-function
//! `#[expect]`, which would hard-fail the very next file that adds the
//! first real call.
#![allow(
    dead_code,
    reason = "shared helper module populated ahead of its call sites; see the module doc comment"
)]

/// A small count — a loop index, a step number, a fixture or channel
/// count — as a float. Every call site here is bounded well below the
/// 2^24 where an `f32` stops counting integers exactly, so the cast
/// never actually loses anything.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the module doc comment"
)]
pub const fn f32_of_usize(n: usize) -> f32 {
    n as f32
}

/// The same audit as [`f32_of_usize`], for a `u32` count — a grid
/// dimension, a universe or channel count — none of which come near
/// 2^24 either.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the module doc comment"
)]
pub const fn f32_of_u32(n: u32) -> f32 {
    n as f32
}

/// The same audit as [`f32_of_usize`], for an `i32` count — a signed
/// pixel or grid offset, still nowhere near 2^24.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the module doc comment"
)]
pub const fn f32_of_i32(n: i32) -> f32 {
    n as f32
}

/// A `u64` count as `f64` — a frame counter, a nanosecond timestamp
/// component. `f64`'s 52-bit mantissa holds every value this crate
/// actually produces (a show does not run for 2^52 nanoseconds).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here fit f64's 52-bit mantissa; see the module doc comment"
)]
pub const fn f64_of_u64(n: u64) -> f64 {
    n as f64
}

/// The same audit as [`f64_of_u64`], for a `usize` count.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here fit f64's 52-bit mantissa; see the module doc comment"
)]
pub const fn f64_of_usize(n: usize) -> f64 {
    n as f64
}

/// An `f64` narrowed to the `f32` the renderer and DMX-facing code work
/// in — a room coordinate, a beat position. Every call site here is far
/// below where `f32`'s ~7 significant digits would matter for a lighting
/// visualizer, the same bet `ignition-show`'s `f32_of_f64` makes for its
/// clock.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "values here are far below where f32 precision would matter; see the module doc comment"
)]
pub const fn f32_of_f64(v: f64) -> f32 {
    v as f32
}

/// The one place a float becomes a DMX byte, clamped so the cast is
/// total: NaN and anything at or below 0 land on 0, anything at or above
/// 255 lands on 255. Mirrors `ignition_proto::Curve`'s private `byte()`
/// — this copy exists because the visualizer clamps floats to bytes on
/// its own render-side paths, not through a `Curve`.
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

/// [`byte_of_f32`], for an `f64` source value.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the clamp above makes this cast total; see the module doc comment"
)]
pub const fn byte_of_f64(value: f64) -> u8 {
    if value.is_nan() {
        return 0;
    }
    value.round().clamp(0.0, 255.0) as u8
}

/// A clamped, non-negative float as an index. NaN and anything below
/// zero become 0; the upper clamp sits at the largest value an `f32`
/// still represents exactly below `u32::MAX`, which is a far larger
/// index than this crate ever builds.
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

/// [`usize_of_f32`], for an `f64` source value.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a non-negative, in-range value before the cast"
)]
pub fn usize_of_f64(value: f64) -> usize {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    value.min(4_294_967_040.0) as usize
}

/// A clamped, non-negative float as a `u32` — a pixel or grid
/// coordinate. Canvas and texture dimensions in this crate are addressed
/// in the thousands at most, never near `u32::MAX`, so the clamp below
/// is the only bound the cast needs.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a non-negative, grid-sized value before the cast"
)]
pub fn u32_of_f32(value: f32) -> u32 {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    value.min(4_294_967_040.0) as u32
}

/// [`u32_of_f32`], for an `f64` source value.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a non-negative, grid-sized value before the cast"
)]
pub fn u32_of_f64(value: f64) -> u32 {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    value.min(4_294_967_040.0) as u32
}

/// A clamped, non-negative float as a `u16` — a DMX universe or slot
/// index, always well under `u16::MAX` (512 channels, a few hundred
/// universes at most).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to u16's range before the cast"
)]
pub fn u16_of_f32(value: f32) -> u16 {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    value.min(f32::from(u16::MAX)) as u16
}

/// A float clamped into `i32`'s range — a signed pixel offset or grid
/// delta. NaN becomes 0; the clamp bounds sit at the largest magnitude
/// an `f32` still represents exactly.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "clamped to i32's representable range before the cast"
)]
pub const fn i32_of_f32(value: f32) -> i32 {
    if value.is_nan() {
        return 0;
    }
    value.clamp(-2_147_483_520.0, 2_147_483_520.0) as i32
}

/// A `usize` count narrowed to `u32` — a vertex or index-buffer count, a
/// channel offset. Saturates instead of wrapping so a count that somehow
/// exceeded `u32::MAX` would clip visibly rather than alias onto a small
/// number; every real caller is far below the ceiling either way.
#[must_use]
pub fn u32_of_usize(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// [`u32_of_usize`], narrowed to `u16` — a DMX channel or universe
/// count.
#[must_use]
pub fn u16_of_usize(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// [`u32_of_usize`], narrowed to `u8` — a small table index or slot
/// count.
#[must_use]
pub fn u8_of_usize(n: usize) -> u8 {
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// A signed `i32` count narrowed to `u16`, saturating at both ends: a
/// negative value clamps to 0 rather than wrapping to a large positive
/// one (which `as` would silently do), and an over-range positive value
/// clamps to `u16::MAX`.
#[must_use]
pub fn u16_of_i32(n: i32) -> u16 {
    u16::try_from(n).unwrap_or_else(|_| if n.is_negative() { 0 } else { u16::MAX })
}

/// A `u16` narrowed to `u8` — a slot byte already known to fit from the
/// domain that produced it, saturating rather than wrapping if it ever
/// does not.
#[must_use]
pub fn u8_of_u16(n: u16) -> u8 {
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// A `u64` count narrowed to `usize`, saturating on the 32-bit targets
/// where that can matter; on the 64-bit targets this crate ships for it
/// is always exact.
#[must_use]
pub fn usize_of_u64(n: u64) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// A non-negative `i32` widened to `usize` — a GDTF `WheelSlotIndex`,
/// already checked non-negative by its caller. A value that turned out
/// negative anyway saturates to 0 rather than wrapping to a huge index.
#[must_use]
pub fn usize_of_i32(n: i32) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// A `u32` index widened to `usize` — a mesh vertex or face index read
/// off disk. Every target this crate ships for has a `usize` at least as
/// wide as `u32`, so this is exact everywhere in practice; the
/// saturating fallback exists only so the conversion has no `as` to
/// audit, not because it is expected to trigger.
#[must_use]
pub fn usize_of_u32(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// A `u64` count narrowed to `u32` — a scaled video dimension computed
/// in `u64` to keep an intermediate multiply from wrapping. Saturates
/// rather than wraps so a value that somehow exceeded `u32::MAX` would
/// clip visibly; every real caller is a clip dimension, nowhere close.
#[must_use]
pub fn u32_of_u64(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// A `u32` narrowed to `u8` — an encoded test level or small table
/// value, saturating rather than wrapping if it ever does not fit.
#[must_use]
pub fn u8_of_u32(n: u32) -> u8 {
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// An `i64` count as `f64` — a container duration or a decoded
/// timestamp, both in microseconds. `f64`'s 52-bit mantissa (53 with the
/// implicit leading bit) holds every value either one actually produces
/// — nobody exports a clip 285,000 years long — so the reason is written
/// once here rather than at every call site.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "durations and timestamps here are microseconds, nowhere near f64's mantissa limit; see the module doc comment"
)]
pub const fn f64_of_i64(n: i64) -> f64 {
    n as f64
}

/// A float clamped into the range this crate's timestamps actually use,
/// then narrowed to `i64` — a seek target in microseconds. NaN becomes
/// 0; the clamp bounds sit at the largest magnitude an `f64` still
/// represents exactly, which is dozens of orders of magnitude past any
/// real clip length.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "clamped to f64's exactly-representable range before the cast"
)]
pub const fn i64_of_f64(value: f64) -> i64 {
    if value.is_nan() {
        return 0;
    }
    value.clamp(-9_007_199_254_740_992.0, 9_007_199_254_740_992.0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_of_f32_clamps_nan_and_both_ends() {
        assert_eq!(byte_of_f32(f32::NAN), 0);
        assert_eq!(byte_of_f32(-10.0), 0);
        assert_eq!(byte_of_f32(300.0), 255);
        assert_eq!(byte_of_f32(127.6), 128);
    }

    #[test]
    fn usize_of_f32_clamps_nan_and_negative() {
        assert_eq!(usize_of_f32(f32::NAN), 0);
        assert_eq!(usize_of_f32(-1.0), 0);
        assert_eq!(usize_of_f32(3.9), 3);
    }

    #[test]
    fn narrowing_int_helpers_saturate_rather_than_wrap() {
        assert_eq!(u32_of_usize(usize::MAX), u32::MAX);
        assert_eq!(u16_of_usize(usize::MAX), u16::MAX);
        assert_eq!(u8_of_usize(usize::MAX), u8::MAX);
        assert_eq!(u16_of_i32(-5), 0);
        assert_eq!(u16_of_i32(i32::MAX), u16::MAX);
        assert_eq!(u8_of_u16(u16::MAX), u8::MAX);
    }
}
