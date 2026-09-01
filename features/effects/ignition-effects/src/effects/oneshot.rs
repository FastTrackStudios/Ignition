//! One-shots — bumps, blinders, white-outs, fills, drains, wipes,
//! lightning, risers, and the movers' arrivals and departures.
//!
//! See `super` for the three rules every effect here follows: relative
//! wherever possible, slaved to `Song`, direction from the selection.
//!
//! Everything here runs `once` and holds its last step. For a `Delta`
//! that ends at zero, holding is the same as not being there, which is
//! how a hit clears itself without a release ever having to arrive —
//! see `crate::bump` for why that is the right trade. The ones that
//! *hold* on purpose (`drain and hold`, `lift off`, `strobe riser`, the
//! strip drain and dissolve, the wipes) end somewhere other than zero
//! and sit there until the cue drops them. A fill ends *at* zero: it
//! arrives at the level the cue set and stays, which is what filling
//! means, and it is why a fill cannot ratchet.
//!
//! Where an effect is a bump — snap up, fall back — it is built by
//! `crate::bump::bump`, so a charted hit and a hand on a flash key fire
//! the very same object.

use super::*;
use crate::bump::{self, Kind};
use crate::step::Ease;
use ignition_proto::ColorChannel;

const FAMILY: &str = "one-shot";

/// A snapping delta step of a given width.
fn snap(pairs: Vec<(Attribute, f32)>, width: f32) -> Step {
    Step {
        apply: vec![RecipeApply::Delta(pairs)],
        width,
        transition: 0.0,
        ..Step::new(Vec::new())
    }
}

/// A fully-transitioning delta step — a ramp into these values.
fn ramp(pairs: Vec<(Attribute, f32)>, width: f32) -> Step {
    Step {
        transition: 1.0,
        ..snap(pairs, width)
    }
}

/// An eased ramp.
fn ease(pairs: Vec<(Attribute, f32)>, width: f32) -> Step {
    Step {
        ease: Ease::Sine,
        ..ramp(pairs, width)
    }
}

/// "Hit hard, fall soft": a fall that leaves at speed and settles
/// gently. What a struck light does — the afterimage is in the tail.
// r[impl effects.step.accel-decel] - used where the shape is honest
const HARD_SOFT: Ease = Ease::Curve {
    accel: 1.0,
    decel: -1.0,
};

/// A ramp that falls hard and settles soft.
fn fall(pairs: Vec<(Attribute, f32)>, width: f32) -> Step {
    Step {
        ease: HARD_SOFT,
        ..ramp(pairs, width)
    }
}

/// The stage as one selection — everything but the audience.
// r[impl effects.library.missing-role-is-empty]
fn stage() -> Selection {
    Selection::Union(vec![
        role("Key"),
        role("Wash"),
        role("Back"),
        role("Bars"),
        role("Movers"),
        role("Beams"),
        role("Floor"),
    ])
}

/// A one-shot on a role.
fn on(target: Selection, steps: Vec<Step>, timing: Timing, tricks: Vec<Trick>) -> Recipe {
    Recipe {
        target,
        steps,
        timing,
        tricks,
        stack: false,
        ..Default::default()
    }
}

