//! Reusable, named attribute bundles — grandMA3/Eos "Presets" (Color
//! Preset, Focus/Position Preset, ...). A preset stores *what*, a `Group`
//! (`group.rs`) stores *who*; `recipe.rs::Recipe` is what pairs the two
//! together into concrete cue values. Only the two preset types the
//! operator asked to start with are modelled here — more (Gobo, Beam,
//! Shapers) are additive, not a redesign, when they're wanted.

use ignition_proto::Vec3;
use serde::{Deserialize, Serialize};

/// An RGB colour to apply to every fixture in a recipe's target group.
/// Deliberately just RGB, not per-fixture White/Amber/UV/Lime channels —
/// `recipe.rs::expand_recipe` only ever emits `ColorAdd{Red/Green/Blue}`;
/// a fixture with no Blue channel (or no colour channel at all) simply has
/// that `CueValue` skipped at DMX-encode time
/// (`ignition_viz::show::apply_cue_output`), the same tolerance the rest
/// of this project already has for a fixture with a smaller footprint
/// than a cue targets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorPreset {
    pub name: String,
    pub red: f32,
    pub green: f32,
    pub blue: f32,
}

/// A real XYZ location in the room — `recipe.rs::expand_recipe` resolves
/// this into real Pan/Tilt values per fixture via `focus.rs`'s aim-at-point
/// math, using each fixture's own real hung position/orientation. This is
/// the preset type real 3D venue geometry (unique to this project among
/// budget-console-class tools) makes possible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FocusPointPreset {
    pub name: String,
    pub target: Vec3,
}
