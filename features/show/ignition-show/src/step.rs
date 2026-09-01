//! Steps and timing — the half of a recipe that makes it a phaser.
//!
//! A recipe with **one** step is a static parametric look. A recipe with
//! **two or more** is a phaser: the same object, continuously
//! interpolated. That is grandMA3's own framing and it is why there is no
//! separate effect type here — see
//! `docs/domain/cue-building-architecture.md`, Decision 3.
//!
//! The consequence worth stating plainly: because a phaser *is* a recipe,
//! `Speed::Master` applies to every recipe uniformly. On MA3 a speed
//! master can drive a phaser but not a recipe-driven effect; the two
//! systems only partly meet. Here one tap-tempo source drives everything
//! in the show, with no special case and no second code path.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// How a step moves toward its own values during its transition.
///
/// This is MA3's Accel/Decel in its "Proportional" mode, and it is what
/// lets a two-step recipe be a *sine* rather than a triangle — which in
/// turn is why `Waveform` can be sugar over steps instead of a parallel
/// engine.
///
/// `Curve` is the general form: `accel` shapes how the move *leaves* the
/// previous value and `decel` how it *arrives* at this one, each −1…+1
/// (MA3's −100…+200 normalised). Negative is softer than linear — the
/// slope at that end goes to zero at −1 — and positive is harder, twice
/// the linear slope at +1. `Linear` and `Sine` are kept as the two
/// spellings everyone reaches for; `Sine` is exactly `Curve` at −1/−1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
// r[impl effects.step.ease]
// r[impl effects.step.accel-decel]
pub enum Ease {
    #[default]
    Linear,
    /// Eases in and out — a full cycle of two `Sine` steps at
    /// `transition: 1.0` is a sine wave. Sugar for `Curve { -1, -1 }`.
    Sine,
    /// `accel` bends the departure, `decel` the arrival; positive is
    /// harder, negative softer. "Hit hard, fall soft" is `+1 / −1`.
    Curve { accel: f32, decel: f32 },
}

impl Ease {
    /// The general form of this ease, so the two named ones are visibly
    /// sugar rather than a second code path.
    // r[impl effects.step.accel-decel] - Linear and Sine remain expressible as sugar
    pub fn as_curve(self) -> (f32, f32) {
        match self {
            Ease::Linear => (0.0, 0.0),
            Ease::Sine => (-1.0, -1.0),
            Ease::Curve { accel, decel } => (accel.clamp(-1.0, 1.0), decel.clamp(-1.0, 1.0)),
        }
    }

    /// Progress through the transition, shaped: 0 at the start, 1 at
    /// the end, monotone between.
    ///
    /// The shape is the cosine ease plus two Hermite tangent corrections,
    /// so the slope leaving is exactly `1 + accel` and the slope arriving
    /// exactly `1 + decel`. That construction is what makes `Sine` land
    /// on the real cosine at −1/−1 rather than on a polynomial that
    /// merely looks like one, and both corrections vanish at the
    /// endpoints so they are always hit exactly. With tangents in 0…2
    /// against a secant of 1 the curve cannot turn back on itself.
    // r[impl effects.step.accel-decel] - the shaping
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            _ => {
                let (accel, decel) = self.as_curve();
                let soft = (1.0 - (t * std::f32::consts::PI).cos()) * 0.5;
                let leave = t * t * t - 2.0 * t * t + t;
                let arrive = t * t * t - t * t;
                (soft + (1.0 + accel) * leave + (1.0 + decel) * arrive).clamp(0.0, 1.0)
            }
        }
    }
}

fn one() -> f32 {
    1.0
}

/// One step of a recipe: what to apply, and how long it owns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl effects.step]
pub struct Step {
    /// What this step sets. A step can set several things at once — a
    /// colour *and* a level — which is what makes "tap through four
    /// looks to build a four-step chase" a sane authoring model.
    pub apply: Vec<crate::recipe::RecipeApply>,
    /// This step's share of one cycle, relative to the other steps.
    /// Widths are normalised, so `[1, 1]` and `[3, 3]` are the same
    /// thing and `[3, 1]` holds the first step three times as long.
    #[serde(default = "one")]
    // r[impl effects.step.width]
    pub width: f32,
    /// Fraction of `width` spent moving toward this step's values rather
    /// than holding them. `0.0` snaps — a classic chase. `1.0` never
    /// stops moving.
    #[serde(default)]
    // r[impl effects.step.transition]
    pub transition: f32,
    #[serde(default)]
    // r[impl effects.step.ease]
    pub ease: Ease,
}

impl Step {
    pub fn new(apply: Vec<crate::recipe::RecipeApply>) -> Self {
        Self {
            apply,
            width: 1.0,
            transition: 0.0,
            ease: Ease::default(),
        }
    }
}

/// Named tempo sources — tap tempo, a fader, or the session tempo map
/// from the FastTrackStudio side. Values are BPM.
// r[impl effects.masters.registry]
pub type SpeedMasters = HashMap<String, f32>;

/// How fast a phaser runs, in whichever unit the operator thinks in.
///
/// All four resolve to *beats per second*; `Timing::measure` says how
/// many beats one full loop of the step list takes, so speed and measure
/// together fix the real-world duration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl effects.speed]
// r[impl recipes.timing-in-musical-terms]
// r[impl effects.masters.registry] - referenced by name
pub enum Speed {
    /// Cycles per second when `measure` is 1 — the plain "how fast".
    Hz(f32),
    Bpm(f32),
    /// Seconds per beat.
    Secs(f32),
    /// Slaved to a named master.
    ///
    /// A name nobody has set runs at `FALLBACK_BPM` and is reported
    /// against every cue that asked for it, rather than freezing —
    /// `r[effects.masters.unknown]`, which weighed the other way round
    /// and chose this. A frozen chase on a stage is indistinguishable
    /// from a look that was meant to be still; a chase at the wrong
    /// tempo is at least obviously running.
    Master(String),
    /// A named master at a multiple — half, double, ×3 — without a
    /// second master. The strobe runs double-time off the same tap the
    /// movers halve against, and a tempo change carries both.
    // r[impl playback.speed-scale]
    Scaled {
        master: String,
        scale: f32,
    },
}

// r[impl effects.speed.default-is-still]
impl Default for Speed {
    /// Static. A one-step recipe never asks, and a multi-step recipe
    /// that forgot to say holds still rather than picking a tempo for
    /// the operator.
    fn default() -> Self {
        Speed::Hz(0.0)
    }
}

impl Speed {
    /// The tempo an unset speed master runs at.
    ///
    /// Deliberately ordinary: fast enough that an envelope completes
    /// promptly, slow enough that a chase which lost its master looks
    /// wrong rather than looking broken.
    // r[impl effects.masters.unknown] - the fallback tempo
    pub const FALLBACK_BPM: f32 = 120.0;

