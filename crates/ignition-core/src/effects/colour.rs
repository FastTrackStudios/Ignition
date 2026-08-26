//! Colour effects — rainbows, two- and three-colour chases, splits,
//! stripes, and the relative accents.
//!
//! See `super` for the three rules every effect here follows: relative
//! wherever possible, slaved to `Song`, direction from the selection.
//!
//! Two kinds of thing live here, and the split matters. A **hue table**
//! (`hue("Red")`, `Colors { .. }`) is absolute: it *sets* the colour and
//! replaces whatever the look established, because a colour has nothing
//! to be relative to. A **colour delta** (`ColorAdd`) is relative: it
//! pushes one emitter up or down under whatever colour is there, so it
//! layers — a white pop rides a blue look and a red look alike. The
//! absolute ones are looks that move; the relative ones are effects.
//!
//! The hue tables are written against the profile's palette *roles* —
//! `Deep`, `Hot`, `Cool`, `Warm` — rather than fixed hues where they
//! can be, so the same chase is the song's own two colours in any show.
//! The vocabulary is Avolites' rainbow and colour shapes and MagicQ's
//! 2col and 3col.

use super::*;
use crate::preset::Ref;
use crate::recipe::Distribute;
use ignition_proto::ColorChannel;

const FAMILY: &str = "colour";

/// A step that lays several named colours across the selection.
fn colours(names: &[&str], distribute: Distribute) -> Step {
    Step::new(vec![RecipeApply::Colors {
        colors: names.iter().map(|n| Ref::Named((*n).into())).collect(),
        distribute,
    }])
}

/// A `hue` step that crossfades into itself over its whole width.
fn hue_fade(name: &str) -> Step {
    Step {
        transition: 1.0,
        ..hue(name)
    }
}

/// A `hue` step with a given width, snapping.
fn hue_wide(name: &str, width: f32) -> Step {
    Step { width, ..hue(name) }
}

/// A three-emitter sine table, the phases a third of a cycle apart.
///
/// The additive rainbow. Red, green and blue each ride a sine 120° from
/// the others, so at any instant one is up, one is down and one is on
/// its way — which is what a hue sweep *is*, written as deltas so it
/// rides whatever the look set instead of replacing it.
fn rgb_wheel(amp: f32) -> Vec<Step> {
    const RESOLUTION: usize = 16;
    let tau = std::f32::consts::TAU;
    (0..RESOLUTION)
        .map(|i| {
            let t = tau * i as f32 / RESOLUTION as f32;
            Step {
                apply: vec![RecipeApply::Delta(vec![
                    (add_chan(ColorChannel::Red), amp * t.sin()),
                    (add_chan(ColorChannel::Green), amp * (t + tau / 3.0).sin()),
                    (
                        add_chan(ColorChannel::Blue),
                        amp * (t + 2.0 * tau / 3.0).sin(),
                    ),
                ])],
                width: 1.0,
                transition: 1.0,
                ..Step::new(Vec::new())
            }
        })
        .collect()
}

/// A colour recipe on the wash — the default target for this family.
fn wash(steps: Vec<Step>, timing: Timing, tricks: Vec<Trick>) -> Recipe {
    Recipe {
        target: role("Wash"),
        steps,
        timing,
        tricks,
        stack: false,
        ..Default::default()
    }
}

/// The two-colour chase, shared by its song- and tap-mastered spellings.
fn two_colour_chase() -> Recipe {
    wash(
        vec![hue("Deep"), hue("Hot")],
        beat(1.0, 180.0, Play::Forward),
        vec![Trick::Group(2)],
    )
}

