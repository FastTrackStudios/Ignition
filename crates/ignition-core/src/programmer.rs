//! The live layer — what the operator is holding right now.
//!
//! Busking is the primary way this desk is played: pick a group, hit a
//! colour, push a fader. Cue playback sits *underneath* that, not beside
//! it — a cue stack is where a busked look gets recorded to, and what
//! fills in around whatever the operator is not currently touching.
//!
//! Mechanically the programmer is two more layers on top of the cue
//! cascade (`cue.rs`), in the same first-one-wins order:
//!
//! ```text
//!   programmer direct values     <- hit a palette with a group selected
//!   programmer faders            <- the eight assignable recipes
//!   ---- everything below is the cue player ----
//!   direct values on the cue
//!   recipes on the cue
//! ```
//!
//! Which is why the cascade was worth building properly: busking did not
//! need a new engine, only two more layers and something to own them.

use crate::cue::CueValue;
use crate::recipe::{Recipe, RecipeApply, Show, expand_recipe};
use crate::selection::{Selection, resolve};
use ignition_proto::{Attribute, ChanId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// How many assignable faders the surface has.
///
/// Eight because that is what fits under a hand, and because every
/// hardware surface this will ever talk to is a multiple of it.
pub const FADERS: usize = 8;

/// One assignable fader.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Fader {
    pub name: String,
    /// What this fader plays. `None` is an unassigned fader, which is
    /// not the same as one at zero.
    pub recipe: Option<Recipe>,
    /// 0.0–1.0.
    pub level: f32,
}

/// The live programmer.
#[derive(Debug, Clone, Default)]
pub struct Programmer {
    /// What the operator currently has selected. Palette hits apply to
    /// this; with nothing selected they do nothing, the same as on a
    /// real desk.
    pub selection: Option<Selection>,
    /// Values the operator set by hand. Top layer — these beat both the
    /// faders and everything the cue stack is doing.
    values: HashMap<(ChanId, Attribute), f32>,
    pub faders: [Fader; FADERS],
}

