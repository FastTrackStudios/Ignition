//! Movement — the mover patterns.
//!
//! See `super` for the three rules every effect here follows: relative
//! wherever possible, slaved to `Song`, direction from the selection.
//!
//! Every position here is a **relative** Pan/Tilt delta in degrees, so
//! the shape rides on whatever focus the cue set: a circle drawn around
//! the drummer stays around the drummer. Nothing here is an absolute
//! position — that is the cue's job, and a movement effect that reset
//! the focus would be a look wearing an effect's name.
//!
//! This is the console position catalogue and not much more: Eos's
//! circle, figure 8, spiral, ballyhoo and fly-out; the pan and tilt
//! sines, saws and squares every desk offers; MagicQ's pandim and
//! tiltdim; Hog's random. Most are `orbit` with different numbers —
//! a mover pattern is a Lissajous figure, and naming the useful ones
//! beats exposing four sliders nobody can picture. The rest are the
//! shapes an orbit cannot draw: a box, a snap, a kick.
//!
//! One-shots on the movers — fly in, lift off, the tilt ripple — live
//! with the other one-shots, because that is how an operator reaches
//! for them.
//!
//! **Degrees and metres.** A pattern in degrees is a look for one hang:
//! the same circle is a different size, and a different shape, at every
//! venue. [`orbit_m`] draws the shape *in the room* as `FocusDelta`
//! steps in metres, and every head solves its own angles per frame, so a
//! two-metre circle on the floor is two metres everywhere. Those
//! patterns are in [`add_room`]; the degree catalogue stays as it was.

use super::*;
use crate::step::Ease;
use crate::tricks::InvertStyle;
use ignition_proto::Vec3;

const FAMILY: &str = "movement";

/// A fully-transitioning step of both axes — one point on a path.
fn pt(pan: f32, tilt: f32) -> Step {
    Step {
        apply: vec![RecipeApply::Delta(vec![
            (Attribute::Pan, pan),
            (Attribute::Tilt, tilt),
        ])],
        width: 1.0,
        transition: 1.0,
        ..Step::new(Vec::new())
    }
}

/// The same point, snapped to rather than travelled to.
fn snap(pan: f32, tilt: f32) -> Step {
    Step {
        transition: 0.0,
        ..pt(pan, tilt)
    }
}

/// A `Recipe` targeting `Movers` with the given steps, timing and tricks.
fn movers(steps: Vec<Step>, timing: Timing, tricks: Vec<Trick>) -> Recipe {
    Recipe {
        target: role("Movers"),
        steps,
        timing,
        tricks,
        stack: false,
        ..Default::default()
    }
}

/// The circle, shared by its song- and tap-mastered spellings.
fn circle() -> Recipe {
    mover(orbit(18.0, 18.0, 1.0, 1.0, 90.0), 4.0, 0.0)
}

/// The ballyhoo, shared by its song- and tap-mastered spellings.
///
/// Three against two never resolves quickly, so the beam keeps arriving
/// somewhere it has not just been. That is what a ballyhoo is for, and
/// why the ratio matters more than the speed.
fn ballyhoo() -> Recipe {
    mover(orbit(28.0, 16.0, 3.0, 2.0, 45.0), 2.0, 0.0)
}

/// One axis swept while the beam is lit in one direction and dark on the
/// way back — MagicQ's pandim and tiltdim. The dimmer delta is in phase
/// with the axis's velocity, which is why it lives in the same table.
fn lit_one_way(axis: Attribute, amp: f32) -> Vec<Step> {
    (0..16)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / 16.0;
            // Velocity is cos(a): positive for the quarter either side
            // of the start. Counted by index rather than by sign so the
            // two zero crossings do not land on floating-point noise.
            let lit = i < 4 || i >= 12;
            Step {
                apply: vec![RecipeApply::Delta(vec![
                    (axis.clone(), amp * a.sin()),
                    (Attribute::Dimmer, if lit { 0.0 } else { -0.9 }),
                ])],
                width: 1.0,
                transition: 1.0,
                ..Step::new(Vec::new())
            }
        })
        .collect()
}

