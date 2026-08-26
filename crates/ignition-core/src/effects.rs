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

/// The same, but slaved to the **tap** master rather than the song.
///
/// Busking, as opposed to playback. The song master is right when a
/// track is running and useless when one is not — a support act, a
/// change-over, a worship set nobody sequenced — and an operator tapping
/// four times is the oldest tempo source there is.
///
/// Deliberately the same recipes with a different rate source rather
/// than a second library. An effect that behaved differently depending
/// on where its tempo came from would be two effects wearing one name.
fn tapped(bars: f32, spread: f32, direction: Play) -> Timing {
    Timing {
        speed: Speed::Master("Tap".into()),
        ..beat(bars, spread, direction)
    }
}

/// A step setting one relative attribute.
fn delta(attr: Attribute, v: f32) -> Step {
    Step::new(vec![RecipeApply::Delta(vec![(attr, v)])])
}


/// A parametric position table — the generator most mover effects are.
///
/// Both axes in **one** recipe. The first version of this library shipped
/// `circle pan` and `circle tilt` as a pair, which is how a console with
/// only per-attribute effects has to do it — and it is a bad deal here:
/// a programmer picks two things that are meaningless apart, and losing
/// one silently turns a circle into a diagonal sweep that looks
/// deliberate enough that nobody notices. A step table can hold Pan and
/// Tilt together, so the shape is one object.
///
/// `pan_cycles` and `tilt_cycles` are how many times each axis goes
/// round per cycle of the effect, which is the whole vocabulary:
///
/// - 1 and 1, a quarter out of phase → a **circle**
/// - 1 and 2 → a **figure of eight**
/// - 3 and 2 → a **ballyhoo**, the classic excitement move, because the
///   ratio does not resolve quickly and the beam never quite repeats
/// - 1 and 0 → a flat **sweep**
///
/// `resolution` is how many steps the curve is drawn with. Sixteen is
/// smooth enough that the interpolation between steps does the rest, and
/// small enough that the table stays readable in the profile.
fn orbit(
    pan_amp: f32,
    tilt_amp: f32,
    pan_cycles: f32,
    tilt_cycles: f32,
    tilt_phase_deg: f32,
) -> Vec<Step> {
    const RESOLUTION: usize = 16;
    let tau = std::f32::consts::TAU;
    (0..RESOLUTION)
        .map(|i| {
            let t = i as f32 / RESOLUTION as f32;
            let pan = pan_amp * (tau * pan_cycles * t).sin();
            let tilt =
                tilt_amp * (tau * tilt_cycles * t + tilt_phase_deg.to_radians()).sin();
            Step {
                apply: vec![RecipeApply::Delta(vec![
                    (Attribute::Pan, pan),
                    (Attribute::Tilt, tilt),
                ])],
                width: 1.0,
                // Fully transitioning, because a position effect is a
                // *path*: snapping between sixteen points would be a
                // stutter rather than a curve.
                transition: 1.0,
                ..Step::new(Vec::new())
            }
        })
        .collect()
}

/// A mover recipe from an orbit.
fn mover(steps: Vec<Step>, bars: f32, spread: f32) -> Recipe {
    Recipe {
        target: role("Movers"),
        steps,
        timing: beat(bars, spread, Play::Forward),
        tricks: Vec::new(),
    }
}

/// A step setting an absolute colour by name.
fn hue(name: &str) -> Step {
    Step::new(vec![RecipeApply::Color(crate::preset::Ref::Named(
        name.into(),
    ))])
}

