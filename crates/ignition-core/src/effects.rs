//! The default effects library.
//!
//! Named recipes written against **roles**, so every one of them resolves
//! at any venue implementing the profile. This is what the whole
//! Profile/Venue/Trick apparatus was for: a programmer picks `chase` and
//! `Wash`, and it works in Norco, Riverside and a room nobody has built
//! yet.
//!
//! Three rules run through all of it, and they are what make these
//! *layerable* rather than a list of looks:
//!
//! **Relative wherever possible.** A `Delta` says how much to add or take
//! away and leaves everything it did not name alone — so a chase runs
//! over whatever colour the cue set, instead of replacing it. An
//! absolute effect is a look; a relative one is an effect.
//!
//! **Slaved to `Song`.** Every rate is in bars, against the song's own
//! tempo master, so "one cycle per bar" is one cycle per bar of *this*
//! song and a tempo change carries the whole library with it.
//!
//! **Direction comes from the selection.** Nothing here says "left to
//! right"; that is an X-ordered selection, and the same effect reads as a
//! direction because the *selection* knows the room.
//!
//! Built in Rust rather than hand-written as JSON because fifteen recipes
//! of nested step tables is a lot of punctuation to get right by eye, and
//! because these are worth testing. They ship *as* data — see the
//! `effects` map on `Profile` — so a venue or a person can add their own
//! without touching this.

use crate::recipe::{Recipe, RecipeApply};
use crate::selection::Selection;
use crate::step::{Play, Speed, Step, Timing, Waveform};
use crate::tricks::Trick;
use crate::Attribute;
use std::collections::BTreeMap;

/// The role an effect is written against, as a selection.
fn role(name: &str) -> Selection {
    Selection::Role(name.into())
}

/// A timing slaved to the song, `bars` per cycle.
fn beat(bars: f32, spread: f32, direction: Play) -> Timing {
    Timing {
        speed: Speed::Master("Song".into()),
        // `measure` is in beats, and everything here is authored in bars,
        // because a lighting idea is "once a bar" or "twice a bar" and
        // never "every 3.7 beats".
        measure: bars * 4.0,
        phase_spread_deg: spread,
        direction,
        ..Default::default()
    }
}

/// A step setting one relative attribute.
fn delta(attr: Attribute, v: f32) -> Step {
    Step::new(vec![RecipeApply::Delta(vec![(attr, v)])])
}

