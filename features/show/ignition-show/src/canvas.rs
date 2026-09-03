//! Procedural canvas content, and content that drives fixtures.
//!
//! A canvas is a surface the venue names — three back-wall TVs, a pixel
//! wall, a row of movers — and the viz already plays a clip onto it.
//! This module is the other half of `docs/spec/canvas.md`: content that
//! is *generated* rather than read from a file, and the rule that turns
//! any content into an attribute value at each member's grid position.
//!
//! Everything here is a pure function of `(u, v, cycles)`. There is no
//! clock, no file, no GPU, and nothing random that is not seeded — so the
//! same recipe at the same song position paints the same picture in the
//! viz, in the DMX cooker, and in a test. `cycles` is the effect clock
//! (`Timing::cycles`), so a wipe timed `Speed::Master("Song")` at
//! `measure: 4` crosses the wall once every bar, exactly as a chase
//! would, and scrubs backwards with the transport.

use crate::num::{float_of, float_of_u32, u32_of, usize_of};

/// A wrapping `f32` → `u32`, preserving the original `as i64 as u32`
/// double-cast's behaviour on a negative input: two's-complement wrap,
/// not a clamp to 0. Only ever feeds a hash seed, where any
/// deterministic value is correct.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a deliberate wrap for a hash seed; see the doc comment"
)]
const fn wrapping_u32_of(value: f32) -> u32 {
    value as i64 as u32
}
use crate::step::{Play, SpeedMasters, Timing};
use ignition_proto::{Attribute, ChanId};
use serde::{Deserialize, Serialize};

/// An RGB colour, 0..=1 per channel, linear.
pub type Rgb = [f32; 3];

/// Which way a wipe or band travels across the grid.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Travel {
    /// Along U: left to right at `Play::Forward`.
    #[default]
    Horizontal,
    /// Along V: top to bottom at `Play::Forward`.
    Vertical,
}

/// The generated sources. Each is authored against a canvas *role*, not
/// a panel count, so the same recipe runs on three TVs and on 64 cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl canvas.procedural] - the sources a canvas can show with no file
pub enum Procedural {
    /// Colours spread along `angle_deg` (0 = left to right, 90 = bottom
    /// to top), the whole ramp scrolling one full length per cycle.
    Gradient { colors: Vec<Rgb>, angle_deg: f32 },
    /// One soft bar of `color` on black, `width` as a fraction of the
    /// canvas, crossing once per cycle.
    Wipe {
        color: Rgb,
        width: f32,
        #[serde(default)]
        direction: Travel,
    },
    /// Smooth value noise, `scale` lattice cells across the canvas,
    /// blended through `colors`, drifting one cell per cycle.
    Noise {
        scale: f32,
        seed: u32,
        colors: Vec<Rgb>,
    },
    /// `count` stripes of `color`, each `width` of its own period wide,
    /// scrolling one period per cycle.
    Band {
        color: Rgb,
        width: f32,
        count: u32,
        #[serde(default)]
        direction: Travel,
    },
    /// Random cells lit at `density` (0..=1), re-rolled every cycle, each
    /// fading out across the cycle so it twinkles rather than blinks.
    Sparkle { density: f32, seed: u32, color: Rgb },
    /// A head travelling a serpentine path with a fading tail — the
    /// snake that walks a matrix.
    ///
    /// The path weaves `rows` rows: along the first, back along the
    /// second, and so on, so the head never jumps. `tail` is how much of
    /// the whole path is still glowing behind it, as a fraction — 0.25
    /// is a quarter of the journey lit.
    ///
    /// `rows` is the picture's own, not the rig's. A picture does not
    /// know how many fixtures are looking at it (that is the whole point
    /// of a canvas), so the author says how tightly to weave and the
    /// grid samples whatever that gives it.
    Snake {
        color: Rgb,
        rows: u32,
        tail: f32,
        /// Whether the weave runs along rows or up columns.
        #[serde(default)]
        direction: Travel,
    },
    /// Drops falling down `columns` columns, each on its own offset, each
    /// with a fading tail behind it.
    ///
    /// For a wall of towers this is the obvious one: the picture has a
    /// direction the room actually has, and every column runs its own
    /// clock so it reads as weather rather than as a chase.
    Rain {
        color: Rgb,
        columns: u32,
        tail: f32,
        seed: u32,
    },
    /// One colour everywhere — the "clear to a colour" source.
    Solid(Rgb),
}

