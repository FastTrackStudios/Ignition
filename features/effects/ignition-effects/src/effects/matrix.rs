//! Matrix effects — pictures painted on a wall of fixtures.
//!
//! Everything else in the library is a *step table* spread along an
//! ordered selection: a chase knows how many units there are and gives
//! each one a phase. That is a line. These are the other thing — a
//! [`CanvasRecipe`], a picture with no idea how many fixtures are
//! looking at it, sampled at each unit's position on a two-axis grid.
//! A chase across a wall of four towers is four steps however many
//! lamps are on them; a picture is a picture at any resolution.
//!
//! All of them declare [`CanvasPlane::Wall`], which is what makes them
//! matrix effects rather than plan-view ones: the grid's V axis becomes
//! *height*, so a snake climbs and rain falls. On a rig whose wash is
//! one flat row the vertical axis is a single cell and these degrade to
//! their horizontal motion — a snake becomes a comet, rain becomes a
//! twinkle. That is the honest fallback and it is why they can ship in
//! a portable library at all.
//!
//! Written against `Wash`, the role whose fixtures are the room's main
//! surface and the only one big enough for a picture to read on.
//!
//! Brightness, not colour: every one drives `Dimmer` through a
//! [`BitmapChannel`], relative, so the cue owns the hue and the picture
//! owns the movement. A cue that left the wash dark shows nothing —
//! these are effects, not looks.

use super::{Add, Attribute, Put, Recipe, RecipeApply, Speed, Step, Timing, role};
use ignition_show::canvas::{
    BitmapChannel, CanvasPlane, CanvasRecipe, Procedural, Quantity, Travel,
};

const FAMILY: &str = "matrix";

/// White, so the channel's brightness reading is the picture's shape and
/// nothing else. The cue's colour is what reaches the lamp.
const INK: [f32; 3] = [1.0, 1.0, 1.0];

