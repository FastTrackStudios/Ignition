//! Intensity effects — chases, pulses, breathes, builds, sparkles,
//! flickers and strobes.
//!
//! See `super` for the three rules every effect here follows: relative
//! wherever possible, slaved to `Song`, direction from the selection.
//!
//! Nearly everything here modulates `Dimmer` with a `Delta`, so it rides
//! whatever level and colour the cue established. The tables are written
//! as offsets from that level: `0.0` is "leave it where the cue put it",
//! `-0.8` is "nearly out", and a positive number is a lift for a base the
//! cue set below full. The two strobes are the named exceptions — a
//! strobe that merely modulated would still show what was under it.
//!
//! The vocabulary is the one every console shares: Avolites' dimmer
//! shapes (sine, saw, square, pulse, block), Hog's tables (sine, step,
//! sawtooth, ramp, random), Eos's play modes (forward, negative, bounce,
//! build) and the theatre flickers (fire, candle, TV, lightning).
//!
//! Two kinds of random. The shipped flickers are `Trick::Shuffle(seed)`
//! over a phase spread — every unit runs the same table, in an order
//! nobody can predict from the stage. `RecipeApply::Random` is the other
//! kind: every unit rolls its own levels at its own rate, a pure
//! function of (seed, unit, time). `random strobe` uses it; the
//! flickers that want it live in [`add_random`] until the library's
//! own checks learn about the generator.

use super::*;
use crate::recipe::Random;

const FAMILY: &str = "intensity";

/// A snapping step table from a list of dimmer offsets.
fn table(values: &[f32]) -> Vec<Step> {
    values
        .iter()
        .map(|v| delta(Attribute::Dimmer, *v))
        .collect()
}

/// One step with a width and transition spelled out.
fn shaped(v: f32, width: f32, transition: f32) -> Step {
    Step {
        apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, v)])],
        width,
        transition,
        ..Step::new(Vec::new())
    }
}

/// A relative dimmer recipe on a role.
fn dimmer(target: &str, steps: Vec<Step>, timing: Timing, tricks: Vec<Trick>) -> Recipe {
    Recipe {
        // r[impl effects.library.roles-only]
        target: role(target),
        steps,
        timing,
        tricks,
        stack: false,
        ..Default::default()
    }
}

/// A pulse-width chase: `on` slots lit, `off` slots dropped by `depth`.
fn pwm(on: usize, off: usize, depth: f32) -> Vec<Step> {
    let mut v = vec![0.0; on];
    v.extend(std::iter::repeat_n(-depth, off));
    table(&v)
}

/// The plain chase, shared by its song- and tap-mastered spellings.
///
/// A full 360 of spread means each fixture is a full cycle behind its
/// neighbour across the selection, which is what makes one point of
/// light travel rather than the rig breathing together.
pub(super) fn chase() -> Recipe {
    dimmer(
        "Wash",
        table(&[0.0, -0.8]),
        beat(1.0, 360.0, Play::Forward),
        Vec::new(),
    )
}

/// The unison pulse, shared by its song- and tap-mastered spellings.
pub(super) fn pulse() -> Recipe {
    dimmer(
        "Wash",
        Waveform::Sine.steps(Attribute::Dimmer, -0.25, 0.25, true),
        beat(1.0, 0.0, Play::Forward),
        Vec::new(),
    )
}

/// The mirrored chase shared by its outward and inward spellings.
fn mirror_chase(direction: Play) -> Recipe {
    dimmer(
        "Wash",
        table(&[0.0, -0.8]),
        beat(1.0, 360.0, direction),
        vec![Trick::Wings(2)],
    )
}

/// A generator as a step table.
///
/// A `Random` is a function of time and has no steps of its own, but
/// the library's contract is that every effect has more than one step
/// (one step is a look, and a look that meant to move would sit still),
/// so it is carried on two identical ones. Blending between equals is
/// the identity; the clock does the work.
// r[impl effects.random]
fn generator(random: Random) -> Vec<Step> {
    let step = Step::new(vec![RecipeApply::Random(random)]);
    vec![step.clone(), step]
}

/// A relative dimmer generator.
fn flicker(
    low: f32,
    high: f32,
    level_var: f32,
    speed_var: f32,
    attack: f32,
    decay: f32,
    seed: u32,
) -> Random {
    Random {
        attr: Attribute::Dimmer,
        low,
        high,
        level_var,
        speed_var,
        attack,
        decay,
        seed,
        ..Default::default()
    }
}