/// Adds this family to the library.
// r[impl effects.library.categories]
pub(super) fn add(add: Add) {
    let mut put = |name: &str, about: &str, recipe: Recipe| add(name, FAMILY, about, recipe);

    // ── hits ─────────────────────────────────────────────────────────

    // Snaps up and falls back on its own, holding at zero, which for a
    // `Delta` is the same as not being there. This is what a charted hit
    // fires — see `crate::trigger` — and `bump::bump` is where it is
    // built, so the chart and a flash key fire the same object.
    // r[impl effects.bump.one-object]
    put(
        "bump",
        "the wash snapping up and falling straight back — every charted hit, snares and stabs",
        bump::bump(role("Wash"), Kind::Level, 0.85),
    );

    // A very short whole-rig lift: an eighth up and gone. Harder and
    // shorter than a bump, with no fall — a snare, not a crash.
    put(
        "stab",
        "the whole stage jumping up for an eighth and straight back — accents on a stop or a stab",
        on(
            stage(),
            vec![
                snap(vec![(Attribute::Dimmer, 0.9)], 1.0),
                snap(vec![(Attribute::Dimmer, 0.0)], 1.0),
            ],
            once(0.125, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // The blinders hitting the room and falling over two beats. This is
    // `bump` on the audience — the same object a charted hit fires —
    // with a longer fall, because a blinder that clears in half a beat
    // is a flash and a blinder is meant to leave an afterimage. The fall
    // leaves hard and settles soft: the afterimage is in the tail.
    // r[impl effects.bump.one-object]
    // r[impl effects.step.accel-decel] - the fall is hard/soft
    put(
        "blinder hit",
        "the blinders hitting the room white and fading over two beats — the downbeat of a last chorus, three hits at most",
        {
            let mut hit = bump::bump(role("Audience"), Kind::White, 1.0);
            hit.timing.measure = 2.0;
            if let Some(last) = hit.steps.last_mut() {
                last.ease = HARD_SOFT;
            }
            hit
        },
    );

    // Snap the colour hotter, then fall back. The colour cousin of a
    // level bump: red and amber pushed and green and blue held back,
    // which reads as the look flushing toward the hot end for a moment
    // rather than getting brighter.
    put(
        "colour bump",
        "the wash flushing hot for a moment and settling back to its colour — hits in a section that is already bright",
        on(
            role("Wash"),
            vec![
                snap(
                    vec![
                        (add_chan(ColorChannel::Red), 0.8),
                        (add_chan(ColorChannel::Amber), 0.6),
                        (add_chan(ColorChannel::Green), -0.4),
                        (add_chan(ColorChannel::Blue), -0.6),
                    ],
                    1.0,
                ),
                fall(
                    vec![
                        (add_chan(ColorChannel::Red), 0.0),
                        (add_chan(ColorChannel::Amber), 0.0),
                        (add_chan(ColorChannel::Green), 0.0),
                        (add_chan(ColorChannel::Blue), 0.0),
                    ],
                    3.0,
                ),
            ],
            Timing {
                once: true,
                ..bump::bump(role("Wash"), Kind::Level, 1.0).timing
            },
            Vec::new(),
        ),
    );

    // ── the whole rig, once ──────────────────────────────────────────

    // Everything to open white, full, for a bar, then released. Every
    // emitter is driven, not just the level, because a white-out has to
    // win over whatever colour is under it — a blue look with the
    // dimmer pushed is a bright blue look.
    put(
        "white out",
        "the whole stage to full white for a bar, then released — the last hit of the song, once",
        on(
            stage(),
            vec![snap(every_emitter(1.0), 4.0), fall(every_emitter(0.0), 1.0)],
            once(1.25, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // A dark blink: the rig *off* for a quarter of a beat and straight
    // back. The negative of a bump, and it reads as a cut — the one
    // accent that works when the rig is already at full.
    put(
        "negative flash",
        "the whole stage cut to black for an instant and straight back — accents on a full stage where a bump has nowhere to go",
        on(
            stage(),
            vec![
                snap(vec![(Attribute::Dimmer, -1.0)], 1.0),
                snap(vec![(Attribute::Dimmer, 0.0)], 1.0),
            ],
            once(0.125, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Strobe on for a bar, then off. Relative on the strobe attribute —
    // a full-scale delta from a look with no strobe *is* full strobe —
    // so the look's colour and level stay where the cue put them.
    put(
        "strobe burst",
        "everything strobing hard for one bar, then stopping — the drop, twice a song at most",
        on(
            Selection::Union(vec![role("Beams"), role("Wash")]),
            vec![
                snap(vec![(Attribute::Strobe, 1.0)], 4.0),
                snap(vec![(Attribute::Strobe, 0.0)], 1.0),
            ],
            once(1.25, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // The strobe rate climbing from nothing to full over eight bars and
    // holding there. The build-up under a riser, ended by whatever cue
    // lands the drop. Eight steps, so the climb is a curve the
    // interpolation can draw rather than one long linear crawl.
    put(
        "strobe riser",
        "the beams' strobe climbing from nothing to full over eight bars and holding — under a riser, ended by the drop",
        on(
            role("Beams"),
            (1..=8)
                .map(|i| {
                    let t = i as f32 / 8.0;
                    // Soft leaving, hard arriving: each bar's climb
                    // accelerates into the next, which is what a riser is.
                    // r[impl effects.step.accel-decel]
                    Step {
                        ease: Ease::Curve {
                            accel: -1.0,
                            decel: 1.0,
                        },
                        ..ramp(vec![(Attribute::Strobe, t * t)], 1.0)
                    }
                })
                .collect(),
            once(8.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Two fast white flashes, a third, then a long dim afterglow that
    // fades out. Lightning, and the afterglow is what sells it — a
    // strike with no decay is a strobe frame.
    put(
        "lightning strike",
        "two white flashes, a third, and a dim afterglow fading over a bar — a single strike on a break or a crash",
        on(
            Selection::Union(vec![role("Back"), role("Bars"), role("Wash")]),
            vec![
                snap(every_emitter(1.0), 1.0),
                snap(every_emitter(0.0), 1.0),
                snap(every_emitter(1.0), 1.0),
                snap(every_emitter(0.0), 2.0),
                snap(every_emitter(0.8), 1.0),
                snap(every_emitter(0.25), 2.0),
                fall(every_emitter(0.0), 8.0),
            ],
            once(1.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── fills and drains ─────────────────────────────────────────────

    // The rig filling over a bar and staying full. `Build` arrives
    // fixture by fixture; `once` stops the wrap emptying them; and the
    // last step is zero, so each fixture arrives at the level the cue
    // set and stays there. Fired for a section that wants to arrive
    // rather than cut.
    put(
        "fill and hold",
        "the wash filling light by light over a bar and staying full — a chorus that arrives rather than cuts",
        on(
            role("Wash"),
            vec![
                snap(vec![(Attribute::Dimmer, -0.7)], 1.0),
                snap(vec![(Attribute::Dimmer, 0.0)], 1.0),
            ],
            once(1.0, 360.0, Play::Build),
            Vec::new(),
        ),
    );

    // The same, emptying: fixtures go out one by one over a bar and stay
    // out. Built as a `Build` run in reverse, and it holds dark.
    put(
        "drain and hold",
        "the wash going out light by light over a bar and staying dark — the end of a section, a breakdown arriving",
        on(
            role("Wash"),
            vec![
                snap(vec![(Attribute::Dimmer, 0.0)], 1.0),
                snap(vec![(Attribute::Dimmer, -0.9)], 1.0),
            ],
            once(1.0, 360.0, Play::Build),
            vec![Trick::Reverse],
        ),
    );

    // A single sine swell travelling once across the back wall and the
    // bars, and gone. One wave, not a wave effect — the thing to fire on
    // a cymbal swell.
    put(
        "cyc wave",
        "one swell of light rolling across the back wall and bars and gone — cymbal swells and section changes",
        on(
            Selection::Union(vec![role("Back"), role("Bars")]),
            // Hand-drawn rather than `Waveform::Sine`, because a one-shot
            // must *end* at zero and the waveform sugar is arranged for
            // looping: its last step is the top of the swing.
            (1..12)
                .map(|i| {
                    let t = std::f32::consts::PI * i as f32 / 12.0;
                    ramp(vec![(Attribute::Dimmer, 0.5 * t.sin())], 1.0)
                })
                .chain(std::iter::once(ramp(vec![(Attribute::Dimmer, 0.0)], 1.0)))
                .collect(),
            once(1.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // A colour replacing the base across the rig, once, and holding.
    // `Build` is what makes fixtures arrive and *stay*; `once` is what
    // stops the wrap resetting them. The first step is an empty delta —
    // "not yet arrived, leave the look alone" — so the wipe front is the
    // new colour meeting the old, not a dark gap.
    // r[impl effects.once]
    put(
        "colour wipe",
        "the hot colour sweeping across the wash over a bar and staying — a section change that wants to be seen arriving",
        on(
            role("Wash"),
            vec![delta(Attribute::Dimmer, 0.0), hue("Hot")],
            once(1.0, 360.0, Play::Build),
            Vec::new(),
        ),
    );

    // ── strip ────────────────────────────────────────────────────────

    // The strip filling from one end and staying full.
    put(
        "strip fill",
        "the bars filling from one end over a bar and staying full — the top of a section or a riser",
        on(
            role("Bars"),
            vec![
                delta(Attribute::Dimmer, -1.0),
                delta(Attribute::Dimmer, 0.0),
            ],
            once(1.0, 360.0, Play::Build),
            Vec::new(),
        ),
    );

    // The strip emptying from one end and staying empty.
    put(
        "strip drain",
        "the bars emptying from one end over a bar and staying dark — the end of a section",
        on(
            role("Bars"),
            vec![
                delta(Attribute::Dimmer, 0.0),
                delta(Attribute::Dimmer, -1.0),
            ],
            once(1.0, 360.0, Play::Build),
            vec![Trick::Reverse],
        ),
    );

    // Pixels dropping out one by one in a shuffled order until the strip
    // is dark, then staying dark.
    put(
        "dissolve",
        "the bars' pixels dropping out one by one in a scattered order over two bars and staying dark — outros",
        on(
            role("Bars"),
            vec![
                delta(Attribute::Dimmer, 0.0),
                delta(Attribute::Dimmer, -1.0),
            ],
            once(2.0, 360.0, Play::Build),
            vec![Trick::Shuffle(1327)],
        ),
    );

    // One comet from the middle of the strip out to both ends, once. A
    // *lift* with a decaying tail rather than a dark tail: the head is
    // brighter than the cue and the tail settles back to it, so the
    // strip is where the cue left it before and after — an earlier
    // version held the strip dark after the burst, which is exactly the
    // ratchet `r[effects.delta-ends-at-nothing]` forbids.
    put(
        "centre burst",
        "a burst of light leaving the centre of the bars for both ends and gone — hits and downbeats",
        on(
            role("Bars"),
            vec![
                snap(vec![(Attribute::Dimmer, 0.6)], 1.0),
                snap(vec![(Attribute::Dimmer, 0.35)], 1.0),
                snap(vec![(Attribute::Dimmer, 0.15)], 1.0),
                snap(vec![(Attribute::Dimmer, 0.0)], 5.0),
            ],
            once(0.5, 360.0, Play::Forward),
            vec![Trick::Wings(2)],
        ),
    );

    // Colour A sweeps along the strip, colour B follows it, and the
    // strip goes dark behind that. Each step sets the hue and the level
    // together so it is one gesture.
    put(
        "tri wipe",
        "the hot colour sweeping along the bars with the deep colour behind it and dark behind that — a section change",
        on(
            role("Bars"),
            vec![
                Step {
                    apply: vec![
                        RecipeApply::Color(crate::preset::Ref::Named("Hot".into())),
                        RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)]),
                    ],
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
                Step {
                    apply: vec![
                        RecipeApply::Color(crate::preset::Ref::Named("Deep".into())),
                        RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)]),
                    ],
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
                Step {
                    apply: vec![
                        RecipeApply::Color(crate::preset::Ref::Named("Deep".into())),
                        RecipeApply::Delta(vec![(Attribute::Dimmer, -1.0)]),
                    ],
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
            ],
            once(1.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── movers ───────────────────────────────────────────────────────

    // The beams start high and dim, drop onto the focus and arrive at
    // full over a bar, then hold there — which for a delta is the same
    // as not being there. The mover's answer to a bump: a section that
    // *lands* rather than fades in.
    put(
        "fly in",
        "the beams dropping from high and dim onto their focus at full over a bar — a chorus landing",
        on(
            role("Movers"),
            vec![
                snap(
                    vec![(Attribute::Tilt, 30.0), (Attribute::Dimmer, -0.6)],
                    1.0,
                ),
                ease(vec![(Attribute::Tilt, 0.0), (Attribute::Dimmer, 0.0)], 3.0),
            ],
            once(1.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // The movers tilting up and out over two beats and staying gone.
    // Tilt and dimmer ramp together, so the beams lift off the stage as
    // they fade rather than fading in place — one gesture, and it holds
    // at the top until the cue replaces it.
    put(
        "lift off",
        "the beams lifting off the stage and fading out as they rise, staying gone — the end of a section",
        on(
            role("Movers"),
            vec![
                snap(vec![(Attribute::Tilt, 0.0), (Attribute::Dimmer, 0.0)], 1.0),
                ramp(
                    vec![(Attribute::Tilt, 30.0), (Attribute::Dimmer, -1.0)],
                    8.0,
                ),
            ],
            once(0.5, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // One narrow tilt bump running along the rig once: a ripple. The
    // bump is a quarter of the cycle wide so it reads as a thing passing
    // rather than the rig breathing.
    put(
        "tilt ripple",
        "one nod running along the line of beams and gone — fills and pickups",
        on(
            role("Movers"),
            vec![
                ease(vec![(Attribute::Tilt, 14.0)], 1.0),
                ease(vec![(Attribute::Tilt, 0.0)], 3.0),
            ],
            once(2.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family() -> BTreeMap<String, Recipe> {
        let mut out = BTreeMap::new();
        add(&mut |name: &str, fam: &str, _: &str, recipe: Recipe| {
            assert_eq!(fam, FAMILY);
            assert!(
                out.insert(name.to_string(), recipe).is_none(),
                "duplicate {name:?}"
            );
        });
        out
    }

    /// Everything here runs once.
    // r[verify effects.once]
    // r[verify recipes.one-shot]
    #[test]
    fn every_one_shot_runs_once() {
        for (name, recipe) in family() {
            assert!(recipe.timing.once, "{name:?} loops");
        }
    }

    /// The hits end at nothing, and so do the fills — a fill arrives at
    /// the cue's level. Only the drains, wipes, the riser and lift off
    /// hold somewhere else.
    // r[verify effects.delta-ends-at-nothing]
    #[test]
    fn hits_end_at_nothing_and_fills_hold() {
        const HOLDS: [&str; 7] = [
            "drain and hold",
            "lift off",
            "strobe riser",
            "strip drain",
            "dissolve",
            "colour wipe",
            "tri wipe",
        ];
        for (name, recipe) in family() {
            let last = recipe.steps.last().expect("steps");
            let ends_at_zero = last.apply.iter().all(|a| match a {
                RecipeApply::Delta(pairs) => pairs.iter().all(|(_, v)| *v == 0.0),
                _ => false,
            });
            assert_eq!(
                ends_at_zero,
                !HOLDS.contains(&name.as_str()),
                "{name:?} ends in the wrong place"
            );
        }
    }

    /// The blinder hit is `bump`'s object with a longer fall, and the
    /// bump is `bump`'s object exactly.
    // r[verify effects.bump.one-object]
    #[test]
    fn the_hits_are_bumps() {
        let f = family();
        let b = bump::bump(role("Audience"), Kind::White, 1.0);
        let hit = &f["blinder hit"];
        assert_eq!(hit.steps.len(), b.steps.len());
        for (h, s) in hit.steps.iter().zip(&b.steps) {
            assert_eq!(h.apply, s.apply);
            assert_eq!(h.width, s.width);
            assert_eq!(h.transition, s.transition);
        }
        // The only difference in shape: the fall leaves hard and
        // settles soft, so the afterimage is in the tail.
        // r[verify effects.step.accel-decel]
        assert_eq!(hit.steps.last().unwrap().ease, HARD_SOFT);
        assert!(hit.timing.measure > b.timing.measure);
        assert_eq!(f["bump"], bump::bump(role("Wash"), Kind::Level, 0.85));
    }

    /// The riser only ever climbs.
    #[test]
    fn the_strobe_riser_climbs_monotonically() {
        let rates: Vec<f32> = family()["strobe riser"]
            .steps
            .iter()
            .map(|s| match &s.apply[0] {
                RecipeApply::Delta(pairs) => pairs[0].1,
                _ => panic!("absolute"),
            })
            .collect();
        for pair in rates.windows(2) {
            assert!(pair[1] > pair[0], "riser falls: {rates:?}");
        }
        assert!((rates.last().unwrap() - 1.0).abs() < 1e-4);
    }
}
