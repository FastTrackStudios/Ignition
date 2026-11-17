//! The audited numeric conversions the rest of the crate reaches for
//! instead of an inline `as`. `as_conversions` is denied so that a
//! lossy cast cannot appear unexamined in domain code — see
//! `docs/ops/clippy.md`. Each helper here states exactly why its cast
//! is total for the domain it is used in, once, so nothing downstream
//! has to re-derive the argument.

#[cfg(test)]
use ignition_proto::ChanId;

/// A small count — a loop index, a step number, a fixture count — as a
/// float. Every call site is bounded well below the 2^24 where an
/// `f32` stops counting integers exactly (a fixture count, a step
/// index, a sample index in a test), so the cast never actually loses
/// anything.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the doc comment"
)]
pub const fn float_of(n: usize) -> f32 {
    n as f32
}

/// The same audit as [`float_of`], for a `u32` count — a canvas's row
/// or column count, a grid dimension — none of which come near 2^24
/// either.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the doc comment"
)]
pub const fn float_of_u32(n: u32) -> f32 {
    n as f32
}

/// The same audit as [`float_of`], for `f64` — a room coordinate, a
/// generated test fixture's index. `f64`'s 52-bit mantissa has far more
/// headroom than `f32`'s, and every call site here is still a small
/// count.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "counts here are small; see the doc comment"
)]
#[cfg(test)]
pub const fn float64_of(n: usize) -> f64 {
    n as f64
}

/// An `f64` beat or position count narrowed to `f32`, the precision the
/// rest of the timing code works in. A song beat count staying under
/// `f32`'s ~7 significant digits for any show anyone will ever run is
/// the same bet the rest of this crate's clock already makes by being
/// `f32`-typed throughout.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "beat counts here are far below where f32 precision would matter; see the doc comment"
)]
pub const fn f32_of_f64(v: f64) -> f32 {
    v as f32
}

/// A clamped, non-negative float as an index. NaN and anything below
/// zero become 0, the same total-clamp shape as `ignition_proto`'s
/// `usize_of`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a non-negative, in-range value before the cast"
)]
pub fn usize_of(value: f32) -> usize {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    // f32 cannot represent u32::MAX exactly; the nearest representable
    // ceiling below it is enough of a clamp — every real caller is an
    // index or count far smaller than this.
    value.min(4_294_967_040.0) as usize
}

/// A patched channel number from a small loop index. Fixture counts in
/// this crate — grids, test rigs, generated shows — are nowhere near
/// `u32::MAX`, so the cast is total in practice.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "fixture counts here are small; see the doc comment"
)]
#[cfg(test)]
pub const fn chan_of(n: usize) -> ChanId {
    n as ChanId
}

/// A pixel or grid coordinate from a small, non-negative float. Canvas
/// grids in this crate are addressed in the tens, never near
/// `u32::MAX`, so the clamp below is the only bound the cast needs.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a non-negative, grid-sized value before the cast"
)]
pub fn u32_of(value: f32) -> u32 {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    value.min(4_294_967_040.0) as u32
}
