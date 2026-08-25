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

/// A reference to a pooled preset, or the value written out in place.
///
/// This is what turns presets from a type into a *pool*: a recipe can say
/// `"Deep Blue"` and mean the palette entry, so re-gelling a show is one
/// edit instead of forty. Writing the value inline stays legal for the
/// one-off that does not deserve a name — and, because the two forms are
/// distinguished structurally (`#[serde(untagged)]`: a JSON string is a
/// name, an object is a value), every show file written before the pool
/// existed still parses unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Ref<T> {
    Named(String),
    Inline(T),
}

/// The venue's preset pools — an Eos "palette" or grandMA3 "preset pool".
///
/// Palettes belong to the *venue*, not to a show: "Drums" is a real place
/// in this room and "House Blue" is this rig's blue, so every show in the
/// building should mean the same thing by them. That is also why an
/// unknown name resolves to `None` and the recipe is skipped rather than
/// erroring — the same tolerance `group::find` already has for a group
/// name a venue does not carry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Palettes {
    #[serde(default)]
    pub colors: Vec<ColorPreset>,
    #[serde(default)]
    pub focus: Vec<FocusPointPreset>,
}

impl Palettes {
    /// A shared empty pool, so `Show::new` can hand out a reference
    /// without every caller having to own one.
    pub const EMPTY: &'static Palettes = &Palettes {
        colors: Vec::new(),
        focus: Vec::new(),
    };

    pub fn color(&self, name: &str) -> Option<&ColorPreset> {
        self.colors.iter().find(|c| c.name == name)
    }

    pub fn focus(&self, name: &str) -> Option<&FocusPointPreset> {
        self.focus.iter().find(|f| f.name == name)
    }

    /// Resolves a colour reference against the pool.
    pub fn resolve_color(&self, r: &Ref<ColorPreset>) -> Option<ColorPreset> {
        match r {
            Ref::Named(name) => self.color(name).cloned(),
            Ref::Inline(c) => Some(c.clone()),
        }
    }

    /// Resolves a focus reference against the pool. The inline form is a
    /// bare point rather than a named `FocusPointPreset`, because at the
    /// point of use what a recipe wants is the location, not the label.
    pub fn resolve_focus(&self, r: &Ref<Vec3>) -> Option<Vec3> {
        match r {
            Ref::Named(name) => self.focus(name).map(|f| f.target),
            Ref::Inline(v) => Some(*v),
        }
    }
}
