//! Bumps — a moment, not a state.
//!
//! A bump snaps something up and lets it fall back on its own. It is the
//! single most-used gesture in busking a band, and it arrives from two
//! completely different places:
//!
//! - a **charted hit**, fired by the transport passing it ([`crate::trigger`])
//! - an operator's **hand**, on a flash key
//!
//! Those are the same event. Building them as one object is the whole
//! point of this module: a snare that flashes the rig and an operator
//! flashing the rig on the same snare should be indistinguishable in the
//! output, and should stay that way when either side is changed. Two
//! implementations would drift within a week, and the drift would show
//! up as "the chart feels different from playing it by hand" — a
//! complaint nobody can debug.
//!
//! # Why these are one-shots and not held
//!
//! A real flash key is momentary: held down it stays up, released it
//! falls. That is a *state*, and modelling it that way means the release
//! has to arrive — which it does not, if the operator's hand slips, or a
//! MIDI note-off is dropped, or a charted hit has no matching end. A
//! one-shot envelope cannot get stuck on. The cost is that holding a
//! flash key does not hold the light, and for a rig driven mostly by a
//! chart that is the right trade.

use crate::Attribute;
use crate::recipe::{Recipe, RecipeApply};
use crate::selection::Selection;
use crate::step::{Speed, Step, Timing};
use ignition_proto::ColorChannel;

/// How a bump behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
// r[impl effects.bump]
pub enum Kind {
    /// Lift the level and let it fall. The plain one.
    Level,
    /// Snap to white. Reads through *any* colour, which is why it is the
    /// accent that always works — a level bump inside a deep blue look
    /// is a slightly brighter deep blue, and lands as nothing.
    White,
    /// Push the colour that is already there harder, rather than
    /// replacing it. Keeps the look's identity and raises its intensity,
    /// so a red chorus punches red rather than punching white.
    ColorBoost,
    /// A short burst of hard on/off. The biggest thing available, and
    /// the one to spend sparingly.
    Burst,
}

impl Kind {
    /// The name an operator sees.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Level => "bump",
            Kind::White => "white",
            Kind::ColorBoost => "boost",
            Kind::Burst => "burst",
        }
    }
}

/// How long a bump takes to fall back, in beats.
///
/// Just under an eighth, so a figure written in eighths has each flash
/// clear before the next arrives. Longer and hits smear into one
/// another; much shorter and the rig ticks rather than punches.
// r[impl effects.bump.fall-beats]
pub const FALL_BEATS: f32 = 0.45;

/// A bump on a selection.
///
/// `depth` is 0..=1 — how hard. A charted hit passes its class weight;
/// a flash key passes 1.
// r[impl effects.bump]
// r[impl effects.bump.one-object]
// r[impl playback.flash-equals-hit]
// r[impl effects.bump.is-not-held]
// r[impl effects.bump.fall-beats]
// r[impl recipes.one-shot]
pub fn bump(target: Selection, kind: Kind, depth: f32) -> Recipe {
    let depth = depth.clamp(0.0, 1.0);
    Recipe {
        target,
        steps: match kind {
            Kind::Level => envelope(vec![(Attribute::Dimmer, 0.7 * depth)]),
            // Every emitter at once. A white bump has to *win* over the
            // colour beneath it, and adding to only red would tint
            // rather than flash.
            // r[impl effects.bump.shape] - White drives every emitter
            Kind::White => envelope(vec![
                (Attribute::Dimmer, 0.5 * depth),
                (
                    Attribute::ColorAdd {
                        channel: ColorChannel::Red,
                    },
                    0.9 * depth,
                ),
                (
                    Attribute::ColorAdd {
                        channel: ColorChannel::Green,
                    },
                    0.9 * depth,
                ),
                (
                    Attribute::ColorAdd {
                        channel: ColorChannel::Blue,
                    },
                    0.9 * depth,
                ),
                (
                    Attribute::ColorAdd {
                        channel: ColorChannel::White,
                    },
                    0.9 * depth,
                ),
            ]),
            // Dimmer only, and harder than `Level`. Pushing the level of
            // a saturated look *is* boosting its colour: the hue is
            // already there, and adding white emitters would wash it out
            // — which is the opposite of the intent.
            // r[impl effects.bump.shape] - ColorBoost touches only the level
            Kind::ColorBoost => envelope(vec![(Attribute::Dimmer, 0.95 * depth)]),
            Kind::Burst => burst(depth),
        },
        timing: Timing {
            // r[impl effects.bump.fall-beats] - measured in beats against Song
            speed: Speed::Master("Song".into()),
            measure: FALL_BEATS,
            // Runs once and holds at zero, which for a `Delta` is the
            // same as not being there. A looping bump is a strobe.
            // r[impl effects.bump.is-not-held]
            // r[impl effects.once]
            once: true,
            ..Default::default()
        },
        tricks: Vec::new(),
        stack: false,
        ..Default::default()
    }
}

/// Snap up, fall back.
///
/// The lift snaps (`transition: 0.0`) because a hit that eases in has
/// already missed the moment it was for. The fall is three times as wide
/// and transitions the whole way, so it is a fall rather than a second
/// snap.
// r[impl effects.bump.shape]
// r[impl effects.delta-ends-at-nothing]
// r[impl effects.modulates-with-delta]
fn envelope(up: Vec<(Attribute, f32)>) -> Vec<Step> {
    let down = up.iter().map(|(a, _)| (a.clone(), 0.0)).collect();
    vec![
        Step {
            apply: vec![RecipeApply::Delta(up)],
            width: 1.0,
            transition: 0.0,
            ..Step::new(Vec::new())
        },
        Step {
            apply: vec![RecipeApply::Delta(down)],
            width: 3.0,
            transition: 1.0,
            ..Step::new(Vec::new())
        },
    ]
}