/// Which two room axes the picture is painted on.
///
/// The canvas is a unit square and the rig is three-dimensional, so
/// something has to say which plane the square lies in. Until this
/// existed the answer was always X/Y, which is right for a truss seen
/// in plan and wrong for the case that motivated it: a wall of towers
/// varies along X and **Z** and is flat in Y, so the picture's whole
/// vertical axis collapsed to one cell and read the middle of the
/// image. A snake could only ever crawl sideways.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CanvasPlane {
    /// X across, Y upstage — the room seen from above. The default, and
    /// what every canvas recipe written before this meant.
    #[default]
    Plan,
    /// X across, Z up — the room seen from the house. A wall of towers,
    /// a truss of pars, anything hung in a vertical array.
    Wall,
    /// Y upstage, Z up — the room seen from the wing.
    Side,
}

impl CanvasPlane {
    /// The grid axes this plane implies: which room axis is the
    /// picture's U, which is its V, and which is the depth nothing
    /// varies along.
    ///
    /// `tolerance` is left at the default — how far apart two fixtures
    /// have to be to count as different cells is a property of the rig,
    /// not of which way the picture faces.
    #[must_use]
    pub fn axes(self) -> crate::tricks::GridAxes {
        use ignition_rig::selection::Axis;
        let d = crate::tricks::GridAxes::default();
        match self {
            Self::Plan => d,
            Self::Wall => crate::tricks::GridAxes {
                x: Axis::X,
                y: Axis::Z,
                z: Axis::Y,
                ..d
            },
            Self::Side => crate::tricks::GridAxes {
                x: Axis::Y,
                y: Axis::Z,
                z: Axis::X,
                ..d
            },
        }
    }
}

/// A procedural source with the motion that any other recipe has.
///
/// `timing` is the ordinary `Timing`: `speed` (including a named
/// master), `measure`, and `direction` — `Reverse` runs the motion the
/// other way, `Bounce` runs it out and back. Spread and offset are
/// per-fixture properties and have no meaning for one picture, so they
/// are ignored here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl canvas.procedural] - timed like any effect
// r[impl canvas.on-the-stack] - a canvas recipe carries recipe timing, so it can be cooked like one
pub struct CanvasRecipe {
    pub source: Procedural,
    #[serde(default)]
    pub timing: Timing,
    /// Which plane of the room the picture hangs in. Defaults to `Plan`,
    /// so every recipe written before this one means what it always did.
    #[serde(default)]
    pub plane: CanvasPlane,
}

impl CanvasRecipe {
    /// The effect clock at `secs`, honouring `Reverse` and `Bounce`.
    #[must_use]
    pub fn cycles_at(&self, secs: f32, masters: &SpeedMasters) -> f32 {
        let raw = self.timing.cycles_at(secs, 0, 1, masters);
        match self.timing.direction {
            Play::Reverse => -raw,
            _ => raw,
        }
    }

    /// The colour at grid position `(u, v)` — both 0..=1 across the
    /// canvas — after `cycles` of motion.
    // r[impl canvas.procedural] - resolved per member from its grid position
    // r[impl canvas.grid] - the canvas is addressed as a two-axis unit square
    #[must_use]
    pub fn sample(&self, u: f32, v: f32, cycles: f32) -> Rgb {
        let u = u.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        match &self.source {
            Procedural::Solid(c) => *c,
            Procedural::Gradient { colors, angle_deg } => {
                sample_gradient(colors, *angle_deg, u, v, cycles)
            }
            Procedural::Wipe {
                color,
                width,
                direction,
            } => sample_wipe(*color, *width, *direction, u, v, cycles),
            Procedural::Noise {
                scale: cells,
                seed,
                colors,
            } => sample_noise(colors, *cells, *seed, u, v, cycles),
            Procedural::Band {
                color,
                width,
                count,
                direction,
            } => sample_band(*color, *width, *count, *direction, u, v, cycles),
            Procedural::Sparkle {
                density,
                seed,
                color,
            } => sample_sparkle(*color, *density, *seed, u, v, cycles),
            Procedural::Snake {
                color,
                rows,
                tail,
                direction,
            } => sample_snake(*color, *rows, *tail, *direction, u, v, cycles),
            Procedural::Rain {
                color,
                columns,
                tail,
                seed,
            } => sample_rain(*color, *columns, *tail, *seed, u, v, cycles),
        }
    }

