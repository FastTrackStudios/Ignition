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

use crate::music::Bars;
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
    /// Where this cue sits in the song, if it belongs to one.
    ///
    /// Optional, and *alongside* list order rather than instead of it.
    /// `at` is what a clock uses; order is what a person pressing GO
    /// uses; both land in the same state. That is the whole of "losing
    /// backing tracks must not mean losing lighting" — see
    /// `docs/domain/musical-time-cues.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Bars>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CueList {
    #[serde(default)]
    pub name: String,
    pub cues: Vec<Cue>,
}

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

/// The layer stack at one point in the show — what tracking carries
/// forward from cue to cue.
#[derive(Debug, Clone, Default)]
struct Layers {
    /// Every `(chan, attr)` any cue up to here has set, and where its
    /// value comes from. A cue that does not mention a channel simply
    /// does not touch this, which is what tracking *is*.
    tracked: HashMap<(ChanId, Attribute), Source>,
    /// Which recipe, if any, is *modulating* each attribute. Relative
    /// values do not compete for the slot — they add on top of whatever
    /// won it — so they get their own map. One modulator per attribute,
    /// latest cue wins, same rule as everything else.
    /// Every relative recipe touching a key, in the order taken.
    ///
    /// A list rather than one winner, and that is the difference between
    /// an accent working and doing nothing. With a single slot, a bump
    /// laid over a running chase *replaced* it — so a hit on fixtures
    /// already being chased produced no lift at all, and once the bump's
    /// envelope held at zero it went on owning the slot and the chase
    /// stayed dead for the rest of the section. Modulation is supposed
    /// to be the layer that composes; one winner is the one rule that
    /// stops it composing.
    ///
    /// Growth is bounded by the cascade: a blocking cue starts from
    /// empty layers, so this only accumulates within a section.
    modulated: HashMap<(ChanId, Attribute), Vec<usize>>,
}

/// One cue's worth of state, plus how far into its own fade it is.
#[derive(Debug, Clone)]
struct Stage {
    layers: Layers,
    fade_secs: f32,
    elapsed: f32,
}

/// The time a recipe should be evaluated at.
///
/// Looping phasers get the shared show clock, so two cues carrying the
/// same chase stay in phase rather than each restarting it. One-shots
/// get time since they were taken, because an envelope that finished
/// before its own cue fired never plays at all.
impl CuePlayer {
    fn recipe_time(&self, id: usize) -> f32 {
        match self.active.get(id) {
            Some(recipe) if recipe.timing.once => {
                self.clock - self.active_since.get(id).copied().unwrap_or(self.clock)
            }
            _ => self.clock,
        }
    }
}

impl Stage {
    fn progress(&self) -> f32 {
        if self.fade_secs > 0.0 {
            (self.elapsed / self.fade_secs).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }
}

/// How many overlapping fades to carry before the oldest is forced to
/// finish. Only reachable by firing GO faster than fades complete,
/// several times over; the alternative is an unbounded stack.
const MAX_FADES: usize = 8;

/// Plays a `CueList`, either stepped by `go()` or driven by `seek()`.
///
/// Has no concept of wall-clock time itself (`no_std`-friendly, matching
/// `ignition-core`'s own aspiration — see `lib.rs`): the caller hands it
/// elapsed seconds each tick, and a musical position if there is one,
/// the same way processing crates in the sibling FastTrackStudio tree
/// are always given time as data rather than reading a clock.
pub struct CuePlayer {
    cues: Vec<Cue>,
    /// Index of the cue currently playing/played-into, or `None` before the
    /// first `go()` (nothing has been fired yet — output is empty/blackout).
    current: Option<usize>,
    /// Recipes introduced by cues so far, kept alive as long as any live
    /// stage still points at them. Compacted on every `go()`, so a
    /// three-hour show does not accumulate one entry per recipe per cue.
    active: Vec<Recipe>,
    /// Show time each active recipe was taken, parallel to `active`.
    ///
    /// Only one-shots read it. A looping phaser deliberately runs off
    /// the shared monotonic clock, because that is what keeps two cues
    /// carrying the same chase in phase with each other instead of each
    /// restarting it. A one-shot needs the opposite: an envelope that
    /// has already finished before its cue was taken is not an envelope,
    /// it is a fixture sitting at its end value.
    active_since: Vec<f32>,
    /// Oldest first. The last stage is the cue being moved into;
    /// everything before it is a fade still resolving.
    ///
    /// A stack rather than a single remembered snapshot because both
    /// sides of a fade have to keep *resolving*: crossing from one
    /// phaser into another, the outgoing one must keep moving while it
    /// goes. A snapshot freezes it. See
    /// `docs/domain/cue-building-architecture.md`, Decision 6.
    stack: Vec<Stage>,
    /// Monotonic show time. Unlike a fade's `elapsed` this never resets,
    /// because a phaser free-runs across cues — restarting its cycle on
    /// every GO would make a chase stutter every time an unrelated cue
    /// fired.
    clock: f32,
}

impl CuePlayer {
    pub fn new(cues: Vec<Cue>) -> Self {
        Self {
            cues,
            current: None,
            active: Vec::new(),
            active_since: Vec::new(),
            stack: Vec::new(),
            clock: 0.0,
        }
    }

