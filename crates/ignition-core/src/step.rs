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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ease {
    #[default]
    Linear,
    /// Eases in and out — a full cycle of two `Sine` steps at
    /// `transition: 1.0` is a sine wave.
    Sine,
}

impl Ease {
    fn apply(self, t: f32) -> f32 {
        match self {
            Ease::Linear => t,
            Ease::Sine => (1.0 - (t * std::f32::consts::PI).cos()) * 0.5,
        }
    }
}

fn one() -> f32 {
    1.0
}

/// One step of a recipe: what to apply, and how long it owns.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// What this step sets. A step can set several things at once — a
    /// colour *and* a level — which is what makes "tap through four
    /// looks to build a four-step chase" a sane authoring model.
    pub apply: Vec<crate::recipe::RecipeApply>,
    /// This step's share of one cycle, relative to the other steps.
    /// Widths are normalised, so `[1, 1]` and `[3, 3]` are the same
    /// thing and `[3, 1]` holds the first step three times as long.
    #[serde(default = "one")]
    pub width: f32,
    /// Fraction of `width` spent moving toward this step's values rather
    /// than holding them. `0.0` snaps — a classic chase. `1.0` never
    /// stops moving.
    #[serde(default)]
    pub transition: f32,
    #[serde(default)]
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
pub type SpeedMasters = HashMap<String, f32>;

/// How fast a phaser runs, in whichever unit the operator thinks in.
///
/// All four resolve to *beats per second*; `Timing::measure` says how
/// many beats one full loop of the step list takes, so speed and measure
/// together fix the real-world duration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Speed {
    /// Cycles per second when `measure` is 1 — the plain "how fast".
    Hz(f32),
    Bpm(f32),
    /// Seconds per beat.
    Secs(f32),
    /// Slaved to a named master. An unknown name resolves to *stopped*
    /// rather than to some default tempo: a phaser frozen at its first
    /// step is an obvious "that is not wired up", where a phaser running
    /// at a plausible-but-wrong speed is not.
    Master(String),
}

impl Default for Speed {
    /// Static. A one-step recipe never asks, and a multi-step recipe
    /// that forgot to say holds still rather than picking a tempo for
    /// the operator.
    fn default() -> Self {
        Speed::Hz(0.0)
    }
}

impl Speed {
    pub fn beats_per_second(&self, masters: &SpeedMasters) -> f32 {
        match self {
            Speed::Hz(h) => *h,
            Speed::Bpm(b) => b / 60.0,
            Speed::Secs(s) if *s > 0.0 => 1.0 / s,
            Speed::Secs(_) => 0.0,
            Speed::Master(name) => masters.get(name).copied().unwrap_or(0.0) / 60.0,
        }
    }
}

/// Which end of the selection leads the wave.
///
/// Note that `Selection::Order` (see `selection.rs`) is usually the
/// better tool: it reorders by *real position*, so "left to right" is a
/// fact about the room. This just runs whatever order the selection
/// produced backwards, which is still the right answer when the
/// selection order is already the one you meant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    #[default]
    Forward,
    Backward,
}

/// The phaser-level layers — shared across every step, which is what
/// makes "eight fixtures phase-offset around one definition" cheap
/// instead of eight authored step tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    #[serde(default)]
    pub speed: Speed,
    /// Beats per full loop of the step list.
    #[serde(default = "one")]
    pub measure: f32,
    /// How much of one cycle is spread across the selection, in its
    /// order. 360 is a classic chase — each fixture visibly offset from
    /// its neighbour. 0 is lockstep: a pulse rather than a chase.
    #[serde(default)]
    pub phase_spread_deg: f32,
    /// A fixed shift applied to every fixture equally. Pairing two
    /// recipes on Pan and Tilt 90 degrees apart is how a circle happens,
    /// without a dedicated "position effect" type: two sine waves a
    /// quarter-cycle apart *is* a circle.
    #[serde(default)]
    pub phase_offset_deg: f32,
    #[serde(default)]
    pub direction: Direction,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            speed: Speed::default(),
            measure: 1.0,
            phase_spread_deg: 0.0,
            phase_offset_deg: 0.0,
            direction: Direction::default(),
        }
    }
}

impl Timing {
    /// Cycles elapsed after `secs`, for one fixture at `index` of
    /// `count` in the selection.
    pub fn cycles_at(&self, secs: f32, index: usize, count: usize, masters: &SpeedMasters) -> f32 {
        let measure = if self.measure > 0.0 {
            self.measure
        } else {
            1.0
        };
        let cycles = secs * self.speed.beats_per_second(masters) / measure;
        let spread = if count > 1 {
            index as f32 / count as f32
        } else {
            0.0
        };
        let spread = match self.direction {
            Direction::Forward => spread,
            Direction::Backward => 1.0 - spread,
        };
        cycles + spread * (self.phase_spread_deg / 360.0) + self.phase_offset_deg / 360.0
    }
}

/// Where in the step list a cycle position falls, and how far through
/// that step's transition it is.
///
/// Returns `(previous step, this step, blend)` — `blend` is 0 at the
/// moment this step takes over and 1 once it is fully arrived, so the
/// caller interpolates the previous step's values into this one's
/// without needing to know anything about widths.
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

/// A periodic shape, as authoring sugar over a step table.
///
/// Kept because "sine" is a worse thing to say as two steps at 100%
/// transition than as the word "sine", and because it is how every other
/// console spells this. It expands on load, so there is still one
/// runtime, one cascade and one player — see Decision 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

    #[test]
    fn speed_units_all_land_on_beats_per_second() {
        let masters = SpeedMasters::from([("Song".to_string(), 120.0)]);
        assert_eq!(Speed::Hz(2.0).beats_per_second(&masters), 2.0);
        assert_eq!(Speed::Bpm(120.0).beats_per_second(&masters), 2.0);
        assert_eq!(Speed::Secs(0.5).beats_per_second(&masters), 2.0);
        assert_eq!(Speed::Master("Song".into()).beats_per_second(&masters), 2.0);
    }

    /// A master that is not wired up freezes rather than guessing.
    #[test]
    fn an_unknown_speed_master_stops() {
        assert_eq!(
            Speed::Master("Nope".into()).beats_per_second(&SpeedMasters::new()),
            0.0
        );
    }

    #[test]
    fn a_single_step_is_always_fully_arrived() {
        let steps = vec![Step::new(vec![])];
        assert_eq!(locate(&steps, 0.37), (0, 0, 1.0));
    }

    #[test]
    fn two_snapping_steps_split_the_cycle_in_half() {
        let steps = vec![Step::new(vec![]), Step::new(vec![])];
        assert_eq!(locate(&steps, 0.0).1, 0);
        assert_eq!(locate(&steps, 0.49).1, 0);
        assert_eq!(locate(&steps, 0.51).1, 1);
        assert_eq!(locate(&steps, 1.25).1, 0, "cycles past 1 wrap");
    }

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
