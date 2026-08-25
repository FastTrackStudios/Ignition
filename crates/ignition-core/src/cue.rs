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

use ignition_proto::{Attribute, ChanId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cue {
    pub name: String,
    #[serde(default)]
    pub fade_secs: f32,
    pub values: Vec<CueValue>,
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
    /// including the current one has ever set, at its *target* value. This
    /// is what makes tracking work: a cue that doesn't mention a channel
    /// simply doesn't touch this map, so the value already there keeps
    /// being the target.
    target: HashMap<(ChanId, Attribute), f32>,
    elapsed: f32,
    fade_secs: f32,
}

impl CuePlayer {
    pub fn new(cues: Vec<Cue>) -> Self {
        Self { cues, current: None, from: HashMap::new(), target: HashMap::new(), elapsed: 0.0, fade_secs: 0.0 }
    }

    /// Advances playback into the next cue, if there is one. No-op at the
    /// end of the list (matches a real console's "GO" on the last cue —
    /// nothing to advance into, stays put).
    pub fn go(&mut self) {
        let next = self.current.map_or(0, |i| i + 1);
        let Some(cue) = self.cues.get(next) else { return };
        self.from = self.output_at(self.elapsed);
        for v in &cue.values {
            self.target.insert((v.chan, v.attr.clone()), v.value);
        }
        self.elapsed = 0.0;
        self.fade_secs = cue.fade_secs;
        self.current = Some(next);
    }

    /// Jumps straight to the end of cue `index`'s fade (as if `go()` had
    /// been called `index + 1` times and enough time had passed for each
    /// fade to finish) — for headless/automated testing and snapshotting a
    /// specific point in a show without stepping through every cue with
    /// real elapsed time. Out-of-range `index` clamps to the last cue.
    pub fn jump_to_end_of(&mut self, index: usize) {
        let target = index.min(self.cues.len().saturating_sub(1));
        while self.current.is_none_or(|i| i < target) && self.current != Some(target) {
            self.go();
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
        self.current.and_then(|i| self.cues.get(i)).map(|c| c.name.as_str())
    }

    /// The interpolated `(chan, attr) -> value` output right now (at
    /// whatever `elapsed` `tick()` has accumulated to).
    pub fn output(&self) -> HashMap<(ChanId, Attribute), f32> {
        self.output_at(self.elapsed)
    }

    fn output_at(&self, elapsed: f32) -> HashMap<(ChanId, Attribute), f32> {
        let t = if self.fade_secs > 0.0 { (elapsed / self.fade_secs).clamp(0.0, 1.0) } else { 1.0 };
        let mut out = HashMap::with_capacity(self.target.len());
        for (key, &target_v) in &self.target {
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
            values: values.into_iter().map(|(chan, attr, value)| CueValue { chan, attr, value }).collect(),
        }
    }

    #[test]
    fn before_the_first_go_output_is_empty() {
        let player = CuePlayer::new(vec![cue("Cue 1", 1.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        assert!(player.output().is_empty());
        assert_eq!(player.current_index(), None);
    }

    #[test]
    fn a_zero_fade_cue_snaps_immediately() {
        let mut player = CuePlayer::new(vec![cue("Cue 1", 0.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        player.go();
        assert_eq!(player.output().get(&(1, Attribute::Dimmer)), Some(&1.0));
    }

    #[test]
    fn a_fade_interpolates_over_elapsed_time() {
        let mut player = CuePlayer::new(vec![cue("Cue 1", 2.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        player.go();
        player.tick(1.0); // halfway through a 2s fade
        let v = *player.output().get(&(1, Attribute::Dimmer)).unwrap();
        assert!((v - 0.5).abs() < 0.001, "expected ~0.5 halfway through the fade, got {v}");
        player.tick(1.0); // now fully elapsed
        assert!((player.output().get(&(1, Attribute::Dimmer)).unwrap() - 1.0).abs() < 0.001);
    }

    #[test]
    fn a_channel_not_mentioned_in_the_next_cue_holds_its_value_tracking() {
        let mut player = CuePlayer::new(vec![
            cue("Cue 1", 0.0, vec![(1, Attribute::Dimmer, 1.0), (2, Attribute::Dimmer, 0.5)]),
            // Cue 2 only touches channel 1 — channel 2 should still read 0.5.
            cue("Cue 2", 0.0, vec![(1, Attribute::Dimmer, 0.2)]),
        ]);
        player.go();
        player.go();
        let out = player.output();
        assert!((out.get(&(1, Attribute::Dimmer)).unwrap() - 0.2).abs() < 0.001);
        assert!(
            (out.get(&(2, Attribute::Dimmer)).unwrap() - 0.5).abs() < 0.001,
            "untouched channel should track forward"
        );
    }

    #[test]
    fn go_on_the_last_cue_is_a_no_op() {
        let mut player = CuePlayer::new(vec![cue("Only Cue", 0.0, vec![(1, Attribute::Dimmer, 1.0)])]);
        player.go();
        assert_eq!(player.current_index(), Some(0));
        player.go(); // no cue 1 to advance into
        assert_eq!(player.current_index(), Some(0));
        assert_eq!(player.output().get(&(1, Attribute::Dimmer)), Some(&1.0));
    }

    #[test]
    fn refiring_go_mid_fade_chains_from_the_actual_current_position_not_the_prior_target() {
        let mut player = CuePlayer::new(vec![
            cue("Cue 1", 4.0, vec![(1, Attribute::Dimmer, 1.0)]),
            cue("Cue 2", 4.0, vec![(1, Attribute::Dimmer, 0.0)]),
        ]);
        player.go();
        player.tick(2.0); // halfway into Cue 1's fade -> dimmer at ~0.5
        player.go(); // fire Cue 2 before Cue 1 finished
        let v = *player.output().get(&(1, Attribute::Dimmer)).unwrap();
        assert!((v - 0.5).abs() < 0.01, "should start Cue 2's fade from the actual mid-fade value, got {v}");
    }

    #[test]
    fn jump_to_end_of_resolves_every_cue_up_to_and_including_the_target() {
        let mut player = CuePlayer::new(vec![
            cue("Cue 1", 1.0, vec![(1, Attribute::Dimmer, 1.0)]),
            cue("Cue 2", 1.0, vec![(2, Attribute::Dimmer, 1.0)]),
            cue("Cue 3", 1.0, vec![(1, Attribute::Dimmer, 0.0)]),
        ]);
        player.jump_to_end_of(1);
        assert_eq!(player.current_index(), Some(1));
        let out = player.output();
        assert_eq!(out.get(&(1, Attribute::Dimmer)), Some(&1.0), "Cue 1's value should have fully resolved");
        assert_eq!(out.get(&(2, Attribute::Dimmer)), Some(&1.0), "Cue 2's own value should have fully resolved");
    }
}