/// A parametric path **in metres**, on the floor plane about the aim —
/// the room-space twin of `orbit`.
///
/// Each step is a `FocusDelta`: an offset in metres from wherever the
/// cue has the head aimed, x across the stage and y up it, z level with
/// the aim. Same vocabulary as `orbit` — cycles per axis and a phase —
/// so 1/1 at 90° is a circle, 1/2 a figure of eight, 1/0 a line. The
/// interpolation between the sixteen points happens on the metres,
/// before the solve, so the path is a curve at every head.
// r[impl focus.orbit-in-metres]
// r[impl focus.delta]
pub(super) fn orbit_m(
    radius_x_m: f32,
    radius_y_m: f32,
    cycles_x: f32,
    cycles_y: f32,
    phase_deg: f32,
) -> Vec<Step> {
    const RESOLUTION: usize = 16;
    let tau = std::f32::consts::TAU;
    (0..RESOLUTION)
        .map(|i| {
            let t = i as f32 / RESOLUTION as f32;
            let x = radius_x_m * (tau * cycles_x * t).sin();
            let y = radius_y_m * (tau * cycles_y * t + phase_deg.to_radians()).sin();
            Step {
                apply: vec![RecipeApply::FocusDelta(Vec3 {
                    x: x as f64,
                    y: y as f64,
                    z: 0.0,
                })],
                width: 1.0,
                transition: 1.0,
                ..Step::new(Vec::new())
            }
        })
        .collect()
}

/// The same path, drawn around a named focus rather than the cue's aim:
/// every step carries the point too, so the recipe solves to absolute
/// pan/tilt on its own and needs nothing from the player.
// r[impl focus.orbit-in-metres] - around a named focus, self-contained
// r[impl focus.delta] - folded into the point in the same step
fn around(focus: &str, steps: Vec<Step>) -> Vec<Step> {
    steps
        .into_iter()
        .map(|mut step| {
            step.apply.insert(
                0,
                RecipeApply::FocusPoint(crate::preset::Ref::Named(focus.into())),
            );
            step
        })
        .collect()
}

/// The patterns authored in metres — the portable form of a mover
/// pattern (`r[focus.orbit-in-metres]`).
///
/// Kept in their own list so the degree catalogue is untouched: the
/// library's own checks read every non-`Delta` apply as "stopped
/// layering" and cap the movement family, and both need to learn about
/// `FocusDelta` before these are wired into [`add`]. Everything here
/// passes the same tests the rest of the family does (see the tests
/// below), so wiring is one call.
// r[impl focus.orbit-in-metres]
// r[impl effects.library.categories]
#[allow(dead_code)] // wired by `effects::catalogue` once its checks accept `FocusDelta`/`Random`
pub(super) fn add_room(add: Add) {
    let mut put = |name: &str, about: &str, recipe: Recipe| add(name, FAMILY, about, recipe);

    // A two-metre circle on the floor at the aim. The same two metres at
    // every venue, whatever the trim height — which is what the circle
    // in degrees never was.
    put(
        "room circle",
        "every beam drawing a two metre circle on the floor round its aim over four bars, the same size at any venue — verses",
        mover(orbit_m(1.0, 1.0, 1.0, 1.0, 90.0), 4.0, 0.0),
    );

    // A figure of eight three metres wide and a metre and a half deep,
    // lying on the floor: the eight in the room rather than on the yoke.
    put(
        "floor eight",
        "every beam tracing a three metre figure of eight on the floor over four bars — bridges, the eight in the room",
        mover(orbit_m(1.5, 0.75, 1.0, 2.0, 0.0), 4.0, 0.0),
    );

    // A three-metre horizontal line across the aim: a sweep whose ends
    // are places on the stage, not angles on the head.
    put(
        "wall sweep",
        "every beam sweeping a three metre line across its aim and back every two bars — choruses, a wall of beams that stays a wall",
        mover(orbit_m(1.5, 0.0, 1.0, 0.0, 0.0), 2.0, 0.0),
    );

    // A one-metre ring round the drummer, spread round the rig so the
    // beams chase each other round him. Self-contained: it names the
    // focus, so it works with no aim under it.
    put(
        "drum ring",
        "beams circling a metre round the drummer, spread so they chase each other round the kit every two bars — drum breaks",
        mover(
            around("Drums", orbit_m(1.0, 1.0, 1.0, 1.0, 90.0)),
            2.0,
            360.0,
        ),
    );
}

