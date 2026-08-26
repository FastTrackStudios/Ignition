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
    /// The tempo an unset speed master runs at.
    ///
    /// Deliberately ordinary: fast enough that an envelope completes
    /// promptly, slow enough that a chase which lost its master looks
    /// wrong rather than looking broken.
    pub const FALLBACK_BPM: f32 = 120.0;

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
            Speed::Master(name) => {
                let bpm = masters.get(name).copied().unwrap_or(0.0);
                (if bpm > 0.0 { bpm } else { Self::FALLBACK_BPM }) / 60.0
            }
        }
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
    pub once: bool,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            speed: Speed::default(),
            measure: 1.0,
            phase_spread_deg: 0.0,
            phase_offset_deg: 0.0,
            direction: Direction::default(),
            once: false,
        }
    }
}

impl Timing {
    /// Cycles elapsed after `secs`, before any per-fixture spread.
    pub fn cycles(&self, secs: f32, masters: &SpeedMasters) -> f32 {
        let measure = if self.measure > 0.0 {
            self.measure
        } else {
            1.0
        };
        secs * self.speed.beats_per_second(masters) / measure
    }

    /// Where a fixture sits in the selection, 0 at the leading end.
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

    /// Cycles elapsed after `secs`, for one fixture at `index` of
    /// `count` in the selection.
    ///
    /// `Bounce` warps the result rather than the step table: one cycle
    /// runs the list out and back, so the wave washes instead of
    /// snapping at the wrap.
    pub fn cycles_at(&self, secs: f32, index: usize, count: usize, masters: &SpeedMasters) -> f32 {
        let spread = self.spread_fraction(index, count);
        let raw = self.cycles(secs, masters)
            + spread * (self.phase_spread_deg / 360.0)
            + self.phase_offset_deg / 360.0;
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
    #[test]
    fn an_unknown_speed_master_falls_back_rather_than_freezing() {
        let rate = Speed::Master("Nope".into()).beats_per_second(&SpeedMasters::new());
        assert!(rate > 0.0, "an unset master froze the effect");
        assert_eq!(rate, Speed::FALLBACK_BPM / 60.0);
    }

    /// A master set to zero is treated the same way, since "stopped" is
    /// not a tempo somebody means to program.
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
            (0.0, 0),  // beat 1
            (0.5, 2),  // beat 2   <- the snare
            (1.0, 4),  // beat 3
            (1.5, 6),  // beat 4   <- the snare
            (2.0, 0),  // beat 1 of the next bar, wrapped
        ] {
            let cycles = timing.cycles_at(secs, 0, 1, &masters);
            let (_, step, _) = locate(&steps, cycles);
            assert_eq!(step, want_slot, "at {secs}s");
        }
    }

    /// Two bars in, the same pattern is in the same place — the phase
    /// must not drift across bars.
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