    /// Advances playback into the next cue, if there is one. No-op at the
    /// end of the list (matches a real console's "GO" on the last cue —
    /// nothing to advance into, stays put).
    pub fn go(&mut self, show: &Show<'_>) {
        let next = self.current.map_or(0, |i| i + 1);
        if next >= self.cues.len() {
            return;
        }
        let cue = self.cues[next].clone();

        // Tracking: start from wherever the show already is, unless this
        // cue blocks.
        let mut layers = if cue.block {
            Layers::default()
        } else {
            self.stack
                .last()
                .map(|s| s.layers.clone())
                .unwrap_or_default()
        };

        // Cook: resolving a recipe now is what establishes *which*
        // (chan, attr) pairs it covers, so tracking knows what it owns.
        // The values it produces are re-resolved every frame — cooking
        // fixes coverage, not output.
        for recipe in &cue.recipes {
            let id = self.active.len();
            self.active.push(recipe.clone());
            self.active_since.push(self.clock);
            for emit in expand_recipe(recipe, show, self.recipe_time(id)) {
                let key = (emit.value.chan, emit.value.attr);
                if emit.relative {
                    let slot = layers.modulated.entry(key).or_default();
                    // A recipe re-taken by a later cue moves to the end
                    // rather than stacking on itself — re-stating a
                    // chase should not double it.
                    slot.retain(|existing| *existing != id);
                    slot.push(id);
                } else {
                    layers.tracked.insert(key, Source::Recipe(id));
                }
            }
        }
        // Layer 1 last, so a direct value on this cue beats a recipe on
        // the same cue. The cascade is an ordering, not a merge.
        for v in &cue.values {
            layers
                .tracked
                .insert((v.chan, v.attr.clone()), Source::Direct(v.value));
        }

        self.stack.push(Stage {
            layers,
            fade_secs: cue.fade_secs,
            elapsed: 0.0,
        });
        while self.stack.len() > MAX_FADES {
            self.stack.remove(0);
        }
        self.collapse();
        self.compact();
        self.current = Some(next);
    }

    /// Jumps straight to the end of cue `index`'s fade (as if `go()` had
    /// been called `index + 1` times and enough time had passed for each
    /// fade to finish) — for headless/automated testing and snapshotting a
    /// specific point in a show without stepping through every cue with
    /// real elapsed time. Out-of-range `index` clamps to the last cue.
    pub fn jump_to_end_of(&mut self, index: usize, show: &Show<'_>) {
        let target = index.min(self.cues.len().saturating_sub(1));
        while self.current != Some(target) {
            let before = self.current;
            self.go(show);
            if let Some(stage) = self.stack.last_mut() {
                stage.elapsed = stage.fade_secs;
            }
            self.collapse();
            if self.current == before {
                break; // ran off the end of the list
            }
        }
    }

