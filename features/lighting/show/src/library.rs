//! A set's library: the colours, palettes, effects, looks, focus points,
//! beam presets and trick presets every song of a set shares, and what a
//! song overrides.
//!
//! Three layers, later over earlier, entry by entry and by name:
//!
//! 1. the **profile** (`data/profiles/<name>.ig-profile`): what ships;
//! 2. the **set's library** (`data/library/<set>.json`, named by the
//!    `.ig-show`'s `library` key): what this run of songs shares;
//! 3. the **song's own** (`library` in the song's file): its overrides.
//!
//! A **palette** is a named set of *slots* (`Primary`, `Secondary`,
//! `Accent`…), each a colour by name or by value. The palette in use (the
//! song's `palette`, else the set's) puts its slots among the colours
//! under the slot's name, so a cue that says `{"Color": "Primary"}` is
//! re-skinned by naming another palette and by nothing else.
//!
//! This module is the one reading of that file: the programming actions
//! write it and the renderer lays it over what it plays from
//! ([`Library::lay_over`]).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::recipe::{Recipe, RecipeRef};
use crate::tricks::{Fan, Trick};
use crate::{ColorPreset, ColorSplit, Distribute, Palettes, Ref, Vec3};

/// The kinds of thing a library holds, as the actions name them.
pub const KINDS: [&str; 8] = [
    "colors", "palettes", "splits", "effects", "looks", "focus", "beams", "tricks",
];

/// What every entry carries so a library stays navigable.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// A path such as `Warm/Sunsets`; empty: the top.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

/// A colour: linear RGB, 0 to 1.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    #[serde(flatten)]
    pub meta: Meta,
}

/// A slot of a palette: a colour by name, or a value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Slot {
    Named(String),
    Value { red: f32, green: f32, blue: f32 },
}

/// A colour palette: named slots a show's cues refer to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Palette {
    pub slots: BTreeMap<String, Slot>,
    #[serde(flatten)]
    pub meta: Meta,
}

/// A multi-colour entry: several colours and how they meet a selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Split {
    pub colors: Vec<String>,
    pub distribute: Distribute,
    #[serde(flatten)]
    pub meta: Meta,
}

/// An effect: a recipe (grandMA3: a phaser) and what it is for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effect {
    pub recipe: Recipe,
    /// intensity, movement, colour, beam, strip, matrix or one-shot.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family: String,
    #[serde(flatten)]
    pub meta: Meta,
}

/// A look: a static scene against roles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LookEntry {
    pub recipes: Vec<RecipeRef>,
    #[serde(default)]
    pub kind: crate::profile::LookKind,
    #[serde(flatten)]
    pub meta: Meta,
}

/// A position palette entry: a point in the room, in metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Focus {
    pub target: Vec3,
    #[serde(flatten)]
    pub meta: Meta,
}

/// A beam or gobo palette entry: attribute values set together (zoom,
/// iris, focus, a gobo slot).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Beam {
    pub values: Vec<(crate::Attribute, f32)>,
    #[serde(flatten)]
    pub meta: Meta,
}

/// A from–to pair across the shaped order, written `{"from": 0, "to": 360}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FromTo {
    pub from: f32,
    pub to: f32,
}

/// A trick preset (grandMA3: a MAtricks pool object): how a selection is
/// cut into units and how a value spreads across them.
///
/// `tricks` cut the selection (block, group, wings, shuffle, shift,
/// reverse, mirror, per axis with `OnAxis`); the from–to pairs then land
/// on the *units* in their shaped order. Applied to a recipe by
/// [`TrickPreset::apply`]; applied to a cue, `delay` and `fade` become its
/// fan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TrickPreset {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tricks: Vec<Trick>,
    /// Phase, degrees, along X (the selection's order).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<FromTo>,
    /// Phase along the grid's Y (across the trusses) and Z (height).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_y: Option<FromTo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_z: Option<FromTo>,
    /// Rate multiplier, first unit to last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<FromTo>,
    /// Delay and fade, in beats, first unit to last: a static look that
    /// wipes. They are a cue's (`Cue::fan`), not a recipe's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<FromTo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fade: Option<FromTo>,
    /// Which way the effect plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<crate::step::Play>,
    /// The selection grid set by hand: rows of channels, X along a row.
    /// None: the grid is read from where the fixtures hang.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<Vec<Vec<crate::ChanId>>>,
    #[serde(flatten)]
    pub meta: Meta,
}