impl Programmer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn select(&mut self, selection: Selection) {
        self.selection = Some(selection);
    }

    pub fn deselect(&mut self) {
        self.selection = None;
    }

    /// Everything the operator has set by hand, as cue values — this is
    /// what "record" writes into a cue.
    pub fn captured(&self) -> Vec<CueValue> {
        let mut out: Vec<CueValue> = self
            .values
            .iter()
            .map(|((chan, attr), value)| CueValue {
                chan: *chan,
                attr: attr.clone(),
                value: *value,
            })
            .collect();
        // Stable order so a recorded cue does not churn in git purely
        // because a HashMap iterated differently.
        out.sort_by(|a, b| {
            (a.chan, format!("{:?}", a.attr)).cmp(&(b.chan, format!("{:?}", b.attr)))
        });
        out
    }

    /// The recipes currently on faders, for recording alongside
    /// `captured()`. A fader at zero is still recorded — an operator who
    /// parks a fader down and records expects it back where they left
    /// it.
    pub fn assigned(&self) -> Vec<Recipe> {
        self.faders
            .iter()
            .filter_map(|f| f.recipe.clone())
            .collect()
    }

    /// Clears everything the operator set by hand, leaving the faders.
    ///
    /// Two separate verbs because they are two separate mistakes: "undo
    /// the colour I just hit" and "put the effects away" are never the
    /// same intent in the middle of a song.
    pub fn clear_values(&mut self) {
        self.values.clear();
    }

    pub fn clear_faders(&mut self) {
        self.faders = Default::default();
    }

    /// Applies a palette (or anything else a recipe can express) to the
    /// current selection, as direct values.
    ///
    /// Resolved immediately rather than stored as a recipe, because this
    /// is the operator's hand on the desk: what they see now is what
    /// they get, and a template that re-resolved under them mid-song
    /// would be a surprise, not a feature.
    pub fn apply(&mut self, apply: RecipeApply, show: &Show<'_>) {
        let Some(selection) = &self.selection else {
            return;
        };
        let recipe = Recipe::new(selection.clone(), apply);
        for emit in expand_recipe(&recipe, show, 0.0) {
            let key = (emit.value.chan, emit.value.attr);
            if emit.relative {
                *self.values.entry(key).or_insert(0.0) += emit.value.value;
            } else {
                self.values.insert(key, emit.value.value);
            }
        }
    }

    /// Releases the current selection's hold on one attribute family —
    /// how an operator takes their hand off without clearing the whole
    /// programmer.
    pub fn release(&mut self, show: &Show<'_>) {
        let Some(selection) = &self.selection else {
            return;
        };
        let chans = resolve(selection, show.groups, show.rig);
        self.values.retain(|(chan, _), _| !chans.contains(chan));
    }

    pub fn set_fader(&mut self, index: usize, fader: Fader) {
        if let Some(slot) = self.faders.get_mut(index) {
            *slot = fader;
        }
    }

    pub fn set_level(&mut self, index: usize, level: f32) {
        if let Some(slot) = self.faders.get_mut(index) {
            slot.level = level.clamp(0.0, 1.0);
        }
    }

    /// Folds the programmer's layers onto whatever the cue stack
    /// produced.
    ///
    /// A fader's level is a **crossfade weight**, not a multiplier. That
    /// one choice makes absolute and relative recipes behave the way an
    /// operator expects from the same control: pushing a colour fader up
    /// fades that colour in over what is underneath, and pushing a
    /// `Delta` pulse up fades the modulation in. A multiplier would
    /// dim the colour toward black instead, which is not what the
    /// control looks like it does.
    pub fn apply_to(
        &self,
        base: &mut HashMap<(ChanId, Attribute), f32>,
        show: &Show<'_>,
        secs: f32,
    ) {
        for fader in &self.faders {
            let (Some(recipe), level) = (&fader.recipe, fader.level) else {
                continue;
            };
            if level <= 0.0 {
                continue;
            }
            for emit in expand_recipe(recipe, show, secs) {
                let key = (emit.value.chan, emit.value.attr);
                let under = base.get(&key).copied().unwrap_or(0.0);
                let value = if emit.relative {
                    under + emit.value.value * level
                } else {
                    under + (emit.value.value - under) * level
                };
                base.insert(key, value);
            }
        }
        // The operator's hand wins over everything, including their own
        // faders.
        for (key, value) in &self.values {
            base.insert(key.clone(), *value);
        }
    }

    /// Whether anything is being held. Drives the "clear" affordance —
    /// a desk that always looks armed teaches operators to ignore it.
    pub fn is_active(&self) -> bool {
        !self.values.is_empty() || self.faders.iter().any(|f| f.level > 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::Group;
    use crate::preset::{ColorPreset, Ref};
    use crate::selection::EMPTY_RIG;
    use crate::step::{Speed, Step, Timing};

    fn groups() -> Vec<Group> {
        vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3],
        }]
    }

    fn show(groups: &[Group]) -> Show<'_> {
        Show::new(groups, &EMPTY_RIG)
    }

    fn base() -> HashMap<(ChanId, Attribute), f32> {
        HashMap::new()
    }

    #[test]
    fn a_palette_hit_with_nothing_selected_does_nothing() {
        let groups = groups();
        let mut p = Programmer::new();
        p.apply(RecipeApply::Dimmer(1.0), &show(&groups));
        assert!(p.captured().is_empty());
    }

    #[test]
    fn a_palette_hit_applies_to_the_selection() {
        let groups = groups();
        let mut p = Programmer::new();
        p.select(Selection::Group("Pars".into()));
        p.apply(RecipeApply::Dimmer(0.7), &show(&groups));
        let captured = p.captured();
        assert_eq!(captured.len(), 3);
        assert!(captured.iter().all(|v| v.value == 0.7));
    }

    #[test]
    fn the_operators_hand_beats_the_cue_stack() {
        let groups = groups();
        let mut p = Programmer::new();
        p.select(Selection::Chans(vec![1]));
        p.apply(RecipeApply::Dimmer(0.2), &show(&groups));

        let mut out = base();
        out.insert((1, Attribute::Dimmer), 1.0); // the cue says full
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.2);
    }

    #[test]
    fn releasing_drops_only_the_selections_hold() {
        let groups = groups();
        let mut p = Programmer::new();
        p.select(Selection::Group("Pars".into()));
        p.apply(RecipeApply::Dimmer(1.0), &show(&groups));
        p.select(Selection::Chans(vec![2]));
        p.release(&show(&groups));
        let held: Vec<ChanId> = p.captured().iter().map(|v| v.chan).collect();
        assert_eq!(held, vec![1, 3]);
    }

    fn colour_fader(level: f32) -> Fader {
        Fader {
            name: "Red".into(),
            recipe: Some(Recipe::new(
                Selection::Chans(vec![1]),
                RecipeApply::Color(Ref::Inline(ColorPreset {
                    name: "Red".into(),
                    red: 1.0,
                    green: 0.0,
                    blue: 0.0,
                })),
            )),
            level,
        }
    }

    /// The design decision worth pinning: level crossfades toward the
    /// recipe rather than scaling it. At half, a red fader over a blue
    /// wash reads half-way to red — not half-brightness red.
    #[test]
    fn a_fader_crossfades_toward_its_recipe() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, colour_fader(0.5));

        let mut out = base();
        let red = Attribute::ColorAdd {
            channel: ignition_proto::ColorChannel::Red,
        };
        out.insert((1, red.clone()), 0.0);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert!((out[&(1, red)] - 0.5).abs() < 0.001);
    }

    #[test]
    fn a_fader_at_zero_contributes_nothing() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(0, colour_fader(0.0));
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 0.4);
        p.apply_to(&mut out, &show(&groups), 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.4);
    }

    /// A relative fader adds rather than crossfading, so a pulse
    /// modulates the colour underneath instead of replacing it.
    #[test]
    fn a_relative_fader_modulates_what_is_underneath() {
        let groups = groups();
        let mut p = Programmer::new();
        p.set_fader(
            0,
            Fader {
                name: "Pulse".into(),
                recipe: Some(Recipe {
                    target: Selection::Chans(vec![1]),
                    steps: vec![Step::new(vec![RecipeApply::Delta(vec![(
                        Attribute::Dimmer,
                        -0.4,
                    )])])],
                    timing: Timing::default(),
                    tricks: Vec::new(),
                }),
                level: 0.5,
            },
        );
        let mut out = base();
        out.insert((1, Attribute::Dimmer), 1.0);
        p.apply_to(&mut out, &show(&groups), 0.0);
        // -0.4 at half weight.
        assert!((out[&(1, Attribute::Dimmer)] - 0.8).abs() < 0.001);
    }

    /// One rate source retiming every fader at once is the thing this
    /// design has that grandMA3 does not — a speed master drives *every*
    /// recipe here, because a phaser is a recipe.
    #[test]
    fn one_speed_master_drives_every_fader() {
        let groups = groups();
        let masters = crate::step::SpeedMasters::from([("Rate".to_string(), 120.0)]);
        let show = Show {
            groups: &groups,
            palettes: crate::preset::Palettes::EMPTY,
            rig: &EMPTY_RIG,
            speeds: &masters,
            roles: &crate::recipe::NO_ROLES,
        };
        let chase = |chan: ChanId| Recipe {
            target: Selection::Chans(vec![chan]),
            steps: vec![
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, 0.0)])]),
                Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, 1.0)])]),
            ],
            timing: Timing {
                speed: Speed::Master("Rate".into()),
                ..Default::default()
            },
            tricks: Vec::new(),
        };
        let mut p = Programmer::new();
        for (i, chan) in [1, 2].into_iter().enumerate() {
            p.set_fader(
                i,
                Fader {
                    name: "Chase".into(),
                    recipe: Some(chase(chan)),
                    level: 1.0,
                },
            );
        }

        // 120 BPM = 2 cycles/sec: both faders are on step 0 early in a
        // cycle and step 1 late, together, from one master.
        let mut early = base();
        p.apply_to(&mut early, &show, 0.05);
        let mut late = base();
        p.apply_to(&mut late, &show, 0.35);
        assert_eq!(
            early[&(1, Attribute::Dimmer)],
            early[&(2, Attribute::Dimmer)]
        );
        assert_ne!(
            early[&(1, Attribute::Dimmer)],
            late[&(1, Attribute::Dimmer)]
        );
    }
}
