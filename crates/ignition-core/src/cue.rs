//! A fixture-agnostic tracking cue-list engine — the "programming a light
//! show" layer this project has been missing: `dmx.rs` (in `ignition-viz`)
//! can only *read* live DMX from an external console; nothing before this
//! could originate a show of its own. Deliberately has no I/O, no DMX byte
//! encoding, and no fixture/channel-map knowledge — it operates purely in
//! terms of `(ChanId, Attribute) -> f32` targets and fade timing, the same
//! way `ignition-proto`'s `Attribute` already treats a fixture's channel
//! layout as an output-time concern, not something upstream code touches.
//! `ignition-viz::show` is the bridge that turns this engine's output into
//! real DMX bytes for the live visualizer to render.
//!
//! Values are normalized the same way `dmx.rs::ResolvedAttributes` already
//! reads them back out: 0.0-1.0 for `Dimmer`/`ColorAdd`/`Strobe`/`Zoom`/
//! `Focus`/`Iris`, degrees for `Pan`/`Tilt` (-270..270 and -135..135 to
//! match `dmx.rs`'s own byte<->degree formulas, which this module's
//! counterpart in `ignition-viz::show` inverts when encoding to bytes).
//!
//! **Tracking** cue-list semantics, matching how a real console (Eos,
//! grandMA) behaves by default: a cue only needs to list what *changes* —
//! any `(chan, attr)` a cue doesn't mention holds wherever the previous cue
//! left it, rather than snapping back to some default. This is what makes
//! programming a real show practical (a 40-cue song doesn't need every
//! fixture re-stated in every cue, just the deltas), and is why `target`
//! below is cumulative across `go()` calls rather than reset per cue.

use crate::recipe::{Recipe, Show, expand_recipe};
use ignition_proto::{Attribute, ChanId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// One fixture attribute's value within a cue — `value`'s unit depends on
/// `attr` (see the module doc): 0.0-1.0 for level-like attributes, degrees
/// for `Pan`/`Tilt`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CueValue {
    pub chan: ChanId,
    pub attr: Attribute,
    pub value: f32,
}

/// One step of a show. `fade_secs` is how long the *move into* this cue
/// takes (its own fade time, not the previous cue's) — matches Eos/grandMA
/// convention where a cue's fade time describes arriving at it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub name: String,
    #[serde(default)]
    pub fade_secs: f32,
    /// **Layer 1** of the cascade — direct values on the cue. Hand
    /// tweaks and recorded output. Always beat a recipe on the same cue,
    /// which is what makes "override one fixture out of the recipe" a
    /// defined operation rather than a merge.
    #[serde(default)]
    pub values: Vec<CueValue>,
    /// **Layer 2** — recipes on the cue, resolved at output time rather
    /// than flattened at load. Storing the template instead of its
    /// output is the whole point: a recipe still knows it targets a
    /// *group*, so adding a fixture to that group changes what the cue
    /// covers with no re-authoring.
    #[serde(default)]
    pub recipes: Vec<Recipe>,
    /// Does not track from its predecessor — everything this cue does
    /// not set goes out rather than holding. Required before song
    /// sections can reorder (a chorus recalled after a bridge must not
    /// inherit the bridge's leftovers).
    #[serde(default)]
    pub block: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CueList {
    #[serde(default)]
    pub name: String,
    pub cues: Vec<Cue>,
}

/// Plays a `CueList` forward, one `go()` at a time, holding the interpolated
/// output for whatever moment `output_at` is asked for. Has no concept of
/// wall-clock time itself (`no_std`-friendly, matches `ignition-core`'s own
/// aspiration — see `lib.rs`) — the caller drives it with elapsed seconds
/// each tick, the same way `daw-audio-graph`-style processing crates in the
/// sibling FastTrackStudio repo are always handed time as data rather than
/// reading a clock themselves.
/// Where a tracked attribute's value comes from.
///
/// Tracking carries the *source*, not a number, because a recipe is a
/// live thing: one left running by a cue three cues back has to keep
/// being asked for its current value, not frozen at whatever it read
/// when it was fired.
#[derive(Debug, Clone, PartialEq)]
enum Source {
    Direct(f32),
    /// Index into `CuePlayer::active`.
    Recipe(usize),
}

