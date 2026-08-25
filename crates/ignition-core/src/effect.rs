//! Effect Recipes — grandMA3 calls this engine "Phasers"; this project
//! uses the more universal lighting-console term ("Effects"/"Chasers" in
//! Eos, QLC+, most other consoles) since it's the more self-explanatory
//! name to anyone not already steeped in grandMA3 specifically: a waveform
//! driving one attribute across a target group's fixtures, each fixture
//! offset in the wave's phase so the group chases/pulses/sweeps together
//! rather than moving in lockstep. Fundamentally a different kind of thing
//! from a `recipe::Recipe` (a fixed target state resolved once per
//! `go()`) — an `EffectRecipe` is a **continuous function of time**,
//! evaluated fresh every tick for as long as it runs, which is why it gets
//! its own player (`EffectPlayer`) rather than compiling into
//! `cue::Cue`/`CueValue` the way static recipes do.
//!
//! Reuses `recipe::RecipeTarget`/`recipe::resolve_target` for *who* an
//! effect applies to — a `Group` name or an explicit channel list, same as
//! a static `Recipe` — so `data/shows/*.json` authoring stays consistent
//! between the two. No I/O, no DMX encoding, no wall-clock access, same
//! rule as `cue.rs`: `tick(dt_secs)` takes elapsed time as data.

use crate::group::Group;
use crate::recipe::{resolve_target, RecipeTarget};
use ignition_proto::{Attribute, ChanId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A periodic shape sampled in *cycles* (`t`'s integer part is the cycle
/// count, only the fractional part matters) — output always in `[-1, 1]`,
/// scaled by `EffectRecipe::size` and offset by `EffectRecipe::base` at
/// the call site, so a `Dimmer` effect and a `Pan` effect share the exact
/// same waveform code despite living in completely different value ranges.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Waveform {
    Sine,
    Square,
    /// Rises from -1 to 1 across the cycle, then jumps back down — a
    /// classic one-direction chase.
    RampUp,
    /// The mirror of `RampUp` — falls from 1 to -1, then jumps back up.
    RampDown,
    Triangle,
}

impl Waveform {
    pub fn sample(self, t: f32) -> f32 {
        let frac = t - t.floor(); // always in [0, 1), regardless of t's sign
        match self {
            Waveform::Sine => (frac * std::f32::consts::TAU).sin(),
            Waveform::Square => if frac < 0.5 { 1.0 } else { -1.0 },
            Waveform::RampUp => frac * 2.0 - 1.0,
            Waveform::RampDown => 1.0 - frac * 2.0,
            Waveform::Triangle => {
                if frac < 0.5 {
                    frac * 4.0 - 1.0
                } else {
                    3.0 - frac * 4.0
                }
            }
        }
    }
}

/// Which end of the target group leads the wave — `Forward` = the first
/// fixture in the resolved target list leads (a chase that reads as
/// running "forward" through however the group was authored/patched);
/// `Backward` reverses it. There's no meaningful "left-to-right" without
/// real fixture ordering data richer than a flat channel list, so this is
/// the whole of what direction means here — good enough for a chase to
/// visibly run one way vs. the other, not a claim about stage-left vs.
/// stage-right.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EffectDirection {
    Forward,
    Backward,
}

fn default_direction() -> EffectDirection {
    EffectDirection::Forward
}

/// One effect: `waveform` drives `attr` on every fixture in `target`,
/// `rate_hz` cycles per second, swinging `size` above/below `base`.
/// `phase_spread_deg` is how much of one full cycle gets spread evenly
/// across the target group (360 = a classic chase, each fixture visibly
/// offset from its neighbour; 0 = every fixture moves in lockstep, a
/// pulse/strobe rather than a chase). `phase_offset_deg` is a *fixed*
/// shift applied to every fixture equally — pairing two `EffectRecipe`s on
/// `Pan` and `Tilt` with a 90-degree difference here is how a circle
/// happens, without a dedicated "Position effect" type: two independent
/// sine waves, quarter-cycle apart, IS a circle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectRecipe {
    pub name: String,
    pub target: RecipeTarget,
    pub attr: Attribute,
    pub waveform: Waveform,
    pub rate_hz: f32,
    pub size: f32,
    pub base: f32,
    #[serde(default)]
    pub phase_spread_deg: f32,
    #[serde(default)]
    pub phase_offset_deg: f32,
    #[serde(default = "default_direction")]
    pub direction: EffectDirection,
}