    /// Rasterises the source into a tightly packed RGBA8 buffer,
    /// `width * height * 4` bytes, row 0 at the top — what a texture
    /// upload wants.
    // r[impl canvas.clip-is-a-source] - a procedural source yields frames the same shape as a clip
    // `w`/`h` are `width`/`height` clamped to at least one pixel, and
    // `x`/`y`/`u`/`v` are pixel and texture coordinates in the same
    // convention as `sample` above.
    #[expect(
        clippy::many_single_char_names,
        reason = "pixel/texture coordinates in the canvas's own convention; see the comment above"
    )]
    #[must_use]
    pub fn render(&self, width: u32, height: u32, cycles: f32) -> Vec<u8> {
        let (w, h) = (width.max(1), height.max(1));
        // A capacity hint only — a wrong-but-safe estimate under
        // pathological input just costs a reallocation, not a panic.
        let capacity = usize::try_from(w)
            .unwrap_or(usize::MAX)
            .saturating_mul(usize::try_from(h).unwrap_or(usize::MAX))
            .saturating_mul(4);
        let mut out = Vec::with_capacity(capacity);
        for y in 0..h {
            let v = (float_of_u32(y) + 0.5) / float_of_u32(h);
            for x in 0..w {
                let u = (float_of_u32(x) + 0.5) / float_of_u32(w);
                let c = self.sample(u, v, cycles);
                out.extend_from_slice(&[to_u8(c[0]), to_u8(c[1]), to_u8(c[2]), 255]);
            }
        }
        out
    }
}

/// Which of the content's numbers a bitmap channel reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quantity {
    /// Perceptual brightness, 0..=1.
    Brightness,
    /// Hue as a fraction of the wheel, 0..=1.
    Hue,
    Red,
    Green,
    Blue,
}

impl Quantity {
    /// The quantity of `c`, 0..=1.
    #[must_use]
    pub fn of(self, c: Rgb) -> f32 {
        match self {
            Self::Brightness => 0.0722f32.mul_add(c[2], 0.2126f32.mul_add(c[0], 0.7152 * c[1])),
            Self::Hue => hue(c),
            Self::Red => c[0],
            Self::Green => c[1],
            Self::Blue => c[2],
        }
    }
}

/// A picture driving an attribute: the content's `quantity` at a
/// member's grid position, mapped from `low` (content 0) to `high`
/// (content 1), written to `attr`.
///
/// This is what makes a canvas a fixture-grid effect rather than a
/// screen: the same wipe that crosses the TVs can cross a row of movers'
/// tilt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl canvas.bitmap-channels]
pub struct BitmapChannel {
    /// The canvas role this reads from.
    pub canvas: String,
    pub quantity: Quantity,
    pub attr: Attribute,
    pub low: f32,
    pub high: f32,
    /// Emit the mapped value as a relative offset about the cascade's
    /// value rather than an absolute one — a wipe that *adds* tilt to
    /// wherever the movers are aimed. Off by default.
    // r[impl canvas.on-the-stack] - a bitmap channel can modulate like a Delta
    #[serde(default)]
    pub relative: bool,
}

impl BitmapChannel {
    /// The attribute value for a content colour.
    #[must_use]
    pub fn map(&self, c: Rgb) -> f32 {
        (self.high - self.low).mul_add(self.quantity.of(c).clamp(0.0, 1.0), self.low)
    }
}

/// The attribute value each cell gets from `recipe` after `cycles`.
///
/// `cells` is each member's channel and its grid position, 0..=1 on
/// both axes — the output of the rig grid (`tricks` derives it from real
/// positions); this function only reads it. The recipe engine should
/// call this where it expands a canvas recipe over its selection, with
/// the same `cycles` it would hand a phaser, and push the results onto
/// the stack like any other emitted value.
// r[impl canvas.bitmap-channels] - any attribute, from the grid position
// r[impl canvas.grid] - members addressed by grid position, not by index
#[must_use]
pub fn sample_for_grid(
    recipe: &CanvasRecipe,
    channel: &BitmapChannel,
    cells: &[(ChanId, f32, f32)],
    cycles: f32,
) -> Vec<(ChanId, Attribute, f32)> {
    cells
        .iter()
        .map(|&(chan, u, v)| {
            let c = recipe.sample(u, v, cycles);
            (chan, channel.attr.clone(), channel.map(c))
        })
        .collect()
}