    // r[impl effects.speed] - all four units resolve to one rate
    // r[impl effects.masters.uniform] - one code path for every recipe
    // r[impl effects.masters.unknown] - unset or zero master falls back rather than freezing
    pub fn beats_per_second(&self, masters: &SpeedMasters) -> f32 {
        match self {
            Speed::Hz(h) => *h,
            Speed::Bpm(b) => b / 60.0,
            Speed::Secs(s) if *s > 0.0 => 1.0 / s,
            Speed::Secs(_) => 0.0,
            // A master nobody has set falls back to a plausible tempo
            // rather than to *stopped*, and the difference is a safety
            // property rather than a nicety. Zero freezes anything slaved
            // to it — which for a looping chase is merely still, and for
            // a **one-shot is stuck at its lift**: a flash key fired
            // before a transport loaded would hold the rig bright with no
            // way to release it, which is precisely the failure one-shots
            // exist to make impossible.
            //
            // A missing master is still reported — `recipe::unresolved`
            // names it against every cue that asked for it — so this
            // does not hide the mistake. It stops the mistake being a
            // stuck light.
            Speed::Master(name) => Self::master_rate(name, masters),
            // A non-positive scale is a typo, not a request to freeze:
            // the same stuck-one-shot argument as an unset master.
            // r[impl playback.speed-scale] - a multiple of the master's rate
            Speed::Scaled { master, scale } => {
                let scale = if *scale > 0.0 { *scale } else { 1.0 };
                Self::master_rate(master, masters) * scale
            }
        }
    }

    /// The master this speed is slaved to, if it is.
    // r[impl playback.speed-scale] - a scaled speed still names one master
    pub fn master(&self) -> Option<&str> {
        match self {
            Speed::Master(name) | Speed::Scaled { master: name, .. } => Some(name),
            _ => None,
        }
    }

    fn master_rate(name: &str, masters: &SpeedMasters) -> f32 {
        let bpm = masters.get(name).copied().unwrap_or(0.0);
        (if bpm > 0.0 { bpm } else { Self::FALLBACK_BPM }) / 60.0
    }
}

/// How a step list is played across the selection.
///
/// Eos gets six visibly different effects out of one step table this
/// way, and it is the cheapest expressiveness in the whole engine —
/// their own docs call `Negative` "the one most people never build",
/// which on a full wash gives a dark gap travelling across the stage,
/// "something no amount of forward chasing does".
///
/// Note that `Selection::Order` (see `selection.rs`) is the better tool
/// for *direction*: it reorders by real position, so "left to right" is
/// a fact about the room rather than about how a group was recorded.
/// Eos cannot do that — its channel order comes from group storage
/// order, which it admits is not even readable back over OSC. `Reverse`
/// here is for when the selection order is already the one you meant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
// r[impl effects.play]
// r[impl effects.play.is-not-direction]
pub enum Play {
    #[default]
    Forward,
    /// The other end of the selection leads.
    #[serde(alias = "Backward")]
    Reverse,
    /// Runs the list out and back — 1,2,3,4,3,2,1 — so the wave washes
    /// rather than snapping back at the wrap.
    Bounce,
    /// Each fixture arrives and stays until the cycle wraps, so the
    /// selection fills up and resets. Not a phase shift of a shared
    /// waveform, which is why it is a mode rather than a step table.
    Build,
    /// Inverted: fixtures sit at the top of the swing and the travelling
    /// point is the one that drops out.
    Negative,
}

/// Kept as the old name so existing show files keep parsing.
pub type Direction = Play;

/// The phaser-level layers — shared across every step, which is what
/// makes "eight fixtures phase-offset around one definition" cheap
/// instead of eight authored step tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl effects.timing.uniform]
pub struct Timing {
    #[serde(default)]
    pub speed: Speed,
    /// Beats per full loop of the step list.
    #[serde(default = "one")]
    // r[impl effects.measure]
    pub measure: f32,
    /// How much of one cycle is spread across the selection, in its
    /// order. 360 is a classic chase — each fixture visibly offset from
    /// its neighbour. 0 is lockstep: a pulse rather than a chase.
    #[serde(default)]
    // r[impl effects.phase.spread]
    pub phase_spread_deg: f32,
    /// The same spread along the selection grid's **Y** axis — across
    /// the trusses rather than along them. Zero, the default, is the
    /// one-dimensional behaviour every existing show has; with X and Y
    /// both set a chase runs diagonally. Lands on the units' grid
    /// positions (`tricks::UnitPos`), so it is nothing on a rig where Y
    /// does not vary.
    #[serde(default, skip_serializing_if = "is_zero")]
    // r[impl effects.phase.spread]
    // r[impl tricks.grid] - phase spread per axis
    pub phase_spread_y_deg: f32,
    /// The same along **Z** — height.
    #[serde(default, skip_serializing_if = "is_zero")]
    // r[impl effects.phase.spread]
    // r[impl tricks.grid] - phase spread per axis
    pub phase_spread_z_deg: f32,
    /// A fixed shift applied to every fixture equally. Pairing two
    /// recipes on Pan and Tilt 90 degrees apart is how a circle happens,
    /// without a dedicated "position effect" type: two sine waves a
    /// quarter-cycle apart *is* a circle.
    #[serde(default)]
    // r[impl effects.phase.offset]
    pub phase_offset_deg: f32,
    #[serde(default)]
    pub direction: Direction,
    /// Run the step list once and hold on the last step, instead of
    /// looping.
    ///
    /// What makes a *bump* expressible. Without it the only way to flash
    /// and come back was two cues — one to lift, one to release a beat
    /// later — which doubles the cue list and says nothing a reader
    /// wants to know: half the cue names in a show became "… out". A
    /// bump is one event with a shape, and this lets the shape live in
    /// the recipe where it belongs.
    ///
    /// Holding rather than stopping matters. A one-shot that wrapped
    /// would strobe; one that vanished would leave the fixture wherever
    /// the last frame caught it. Held on the final step, an envelope
    /// ending at zero simply stops contributing, which for a `Delta` is
    /// exactly nothing.
    #[serde(default)]
    // r[impl effects.once]
    // r[impl recipes.one-shot]
    pub once: bool,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            speed: Speed::default(),
            measure: 1.0,
            phase_spread_deg: 0.0,
            phase_spread_y_deg: 0.0,
            phase_spread_z_deg: 0.0,
            phase_offset_deg: 0.0,
            direction: Direction::default(),
            once: false,
        }
    }
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

impl Timing {
    /// Cycles elapsed after `secs`, before any per-fixture spread.
    // r[impl effects.measure]
    pub fn cycles(&self, secs: f32, masters: &SpeedMasters) -> f32 {
        let measure = if self.measure > 0.0 {
            self.measure
        } else {
            1.0
        };
        secs * self.speed.beats_per_second(masters) / measure
    }

    /// Where a fixture sits in the selection, 0 at the leading end.
    // r[impl effects.phase.in-selection-order]
    // r[impl effects.play] - Reverse mirrors the spread fraction
    pub fn spread_fraction(&self, index: usize, count: usize) -> f32 {
        let f = if count > 1 {
            index as f32 / count as f32
        } else {
            0.0
        };
        match self.direction {
            Play::Reverse => 1.0 - f,
            _ => f,
        }
    }