/// The flickers as generators — the honest model of fire, a candle, a
/// television and a storm: every fixture at its own rate, rolling its
/// own levels, never two agreeing. Same names as the table versions in
/// [`add`], so wiring these in *replaces* those; not wired yet because
/// the library's checks read any non-`Delta` apply as "stopped
/// layering", and these are relative.
// r[impl effects.random]
#[allow(dead_code)] // wired by `effects::catalogue` once its checks accept `FocusDelta`/`Random`
pub(super) fn add_random(add: Add) {
    let mut put = |name: &str, about: &str, recipe: Recipe| add(name, FAMILY, about, recipe);

    // Cold and busy: quick re-rolls either side of the base, snapping.
    put(
        "tv flicker",
        "the key light flickering unevenly like a television on a face, every fixture at its own rate — intros and spoken sections",
        dimmer(
            "Key",
            generator(flicker(-0.4, 0.15, 0.05, 0.5, 0.0, 0.0, 4117)),
            beat(0.5, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Warm and busy: mostly a little up, quick dips, fast ramps.
    put(
        "fire flicker",
        "the wash guttering like firelight, every fixture at its own rate — under an amber look in intros and bridges",
        dimmer(
            "Wash",
            generator(flicker(-0.2, 0.2, 0.1, 0.6, 0.3, 0.2, 5563)),
            beat(0.5, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Slow, small, and never quite still: long attacks, no snap.
    put(
        "candle",
        "the key light wavering slowly and slightly like a candle, never quite still — ballads and the quietest intros",
        dimmer(
            "Key",
            generator(flicker(-0.08, 0.06, 0.02, 0.4, 0.6, 0.4, 6007)),
            beat(2.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Nothing, nothing, then a flash that decays: a level range that
    // is almost all dark, and a decay so a strike falls away rather
    // than switching off.
    put(
        "lightning",
        "scattered white flashes on the back wall with long dark gaps, each strike falling away — intros and breakdowns",
        dimmer(
            "Back",
            generator(flicker(-0.05, 1.0, 0.0, 0.7, 0.0, 0.3, 7331)),
            beat(4.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );
}

/// The whole rig as one selection. A `Union` of roles, so it resolves
/// at any venue and quietly covers less where a room has fewer layers.
// r[impl effects.library.missing-role-is-empty] - a union of roles quietly covers less where a role is unbound
fn whole_rig() -> Selection {
    Selection::Union(vec![role("Key"), role("Wash"), role("Back"), role("Bars")])
}

/// Adds this family to the library.
// r[impl effects.library.categories]
/// The flickers whose generator versions replace their step tables.
const GENERATED: [&str; 4] = ["tv flicker", "fire flicker", "candle", "lightning"];

pub(super) fn add(add: Add) {
    add_filtered(add, true)
}

/// Every table in this family, the four generator-replaced flickers
/// included — the reference shapes, for tests.
#[cfg(test)]
pub(super) fn add_tables(add: Add) {
    add_filtered(add, false)
}

fn add_filtered(add: Add, skip_generated: bool) {
    // Four flickers have generator versions in [`add_random`], which the
    // library wires in their place; the table spellings stay here as
    // the reference shape and are not registered.
    let mut put = |name: &str, about: &str, recipe: Recipe| {
        if skip_generated && GENERATED.contains(&name) {
            return;
        }
        add(name, FAMILY, about, recipe)
    };

    // ── chases ───────────────────────────────────────────────────────

    put(
        "chase",
        "one point of light travelling across the wash once a bar — choruses, any rig",
        chase(),
    );

    // The same shape with the swing inverted: the travelling point is
    // the one that *drops out*. grandMA3's docs single this out as the
    // thing forward chasing cannot do, and it reads completely
    // differently — a hole moving through a lit rig rather than a light
    // moving through a dark one.
    put(
        "dark chase",
        "a dark hole travelling across a lit wash once a bar — choruses on a full stage, motion without more light",
        dimmer(
            "Wash",
            table(&[0.0, -0.8]),
            beat(1.0, 360.0, Play::Negative),
            Vec::new(),
        ),
    );

    put(
        "chase eighths",
        "the chase at twice the pace, a point of light crossing every half bar — last choruses and drops",
        dimmer(
            "Wash",
            table(&[0.0, -0.75]),
            beat(0.5, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Neighbours move together, so the travelling point is twice as wide
    // and half as busy. Avolites calls this a block chase.
    put(
        "pair chase",
        "the chase in pairs, a wider point of light crossing the wash once a bar — choruses on a big wash",
        dimmer(
            "Wash",
            table(&[0.0, -0.8]),
            beat(1.0, 360.0, Play::Forward),
            vec![Trick::Block(2)],
        ),
    );

    // Four interleaved groups lighting in turn, so the chase covers the
    // whole width every step. The classic one-in-four.
    put(
        "quarter chase",
        "every fourth light on in turn across the whole width, once a bar — driving choruses, keeps the stage covered",
        dimmer(
            "Wash",
            table(&[0.0, -0.8, -0.8, -0.8]),
            beat(1.0, 360.0, Play::Forward),
            vec![Trick::Group(4)],
        ),
    );

    // A chase from centre to both edges at once: the back half of the
    // selection is mirrored, so one definition spreads outward.
    put(
        "mirror chase out",
        "two points of light leaving the centre for both edges once a bar — choruses, symmetric rigs",
        mirror_chase(Play::Forward),
    );

    put(
        "mirror chase in",
        "two points of light closing from both edges to the centre once a bar — pre-choruses, symmetric rigs",
        mirror_chase(Play::Reverse),
    );

    // Odds and evens trading places — the cheapest effect that reads as
    // deliberate rather than as a chase, and it survives any rig size
    // because `Group(2)` is a proportion.
    put(
        "alternate",
        "odds and evens trading places every half bar — verses that want a pulse without travel",
        dimmer(
            "Wash",
            table(&[0.0, -0.8]),
            beat(1.0, 180.0, Play::Forward),
            vec![Trick::Group(2)],
        ),
    );

    // Odds rise while evens fall, in straight lines — the eased version
    // of `alternate`, a see-saw across the wash.
    put(
        "see saw",
        "odds rising as evens fall in a smooth see-saw once a bar — verses and bridges, gentler than alternate",
        dimmer(
            "Wash",
            Waveform::Triangle.steps(Attribute::Dimmer, -0.3, 0.3, true),
            beat(1.0, 180.0, Play::Forward),
            vec![Trick::Group(2)],
        ),
    );

    // One dot running to the end of the wash and back, never jumping.
    // A narrow window so it stays one dot on any rig size.
    put(
        "snake",
        "a single light running to the end of the wash and back every half bar — breakdowns and intros",
        dimmer(
            "Wash",
            pwm(1, 5, 0.85),
            beat(0.5, 360.0, Play::Bounce),
            Vec::new(),
        ),
    );

    // ── waves ────────────────────────────────────────────────────────

    // Everything together. Zero spread is the whole point: this is the
    // rig breathing, not something travelling across it.
    put(
        "pulse",
        "the whole wash swelling and settling together once a bar — choruses, the rig breathing with the beat",
        pulse(),
    );

    put(
        "breathe",
        "the whole wash swelling slowly over four bars — verses and bridges, movement without being busy",
        dimmer(
            "Wash",
            Waveform::Sine.steps(Attribute::Dimmer, -0.2, 0.2, true),
            beat(4.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // A breathe that travels: each fixture a little behind its
    // neighbour, so the rise rolls across the stage rather than arriving
    // everywhere at once.
    put(
        "offset breathe",
        "a slow swell rolling across the wash over two bars — verses, the stage breathing in sequence",
        dimmer(
            "Wash",
            Waveform::Sine.steps(Attribute::Dimmer, -0.2, 0.2, true),
            beat(2.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // A smooth travelling swell rather than a hard chase — the same
    // spread, a sine instead of two steps, and it reads as a wave
    // passing through the rig rather than a light moving along it.
    put(
        "wave",
        "a soft wave of light passing along the wash every two bars — pre-choruses and outros",
        dimmer(
            "Wash",
            Waveform::Sine.steps(Attribute::Dimmer, -0.3, 0.3, true),
            beat(2.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Out and back rather than snapping at the wrap, so the wave washes.
    put(
        "bounce",
        "a wave of light washing to the end of the rig and back every two bars — bridges, never jumps",
        dimmer(
            "Wash",
            Waveform::Triangle.steps(Attribute::Dimmer, -0.35, 0.35, true),
            beat(2.0, 360.0, Play::Bounce),
            Vec::new(),
        ),
    );

    // A sawtooth laid along the wash: each fixture rises and snaps back,
    // a step behind its neighbour, so a ramp appears to slide across.
    put(
        "saw wave",
        "a ramp of light sliding along the wash and snapping back every two bars — post-choruses, harder than wave",
        dimmer(
            "Wash",
            Waveform::RampUp.steps(Attribute::Dimmer, -0.3, 0.3, true),
            beat(2.0, 360.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── fills ────────────────────────────────────────────────────────

    // Fixtures arrive and stay until the cycle wraps, so the rig fills
    // up and resets. Not a phase shift of a shared wave — a threshold
    // against each fixture's own place in the selection, which is why
    // `Play::Build` is a mode rather than a step table.
    put(
        "build",
        "the wash filling one light at a time over two bars, then resetting — pre-choruses and risers",
        dimmer(
            "Wash",
            table(&[-0.7, 0.0]),
            beat(2.0, 360.0, Play::Build),
            Vec::new(),
        ),
    );

    // The rig emptying rather than filling: each fixture drops out in
    // turn and stays out until the wrap brings everything back.
    put(
        "drain",
        "the wash emptying one light at a time over two bars, then resetting — ends of sections, outros",
        dimmer(
            "Wash",
            table(&[0.0, -0.85]),
            beat(2.0, 360.0, Play::Build),
            Vec::new(),
        ),
    );

    // Opening outward from centre. `Wings(2)` mirrors the back half, so
    // one definition runs symmetrically — the thing that would otherwise
    // need two hand-authored builds pointed at each other.
    put(
        "open out",
        "the wash filling from the centre to both edges over two bars — a chorus arriving, symmetric rigs",
        dimmer(
            "Wash",
            table(&[-0.7, 0.0]),
            beat(2.0, 360.0, Play::Build),
            vec![Trick::Wings(2)],
        ),
    );

    // ── pulses on the beat ───────────────────────────────────────────

    // Snap up on the beat and fade out over the rest of it — the shape
    // of a kick drum. No spread, so the whole wash hits together.
    put(
        "ramp pulse",
        "the wash snapping up on every beat and fading out before the next — drops and last choruses, a kick drum in light",
        dimmer(
            "Wash",
            Waveform::RampDown.steps(Attribute::Dimmer, -0.35, 0.35, true),
            beat(0.25, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // A short bright tick on each beat: lit for a fifth of the beat,
    // dropped for the rest.
    put(
        "beat pulse",
        "a short dip on every beat with the wash mostly up — choruses that want a click rather than a swell",
        dimmer(
            "Wash",
            pwm(1, 4, 0.7),
            beat(0.25, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Two quick pulses then a rest — a heartbeat, the one rhythmic
    // intensity figure that does not read as a chase.
    put(
        "heartbeat",
        "two quick lifts and a rest every two bars — intros, breakdowns and anything tense",
        dimmer(
            "Wash",
            vec![
                shaped(0.35, 1.0, 0.0),
                shaped(0.0, 1.0, 1.0),
                shaped(0.35, 1.0, 0.0),
                shaped(0.0, 5.0, 1.0),
            ],
            beat(2.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── sparkle ──────────────────────────────────────────────────────

    // A shuffled chase reads as twinkle rather than as travel: the same
    // one point of light, visiting the rig in an order nobody can
    // predict — but a *reproducible* one, so the look can be recalled.
    put(
        "sparkle",
        "single lights twinkling across the wash in a scattered order — pre-choruses, outros and ballad choruses",
        dimmer(
            "Wash",
            table(&[0.0, -0.85]),
            beat(1.0, 360.0, Play::Forward),
            vec![Trick::Shuffle(1741)],
        ),
    );

    // Everything on, and single fixtures dropping out for a moment in a
    // shuffled order — the negative of `sparkle`: holes twinkling in a
    // lit rig. A narrow window so the drop reads as a blink.
    put(
        "dark sparkle",
        "single lights blinking out of a lit wash in a scattered order — full-stage choruses that want texture",
        dimmer(
            "Wash",
            pwm(1, 5, 0.9),
            beat(1.0, 360.0, Play::Negative),
            vec![Trick::Shuffle(2203)],
        ),
    );

    // A tiny fast wobble, out of step across a shuffled selection, so
    // the wash looks like it is catching light on water.
    put(
        "shimmer",
        "a tiny fast flicker across the wash like light on water — intros and ambient verses, barely there",
        dimmer(
            "Wash",
            Waveform::Sine.steps(Attribute::Dimmer, 0.0, 0.08, true),
            beat(0.125, 360.0, Play::Forward),
            vec![Trick::Shuffle(3301)],
        ),
    );

    // ── flickers ─────────────────────────────────────────────────────

    // The blue flicker of a television on a face: an irregular table
    // either side of the base, shuffled so no two fixtures agree. There
    // is no random rate in the model; the uneven widths stand in for it.
    put(
        "tv flicker",
        "the key light flickering unevenly like a television on a face — intros and spoken sections",
        dimmer(
            "Key",
            vec![
                shaped(0.0, 2.0, 0.0),
                shaped(-0.3, 1.0, 0.0),
                shaped(0.1, 3.0, 0.0),
                shaped(-0.15, 1.0, 0.0),
                shaped(-0.4, 2.0, 0.0),
                shaped(0.05, 1.0, 0.0),
                shaped(-0.2, 2.0, 0.0),
                shaped(0.15, 1.0, 0.0),
            ],
            beat(1.0, 360.0, Play::Forward),
            vec![Trick::Shuffle(4117)],
        ),
    );

    // Firelight: mostly a little above the base with quick dips, on a
    // half-bar table, shuffled — warm and busy where `tv flicker` is
    // cold and busy.
    put(
        "fire flicker",
        "the wash guttering like firelight, busy and warm — under an amber look in intros and bridges",
        dimmer(
            "Wash",
            vec![
                shaped(0.1, 2.0, 1.0),
                shaped(0.2, 1.0, 1.0),
                shaped(-0.1, 1.0, 0.5),
                shaped(0.15, 3.0, 1.0),
                shaped(0.05, 1.0, 1.0),
                shaped(-0.2, 1.0, 0.3),
                shaped(0.2, 2.0, 1.0),
            ],
            beat(0.5, 360.0, Play::Forward),
            vec![Trick::Shuffle(5563)],
        ),
    );

    // A candle: slow, small, and never quite still. The gentlest thing
    // in the family — it only reads once the room is quiet.
    put(
        "candle",
        "the key light wavering slowly and slightly like a candle — ballads and the quietest intros",
        dimmer(
            "Key",
            vec![
                shaped(0.0, 3.0, 1.0),
                shaped(-0.06, 2.0, 1.0),
                shaped(0.04, 3.0, 1.0),
                shaped(-0.03, 1.0, 1.0),
                shaped(0.06, 2.0, 1.0),
            ],
            beat(4.0, 360.0, Play::Forward),
            vec![Trick::Shuffle(6007)],
        ),
    );

    // Lightning: nothing, nothing, nothing, and then one full flash on a
    // fixture nobody was watching. A mostly-zero table over a shuffled
    // 360, so the flashes land in a scattered order; the storm is
    // periodic over four bars and the shuffle is what hides it.
    put(
        "lightning",
        "scattered white flashes on the back wall with long dark gaps, like a storm — intros and breakdowns",
        dimmer(
            "Back",
            vec![
                shaped(1.0, 0.2, 0.0),
                shaped(0.0, 5.0, 0.0),
                shaped(1.0, 0.1, 0.0),
                shaped(0.4, 0.3, 0.0),
                shaped(0.0, 9.0, 0.0),
                shaped(1.0, 0.2, 0.0),
                shaped(0.0, 14.0, 0.0),
            ],
            beat(4.0, 360.0, Play::Forward),
            vec![Trick::Shuffle(7331)],
        ),
    );

    // Camera flashes from the crowd: the audience blinders popping one
    // at a time in a shuffled order, each for a sliver of a beat.
    put(
        "paparazzi",
        "blinders popping one at a time like camera flashes — bows, walk-ons and last choruses",
        dimmer(
            "Audience",
            pwm(1, 7, 0.9),
            beat(0.5, 360.0, Play::Forward),
            vec![Trick::Shuffle(8009)],
        ),
    );

    // ── strobes ──────────────────────────────────────────────────────

    // Hard on/off at eighths. Absolute rather than relative on purpose:
    // a strobe that only modulated would still show whatever was under
    // it, and a strobe is supposed to be the only thing you can see.
    put(
        "strobe",
        "the wash flashing hard on and off at eighths — peaks only, a few bars at a time",
        dimmer(
            "Wash",
            vec![
                Step::new(vec![RecipeApply::Dimmer(1.0)]),
                Step::new(vec![RecipeApply::Dimmer(0.0)]),
            ],
            beat(0.25, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Irregular rather than metronomic. A generator: every fixture
    // rolls full or dark at its own rate, so the flashes never fall
    // into a pattern the ear can find — chaos where a plain strobe is
    // a machine. Absolute, like `strobe`: a strobe that only modulated
    // would still show what was under it. The shuffle scatters which
    // fixture gets which roll.
    // r[impl effects.random]
    put(
        "random strobe",
        "single lights flashing in a scattered order, chaos rather than a machine — drops and last choruses",
        dimmer(
            "Wash",
            generator(Random {
                attr: Attribute::Dimmer,
                low: 0.0,
                high: 1.0,
                level_var: 0.0,
                speed_var: 0.5,
                attack: 0.0,
                decay: 0.0,
                seed: 4409,
                absolute: true,
                ..Default::default()
            }),
            beat(0.25, 0.0, Play::Forward),
            vec![Trick::Shuffle(4409)],
        ),
    );

    // Odds and evens of the blinders trading at eighths. Every emitter
    // is driven so it reads white through any colour — a texture an
    // operator holds under a drop.
    put(
        "blinder chase",
        "the blinders alternating odds and evens at eighths, white — drops, a few bars at most",
        Recipe {
            target: role("Audience"),
            steps: vec![
                Step {
                    apply: vec![RecipeApply::Delta(every_emitter(0.9))],
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
                Step {
                    apply: vec![RecipeApply::Delta(every_emitter(0.0))],
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
            ],
            timing: beat(0.25, 180.0, Play::Forward),
            tricks: vec![Trick::Group(2)],
            stack: false,
            ..Default::default()
        },
    );

    // ── layers and the whole rig ─────────────────────────────────────

    // The back wall breathing under everything. Slow and shallow: this
    // is meant to be noticed only when it stops.
    put(
        "back breathe",
        "the back wall swelling gently over four bars under everything else — verses, felt not seen",
        dimmer(
            "Back",
            Waveform::Sine.steps(Attribute::Dimmer, -0.12, 0.12, true),
            beat(4.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Everything, chased as one selection. Ordered by the venue's own
    // spatial ordering when the show wraps it, so this genuinely
    // travels across the room rather than round a fixture list.
    put(
        "rig chase",
        "one point of light travelling across every layer of the rig every two bars — last choruses, the biggest chase there is",
        Recipe {
            target: whole_rig(),
            steps: table(&[0.0, -0.9]),
            timing: beat(2.0, 360.0, Play::Forward),
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        },
    );

    // The whole rig filling and resetting — the biggest single gesture
    // available without a strobe.
    put(
        "rig build",
        "every layer of the rig filling light by light over four bars, then resetting — the riser into a last chorus",
        Recipe {
            target: whole_rig(),
            steps: table(&[-0.9, 0.0]),
            timing: beat(4.0, 360.0, Play::Build),
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        },
    );

    // ── busked ───────────────────────────────────────────────────────
    //
    // The two most buskable shapes on the tap master. Not a separate
    // library: an effect that behaved differently depending on where
    // its tempo came from would be two effects sharing a name. These
    // exist so an operator with no track running still has the
    // vocabulary.

    put(
        "tap chase",
        "the chase on tap tempo — busking a set with no track running",
        Recipe {
            timing: tapped(1.0, 360.0, Play::Forward),
            ..chase()
        },
    );

    put(
        "tap pulse",
        "the pulse on tap tempo — busking a set with no track running",
        Recipe {
            timing: tapped(1.0, 0.0, Play::Forward),
            ..pulse()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn family() -> BTreeMap<String, Recipe> {
        let mut out = BTreeMap::new();
        let mut count = 0usize;
        add_tables(&mut |name: &str, fam: &str, _: &str, recipe: Recipe| {
            assert_eq!(fam, FAMILY);
            count += 1;
            out.insert(name.to_string(), recipe);
        });
        assert_eq!(count, out.len(), "a name in this family is used twice");
        out
    }

    fn random_family() -> BTreeMap<String, Recipe> {
        let mut out = BTreeMap::new();
        add_random(&mut |name: &str, fam: &str, about: &str, recipe: Recipe| {
            assert_eq!(fam, FAMILY);
            assert!(about.contains(" — "), "{name:?} needs a note");
            out.insert(name.to_string(), recipe);
        });
        out
    }

    /// The generator flickers replace table flickers of the same name,
    /// are relative, and give every fixture its own flicker.
    // r[verify effects.random]
    #[test]
    fn the_generator_flickers_are_relative_and_differ_per_fixture() {
        use crate::recipe::{Show, expand_recipe};
        let tables = family();
        for (name, r) in random_family() {
            assert!(tables.contains_key(&name), "{name:?} is not a replacement");
            assert!(r.steps.iter().all(|s| matches!(
                s.apply[0],
                RecipeApply::Random(Random {
                    absolute: false,
                    ..
                })
            )));
            let recipe = Recipe {
                target: Selection::Chans(vec![1, 2, 3, 4]),
                timing: Timing {
                    speed: Speed::Hz(1.0),
                    ..r.timing.clone()
                },
                ..r.clone()
            };
            let levels: Vec<f32> = (0..40)
                .map(|i| {
                    expand_recipe(
                        &recipe,
                        &Show::new(&[], &crate::selection::EMPTY_RIG),
                        i as f32 * 0.31,
                    )
                    .into_iter()
                    .map(|e| e.value.value)
                    .sum::<f32>()
                })
                .collect();
            assert!(
                levels.windows(2).any(|w| w[0] != w[1]),
                "{name:?} never moves"
            );
        }
        // The random strobe is the generator, absolute, full or dark.
        let strobe = &tables["random strobe"];
        match &strobe.steps[0].apply[0] {
            RecipeApply::Random(r) => {
                assert!(r.absolute);
                assert_eq!((r.low, r.high), (0.0, 1.0));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn everything_here_is_relative_but_the_strobes() {
        for (name, r) in family() {
            let relative = r.steps.iter().all(|s| {
                s.apply.iter().all(|a| {
                    matches!(
                        a,
                        RecipeApply::Delta(_)
                            | RecipeApply::Random(Random {
                                absolute: false,
                                ..
                            })
                    )
                })
            });
            assert_eq!(
                relative,
                !name.contains("strobe"),
                "{name:?} has the wrong layering"
            );
        }
    }

    #[test]
    fn the_flickers_are_shuffled() {
        let f = family();
        for name in [
            "tv flicker",
            "fire flicker",
            "candle",
            "lightning",
            "paparazzi",
            "sparkle",
            "dark sparkle",
            "random strobe",
        ] {
            assert!(
                f[name]
                    .tricks
                    .iter()
                    .any(|t| matches!(t, Trick::Shuffle(_))),
                "{name:?} is not shuffled"
            );
        }
    }

    /// A drain is a build whose held step is dark; a dark chase and a
    /// dark sparkle are negatives. Getting these modes wrong makes the
    /// effect a plain chase wearing the wrong name.
    #[test]
    fn the_fills_build_and_the_dark_ones_are_negative() {
        let f = family();
        for name in ["build", "drain", "open out", "rig build"] {
            assert_eq!(f[name].timing.direction, Play::Build, "{name:?}");
        }
        assert_eq!(f["dark chase"].timing.direction, Play::Negative);
        assert_eq!(f["dark sparkle"].timing.direction, Play::Negative);
        assert_eq!(f["snake"].timing.direction, Play::Bounce);
        assert_eq!(f["bounce"].timing.direction, Play::Bounce);
        assert_eq!(f["mirror chase in"].timing.direction, Play::Reverse);
    }

    /// The strobes are fast enough to be strobes: a cycle per beat, so
    /// an eighth on and an eighth off.
    #[test]
    fn the_strobes_run_at_eighths() {
        let f = family();
        assert_eq!(f["strobe"].timing.measure, 1.0);
        assert_eq!(f["random strobe"].timing.measure, 1.0);
    }
}