/// The plain chase, shared by its song- and tap-mastered spellings.
fn library_chase() -> Recipe {
    Recipe {
        target: role("Wash"),
        steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.8)],
        timing: beat(1.0, 360.0, Play::Forward),
        tricks: Vec::new(),
    }
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
    add("chase", library_chase());

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


    // A smooth travelling swell rather than a hard chase — the same
    // spread, a sine instead of two steps, and it reads as a wave
    // passing through the rig rather than a light moving along it.
    add(
        "wave",
        Recipe {
            target: role("Wash"),
            steps: Waveform::Sine.steps(Attribute::Dimmer, -0.3, 0.3, true),
            timing: beat(2.0, 360.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // Eighth-note chase, for a chorus that wants driving rather than
    // moving.
    add(
        "chase eighths",
        Recipe {
            target: role("Wash"),
            steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.75)],
            timing: beat(0.5, 360.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // Two quick pulses then a rest — a heartbeat, which is the one
    // rhythmic intensity figure that does not read as a chase.
    add(
        "heartbeat",
        Recipe {
            target: role("Wash"),
            steps: vec![
                Step { apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.35)])], width: 1.0, transition: 0.0, ..Step::new(Vec::new()) },
                Step { apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])], width: 1.0, transition: 1.0, ..Step::new(Vec::new()) },
                Step { apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.35)])], width: 1.0, transition: 0.0, ..Step::new(Vec::new()) },
                Step { apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])], width: 5.0, transition: 1.0, ..Step::new(Vec::new()) },
            ],
            timing: beat(2.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // The rig emptying rather than filling — `Build` run backwards, for
    // the end of a section.
    add(
        "empty out",
        Recipe {
            target: role("Wash"),
            steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.85)],
            timing: beat(2.0, 360.0, Play::Build),
            tricks: vec![Trick::Reverse],
        },
    );

    // A shuffled, sparse twinkle on the bars, where a wash would be too
    // broad to read as detail.
    add(
        "bar sparkle",
        Recipe {
            target: role("Bars"),
            steps: vec![delta(Attribute::Dimmer, 0.25), delta(Attribute::Dimmer, 0.0)],
            timing: beat(0.5, 360.0, Play::Forward),
            tricks: vec![Trick::Shuffle(907)],
        },
    );

    // Blinders. Absolute and brief, aimed out rather than at the stage —
    // the one effect whose whole job is to stop the audience seeing the
    // band for a moment.
    add(
        "audience blind",
        Recipe {
            target: role("Audience"),
            steps: vec![
                Step { apply: vec![RecipeApply::Dimmer(1.0)], width: 1.0, transition: 0.0, ..Step::new(Vec::new()) },
                Step { apply: vec![RecipeApply::Dimmer(0.0)], width: 3.0, transition: 1.0, ..Step::new(Vec::new()) },
            ],
            timing: Timing { once: true, ..beat(0.5, 0.0, Play::Forward) },
            tricks: Vec::new(),
        },
    );


    // ── busked ───────────────────────────────────────────────────────
    //
    // The same shapes on the tap master. Not a separate library: an
    // effect that behaved differently depending on where its tempo came
    // from would be two effects sharing a name. These exist so an
    // operator with no track running still has the vocabulary.

    add(
        "tap chase",
        Recipe {
            timing: tapped(1.0, 360.0, Play::Forward),
            ..library_chase()
        },
    );
    add(
        "tap pulse",
        Recipe {
            target: role("Wash"),
            steps: Waveform::Sine.steps(Attribute::Dimmer, -0.25, 0.25, true),
            timing: tapped(1.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );
    add(
        "tap circle",
        Recipe {
            timing: tapped(4.0, 0.0, Play::Forward),
            ..mover(orbit(18.0, 18.0, 1.0, 1.0, 90.0), 4.0, 0.0)
        },
    );

    // ── rig ──────────────────────────────────────────────────────────
    //
    // Effects that take the *whole* rig at once rather than one layer.
    // Everything above is deliberately scoped to a role so it can be
    // layered under something else; these are the opposite, and are for
    // the four bars where subtlety is not the goal.
    //
    // `Union` of roles, so they resolve at any venue and quietly cover
    // less where a room has fewer layers.

    let whole_rig = || {
        Selection::Union(vec![
            role("Key"),
            role("Wash"),
            role("Back"),
            role("Bars"),
        ])
    };

    // Everything, chased as one selection. Ordered by the venue's own
    // spatial ordering when the show wraps it, so this genuinely travels
    // across the room rather than round a fixture list.
    add(
        "rig chase",
        Recipe {
            target: whole_rig(),
            steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.9)],
            timing: beat(2.0, 360.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // The whole rig filling and resetting — the biggest single gesture
    // available without a strobe.
    add(
        "rig build",
        Recipe {
            target: whole_rig(),
            steps: vec![delta(Attribute::Dimmer, -0.9), delta(Attribute::Dimmer, 0.0)],
            timing: beat(4.0, 360.0, Play::Build),
            tricks: Vec::new(),
        },
    );

    // Everything at once, hard, once. A whole-rig stab.
    add(
        "rig stab",
        Recipe {
            target: whole_rig(),
            steps: vec![
                Step { apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.8)])], width: 1.0, transition: 0.0, ..Step::new(Vec::new()) },
                Step { apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])], width: 3.0, transition: 1.0, ..Step::new(Vec::new()) },
            ],
            timing: Timing { once: true, ..beat(0.25, 0.0, Play::Forward) },
            tricks: Vec::new(),
        },
    );

    // Odds and evens across the entire rig, which reads much larger than
    // the same trick on one layer.
    add(
        "rig alternate",
        Recipe {
            target: whole_rig(),
            steps: vec![delta(Attribute::Dimmer, 0.0), delta(Attribute::Dimmer, -0.85)],
            timing: beat(1.0, 180.0, Play::Forward),
            tricks: vec![Trick::Group(2)],
        },
    );

    // ── multi-parameter ──────────────────────────────────────────────
    //
    // One effect moving more than one kind of thing. `orbit` already
    // does this for pan and tilt; these go further, and the reason to
    // bother is that the combination is a *look* — a beam that opens as
    // it rises reads as one gesture, where a zoom effect and a tilt
    // effect run separately read as two.

    // Rising and opening together.
    add(
        "fly out",
        Recipe {
            target: role("Movers"),
            steps: (0..12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    let up = (std::f32::consts::TAU * t).sin();
                    Step {
                        apply: vec![RecipeApply::Delta(vec![
                            (Attribute::Tilt, 22.0 * up),
                            (Attribute::Zoom, 0.3 * up),
                        ])],
                        width: 1.0,
                        transition: 1.0,
                        ..Step::new(Vec::new())
                    }
                })
                .collect(),
            timing: beat(4.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // A circle that also breathes in intensity — the beam is dimmest at
    // the far side of the arc, which reads as depth rather than as two
    // effects happening at once.
    add(
        "circle breathe",
        Recipe {
            target: role("Movers"),
            steps: (0..16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    let a = std::f32::consts::TAU * t;
                    Step {
                        apply: vec![RecipeApply::Delta(vec![
                            (Attribute::Pan, 18.0 * a.sin()),
                            (Attribute::Tilt, 18.0 * a.cos()),
                            (Attribute::Dimmer, -0.25 + 0.25 * a.cos()),
                        ])],
                        width: 1.0,
                        transition: 1.0,
                        ..Step::new(Vec::new())
                    }
                })
                .collect(),
            timing: beat(4.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // ── strobes ──────────────────────────────────────────────────────

    // Irregular rather than metronomic. A shuffled selection on a fast
    // chase gives fixtures firing in an order that does not repeat
    // audibly, which is what makes a random strobe read as chaos where a
    // plain strobe reads as a machine.
    add(
        "random strobe",
        Recipe {
            target: role("Wash"),
            steps: vec![
                Step::new(vec![RecipeApply::Dimmer(1.0)]),
                Step::new(vec![RecipeApply::Dimmer(0.0)]),
                Step::new(vec![RecipeApply::Dimmer(0.0)]),
                Step::new(vec![RecipeApply::Dimmer(0.0)]),
            ],
            timing: beat(0.25, 360.0, Play::Forward),
            tricks: vec![Trick::Shuffle(4409)],
        },
    );

    // ── mover position ───────────────────────────────────────────────
    //
    // Every one of these is `orbit` with different numbers, which is the
    // point: a mover pattern is a Lissajous figure, and naming the
    // useful ones beats exposing four sliders nobody can picture.

    // The workhorse. Slow, wide, and it reads as motion without reading
    // as an effect — the thing to leave running under a whole verse.
    add("circle", mover(orbit(18.0, 18.0, 1.0, 1.0, 90.0), 4.0, 0.0));

    // Same shape, tighter and quicker, for a chorus.
    add("circle tight", mover(orbit(9.0, 9.0, 1.0, 1.0, 90.0), 2.0, 0.0));

    // The spokes. One circle, phase-spread all the way round the
    // selection, so the rig looks like a wheel turning rather than like
    // several heads doing the same thing.
    add("windmill", mover(orbit(16.0, 16.0, 1.0, 1.0, 90.0), 4.0, 360.0));

    // Tilt at twice pan: the beam crosses itself at the middle.
    add("figure eight", mover(orbit(20.0, 12.0, 1.0, 2.0, 0.0), 4.0, 0.0));

    // Three against two never resolves quickly, so the beam keeps
    // arriving somewhere it has not just been. That is what a ballyhoo
    // is for, and why the ratio matters more than the speed.
    add("ballyhoo", mover(orbit(28.0, 16.0, 3.0, 2.0, 45.0), 2.0, 0.0));

    // The same idea at half the size and twice the rate — a nervous,
    // close-in version for a breakdown.
    add("ballyhoo tight", mover(orbit(14.0, 9.0, 3.0, 2.0, 45.0), 1.0, 0.0));

    // Pan only. Flat, slow, and the least demanding thing a mover can
    // do while still being alive.
    add("sway", mover(orbit(25.0, 0.0, 1.0, 0.0, 0.0), 4.0, 0.0));

    // Pan only, spread across the rig: a wave rolling along the truss.
    add("pan wave", mover(orbit(20.0, 0.0, 1.0, 0.0, 0.0), 2.0, 360.0));

    // Tilt only, spread: beams rolling up and over, which reads
    // completely differently from the same wave on pan.
    add("tilt wave", mover(orbit(0.0, 18.0, 0.0, 1.0, 0.0), 2.0, 360.0));

    // A quick nod. Short and shallow — punctuation, not a look.
    add("nod", mover(orbit(0.0, 8.0, 0.0, 1.0, 0.0), 0.5, 0.0));

    // Mirrored halves opening and closing. `Wings(2)` is what makes one
    // definition symmetric; without it this is just everything sweeping
    // the same way.
    add(
        "converge",
        Recipe {
            tricks: vec![Trick::Wings(2)],
            ..mover(orbit(30.0, 0.0, 1.0, 0.0, 0.0), 4.0, 180.0)
        },
    );

    // Odds one way, evens the other. The cheapest mover effect that
    // reads as designed rather than as a sweep, and it survives any rig
    // size because `Group(2)` is a proportion.
    add(
        "cross",
        Recipe {
            tricks: vec![Trick::Group(2)],
            ..mover(orbit(26.0, 0.0, 1.0, 0.0, 0.0), 2.0, 180.0)
        },
    );

    // A static fan — a *look* rather than an effect, and here because it
    // is the shape most of the others are built on top of.
    add(
        "fan",
        Recipe {
            target: role("Movers"),
            steps: vec![Step::new(vec![RecipeApply::Delta(vec![(
                Attribute::Pan,
                28.0,
            )])])],
            timing: beat(1.0, 360.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // The fan, breathing open and shut.
    add("fan breathe", mover(orbit(22.0, 0.0, 1.0, 0.0, 0.0), 8.0, 300.0));

    // ── beam ─────────────────────────────────────────────────────────

    // Zoom pumping with the music. Relative, so it rides whatever zoom
    // the look established rather than resetting it.
    add(
        "zoom pulse",
        Recipe {
            target: role("Movers"),
            steps: Waveform::Sine.steps(Attribute::Zoom, 0.0, 0.25, true),
            timing: beat(1.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // Slow enough to be felt rather than seen.
    add(
        "zoom breathe",
        Recipe {
            target: role("Movers"),
            steps: Waveform::Sine.steps(Attribute::Zoom, 0.0, 0.15, true),
            timing: beat(8.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // ── colour ───────────────────────────────────────────────────────
    //
    // Absolute, unavoidably: a colour effect is *setting* the colour, so
    // there is nothing to be relative to. They therefore replace whatever
    // colour the look established, which is what they are for — but it
    // does mean stacking two of them is last-wins rather than a blend.

    // The rig walking through the spectrum together.
    add(
        "colour cycle",
        Recipe {
            target: role("Wash"),
            steps: vec![hue("Red"), hue("Amber"), hue("Green"), hue("Cyan"), hue("Blue"), hue("Magenta")],
            timing: beat(8.0, 0.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // The same table spread across the selection, so the spectrum is
    // laid out *along the rig* at any one moment and then travels.
    add(
        "rainbow",
        Recipe {
            target: role("Wash"),
            steps: vec![hue("Red"), hue("Amber"), hue("Green"), hue("Cyan"), hue("Blue"), hue("Magenta")],
            timing: beat(8.0, 360.0, Play::Forward),
            tricks: Vec::new(),
        },
    );

    // Two colours trading on odds and evens — the most useful colour
    // effect there is, and the one that survives being run under
    // everything else.
    add(
        "two tone",
        Recipe {
            target: role("Wash"),
            steps: vec![hue("Deep"), hue("Hot")],
            timing: beat(2.0, 180.0, Play::Forward),
            tricks: vec![Trick::Group(2)],
        },
    );

    // Warm against cool, which is the same trick with the temperature
    // rather than the hue and reads far calmer.
    add(
        "warm cool split",
        Recipe {
            target: role("Wash"),
            steps: vec![hue("Warm"), hue("Cool")],
            timing: beat(4.0, 180.0, Play::Forward),
            tricks: vec![Trick::Group(2)],
        },
    );

    // A single white frame in a coloured look — the cheapest accent
    // there is, and it wants a *narrow* window or it stops being one.
    add(
        "white flash",
        Recipe {
            target: role("Wash"),
            steps: vec![
                Step {
                    apply: vec![RecipeApply::Color(crate::preset::Ref::Named("Open White".into()))],
                    width: 1.0,
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
                Step {
                    apply: vec![RecipeApply::Color(crate::preset::Ref::Named("Deep".into()))],
                    width: 7.0,
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
            ],
            timing: beat(2.0, 0.0, Play::Forward),
            tricks: Vec::new(),
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

    /// Every name an effect reaches for is a role.
    ///
    /// The rule the library exists to keep, and it has to walk the whole
    /// selection tree rather than check the outermost node: a rig effect
    /// targets a `Union` of roles, and an earlier version of this test
    /// asked only whether the top-level selection *was* a role — which
    /// the union is not, and which would have passed for a union of
    /// venue group names just the same.
    ///
    /// One `Selection::Group` in here and that effect works at exactly
    /// one address, which is worse than it not existing: it would be
    /// picked from the same list as the portable ones and fail somewhere
    /// else.
    #[test]
    fn every_effect_names_only_roles() {
        fn venue_names(sel: &Selection, out: &mut Vec<String>) {
            match sel {
                Selection::Role(_) | Selection::Chans(_) => {}
                Selection::Group(n) => out.push(format!("group {n:?}")),
                Selection::Tag(t) => out.push(format!("tag {t:?}")),
                Selection::Model(m) => out.push(format!("model {m:?}")),
                Selection::Union(parts) | Selection::Intersect(parts) => {
                    parts.iter().for_each(|p| venue_names(p, out))
                }
                Selection::Except { of, minus } => {
                    venue_names(of, out);
                    venue_names(minus, out);
                }
                Selection::Where { of, .. } | Selection::Order { of, .. } => venue_names(of, out),
            }
        }
        for (name, recipe) in library() {
            let mut found = Vec::new();
            venue_names(&recipe.target, &mut found);
            assert!(
                found.is_empty(),
                "effect {name:?} reaches for venue-specific names: {found:?}"
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

    /// Every rate is against a named master, never a hard-coded tempo.
    ///
    /// `Song` for playback and `Tap` for busking — the same shapes with
    /// a different rate source, which is why they are one library rather
    /// than two. What must never appear is a raw Hz or BPM: that is a
    /// tempo somebody dialled in once, and it will not follow the music.
    #[test]
    fn every_effect_is_slaved_to_a_named_master() {
        for (name, recipe) in library() {
            match &recipe.timing.speed {
                Speed::Master(m) => assert!(
                    m == "Song" || m == "Tap",
                    "effect {name:?} runs on unknown master {m:?}"
                ),
                other => panic!("effect {name:?} has a hard-coded rate: {other:?}"),
            }
        }
    }

    /// Effects layer, so an intensity effect is relative unless there is
    /// a reason it cannot be.
    ///
    /// Two kinds of exception, and keeping them apart is the point.
    /// **Colour effects are absolute by nature**: setting a colour has
    /// nothing to be relative to, so they replace whatever the look
    /// established — which is what they are for, and also why stacking
    /// two is last-wins rather than a blend. The **named** exceptions are
    /// intensity effects that are deliberately absolute: a strobe that
    /// merely modulated would still show what was under it, and blinders
    /// that merely modulated would not blind.
    ///
    /// Listing them exactly is what stops the next accidental absolute —
    /// an effect that quietly stops layering is very hard to notice from
    /// the stage, because it looks like it is working.
    #[test]
    fn intensity_effects_are_relative_except_where_named() {
        // Each of these overrides rather than layers, on purpose: a
        // strobe or a blinder that merely modulated would still show
        // what was under it, and a whole-rig stab is meant to be the
        // only thing visible for its quarter beat.
        const DELIBERATELY_ABSOLUTE: [&str; 4] =
            ["strobe", "random strobe", "audience blind", "rig stab"];

        let mut unexpected: Vec<String> = Vec::new();
        for (name, recipe) in library() {
            let sets_colour = recipe.steps.iter().any(|s| {
                s.apply
                    .iter()
                    .any(|a| matches!(a, RecipeApply::Color(_)))
            });
            let absolute = recipe.steps.iter().any(|s| {
                s.apply
                    .iter()
                    .any(|a| !matches!(a, RecipeApply::Delta(_)))
            });
            if absolute && !sets_colour && !DELIBERATELY_ABSOLUTE.contains(&name.as_str()) {
                unexpected.push(name);
            }
        }
        assert!(
            unexpected.is_empty(),
            "these stopped layering without being declared: {unexpected:?}"
        );
    }

    /// A circle is one recipe whose steps carry both axes, a quarter
    /// cycle apart. Shipped as a pan/tilt *pair* — which is how a console
    /// with per-attribute effects has to do it — losing one silently
    /// turns the circle into a diagonal sweep that looks deliberate
    /// enough that nobody notices.
    #[test]
    fn a_circle_is_one_recipe_carrying_both_axes() {
        let circle = &library()["circle"];
        assert!(circle.steps.len() > 8, "too coarse to read as a curve");
        for step in &circle.steps {
            let attrs: Vec<&Attribute> = step
                .apply
                .iter()
                .flat_map(|a| match a {
                    RecipeApply::Delta(pairs) => pairs.iter().map(|(a, _)| a).collect(),
                    _ => Vec::new(),
                })
                .collect();
            assert!(attrs.contains(&&Attribute::Pan), "a step with no pan");
            assert!(attrs.contains(&&Attribute::Tilt), "a step with no tilt");
        }
    }

    /// The quarter-cycle offset is what makes it a circle rather than a
    /// line, and it lives in the table now. Pan peaks a quarter of the
    /// way round; tilt peaks at the start.
    #[test]
    fn the_circles_axes_are_a_quarter_cycle_apart() {
        let circle = &library()["circle"];
        let axis = |step: &Step, want: &Attribute| -> f32 {
            step.apply
                .iter()
                .filter_map(|a| match a {
                    RecipeApply::Delta(pairs) => {
                        pairs.iter().find(|(at, _)| at == want).map(|(_, v)| *v)
                    }
                    _ => None,
                })
                .next()
                .unwrap_or_default()
        };
        let quarter = circle.steps.len() / 4;
        // Pan starts at zero and is at its widest a quarter round.
        assert!(axis(&circle.steps[0], &Attribute::Pan).abs() < 1e-4);
        assert!(axis(&circle.steps[quarter], &Attribute::Pan).abs() > 10.0);
        // Tilt does the opposite, which is exactly the offset.
        assert!(axis(&circle.steps[0], &Attribute::Tilt).abs() > 10.0);
        assert!(axis(&circle.steps[quarter], &Attribute::Tilt).abs() < 1e-4);
    }

    /// A ballyhoo's axes must not share a simple ratio, or the beam
    /// resolves into a repeating shape and stops reading as excitement.
    #[test]
    fn a_ballyhoo_does_not_resolve_quickly() {
        let ballyhoo = &library()["ballyhoo"];
        let circle = &library()["circle"];
        assert_ne!(
            ballyhoo.steps, circle.steps,
            "the ballyhoo is drawing a circle"
        );
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
        for name in ["chase", "pulse", "build", "sway", "sparkle", "circle", "ballyhoo"] {
            assert!(
                library()[name].steps.len() > 1,
                "{name:?} has one step and cannot move"
            );
        }
    }
}