pub struct CuePlayer {
    cues: Vec<Cue>,
    /// Index of the cue currently playing/played-into, or `None` before the
    /// first `go()` (nothing has been fired yet — output is empty/blackout).
    current: Option<usize>,
    /// Values at the moment the current move started — the fade-FROM
    /// snapshot, taken fresh on every `go()` so re-firing mid-fade chains
    /// smoothly from wherever playback actually was, not the previous cue's
    /// resting value.
    from: HashMap<(ChanId, Attribute), f32>,
    /// Cumulative tracked state — every `(chan, attr)` any cue up to and
    /// including the current one has set, and where its value comes from.
    /// This is what makes tracking work: a cue that doesn't mention a
    /// channel simply doesn't touch this map.
    tracked: HashMap<(ChanId, Attribute), Source>,
    /// Recipes introduced by cues so far, kept alive as long as anything
    /// in `tracked` still points at them. Compacted on every `go()`, so a
    /// three-hour show does not accumulate one entry per recipe per cue.
    active: Vec<Recipe>,
    elapsed: f32,
    fade_secs: f32,
}

impl CuePlayer {
    pub fn new(cues: Vec<Cue>) -> Self {
        Self {
            cues,
            current: None,
            from: HashMap::new(),
            tracked: HashMap::new(),
            active: Vec::new(),
            elapsed: 0.0,
            fade_secs: 0.0,
        }
    }

    /// Advances playback into the next cue, if there is one. No-op at the
    /// end of the list (matches a real console's "GO" on the last cue —
    /// nothing to advance into, stays put).
    pub fn go(&mut self, show: &Show<'_>) {
        let next = self.current.map_or(0, |i| i + 1);
        if self.cues.get(next).is_none() {
            return;
        }
        self.from = self.output_at(self.elapsed, show);
        let cue = self.cues[next].clone();

        if cue.block {
            self.tracked.clear();
            self.active.clear();
        }

        // Cook: resolving a recipe now is what establishes *which*
        // (chan, attr) pairs it covers, so tracking knows what it owns.
        // The values it produces are re-resolved every frame — cooking
        // fixes coverage, not output.
        for recipe in &cue.recipes {
            let id = self.active.len();
            self.active.push(recipe.clone());
            for value in expand_recipe(recipe, show) {
                self.tracked
                    .insert((value.chan, value.attr), Source::Recipe(id));
            }
        }
        // Layer 1 last, so a direct value on this cue beats a recipe on
        // the same cue. The cascade is an ordering, not a merge.
        for v in &cue.values {
            self.tracked
                .insert((v.chan, v.attr.clone()), Source::Direct(v.value));
        }

        self.compact();
        self.elapsed = 0.0;
        self.fade_secs = cue.fade_secs;
        self.current = Some(next);
    }

    /// Drops recipes nothing tracks to any more, renumbering the rest.
    ///
    /// Without this, `active` gains an entry per recipe per `go()` for
    /// the length of the show and never gives one back — invisible at
    /// eleven cues, a real leak over a three-hour service.
    fn compact(&mut self) {
        let live: HashSet<usize> = self
            .tracked
            .values()
            .filter_map(|s| match s {
                Source::Recipe(id) => Some(*id),
                Source::Direct(_) => None,
            })
            .collect();
        if live.len() == self.active.len() {
            return;
        }
        let mut remap = HashMap::with_capacity(live.len());
        let mut kept = Vec::with_capacity(live.len());
        for (id, recipe) in self.active.drain(..).enumerate() {
            if live.contains(&id) {
                remap.insert(id, kept.len());
                kept.push(recipe);
            }
        }
        self.active = kept;
        for source in self.tracked.values_mut() {
            if let Source::Recipe(id) = source {
                *id = remap[id];
            }
        }
    }

    /// Jumps straight to the end of cue `index`'s fade (as if `go()` had
    /// been called `index + 1` times and enough time had passed for each
    /// fade to finish) — for headless/automated testing and snapshotting a
    /// specific point in a show without stepping through every cue with
    /// real elapsed time. Out-of-range `index` clamps to the last cue.
    pub fn jump_to_end_of(&mut self, index: usize, show: &Show<'_>) {
        let target = index.min(self.cues.len().saturating_sub(1));
        while self.current.is_none_or(|i| i < target) && self.current != Some(target) {
            self.go(show);
            self.elapsed = self.fade_secs;
            if self.current == Some(target) {
                break;
            }
        }
    }

