//! What the eight faders play when the app opens.
//!
//! Chosen so the surface is playable the moment it loads rather than
//! being eight empty slots: four looks, three movement effects, one
//! strobe. Every phaser here is slaved to the `Rate` master, so the one
//! slider on the right retimes the whole surface at once — the thing
//! this engine does that grandMA3's recipes cannot, because a phaser
//! here *is* a recipe.
//!
//! Absolute faders crossfade toward their look; `Delta` faders modulate
//! whatever is underneath without needing to know what it is. That is
//! why the chases can sit on top of a wash from another fader and not
//! fight it — see `ignition_core::programmer`.

use ignition_core::preset::Ref;
use ignition_core::selection::{Axis, Cmp, Dir, Order, Where};
use ignition_core::{Attribute, Recipe, RecipeApply, Selection, Speed, Timing, Vec3, Waveform};

/// A fader as the surface presents it: what it does, and what colour to
/// draw it. The colour is presentation and deliberately not part of
/// `ignition_core::Fader`.
pub struct FaderSpec {
    pub name: &'static str,
    pub css: &'static str,
    pub recipe: Recipe,
}

/// The ceiling wash, ordered left to right by real position.
///
/// Spatial rather than by channel number, so the chase runs across the
/// room even if the rig is re-patched — see `ignition_core::selection`.
fn ceiling_left_to_right() -> Selection {
    Selection::Order {
        of: Box::new(ceiling()),
        by: Order::Axis(Axis::X, Dir::Asc),
    }
}

/// The ceiling wash, ordered outward from centre-front.
fn ceiling_centre_out() -> Selection {
    Selection::Order {
        of: Box::new(ceiling()),
        by: Order::Distance {
            from: Vec3 {
                x: 0.0,
                y: -3.0,
                z: 2.7,
            },
            dir: Dir::Asc,
        },
    }
}

/// Everything tagged as a wash that is hung above head height — which
/// is the truss, and excludes the floor package.
fn ceiling() -> Selection {
    Selection::Where {
        of: Box::new(Selection::Tag("Luminaire_LED_Wash".into())),
        filter: Where::Half {
            axis: Axis::Z,
            cmp: Cmp::Gt,
            at: 2.0,
        },
    }
}

fn look(target: Selection, colour: &str, level: f32) -> Recipe {
    let mut recipe = Recipe::new(target.clone(), RecipeApply::Dimmer(level));
    recipe.steps[0]
        .apply
        .push(RecipeApply::Color(Ref::Named(colour.to_string())));
    recipe
}

/// A phaser on one attribute, slaved to the `Rate` master.
fn chase(
    target: Selection,
    attr: Attribute,
    base: f32,
    size: f32,
    shape: Waveform,
    spread: f32,
    measure: f32,
) -> Recipe {
    Recipe {
        target,
        // Relative, so a chase modulates whatever look is underneath
        // instead of replacing it.
        steps: shape.steps(attr, base, size, true),
        timing: Timing {
            speed: Speed::Master("Rate".into()),
            measure,
            phase_spread_deg: spread,
            ..Default::default()
        },
    }
}

pub fn defaults() -> Vec<FaderSpec> {
    vec![
        FaderSpec {
            name: "Warm",
            css: "#e8b878",
            recipe: look(ceiling(), "Warm White", 1.0),
        },
        FaderSpec {
            name: "Cool",
            css: "#9fc4e8",
            recipe: look(ceiling(), "Cool White", 1.0),
        },
        FaderSpec {
            name: "Back",
            css: "#5a48d8",
            recipe: look(Selection::Group("Back Wall Pars".into()), "Congo", 1.0),
        },
        FaderSpec {
            name: "Strips",
            css: "#d84898",
            recipe: look(Selection::Group("Strips All".into()), "Magenta", 1.0),
        },
        FaderSpec {
            name: "L→R",
            css: "#48c8d8",
            recipe: chase(
                ceiling_left_to_right(),
                Attribute::Dimmer,
                -0.5,
                0.5,
                Waveform::Sine,
                360.0,
                1.0,
            ),
        },
        FaderSpec {
            name: "Bloom",
            css: "#48d888",
            recipe: chase(
                ceiling_centre_out(),
                Attribute::Dimmer,
                -0.5,
                0.5,
                Waveform::Sine,
                360.0,
                2.0,
            ),
        },
        FaderSpec {
            name: "Swing",
            css: "#d8a848",
            // Pan only: paired with a Tilt phaser a quarter-cycle apart
            // this would be a circle, but one axis reads better as a
            // hand-playable fader.
            recipe: chase(
                Selection::Model("Moving Head".into()),
                Attribute::Pan,
                0.0,
                25.0,
                Waveform::Sine,
                180.0,
                4.0,
            ),
        },
        FaderSpec {
            name: "Strobe",
            css: "#ffffff",
            recipe: chase(
                Selection::Tag("Luminaire_LED_Wash".into()),
                Attribute::Dimmer,
                0.0,
                1.0,
                Waveform::Square,
                0.0,
                // A quarter of a beat: fast enough to read as a strobe
                // at any sane tempo, and still tied to the rate master.
                0.25,
            ),
        },
    ]
}