/// Hard on/off, four times, then out.
// r[impl effects.bump] - Burst
// r[impl effects.delta-ends-at-nothing]
fn burst(depth: f32) -> Vec<Step> {
    let hit = |v: f32| Step {
        apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, v)])],
        width: 1.0,
        transition: 0.0,
        ..Step::new(Vec::new())
    };
    vec![
        hit(0.9 * depth),
        hit(0.0),
        hit(0.9 * depth),
        hit(0.0),
        hit(0.9 * depth),
        hit(0.0),
        Step {
            apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])],
            width: 4.0,
            transition: 1.0,
            ..Step::new(Vec::new())
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::Group;
    use crate::recipe::{Emit, Show, expand_recipe};
    use crate::selection::EMPTY_RIG;

    fn show(groups: &[Group]) -> Show<'_> {
        Show::new(groups, &EMPTY_RIG)
    }

    fn pars() -> Vec<Group> {
        vec![Group {
            name: "Pars".into(),
            chans: vec![1, 2],
        }]
    }

    fn at(kind: Kind, secs: f32) -> Vec<Emit> {
        let groups = pars();
        let recipe = bump(Selection::Group("Pars".into()), kind, 1.0);
        expand_recipe(&recipe, &show(&groups), secs)
    }

    fn dimmer(emits: &[Emit]) -> f32 {
        emits
            .iter()
            .find(|e| e.value.attr == Attribute::Dimmer && e.value.chan == 1)
            .map(|e| e.value.value)
            .unwrap_or_default()
    }

    /// Every bump is relative, so it adds to the look rather than
    /// replacing it. A bump that overwrote would take the colour with
    /// it and leave the rig somewhere the cue never asked for.
    // r[verify effects.modulates-with-delta]
    // r[verify effects.bump]
    #[test]
    fn every_bump_is_relative() {
        for kind in [Kind::Level, Kind::White, Kind::ColorBoost, Kind::Burst] {
            let recipe = bump(Selection::Group("Pars".into()), kind, 1.0);
            for step in &recipe.steps {
                assert!(
                    step.apply
                        .iter()
                        .all(|a| matches!(a, RecipeApply::Delta(_))),
                    "{:?} is not relative",
                    kind
                );
            }
        }
    }

    /// It lifts at the moment it fires — the whole point of a bump.
    // r[verify effects.bump.shape]
    #[test]
    fn a_bump_lifts_immediately() {
        assert!(dimmer(&at(Kind::Level, 0.0)) > 0.5);
    }

    /// A white bump touches every emitter, not just one. Adding to red
    /// alone would tint rather than flash, which inside a blue look
    /// reads as nothing happening.
    // r[verify effects.bump.shape]
    #[test]
    fn a_white_bump_drives_every_emitter() {
        let emits = at(Kind::White, 0.0);
        for channel in [
            ColorChannel::Red,
            ColorChannel::Green,
            ColorChannel::Blue,
            ColorChannel::White,
        ] {
            let attr = Attribute::ColorAdd { channel };
            assert!(
                emits
                    .iter()
                    .any(|e| e.value.attr == attr && e.value.value > 0.5),
                "no lift on {channel:?}"
            );
        }
    }

    /// A colour boost leaves the colour alone and pushes the level.
    /// Adding white emitters to a saturated look washes it out, which is
    /// the opposite of boosting it.
    // r[verify effects.bump.shape]
    #[test]
    fn a_colour_boost_does_not_add_white() {
        let emits = at(Kind::ColorBoost, 0.0);
        assert!(dimmer(&emits) > 0.8, "it did not boost");
        assert!(
            !emits
                .iter()
                .any(|e| matches!(e.value.attr, Attribute::ColorAdd { .. })),
            "a boost tinted the look"
        );
    }

    /// Depth scales it, so a charted soft hit is softer than a hard one
    /// without being a different bump.
    // r[verify effects.bump]
    #[test]
    fn depth_scales_the_lift() {
        let groups = pars();
        let soft = bump(Selection::Group("Pars".into()), Kind::Level, 0.25);
        let hard = bump(Selection::Group("Pars".into()), Kind::Level, 1.0);
        let level = |r: &Recipe| dimmer(&expand_recipe(r, &show(&groups), 0.0));
        assert!(level(&soft) < level(&hard) * 0.5);
    }

    /// It ends at nothing. A bump that settled anywhere but zero would
    /// leave the rig permanently brighter every time one fired, and a
    /// song's worth of snares would ratchet the whole show up.
    // r[verify effects.delta-ends-at-nothing]
    #[test]
    fn a_bump_ends_at_nothing() {
        let recipe = bump(Selection::Group("Pars".into()), Kind::Level, 1.0);
        let last = recipe.steps.last().expect("steps");
        for apply in &last.apply {
            if let RecipeApply::Delta(pairs) = apply {
                assert!(pairs.iter().all(|(_, v)| *v == 0.0), "ends at {pairs:?}");
            }
        }
    }

    /// One-shot, so it cannot get stuck on. A held flash key whose
    /// release never arrives — a slipped hand, a dropped note-off — is
    /// the failure this design refuses to have.
    // r[verify effects.bump.is-not-held]
    // r[verify effects.once]
    #[test]
    fn a_bump_cannot_stick_on() {
        let recipe = bump(Selection::Group("Pars".into()), Kind::Level, 1.0);
        assert!(recipe.timing.once);
    }
}