impl EffectRecipe {
    fn value_at(&self, fixture_index: usize, fixture_count: usize, elapsed_secs: f32) -> f32 {
        let spread_frac = if fixture_count > 1 { fixture_index as f32 / fixture_count as f32 } else { 0.0 };
        let spread_frac = match self.direction {
            EffectDirection::Forward => spread_frac,
            EffectDirection::Backward => 1.0 - spread_frac,
        };
        let t = elapsed_secs * self.rate_hz + spread_frac * (self.phase_spread_deg / 360.0)
            + self.phase_offset_deg / 360.0;
        self.base + self.waveform.sample(t) * self.size
    }
}

/// A named collection of effects — `data/shows/*.json`'s top-level shape
/// for `live --effects`, the `EffectRecipe` counterpart to
/// `recipe::RecipeCueList`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectList {
    #[serde(default)]
    pub name: String,
    pub effects: Vec<EffectRecipe>,
}

/// Runs a set of effects indefinitely — unlike `CuePlayer`, there is no
/// `go()`/stepping concept; every loaded effect evaluates every tick for
/// as long as the player exists. `output()` resolves each effect's target
/// against `groups` fresh every call rather than caching the resolved
/// channel list, since which venue's groups are passed could in principle
/// change between calls (a cheap re-resolve at this project's fixture
/// counts — hundreds, not thousands).
pub struct EffectPlayer {
    effects: Vec<EffectRecipe>,
    elapsed: f32,
}

impl EffectPlayer {
    pub fn new(effects: Vec<EffectRecipe>) -> Self {
        Self { effects, elapsed: 0.0 }
    }

