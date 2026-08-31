//! Pixel-strip effects — comets, scanners, meteors, twinkles, and the
//! accent layers on the bars.
//!
//! See `super` for the three rules every effect here follows: relative
//! wherever possible, slaved to `Song`, direction from the selection.
//!
//! All of these are written against `Bars`, the one role whose fixtures
//! sit in a line, because a comet on a scattered wash is just a chase
//! with a soft edge. The tables are dimmer offsets from whatever the cue
//! set: `0.0` is the head at the cue's level, and the tail is a run of
//! deeper and deeper drops. A strip the cue left dark shows nothing —
//! these are effects, not looks, and that is the rule.
//!
//! The vocabulary is WLED's: comet, scanner, meteor, twinkle, running
//! sines, rainbow. Per-pixel colour is `RecipeApply::Colors`, which lays
//! a list across the selection but cannot be phase-shifted per unit, so
//! a colour that *scrolls* is a spread hue table — see `rainbow scroll`.
//!
//! The one-shots on the strip — fill, drain, dissolve, centre burst, the
//! tri wipe — live with the other one-shots, because that is how an
//! operator reaches for them.

use super::*;

const FAMILY: &str = "strip";

/// A snapping step table from a list of dimmer offsets.
fn table(values: &[f32]) -> Vec<Step> {
    values
        .iter()
        .map(|v| delta(Attribute::Dimmer, *v))
        .collect()
}

/// A relative dimmer recipe on the bars.
fn bars(steps: Vec<Step>, timing: Timing, tricks: Vec<Trick>) -> Recipe {
    Recipe {
        // r[impl effects.library.roles-only]
        target: role("Bars"),
        steps,
        timing,
        tricks,
        stack: false,
        ..Default::default()
    }
}

/// A pulse-width chase: `on` pixels lit, `off` pixels dropped by `depth`.
fn pwm(on: usize, off: usize, depth: f32) -> Vec<Step> {
    let mut v = vec![0.0; on];
    v.extend(std::iter::repeat_n(-depth, off));
    table(&v)
}

/// A head at the cue's level with a tail that decays behind it, then
/// dark for the rest of the cycle. The deltas fall monotonically, which
/// is what makes it a comet rather than a blob.
pub(super) fn comet_table() -> Vec<Step> {
    table(&[0.0, -0.3, -0.55, -0.75, -0.9, -1.0, -1.0, -1.0])
}