/// Adds this family to the library.
// r[impl effects.library.categories]
// r[impl color.multi.distribution] - stripes, split swap
pub(super) fn add(add: Add) {
    let mut put = |name: &str, about: &str, recipe: Recipe| add(name, FAMILY, about, recipe);

    // ── rainbows ─────────────────────────────────────────────────────

    // The rig walking through the spectrum together. Relative, so over
    // white it is a rainbow and over a saturated look it is that look
    // shifting hue.
    put(
        "colour cycle",
        "the whole wash drifting round the spectrum together over four bars — outros and ambient verses, over white or a pale look",
        wash(rgb_wheel(0.6), beat(4.0, 0.0, Play::Forward), Vec::new()),
    );

    // The same wheel spread the full 360 across the selection, so the
    // whole spectrum is laid along the rig at any one moment and rolls.
    put(
        "rainbow",
        "the spectrum laid along the wash and rolling across it over four bars — outros and party choruses",
        wash(rgb_wheel(0.6), beat(4.0, 360.0, Play::Forward), Vec::new()),
    );

    // Red and blue pushed in opposite directions, which reads as the hue
    // leaning one way then the other around whatever colour the look
    // established — the colour version of a sway.
    put(
        "hue rock",
        "the look leaning warmer then cooler around its own colour over four bars — verses, felt not seen",
        wash(
            (0..12)
                .map(|i| {
                    let t = std::f32::consts::TAU * i as f32 / 12.0;
                    let lean = 0.5 * t.sin();
                    Step {
                        apply: vec![RecipeApply::Delta(vec![
                            (add_chan(ColorChannel::Red), lean),
                            (add_chan(ColorChannel::Blue), -lean),
                        ])],
                        width: 1.0,
                        transition: 1.0,
                        ..Step::new(Vec::new())
                    }
                })
                .collect(),
            beat(4.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── two colours ──────────────────────────────────────────────────

    // Two hues snapping on odds and evens — the club-standard A/B
    // alternation, on the beat, in the song's own two colours.
    put(
        "two colour chase",
        "the palette's two colours trading places on odds and evens once a bar — breakdowns and post-choruses",
        two_colour_chase(),
    );

    put(
        "tap two colour",
        "the two colour chase on tap tempo — busking a set with no track running",
        Recipe {
            timing: tapped(1.0, 180.0, Play::Forward),
            ..two_colour_chase()
        },
    );

    // A and B crossfading, everything together. Each step transitions
    // its whole width, so the rig never *is* either colour for long —
    // it is always on its way, which reads as breathing between two
    // hues rather than switching.
    put(
        "two colour fade",
        "the whole wash crossfading between the palette's two colours every two bars — verses and bridges",
        wash(
            vec![hue_fade("Deep"), hue_fade("Hot")],
            beat(2.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Warm against cool on odds and evens — the same trick with the
    // temperature rather than the hue, and it reads far calmer.
    put(
        "warm cool split",
        "warm and cool white trading on odds and evens every four bars — verses that want colour without colour",
        wash(
            vec![hue("Warm"), hue("Cool")],
            beat(4.0, 180.0, Play::Forward),
            vec![Trick::Group(2)],
        ),
    );

    // The rig cut in two halves, each a colour, swapping every two bars.
    // `Distribute::Block` is what makes it halves rather than odds and
    // evens — stage left is one colour, stage right the other, and then
    // they trade.
    put(
        "split swap",
        "one half of the stage in each palette colour, the halves swapping every two bars — post-choruses and bridges",
        wash(
            vec![
                colours(&["Deep", "Hot"], Distribute::Block),
                colours(&["Hot", "Deep"], Distribute::Block),
            ],
            beat(2.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Alternating pairs — A A B B along the rig — trading places. The
    // `Block(2)` trick makes each pair one unit and the cycle lays the
    // two colours across those units; the second step flips the order
    // so the stripes march.
    put(
        "colour stripes",
        "the wash striped in pairs of each palette colour, the stripes swapping every two bars — choruses",
        wash(
            vec![
                colours(&["Deep", "Hot"], Distribute::Cycle),
                colours(&["Hot", "Deep"], Distribute::Cycle),
            ],
            beat(2.0, 0.0, Play::Forward),
            vec![Trick::Block(2)],
        ),
    );

    // ── multi-colour chases ──────────────────────────────────────────

    // Three hues walking the rig — each fixture a step behind its
    // neighbour, so all three are on the rig at once and travel.
    put(
        "three colour chase",
        "three palette colours laid along the wash and walking across it every two bars — choruses",
        wash(
            vec![hue("Deep"), hue("Hot"), hue("Cool")],
            beat(2.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // The warm family, a bar a step, travelling: gold to amber to red
    // to pink. A sunset walking along the truss.
    put(
        "warm chase",
        "gold, amber, red and pink walking along the wash like a sunset over four bars — outros and warm bridges",
        wash(
            vec![hue("Gold"), hue("Amber"), hue("Red"), hue("Pink")],
            beat(4.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // The cold family: sky to blue to lavender to cyan. The same walk
    // with the temperature flipped, and it reads as ice where the other
    // reads as fire.
    put(
        "cold chase",
        "sky, blue, lavender and cyan walking along the wash like ice over four bars — intros and cool bridges",
        wash(
            vec![hue("Sky"), hue("Blue"), hue("Lavender"), hue("Cyan")],
            beat(4.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Four hues visiting the rig in a shuffled order, snapping. The
    // colour equivalent of a mover ballyhoo — every fixture somewhere it
    // was not just at, and nowhere twice in a row.
    put(
        "colour ballyhoo",
        "every light snapping through the palette in a scattered order every two bars — party choruses",
        wash(
            vec![hue("Deep"), hue("Hot"), hue("Cool"), hue("Warm")],
            beat(2.0, 360.0, Play::Forward),
            vec![Trick::Shuffle(2203)],
        ),
    );

    // ── accents ──────────────────────────────────────────────────────

    // Single fixtures flicking to the hot colour and back, in a shuffled
    // order. The window is narrow — one part in eight — so it reads as
    // glints on a base rather than a chase.
    put(
        "colour sparkle",
        "single lights glinting to the hot colour on a deep base in a scattered order — pre-choruses and outros",
        wash(
            vec![hue_wide("Hot", 1.0), hue_wide("Deep", 7.0)],
            beat(1.0, 360.0, Play::Forward),
            vec![Trick::Shuffle(3319)],
        ),
    );

    // The white emitter popping for a sliver of the bar over whatever
    // colour is there. Relative, so it is an accent *in* the look rather
    // than a replacement of it — a white pop over a red look is pink for
    // an instant, which is the whole appeal.
    put(
        "white pop",
        "a brief white flash through whatever colour the wash is in, once a bar — choruses, the cheapest accent there is",
        wash(
            vec![
                Step {
                    apply: vec![RecipeApply::Delta(vec![(
                        add_chan(ColorChannel::White),
                        0.8,
                    )])],
                    width: 1.0,
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
                Step {
                    apply: vec![RecipeApply::Delta(vec![(
                        add_chan(ColorChannel::White),
                        0.0,
                    )])],
                    width: 15.0,
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
            ],
            beat(1.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Fire. The green emitter dips under a red or amber base, each
    // fixture on its own shuffled phase, so the colour guttering between
    // red and orange is different everywhere. Relative: it needs a warm
    // look under it, and does nothing worth seeing over a blue one.
    put(
        "colour fire",
        "an amber look guttering between red and orange, different everywhere — under a warm look in intros and bridges",
        wash(
            (0..8)
                .map(|i| {
                    // A deliberately uneven dip so the flicker does not
                    // read as a sine.
                    let dip = [0.0, -0.25, -0.1, -0.35, -0.05, -0.3, -0.15, -0.2][i];
                    Step {
                        apply: vec![RecipeApply::Delta(vec![(
                            add_chan(ColorChannel::Green),
                            dip,
                        )])],
                        width: 1.0,
                        transition: 1.0,
                        ..Step::new(Vec::new())
                    }
                })
                .collect(),
            beat(1.0, 360.0, Play::Forward),
            vec![Trick::Shuffle(5501)],
        ),
    );

    // Saturation breathing: the white emitter rising and falling under
    // the look, so a colour washes toward pastel and back over a few
    // bars. Felt rather than seen — the thing to leave under a verse.
    put(
        "saturation breathe",
        "the look washing toward pastel and back to full colour over four bars — verses, felt not seen",
        wash(
            Waveform::Sine.steps(add_chan(ColorChannel::White), 0.2, 0.2, true),
            beat(4.0, 0.0, Play::Forward),
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

    /// Every colour effect touches colour — a hue table or an emitter
    /// delta — and nothing here runs once.
    #[test]
    fn every_colour_effect_moves_colour_and_loops() {
        for (name, recipe) in family() {
            assert!(recipe.steps.len() >= 2, "{name:?} cannot move");
            assert!(!recipe.timing.once, "{name:?} runs once");
            let colours = recipe.steps.iter().any(|s| {
                s.apply.iter().any(|a| match a {
                    RecipeApply::Color(_) | RecipeApply::Colors { .. } => true,
                    RecipeApply::Delta(pairs) => pairs
                        .iter()
                        .any(|(at, _)| matches!(at, Attribute::ColorAdd { .. })),
                    _ => false,
                })
            });
            assert!(colours, "{name:?} never touches colour");
        }
    }

    /// The rainbow is spread and the cycle is not — that is the only
    /// difference between them, and it is the whole difference.
    // r[verify effects.phase.spread]
    #[test]
    fn the_rainbow_travels_and_the_cycle_does_not() {
        let f = family();
        assert_eq!(f["rainbow"].steps, f["colour cycle"].steps);
        assert_eq!(f["rainbow"].timing.phase_spread_deg, 360.0);
        assert_eq!(f["colour cycle"].timing.phase_spread_deg, 0.0);
        assert!(
            f["rainbow"]
                .steps
                .iter()
                .all(|s| s.apply.iter().all(|a| matches!(a, RecipeApply::Delta(_))))
        );
    }

    /// The accents that ride a look are relative; the chases that set
    /// one are absolute. Both on purpose.
    #[test]
    fn the_accents_layer() {
        let f = family();
        for name in ["white pop", "colour fire", "saturation breathe", "hue rock"] {
            assert!(
                f[name]
                    .steps
                    .iter()
                    .all(|s| s.apply.iter().all(|a| matches!(a, RecipeApply::Delta(_)))),
                "{name:?} stopped layering"
            );
        }
    }
}