impl TrickPreset {
    /// This preset on a recipe: its tricks replace the recipe's, its
    /// phase, speed and direction the recipe's timing, and a hand-made
    /// grid wraps the target. What the preset leaves unsaid is left.
    pub fn apply(&self, recipe: &mut Recipe) {
        recipe.tricks.clone_from(&self.tricks);
        recipe.tricks_ref = None;
        if let Some(p) = self.phase {
            recipe.timing.phase_offset_deg = p.from;
            recipe.timing.phase_spread_deg = p.to - p.from;
        }
        if let Some(p) = self.phase_y {
            recipe.timing.phase_spread_y_deg = p.to - p.from;
        }
        if let Some(p) = self.phase_z {
            recipe.timing.phase_spread_z_deg = p.to - p.from;
        }
        if let Some(s) = self.speed {
            recipe.timing.speed_from = s.from;
            recipe.timing.speed_to = s.to;
        }
        if let Some(direction) = self.direction {
            recipe.timing.direction = direction;
        }
        if let Some(rows) = &self.grid {
            let of = match std::mem::replace(
                &mut recipe.target,
                crate::Selection::Chans(Vec::new()),
            ) {
                crate::Selection::Layout { of, .. } => of,
                other => Box::new(other),
            };
            recipe.target = crate::Selection::Layout {
                of,
                rows: rows.clone(),
            };
        }
    }

    /// The cue fan this preset asks for, when it spreads delay or fade.
    #[must_use]
    pub fn cue_fan(&self) -> Option<crate::cue::CueFan> {
        if self.delay.is_none() && self.fade.is_none() {
            return None;
        }
        let fan = |v: Option<FromTo>| {
            v.map_or_else(Fan::default, |v| Fan {
                from: v.from,
                to: v.to,
                ..Fan::default()
            })
        };
        Some(crate::cue::CueFan {
            delay: fan(self.delay),
            fade: fan(self.fade),
        })
    }
}

/// One layer of a library: a set's file, or a song's overrides.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Library {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The palette in use, when this layer says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, Color>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub palettes: BTreeMap<String, Palette>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub splits: BTreeMap<String, Split>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub effects: BTreeMap<String, Effect>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub looks: BTreeMap<String, LookEntry>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub focus: BTreeMap<String, Focus>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub beams: BTreeMap<String, Beam>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tricks: BTreeMap<String, TrickPreset>,
}

/// The slot that stands in for one a palette lacks: `Accent 2` → `Accent`
/// (its name without a trailing number), else `Primary`; `None` when the
/// palette has neither.
#[must_use]
pub fn stand_in(slot: &str, has: impl Fn(&str) -> bool) -> Option<String> {
    let base = slot.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end();
    if base != slot && has(base) {
        return Some(base.to_owned());
    }
    has("Primary").then(|| "Primary".to_owned())
}

/// The pools a show plays from, as the renderer holds them.
pub struct Pools<'a> {
    pub palettes: &'a mut Palettes,
    pub effects: &'a mut BTreeMap<String, Recipe>,
    pub looks: &'a mut BTreeMap<String, crate::profile::Look>,
    pub tricks: &'a mut BTreeMap<String, Vec<Trick>>,
}

impl Library {
    /// This layer over another: its entries win by name, and so does its
    /// palette in use when it names one.
    #[must_use]
    pub fn over(mut self, under: &Self) -> Self {
        fn fill<T: Clone>(top: &mut BTreeMap<String, T>, under: &BTreeMap<String, T>) {
            for (name, entry) in under {
                top.entry(name.clone()).or_insert_with(|| entry.clone());
            }
        }
        fill(&mut self.colors, &under.colors);
        fill(&mut self.palettes, &under.palettes);
        fill(&mut self.splits, &under.splits);
        fill(&mut self.effects, &under.effects);
        fill(&mut self.looks, &under.looks);
        fill(&mut self.focus, &under.focus);
        fill(&mut self.beams, &under.beams);
        fill(&mut self.tricks, &under.tricks);
        if self.palette.is_none() {
            self.palette.clone_from(&under.palette);
        }
        self
    }

    /// The slots of the palette in use, as `(slot, red, green, blue)`. A
    /// slot naming a colour is looked up here first, then in `known`.
    ///
    /// A slot another palette of the library has and this one lacks
    /// stands in from this one, so a show written against one palette
    /// still resolves under any other: `Accent 2` takes `Accent`, a slot
    /// with no numbered base takes `Primary` (see [`stand_in`]).
    #[must_use]
    pub fn slots(&self, known: &Palettes) -> Vec<(String, f32, f32, f32)> {
        let Some(palette) = self.palette.as_ref().and_then(|p| self.palettes.get(p)) else {
            return Vec::new();
        };
        let rgb = |value: &Slot| match value {
            Slot::Value { red, green, blue } => Some((*red, *green, *blue)),
            Slot::Named(name) => self
                .colors
                .get(name)
                .map(|c| (c.red, c.green, c.blue))
                .or_else(|| known.color(name).map(|c| (c.red, c.green, c.blue))),
        };
        let mut out: Vec<(String, f32, f32, f32)> = palette
            .slots
            .iter()
            .filter_map(|(slot, value)| rgb(value).map(|(r, g, b)| (slot.clone(), r, g, b)))
            .collect();
        let every: std::collections::BTreeSet<&String> =
            self.palettes.values().flat_map(|p| p.slots.keys()).collect();
        for slot in every {
            if palette.slots.contains_key(slot) {
                continue;
            }
            let from = stand_in(slot, |s| palette.slots.contains_key(s));
            if let Some((r, g, b)) = from.and_then(|s| palette.slots.get(&s)).and_then(rgb) {
                out.push((slot.clone(), r, g, b));
            }
        }
        out
    }

