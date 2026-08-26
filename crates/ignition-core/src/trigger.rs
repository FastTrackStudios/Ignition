//! Hits that fire when the song plays them.
//!
//! The cue list says what the *look* is. This says what happens on a
//! snare. They are different kinds of thing and had been conflated: the
//! charted snare pattern was being folded into a one-bar phaser and
//! stored inside the section cue, which was wrong in three ways at once.
//!
//! It flattened the music. A phaser derived from "the hit lands in most
//! bars" plays the same bar over and over, so every fill, every dropped
//! backbeat and every turnaround came out as the plain pattern — the
//! parts of a song a light show should be *following* are exactly the
//! parts a derived pattern discards.
//!
//! It ran on the wrong clock. A phaser advances on show time, so a
//! stopped song stopped nothing and the rig went on ticking at an
//! arrangement that was not playing.
//!
//! And it made phase a thing that could be wrong. A note has no phase
//! problem: it is at a position, and either the playhead has reached it
//! or it has not.
//!
//! So a trigger is not scheduled and not derived. The transport moves,
//! this reports which triggers were crossed, and each fires a one-shot
//! that decays on its own. No signal, no light — which is the property
//! the whole design is for.

use crate::music::{Bars, Position, SongMap};
use crate::recipe::{Emit, Recipe, Show, expand_recipe};
use crate::{Attribute, ChanId};

use std::collections::HashMap;

/// One charted event, and what it does to the rig.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
// r[impl triggers.shape]
pub struct Trigger {
    /// Where in the song it happens, as written — relative to a section
    /// so it moves with it, or an absolute bar for older files.
    // r[impl song.relative-position] - triggers carry the relative form too
    pub at: Position,
    /// The bar `at` resolved to, cached in the file for a player with
    /// no song map. See `Cue::resolved`.
    // r[impl song.relative-position.resolved-on-load]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<Bars>,
    /// What it fires. Should be relative and one-shot: a trigger adds to
    /// the look for a moment and gets out of the way, and a looping
    /// recipe here would never stop.
    pub recipe: Recipe,
    /// For reports and the overlay — "Snare 12.2.00".
    pub name: String,
    /// Snap and **hold** at the peak, rather than fall.
    ///
    /// Released by the next trigger to fire or the next cue to be
    /// taken — never by a note-off from the source, so it cannot get
    /// stuck the way a held flash key can (see `bump.rs`). A figure
    /// then reads as the stage carved and *left* carved until the next
    /// moment moves the carve along.
    // r[impl triggers.hold]
    #[serde(default)]
    pub hold: bool,
}

impl Trigger {
    /// The bar the bus fires this at: the resolved bar, or an absolute
    /// `at`. A relative position nobody has resolved never fires.
    // r[impl song.relative-position.resolved-on-load]
    pub fn bars(&self) -> Option<Bars> {
        self.resolved.or(match &self.at {
            Position::Absolute(bars) => Some(*bars),
            _ => None,
        })
    }

    /// Resolves `at` against a song map; `false` when the map has no
    /// such section.
    pub fn resolve_position(&mut self, song: &SongMap) -> bool {
        match self.at.resolve(song) {
            Some(bars) => {
                self.resolved = Some(bars);
                true
            }
            None => false,
        }
    }
}

/// A trigger that is currently sounding.
#[derive(Debug, Clone, Copy)]
struct Live {
    trigger: usize,
    /// Show time it fired, so its envelope runs from its own start.
    // r[impl triggers.own-clock]
    started: f32,
}

/// The charted triggers for a song, and whichever are ringing.
#[derive(Debug, Clone, Default)]
// r[impl triggers.are-not-cues] - held apart from the cue list, never in the GO order
// r[impl files.show.triggers]
pub struct TriggerBus {
    triggers: Vec<Trigger>,
    live: Vec<Live>,
    /// Where the playhead was last seen. `None` before the first update,
    /// and after a seek — see [`TriggerBus::locate`].
    last: Option<Bars>,
}

/// How many triggers may ring at once.
///
/// A hit decays in well under a beat, so a handful is generous. The cap
/// exists because a scrub or a stall could otherwise leave an unbounded
/// list of envelopes that will never be looked at again.
// r[impl triggers.bounded]
const MAX_LIVE: usize = 32;

