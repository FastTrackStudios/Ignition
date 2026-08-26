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
    /// How far every effect swings, 0..=1.
    ///
    /// The control an operator holds for most of a night. Distinct from
    /// a master or a dimmer, and the distinction is the whole point: a
    /// master scales what the fixtures *output*, size scales how far an
    /// effect *swings*. Halving a master makes a chase dimmer overall;
    /// halving size makes it flatter while the look underneath stays
    /// exactly where it was. Conflating them makes an effect impossible
    /// to withdraw from a look without dimming the look too.
    ///
    /// At zero every effect is inert and whatever is beneath shows
    /// through unchanged — which is what makes this a *withdrawal*
    /// rather than a blackout.
    pub size: f32,
    /// A multiplier on every effect's rate, against its speed master.
    ///
    /// The tap master already sets the tempo; this is how one operator
    /// runs the rig at half or double it without a second recipe for
    /// every entry in the library.
    pub rate: f32,
    /// Per-role intensity masters, by role name.
    ///
    /// A busking operator's grip on a rig a cue list is otherwise
    /// driving: pull `Movers` down for a ballad without editing a cue.
    /// Scaling rather than limiting, per `r[groups.master.modes]` — a
    /// fixture at 50% under a master at 50% outputs 25%, which is what
    /// an operator expects from something that behaves like a fader.
    pub masters: std::collections::BTreeMap<String, f32>,
    /// A role played on its own, everything else pulled down.
    ///
    /// What a solo button does. Not a selection — selection is what the
    /// *next* palette hit lands on, and a solo has to change the output
    /// without changing what is armed, or hitting solo would silently
    /// redirect the operator's next move.
    pub solo: Option<String>,
    /// How far down the un-soloed rig goes. Not to zero by default: a
    /// solo that blacks the room reads as a fault, where one that leaves
    /// a floor under it reads as a decision.
    pub solo_floor: f32,
    /// Bumps fired by hand and still ringing, with the show time each
    /// started.
    ///
    /// Transient rather than a fader, because a flash key is an *event*.
    /// Parked on a fader it would need a release to arrive, and a flash
    /// whose release is dropped — a slipped hand, a lost MIDI note-off —
    /// leaves the rig stuck bright. A one-shot with its own start time
    /// cannot do that.
    flashes: Vec<(Recipe, f32)>,
}

impl Programmer {
    pub fn new() -> Self {
        Self {
            // Effects at full and no masters pulled: a desk that starts
            // with its controls somewhere other than neutral makes every
            // first look a surprise.
            size: 1.0,
            rate: 1.0,
            solo_floor: 0.15,
            ..Self::default()
        }
    }

    /// Puts a role on its own until cleared.
    pub fn solo(&mut self, role: &str) {
        self.solo = Some(role.to_string());
    }

    pub fn clear_solo(&mut self) {
        self.solo = None;
    }

    /// Fires a bump by hand.
    ///
    /// `now` is show time, so the envelope runs from this moment rather
    /// than from wherever the shared clock happens to be — the same rule
    /// the cue player applies to one-shots, and for the same reason: an
    /// envelope evaluated against a clock that started hours ago has
    /// already finished before it is seen.
    pub fn flash(&mut self, target: Selection, kind: crate::bump::Kind, now: f32) {
        self.flashes
            .push((crate::bump::bump(target, kind, 1.0), now));
        // A cap rather than unbounded growth: `retire_flashes` clears
        // finished ones every frame, so reaching this means something is
        // not calling it, and dropping the oldest is better than growing
        // for the rest of the night.
        while self.flashes.len() > 16 {
            self.flashes.remove(0);
        }
    }

    /// Drops bumps whose envelope has run out.
    pub fn retire_flashes(&mut self, show: &Show<'_>, now: f32) {
        self.flashes
            .retain(|(recipe, started)| recipe.timing.cycles(now - started, show.speeds) < 1.0);
    }