    /// Lays this (already layered) library over what a show plays from:
    /// its colours, the palette in use's slots as colours, its splits and
    /// focus points into the palettes; its effects, looks and trick
    /// chains into theirs. Entries here replace same-named ones.
    pub fn lay_over(&self, pools: &mut Pools<'_>) {
        let put = |name: &str, red: f32, green: f32, blue: f32, palettes: &mut Palettes| {
            let preset = ColorPreset {
                name: name.to_owned(),
                red,
                green,
                blue,
                ..ColorPreset::default()
            };
            match palettes.colors.iter_mut().find(|c| c.name == name) {
                Some(slot) => *slot = preset,
                None => palettes.colors.push(preset),
            }
        };
        for (name, c) in &self.colors {
            put(name, c.red, c.green, c.blue, pools.palettes);
        }
        for (slot, red, green, blue) in self.slots(pools.palettes) {
            put(&slot, red, green, blue, pools.palettes);
        }
        for (name, split) in &self.splits {
            let entry = ColorSplit {
                name: name.clone(),
                colors: split.colors.iter().cloned().map(Ref::Named).collect(),
                distribute: split.distribute,
            };
            match pools.palettes.splits.iter_mut().find(|s| &s.name == name) {
                Some(slot) => *slot = entry,
                None => pools.palettes.splits.push(entry),
            }
        }
        for (name, focus) in &self.focus {
            pools.palettes.set_focus(name, focus.target);
        }
        for (name, effect) in &self.effects {
            pools.effects.insert(name.clone(), effect.recipe.clone());
        }
        for (name, look) in &self.looks {
            pools.looks.insert(
                name.clone(),
                crate::profile::Look {
                    kind: look.kind,
                    about: look.meta.about.clone(),
                    recipes: look.recipes.clone(),
                },
            );
        }
        for (name, preset) in &self.tricks {
            pools.tricks.insert(name.clone(), preset.tricks.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_song_layer_wins_and_the_palette_in_use_becomes_colours() {
        let set: Library = serde_json::from_value(serde_json::json!({
            "palette": "Sunset",
            "colors": {"Ember": {"red": 1.0, "green": 0.3, "blue": 0.0, "tags": ["warm"]}},
            "palettes": {
                "Sunset": {"slots": {"Primary": "Ember", "Accent": {"red": 0.0, "green": 0.0, "blue": 1.0}}},
                "Ice": {"slots": {"Primary": {"red": 0.2, "green": 0.6, "blue": 1.0}}}
            }
        }))
        .unwrap_or_default();
        let song = Library {
            palette: Some("Ice".into()),
            ..Library::default()
        };
        let mut palettes = Palettes::default();
        let (mut effects, mut looks, mut tricks) =
            (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
        set.lay_over(&mut Pools {
            palettes: &mut palettes,
            effects: &mut effects,
            looks: &mut looks,
            tricks: &mut tricks,
        });
        assert_eq!(palettes.color("Primary").map(|c| c.green), Some(0.3));
        song.over(&set).lay_over(&mut Pools {
            palettes: &mut palettes,
            effects: &mut effects,
            looks: &mut looks,
            tricks: &mut tricks,
        });
        assert_eq!(palettes.color("Primary").map(|c| c.green), Some(0.6));
        assert_eq!(palettes.color("Ember").map(|c| c.red), Some(1.0));
    }

    #[test]
    fn a_trick_preset_sets_tricks_phase_and_speed_on_a_recipe() {
        let preset: TrickPreset = serde_json::from_value(serde_json::json!({
            "tricks": [{"Wings": 2}],
            "phase": {"from": 0.0, "to": 360.0},
            "speed": {"from": 1.0, "to": 2.0}
        }))
        .unwrap_or_default();
        let mut recipe = Recipe::default();
        preset.apply(&mut recipe);
        assert_eq!(recipe.tricks, vec![Trick::Wings(2)]);
        assert!((recipe.timing.phase_spread_deg - 360.0).abs() < 1e-6);
        assert!((recipe.timing.rate_at(11, 12) - 2.0).abs() < 1e-6);
        assert!((recipe.timing.rate_at(0, 12) - 1.0).abs() < 1e-6);
    }
}
