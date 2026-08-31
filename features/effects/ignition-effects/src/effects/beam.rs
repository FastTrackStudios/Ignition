//! Beam effects — zoom, iris, focus, gobo, and the shutter strobes.
//!
//! See `super` for the three rules every effect here follows: relative
//! wherever possible, slaved to `Song`, direction from the selection.
//!
//! Everything here is a `Delta`, including the strobes and the gobo
//! steps. That is a deliberate choice rather than a limitation: a beam
//! effect rides on whatever zoom, iris or focus the look established,
//! and a strobe *delta* of +1 on a look with strobe at zero is a strobe
//! at full — while the same effect over a look already strobing leaves
//! it alone rather than fighting it.
//!
//! `GoboWheel` is slot-indexed in the protocol — the attribute names a
//! slot, and the value is where along the wheel's channel the fixture
//! sits — so the gobo effects nudge the slot-0 channel by fractions of a
//! turn. There is no gobo-rotation, prism or frost attribute yet, so
//! those console standards are not in this file.
//!
//! The vocabulary is MagicQ's 2iris and 2focus and the zoom, iris and
//! focus shapes every desk with beam attributes offers.

use super::*;

const FAMILY: &str = "beam";

/// The beams — the role most of this family is written against.
fn beams(steps: Vec<Step>, timing: Timing, tricks: Vec<Trick>) -> Recipe {
    Recipe {
        target: role("Beams"),
        steps,
        timing,
        tricks,
        stack: false,
        ..Default::default()
    }
}

/// A snapped one-attribute step.
fn snap(attr: Attribute, v: f32) -> Step {
    Step {
        transition: 0.0,
        ..delta(attr, v)
    }
}

