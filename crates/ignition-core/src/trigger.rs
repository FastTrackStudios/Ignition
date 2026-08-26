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

use crate::{Attribute, ChanId};
use crate::music::Bars;
use crate::recipe::{Emit, Recipe, Show, expand_recipe};

use std::collections::HashMap;

/// One charted event, and what it does to the rig.
#[derive(Debug, Clone, PartialEq)]
pub struct Trigger {
    /// Where in the song it happens.
    pub at: Bars,
    /// What it fires. Should be relative and one-shot: a trigger adds to
    /// the look for a moment and gets out of the way, and a looping
    /// recipe here would never stop.
    pub recipe: Recipe,
    /// For reports and the overlay — "Snare 12.2.00".
    pub name: String,
}

/// A trigger that is currently sounding.
#[derive(Debug, Clone, Copy)]
struct Live {
    trigger: usize,
    /// Show time it fired, so its envelope runs from its own start.
    started: f32,
}

/// The charted triggers for a song, and whichever are ringing.
#[derive(Debug, Clone, Default)]
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
const MAX_LIVE: usize = 32;

impl TriggerBus {
    pub fn new(triggers: Vec<Trigger>) -> Self {
        let mut triggers = triggers;
        triggers.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
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
    pub fn locate(&mut self, position: Bars) {
        self.last = Some(position);
        self.live.clear();
    }

    /// Advances the playhead, firing whatever it passes.
    ///
    /// Nothing fires unless the position actually moves forward, so a
    /// stopped transport is silent for free rather than by a check
    /// somebody has to remember to write.
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

        for (index, trigger) in self.triggers.iter().enumerate() {
            // Half-open: a trigger exactly on the previous position
            // already fired last frame, and one exactly on the new
            // position fires now. Closed at both ends it would double.
            if trigger.at > previous && trigger.at <= position {
                self.live.push(Live {
                    trigger: index,
                    started: clock,
                });
            }
        }
        while self.live.len() > MAX_LIVE {
            self.live.remove(0);
        }
    }

    /// What the ringing triggers are contributing, as relative values.
    ///
    /// Summed across triggers, deliberately and unlike the cue cascade's
    /// last-wins: two hits landing together are two hits, and a kick
    /// under a crash should read as both. Last-wins is for two sources
    /// disagreeing about one value; this is one source firing twice.
    pub fn output(&self, show: &Show<'_>, clock: f32) -> HashMap<(ChanId, Attribute), f32> {
        let mut out: HashMap<(ChanId, Attribute), f32> = HashMap::new();
        for live in &self.live {
            let Some(trigger) = self.triggers.get(live.trigger) else {
                continue;
            };
            let age = clock - live.started;
            for Emit { value, relative } in expand_recipe(&trigger.recipe, show, age) {
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
    pub fn retire(&mut self, show: &Show<'_>, clock: f32) {
        let triggers = &self.triggers;
        self.live.retain(|live| {
            triggers.get(live.trigger).is_some_and(|t| {
                t.recipe.timing.cycles(clock - live.started, show.speeds) < 1.0
            })
        });
    }

    /// How many triggers are ringing — for the overlay.
    pub fn live_count(&self) -> usize {
        self.live.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::Group;
    use crate::recipe::RecipeApply;
    use crate::selection::{EMPTY_RIG, Selection};
    use crate::step::{Speed, Step, Timing};
    use crate::Attribute;

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
        }
    }

    fn trigger(bar: u32, beat: f64, level: f32) -> Trigger {
        Trigger {
            at: Bars::new(bar, beat),
            recipe: bump(level),
            name: format!("hit {bar}.{beat}"),
        }
    }

    fn bus() -> TriggerBus {
        TriggerBus::new(vec![
            trigger(1, 2.0, 0.5),
            trigger(1, 4.0, 0.5),
            trigger(2, 2.0, 0.5),
        ])
    }

    /// The core promise: the playhead passing a trigger fires it.
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
    #[test]
    fn a_seek_does_not_fire_what_it_skipped() {
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.locate(Bars::bar(3));
        assert_eq!(bus.live_count(), 0);
    }

    /// Jumping backwards is a seek even when it arrives as an advance —
    /// there is no playing a song in reverse.
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
    #[test]
    fn one_advance_can_fire_several() {
        let mut bus = bus();
        bus.locate(Bars::bar(1));
        bus.advance(Bars::new(2, 3.0), 0.0);
        assert_eq!(bus.live_count(), 3);
    }

    /// Two triggers together sum, unlike the cue cascade's last-wins: a
    /// kick under a crash is both, not the louder of the two.
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

    /// A finished envelope is dropped, so the bus does not grow across a
    /// three-minute song.
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
