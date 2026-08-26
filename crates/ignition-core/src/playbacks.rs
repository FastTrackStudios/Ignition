//! Several cue players at once, each in a class of the stack.
//!
//! One song list was enough for one night. It is not enough for a show
//! that has a *look* list the operator steps through, a *movers* list
//! that positions the rig independently, and a *song* list the chart
//! drives — three things that change at three different rates, and that
//! an operator wants to take separately. This is the fold that lets
//! them coexist: classes decide who wins between lists, and inside a
//! class the dimmer takes the highest and everything else the latest.
//!
//! Deliberately independent of the visualizer: the wiring that hands a
//! `Playbacks` to the viz loop in place of one `CuePlayer` is a separate
//! change, and this module has to be testable without it.

use crate::cue::CuePlayer;
use crate::recipe::Show;
use ignition_proto::{Attribute, ChanId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The priority class a playback runs in. Declared in ascending order:
/// a later variant beats an earlier one on every attribute it asserts.
///
/// `Show` is the base — the generic rig look. `Look` steps over it,
/// `Movers` places the rig over both, and `Song` is the chart, which
/// sits on top because it is the most specific thing anyone wrote for
/// tonight. There is no numeric priority; the class *is* the priority,
/// per `r[playback.playbacks-have-priority]`.
// r[impl playback.playbacks-have-priority] - the class is the priority; nothing numeric
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Class {
    Show,
    Look,
    Movers,
    Song,
}

/// One cue player and where it sits.
pub struct Playback {
    pub class: Class,
    pub player: CuePlayer,
    /// A disabled playback is folded as though it were not there.
    pub enabled: bool,
}

/// Every cue player the engine is running.
// r[impl playback.several-players]
#[derive(Default)]
pub struct Playbacks {
    pub entries: Vec<Playback>,
}

impl Playbacks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a player and returns its index.
    pub fn push(&mut self, class: Class, player: CuePlayer) -> usize {
        self.entries.push(Playback {
            class,
            player,
            enabled: true,
        });
        self.entries.len() - 1
    }

    pub fn get(&self, index: usize) -> Option<&Playback> {
        self.entries.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Playback> {
        self.entries.get_mut(index)
    }

    /// The first enabled player of a class, if any — where "GO on the
    /// song list" lands.
    pub fn of_class(&mut self, class: Class) -> Option<&mut CuePlayer> {
        self.entries
            .iter_mut()
            .find(|p| p.class == class && p.enabled)
            .map(|p| &mut p.player)
    }

    /// Advances every player's clock by the same frame.
    pub fn tick(&mut self, dt_secs: f32) {
        for entry in &mut self.entries {
            entry.player.tick(dt_secs);
        }
    }

    /// Sets every player's clock — the shared show clock.
    pub fn set_clock(&mut self, secs: f32) {
        for entry in &mut self.entries {
            entry.player.set_clock(secs);
        }
    }

    /// The folded output of every enabled player.
    ///
    /// Lower classes first, so a higher class overwrites; within a
    /// class, later entries overwrite — except the dimmer, which takes
    /// the highest of the class. A key a higher entry does not assert
    /// falls through to whatever beneath it did, and to nothing at all
    /// if no entry has it: nothing is left holding a value from a
    /// source that has gone.
    ///
    /// `ringing` is the trigger bus's transient keys, handed to each
    /// player as `output_under` does for one.
    // r[impl playback.playbacks-have-priority] - a higher class always beats a lower one
    // r[impl playback.dimmer-htp-between-equals]
    // r[impl playback.release-falls-through] - an absent key falls to the next entry
    // r[impl playback.output-is-pure]
    // r[impl playback.no-merge-at-dmx] - merged in attribute space
    pub fn output(
        &self,
        show: &Show<'_>,
        ringing: &HashSet<(ChanId, Attribute)>,
    ) -> HashMap<(ChanId, Attribute), f32> {
        let mut order: Vec<&Playback> = self.entries.iter().filter(|p| p.enabled).collect();
        // Stable, so two entries of one class keep their declaration
        // order — that is what "later wins" means here.
        order.sort_by_key(|p| p.class);

        let mut out: HashMap<(ChanId, Attribute), f32> = HashMap::new();
        let mut holder: HashMap<(ChanId, Attribute), Class> = HashMap::new();
        for entry in order {
            for (key, value) in entry.player.output_under(show, ringing) {
                let same_class = holder.get(&key) == Some(&entry.class);
                let value = match out.get(&key) {
                    Some(under) if same_class && key.1 == Attribute::Dimmer => under.max(value),
                    _ => value,
                };
                out.insert(key.clone(), value);
                holder.insert(key, entry.class);
            }
        }
        out
    }

    /// Which class won a key in the last fold, and what every enabled
    /// entry would have produced for it, lowest class first — for
    /// answering "why was that fixture at 0.3".
    // r[impl playback.inspectable] - per key, across players
    pub fn inspect(
        &self,
        show: &Show<'_>,
        ringing: &HashSet<(ChanId, Attribute)>,
        key: &(ChanId, Attribute),
    ) -> Vec<(Class, Option<f32>)> {
        let mut order: Vec<&Playback> = self.entries.iter().filter(|p| p.enabled).collect();
        order.sort_by_key(|p| p.class);
        order
            .into_iter()
            .map(|p| {
                (
                    p.class,
                    p.player.output_under(show, ringing).get(key).copied(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cue::{Cue, CueValue};
    use crate::group::Group;
    use crate::selection::EMPTY_RIG;

    fn show(groups: &[Group]) -> Show<'_> {
        Show::new(groups, &EMPTY_RIG)
    }

    fn cue(values: &[(ChanId, Attribute, f32)]) -> Cue {
        Cue {
            name: "cue".into(),
            values: values
                .iter()
                .map(|(chan, attr, value)| CueValue {
                    chan: *chan,
                    attr: attr.clone(),
                    value: *value,
                })
                .collect(),
            ..Default::default()
        }
    }

    fn player(cues: Vec<Cue>, show: &Show<'_>) -> CuePlayer {
        let mut p = CuePlayer::new(cues);
        p.go(show);
        p
    }

    fn dimmer(chan: ChanId, v: f32) -> (ChanId, Attribute, f32) {
        (chan, Attribute::Dimmer, v)
    }

    fn pan(chan: ChanId, v: f32) -> (ChanId, Attribute, f32) {
        (chan, Attribute::Pan, v)
    }

    /// r[verify playback.several-players]
    /// r[verify playback.playbacks-have-priority]
    #[test]
    fn a_higher_class_beats_a_lower_one_on_every_attribute() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(
            Class::Song,
            player(vec![cue(&[dimmer(1, 0.2), pan(1, 10.0)])], &show),
        );
        pb.push(
            Class::Show,
            player(vec![cue(&[dimmer(1, 0.9), pan(1, 50.0)])], &show),
        );
        let out = pb.output(&show, &HashSet::new());
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.2, "not HTP across classes");
        assert_eq!(out[&(1, Attribute::Pan)], 10.0);
    }

    /// r[verify playback.dimmer-htp-between-equals]
    #[test]
    fn within_a_class_dimmer_is_htp_and_everything_else_ltp() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(
            Class::Look,
            player(vec![cue(&[dimmer(1, 0.9), pan(1, 10.0)])], &show),
        );
        pb.push(
            Class::Look,
            player(vec![cue(&[dimmer(1, 0.4), pan(1, 50.0)])], &show),
        );
        let out = pb.output(&show, &HashSet::new());
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.9, "highest dimmer");
        assert_eq!(out[&(1, Attribute::Pan)], 50.0, "latest pan");
    }

    /// r[verify playback.release-falls-through]
    #[test]
    fn a_key_the_higher_player_does_not_assert_falls_through() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(
            Class::Show,
            player(vec![cue(&[dimmer(1, 0.5), dimmer(2, 0.7)])], &show),
        );
        pb.push(Class::Song, player(vec![cue(&[dimmer(1, 1.0)])], &show));
        let out = pb.output(&show, &HashSet::new());
        assert_eq!(out[&(1, Attribute::Dimmer)], 1.0);
        assert_eq!(out[&(2, Attribute::Dimmer)], 0.7, "fell through to Show");
    }

    /// When the top player releases a key entirely, nothing is left
    /// holding its value: the key comes from the next player or not at
    /// all.
    /// r[verify playback.release-falls-through]
    #[test]
    fn disabling_a_player_releases_everything_it_held() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(Class::Show, player(vec![cue(&[dimmer(1, 0.5)])], &show));
        let song = pb.push(
            Class::Song,
            player(vec![cue(&[dimmer(1, 1.0), dimmer(3, 1.0)])], &show),
        );
        pb.get_mut(song).unwrap().enabled = false;
        let out = pb.output(&show, &HashSet::new());
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.5);
        assert!(
            !out.contains_key(&(3, Attribute::Dimmer)),
            "nothing holds it"
        );
    }

    /// r[verify playback.inspectable]
    #[test]
    fn inspect_lists_every_players_answer_lowest_class_first() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(Class::Song, player(vec![cue(&[dimmer(1, 1.0)])], &show));
        pb.push(Class::Show, player(vec![cue(&[dimmer(1, 0.5)])], &show));
        let key = (1, Attribute::Dimmer);
        assert_eq!(
            pb.inspect(&show, &HashSet::new(), &key),
            vec![(Class::Show, Some(0.5)), (Class::Song, Some(1.0))]
        );
    }
}