// ---- helpers ----------------------------------------------------------

fn frac(t: f32) -> f32 {
    t - t.floor()
}

const fn along(u: f32, v: f32, d: Travel) -> f32 {
    match d {
        Travel::Horizontal => u,
        Travel::Vertical => v,
    }
}

/// Shortest distance between two positions on a unit ring.
fn wrapped_distance(a: f32, b: f32) -> f32 {
    let d = frac(a - b);
    d.min(1.0 - d)
}

// `u`/`v` below are the canvas's own texture-coordinate convention (see
// `Content`'s doc comment); the one-letter locals derived from them per
// source (`s`/`c` for a sine/cosine pair, `t` for a scalar position) are
// the standard names for that maths, and spelling them out would make
// the trig harder to read, not easier.
#[expect(
    clippy::many_single_char_names,
    reason = "u/v texture coordinates and their derived sin/cos/position locals; see the comment above"
)]
fn sample_gradient(colors: &[Rgb], angle_deg: f32, u: f32, v: f32, cycles: f32) -> Rgb {
    let (s, c) = angle_deg.to_radians().sin_cos();
    // Project onto the angle, with V up so 90° reads bottom-to-top, then
    // scroll.
    let t = (u - 0.5).mul_add(c, (0.5 - v) * s) + 0.5 + cycles;
    ramp(colors, frac(t))
}

fn sample_wipe(color: Rgb, width: f32, direction: Travel, u: f32, v: f32, cycles: f32) -> Rgb {
    let pos = along(u, v, direction);
    let head = frac(cycles);
    let width = width.max(1e-3);
    // Distance from the bar's centre, wrapped so the bar re-enters the
    // far edge as it leaves the near one.
    let dist = wrapped_distance(pos, head);
    let alpha = (1.0 - dist / (width * 0.5)).clamp(0.0, 1.0);
    scale(color, alpha)
}

fn sample_noise(colors: &[Rgb], cells: f32, seed: u32, u: f32, v: f32, cycles: f32) -> Rgb {
    let cells = cells.max(1e-3);
    let n = value_noise(u.mul_add(cells, cycles), v * cells, seed);
    ramp(colors, n)
}

fn sample_band(
    color: Rgb,
    width: f32,
    count: u32,
    direction: Travel,
    u: f32,
    v: f32,
    cycles: f32,
) -> Rgb {
    let count = float_of_u32(count.max(1));
    let pos = along(u, v, direction);
    let phase = frac(pos.mul_add(count, -cycles));
    let a = if phase < width.clamp(0.0, 1.0) {
        1.0
    } else {
        0.0
    };
    scale(color, a)
}

fn sample_sparkle(color: Rgb, density: f32, seed: u32, u: f32, v: f32, cycles: f32) -> Rgb {
    // A fixed 32×18 field of cells, so a sparkle has a size on a big
    // wall rather than being one pixel.
    let gx = u32_of((u * 32.0).floor());
    let gy = u32_of((v * 18.0).floor());
    // Unlike `gx`/`gy`, a negative `cycles` (a clock seeked before its
    // start) is meant to wrap rather than clamp to 0 — it is only a
    // seed for the sparkle hash below, so any deterministic value
    // works, and wrapping keeps the seed still varying pass to pass on
    // the negative side instead of collapsing every negative cycle to
    // the same field.
    let pass = wrapping_u32_of(cycles.floor());
    let roll = hash3(gx, gy, seed.wrapping_add(pass.wrapping_mul(0x9E37_79B9)));
    let lit = unit(roll) < density.clamp(0.0, 1.0);
    if lit {
        scale(color, 1.0 - frac(cycles))
    } else {
        [0.0; 3]
    }
}