/// Adds this family to the library.
// r[impl effects.library.categories]
pub(super) fn add(add: Add) {
    let mut put = |name: &str, about: &str, recipe: Recipe| add(name, FAMILY, about, recipe);

    // ── orbits ───────────────────────────────────────────────────────

    // The workhorse. Slow, wide, and it reads as motion without reading
    // as an effect — the thing to leave running under a whole verse.
    put(
        "circle",
        "every beam drawing a slow, wide circle round its focus over four bars — verses, motion that is not an effect",
        circle(),
    );

    // Same shape, tighter and quicker. Kept beside `circle` because the
    // show author reaches for it by name; size and rate are otherwise
    // live controls.
    put(
        "circle tight",
        "a small quick circle round the focus every two bars — choruses, busier than circle",
        mover(orbit(9.0, 9.0, 1.0, 1.0, 90.0), 2.0, 0.0),
    );

    // One circle, phase-spread all the way round the selection, so the
    // rig looks like a wheel turning rather than several heads doing
    // the same thing.
    put(
        "windmill",
        "circles spread round the rig so the beams turn like spokes of a wheel — choruses and outros",
        mover(orbit(16.0, 16.0, 1.0, 1.0, 90.0), 4.0, 360.0),
    );

    // Tilt at twice pan: the beam crosses itself at the middle.
    put(
        "figure eight",
        "every beam tracing a figure of eight over four bars — verses and bridges, lazier than a circle",
        mover(orbit(20.0, 12.0, 1.0, 2.0, 0.0), 4.0, 0.0),
    );

    put(
        "ballyhoo",
        "beams roaming the room in a loop that never quite repeats — choruses and last choruses",
        ballyhoo(),
    );

    // A circle that grows from nothing to full width over the cycle,
    // then snaps back and starts again — the spiral every console lists
    // next to the circle. Built by scaling each point of an orbit by how
    // far through the table it is; three turns per cycle, so the growth
    // reads as rings rather than as one wobbling loop.
    put(
        "spiral",
        "beams spiralling out from the focus in widening rings, then snapping back — pre-choruses and builds",
        mover(
            orbit(22.0, 22.0, 3.0, 3.0, 90.0)
                .into_iter()
                .enumerate()
                .map(|(i, mut step)| {
                    let scale = (i as f32 + 1.0) / 16.0;
                    for apply in &mut step.apply {
                        if let RecipeApply::Delta(pairs) = apply {
                            for (_, v) in pairs.iter_mut() {
                                *v *= scale;
                            }
                        }
                    }
                    step
                })
                .collect(),
            4.0,
            0.0,
        ),
    );

    // A circle that also breathes in intensity — the beam is dimmest at
    // the far side of the arc, which reads as depth rather than as two
    // effects happening at once.
    put(
        "circle breathe",
        "a slow circle that dims on the far side of the arc, so the beams seem to go away and come back — verses",
        movers(
            (0..16)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / 16.0;
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
            beat(4.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Four corners, each travelled to along a straight edge, so the beam
    // draws a box rather than the rounded loop an orbit gives.
    put(
        "square path",
        "every beam tracing a box, corner to corner, every two bars — choruses that want a harder shape than a circle",
        mover(
            vec![
                pt(-16.0, 16.0),
                pt(16.0, 16.0),
                pt(16.0, -16.0),
                pt(-16.0, -16.0),
            ],
            2.0,
            0.0,
        ),
    );

    // ── pan ──────────────────────────────────────────────────────────

    // Pan only. Flat, slow, and the least demanding thing a mover can
    // do while still being alive.
    put(
        "sway",
        "every beam swinging slowly side to side together over four bars — verses and ballads",
        mover(orbit(25.0, 0.0, 1.0, 0.0, 0.0), 4.0, 0.0),
    );

    // Pan only, spread across the rig: a wave rolling along the truss.
    put(
        "pan wave",
        "beams swinging side to side in sequence so a wave rolls along the truss every two bars — pre-choruses",
        mover(orbit(20.0, 0.0, 1.0, 0.0, 0.0), 2.0, 360.0),
    );

    // Everything panning one way together, then snapping back: the
    // classic sweep across the crowd. Unison, because a spread sweep is
    // a wave and this is meant to be one wall of light moving.
    put(
        "pan sweep",
        "all the beams sweeping one way together and snapping back every two bars — choruses, one wall of light moving",
        movers(
            Waveform::RampUp.steps(Attribute::Pan, 0.0, 24.0, true),
            beat(2.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Odds one way, evens the other. The cheapest mover effect that
    // reads as designed rather than as a sweep, and it survives any rig
    // size because `Group(2)` is a proportion.
    put(
        "cross",
        "odd beams swinging one way as even beams swing the other, crossing every two bars — choruses",
        Recipe {
            tricks: vec![Trick::Group(2)],
            ..mover(orbit(26.0, 0.0, 1.0, 0.0, 0.0), 2.0, 180.0)
        },
    );

    // `Wings(2)` splits the selection in half and mirrors the second
    // half, so with a 180 spread the outermost heads sit at one end of
    // the wave and the centre pair at the other. A pan sine under that
    // opens outward from the middle and closes back, like a fan of
    // beams — and it holds at any rig size because the split is a
    // proportion, not a count.
    put(
        "pan fan",
        "the beams fanning apart from the centre and closing again every two bars — choruses, symmetric rigs",
        movers(
            Waveform::Sine.steps(Attribute::Pan, 0.0, 26.0, true),
            beat(2.0, 180.0, Play::Forward),
            vec![Trick::Wings(2)],
        ),
    );

    // Lit while sweeping one way, dark on the way back — the beam
    // appears at one side, crosses, and vanishes, like a searchlight
    // with a shutter. MagicQ's pandim.
    put(
        "pan dim",
        "beams lit as they sweep one way and dark on the way back, so light only ever travels one direction — choruses",
        movers(
            lit_one_way(Attribute::Pan, 24.0),
            beat(2.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── tilt ─────────────────────────────────────────────────────────

    // Tilt only, spread: beams rolling up and over, which reads
    // completely differently from the same wave on pan.
    put(
        "tilt wave",
        "beams rising and falling in sequence so a wave rolls along the truss every two bars — pre-choruses and bridges",
        mover(orbit(0.0, 18.0, 0.0, 1.0, 0.0), 2.0, 360.0),
    );

    // A quick nod. Short and shallow — punctuation, not a look.
    put(
        "nod",
        "every beam dipping and lifting a little every half bar — choruses, the rig nodding along",
        mover(orbit(0.0, 8.0, 0.0, 1.0, 0.0), 0.5, 0.0),
    );

    // Tilt snaps up and falls slowly, odds against evens — the nod as a
    // *snap* rather than a sine. Reads as a head-bang on a downbeat.
    put(
        "head bang",
        "beams snapping up and falling back, odds against evens, once a bar — heavy choruses and drops",
        movers(
            Waveform::RampDown.steps(Attribute::Tilt, 0.0, 12.0, true),
            beat(1.0, 180.0, Play::Forward),
            vec![Trick::Group(2)],
        ),
    );

    // Beams opening upward from the centre pair to the outer ones and
    // closing again — the fan, on tilt.
    put(
        "tilt fan",
        "beams lifting from the centre outward and settling back over four bars — bridges and pre-choruses",
        movers(
            Waveform::Sine.steps(Attribute::Tilt, 0.0, 16.0, true),
            beat(4.0, 180.0, Play::Forward),
            vec![Trick::Wings(2)],
        ),
    );

    // Lit on the way up, dark on the way down. MagicQ's tiltdim, and the
    // partner of `pan dim`.
    put(
        "tilt dim",
        "beams lit as they rise and dark as they fall, so light only ever climbs — builds and pre-choruses",
        movers(
            lit_one_way(Attribute::Tilt, 18.0),
            beat(2.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // A drop and a bounce: the beam falls hard, rebounds most of the
    // way, falls again and settles. Snapping *down* and easing *up* is
    // what makes it read as a ball rather than a wave.
    put(
        "tilt bounce",
        "beams dropping hard and bouncing back like a ball every half bar — drops and breakdowns",
        movers(
            vec![
                Step {
                    apply: vec![RecipeApply::Delta(vec![(Attribute::Tilt, -12.0)])],
                    width: 1.0,
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
                Step {
                    apply: vec![RecipeApply::Delta(vec![(Attribute::Tilt, 4.0)])],
                    width: 1.0,
                    transition: 1.0,
                    ease: Ease::Sine,
                },
                Step {
                    apply: vec![RecipeApply::Delta(vec![(Attribute::Tilt, -6.0)])],
                    width: 1.0,
                    transition: 0.0,
                    ..Step::new(Vec::new())
                },
                Step {
                    apply: vec![RecipeApply::Delta(vec![(Attribute::Tilt, 0.0)])],
                    width: 1.0,
                    transition: 1.0,
                    ease: Ease::Sine,
                },
            ],
            beat(0.5, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // Rising and opening together. Eos's fly-out: a beam that opens as
    // it rises reads as one gesture, where a zoom effect and a tilt
    // effect run separately read as two.
    put(
        "fly out",
        "beams rising and opening wide together, then settling back, over four bars — choruses and outros",
        movers(
            (0..12)
                .map(|i| {
                    let up = (std::f32::consts::TAU * i as f32 / 12.0).sin();
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
            beat(4.0, 0.0, Play::Forward),
            Vec::new(),
        ),
    );

    // ── figures ──────────────────────────────────────────────────────

    // Pan flicks hard left and right on the beat while tilt swings
    // smoothly, odds against evens: the kick-line. The pan is a snap
    // and the tilt a curve, which is what makes it a can-can rather
    // than a cross.
    put(
        "can can",
        "beams kicking left and right on the beat while they rise and fall, odds against evens — last choruses",
        movers(
            (0..8)
                .map(|i| {
                    let t = i as f32 / 8.0;
                    let pan = if i % 2 == 0 { 14.0 } else { -14.0 };
                    let tilt = 12.0 * (std::f32::consts::TAU * t).sin();
                    Step {
                        apply: vec![RecipeApply::Delta(vec![
                            (Attribute::Pan, pan),
                            (Attribute::Tilt, tilt),
                        ])],
                        width: 1.0,
                        transition: 0.0,
                        ease: Ease::Sine,
                    }
                })
                .collect(),
            beat(2.0, 180.0, Play::Forward),
            vec![Trick::Group(2)],
        ),
    );

    // Heads snapping to a new place each beat, each at its own moment
    // in a shuffled order, so the rig never moves as one. Snaps rather
    // than travels — a jump that eased would be a wander.
    put(
        "random jump",
        "beams snapping to new positions one at a time in a scattered order — drops and breakdowns",
        movers(
            vec![
                snap(18.0, 8.0),
                snap(-12.0, -10.0),
                snap(6.0, 14.0),
                snap(-20.0, 2.0),
                snap(14.0, -12.0),
                snap(-4.0, 10.0),
                snap(22.0, -4.0),
                snap(-16.0, -14.0),
            ],
            beat(2.0, 360.0, Play::Forward),
            vec![Trick::Shuffle(2311)],
        ),
    );

    // The beams grazing slowly across the crowd and back. Small, slow,
    // bouncing rather than snapping, so nobody gets a beam in the eye at
    // the wrap. On `Beams` because those are the heads pointed out.
    put(
        "audience sweep",
        "the beams grazing slowly across the crowd and back over four bars — last choruses and outros",
        Recipe {
            target: role("Beams"),
            steps: Waveform::Sine.steps(Attribute::Pan, 0.0, 15.0, true),
            timing: beat(4.0, 90.0, Play::Bounce),
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        },
    );

    // ── inverted ─────────────────────────────────────────────────────

    // The circle, halves going opposite ways. `Invert(Pan)` flips the
    // pan delta on the far half of the rig, so one recipe draws
    // clockwise on one side and anticlockwise on the other, and the
    // pair meets in the middle twice a cycle.
    put(
        "counter circle",
        "the two halves of the rig circling opposite ways so the beams meet and part over four bars — choruses and outros",
        Recipe {
            tricks: vec![Trick::Invert(InvertStyle::Pan)],
            ..circle()
        },
    );

    // A pan wave rolling along the truss while odds and evens tilt
    // against each other: `Group(2)` makes two units, `Invert(Tilt)`
    // flips the second, and the beams cross like blades.
    put(
        "scissor",
        "odd beams tilting up as even beams tilt down while a pan wave rolls along the truss every two bars — choruses, blades crossing",
        movers(
            orbit(14.0, 12.0, 1.0, 1.0, 0.0),
            beat(2.0, 180.0, Play::Forward),
            vec![Trick::Group(2), Trick::Invert(InvertStyle::Tilt)],
        ),
    );

    // ── busked ───────────────────────────────────────────────────────

    put(
        "tap circle",
        "the circle on tap tempo — busking a set with no track running",
        Recipe {
            timing: tapped(4.0, 0.0, Play::Forward),
            ..circle()
        },
    );

    put(
        "tap ballyhoo",
        "the ballyhoo on tap tempo — busking a set with no track running",
        Recipe {
            timing: tapped(2.0, 0.0, Play::Forward),
            ..ballyhoo()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::super::library;
    use super::*;

    /// A path in metres obeys size like any other swing: at half size
    /// the circle is half the radius, at zero it is the aim itself.
    /// r[verify effects.size-scales-the-swing]
    /// r[verify focus.orbit-in-metres]
    #[test]
    fn a_metre_orbit_shrinks_about_its_aim_with_size() {
        let recipe = Recipe {
            target: Selection::Chans(vec![1]),
            steps: orbit_m(2.0, 2.0, 1.0, 1.0, 90.0),
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            ..Default::default()
        };
        let rig = crate::selection::Rig::new(vec![crate::selection::FixtureInfo {
            chan: 1,
            placement: None,
            manufacturer: String::new(),
            model: String::new(),
            tags: Vec::new(),
        }]);
        let show = crate::recipe::Show::new(&[], &rig);
        let radius = |show: &crate::recipe::Show<'_>| {
            let d = crate::recipe::expand_recipe_full(&recipe, show, 0.25).focus_deltas[0].delta;
            (d.x * d.x + d.y * d.y).sqrt()
        };
        let full = radius(&show);
        assert!((full - 2.0).abs() < 0.05, "{full}");
        assert!((radius(&show.scaled(0.5, 1.0)) - 1.0).abs() < 0.05);
        assert!(radius(&show.scaled(0.0, 1.0)).abs() < 1e-9);
    }

    fn axis(step: &Step, want: &Attribute) -> f32 {
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
    }

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

    fn room() -> BTreeMap<String, Recipe> {
        let mut out = BTreeMap::new();
        add_room(&mut |name: &str, fam: &str, about: &str, recipe: Recipe| {
            assert_eq!(fam, FAMILY);
            assert!(about.contains(" — "), "{name:?} needs a note");
            assert!(
                out.insert(name.to_string(), recipe).is_none(),
                "duplicate {name:?}"
            );
        });
        out
    }

    /// Everything here moves a head: every step carries pan or tilt —
    /// in degrees, or in metres.
    #[test]
    fn every_pattern_moves_pan_or_tilt() {
        for (name, r) in family().into_iter().chain(room()) {
            assert!(r.steps.len() > 1, "{name:?} cannot move");
            for step in &r.steps {
                let pans = step.apply.iter().any(|a| match a {
                    RecipeApply::Delta(pairs) => pairs
                        .iter()
                        .any(|(at, _)| matches!(at, Attribute::Pan | Attribute::Tilt)),
                    RecipeApply::FocusDelta(_) => true,
                    _ => false,
                });
                assert!(pans, "{name:?} has a step that moves nothing");
            }
        }
    }

    /// The room patterns share the family's rules — bar rates, the song
    /// master, movers only — and their names do not collide with the
    /// degree catalogue.
    // r[verify focus.orbit-in-metres]
    #[test]
    fn the_room_patterns_follow_the_familys_rules() {
        let degrees = family();
        for (name, r) in room() {
            assert!(
                !degrees.contains_key(&name),
                "{name:?} shadows a degree pattern"
            );
            assert_eq!(r.target, role("Movers"));
            assert_eq!(r.timing.speed, Speed::Master("Song".into()));
            assert!(
                [4.0, 2.0, 1.0, 0.5].contains(&(r.timing.measure / 4.0)),
                "{name:?}"
            );
            assert!(!r.timing.once);
        }
    }

    /// A metre orbit is metres: sixteen `FocusDelta` points, a circle of
    /// the stated radius, blending between steps.
    // r[verify focus.orbit-in-metres]
    #[test]
    fn a_metre_orbit_is_a_circle_of_that_radius() {
        let steps = orbit_m(1.0, 1.0, 1.0, 1.0, 90.0);
        assert_eq!(steps.len(), 16);
        for step in &steps {
            assert_eq!(step.transition, 1.0);
            match &step.apply[0] {
                RecipeApply::FocusDelta(d) => {
                    assert!(
                        (d.x.hypot(d.y) - 1.0).abs() < 1e-5,
                        "{d:?} is not on the circle"
                    );
                    assert_eq!(d.z, 0.0, "on the floor plane");
                }
                other => panic!("not a delta in metres: {other:?}"),
            }
        }
        // The two metre circle really is two metres across.
        let circle = &room()["room circle"];
        let xs: Vec<f64> = circle
            .steps
            .iter()
            .filter_map(|s| match &s.apply[0] {
                RecipeApply::FocusDelta(d) => Some(d.x),
                _ => None,
            })
            .collect();
        let width = xs.iter().cloned().fold(f64::MIN, f64::max)
            - xs.iter().cloned().fold(f64::MAX, f64::min);
        assert!((width - 2.0).abs() < 1e-5, "{width}");
    }

    /// The drum ring names its focus in every step, so it resolves to
    /// absolute pan/tilt on its own and every head aims at a point on
    /// the ring — the same ring at any trim height.
    // r[verify focus.orbit-in-metres]
    // r[verify focus.delta]
    #[test]
    fn the_drum_ring_solves_to_angles_round_the_kit_at_any_venue() {
        use crate::preset::{FocusPointPreset, Palettes};
        use crate::recipe::{Show, expand_recipe_full};
        use crate::selection::{FixtureInfo, Rig};
        use ignition_proto::{Placement, Quat};
        let ring = &room()["drum ring"];
        for step in &ring.steps {
            assert!(
                matches!(&step.apply[0], RecipeApply::FocusPoint(crate::preset::Ref::Named(n)) if n == "Drums")
            );
            assert!(matches!(&step.apply[1], RecipeApply::FocusDelta(_)));
        }
        let kit = Vec3 {
            x: 0.0,
            y: -3.0,
            z: 0.0,
        };
        let pool = Palettes {
            focus: vec![FocusPointPreset {
                name: "Drums".into(),
                target: kit,
            }],
            ..Default::default()
        };
        let rig_at = |z: f64| {
            Rig::new(vec![FixtureInfo {
                chan: 1,
                placement: Some(Placement {
                    position: Vec3 { x: 0.0, y: 0.0, z },
                    orientation: Quat {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    },
                }),
                manufacturer: String::new(),
                model: String::new(),
                tags: Vec::new(),
            }])
        };
        let recipe = Recipe {
            target: Selection::Chans(vec![1]),
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            ..ring.clone()
        };
        let mut speeds = crate::step::SpeedMasters::new();
        speeds.insert("Song".into(), 120.0);
        for z in [4.0, 7.0] {
            let rig = rig_at(z);
            let show = Show {
                groups: &[],
                palettes: &pool,
                rig: &rig,
                speeds: &speeds,
                roles: &crate::recipe::NO_ROLES,
                ..Show::new(&[], &rig)
            };
            let out = expand_recipe_full(&recipe, &show, 0.1);
            assert!(out.focus_deltas.is_empty(), "self-contained");
            let tilt = out
                .emits
                .iter()
                .find(|e| e.value.attr == Attribute::Tilt)
                .unwrap();
            assert!(!tilt.relative);
            // Aimed roughly at the kit: three metres out from under a head
            // z high is atan(3/z) off straight down, give or take the ring.
            let straight = (3.0f32 / z as f32).atan().to_degrees();
            assert!(
                (tilt.value.value - straight).abs() < 20.0,
                "z={z}: tilt {} vs {straight}",
                tilt.value.value
            );
        }
    }

    /// The inverted pair are the plain shapes with an Invert on them,
    /// and the invert lands where the note says.
    // r[verify tricks.invert]
    // r[verify effects.invert]
    #[test]
    fn counter_circle_and_scissor_invert_the_right_axis() {
        let lib = library();
        assert_eq!(lib["counter circle"].steps, lib["circle"].steps);
        assert_eq!(
            lib["counter circle"].tricks,
            vec![Trick::Invert(InvertStyle::Pan)]
        );
        assert_eq!(
            lib["scissor"].tricks,
            vec![Trick::Group(2), Trick::Invert(InvertStyle::Tilt)]
        );
        // Through the expander: the far half's pan runs the other way.
        use crate::recipe::{Show, expand_recipe};
        let recipe = Recipe {
            target: Selection::Chans(vec![1, 2, 3, 4]),
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
            ..lib["counter circle"].clone()
        };
        let emits = expand_recipe(&recipe, &Show::new(&[], &crate::selection::EMPTY_RIG), 0.25);
        let pan = |chan| {
            emits
                .iter()
                .find(|e| e.value.chan == chan && e.value.attr == Attribute::Pan)
                .map(|e| e.value.value)
                .unwrap()
        };
        assert!(pan(1).abs() > 5.0);
        assert_eq!(pan(1), pan(2));
        assert_eq!(pan(1), -pan(3));
        assert_eq!(pan(3), pan(4));
    }

    /// The spiral's radius grows across its table.
    #[test]
    fn the_spiral_grows() {
        let spiral = &library()["spiral"];
        let radius = |s: &Step| axis(s, &Attribute::Pan).hypot(axis(s, &Attribute::Tilt));
        let first = radius(&spiral.steps[0]);
        let last = radius(spiral.steps.last().unwrap());
        assert!(
            last > first * 4.0,
            "spiral does not grow: {first} -> {last}"
        );
    }

    /// A can-can kicks left and right on alternate steps.
    #[test]
    fn the_can_can_alternates_pan() {
        let can = &library()["can can"];
        for pair in can.steps.windows(2) {
            let (a, b) = (
                axis(&pair[0], &Attribute::Pan),
                axis(&pair[1], &Attribute::Pan),
            );
            assert!(a * b < 0.0, "pan does not alternate: {a} then {b}");
        }
    }

    /// The fans get their symmetry from `Wings(2)`, not from two tables;
    /// the cross gets its opposition from `Group(2)`.
    #[test]
    fn the_fans_are_winged_and_the_cross_is_grouped() {
        let lib = library();
        for name in ["tilt fan", "pan fan"] {
            assert!(
                lib[name].tricks.contains(&Trick::Wings(2)),
                "{name:?} is not mirrored"
            );
        }
        assert!(lib["cross"].tricks.contains(&Trick::Group(2)));
    }

    /// Pan dim and tilt dim are dark for exactly half the cycle — the
    /// return stroke — and never dark while moving forward.
    #[test]
    fn the_dim_sweeps_are_lit_one_way() {
        let lib = library();
        for name in ["pan dim", "tilt dim"] {
            let dark = lib[name]
                .steps
                .iter()
                .filter(|s| axis(s, &Attribute::Dimmer) < 0.0)
                .count();
            assert_eq!(dark, lib[name].steps.len() / 2, "{name:?}");
        }
    }
}