    /// The phase offset, in **cycles**, of a unit at `pos` in the grid:
    /// each axis's fraction (0 at the leading end, `i / count` after,
    /// mirrored under `Reverse` exactly as [`Timing::spread_fraction`]
    /// is) times that axis's spread in degrees, summed, over 360.
    ///
    /// On a `[n, 1, 1]` grid with only `phase_spread_deg` set this is
    /// `spread_fraction(index, count) * phase_spread_deg / 360`, so the
    /// 1-D path is unchanged. Equal X and Y spreads on a matrix make a
    /// diagonal: every unit on an anti-diagonal (`x + y` constant) has
    /// the same phase.
    // r[impl effects.phase.spread]
    // r[impl tricks.grid] - phase spread per axis
    pub fn spread_fraction_3d(&self, pos: &crate::tricks::UnitPos) -> f32 {
        use crate::selection::Axis;
        let f = |axis: Axis| match self.direction {
            Play::Reverse => 1.0 - pos.fraction(axis),
            _ => pos.fraction(axis),
        };
        (f(Axis::X) * self.phase_spread_deg
            + f(Axis::Y) * self.phase_spread_y_deg
            + f(Axis::Z) * self.phase_spread_z_deg)
            / 360.0
    }

    /// Where a unit at `pos` arrives under `Build`, 0…1 — the 3-D twin
    /// of [`Timing::spread_fraction`] for the threshold recipe.rs tests
    /// against. The spreads weight the axes; with none set (a Build has
    /// no need of a spread) it is the X fraction, which is what the 1-D
    /// path uses today.
    // r[impl effects.play] - Build, per grid position
    pub fn build_fraction_3d(&self, pos: &crate::tricks::UnitPos) -> f32 {
        let total = self.phase_spread_deg.abs()
            + self.phase_spread_y_deg.abs()
            + self.phase_spread_z_deg.abs();
        if total == 0.0 {
            return self.spread_fraction(pos.x, pos.count[0]);
        }
        (self.spread_fraction_3d(pos) * 360.0 / total).clamp(0.0, 1.0)
    }

    /// [`Timing::cycles_at`] for a unit at a grid position: identical
    /// on a one-axis grid, and the only way the Y and Z spreads reach
    /// the clock.
    // r[impl effects.phase.spread]
    // r[impl tricks.grid] - phase spread per axis
    pub fn cycles_at_pos(
        &self,
        secs: f32,
        pos: &crate::tricks::UnitPos,
        masters: &SpeedMasters,
    ) -> f32 {
        let raw = self.cycles(secs, masters)
            + self.spread_fraction_3d(pos)
            + self.phase_offset_deg / 360.0;
        self.finish(raw)
    }

    /// The tail of `cycles_at` that `Bounce` and `once` share.
    fn finish(&self, raw: f32) -> f32 {
        let warped = match self.direction {
            Play::Bounce => {
                let u = raw - raw.floor();
                if u < 0.5 { u * 2.0 } else { 2.0 - u * 2.0 }
            }
            _ => raw,
        };
        if self.once {
            // Just under one cycle, so `locate` stays inside the last
            // step rather than wrapping to the first. Landing exactly on
            // 1.0 would restart the envelope, which is the bug this mode
            // exists to avoid.
            warped.min(1.0 - f32::EPSILON)
        } else {
            warped
        }
    }

    /// Cycles elapsed after `secs`, for one fixture at `index` of
    /// `count` in the selection.
    ///
    /// `Bounce` warps the result rather than the step table: one cycle
    /// runs the list out and back, so the wave washes instead of
    /// snapping at the wrap.
    // r[impl effects.phase.spread]
    // r[impl effects.phase.offset]
    // r[impl effects.play] - Bounce
    // r[impl effects.once] - clamps just under one cycle and holds
    // r[impl recipes.one-shot] - hold at the last step; the clock choice is cue.rs recipe_time
    pub fn cycles_at(&self, secs: f32, index: usize, count: usize, masters: &SpeedMasters) -> f32 {
        let spread = self.spread_fraction(index, count);
        let raw = self.cycles(secs, masters)
            + spread * (self.phase_spread_deg / 360.0)
            + self.phase_offset_deg / 360.0;
        self.finish(raw)
    }
}

/// Where in the step list a cycle position falls, and how far through
/// that step's transition it is.
///
/// Returns `(previous step, this step, blend)` — `blend` is 0 at the
/// moment this step takes over and 1 once it is fully arrived, so the
/// caller interpolates the previous step's values into this one's
/// without needing to know anything about widths.
// r[impl effects.step.width]
// r[impl effects.step.transition]
// r[impl effects.step.ease]
pub fn locate(steps: &[Step], cycles: f32) -> (usize, usize, f32) {
    if steps.len() < 2 {
        return (0, 0, 1.0);
    }
    let total: f32 = steps.iter().map(|s| s.width.max(0.0)).sum();
    if total <= 0.0 {
        return (0, 0, 1.0);
    }
    let u = (cycles - cycles.floor()) * total;

    let mut edge = 0.0;
    for (i, step) in steps.iter().enumerate() {
        let width = step.width.max(0.0);
        if u < edge + width || i == steps.len() - 1 {
            let progress = if width > 0.0 { (u - edge) / width } else { 1.0 };
            let blend = if step.transition > 0.0 {
                step.ease
                    .apply((progress / step.transition).clamp(0.0, 1.0))
            } else {
                1.0
            };
            let prev = if i == 0 { steps.len() - 1 } else { i - 1 };
            return (prev, i, blend);
        }
        edge += width;
    }
    (0, 0, 1.0)
}

/// Pure transforms over a step table.
///
/// An ellipse tilted thirty degrees is a circle with two transforms, not
/// a new table — so authoring reshapes the table it has rather than
/// drawing another. Every function here takes a table and returns a new
/// one; none looks at the timing, which is where direction and phase
/// already live.
///
/// The two-axis transforms act on `Pan`/`Tilt` pairs inside `Delta`
/// applies, because that is what a position path *is* here; a step
/// naming only one axis is treated as having the other at zero.
// r[impl effects.step-transforms]
pub mod transform {
    use super::Step;
    use crate::recipe::RecipeApply;
    use ignition_proto::Attribute;

    /// The table played backwards. The transition and ease that shaped
    /// the move *into* each step travel with the step, so a snap-then-
    /// fall becomes a rise-then-snap rather than a table whose shaping
    /// no longer belongs to its moves.
    // r[impl effects.step-transforms] - reverse in time
    pub fn reverse_time(mut steps: Vec<Step>) -> Vec<Step> {
        steps.reverse();
        steps
    }

    /// The table starting `by` steps later; negative starts it earlier.
    // r[impl effects.step-transforms] - phase shift
    pub fn shift_phase(mut steps: Vec<Step>, by: isize) -> Vec<Step> {
        if steps.is_empty() {
            return steps;
        }
        let n = steps.len() as isize;
        let k = by.rem_euclid(n) as usize;
        steps.rotate_left(k);
        steps
    }

    /// Every `Delta` on `attr` negated: a tilt path that nods down now
    /// nods up.
    // r[impl effects.step-transforms] - flip one axis
    pub fn flip(steps: Vec<Step>, attr: &Attribute) -> Vec<Step> {
        map_delta(steps, |a, v| if a == attr { -v } else { v })
    }