    /// The index of the last cue at or before a musical position.
    ///
    /// Cues without an `at` are invisible to this: a list can mix
    /// positioned cues with hand-only ones, and the unpositioned ones
    /// simply never come up under a clock.
    pub fn index_at(&self, position: Bars) -> Option<usize> {
        self.cues
            .iter()
            .enumerate()
            .rev()
            .find(|(_, cue)| cue.at.is_some_and(|at| at <= position))
            .map(|(i, _)| i)
    }

    /// Puts playback where a musical position says it should be.
    ///
    /// This — not `go` — is what makes a synced show survive being
    /// looped, restarted, or jumped around in. An event-driven player
    /// that has been *told* to fire cues 1..9 has no answer to "we went
    /// back to bar 22" except replaying its own history. Asking instead
    /// "what is the state at bar 22" has one answer, and it is the same
    /// answer however you arrived.
    ///
    /// Cheap to call every frame: it returns immediately unless the
    /// position implies a different cue than the one already current.
    /// Seeking *backwards* rebuilds from the top of the list, because
    /// tracking means a cue's state is the sum of everything before it.
    pub fn seek(&mut self, position: Bars, show: &Show<'_>) {
        let Some(target) = self.index_at(position) else {
            // Before the first positioned cue — nothing has happened yet.
            if self.current.is_some() {
                self.reset();
            }
            return;
        };
        if self.current == Some(target) {
            return;
        }
        if self.current.is_none_or(|current| current > target) {
            self.reset();
        }
        self.jump_to_end_of(target, show);
    }

    /// Back to before the first cue, keeping the show clock — a phaser
    /// that is running should not restart because the operator jumped
    /// the transport.
    fn reset(&mut self) {
        self.current = None;
        self.stack.clear();
        self.active.clear();
    }

    pub fn tick(&mut self, dt_secs: f32) {
        self.clock += dt_secs;
        for stage in &mut self.stack {
            stage.elapsed += dt_secs;
        }
        self.collapse();
    }

    /// Advances the show clock without advancing any fade — for
    /// snapshotting a running phaser at a chosen moment.
    pub fn advance_clock(&mut self, secs: f32) {
        self.clock += secs;
    }

    pub fn clock(&self) -> f32 {
        self.clock
    }

    /// Drops fades that have finished. Once a stage is fully arrived,
    /// everything under it contributes nothing.
    fn collapse(&mut self) {
        if let Some(last) = self.stack.iter().rposition(|s| s.progress() >= 1.0)
            && last > 0
        {
            self.stack.drain(..last);
        }
    }