impl TriggerBus {
    pub fn new(triggers: Vec<Trigger>) -> Self {
        let mut triggers = triggers;
        triggers.sort_by(|a, b| {
            a.bars()
                .partial_cmp(&b.bars())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self {
            triggers,
            live: Vec::new(),
            last: None,
        }
    }

    pub fn triggers(&self) -> &[Trigger] {
        &self.triggers
    }

    pub fn is_empty(&self) -> bool {
        self.triggers.is_empty()
    }

    /// Moves the playhead without firing anything.
    ///
    /// What a seek does. Scrubbing across a chorus must not fire forty
    /// snares at once — a jump is not a performance of the span it
    /// crossed, and treating it as one is how a scrub becomes a strobe.
    // r[impl triggers.seek-locates]
    // r[impl triggers.locate-clears]
    // r[impl song.transport.seek-is-a-locate]
    pub fn locate(&mut self, position: Bars) {
        self.last = Some(position);
        self.live.clear();
    }

    /// Advances the playhead, firing whatever it passes.
    ///
    /// Nothing fires unless the position actually moves forward, so a
    /// stopped transport is silent for free rather than by a check
    /// somebody has to remember to write.
    // r[impl triggers.crossing-fires] - half-open window (previous, position]
    // r[impl triggers.stopped-fires-nothing]
    // r[impl song.transport.stopped-fires-nothing]
    // r[impl triggers.seek-locates] - a backwards move is a locate
    // r[impl triggers.locate-clears]
    // r[impl triggers.one-sweep-many]
    // r[impl triggers.own-clock] - each firing stamps its own start
    // r[impl triggers.bounded] - oldest dropped first
    pub fn advance(&mut self, position: Bars, clock: f32) {
        let Some(previous) = self.last else {
            // First sight of the playhead is a locate, not a sweep from
            // the top of the song.
            self.locate(position);
            return;
        };
        self.last = Some(position);

        if position <= previous {
            // Stopped, or jumped backwards. A backwards jump is a seek
            // even when it arrives through `advance`, because there is
            // no such thing as playing a song in reverse here.
            if position < previous {
                self.live.clear();
            }
            return;
        }

        let before = self.live.len();
        for (index, trigger) in self.triggers.iter().enumerate() {
            // Half-open: a trigger exactly on the previous position
            // already fired last frame, and one exactly on the new
            // position fires now. Closed at both ends it would double.
            let Some(at) = trigger.bars() else {
                continue;
            };
            if at > previous && at <= position {
                self.live.push(Live {
                    trigger: index,
                    started: clock,
                });
            }
        }
        if self.live.len() > before {
            // Something fired: whatever was being held is released.
            // Hits landing together in one sweep are one moment and
            // keep each other — a cutout is a cut and a lift at once.
            // r[impl triggers.hold.released-by-next]
            let mut fired = self.live.split_off(before);
            self.live.retain(|l| !self.triggers[l.trigger].hold);
            self.live.append(&mut fired);
        }
        while self.live.len() > MAX_LIVE {
            self.live.remove(0);
        }
    }

    /// Releases every held trigger — what taking a cue does.
    // r[impl triggers.hold.released-by-cue]
    pub fn release(&mut self) {
        let triggers = &self.triggers;
        self.live.retain(|l| !triggers[l.trigger].hold);
    }

    /// The time to evaluate a ringing trigger's envelope at: its age,
    /// unless it holds — then pinned inside its first step, so the
    /// snap's value is what shows for as long as it rings.
    fn envelope_time(&self, trigger: &Trigger, age: f32, show: &Show<'_>) -> f32 {
        if !trigger.hold {
            return age;
        }
        let total: f32 = trigger.recipe.steps.iter().map(|s| s.width).sum();
        let first = trigger.recipe.steps.first().map_or(1.0, |s| s.width);
        if total <= 0.0 {
            return age;
        }
        // Halfway through the first step: past any transition into it,
        // before the move out of it.
        let peak = 0.5 * first / total;
        let cycles = trigger.recipe.timing.cycles(age, show.speeds);
        if cycles > peak && cycles > 0.0 {
            age * peak / cycles
        } else {
            age
        }
    }

    /// What the ringing triggers are contributing, as relative values.
    ///
    /// Summed across triggers, deliberately and unlike the cue cascade's
    /// last-wins: two hits landing together are two hits, and a kick
    /// under a crash should read as both. Last-wins is for two sources
    /// disagreeing about one value; this is one source firing twice.
    // r[impl triggers.simultaneous-sum]
    // r[impl playback.triggers-sum]
    // r[impl triggers.own-clock] - envelope age is clock minus the firing's own start
    pub fn output(&self, show: &Show<'_>, clock: f32) -> HashMap<(ChanId, Attribute), f32> {
        let mut out: HashMap<(ChanId, Attribute), f32> = HashMap::new();
        for live in &self.live {
            let Some(trigger) = self.triggers.get(live.trigger) else {
                continue;
            };
            let age = clock - live.started;
            if age < 0.0 {
                // The clock has been moved behind this hit's start — a
                // locate that arrived through the wrong door. Nothing
                // to show; `retire` drops it.
                continue;
            }
            let at = self.envelope_time(trigger, age, show);
            for Emit {
                value, relative, ..
            } in expand_recipe(&trigger.recipe, show, at)
            {
                if !relative {
                    continue;
                }
                *out.entry((value.chan, value.attr)).or_insert(0.0) += value.value;
            }
        }
        out
    }

    /// Drops triggers whose envelope has finished.
    ///
    /// Called after `output` rather than inside it, so the pass that
    /// reads is not the pass that mutates and a caller may render the
    /// same frame twice without changing it.
    // r[impl triggers.retire]
    pub fn retire(&mut self, show: &Show<'_>, clock: f32) {
        let triggers = &self.triggers;
        self.live.retain(|live| {
            let age = clock - live.started;
            age >= 0.0
                && triggers
                    .get(live.trigger)
                    // A held trigger is released, never retired.
                    .is_some_and(|t| t.hold || t.recipe.timing.cycles(age, show.speeds) < 1.0)
        });
    }

    /// How many triggers are ringing — for the overlay.
    // r[impl triggers.visible] - the count; the last-fired name is not yet exposed
    pub fn live_count(&self) -> usize {
        self.live.len()
    }

    /// Index of the most recently fired trigger, while it rings.
    pub fn last_fired_index(&self) -> Option<usize> {
        self.live.last().map(|l| l.trigger)
    }

    /// The most recently fired trigger's name, for the overlay.
    // r[impl triggers.visible] - the last fired name
    pub fn last_fired(&self) -> Option<&str> {
        self.live
            .last()
            .and_then(|l| self.triggers.get(l.trigger))
            .map(|t| t.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Attribute;
    use crate::group::Group;
    use crate::recipe::RecipeApply;
    use crate::selection::{EMPTY_RIG, Selection};
    use crate::step::{Speed, Step, Timing};

    fn pars() -> Vec<Group> {
        vec![Group {
            name: "Pars".into(),
            chans: vec![1, 2],
        }]
    }

    fn bump(level: f32) -> Recipe {
        Recipe {
            target: Selection::Group("Pars".into()),
            steps: vec![
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, level)])]),
                Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])]),
            ],
            timing: Timing {
                speed: Speed::Bpm(60.0),
                measure: 1.0,
                once: true,
                ..Default::default()
            },
            tricks: Vec::new(),
            stack: false,
            ..Default::default()
        }
    }

    fn trigger(bar: u32, beat: f64, level: f32) -> Trigger {
        Trigger {
            at: Bars::new(bar, beat).into(),
            resolved: None,
            recipe: bump(level),
            name: format!("hit {bar}.{beat}"),
            hold: false,
        }
    }

    fn held(bar: u32, beat: f64, level: f32) -> Trigger {
        Trigger {
            hold: true,
            ..trigger(bar, beat, level)
        }
    }

    /// A held hit stays at its peak long after its envelope would have
    /// fallen, until the next hit releases it.
    ///
    /// r[verify triggers.hold]
    /// r[verify triggers.hold.released-by-next]
    #[test]
    fn a_held_hit_stays_up_until_the_next_one() {
        let groups = pars();
        let show = Show::new(&groups, &EMPTY_RIG);
        let mut bus = TriggerBus::new(vec![held(1, 2.0, 0.5), held(2, 2.0, 0.3)]);
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 0.0);
        let peak = bus.output(&show, 0.01)[&(1, Attribute::Dimmer)];
        // Ten seconds on — many envelopes' worth — still at the peak.
        bus.retire(&show, 10.0);
        let later = bus.output(&show, 10.0)[&(1, Attribute::Dimmer)];
        assert!(
            (later - peak).abs() < 1e-4,
            "held hit fell: {peak} then {later}"
        );
        // The next hit takes over outright.
        bus.advance(Bars::new(2, 2.5), 10.0);
        let next = bus.output(&show, 10.01)[&(1, Attribute::Dimmer)];
        assert!(
            (next - 0.3).abs() < 0.05,
            "previous hold not released: {next}"
        );
        assert_eq!(bus.live_count(), 1);
    }

    /// A cue releases a hold.
    ///
    /// r[verify triggers.hold.released-by-cue]
    #[test]
    fn a_cue_releases_a_held_hit() {
        let groups = pars();
        let show = Show::new(&groups, &EMPTY_RIG);
        let mut bus = TriggerBus::new(vec![held(1, 2.0, 0.5)]);
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 0.0);
        bus.release();
        assert!(bus.output(&show, 0.5).is_empty());
        assert_eq!(bus.live_count(), 0);
    }

    /// Two held triggers fired in one sweep keep each other — a cutout
    /// is a cut and a lift at once.
    #[test]
    fn hits_fired_together_hold_together() {
        let mut bus = TriggerBus::new(vec![held(1, 2.0, 0.5), held(1, 2.0, 0.5)]);
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 0.0);
        assert_eq!(bus.live_count(), 2);
    }

    fn bus() -> TriggerBus {
        TriggerBus::new(vec![
            trigger(1, 2.0, 0.5),
            trigger(1, 4.0, 0.5),
            trigger(2, 2.0, 0.5),
        ])
    }

    /// The core promise: the playhead passing a trigger fires it.
    /// r[verify triggers.crossing-fires]
    #[test]
    fn crossing_a_trigger_fires_it() {
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 0.0);
        assert_eq!(bus.live_count(), 1);
    }

    /// A stopped transport reports the same position every frame, and
    /// must fire nothing. This is the whole reason triggers exist rather
    /// than a phaser: no signal, no light.
    /// r[verify triggers.stopped-fires-nothing]
    /// r[verify song.transport.stopped-fires-nothing]
    #[test]
    fn a_stopped_playhead_fires_nothing() {
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 0.0);
        let after_first = bus.live_count();
        for frame in 1..20 {
            bus.advance(Bars::new(1, 2.5), frame as f32 * 0.016);
        }
        assert_eq!(bus.live_count(), after_first, "a still playhead re-fired");
    }

    /// A trigger fires once, not once per frame it stays behind the
    /// playhead.
    /// r[verify triggers.crossing-fires] - exactly once per crossing
    #[test]
    fn a_trigger_fires_exactly_once() {
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 0.0);
        bus.advance(Bars::new(1, 3.0), 0.1);
        bus.advance(Bars::new(1, 3.5), 0.2);
        assert_eq!(bus.live_count(), 1);
    }

    /// Seeking across a span must not perform it. Scrubbing over a
    /// chorus firing forty snares at once is a strobe, not a preview.
    /// r[verify triggers.seek-locates]
    /// r[verify song.transport.seek-is-a-locate]
    #[test]
    fn a_seek_does_not_fire_what_it_skipped() {
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.locate(Bars::bar(3));
        assert_eq!(bus.live_count(), 0);
    }

    /// Jumping backwards is a seek even when it arrives as an advance —
    /// there is no playing a song in reverse.
    /// r[verify triggers.seek-locates]
    /// r[verify triggers.locate-clears]
    #[test]
    fn moving_backwards_clears_rather_than_firing() {
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 0.0);
        assert_eq!(bus.live_count(), 1);
        bus.advance(Bars::bar(1), 0.1);
        assert_eq!(bus.live_count(), 0);
    }

    /// One sweep can cross several triggers, and must fire all of them —
    /// a frame is 16 ms and a fill can put two hits inside one.
    /// r[verify triggers.one-sweep-many]
    #[test]
    fn one_advance_can_fire_several() {
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(2, 3.0), 0.0);
        assert_eq!(bus.live_count(), 3);
    }

    /// Two triggers together sum, unlike the cue cascade's last-wins: a
    /// kick under a crash is both, not the louder of the two.
    /// r[verify triggers.simultaneous-sum]
    /// r[verify playback.triggers-sum]
    #[test]
    fn simultaneous_triggers_sum() {
        let groups = pars();
        let show = Show::new(&groups, &EMPTY_RIG);
        let mut bus = TriggerBus::new(vec![trigger(1, 2.0, 0.3), trigger(1, 2.0, 0.4)]);
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 0.0);
        let out = bus.output(&show, 0.0);
        let value = out[&(1, Attribute::Dimmer)];
        assert!((value - 0.7).abs() < 1e-4, "expected 0.3+0.4: {value}");
    }

    /// A hit the clock has been moved behind is silent and retired,
    /// rather than ringing until the clock catches up.
    ///
    /// r[verify triggers.retire]
    #[test]
    fn a_hit_behind_the_clock_is_silent_and_retired() {
        let groups = pars();
        let show = Show::new(&groups, &EMPTY_RIG);
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 61.0);
        assert!(bus.output(&show, 5.0).is_empty(), "rang at a negative age");
        bus.retire(&show, 5.0);
        assert_eq!(bus.live_count(), 0);
    }

    /// A finished envelope is dropped, so the bus does not grow across a
    /// three-minute song.
    /// r[verify triggers.retire]
    #[test]
    fn finished_triggers_retire() {
        let groups = pars();
        let show = Show::new(&groups, &EMPTY_RIG);
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(1, 2.5), 0.0);
        assert_eq!(bus.live_count(), 1);
        bus.retire(&show, 10.0);
        assert_eq!(bus.live_count(), 0);
    }
}