// Where this cell sits along the weave, 0..1, then how long ago the
// head passed it.
fn sample_snake(
    color: Rgb,
    rows: u32,
    tail: f32,
    direction: Travel,
    u: f32,
    v: f32,
    cycles: f32,
) -> Rgb {
    let rows = rows.max(1);
    // Along the weave and across it — swapping the two is what
    // `direction` means, so one picture does both a snake that crawls
    // in rows and one that climbs columns.
    let (across, along_row) = match direction {
        Travel::Horizontal => (v, u),
        Travel::Vertical => (u, v),
    };
    let row = u32_of((across * float_of_u32(rows)).floor()).min(rows.saturating_sub(1));
    // Odd rows run backwards, which is what makes it a serpentine
    // rather than a carriage return: the head leaves one row where it
    // enters the next.
    let along_row = if row.is_multiple_of(2) {
        along_row
    } else {
        1.0 - along_row
    };
    let s = (float_of_u32(row) + along_row) / float_of_u32(rows);
    let behind = frac(cycles - s);
    let tail = tail.clamp(1e-4, 1.0);
    let a = if behind <= tail {
        1.0 - behind / tail
    } else {
        0.0
    };
    scale(color, a)
}

fn sample_rain(color: Rgb, columns: u32, tail: f32, seed: u32, u: f32, v: f32, cycles: f32) -> Rgb {
    let columns = columns.max(1);
    let col = u32_of((u * float_of_u32(columns)).floor()).min(columns.saturating_sub(1));
    // Each column on its own offset, so the wall reads as weather
    // rather than as one bar falling.
    let offset = unit(hash3(col, 0, seed));
    // Falling: the drop is at the top when its phase is 0.
    let drop = frac(cycles + offset);
    let behind = frac(v - drop);
    let tail = tail.clamp(1e-4, 1.0);
    let a = if behind <= tail {
        1.0 - behind / tail
    } else {
        0.0
    };
    scale(color, a)
}

fn scale(c: Rgb, a: f32) -> Rgb {
    [c[0] * a, c[1] * a, c[2] * a]
}

/// The one place a canvas sample becomes an RGBA8 byte. The clamp makes
/// the cast total the same way `ignition_proto`'s DMX `byte` does.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the clamp above makes this cast total; see the doc comment"
)]
const fn to_u8(x: f32) -> u8 {
    x.clamp(0.0, 1.0).mul_add(255.0, 0.5) as u8
}

/// Linear interpolation through a colour list at `t` in 0..=1, wrapping
/// so a scrolling ramp has no seam.
fn ramp(colors: &[Rgb], t: f32) -> Rgb {
    match colors.len() {
        0 => [0.0; 3],
        1 => colors.first().copied().unwrap_or([0.0; 3]),
        n => {
            let pos = frac(t) * float_of(n);
            let lo = usize_of(pos.floor()).checked_rem(n).unwrap_or(0);
            let hi = lo.saturating_add(1).checked_rem(n).unwrap_or(0);
            let blend = pos - pos.floor();
            // `lo` and `hi` are both reduced modulo `colors.len()`
            // above, so they are in range whenever `colors` is
            // non-empty (this arm only runs when it is) — the fallback
            // to black can never actually trigger, it only satisfies
            // the lint.
            let (from, to) = (
                colors.get(lo).copied().unwrap_or([0.0; 3]),
                colors.get(hi).copied().unwrap_or([0.0; 3]),
            );
            [
                (to[0] - from[0]).mul_add(blend, from[0]),
                (to[1] - from[1]).mul_add(blend, from[1]),
                (to[2] - from[2]).mul_add(blend, from[2]),
            ]
        }
    }
}

#[expect(
    clippy::float_cmp,
    reason = "max is exactly one of c[0..3] by construction — f32::max never rounds — so this is an identity check, not a comparison of two independently computed values"
)]
fn hue(c: Rgb) -> f32 {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    let d = max - min;
    if d <= 1e-6 {
        return 0.0;
    }
    let h = if max == c[0] {
        (c[1] - c[2]) / d
    } else if max == c[1] {
        2.0 + (c[2] - c[0]) / d
    } else {
        4.0 + (c[0] - c[1]) / d
    };
    frac(h / 6.0)
}

/// A small integer hash — deterministic on every platform, which is the
/// whole reason there is no `rand` here.
fn hash3(x: u32, y: u32, seed: u32) -> u32 {
    let mut h = seed ^ 0x8F1B_BCDC;
    for k in [x, y] {
        h ^= k.wrapping_mul(0x9E37_79B9);
        h = h.rotate_left(13).wrapping_mul(0x85EB_CA6B);
        h ^= h >> 16;
    }
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^ (h >> 16)
}

