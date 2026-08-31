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
    /// One colour everywhere — the "clear to a colour" source.
    Solid(Rgb),
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
}

impl CanvasRecipe {
    /// The effect clock at `secs`, honouring `Reverse` and `Bounce`.
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
    pub fn sample(&self, u: f32, v: f32, cycles: f32) -> Rgb {
        let u = u.clamp(0.0, 1.0);
        let v = v.clamp(0.0, 1.0);
        match &self.source {
            Procedural::Solid(c) => *c,
            Procedural::Gradient { colors, angle_deg } => {
                let (s, c) = angle_deg.to_radians().sin_cos();
                // Project onto the angle, with V up so 90° reads
                // bottom-to-top, then scroll.
                let t = ((u - 0.5) * c + (0.5 - v) * s) + 0.5 + cycles;
                ramp(colors, frac(t))
            }
            Procedural::Wipe {
                color,
                width,
                direction,
            } => {
                let pos = along(u, v, *direction);
                let head = frac(cycles);
                let w = width.max(1e-3);
                // Distance from the bar's centre, wrapped so the bar
                // re-enters the far edge as it leaves the near one.
                let d = wrapped_distance(pos, head);
                let a = (1.0 - d / (w * 0.5)).clamp(0.0, 1.0);
                scale(*color, a)
            }
            Procedural::Noise {
                scale: cells,
                seed,
                colors,
            } => {
                let cells = cells.max(1e-3);
                let n = value_noise(u * cells + cycles, v * cells, *seed);
                ramp(colors, n)
            }
            Procedural::Band {
                color,
                width,
                count,
                direction,
            } => {
                let count = (*count).max(1) as f32;
                let pos = along(u, v, *direction);
                let phase = frac(pos * count - cycles);
                let a = if phase < width.clamp(0.0, 1.0) {
                    1.0
                } else {
                    0.0
                };
                scale(*color, a)
            }
            Procedural::Sparkle {
                density,
                seed,
                color,
            } => {
                // A fixed 32×18 field of cells, so a sparkle has a size
                // on a big wall rather than being one pixel.
                let gx = (u * 32.0).floor() as u32;
                let gy = (v * 18.0).floor() as u32;
                let pass = cycles.floor() as i64 as u32;
                let roll = hash3(gx, gy, seed.wrapping_add(pass.wrapping_mul(0x9E37_79B9)));
                let lit = unit(roll) < density.clamp(0.0, 1.0);
                if lit {
                    scale(*color, 1.0 - frac(cycles))
                } else {
                    [0.0; 3]
                }
            }
        }
    }

    /// Rasterises the source into a tightly packed RGBA8 buffer,
    /// `width * height * 4` bytes, row 0 at the top — what a texture
    /// upload wants.
    // r[impl canvas.clip-is-a-source] - a procedural source yields frames the same shape as a clip
    pub fn render(&self, width: u32, height: u32, cycles: f32) -> Vec<u8> {
        let (w, h) = (width.max(1), height.max(1));
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let v = (y as f32 + 0.5) / h as f32;
            for x in 0..w {
                let u = (x as f32 + 0.5) / w as f32;
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
    pub fn of(self, c: Rgb) -> f32 {
        match self {
            Quantity::Brightness => 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2],
            Quantity::Hue => hue(c),
            Quantity::Red => c[0],
            Quantity::Green => c[1],
            Quantity::Blue => c[2],
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
    pub fn map(&self, c: Rgb) -> f32 {
        self.low + (self.high - self.low) * self.quantity.of(c).clamp(0.0, 1.0)
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

fn along(u: f32, v: f32, d: Travel) -> f32 {
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

fn scale(c: Rgb, a: f32) -> Rgb {
    [c[0] * a, c[1] * a, c[2] * a]
}

fn to_u8(x: f32) -> u8 {
    (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Linear interpolation through a colour list at `t` in 0..=1, wrapping
/// so a scrolling ramp has no seam.
fn ramp(colors: &[Rgb], t: f32) -> Rgb {
    match colors.len() {
        0 => [0.0; 3],
        1 => colors[0],
        n => {
            let x = frac(t) * n as f32;
            let i = x.floor() as usize % n;
            let j = (i + 1) % n;
            let f = x - x.floor();
            let (a, b) = (colors[i], colors[j]);
            [
                a[0] + (b[0] - a[0]) * f,
                a[1] + (b[1] - a[1]) * f,
                a[2] + (b[2] - a[2]) * f,
            ]
        }
    }
}

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

fn unit(h: u32) -> f32 {
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Bilinear value noise on an integer lattice, smoothstepped.
fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (ix, iy) = (x0 as i64 as u32, y0 as i64 as u32);
    let at = |dx: u32, dy: u32| unit(hash3(ix.wrapping_add(dx), iy.wrapping_add(dy), seed));
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * sx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * sx;
    top + (bottom - top) * sy
}

/// Named recipes an operator can ask for without writing JSON.
// r[impl canvas.procedural] - a colour sweep needs no file and no JSON
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::step::Speed;

    fn still(source: Procedural) -> CanvasRecipe {
        CanvasRecipe {
            source,
            timing: Timing::default(),
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
            let c = n.sample(i as f32 / 50.0, 0.3, 1.7);
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
            .map(|i| (i as ChanId + 1, (i as f32 + 0.5) / 4.0, 0.5))
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