    /// Sets a role's intensity master, 0..=1.
    pub fn set_master(&mut self, role: &str, level: f32) {
        self.masters.insert(role.to_string(), level.clamp(0.0, 1.0));
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
            // Rate scales the clock the recipe is evaluated at, not its
            // measure — so it stretches an effect that is already
            // running rather than restarting it, which is what a fader
            // has to do to be usable while the show is on.
            let at = secs * self.rate.max(0.0);
            for emit in expand_recipe(recipe, show, at) {
                let key = (emit.value.chan, emit.value.attr);
                let under = base.get(&key).copied().unwrap_or(0.0);
                // Size and the fader's own level multiply. The fader
                // says how much of *this* effect; size says how much of
                // every effect at once.
                let weight = level * self.size.clamp(0.0, 1.0);
                let value = if emit.relative {
                    under + emit.value.value * weight
                } else {
                    under + (emit.value.value - under) * weight
                };
                base.insert(key, value);
            }
        }

        // Flashes land *before* the masters, so pulling a role down
        // quietens its flashes too — a master an operator has pulled
        // should hold whatever arrives, including their own hand.
        for (recipe, started) in &self.flashes {
            for emit in expand_recipe(recipe, show, secs - started) {
                let key = (emit.value.chan, emit.value.attr);
                let under = base.get(&key).copied().unwrap_or(0.0);
                let value = if emit.relative {
                    under + emit.value.value
                } else {
                    emit.value.value
                };
                base.insert(key, value);
            }
        }

        self.apply_masters(base, show);