    /// Pan scaled by `pan_k` and tilt by `tilt_k`: a circle becomes an
    /// ellipse, or a wider or tighter circle.
    // r[impl effects.step-transforms] - scale a two-axis path
    pub fn scale_axes(steps: Vec<Step>, pan_k: f32, tilt_k: f32) -> Vec<Step> {
        map_delta(steps, |a, v| match a {
            Attribute::Pan => v * pan_k,
            Attribute::Tilt => v * tilt_k,
            _ => v,
        })
    }

    /// Pan and tilt exchanged: a pan sweep becomes a tilt sweep.
    // r[impl effects.step-transforms] - swap axes
    pub fn swap_axes(steps: Vec<Step>) -> Vec<Step> {
        map_pairs(steps, |pairs| {
            for (a, _) in pairs.iter_mut() {
                match *a {
                    Attribute::Pan => *a = Attribute::Tilt,
                    Attribute::Tilt => *a = Attribute::Pan,
                    _ => {}
                }
            }
        })
    }

    /// The pan/tilt path rotated by `theta` degrees, counter-clockwise
    /// with pan as x and tilt as y. Ninety degrees of a circle is the
    /// same circle a quarter later.
    // r[impl effects.step-transforms] - rotate a two-axis path
    pub fn rotate_deg(steps: Vec<Step>, theta: f32) -> Vec<Step> {
        let (sin, cos) = theta.to_radians().sin_cos();
        map_pairs(steps, |pairs| {
            let axis = |want: &Attribute| pairs.iter().find(|(a, _)| a == want).map(|(_, v)| *v);
            let (pan, tilt) = (axis(&Attribute::Pan), axis(&Attribute::Tilt));
            if pan.is_none() && tilt.is_none() {
                return;
            }
            let (p, t) = (pan.unwrap_or(0.0), tilt.unwrap_or(0.0));
            let (np, nt) = (p * cos - t * sin, p * sin + t * cos);
            pairs.retain(|(a, _)| !matches!(a, Attribute::Pan | Attribute::Tilt));
            pairs.push((Attribute::Pan, np));
            pairs.push((Attribute::Tilt, nt));
        })
    }

    fn map_delta(steps: Vec<Step>, f: impl Fn(&Attribute, f32) -> f32) -> Vec<Step> {
        map_pairs(steps, |pairs| {
            for (a, v) in pairs.iter_mut() {
                *v = f(a, *v);
            }
        })
    }

    fn map_pairs(mut steps: Vec<Step>, f: impl Fn(&mut Vec<(Attribute, f32)>)) -> Vec<Step> {
        for step in steps.iter_mut() {
            for apply in step.apply.iter_mut() {
                if let RecipeApply::Delta(pairs) = apply {
                    f(pairs);
                }
            }
        }
        steps
    }
}

/// A periodic shape, as authoring sugar over a step table.
///
/// Kept because "sine" is a worse thing to say as two steps at 100%
/// transition than as the word "sine", and because it is how every other
/// console spells this. It expands on load, so there is still one
/// runtime, one cascade and one player — see Decision 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// r[impl effects.waveform.is-sugar]
pub enum Waveform {
    Sine,
    Triangle,
    Square,
    /// Rises across the cycle, then snaps back — a one-direction chase.
    RampUp,
    RampDown,
}