/// A hash's top 24 bits as a fraction in `[0, 1)`. `h >> 8` fits in 24
/// bits by construction, and `1u32 << 24` is exactly representable, so
/// both casts are exact rather than lossy.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "both operands fit an f32 exactly; see the doc comment"
)]
fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Bilinear value noise on an integer lattice, smoothstepped.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (
        fx * fx * 2.0f32.mul_add(-fx, 3.0),
        fy * fy * 2.0f32.mul_add(-fy, 3.0),
    );
    let (ix, iy) = (wrapping_u32_of(x0), wrapping_u32_of(y0));
    let at = |dx: u32, dy: u32| unit(hash3(ix.wrapping_add(dx), iy.wrapping_add(dy), seed));
    let top = (at(1, 0) - at(0, 0)).mul_add(sx, at(0, 0));
    let bottom = (at(1, 1) - at(0, 1)).mul_add(sx, at(0, 1));
    (bottom - top).mul_add(sy, top)
}

/// Named recipes an operator can ask for without writing JSON.
// r[impl canvas.procedural] - a colour sweep needs no file and no JSON
#[must_use]
pub fn named(name: &str) -> Option<CanvasRecipe> {
    let song = || Timing {
        speed: crate::step::Speed::Master("Song".into()),
        measure: 4.0,
        ..Timing::default()
    };
    let source = match name {
        "rainbow" => Procedural::Gradient {
            colors: vec![
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 1.0, 1.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
            ],
            angle_deg: 0.0,
        },
        "wipe" => Procedural::Wipe {
            color: [1.0; 3],
            width: 0.25,
            direction: Travel::Horizontal,
        },
        "noise" => Procedural::Noise {
            scale: 4.0,
            seed: 1,
            colors: vec![[0.0, 0.0, 0.3], [0.0, 0.6, 1.0], [1.0; 3]],
        },
        "bands" => Procedural::Band {
            color: [1.0, 0.5, 0.0],
            width: 0.5,
            count: 4,
            direction: Travel::Horizontal,
        },
        "sparkle" => Procedural::Sparkle {
            density: 0.1,
            seed: 7,
            color: [1.0; 3],
        },
        _ => return None,
    };
    Some(CanvasRecipe {
        source,
        timing: song(),
        plane: CanvasPlane::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num::chan_of;
    use crate::step::Speed;

    fn still(source: Procedural) -> CanvasRecipe {
        CanvasRecipe {
            source,
            timing: Timing::default(),
            plane: CanvasPlane::default(),
        }
    }

    fn close(a: Rgb, b: Rgb) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    /// A left-to-right two-colour gradient is the first colour at the
    /// left edge and, one seam away, back to it at the right; the
    /// second colour sits in the middle.
    #[test]
    /// r[verify canvas.procedural]
    fn gradient_endpoints_are_its_colours() {
        let g = still(Procedural::Gradient {
            colors: vec![[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            angle_deg: 0.0,
        });
        assert!(close(g.sample(0.0, 0.5, 0.0), [1.0, 0.0, 0.0]));
        assert!(close(g.sample(0.5, 0.5, 0.0), [0.0, 0.0, 1.0]));
        // Quarter way is a half blend.
        assert!(close(g.sample(0.25, 0.5, 0.0), [0.5, 0.0, 0.5]));
    }

    /// The bar is wherever the clock says: at a quarter cycle it is a
    /// quarter of the way across, and the left edge is dark.
    #[test]
    /// r[verify canvas.procedural]
    fn a_wipe_moves_with_cycles() {
        let w = still(Procedural::Wipe {
            color: [1.0; 3],
            width: 0.1,
            direction: Travel::Horizontal,
        });
        assert!(close(w.sample(0.0, 0.5, 0.0), [1.0; 3]));
        assert!(close(w.sample(0.25, 0.5, 0.0), [0.0; 3]));
        assert!(close(w.sample(0.25, 0.5, 0.25), [1.0; 3]));
        assert!(close(w.sample(0.0, 0.5, 0.25), [0.0; 3]));
    }

    /// Reverse runs the same wipe the other way, off the same clock.
    #[test]
    /// r[verify canvas.procedural]
    fn reverse_runs_the_motion_backwards() {
        let mut w = still(Procedural::Wipe {
            color: [1.0; 3],
            width: 0.1,
            direction: Travel::Horizontal,
        });
        w.timing.speed = Speed::Hz(1.0);
        w.timing.direction = Play::Reverse;
        let masters = SpeedMasters::new();
        let c = w.cycles_at(0.25, &masters);
        assert!(close(w.sample(0.75, 0.5, c), [1.0; 3]));
    }

    /// Two renders of the same seeded sparkle are byte-identical, and a
    /// different seed is a different picture.
    #[test]
    /// r[verify canvas.procedural]
    fn sparkle_is_deterministic() {
        let a = still(Procedural::Sparkle {
            density: 0.3,
            seed: 5,
            color: [1.0; 3],
        });
        let b = still(Procedural::Sparkle {
            density: 0.3,
            seed: 6,
            color: [1.0; 3],
        });
        assert_eq!(a.render(64, 36, 2.0), a.render(64, 36, 2.0));
        assert_ne!(a.render(64, 36, 2.0), b.render(64, 36, 2.0));
        // And something is actually lit.
        assert!(a.render(64, 36, 0.0).iter().any(|&p| p > 0));
    }

    #[test]
    /// r[verify canvas.clip-is-a-source]
    fn render_is_rgba_of_the_requested_size() {
        let s = still(Procedural::Solid([0.0, 1.0, 0.0]));
        let px = s.render(320, 90, 0.0);
        assert_eq!(px.len(), 320 * 90 * 4);
        assert_eq!(&px[..4], &[0, 255, 0, 255]);
    }

    #[test]
    fn noise_stays_in_range_and_is_seeded() {
        let n = still(Procedural::Noise {
            scale: 3.0,
            seed: 9,
            colors: vec![[0.0; 3], [1.0; 3]],
        });
        for i in 0..50 {
            let c = n.sample(float_of(i) / 50.0, 0.3, 1.7);
            assert!((0.0..=1.0).contains(&c[0]));
        }
        assert_eq!(n.render(16, 16, 0.5), n.render(16, 16, 0.5));
    }

    /// The core case from the spec: a wipe across a 4×1 row of fixtures,
    /// read as dimmer. The bright cell walks along the row as the clock
    /// advances.
    #[test]
    /// r[verify canvas.bitmap-channels]
    /// r[verify canvas.grid]
    fn a_wipe_walks_along_a_row_of_fixtures() {
        let recipe = still(Procedural::Wipe {
            color: [1.0; 3],
            width: 0.3,
            direction: Travel::Horizontal,
        });
        let chan = BitmapChannel {
            canvas: "wall".into(),
            quantity: Quantity::Brightness,
            attr: Attribute::Dimmer,
            low: 0.0,
            high: 100.0,
            relative: false,
        };
        let cells: Vec<(ChanId, f32, f32)> = (0..4)
            .map(|i| (chan_of(i) + 1, (float_of(i) + 0.5) / 4.0, 0.5))
            .collect();
        let brightest = |cycles: f32| {
            sample_for_grid(&recipe, &chan, &cells, cycles)
                .into_iter()
                .max_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(chan, attr, val)| {
                    assert_eq!(attr, Attribute::Dimmer);
                    assert!(val > 50.0 && val <= 100.0, "{val}");
                    chan
                })
                .unwrap()
        };
        assert_eq!(brightest(0.125), 1);
        assert_eq!(brightest(0.375), 2);
        assert_eq!(brightest(0.625), 3);
        assert_eq!(brightest(0.875), 4);
    }

    /// Any attribute, not just dimmer — hue driving tilt between two
    /// angles.
    #[test]
    /// r[verify canvas.bitmap-channels]
    fn a_quantity_maps_onto_any_attribute_range() {
        let recipe = still(Procedural::Solid([0.0, 1.0, 0.0]));
        let chan = BitmapChannel {
            canvas: "movers".into(),
            quantity: Quantity::Hue,
            attr: Attribute::Tilt,
            low: -30.0,
            high: 30.0,
            relative: false,
        };
        let out = sample_for_grid(&recipe, &chan, &[(1, 0.5, 0.5)], 0.0);
        // Green is a third of the way round the wheel.
        assert_eq!(out[0].1, Attribute::Tilt);
        assert!(
            (out[0].2 - (-30.0 + 60.0 / 3.0)).abs() < 1e-3,
            "{}",
            out[0].2
        );
    }

    #[test]
    fn a_recipe_round_trips_through_json() {
        let r = named("rainbow").unwrap();
        let json = serde_json::to_string(&r).unwrap();
        let back: CanvasRecipe = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
        assert!(named("nope").is_none());
    }
}