        // The operator's hand wins over everything, including their own
        // faders and their own masters — pulling a master down and then
        // setting a level by hand should give the level they set.
        for (key, value) in &self.values {
            base.insert(key.clone(), *value);
        }
    }

    /// Scales intensity per role, and applies a solo.
    ///
    /// Dimmer only, deliberately. A master that scaled pan would drag
    /// every mover toward its home position as it came down, and a
    /// master that scaled colour would desaturate the rig — neither is
    /// what "quieter" means.
    fn apply_masters(&self, base: &mut HashMap<(ChanId, Attribute), f32>, show: &Show<'_>) {
        if self.masters.is_empty() && self.solo.is_none() {
            return;
        }
        // Resolve each named role once, not once per channel.
        let mut scale: HashMap<ChanId, f32> = HashMap::new();
        let note = |role: &str, factor: f32, scale: &mut HashMap<ChanId, f32>| {
            for chan in crate::selection::resolve_with(
                &Selection::Role(role.to_string()),
                show.groups,
                show.rig,
                show.roles,
            ) {
                // Lowest wins where a fixture plays two roles — a head
                // that is both Key and Wash should follow whichever of
                // them the operator pulled down, or a master would be
                // defeated by any other role the fixture happens to
                // belong to.
                let slot = scale.entry(chan).or_insert(1.0);
                *slot = slot.min(factor);
            }
        };

        for (role, level) in &self.masters {
            note(role, *level, &mut scale);
        }

        if let Some(solo) = &self.solo {
            let lit: std::collections::HashSet<ChanId> = crate::selection::resolve_with(
                &Selection::Role(solo.clone()),
                show.groups,
                show.rig,
                show.roles,
            )
            .into_iter()
            .collect();
            // Everything already carrying a dimmer that is *not* soloed
            // goes down to the floor. Taken from the base rather than
            // from the rig, so a fixture that was dark stays dark
            // instead of being lifted to the floor level.
            for (chan, _) in base.keys().filter(|(_, a)| *a == Attribute::Dimmer) {
                if !lit.contains(chan) {
                    let slot = scale.entry(*chan).or_insert(1.0);
                    *slot = slot.min(self.solo_floor.clamp(0.0, 1.0));
                }
            }
        }

        for ((chan, attr), value) in base.iter_mut() {
            if *attr != Attribute::Dimmer {
                continue;
            }
            if let Some(factor) = scale.get(chan) {
                *value *= *factor;
            }
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

    // ── live control ─────────────────────────────────────────────────

    /// A role master scales that role's fixtures and nothing else.
    #[test]
    fn a_master_scales_only_its_own_role() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.5);

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 1.0); // Key
        out.insert((9, Attribute::Dimmer), 1.0); // not Key
        p.apply_to(&mut out, &show, 0.0);

        assert!((out[&(1, Attribute::Dimmer)] - 0.5).abs() < 1e-6);
        assert!((out[&(9, Attribute::Dimmer)] - 1.0).abs() < 1e-6, "an unrelated fixture moved");
    }

    /// Scaling, not limiting. A fixture at half under a master at half
    /// is a quarter — which is what an operator expects from something
    /// that behaves like a fader, and is the distinction
    /// `r[groups.master.modes]` exists to pin.
    #[test]
    fn a_master_scales_rather_than_limits() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.5);

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 0.5);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.25).abs() < 1e-6);
    }

    /// Dimmer only. A master that scaled pan would drag every mover
    /// toward its home position on the way down.
    #[test]
    fn a_master_leaves_position_alone() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.set_master("Key", 0.25);

        let mut out = HashMap::new();
        out.insert((1, Attribute::Pan), 40.0);
        p.apply_to(&mut out, &show, 0.0);
        assert!((out[&(1, Attribute::Pan)] - 40.0).abs() < 1e-6);
    }

    /// Solo pulls everything else down to the floor, and leaves what is
    /// already dark dark — a solo that lifted unlit fixtures to the
    /// floor level would turn a blackout into a dim wash.
    #[test]
    fn solo_pulls_down_everything_else() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);
        let mut p = Programmer::new();
        p.solo("Key");

        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 1.0); // Key — stays
        out.insert((9, Attribute::Dimmer), 1.0); // other — floored
        p.apply_to(&mut out, &show, 0.0);

        assert!((out[&(1, Attribute::Dimmer)] - 1.0).abs() < 1e-6);
        assert!((out[&(9, Attribute::Dimmer)] - p.solo_floor).abs() < 1e-6);
        assert!(p.solo_floor > 0.0, "a solo that blacks the room reads as a fault");
    }

    /// Size withdraws an effect without touching what is under it. This
    /// is the difference from a master, and the reason both exist.
    #[test]
    fn size_flattens_the_effect_and_leaves_the_look() {
        let groups = groups();
        let venue = roles();
        let show = show_with_roles(&groups, &venue);

        let chase = |chan: ChanId| Recipe {
            target: Selection::Chans(vec![chan]),
            steps: vec![Step::new(vec![RecipeApply::Delta(vec![(
                Attribute::Dimmer,
                -0.5,
            )])])],
            timing: Timing::default(),
            tricks: Vec::new(),
        };

        let mut p = Programmer::new();
        p.set_fader(
            0,
            Fader {
                name: "Chase".into(),
                recipe: Some(chase(1)),
                level: 1.0,
            },
        );

        let run = |p: &Programmer| {
            let mut out = HashMap::new();
            out.insert((1, Attribute::Dimmer), 0.8);
            p.apply_to(&mut out, &show, 0.0);
            out[&(1, Attribute::Dimmer)]
        };

        assert!((run(&p) - 0.3).abs() < 1e-6, "full size: 0.8 - 0.5");
        p.size = 0.5;
        assert!((run(&p) - 0.55).abs() < 1e-6, "half size: 0.8 - 0.25");
        p.size = 0.0;
        assert!(
            (run(&p) - 0.8).abs() < 1e-6,
            "at zero the look underneath must show through untouched"
        );
    }

    fn roles() -> Bound {
        let mut bound = Bound::default();
        bound
            .0
            .insert("Key".into(), Selection::Chans(vec![1, 2, 3]));
        bound
    }

    #[derive(Default)]
    struct Bound(std::collections::BTreeMap<String, Selection>);

    impl crate::selection::Roles for Bound {
        fn role(&self, name: &str) -> Option<&Selection> {
            self.0.get(name)
        }
    }

    fn show_with_roles<'a>(groups: &'a [Group], roles: &'a Bound) -> Show<'a> {
        Show {
            groups,
            palettes: crate::Palettes::EMPTY,
            rig: &EMPTY_RIG,
            speeds: &crate::recipe::NO_SPEEDS,
            roles,
        }
    }
}