    /// Drops recipes no live stage points at any more, renumbering the
    /// rest.
    ///
    /// Without this, `active` gains an entry per recipe per `go()` for
    /// the length of the show and never gives one back — invisible at
    /// eleven cues, a real leak over a three-hour service.
    fn compact(&mut self) {
        let live: HashSet<usize> = self
            .stack
            .iter()
            .flat_map(|stage| {
                stage
                    .layers
                    .tracked
                    .values()
                    .filter_map(|s| match s {
                        Source::Recipe(id) => Some(*id),
                        Source::Direct(_) => None,
                    })
                    .chain(stage.layers.modulated.values().flatten().copied())
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
        for stage in &mut self.stack {
            for source in stage.layers.tracked.values_mut() {
                if let Source::Recipe(id) = source {
                    *id = remap[id];
                }
            }
            for ids in stage.layers.modulated.values_mut() {
                for id in ids.iter_mut() {
                    *id = remap[id];
                }
            }
        }
    }

    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn current_name(&self) -> Option<&str> {
        self.current
            .and_then(|i| self.cues.get(i))
            .map(|c| c.name.as_str())
    }

    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }

    /// The `(chan, attr) -> value` output right now.
    ///
    /// Every stage in the stack resolves live and they are folded oldest
    /// to newest, so a phaser being faded *out of* keeps moving while it
    /// goes, exactly like the one being faded into.
    ///
    /// Deliberately a pure function of (stack, `show`, clock) with
    /// nothing cached between calls. That is affordable at this rig's
    /// scale and it is what keeps a memoisation layer a legal
    /// optimisation later rather than a rewrite — see
    /// `docs/domain/cue-building-architecture.md`, Decision 1.
    pub fn output(&self, show: &Show<'_>) -> HashMap<(ChanId, Attribute), f32> {
        let mut out: HashMap<(ChanId, Attribute), f32> = HashMap::new();
        for stage in &self.stack {
            let target = self.resolve(&stage.layers, show);
            out = blend(&out, &target, stage.progress());
        }
        out
    }

    /// One stage's layer stack, resolved through the cascade.
    fn resolve(&self, layers: &Layers, show: &Show<'_>) -> HashMap<(ChanId, Attribute), f32> {
        // Resolve only the recipes something still points at, once each,
        // rather than once per attribute they cover.
        let mut resolved: HashMap<usize, HashMap<(ChanId, Attribute), f32>> = HashMap::new();
        let referenced = layers
            .tracked
            .values()
            .filter_map(|s| match s {
                Source::Recipe(id) => Some(*id),
                Source::Direct(_) => None,
            })
            .chain(layers.modulated.values().flatten().copied());
        for id in referenced {
            resolved.entry(id).or_insert_with(|| {
                expand_recipe(&self.active[id], show, self.recipe_time(id))
                    .into_iter()
                    .map(|e| ((e.value.chan, e.value.attr), e.value.value))
                    .collect()
            });
        }

        let mut out = HashMap::with_capacity(layers.tracked.len());
        for (key, source) in &layers.tracked {
            let value = match source {
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
            // Modulation is applied *after* the cascade has picked a
            // winner, not as another competitor for the slot. That is
            // what "-40% dimmer, and the colour is not my business"
            // means mechanically.
            // Summed, so a bump lands *on top of* whatever chase is
            // already running rather than taking its place.
            let modulation: f32 = layers
                .modulated
                .get(key)
                .into_iter()
                .flatten()
                .filter_map(|id| resolved[id].get(key))
                .sum();
            out.insert(key.clone(), value + modulation);
        }
        out
    }
}

/// Crossfades two resolved frames.
///
/// A key only `next` has fades in from 0 — the fixture coming up from
/// off, the same as a real desk's first cue on a previously-untouched
/// channel. A key only `prev` has fades *out* to 0, which is how a
/// `block` cue takes back what it does not set instead of snapping it
/// dark.
fn blend(
    prev: &HashMap<(ChanId, Attribute), f32>,
    next: &HashMap<(ChanId, Attribute), f32>,
    t: f32,
) -> HashMap<(ChanId, Attribute), f32> {
    let mut out = HashMap::with_capacity(next.len().max(prev.len()));
    for (key, target) in next {
        let from = prev.get(key).copied().unwrap_or(0.0);
        out.insert(key.clone(), from + (target - from) * t);
    }
    for (key, leaving) in prev {
        if !next.contains_key(key) && t < 1.0 {
            out.insert(key.clone(), leaving * (1.0 - t));
        }
    }
    out
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
            recipes: vec![Recipe::new(
                Selection::Group("Pars".to_string()),
                RecipeApply::Dimmer(level),
            )],
            ..Default::default()
        }
    }

    /// A bump over a running chase must *add* to it.
    ///
    /// The failure this pins was invisible in the cue list and obvious
    /// on stage: figures landing on fixtures that were already being
    /// chased did nothing at all, because the accent replaced the chase
    /// in the single modulator slot instead of stacking on it — and then
    /// went on holding that slot at zero, so the chase stayed dead for
    /// the rest of the section too.
    #[test]
    fn a_bump_adds_to_a_running_chase_rather_than_replacing_it() {
        use crate::step::{Speed, Step, Timing};
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);

        let flat = |v: f32| {
            Recipe {
                target: Selection::Group("Pars".to_string()),
                steps: vec![Step::new(vec![RecipeApply::Delta(vec![(
                    Attribute::Dimmer,
                    v,
                )])])],
                timing: Timing {
                    speed: Speed::Bpm(60.0),
                    ..Default::default()
                },
            }
        };