/// Every effect in the default library.
///
/// Keyed by the name a programmer types. Names are lower case and plain
/// English: this is a list somebody scrolls at a desk, and `chase` beats
/// `IntensityChaseForward`.
pub fn library() -> BTreeMap<String, Recipe> {
    let mut out = BTreeMap::new();
    let mut add = |name: &str, recipe: Recipe| {
        out.insert(name.to_string(), recipe);
    };

    // ── intensity ────────────────────────────────────────────────────

    // The plain travelling chase. A full 360 of spread means each
    // fixture is a full cycle behind its neighbour across the selection,
    // which is what makes one point of light travel rather than the rig
    // breathing together.
    add(
        "chase",
        Recipe {
            target: role("Wash"),
            steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.8)],
            timing: beat(1.0, 360.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // The same shape with the swing inverted: the travelling point is
    // the one that *drops out*. grandMA3's docs single this out as the
    // thing forward chasing cannot do, and it reads completely
    // differently — a hole moving through a lit rig rather than a light
    // moving through a dark one.
    add(
        "dark chase",
        Recipe {
            target: role("Wash"),
            steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.8)],
            timing: beat(1.0, 360.0, Play::Negative),
            tricks: Vec::new(),
        },
    );

    // Everything together. Zero spread is the whole point: this is the
    // rig breathing, not something travelling across it.
    add(
        "pulse",
        Recipe {
            target: role("Wash"),
            steps: Waveform::Sine.steps(Attribute::Dimmer, -0.25, 0.25, true),
            timing: beat(1.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // Half-time pulse, for a verse that should move without being busy.
    add(
        "slow swell",
        Recipe {
            target: role("Wash"),
            steps: Waveform::Sine.steps(Attribute::Dimmer, -0.2, 0.2, true),
            timing: beat(4.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // Fixtures arrive and stay until the cycle wraps, so the rig fills up
    // and resets. Not a phase shift of a shared wave — a threshold
    // against each fixture's own place in the selection, which is why
    // `Play::Build` is a mode rather than a step table.
    add(
        "build",
        Recipe {
            target: role("Wash"),
            steps: vec![delta(Attribute::Dimmer, -0.7), delta(Attribute::Dimmer, 0.0)],
            timing: beat(2.0, 360.0, Play::Build),
            tricks: Vec::new(),
        },
    );

    // Out and back rather than snapping at the wrap, so the wave washes.
    add(
        "bounce",
        Recipe {
            target: role("Wash"),
            steps: Waveform::Triangle.steps(Attribute::Dimmer, -0.35, 0.35, true),
            timing: beat(2.0, 360.0, Play::Bounce),
            tricks: Vec::new(),
        },
    );

    // Hard on/off at eighths. Absolute rather than relative on purpose:
    // a strobe that only modulated would still show whatever was under
    // it, and a strobe is supposed to be the only thing you can see.
    add(
        "strobe",
        Recipe {
            target: role("Wash"),
            steps: vec![
                Step::new(vec![RecipeApply::Dimmer(1.0)]),
                Step::new(vec![RecipeApply::Dimmer(0.0)]),
            ],
            timing: beat(0.5, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // A shuffled chase reads as twinkle rather than as travel: the same
    // one point of light, visiting the rig in an order nobody can
    // predict — but a *reproducible* one, so the look can be recalled.
    add(
        "sparkle",
        Recipe {
            target: role("Wash"),
            steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.85)],
            timing: beat(1.0, 360.0, Play::Forward),
            tricks: vec![Trick::Shuffle(1741)],
        },
    );

    // Odds and evens trading places — the cheapest effect that reads as
    // deliberate rather than as a chase, and it survives any rig size
    // because `Group(2)` is a proportion.
    add(
        "alternate",
        Recipe {
            target: role("Wash"),
            steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.8)],
            timing: beat(1.0, 180.0, Play::Forward),
            tricks: vec![Trick::Group(2)],
        },
    );

    // Opening outward from centre. `Wings(2)` mirrors the back half, so
    // one definition runs symmetrically — the thing that would otherwise
    // need two hand-authored chases pointed at each other.
    add(
        "open out",
        Recipe {
            target: role("Wash"),
            steps: vec![delta(Attribute::Dimmer, -0.7), delta(Attribute::Dimmer, 0.0)],
            timing: beat(2.0, 360.0, Play::Build),
            tricks: vec![Trick::Wings(2)],
        },
    );

    // A one-shot: snaps up and falls back on its own, holding at zero,
    // which for a `Delta` is the same as not being there. This is what a
    // charted hit fires — see `crate::trigger`.
    add(
        "bump",
        Recipe {
            target: role("Wash"),
            steps: vec![
                Step {
                    apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.6)])],
                    width: 1.0,
                    // Snapping up is what makes it a hit. A bump that
                    // eases in has already missed the moment it was for.
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
                Step {
                    apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])],
                    width: 3.0,
                    transition: 1.0,
                    ..Step::new(Vec::new())
                },
            ],
            timing: Timing {
                once: true,
                ..beat(0.125, 0.0, Play::Forward)
            },
            tricks: Vec::new(),
        },
    );

    // ── movement ─────────────────────────────────────────────────────

    // Pan spread across the movers with no motion of its own — a static
    // fan, which is a *look* rather than an effect and is here because
    // it is the shape everything else is built on.
    add(
        "fan",
        Recipe {
            target: role("Movers"),
            steps: vec![Step::new(vec![RecipeApply::Delta(vec![(
                Attribute::Pan,
                30.0,
            )])])],
            timing: Timing {
                phase_spread_deg: 360.0,
                ..beat(1.0, 360.0, Play::Forward)
            },
            tricks: Vec::new(),
        },
    );

    // A slow pan swing, a whole number of bars per sweep so it lands
    // with the music instead of drifting against it.
    add(
        "sweep",
        Recipe {
            target: role("Movers"),
            steps: Waveform::Sine.steps(Attribute::Pan, 0.0, 25.0, true),
            timing: beat(4.0, 180.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // The classic, and the reason phase offset exists as a separate
    // layer from phase spread: a circle is two sine waves a quarter
    // cycle apart, on Pan and Tilt. No dedicated "position effect" type
    // is needed, and pairing any two attributes this way gives the same
    // trick for free.
    add(
        "circle pan",
        Recipe {
            target: role("Movers"),
            steps: Waveform::Sine.steps(Attribute::Pan, 0.0, 20.0, true),
            timing: beat(2.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );
    add(
        "circle tilt",
        Recipe {
            target: role("Movers"),
            steps: Waveform::Sine.steps(Attribute::Tilt, 0.0, 20.0, true),
            timing: Timing {
                // The quarter cycle that turns two swings into a circle.
                phase_offset_deg: 90.0,
                ..beat(2.0, 0.0, Play::Forward)
            },
            tricks: Vec::new(),
        },
    );

    // A figure of eight is the same pair with tilt at twice the rate.
    add(
        "figure eight tilt",
        Recipe {
            target: role("Movers"),
            steps: Waveform::Sine.steps(Attribute::Tilt, 0.0, 15.0, true),
            timing: Timing {
                phase_offset_deg: 90.0,
                ..beat(1.0, 0.0, Play::Forward)
            },
            tricks: Vec::new(),
        },
    );

    // Movers thrown apart in mirrored halves, so the rig opens rather
    // than all sweeping the same way.
    add(
        "mirror sweep",
        Recipe {
            target: role("Movers"),
            steps: Waveform::Sine.steps(Attribute::Pan, 0.0, 30.0, true),
            timing: beat(4.0, 360.0, Play::Forward),
            tricks: vec![Trick::Wings(2)],
        },
    );

    // ── accent layers ────────────────────────────────────────────────

    // The bar lights ticking, for a verse that wants texture rather than
    // movement. On `Bars` because that is the layer that can carry a
    // pulse without competing with the wash the vocal is lit by.
    add(
        "bar tick",
        Recipe {
            target: role("Bars"),
            steps: vec![delta(Attribute::Dimmer, 0.12), delta(Attribute::Dimmer, 0.0)],
            timing: beat(0.5, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // A chase along the bars, which reads as travel far more strongly
    // than the same chase on a wash because the fixtures are in a line.
    add(
        "bar chase",
        Recipe {
            target: role("Bars"),
            steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.6)],
            timing: beat(1.0, 360.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // The back wall breathing under everything. Slow and shallow: this
    // is meant to be noticed only when it stops.
    add(
        "back breathe",
        Recipe {
            target: role("Back"),
            steps: Waveform::Sine.steps(Attribute::Dimmer, -0.12, 0.12, true),
            timing: beat(8.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::RoleKind;

    /// Every effect targets a role, never a venue's own group name.
    ///
    /// The rule the library exists to keep. One `Selection::Group` in
    /// here and that effect works at exactly one address, which is worse
    /// than it not existing — it would be picked from the same list as
    /// the portable ones and fail somewhere else.
    #[test]
    fn every_effect_targets_a_role() {
        for (name, recipe) in library() {
            assert!(
                matches!(recipe.target, Selection::Role(_)),
                "effect {name:?} targets {:?}, not a role",
                recipe.target
            );
        }
    }

    /// Every role an effect names is one the default profile declares.
    ///
    /// Without this the library could quietly depend on a role nobody
    /// implements, and the failure would appear at a venue rather than
    /// here.
    #[test]
    fn every_targeted_role_is_declared() {
        let profile: crate::profile::Profile = serde_json::from_str(
            &std::fs::read_to_string("../../data/profiles/ignition.ig-profile")
                .unwrap_or_default(),
        )
        .unwrap_or_default();
        if profile.roles.is_empty() {
            return;
        }
        let declared = profile.vocabulary(RoleKind::Group);
        for (name, recipe) in library() {
            if let Selection::Role(role) = &recipe.target {
                assert!(
                    declared.contains(&role.as_str()),
                    "effect {name:?} targets role {role:?}, which the profile does not declare"
                );
            }
        }
    }

    /// Every rate is against the song, so the library follows a tempo
    /// change rather than needing to be re-dialled.
    #[test]
    fn every_effect_is_slaved_to_the_song() {
        for (name, recipe) in library() {
            assert!(
                matches!(&recipe.timing.speed, Speed::Master(m) if m == "Song"),
                "effect {name:?} runs on {:?} rather than the song",
                recipe.timing.speed
            );
        }
    }

    /// Effects layer, so all but the deliberate exceptions are relative.
    ///
    /// `strobe` is absolute on purpose — a strobe that only modulated
    /// would still show what was under it — and naming the exception
    /// here is what stops the next one being added by accident.
    #[test]
    fn effects_are_relative_except_where_named() {
        let absolute: Vec<String> = library()
            .into_iter()
            .filter(|(_, r)| {
                r.steps.iter().any(|s| {
                    s.apply
                        .iter()
                        .any(|a| !matches!(a, RecipeApply::Delta(_)))
                })
            })
            .map(|(name, _)| name)
            .collect();
        assert_eq!(absolute, vec!["strobe".to_string()]);
    }

    /// A circle is two swings a quarter cycle apart. If the offset were
    /// ever lost the pair would become one diagonal sweep, which looks
    /// deliberate enough that nobody would notice it was wrong.
    #[test]
    fn the_circle_pair_is_a_quarter_cycle_apart() {
        let lib = library();
        let pan = &lib["circle pan"];
        let tilt = &lib["circle tilt"];
        assert_eq!(pan.timing.phase_offset_deg, 0.0);
        assert_eq!(tilt.timing.phase_offset_deg, 90.0);
        assert_eq!(pan.timing.measure, tilt.timing.measure);
    }

    /// A bump runs once and holds. Looping, it is a strobe.
    #[test]
    fn the_bump_is_a_one_shot() {
        assert!(library()["bump"].timing.once);
        assert!(!library()["chase"].timing.once);
    }

    /// Effects with two or more steps are phasers; the static ones are
    /// looks. Both are recipes, which is the point — but an effect that
    /// meant to move and has one step would silently sit still.
    #[test]
    fn the_moving_effects_have_more_than_one_step() {
        for name in ["chase", "pulse", "build", "sweep", "sparkle"] {
            assert!(
                library()[name].steps.len() > 1,
                "{name:?} has one step and cannot move"
            );
        }
    }
}