impl Waveform {
    /// The step table for this shape swinging `size` either side of
    /// `base` on `attr`.
    pub fn steps(
        self,
        attr: ignition_proto::Attribute,
        base: f32,
        size: f32,
        relative: bool,
    ) -> Vec<Step> {
        let make = |v: f32| {
            let pair = vec![(attr.clone(), v)];
            vec![if relative {
                crate::recipe::RecipeApply::Delta(pair)
            } else {
                crate::recipe::RecipeApply::Raw(pair)
            }]
        };
        // r[impl effects.waveform.is-sugar] - the expansion
        // r[impl effects.waveform.starts-low] - high step listed first
        let (lo, hi) = (base - size, base + size);
        // Step order looks backwards and is not. A step's transition
        // moves *into* it from the step before, so at cycle 0 the value
        // is the last step's, not the first's. Listing `hi` first
        // therefore starts the wave at `lo` and rises to `hi` at the
        // half-cycle — a pulse that builds, which is what every other
        // console means by a sine.
        // The snap-back on a ramp is a step of near-zero width rather
        // than a true discontinuity — 2% of the cycle, which reads as
        // instant and keeps the model to one mechanism.
        // r[impl effects.waveform.ramp-snaps]
        const SNAP: f32 = 0.02;
        match self {
            Waveform::Sine => vec![
                Step {
                    apply: make(hi),
                    width: 1.0,
                    transition: 1.0,
                    ease: Ease::Sine,
                },
                Step {
                    apply: make(lo),
                    width: 1.0,
                    transition: 1.0,
                    ease: Ease::Sine,
                },
            ],
            Waveform::Triangle => vec![
                Step {
                    apply: make(hi),
                    width: 1.0,
                    transition: 1.0,
                    ease: Ease::Linear,
                },
                Step {
                    apply: make(lo),
                    width: 1.0,
                    transition: 1.0,
                    ease: Ease::Linear,
                },
            ],
            Waveform::Square => vec![Step::new(make(hi)), Step::new(make(lo))],
            Waveform::RampUp => vec![
                Step {
                    apply: make(lo),
                    width: SNAP,
                    transition: 0.0,
                    ease: Ease::Linear,
                },
                Step {
                    apply: make(hi),
                    width: 1.0 - SNAP,
                    transition: 1.0,
                    ease: Ease::Linear,
                },
            ],
            Waveform::RampDown => vec![
                Step {
                    apply: make(hi),
                    width: SNAP,
                    transition: 0.0,
                    ease: Ease::Linear,
                },
                Step {
                    apply: make(lo),
                    width: 1.0 - SNAP,
                    transition: 1.0,
                    ease: Ease::Linear,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // r[verify effects.speed]

    #[test]
    fn speed_units_all_land_on_beats_per_second() {
        let masters = SpeedMasters::from([("Song".to_string(), 120.0)]);
        assert_eq!(Speed::Hz(2.0).beats_per_second(&masters), 2.0);
        assert_eq!(Speed::Bpm(120.0).beats_per_second(&masters), 2.0);
        assert_eq!(Speed::Secs(0.5).beats_per_second(&masters), 2.0);
        assert_eq!(Speed::Master("Song".into()).beats_per_second(&masters), 2.0);
    }

    /// A master that is not wired up runs at a fallback rather than
    /// freezing.
    ///
    /// This test asserted the opposite, and the assumption behind it was
    /// wrong in a way only a one-shot exposes. Freezing a *loop* is
    /// harmless — it is merely still. Freezing a **one-shot** parks it
    /// at its lift, so a flash key fired before a transport loaded would
    /// hold the rig bright with no way to release it: exactly the stuck
    /// light that one-shots exist to make impossible.
    ///
    /// The mistake is still reported — `recipe::unresolved` names a
    /// missing master against every cue that asked for it. What changed
    /// is that it is no longer reported *by leaving a light on*.
    // r[verify effects.masters.unknown]
    #[test]
    fn an_unknown_speed_master_falls_back_rather_than_freezing() {
        let rate = Speed::Master("Nope".into()).beats_per_second(&SpeedMasters::new());
        assert!(rate > 0.0, "an unset master froze the effect");
        assert_eq!(rate, Speed::FALLBACK_BPM / 60.0);
    }

    /// A master set to zero is treated the same way, since "stopped" is
    /// not a tempo somebody means to program.
    // r[verify effects.masters.unknown]
    #[test]
    fn a_zero_master_also_falls_back() {
        let mut masters = SpeedMasters::new();
        masters.insert("Song".into(), 0.0);
        assert!(Speed::Master("Song".into()).beats_per_second(&masters) > 0.0);
    }

    #[test]
    fn a_single_step_is_always_fully_arrived() {
        let steps = vec![Step::new(vec![])];
        assert_eq!(locate(&steps, 0.37), (0, 0, 1.0));
    }

    // r[verify effects.step.transition]

    #[test]
    fn two_snapping_steps_split_the_cycle_in_half() {
        let steps = vec![Step::new(vec![]), Step::new(vec![])];
        assert_eq!(locate(&steps, 0.0).1, 0);
        assert_eq!(locate(&steps, 0.49).1, 0);
        assert_eq!(locate(&steps, 0.51).1, 1);
        assert_eq!(locate(&steps, 1.25).1, 0, "cycles past 1 wrap");
    }

    // r[verify effects.step.width]

    #[test]
    fn width_is_relative_not_absolute() {
        let wide = |w: f32| Step {
            apply: vec![],
            width: w,
            transition: 0.0,
            ease: Ease::Linear,
        };
        let steps = vec![wide(3.0), wide(1.0)];
        assert_eq!(
            locate(&steps, 0.7).1,
            0,
            "the wide step owns 3/4 of the cycle"
        );
        assert_eq!(locate(&steps, 0.8).1, 1);
    }

    // r[verify effects.step.transition]

    #[test]
    fn a_full_transition_blends_across_the_whole_step() {
        let steps = vec![
            Step {
                apply: vec![],
                width: 1.0,
                transition: 1.0,
                ease: Ease::Linear,
            },
            Step {
                apply: vec![],
                width: 1.0,
                transition: 1.0,
                ease: Ease::Linear,
            },
        ];
        let (_, _, quarter) = locate(&steps, 0.25);
        assert!((quarter - 0.5).abs() < 0.001, "{quarter}");
        let (_, _, at_end) = locate(&steps, 0.49);
        assert!(at_end > 0.95, "{at_end}");
    }

    /// A sine's structure. That the values really trace a sine is
    /// checked in `recipe.rs`, where they are actually resolved.
    // r[verify effects.waveform.is-sugar]
    // r[verify effects.step.ease]
    #[test]
    fn a_sine_waveform_is_two_fully_eased_steps() {
        let steps = Waveform::Sine.steps(ignition_proto::Attribute::Dimmer, 0.5, 0.5, false);
        assert_eq!(steps.len(), 2);
        assert!(
            steps
                .iter()
                .all(|s| s.transition == 1.0 && s.ease == Ease::Sine)
        );
    }

    /// Every shape lists its high step first, so a cycle *starts* at the
    /// low end — a transition moves into a step, and at cycle zero the
    /// value is the previous (last, low) step's. Checked structurally
    /// here for all five shapes; `recipe.rs` resolves the sine and sees
    /// `base - size` at t = 0.
    /// r[verify effects.waveform.starts-low]
    #[test]
    fn every_waveform_starts_at_the_low_end_of_its_swing() {
        use crate::recipe::RecipeApply;
        let level = |step: &Step| match &step.apply[0] {
            RecipeApply::Raw(v) | RecipeApply::Delta(v) => v[0].1,
            other => panic!("{other:?}"),
        };
        for shape in [
            Waveform::Sine,
            Waveform::Triangle,
            Waveform::Square,
            Waveform::RampUp,
            Waveform::RampDown,
        ] {
            let steps = shape.steps(ignition_proto::Attribute::Dimmer, 0.5, 0.3, false);
            // The value at cycle 0 is what the cycle wraps *from*: for
            // an eased shape that is the last step, for a ramp up the
            // snap step listed first. Either way it is `base - size`.
            let at_zero = match shape {
                Waveform::RampUp => level(&steps[0]),
                _ => level(steps.last().unwrap()),
            };
            assert!(
                (at_zero - 0.2).abs() < 1e-6,
                "{shape:?} starts at {at_zero}"
            );
        }
        let sine = Waveform::Sine.steps(ignition_proto::Attribute::Dimmer, 0.5, 0.3, false);
        assert!((level(&sine[0]) - 0.8).abs() < 1e-6, "high listed first");
    }

    // r[verify effects.phase.spread]

    // r[verify effects.phase.in-selection-order]

    #[test]
    fn phase_spread_offsets_each_fixture_around_the_cycle() {
        let timing = Timing {
            phase_spread_deg: 360.0,
            ..Default::default()
        };
        let masters = SpeedMasters::new();
        assert_eq!(timing.cycles_at(0.0, 0, 4, &masters), 0.0);
        assert_eq!(timing.cycles_at(0.0, 1, 4, &masters), 0.25);
        assert_eq!(timing.cycles_at(0.0, 3, 4, &masters), 0.75);
    }

    /// Per-axis spread: equal X and Y spreads put every unit on an
    /// anti-diagonal of a 4×4 grid at the same phase, and a Y-only
    /// spread on a one-truss grid is nothing at all.
    // r[verify effects.phase.spread]
    // r[verify tricks.grid]
    // r[verify tricks.grid.degenerate-axes]
    #[test]
    fn per_axis_spread_makes_a_diagonal_and_ignores_a_flat_axis() {
        use crate::tricks::UnitPos;
        let masters = SpeedMasters::new();
        let diagonal = Timing {
            phase_spread_deg: 180.0,
            phase_spread_y_deg: 180.0,
            ..Default::default()
        };
        let at = |x, y| UnitPos {
            x,
            y,
            z: 0,
            count: [4, 4, 1],
        };
        let anti: Vec<f32> = [(0, 3), (1, 2), (2, 1), (3, 0)]
            .iter()
            .map(|&(x, y)| diagonal.cycles_at_pos(0.0, &at(x, y), &masters))
            .collect();
        assert!(anti.iter().all(|c| (c - anti[0]).abs() < 1e-6), "{anti:?}");
        assert!(diagonal.cycles_at_pos(0.0, &at(0, 0), &masters) < anti[0]);
        assert!(diagonal.cycles_at_pos(0.0, &at(3, 3), &masters) > anti[0]);

        // Only X set: identical to the 1-D clock, so nothing moves.
        let x_only = Timing {
            phase_spread_deg: 360.0,
            ..Default::default()
        };
        for i in 0..4 {
            let pos = UnitPos {
                x: i,
                y: 0,
                z: 0,
                count: [4, 1, 1],
            };
            assert_eq!(
                x_only.cycles_at_pos(0.0, &pos, &masters),
                x_only.cycles_at(0.0, i, 4, &masters)
            );
        }
        // Y on a single truss: everyone in lockstep.
        let y_only = Timing {
            phase_spread_y_deg: 360.0,
            ..Default::default()
        };
        let lone = UnitPos {
            x: 2,
            y: 0,
            z: 0,
            count: [8, 1, 1],
        };
        assert_eq!(y_only.spread_fraction_3d(&lone), 0.0);
        assert_eq!(
            y_only.build_fraction_3d(&lone),
            0.0,
            "lockstep, like the phase"
        );
        // With no spread at all a Build still fills along X, as today.
        assert_eq!(Timing::default().build_fraction_3d(&lone), 0.25);
    }

    /// The two new fields are absent from disk when unset, so every
    /// existing show file round-trips byte for byte.
    #[test]
    fn per_axis_spread_is_skipped_when_zero() {
        let json = serde_json::to_string(&Timing::default()).unwrap();
        assert!(!json.contains("phase_spread_y_deg"), "{json}");
        let t: Timing = serde_json::from_str(r#"{"phase_spread_y_deg":90}"#).unwrap();
        assert_eq!(t.phase_spread_y_deg, 90.0);
        assert_eq!(t.phase_spread_z_deg, 0.0);
    }

    // r[verify effects.measure]

    #[test]
    fn measure_stretches_a_loop_across_more_beats() {
        let masters = SpeedMasters::new();
        let one_beat = Timing {
            speed: Speed::Bpm(60.0),
            measure: 1.0,
            ..Default::default()
        };
        let four_beat = Timing {
            speed: Speed::Bpm(60.0),
            measure: 4.0,
            ..Default::default()
        };
        assert_eq!(one_beat.cycles_at(4.0, 0, 1, &masters), 4.0);
        assert_eq!(four_beat.cycles_at(4.0, 0, 1, &masters), 1.0);
    }
}

#[cfg(test)]
mod once_tests {
    use super::*;

    fn timing(once: bool) -> Timing {
        Timing {
            speed: Speed::Bpm(60.0),
            measure: 1.0,
            once,
            ..Default::default()
        }
    }

    /// A looping phaser keeps going round; that is the point of one.
    #[test]
    fn a_looping_phaser_wraps() {
        let t = timing(false);
        let masters = SpeedMasters::default();
        assert!(t.cycles_at(4.0, 0, 1, &masters) > 3.0);
    }

    /// A one-shot stops at the end of its first cycle and stays there,
    /// which is what lets a bump release itself instead of needing a
    /// second cue to put it out.
    // r[verify effects.once]
    // r[verify recipes.one-shot]
    #[test]
    fn a_one_shot_holds_at_the_end() {
        let t = timing(true);
        let masters = SpeedMasters::default();
        let late = t.cycles_at(60.0, 0, 1, &masters);
        assert!(late < 1.0, "wrapped to {late}");
        assert!(late > 0.99, "did not reach the end: {late}");
        // Still there much later — held, not restarted.
        assert_eq!(late, t.cycles_at(600.0, 0, 1, &masters));
    }

    /// Before the end it behaves normally, or the envelope would be a
    /// step rather than a shape.
    // r[verify effects.once]
    #[test]
    fn a_one_shot_runs_normally_until_it_ends() {
        let t = timing(true);
        let masters = SpeedMasters::default();
        let half = t.cycles_at(0.5, 0, 1, &masters);
        assert!((half - 0.5).abs() < 1e-4, "{half}");
    }
}

#[cfg(test)]
mod bar_phase_tests {
    use super::*;

    /// A one-bar, eight-step pattern must put step `i` on eighth `i` of
    /// the bar when the clock is the song's own position.
    ///
    /// This is the "snare lights on beat 3" report. The pattern was
    /// right — slots 2 and 6, which are beats 2 and 4 — and it landed
    /// wrong because the clock it ran on had no relationship to the bar.
    /// With the song driving the clock, cycle position *is* position in
    /// the bar, and this pins that.
    // r[verify effects.measure]
    // r[verify effects.sync.follows-the-song]
    #[test]
    fn a_one_bar_pattern_lands_on_its_own_eighths() {
        let timing = Timing {
            speed: Speed::Bpm(120.0),
            measure: 4.0,
            ..Default::default()
        };
        let masters = SpeedMasters::default();
        let steps: Vec<Step> = (0..8).map(|_| Step::new(Vec::new())).collect();

        // At 120 bpm a beat is 0.5 s, so beat 2 of bar 1 is 0.5 s in.
        for (secs, want_slot) in [
            (0.0, 0), // beat 1
            (0.5, 2), // beat 2   <- the snare
            (1.0, 4), // beat 3
            (1.5, 6), // beat 4   <- the snare
            (2.0, 0), // beat 1 of the next bar, wrapped
        ] {
            let cycles = timing.cycles_at(secs, 0, 1, &masters);
            let (_, step, _) = locate(&steps, cycles);
            assert_eq!(step, want_slot, "at {secs}s");
        }
    }

    /// Two bars in, the same pattern is in the same place — the phase
    /// must not drift across bars.
    // r[verify effects.sync.follows-the-song]
    #[test]
    fn the_pattern_does_not_drift_across_bars() {
        let timing = Timing {
            speed: Speed::Bpm(120.0),
            measure: 4.0,
            ..Default::default()
        };
        let masters = SpeedMasters::default();
        let steps: Vec<Step> = (0..8).map(|_| Step::new(Vec::new())).collect();
        // Beat 2 of bar 1, and of bar 21.
        let early = locate(&steps, timing.cycles_at(0.5, 0, 1, &masters)).1;
        let late = locate(&steps, timing.cycles_at(0.5 + 40.0, 0, 1, &masters)).1;
        assert_eq!(early, late);
    }
}

#[cfg(test)]
mod ease_tests {
    use super::*;

    fn grid() -> impl Iterator<Item = f32> {
        (0..=200).map(|i| i as f32 / 200.0)
    }

    /// Whatever the bends, the move starts where it started and ends
    /// where it was going.
    // r[verify effects.step.accel-decel]
    #[test]
    fn every_curve_hits_its_endpoints_exactly() {
        for accel in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            for decel in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                let e = Ease::Curve { accel, decel };
                assert_eq!(e.apply(0.0), 0.0, "{e:?}");
                assert_eq!(e.apply(1.0), 1.0, "{e:?}");
            }
        }
        assert_eq!(Ease::Sine.apply(0.0), 0.0);
        assert_eq!(Ease::Sine.apply(1.0), 1.0);
    }

    /// A move never turns back on itself, however hard or soft either
    /// end is: an overshoot on a tilt is a wobble the audience sees.
    // r[verify effects.step.accel-decel]
    #[test]
    fn every_curve_is_monotone() {
        for accel in [-1.0, -0.7, -0.3, 0.0, 0.3, 0.7, 1.0] {
            for decel in [-1.0, -0.7, -0.3, 0.0, 0.3, 0.7, 1.0] {
                let e = Ease::Curve { accel, decel };
                let mut last = 0.0;
                for t in grid() {
                    let v = e.apply(t);
                    assert!(v >= last - 1e-6, "{e:?} turned back at {t}: {v} < {last}");
                    last = v;
                }
            }
        }
    }

    /// The old spellings are sugar: `Sine` is the −1/−1 curve, exactly
    /// enough that nobody could tell the two apart.
    // r[verify effects.step.accel-decel]
    // r[verify effects.step.ease]
    #[test]
    fn sine_is_the_soft_soft_curve() {
        let curve = Ease::Curve {
            accel: -1.0,
            decel: -1.0,
        };
        for t in grid() {
            assert!(
                (Ease::Sine.apply(t) - curve.apply(t)).abs() < 1e-3,
                "at {t}"
            );
            assert!((Ease::Linear.apply(t) - t).abs() < 1e-6);
        }
        assert_eq!(Ease::Sine.as_curve(), (-1.0, -1.0));
        assert_eq!(Ease::Linear.as_curve(), (0.0, 0.0));
    }

    /// "Hit hard, fall soft": the move leaves faster than it arrives.
    // r[verify effects.step.accel-decel]
    #[test]
    fn hit_hard_fall_soft_leaves_faster_than_it_arrives() {
        let e = Ease::Curve {
            accel: 1.0,
            decel: -1.0,
        };
        let leaving = e.apply(0.1) - e.apply(0.0);
        let arriving = e.apply(1.0) - e.apply(0.9);
        assert!(
            leaving > 3.0 * arriving,
            "leave {leaving} arrive {arriving}"
        );
        // And the mirror image arrives harder than it leaves.
        let soft_hard = Ease::Curve {
            accel: -1.0,
            decel: 1.0,
        };
        assert!(soft_hard.apply(0.1) < soft_hard.apply(1.0) - soft_hard.apply(0.9));
        // Harder than linear really is harder.
        assert!(e.apply(0.1) > Ease::Linear.apply(0.1));
    }

    /// The old files still load, and the new form round-trips.
    // r[verify effects.step.accel-decel]
    #[test]
    fn ease_serde_keeps_the_old_spellings() {
        let sine: Step = serde_json::from_str(r#"{"apply":[],"ease":"Sine"}"#).unwrap();
        assert_eq!(sine.ease, Ease::Sine);
        let curve: Step =
            serde_json::from_str(r#"{"apply":[],"ease":{"Curve":{"accel":1.0,"decel":-1.0}}}"#)
                .unwrap();
        assert_eq!(
            curve.ease,
            Ease::Curve {
                accel: 1.0,
                decel: -1.0
            }
        );
        let text = serde_json::to_string(&curve.ease).unwrap();
        assert_eq!(text, r#"{"Curve":{"accel":1.0,"decel":-1.0}}"#);
    }
}

#[cfg(test)]
mod speed_scale_tests {
    use super::*;

    /// Double-time off the same master, and a tempo change carries it.
    // r[verify playback.speed-scale]
    #[test]
    fn a_scaled_speed_is_a_multiple_of_its_master() {
        let mut masters = SpeedMasters::from([("Song".to_string(), 120.0)]);
        let double = Speed::Scaled {
            master: "Song".into(),
            scale: 2.0,
        };
        let half = Speed::Scaled {
            master: "Song".into(),
            scale: 0.5,
        };
        assert_eq!(double.beats_per_second(&masters), 4.0);
        assert_eq!(half.beats_per_second(&masters), 1.0);
        masters.insert("Song".into(), 60.0);
        assert_eq!(double.beats_per_second(&masters), 2.0);
        assert_eq!(double.master(), Some("Song"));
        assert_eq!(Speed::Hz(1.0).master(), None);
    }

    /// An unset master and a zero scale both fall back rather than
    /// freezing — the stuck-one-shot argument, twice.
    // r[verify playback.speed-scale]
    // r[verify effects.masters.unknown]
    #[test]
    fn a_scaled_speed_falls_back_like_a_plain_master() {
        let none = SpeedMasters::new();
        let s = Speed::Scaled {
            master: "Nope".into(),
            scale: 2.0,
        };
        assert_eq!(s.beats_per_second(&none), Speed::FALLBACK_BPM / 60.0 * 2.0);
        let zero = Speed::Scaled {
            master: "Nope".into(),
            scale: 0.0,
        };
        assert_eq!(zero.beats_per_second(&none), Speed::FALLBACK_BPM / 60.0);
    }

    // r[verify playback.speed-scale]
    #[test]
    fn scaled_speed_serde_shape() {
        let s: Speed = serde_json::from_str(r#"{"Scaled":{"master":"Song","scale":2.0}}"#).unwrap();
        assert_eq!(
            s,
            Speed::Scaled {
                master: "Song".into(),
                scale: 2.0
            }
        );
        let m: Speed = serde_json::from_str(r#"{"Master":"Song"}"#).unwrap();
        assert_eq!(m, Speed::Master("Song".into()));
    }
}

#[cfg(test)]
mod transform_tests {
    use super::transform::*;
    use super::*;
    use crate::recipe::RecipeApply;
    use ignition_proto::Attribute;

    /// A sixteen-point circle of radius `r`: pan a sine, tilt a quarter
    /// ahead.
    fn circle(r_pan: f32, r_tilt: f32) -> Vec<Step> {
        (0..16)
            .map(|i| {
                let t = std::f32::consts::TAU * i as f32 / 16.0;
                Step {
                    transition: 1.0,
                    ..Step::new(vec![RecipeApply::Delta(vec![
                        (Attribute::Pan, r_pan * t.sin()),
                        (Attribute::Tilt, r_tilt * t.cos()),
                    ])])
                }
            })
            .collect()
    }

    fn axis(step: &Step, want: &Attribute) -> f32 {
        step.apply
            .iter()
            .find_map(|a| match a {
                RecipeApply::Delta(p) => p.iter().find(|(a, _)| a == want).map(|(_, v)| *v),
                _ => None,
            })
            .unwrap_or(0.0)
    }

    fn same_path(a: &[Step], b: &[Step]) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            for attr in [Attribute::Pan, Attribute::Tilt] {
                let (p, q) = (axis(x, &attr), axis(y, &attr));
                assert!((p - q).abs() < 1e-3, "{attr:?}: {p} vs {q}");
            }
        }
    }

    /// Rotating a circle a quarter turn is the same circle a quarter of
    /// a cycle later — the geometric fact the transforms exist for.
    // r[verify effects.step-transforms]
    #[test]
    fn a_circle_rotated_ninety_is_the_circle_a_quarter_later() {
        let c = circle(20.0, 20.0);
        let rotated = rotate_deg(c.clone(), -90.0);
        let shifted = shift_phase(c.clone(), 4);
        same_path(&rotated, &shifted);
        // And the other way round.
        same_path(&rotate_deg(c.clone(), 90.0), &shift_phase(c, -4));
    }

    /// Flipping an axis negates it and touches nothing else.
    // r[verify effects.step-transforms]
    #[test]
    fn flip_inverts_one_axis() {
        let c = circle(20.0, 10.0);
        let f = flip(c.clone(), &Attribute::Tilt);
        for (a, b) in c.iter().zip(&f) {
            assert_eq!(axis(a, &Attribute::Pan), axis(b, &Attribute::Pan));
            assert_eq!(axis(a, &Attribute::Tilt), -axis(b, &Attribute::Tilt));
        }
    }

    /// Scaling one axis turns a circle into the ellipse drawn directly.
    // r[verify effects.step-transforms]
    #[test]
    fn scale_makes_an_ellipse() {
        let ellipse = scale_axes(circle(20.0, 20.0), 1.5, 0.5);
        same_path(&ellipse, &circle(30.0, 10.0));
    }

    /// Swapping axes twice is the identity, and once exchanges them.
    // r[verify effects.step-transforms]
    #[test]
    fn swap_exchanges_pan_and_tilt() {
        let c = circle(20.0, 10.0);
        let s = swap_axes(c.clone());
        for (a, b) in c.iter().zip(&s) {
            assert_eq!(axis(a, &Attribute::Pan), axis(b, &Attribute::Tilt));
            assert_eq!(axis(a, &Attribute::Tilt), axis(b, &Attribute::Pan));
        }
        assert_eq!(swap_axes(s), c);
    }

    /// Reversing runs the table backwards; twice is the table again. A
    /// shift by the whole length is nothing.
    // r[verify effects.step-transforms]
    #[test]
    fn reverse_and_shift_are_what_they_say() {
        let c = circle(20.0, 20.0);
        let r = reverse_time(c.clone());
        assert_eq!(r[0], c[15]);
        assert_eq!(reverse_time(r), c);
        assert_eq!(shift_phase(c.clone(), 16), c);
        assert_eq!(shift_phase(c.clone(), 1)[0], c[1]);
        assert!(shift_phase(Vec::new(), 3).is_empty());
    }

    /// A step naming only pan still rotates: the missing axis is zero,
    /// and appears once the rotation gives it a value.
    // r[verify effects.step-transforms]
    #[test]
    fn rotation_fills_in_a_missing_axis() {
        let sweep = vec![delta_pan(10.0), delta_pan(-10.0)];
        let up = rotate_deg(sweep, 90.0);
        assert!(axis(&up[0], &Attribute::Pan).abs() < 1e-4);
        assert!((axis(&up[0], &Attribute::Tilt) - 10.0).abs() < 1e-4);
    }

    fn delta_pan(v: f32) -> Step {
        Step::new(vec![RecipeApply::Delta(vec![(Attribute::Pan, v)])])
    }

    /// An effect that does not say how fast it goes holds still.
    ///
    /// A one-step recipe never asks. A multi-step one that forgot to
    /// say is a mistake, and a still chase is a visible mistake where a
    /// chase running at some plausible default is an invisible one.
    ///
    /// r[verify effects.speed.default-is-still]
    #[test]
    fn an_effect_that_names_no_speed_holds_still() {
        let timing = Timing::default();
        let masters = SpeedMasters::default();
        for secs in [0.0, 0.5, 10.0, 600.0] {
            assert_eq!(
                timing.cycles(secs, &masters),
                0.0,
                "a speechless effect ran at {secs}s"
            );
        }
    }

    /// The phase offset shifts every fixture by the same amount.
    ///
    /// Two recipes on Pan and Tilt a quarter-cycle apart *are* a circle,
    /// and this is how that is said without a dedicated position-effect
    /// type — so the offset has to be uniform across the selection,
    /// unlike the spread, which is deliberately not.
    ///
    /// r[verify effects.phase.offset]
    #[test]
    fn the_phase_offset_moves_every_fixture_together() {
        let masters = SpeedMasters::from([("Song".to_string(), 120.0f32)]);
        let quarter = Timing {
            speed: Speed::Master("Song".into()),
            phase_offset_deg: 90.0,
            ..Default::default()
        };
        let plain = Timing {
            phase_offset_deg: 0.0,
            ..quarter.clone()
        };

        // Same shift for the first fixture and the last: an offset is
        // not a spread.
        let at = |x: usize| crate::tricks::UnitPos {
            x,
            y: 0,
            z: 0,
            count: [4, 1, 1],
        };
        for index in [0usize, 3] {
            let a = plain.cycles_at_pos(1.0, &at(index), &masters);
            let b = quarter.cycles_at_pos(1.0, &at(index), &masters);
            assert!(
                ((b - a) - 0.25).abs() < 1e-5,
                "fixture {index} shifted by {} rather than a quarter",
                b - a
            );
        }
    }

    /// Reverse mirrors the spread; it is not how an author says "left
    /// to right".
    ///
    /// Direction belongs to the selection's order. Reverse exists for
    /// when that order is already the one meant and the other end
    /// should lead — so it flips which end of the *same* selection
    /// leads, and nothing else.
    ///
    /// r[verify effects.play.is-not-direction]
    #[test]
    fn reverse_flips_which_end_leads_and_nothing_else() {
        let forward = Timing::default();
        let backward = Timing {
            direction: Play::Reverse,
            ..Default::default()
        };

        let count = 4;
        for index in 0..count {
            let f = forward.spread_fraction(index, count);
            let b = backward.spread_fraction(index, count);
            assert!(
                (f + b - 1.0).abs() < 1e-5,
                "fixture {index} is not mirrored: {f} and {b}"
            );
        }

        // The leading end swaps, which is the whole of what it does.
        assert!(forward.spread_fraction(0, count) < forward.spread_fraction(3, count));
        assert!(backward.spread_fraction(0, count) > backward.spread_fraction(3, count));
    }

    /// A ramp's snap-back is a very narrow step, not a discontinuity.
    ///
    /// It reads as instant and keeps the whole model to one mechanism:
    /// everything is steps with widths and transitions, including the
    /// one place a waveform appears to jump.
    ///
    /// r[verify effects.waveform.ramp-snaps]
    #[test]
    fn a_ramps_snap_back_is_a_step_and_not_a_jump() {
        for waveform in [Waveform::RampUp, Waveform::RampDown] {
            let steps = waveform.steps(Attribute::Dimmer, 0.5, 0.5, true);
            assert_eq!(steps.len(), 2, "{waveform:?} is not two steps");

            let snap = &steps[0];
            assert!(
                snap.width > 0.0 && snap.width <= 0.05,
                "{waveform:?}'s snap is {} of the cycle — not near-zero, \
                 and a width of zero would be the discontinuity this avoids",
                snap.width
            );
            assert_eq!(snap.transition, 0.0, "the snap eases, so it is not a snap");

            // The ramp proper is the rest of the cycle and travels the
            // whole way.
            let ramp = &steps[1];
            assert!((snap.width + ramp.width - 1.0).abs() < 1e-6);
            assert_eq!(ramp.transition, 1.0);
        }
    }

    /// Timing is carried once per effect and applies to every step.
    ///
    /// A per-step speed is a different effect wearing the same name,
    /// and a reader cannot tell which step is setting the pace. The
    /// guard is structural: a `Step` serialises with no timing on it at
    /// all, so there is nowhere for a second opinion to live.
    ///
    /// r[verify effects.timing.uniform]
    #[test]
    fn timing_is_carried_once_and_not_per_step() {
        let step = Step {
            apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.5)])],
            width: 1.0,
            transition: 1.0,
            ease: Ease::Sine,
        };
        let json = serde_json::to_string(&step).expect("a step serialises");

        for field in ["speed", "measure", "phase", "direction", "once"] {
            assert!(!json.contains(field), "a step carries `{field}`: {json}");
        }

        // And the effect carries them, once.
        let timing = Timing::default();
        let json = serde_json::to_string(&timing).expect("timing serialises");
        assert!(json.contains("measure"), "the effect does not carry timing");
    }
}