/// Adds this family to the library.
// r[impl effects.library.categories]
pub(super) fn add(add: Add) {
    let mut put = |name: &str, about: &str, recipe: Recipe| add(name, FAMILY, about, recipe);

    // ── zoom ─────────────────────────────────────────────────────────

    // Zoom pumping with the music, on the movers because those are the
    // heads with a zoom worth pumping. Relative, so it rides whatever
    // zoom the look established rather than resetting it.
    put(
        "zoom pulse",
        "every beam opening and closing together once a bar — choruses, the beams breathing with the beat",
        Recipe {
            target: role("Movers"),
            steps: Waveform::Sine.steps(Attribute::Zoom, 0.0, 0.25, true),
            timing: beat(1.0, 0.0, Play::Forward),
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        },
    );

    // The beams opening and closing in turn along the rig: one zoom
    // sine, spread a full turn, so at any moment the widths are laid out
    // as a wave that travels.
    put(
        "zoom wave",
        "beams opening and closing in sequence so a wave of width rolls along the rig every two bars — pre-choruses",
        beams(
            Waveform::Sine.steps(Attribute::Zoom, 0.0, 0.25, true),
            beat(2.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Snaps wide and closes down slowly, odds against evens. The zoom
    // equivalent of a bump chase — it reads as a punch on the beat
    // where `zoom pulse` reads as breathing.
    put(
        "zoom snap",
        "beams snapping wide and closing slowly, odds against evens, once a bar — drops and heavy choruses",
        beams(
            Waveform::RampDown.steps(Attribute::Zoom, 0.15, 0.15, true),
            beat(1.0, 180.0, Play::Forward),
            vec![Trick::Group(2)],
        ),
    );

    // ── iris ─────────────────────────────────────────────────────────

    // The iris pumping with the beat, together. Closing the iris on a
    // beat is the classic way to make a beam *hit* without touching the
    // dimmer, and it stays a beam effect rather than a bump.
    put(
        "iris pulse",
        "every beam's iris pinching in and out together once a bar — choruses, a hit without touching the dimmer",
        beams(
            Waveform::Sine.steps(Attribute::Iris, 0.0, 0.3, true),
            beat(1.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // A closing iris travelling along the rig: a sawtooth, spread all
    // the way round, so one beam is snapping open as its neighbour
    // starts to close.
    put(
        "iris chase",
        "a pinched iris travelling along the rig, each beam snapping open as the next closes, once a bar — choruses",
        beams(
            Waveform::RampDown.steps(Attribute::Iris, -0.25, 0.25, true),
            beat(1.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── focus ────────────────────────────────────────────────────────

    // A gobo drifting in and out of sharpness. Nothing else in the
    // library does this, and it is the one beam effect that reads as
    // *atmosphere* rather than as motion.
    put(
        "focus breathe",
        "gobos drifting in and out of sharpness together over four bars — intros and bridges, texture rather than motion",
        beams(
            Waveform::Sine.steps(Attribute::Focus, 0.0, 0.2, true),
            beat(4.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // The same drift spread along the rig, so a soft patch travels
    // through a line of sharp beams.
    put(
        "defocus wave",
        "a soft patch travelling through a line of sharp gobos over four bars — bridges and outros",
        beams(
            Waveform::Sine.steps(Attribute::Focus, 0.0, 0.25, true),
            beat(4.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── gobo ─────────────────────────────────────────────────────────

    // A fast, small jitter of the wheel channel around whatever slot the
    // look chose — the wheel rocks at the slot's edge, which is what
    // "gobo shake" does on the consoles that list it. Snapped rather
    // than eased, or a slow wheel will smear it into a wobble.
    put(
        "gobo shake",
        "the gobo wheel jittering at the edge of its slot so the pattern shivers — drops and breakdowns",
        beams(
            vec![
                snap(Attribute::GoboWheel { slot: 0 }, 0.02),
                snap(Attribute::GoboWheel { slot: 0 }, -0.02),
                snap(Attribute::GoboWheel { slot: 0 }, 0.015),
                snap(Attribute::GoboWheel { slot: 0 }, -0.01),
            ],
            beat(0.25, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Odds on the look's gobo, evens a quarter-turn round the wheel, and
    // they trade every two bars. Relative, so it alternates between
    // whichever two slots sit a quarter turn apart from the look's own.
    put(
        "gobo step",
        "odd and even beams trading between two gobos every two bars — bridges and intros that want texture",
        beams(
            vec![
                snap(Attribute::GoboWheel { slot: 0 }, 0.0),
                snap(Attribute::GoboWheel { slot: 0 }, 0.25),
            ],
            beat(2.0, 180.0, Play::Forward),
            vec![Trick::Group(2)],
        ),
    );

    // ── shutter ──────────────────────────────────────────────────────

    // The shutter strobe passing along the rig: one beam strobing at a
    // time, on for a quarter of the cycle, then the next. A chase where
    // the travelling thing is a flicker rather than a level.
    put(
        "strobe chase",
        "a strobing beam travelling along the rig once a bar, the rest steady — last choruses",
        beams(
            vec![
                snap(Attribute::Strobe, 0.8),
                snap(Attribute::Strobe, 0.0),
                snap(Attribute::Strobe, 0.0),
                snap(Attribute::Strobe, 0.0),
            ],
            beat(1.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Beams bursting into strobe in an order nobody can predict — but a
    // reproducible one, so the look can be recalled. Sparse: one in six
    // steps, so a pop is a pop and not a bed.
    put(
        "random strobe pops",
        "single beams bursting into strobe for a moment in a scattered order — drops, sparse enough to stay a surprise",
        beams(
            vec![
                snap(Attribute::Strobe, 0.9),
                snap(Attribute::Strobe, 0.0),
                snap(Attribute::Strobe, 0.0),
                snap(Attribute::Strobe, 0.0),
                snap(Attribute::Strobe, 0.0),
                snap(Attribute::Strobe, 0.0),
            ],
            beat(0.5, 360.0, Play::Forward),
            vec![Trick::Shuffle(6131)],
        ),
    );

    // A slow strobe that swells in rate and falls away every two bars,
    // together — a strobe as a *bed* rather than a hit, for a bridge
    // that wants tension without a flash.
    put(
        "strobe bed",
        "a slow shutter strobe swelling in and fading out of the look every two bars — bridges that want tension without a flash",
        beams(
            Waveform::Sine.steps(Attribute::Strobe, 0.2, 0.2, true),
            beat(2.0, 0.0, Play::Forward),
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

    /// Everything here layers: no absolute applies, even on the strobes,
    /// and everything touches a beam attribute rather than the dimmer.
    #[test]
    fn every_beam_effect_is_relative_and_on_a_beam_attribute() {
        for (name, recipe) in family() {
            assert!(recipe.steps.len() > 1, "{name:?} cannot move");
            for step in &recipe.steps {
                for apply in &step.apply {
                    let RecipeApply::Delta(pairs) = apply else {
                        panic!("{name:?} is absolute");
                    };
                    for (at, _) in pairs {
                        assert!(
                            matches!(
                                at,
                                Attribute::Zoom
                                    | Attribute::Iris
                                    | Attribute::Focus
                                    | Attribute::Strobe
                                    | Attribute::GoboWheel { .. }
                            ),
                            "{name:?} touches {at:?}, which is not a beam attribute"
                        );
                    }
                }
            }
        }
    }

    /// The strobe bed never drops below the look's own shutter: it is a
    /// bed, and a bed that went negative would be fighting the cue.
    #[test]
    fn the_strobe_bed_only_adds() {
        for step in &family()["strobe bed"].steps {
            if let RecipeApply::Delta(pairs) = &step.apply[0] {
                assert!(pairs[0].1 >= 0.0);
            }
        }
    }
}