    pub fn tick(&mut self, dt_secs: f32) {
        self.elapsed += dt_secs;
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current_name(&self) -> Option<&str> {
        self.current
            .and_then(|i| self.cues.get(i))
            .map(|c| c.name.as_str())
    }

    /// The interpolated `(chan, attr) -> value` output right now (at
    /// whatever `elapsed` `tick()` has accumulated to).
    pub fn output(&self, show: &Show<'_>) -> HashMap<(ChanId, Attribute), f32> {
        self.output_at(self.elapsed, show)
    }

    /// Resolves the tracked layer stack into one frame of output.
    ///
    /// Deliberately a pure function of (tracked state, `show`, `elapsed`)
    /// with nothing cached between calls. That is affordable at this
    /// rig's scale and it is what keeps a memoisation layer a legal
    /// optimisation later rather than a rewrite — see
    /// `docs/domain/cue-building-architecture.md`, Decision 1.
    fn output_at(&self, elapsed: f32, show: &Show<'_>) -> HashMap<(ChanId, Attribute), f32> {
        let t = if self.fade_secs > 0.0 {
            (elapsed / self.fade_secs).clamp(0.0, 1.0)
        } else {
            1.0
        };
        // Resolve only the recipes something still tracks to, once each,
        // rather than once per attribute they cover.
        let mut resolved: HashMap<usize, HashMap<(ChanId, Attribute), f32>> = HashMap::new();
        for source in self.tracked.values() {
            if let Source::Recipe(id) = source {
                resolved.entry(*id).or_insert_with(|| {
                    expand_recipe(&self.active[*id], show)
                        .into_iter()
                        .map(|v| ((v.chan, v.attr), v.value))
                        .collect()
                });
            }
        }

        let mut out = HashMap::with_capacity(self.tracked.len());
        for (key, source) in &self.tracked {
            let target_v = match source {
                Source::Direct(v) => *v,
                // A recipe whose selection has since stopped covering
                // this channel simply contributes nothing, rather than
                // holding a stale value — the tolerance the rest of
                // recipe resolution already has.
                Source::Recipe(id) => match resolved[id].get(key) {
                    Some(v) => *v,
                    None => continue,
                },
            };
            // A key with no prior value (first time this (chan, attr) has
            // ever been targeted) fades in from 0 rather than snapping —
            // reads as the fixture coming up from off, the same as a real
            // desk's first cue on a previously-untouched channel.
            let from_v = self.from.get(key).copied().unwrap_or(0.0);
            out.insert(key.clone(), from_v + (target_v - from_v) * t);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(name: &str, fade_secs: f32, values: Vec<(ChanId, Attribute, f32)>) -> Cue {
        Cue {
            name: name.to_string(),
            fade_secs,
            values: values
                .into_iter()
                .map(|(chan, attr, value)| CueValue { chan, attr, value })
                .collect(),
            ..Default::default()
        }
    }

    /// These tests exercise tracking and fades, none of which need a
    /// venue — the layer-2 path has its own tests below.
    fn bare() -> Show<'static> {
        Show::new(&[], &crate::selection::EMPTY_RIG)
    }

    #[test]
    fn before_the_first_go_output_is_empty() {
        let player = CuePlayer::new(vec![cue("Cue 1", 1.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        assert!(player.output(&bare()).is_empty());
        assert_eq!(player.current_index(), None);
    }

    #[test]
    fn a_zero_fade_cue_snaps_immediately() {
        let mut player = CuePlayer::new(vec![cue("Cue 1", 0.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        player.go(&bare());
        assert_eq!(
            player.output(&bare()).get(&(1, Attribute::Dimmer)),
            Some(&1.0)
        );
    }

    #[test]
    fn a_fade_interpolates_over_elapsed_time() {
        let mut player = CuePlayer::new(vec![cue("Cue 1", 2.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        player.go(&bare());
        player.tick(1.0); // halfway through a 2s fade
        let v = *player.output(&bare()).get(&(1, Attribute::Dimmer)).unwrap();
        assert!(
            (v - 0.5).abs() < 0.001,
            "expected ~0.5 halfway through the fade, got {v}"
        );
        player.tick(1.0); // now fully elapsed
        assert!((player.output(&bare()).get(&(1, Attribute::Dimmer)).unwrap() - 1.0).abs() < 0.001);
    }

    #[test]
    fn a_channel_not_mentioned_in_the_next_cue_holds_its_value_tracking() {
        let mut player = CuePlayer::new(vec![
            cue(
                "Cue 1",
                0.0,
                vec![(1, Attribute::Dimmer, 1.0), (2, Attribute::Dimmer, 0.5)],
            ),
            // Cue 2 only touches channel 1 — channel 2 should still read 0.5.
            cue("Cue 2", 0.0, vec![(1, Attribute::Dimmer, 0.2)]),
        ]);
        player.go(&bare());
        player.go(&bare());
        let out = player.output(&bare());
        assert!((out.get(&(1, Attribute::Dimmer)).unwrap() - 0.2).abs() < 0.001);
        assert!(
            (out.get(&(2, Attribute::Dimmer)).unwrap() - 0.5).abs() < 0.001,
            "untouched channel should track forward"
        );
    }

    #[test]
    fn go_on_the_last_cue_is_a_no_op() {
        let mut player = CuePlayer::new(vec![cue(
            "Only Cue",
            0.0,
            vec![(1, Attribute::Dimmer, 1.0)],
        )]);
        player.go(&bare());
        assert_eq!(player.current_index(), Some(0));
        player.go(&bare()); // no cue 1 to advance into
        assert_eq!(player.current_index(), Some(0));
        assert_eq!(
            player.output(&bare()).get(&(1, Attribute::Dimmer)),
            Some(&1.0)
        );
    }

    #[test]
    fn refiring_go_mid_fade_chains_from_the_actual_current_position_not_the_prior_target() {
        let mut player = CuePlayer::new(vec![
            cue("Cue 1", 4.0, vec![(1, Attribute::Dimmer, 1.0)]),
            cue("Cue 2", 4.0, vec![(1, Attribute::Dimmer, 0.0)]),
        ]);
        player.go(&bare());
        player.tick(2.0); // halfway into Cue 1's fade -> dimmer at ~0.5
        player.go(&bare()); // fire Cue 2 before Cue 1 finished
        let v = *player.output(&bare()).get(&(1, Attribute::Dimmer)).unwrap();
        assert!(
            (v - 0.5).abs() < 0.01,
            "should start Cue 2's fade from the actual mid-fade value, got {v}"
        );
    }

    #[test]
    fn jump_to_end_of_resolves_every_cue_up_to_and_including_the_target() {
        let mut player = CuePlayer::new(vec![
            cue("Cue 1", 1.0, vec![(1, Attribute::Dimmer, 1.0)]),
            cue("Cue 2", 1.0, vec![(2, Attribute::Dimmer, 1.0)]),
            cue("Cue 3", 1.0, vec![(1, Attribute::Dimmer, 0.0)]),
        ]);
        player.jump_to_end_of(1, &bare());
        assert_eq!(player.current_index(), Some(1));
        let out = player.output(&bare());
        assert_eq!(
            out.get(&(1, Attribute::Dimmer)),
            Some(&1.0),
            "Cue 1's value should have fully resolved"
        );
        assert_eq!(
            out.get(&(2, Attribute::Dimmer)),
            Some(&1.0),
            "Cue 2's own value should have fully resolved"
        );
    }

    // -----------------------------------------------------------------
    // The cascade and recipe tracking
    // -----------------------------------------------------------------

    use crate::group::Group;
    use crate::recipe::RecipeApply;
    use crate::selection::Selection;

    fn pars() -> Vec<Group> {
        vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3],
        }]
    }

    fn recipe_cue(name: &str, level: f32) -> Cue {
        Cue {
            name: name.to_string(),
            recipes: vec![Recipe {
                target: Selection::Group("Pars".to_string()),
                apply: RecipeApply::Dimmer(level),
            }],
            ..Default::default()
        }
    }

    #[test]
    fn a_recipe_on_a_cue_resolves_at_output_time() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![recipe_cue("Wash", 0.8)]);
        player.go(&show);
        let out = player.output(&show);
        assert_eq!(out.len(), 3);
        assert_eq!(out.get(&(2, Attribute::Dimmer)), Some(&0.8));
    }

    /// The point of storing the template: the same cue covers a fixture
    /// the group did not have when the show loaded.
    #[test]
    fn a_recipe_covers_a_fixture_added_to_its_group_after_loading() {
        let mut player = CuePlayer::new(vec![recipe_cue("Wash", 1.0)]);
        let before = pars();
        player.go(&Show::new(&before, &crate::selection::EMPTY_RIG));

        let after = vec![Group {
            name: "Pars".to_string(),
            chans: vec![1, 2, 3, 4],
        }];
        let grown = Show::new(&after, &crate::selection::EMPTY_RIG);
        // Re-firing is the cook that picks up the new fixture.
        let mut player2 = CuePlayer::new(vec![recipe_cue("Wash", 1.0)]);
        player2.go(&grown);
        assert_eq!(player2.output(&grown).len(), 4);
        // ...and the original player is unchanged, because coverage is
        // fixed at cook time rather than drifting under a running cue.
        assert_eq!(player.output(&grown).len(), 3);
    }

    /// Layer 1 beats layer 2 *within one cue* — the ordering is what
    /// makes "override one fixture out of the recipe" defined.
    #[test]
    fn a_direct_value_beats_a_recipe_on_the_same_cue() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut cue = recipe_cue("Wash", 0.8);
        cue.values = vec![CueValue {
            chan: 2,
            attr: Attribute::Dimmer,
            value: 0.1,
        }];
        let mut player = CuePlayer::new(vec![cue]);
        player.go(&show);
        let out = player.output(&show);
        assert_eq!(out.get(&(1, Attribute::Dimmer)), Some(&0.8));
        assert_eq!(
            out.get(&(2, Attribute::Dimmer)),
            Some(&0.1),
            "the tweak wins"
        );
        assert_eq!(out.get(&(3, Attribute::Dimmer)), Some(&0.8));
    }

    #[test]
    fn a_later_cues_recipe_supersedes_an_earlier_ones() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![recipe_cue("A", 0.2), recipe_cue("B", 0.9)]);
        player.go(&show);
        player.go(&show);
        assert_eq!(
            player.output(&show).get(&(1, Attribute::Dimmer)),
            Some(&0.9)
        );
    }

    #[test]
    fn a_recipe_tracks_forward_through_a_cue_that_does_not_mention_it() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut player = CuePlayer::new(vec![
            recipe_cue("Wash", 0.7),
            cue("Something Else", 0.0, vec![(9, Attribute::Dimmer, 1.0)]),
        ]);
        player.go(&show);
        player.go(&show);
        assert_eq!(
            player.output(&show).get(&(1, Attribute::Dimmer)),
            Some(&0.7)
        );
    }

    #[test]
    fn a_block_cue_drops_everything_it_does_not_set() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let mut blocked = cue("Fresh", 0.0, vec![(9, Attribute::Dimmer, 1.0)]);
        blocked.block = true;
        let mut player = CuePlayer::new(vec![recipe_cue("Wash", 0.7), blocked]);
        player.go(&show);
        player.go(&show);
        let out = player.output(&show);
        assert!(!out.contains_key(&(1, Attribute::Dimmer)), "{out:?}");
        assert_eq!(out.get(&(9, Attribute::Dimmer)), Some(&1.0));
    }

    /// Without compaction `active` gains an entry per recipe per `go()`
    /// and never gives one back.
    #[test]
    fn superseded_recipes_are_dropped_rather_than_accumulating() {
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);
        let cues: Vec<Cue> = (0..20)
            .map(|i| recipe_cue("Wash", i as f32 / 20.0))
            .collect();
        let mut player = CuePlayer::new(cues);
        for _ in 0..20 {
            player.go(&show);
        }
        assert_eq!(
            player.active.len(),
            1,
            "only the newest recipe is still tracked"
        );
    }
}