/// A picture on the wash, as a relative dimmer.
///
/// `low` is how far down the picture pulls a fixture it leaves black:
/// −0.85 is "almost out but not quite", which keeps the wall present
/// between passes instead of strobing the room off.
fn picture(source: Procedural, measure: f32, low: f32) -> Recipe {
    Recipe {
        // r[impl effects.library.roles-only]
        target: role("Wash"),
        steps: vec![Step::new(vec![RecipeApply::Canvas {
            recipe: CanvasRecipe {
                source,
                timing: Timing {
                    speed: Speed::Master("Song".into()),
                    measure,
                    ..Default::default()
                },
                // The whole point: V is height.
                // r[impl canvas.grid]
                plane: CanvasPlane::Wall,
            },
            channel: BitmapChannel {
                canvas: "wash".into(),
                quantity: Quantity::Brightness,
                attr: Attribute::Dimmer,
                low,
                high: 0.0,
                relative: true,
            },
        }])],
        timing: Timing {
            speed: Speed::Master("Song".into()),
            measure,
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn add(add: Add) {
    let mut put = |name: &str, about: &str, recipe: Recipe| add(name, FAMILY, about, recipe);
    add_snakes(&mut put);
    add_rain(&mut put);
    add_flat(&mut put);
}

/// The snake picture, at its three speeds and axes.
fn add_snakes(add: Put) {
    // ── Snakes ───────────────────────────────────────────────────────
    add(
        "wall snake",
        "a head crawling a serpentine path across the wash with a fading tail — the matrix move, for a bridge or a breakdown that needs one thing happening",
        picture(
            Procedural::Snake {
                color: INK,
                rows: 4,
                tail: 0.35,
                direction: Travel::Horizontal,
            },
            16.0,
            -0.85,
        ),
    );
    add(
        "wall snake climb",
        "the same snake weaving up the columns instead of across the rows — reads as a climb on a rig with height, as a slow chase on one without",
        picture(
            Procedural::Snake {
                color: INK,
                rows: 4,
                tail: 0.35,
                direction: Travel::Vertical,
            },
            16.0,
            -0.85,
        ),
    );
    add(
        "wall snake fast",
        "a short-tailed snake at a bar a lap — a last-chorus move, too busy for a verse",
        picture(
            Procedural::Snake {
                color: INK,
                rows: 6,
                tail: 0.18,
                direction: Travel::Horizontal,
            },
            4.0,
            -0.9,
        ),
    );
}

/// The rain picture, light and heavy.
fn add_rain(add: Put) {
    // ── Rain ─────────────────────────────────────────────────────────
    add(
        "wall rain",
        "drops falling down each column on its own offset — weather rather than a chase, and the one effect that reads as *down* on a tower wall",
        picture(
            Procedural::Rain {
                color: INK,
                columns: 4,
                tail: 0.45,
                seed: 5081,
            },
            16.0,
            -0.8,
        ),
    );
    add(
        "wall rain heavy",
        "the same rain twice as fast with more columns and a shorter tail — a downpour, for a drop or a breakdown's last bar",
        picture(
            Procedural::Rain {
                color: INK,
                columns: 8,
                tail: 0.22,
                seed: 991,
            },
            8.0,
            -0.9,
        ),
    );
}

/// The flat wipe, band and sparkle pictures — the ones that read on any
/// wall, tall or flat.
fn add_flat(add: Put) {
    // ── The flat ones, which read on any wall ────────────────────────
    add(
        "wall wipe",
        "one soft bar crossing the wash bottom to top — the simplest thing a wall can do that a row cannot",
        picture(
            Procedural::Wipe {
                color: INK,
                width: 0.45,
                direction: Travel::Vertical,
            },
            16.0,
            -0.8,
        ),
    );
    add(
        "wall bands",
        "three horizontal bands scrolling up the wash — a slow riser under a pre-chorus",
        picture(
            Procedural::Band {
                color: INK,
                width: 0.4,
                count: 3,
                direction: Travel::Vertical,
            },
            16.0,
            -0.7,
        ),
    );
    add(
        "wall sparkle",
        "cells lighting at random across the wall and fading — a texture, not a move; safe under a verse",
        picture(
            Procedural::Sparkle {
                density: 0.28,
                seed: 3313,
                color: INK,
            },
            8.0,
            -0.6,
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::super::float_of;
    use super::*;
    use std::collections::BTreeMap;

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

    fn canvas_of(r: &Recipe) -> &CanvasRecipe {
        r.steps[0]
            .apply
            .iter()
            .find_map(|a| match a {
                RecipeApply::Canvas { recipe, .. } => Some(recipe),
                _ => None,
            })
            .expect("a canvas apply")
    }

    /// Every one of these is a picture on the wall plane, on the wash,
    /// on the song's clock, relative.
    ///
    /// The plane is the load-bearing one: on `Plan` the grid's V axis is
    /// upstage/downstage, which for a wall of towers is a single cell —
    /// a snake would crawl sideways and rain would not fall.
    /// r[verify canvas.grid]
    /// r[verify effects.library.roles-only]
    #[test]
    fn every_matrix_effect_paints_on_the_wall() {
        for (name, r) in family() {
            assert_eq!(r.target, role("Wash"), "{name:?} left the wash");
            let c = canvas_of(&r);
            assert_eq!(c.plane, CanvasPlane::Wall, "{name:?} is not on the wall");
            assert!(
                matches!(c.timing.speed, Speed::Master(ref m) if m == "Song"),
                "{name:?} is not on the song"
            );
            assert!(!c.timing.once, "{name:?} runs once");
            let relative = r.steps[0].apply.iter().any(|a| match a {
                RecipeApply::Canvas { channel, .. } => channel.relative,
                _ => false,
            });
            assert!(
                relative,
                "{name:?} is absolute and would own the cue's level"
            );
        }
    }

    /// The periods are the guide's: 4, 2 or 1 bars, never faster.
    /// `measure` is in beats, so those are 16, 8 and 4.
    /// r[verify effects.measure]
    #[test]
    fn the_periods_are_whole_bars() {
        for (name, r) in family() {
            let m = canvas_of(&r).timing.measure;
            assert!(
                [4.0, 8.0, 16.0].contains(&m),
                "{name:?} loops every {} bars",
                m / 4.0
            );
        }
    }

    /// The head actually moves, and it moves *through the wall* rather
    /// than along a row of it.
    ///
    /// Four towers of three, Room 138's shape in miniature. At two
    /// moments a third of a lap apart the brightest fixture must be a
    /// different one — and over a whole lap the snake must have been
    /// brightest on every fixture at some point, which is what
    /// distinguishes a picture that crawls the grid from one that only
    /// slides across it.
    /// r[verify canvas.grid]
    #[test]
    fn the_snake_visits_the_whole_wall() {
        use ignition_proto::{Placement, Quat, Vec3};
        use ignition_rig::selection::{FixtureInfo, Rig};
        use ignition_show::{Show, expand_recipe};

        let mut heads = Vec::new();
        let mut chan = 1;
        for x in 0..4 {
            for z in 0..3 {
                heads.push(FixtureInfo {
                    chan,
                    placement: Some(Placement {
                        position: Vec3 {
                            x: f64::from(x),
                            y: 0.0,
                            z: f64::from(z),
                        },
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
                });
                chan += 1;
            }
        }
        let rig = Rig::new(heads);
        let show = Show::new(&[], &rig);

        // Pointed at the channels directly: this test is about the
        // picture crawling the grid, not about role binding, and a
        // `Roles` impl here would only be scaffolding.
        let mut recipe = family()["wall snake"].clone();
        recipe.target = ignition_rig::Selection::Chans((1..=12).collect());
        // The brightest fixture at a moment — the head.
        let head_at = |secs: f32| {
            expand_recipe(&recipe, &show, secs)
                .into_iter()
                .max_by(|a, b| a.value.value.total_cmp(&b.value.value))
                .map(|e| e.value.chan)
                .expect("the snake emits")
        };

        // `wall snake` loops every 16 beats; at the default 120 bpm that
        // is 8 seconds.
        let lap = 8.0;
        assert_ne!(
            head_at(0.0),
            head_at(lap / 3.0),
            "the head did not move in a third of a lap"
        );

        let mut visited = std::collections::BTreeSet::new();
        for i in 0..48_usize {
            visited.insert(head_at(lap * float_of(i) / 48.0));
        }
        assert!(
            visited.len() >= 8,
            "the head only ever reached {} of the twelve fixtures: {visited:?}",
            visited.len()
        );
    }

    /// A snake's tail is shorter than its path, or it is a solid bar
    /// going round and round.
    #[test]
    fn a_snakes_tail_leaves_the_wall_somewhere_dark() {
        for (name, r) in family() {
            if let Procedural::Snake { tail, .. } | Procedural::Rain { tail, .. } =
                canvas_of(&r).source
            {
                assert!(
                    (0.05..0.6).contains(&tail),
                    "{name:?} has a tail of {tail}, which is the whole wall"
                );
            }
        }
    }
}