        // A section look with a steady +0.2 modulation on it.
        let look = Cue {
            name: "Look".to_string(),
            recipes: vec![
                Recipe::new(
                    Selection::Group("Pars".to_string()),
                    RecipeApply::Dimmer(0.3),
                ),
                flat(0.2),
            ],
            block: true,
            ..Default::default()
        };
        // ...and an accent adding another +0.5 on top.
        let accent = Cue {
            name: "· hit".to_string(),
            recipes: vec![flat(0.5)],
            block: false,
            ..Default::default()
        };

        let mut player = CuePlayer::new(vec![look, accent]);
        player.go(&show);
        let base = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!((base - 0.5).abs() < 1e-4, "look alone should be 0.3+0.2: {base}");

        player.go(&show);
        let both = player.output(&show)[&(1, Attribute::Dimmer)];
        assert!(
            (both - 1.0).abs() < 1e-4,
            "the accent replaced the chase instead of adding: {both}"
        );
    }

    /// A bump releases itself, which is the whole reason `once` exists.
    /// Before it, a flash needed two cues — one to lift and one to put
    /// out a beat later — so half the cue names in a show were "… out"
    /// and the list said nothing a reader wanted to know.
    #[test]
    fn a_one_shot_bump_releases_itself() {
        use crate::step::{Speed, Step, Timing};
        let groups = pars();
        let show = Show::new(&groups, &crate::selection::EMPTY_RIG);

        let up = Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.6)])]);
        let down = Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])]);
        let bump = Cue {
            name: "· hit".to_string(),
            recipes: vec![Recipe {
                target: Selection::Group("Pars".to_string()),
                steps: vec![up, down],
                timing: Timing {
                    speed: Speed::Bpm(60.0),
                    measure: 1.0,
                    once: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        };

        // A Delta modulates a tracked value, so there has to be a look
        // for it to sit on — which is exactly how the show runs it: a
        // section sets the level, the accent lifts it.
        let mut player = CuePlayer::new(vec![recipe_cue("Look", 0.3), bump]);
        player.go(&show);
        player.go(&show);
        let peak = player
            .output(&show)
            .get(&(1, Attribute::Dimmer))
            .copied()
            .unwrap_or_default();
        assert!(peak > 0.5, "the bump never lifted: {peak}");

        // A second later the envelope has run out and holds at zero.
        player.tick(1.0);
        let after = player
            .output(&show)
            .get(&(1, Attribute::Dimmer))
            .copied()
            .unwrap_or_default();
        assert!((after - 0.3).abs() < 0.05, "the bump did not release to the look: {after}");

        // And stays there rather than looping round for another flash.
        player.tick(5.0);
        let later = player
            .output(&show)
            .get(&(1, Attribute::Dimmer))
            .copied()
            .unwrap_or_default();
        assert!((later - 0.3).abs() < 0.05, "the bump looped: {later}");
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

    // -----------------------------------------------------------------
    // Both sides of a fade resolve live
    // -----------------------------------------------------------------

    use crate::step::{Speed, Step, Timing};

    /// A two-step phaser on channel 1 swinging between `lo` and `hi`
    /// once per second.
    fn chase(lo: f32, hi: f32) -> Recipe {
        let at = |v: f32| Step::new(vec![RecipeApply::Raw(vec![(Attribute::Dimmer, v)])]);
        Recipe {
            target: Selection::Chans(vec![1]),
            steps: vec![at(lo), at(hi)],
            timing: Timing {
                speed: Speed::Hz(1.0),
                ..Default::default()
            },
        }
    }

    /// The bug a snapshot `from` has: crossing from one phaser into
    /// another, the outgoing one must keep moving while it goes. Held
    /// halfway through the fade, the output has to change as the clock
    /// advances even though the fade position does not.
    #[test]
    fn a_phaser_being_faded_out_of_keeps_moving() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            Cue {
                name: "A".into(),
                recipes: vec![chase(0.0, 1.0)],
                ..Default::default()
            },
            Cue {
                name: "B".into(),
                fade_secs: 10.0,
                // Static, so anything that moves in the output can only
                // have come from the outgoing phaser.
                values: vec![CueValue {
                    chan: 1,
                    attr: Attribute::Dimmer,
                    value: 0.5,
                }],
                ..Default::default()
            },
        ]);
        player.go(&show);
        player.go(&show);

        // Advance the clock a quarter-cycle at a time, but read the
        // output at the same fade position each time by rewinding it.
        let sample = |p: &mut CuePlayer, dt: f32| {
            p.tick(dt);
            for stage in &mut p.stack {
                stage.elapsed = 1.0; // a tenth of the way into the fade
            }
            *p.output(&show).get(&(1, Attribute::Dimmer)).unwrap()
        };
        let first = sample(&mut player, 0.0);
        let later = sample(&mut player, 0.5);
        assert!(
            (first - later).abs() > 0.05,
            "the outgoing phaser froze: {first} then {later}"
        );
    }

    /// The incoming side moves too, which the snapshot model already
    /// got right — pinned so a future change cannot lose it.
    #[test]
    fn a_phaser_being_faded_into_moves_while_it_arrives() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue("Static", 0.0, vec![(1, Attribute::Dimmer, 0.0)]),
            Cue {
                name: "Chase".into(),
                fade_secs: 10.0,
                recipes: vec![chase(0.0, 1.0)],
                ..Default::default()
            },
        ]);
        player.go(&show);
        player.go(&show);
        let sample = |p: &mut CuePlayer, dt: f32| {
            p.tick(dt);
            for stage in &mut p.stack {
                stage.elapsed = 1.0;
            }
            *p.output(&show).get(&(1, Attribute::Dimmer)).unwrap()
        };
        let first = sample(&mut player, 0.0);
        let later = sample(&mut player, 0.5);
        assert!((first - later).abs() > 0.01, "{first} then {later}");
    }

    /// Overlapping fades compose rather than the newest simply
    /// discarding the one in flight.
    #[test]
    fn firing_go_mid_fade_stacks_the_fades() {
        let show = bare();
        let mut player = CuePlayer::new(vec![
            cue("A", 0.0, vec![(1, Attribute::Dimmer, 1.0)]),
            cue("B", 4.0, vec![(1, Attribute::Dimmer, 0.0)]),
            cue("C", 4.0, vec![(1, Attribute::Dimmer, 1.0)]),
        ]);
        player.go(&show);
        player.go(&show);
        player.tick(2.0); // halfway to 0, so ~0.5
        let mid = *player.output(&show).get(&(1, Attribute::Dimmer)).unwrap();
        assert!((mid - 0.5).abs() < 0.01, "{mid}");

        player.go(&show); // back up to 1.0, from ~0.5, while B still runs
        assert_eq!(player.stack.len(), 3, "B's fade is still in flight");
        player.tick(4.0);
        let done = *player.output(&show).get(&(1, Attribute::Dimmer)).unwrap();
        assert!((done - 1.0).abs() < 0.01, "{done}");
        assert_eq!(player.stack.len(), 1, "finished fades collapse");
    }

    #[test]
    fn spamming_go_does_not_grow_the_stack_without_bound() {
        let show = bare();
        let cues: Vec<Cue> = (0..40)
            .map(|i| cue(&format!("C{i}"), 100.0, vec![(1, Attribute::Dimmer, 1.0)]))
            .collect();
        let mut player = CuePlayer::new(cues);
        for _ in 0..40 {
            player.go(&show);
        }
        assert!(player.stack.len() <= MAX_FADES, "{}", player.stack.len());
    }

    // -----------------------------------------------------------------
    // Musical position
    // -----------------------------------------------------------------

    /// A cue at a bar, setting one channel to a recognisable level.
    fn at_bar(bar: u32, level: f32) -> Cue {
        Cue {
            name: format!("bar {bar}"),
            values: vec![CueValue {
                chan: 1,
                attr: Attribute::Dimmer,
                value: level,
            }],
            at: Some(Bars::bar(bar)),
            ..Default::default()
        }
    }

    fn song() -> Vec<Cue> {
        vec![
            at_bar(1, 0.1),
            at_bar(11, 0.2),
            at_bar(23, 0.3),
            at_bar(31, 0.4),
        ]
    }

    fn level(player: &CuePlayer, show: &Show<'_>) -> Option<f32> {
        player.output(show).get(&(1, Attribute::Dimmer)).copied()
    }

    #[test]
    fn seeking_lands_on_the_last_cue_at_or_before_the_position() {
        let show = bare();
        let mut player = CuePlayer::new(song());
        player.seek(Bars::bar(23), &show);
        assert_eq!(level(&player, &show), Some(0.3));
        // Mid-section stays on the section's cue.
        player.seek(Bars::new(27, 2.5), &show);
        assert_eq!(level(&player, &show), Some(0.3));
    }

    /// The point of position-addressing: a jump backwards has to land in
    /// the same state as arriving there forwards. An event-driven player
    /// cannot do this without replaying its own history.
    #[test]
    fn seeking_backwards_lands_where_playing_forwards_would() {
        let show = bare();
        let mut forwards = CuePlayer::new(song());
        forwards.seek(Bars::bar(11), &show);
        let expected = level(&forwards, &show);

        let mut jumped = CuePlayer::new(song());
        jumped.seek(Bars::bar(31), &show);
        jumped.seek(Bars::bar(11), &show);
        assert_eq!(level(&jumped, &show), expected);
    }

    /// Looping a section is the same motion, repeated — the case a live
    /// rehearsal spends all afternoon in.
    #[test]
    fn looping_a_section_is_stable() {
        let show = bare();
        let mut player = CuePlayer::new(song());
        for _ in 0..3 {
            for bar in [23u32, 26, 29, 23] {
                player.seek(Bars::bar(bar), &show);
                assert_eq!(level(&player, &show), Some(0.3), "bar {bar}");
            }
        }
    }

    #[test]
    fn before_the_first_positioned_cue_nothing_is_lit() {
        let show = bare();
        let mut player = CuePlayer::new(vec![at_bar(11, 0.2), at_bar(23, 0.3)]);
        player.seek(Bars::bar(23), &show);
        assert!(level(&player, &show).is_some());
        player.seek(Bars::bar(1), &show);
        assert!(
            player.output(&show).is_empty(),
            "rewinding past the show clears it"
        );
    }

    /// A list can mix positioned cues with hand-only ones; the clock
    /// simply never picks the unpositioned ones up.
    #[test]
    fn cues_without_a_position_are_invisible_to_seeking() {
        let show = bare();
        let mut cues = song();
        cues.insert(2, cue("Hand only", 0.0, vec![(1, Attribute::Dimmer, 0.99)]));
        let mut player = CuePlayer::new(cues);
        player.seek(Bars::bar(23), &show);
        // Bar 23's cue is now at index 3, and the hand-only one at 2 was
        // passed through on the way — tracking still applied it, which
        // is correct: seeking replays the list, it does not skip it.
        assert_eq!(level(&player, &show), Some(0.3));
        assert_eq!(player.index_at(Bars::bar(23)), Some(3));
    }

    /// Seeking every frame is the normal case, so it has to be free when
    /// nothing changed.
    #[test]
    fn seeking_within_the_same_cue_does_not_refire_it() {
        let show = bare();
        let mut player = CuePlayer::new(song());
        player.seek(Bars::bar(23), &show);
        let stack = player.stack.len();
        for beat in 1..16 {
            player.seek(Bars::new(23 + beat / 4, (beat % 4) as f64 + 1.0), &show);
        }
        assert_eq!(
            player.stack.len(),
            stack,
            "a re-seek inside a cue restacked it"
        );
    }

    /// The show clock is not the transport: a phaser mid-cycle should
    /// not restart because somebody moved the playhead.
    #[test]
    fn seeking_does_not_disturb_the_show_clock() {
        let show = bare();
        let mut player = CuePlayer::new(song());
        player.tick(4.0);
        player.seek(Bars::bar(31), &show);
        player.seek(Bars::bar(1), &show);
        assert!((player.clock() - 4.0).abs() < 1e-6);
    }
}