    pub fn tick(&mut self, dt_secs: f32) {
        self.elapsed += dt_secs;
    }

    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    /// The current `(chan, attr) -> value` output across every loaded
    /// effect. Two effects targeting the same `(chan, attr)` — last one in
    /// `effects` wins, same convention `CuePlayer::go()` uses when folding
    /// a cue's values into its tracked state.
    pub fn output(&self, groups: &[Group]) -> HashMap<(ChanId, Attribute), f32> {
        let mut out = HashMap::new();
        for effect in &self.effects {
            let chans = resolve_target(&effect.target, groups);
            let n = chans.len();
            for (i, chan) in chans.into_iter().enumerate() {
                let value = effect.value_at(i, n, self.elapsed);
                out.insert((chan, effect.attr.clone()), value);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_samples_the_expected_quarter_points() {
        assert!((Waveform::Sine.sample(0.0) - 0.0).abs() < 0.001);
        assert!((Waveform::Sine.sample(0.25) - 1.0).abs() < 0.001);
        assert!((Waveform::Sine.sample(0.5) - 0.0).abs() < 0.001);
        assert!((Waveform::Sine.sample(0.75) - (-1.0)).abs() < 0.001);
    }

    #[test]
    fn ramp_up_rises_across_the_cycle_and_wraps() {
        assert!((Waveform::RampUp.sample(0.0) - (-1.0)).abs() < 0.001);
        assert!((Waveform::RampUp.sample(0.5) - 0.0).abs() < 0.001);
        assert!((Waveform::RampUp.sample(0.999) - 1.0).abs() < 0.01);
        // Wraps: 1.25 has the same fractional part as 0.25.
        assert!((Waveform::RampUp.sample(1.25) - Waveform::RampUp.sample(0.25)).abs() < 0.001);
    }

    fn groups() -> Vec<Group> {
        vec![Group { name: "Pars".to_string(), chans: vec![1, 2, 3, 4] }]
    }

    #[test]
    fn a_full_spread_chase_offsets_each_fixture_differently() {
        let player = EffectPlayer::new(vec![EffectRecipe {
            name: "Chase".to_string(),
            target: RecipeTarget::Group("Pars".to_string()),
            attr: Attribute::Dimmer,
            waveform: Waveform::Sine,
            rate_hz: 1.0,
            size: 1.0,
            base: 0.0,
            phase_spread_deg: 360.0,
            phase_offset_deg: 0.0,
            direction: EffectDirection::Forward,
        }]);
        let out = player.output(&groups());
        let v1 = *out.get(&(1, Attribute::Dimmer)).unwrap();
        let v2 = *out.get(&(2, Attribute::Dimmer)).unwrap();
        let v3 = *out.get(&(3, Attribute::Dimmer)).unwrap();
        assert!(v1 != v2 && v2 != v3, "a full-spread chase should offset every fixture's phase");
    }

    #[test]
    fn zero_spread_moves_every_fixture_in_lockstep() {
        let player = EffectPlayer::new(vec![EffectRecipe {
            name: "Pulse".to_string(),
            target: RecipeTarget::Group("Pars".to_string()),
            attr: Attribute::Dimmer,
            waveform: Waveform::Sine,
            rate_hz: 1.0,
            size: 0.5,
            base: 0.5,
            phase_spread_deg: 0.0,
            phase_offset_deg: 0.0,
            direction: EffectDirection::Forward,
        }]);
        let out = player.output(&groups());
        let v1 = *out.get(&(1, Attribute::Dimmer)).unwrap();
        let v4 = *out.get(&(4, Attribute::Dimmer)).unwrap();
        assert!((v1 - v4).abs() < 0.0001, "zero spread should keep every fixture in lockstep");
    }

    #[test]
    fn tick_advances_the_waveform_over_time() {
        let mut player = EffectPlayer::new(vec![EffectRecipe {
            name: "Pulse".to_string(),
            target: RecipeTarget::Chans(vec![1]),
            attr: Attribute::Dimmer,
            waveform: Waveform::RampUp,
            rate_hz: 1.0,
            size: 1.0,
            base: 0.0,
            phase_spread_deg: 0.0,
            phase_offset_deg: 0.0,
            direction: EffectDirection::Forward,
        }]);
        let before = *player.output(&[]).get(&(1, Attribute::Dimmer)).unwrap();
        player.tick(0.25); // a quarter cycle at 1Hz
        let after = *player.output(&[]).get(&(1, Attribute::Dimmer)).unwrap();
        assert!(after > before, "a ramp should have risen after ticking forward");
    }

    #[test]
    fn a_90_degree_offset_pair_traces_a_circle_on_two_attributes() {
        // The Pan/Tilt-circle trick this module's own doc describes: two
        // effects, same rate, 90 degrees apart, sine on each axis.
        let mut pan = EffectPlayer::new(vec![EffectRecipe {
            name: "Pan".to_string(),
            target: RecipeTarget::Chans(vec![1]),
            attr: Attribute::Pan,
            waveform: Waveform::Sine,
            rate_hz: 1.0,
            size: 30.0,
            base: 0.0,
            phase_spread_deg: 0.0,
            phase_offset_deg: 0.0,
            direction: EffectDirection::Forward,
        }]);
        let mut tilt = EffectPlayer::new(vec![EffectRecipe {
            name: "Tilt".to_string(),
            target: RecipeTarget::Chans(vec![1]),
            attr: Attribute::Tilt,
            waveform: Waveform::Sine,
            rate_hz: 1.0,
            size: 30.0,
            base: 0.0,
            phase_spread_deg: 0.0,
            phase_offset_deg: 90.0,
            direction: EffectDirection::Forward,
        }]);
        // At t=0: pan should be at its base (sine(0)=0), tilt should be at
        // its peak (sine(0.25 cycle)=1) — the two axes 90 degrees apart.
        let pan_v = *pan.output(&[]).get(&(1, Attribute::Pan)).unwrap();
        let tilt_v = *tilt.output(&[]).get(&(1, Attribute::Tilt)).unwrap();
        assert!(pan_v.abs() < 0.5, "pan should start near its base");
        assert!((tilt_v - 30.0).abs() < 0.5, "tilt should start near its peak, 90deg ahead of pan");
        pan.tick(0.25);
        tilt.tick(0.25);
        let pan_v2 = *pan.output(&[]).get(&(1, Attribute::Pan)).unwrap();
        let tilt_v2 = *tilt.output(&[]).get(&(1, Attribute::Tilt)).unwrap();
        assert!((pan_v2 - 30.0).abs() < 0.5, "a quarter cycle later, pan should be at its own peak");
        assert!(tilt_v2.abs() < 0.5, "...and tilt should have come back down to its base");
    }
}