/// Adds this family to the library.
// r[impl effects.library.categories]
pub(super) fn add(add: Add) {
    let mut put = |name: &str, about: &str, recipe: Recipe| add(name, FAMILY, about, recipe);

    // One pixel lit, three dark, running along the strip — the sparsest
    // strip chase that still reads as a pattern rather than a dot.
    put(
        "strip chase",
        "every fourth pixel lit and running along the bars once a bar — choruses, the pixel chase",
        bars(pwm(1, 3, 0.9), beat(1.0, 360.0, Play::Forward), Vec::new()),
    );

    // A bright head with a fading tail streaking along the strip once a
    // bar.
    put(
        "comet",
        "a bright head with a fading tail streaking along the bars once a bar — pre-choruses and choruses",
        bars(comet_table(), beat(1.0, 360.0, Play::Forward), Vec::new()),
    );

    // The comet running out to the end and back, without jumping — the
    // Cylon eye.
    put(
        "scanner",
        "a comet running to the end of the bars and back, never jumping — verses and breakdowns",
        bars(comet_table(), beat(1.0, 360.0, Play::Bounce), Vec::new()),
    );

    // Two scanners mirrored from the centre, meeting and parting.
    put(
        "dual scanner",
        "two comets leaving the centre of the bars for both ends and coming back — choruses, symmetric",
        bars(
            comet_table(),
            beat(1.0, 360.0, Play::Bounce),
            vec![Trick::Wings(2)],
        ),
    );

    // Comets in a shuffled order, so heads and tails fall through the
    // strip from nowhere in particular, twice a bar.
    put(
        "meteor",
        "comets falling through the bars from scattered places every half bar — drops and last choruses",
        bars(
            comet_table(),
            beat(0.5, 360.0, Play::Forward),
            vec![Trick::Shuffle(1109)],
        ),
    );

    // Single pixels winking off for a sliver of a beat in a shuffled
    // order: a starfield.
    put(
        "strip twinkle",
        "single pixels winking out of a lit strip in a scattered order like stars — intros and ballads",
        bars(
            pwm(1, 7, 0.9),
            beat(0.25, 360.0, Play::Negative),
            vec![Trick::Shuffle(1511)],
        ),
    );

    // A sine spread twice round the strip, so two bumps of light are
    // travelling along it at any moment.
    put(
        "running sines",
        "two soft bumps of light gliding along the bars every two bars — verses and outros",
        bars(
            Waveform::Sine.steps(Attribute::Dimmer, -0.45, 0.45, true),
            beat(2.0, 720.0, Play::Forward),
            Vec::new(),
        ),
    );

    // The whole strip flashing at eighths — relative, so it runs over
    // whatever colour the cue laid across the pixels.
    put(
        "strip strobe",
        "the whole strip flashing at eighths in whatever colour it was — peaks only, a few bars",
        bars(pwm(1, 1, 0.95), beat(0.25, 0.0, Play::Forward), Vec::new()),
    );

    // A spectrum laid along the strip and walking. A true per-pixel
    // scroll of a `Colors` gradient is not expressible — `Colors` lays
    // its list across the selection once and cannot be phase-shifted
    // per unit — so this is a hue table spread 360 across the pixels,
    // which is the same picture in three bands rather than a gradient.
    put(
        "rainbow scroll",
        "red, green and blue bands scrolling along the bars every two bars — party choruses and outros",
        bars(
            vec![hue("Red"), hue("Green"), hue("Blue")],
            beat(2.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── accent layers ────────────────────────────────────────────────

    // The bars ticking, for a verse that wants texture rather than
    // movement. On `Bars` because that is the layer that can carry a
    // pulse without competing with the wash the vocal is lit by.
    put(
        "bar tick",
        "the bars lifting a little on every beat under the wash — verses that want a pulse without motion",
        bars(
            table(&[0.12, 0.0]),
            beat(0.5, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // A shuffled, sparse twinkle on the bars, where a wash would be too
    // broad to read as detail.
    put(
        "bar sparkle",
        "single pixels lifting in a scattered order across the bars — pre-choruses and ballad choruses",
        bars(
            table(&[0.25, 0.0]),
            beat(0.5, 360.0, Play::Forward),
            vec![Trick::Shuffle(907)],
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

    fn dimmer_of(step: &Step) -> f32 {
        step.apply
            .iter()
            .find_map(|a| match a {
                RecipeApply::Delta(pairs) => pairs
                    .iter()
                    .find(|(at, _)| *at == Attribute::Dimmer)
                    .map(|(_, v)| *v),
                _ => None,
            })
            .expect("a dimmer delta")
    }

    #[test]
    fn every_effect_loops_on_the_bars() {
        for (name, r) in family() {
            assert!(r.steps.len() >= 2, "{name:?} has one step and cannot move");
            assert!(!r.timing.once, "{name:?} runs once");
            assert_eq!(r.target, role("Bars"), "{name:?} left the strip");
        }
    }

    #[test]
    fn the_comets_tail_decays() {
        let f = family();
        for name in ["comet", "scanner", "dual scanner", "meteor"] {
            let levels: Vec<f32> = f[name].steps.iter().map(dimmer_of).collect();
            assert!(
                levels.windows(2).all(|w| w[1] <= w[0]),
                "{name:?} tail does not decay: {levels:?}"
            );
            assert!(levels[0] > levels[1], "{name:?} has no head");
        }
    }

    #[test]
    fn the_scanners_bounce_and_the_sines_run_twice_round() {
        let f = family();
        assert_eq!(f["scanner"].timing.direction, Play::Bounce);
        assert_eq!(f["dual scanner"].tricks, vec![Trick::Wings(2)]);
        assert_eq!(f["running sines"].timing.phase_spread_deg, 720.0);
    }
}
