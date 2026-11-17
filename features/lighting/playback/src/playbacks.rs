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
use crate::programmer::Transport;
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
    /// This playback's own intensity master, 0..=1: every dimmer it
    /// produces is scaled by it before the fold, so the song list can be
    /// pulled under a look list without touching either's content.
    /// Dimmer only, like every master.
    // r[impl playback.playback-master]
    pub master: f32,
}

impl Playback {
    /// This entry's output with its master applied.
    // r[impl playback.playback-master] - scaled per entry, inside the fold
    fn scaled(
        &self,
        mut out: HashMap<(ChanId, Attribute), f32>,
    ) -> HashMap<(ChanId, Attribute), f32> {
        let master = self.master.clamp(0.0, 1.0);
        if master < 1.0 {
            for ((_, attr), value) in &mut out {
                if *attr == Attribute::Dimmer {
                    *value *= master;
                }
            }
        }
        out
    }
}

/// Every cue player the engine is running.
// r[impl playback.several-players]
#[derive(Default)]
pub struct Playbacks {
    pub entries: Vec<Playback>,
}

impl Playbacks {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a player and returns its index.
    pub fn push(&mut self, class: Class, player: CuePlayer) -> usize {
        let index = self.entries.len();
        self.entries.push(Playback {
            class,
            player,
            enabled: true,
            master: 1.0,
        });
        index
    }

    /// Sets an entry's intensity master, 0..=1.
    // r[impl playback.playback-master]
    pub fn set_master(&mut self, index: usize, level: f32) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.master = level.clamp(0.0, 1.0);
        }
    }

    /// The first enabled Song playback — where the transport keys land.
    pub fn song_mut(&mut self) -> Option<&mut Playback> {
        self.entries
            .iter_mut()
            .find(|p| p.class == Class::Song && p.enabled)
    }

    /// Carries out a transport request a key made (see
    /// `Programmer::key_down`) on the Song playback. Pause toggles:
    /// pressed once the list holds its place, pressed again it resumes
    /// from there.
    // r[impl playback.temp-and-pause] - pause / resume / go back on the Song list
    pub fn transport(&mut self, request: Transport, show: &Show<'_>) {
        let Some(song) = self.song_mut() else {
            return;
        };
        match request {
            Transport::TogglePause => {
                if song.player.is_paused() {
                    song.player.resume();
                } else {
                    song.player.pause();
                }
            }
            Transport::Load(index) => song.player.load(index),
            Transport::GoBack => song.player.go_back(show),
        }
    }

    #[must_use]
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
    #[must_use]
    pub fn output(
        &self,
        show: &Show<'_>,
        ringing: &HashSet<(ChanId, Attribute)>,
    ) -> HashMap<(ChanId, Attribute), f32> {
        let mut order: Vec<&Playback> = self.entries.iter().filter(|p| p.enabled).collect();
        // Stable, so two entries of one class keep their declaration
        // order — that is what "later wins" means here.
        order.sort_by_key(|p| p.class);

        self.fold(|entry| entry.scaled(entry.player.output_under(show, ringing)))
    }

    /// `output`, with every attribute nothing asserts falling to its
    /// default — the floor a released attribute lands on, and what a
    /// list's implicit cue zero establishes before its first cue.
    ///
    /// Each player is asked through `CuePlayer::output_with_defaults`,
    /// so a fade *out of* a cue lands on the default rather than on
    /// zero; then any default no player touched is filled in. `ringing`
    /// is accepted for symmetry with `output` but the per-player call
    /// does not take it: a hit lands over the defaults on the trigger
    /// bus above this fold. The host
    /// supplies `defaults` from the fixture types — the visualizer
    /// builds them from each fixture's profile, since only it knows
    /// what a spot's rest zoom is.
    // r[impl playback.defaults] - the floor, and cue zero
    #[must_use]
    pub fn output_with_defaults(
        &self,
        show: &Show<'_>,
        _ringing: &HashSet<(ChanId, Attribute)>,
        defaults: &HashMap<(ChanId, Attribute), f32>,
    ) -> HashMap<(ChanId, Attribute), f32> {
        let mut out = self.fold(|entry| {
            let raw = entry.player.output_with_defaults(show, defaults);
            entry.scaled(raw)
        });
        for (key, value) in defaults {
            out.entry(key.clone()).or_insert(*value);
        }
        out
    }

    /// The class fold over every enabled entry, given how to read one.
    fn fold(
        &self,
        read: impl Fn(&Playback) -> HashMap<(ChanId, Attribute), f32>,
    ) -> HashMap<(ChanId, Attribute), f32> {
        let mut order: Vec<&Playback> = self.entries.iter().filter(|p| p.enabled).collect();
        // Stable, so two entries of one class keep their declaration
        // order — that is what "later wins" means here.
        order.sort_by_key(|p| p.class);

        let mut out: HashMap<(ChanId, Attribute), f32> = HashMap::new();
        let mut holder: HashMap<(ChanId, Attribute), Class> = HashMap::new();
        for entry in order {
            for (key, value) in read(entry) {
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
    #[must_use]
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
                    p.scaled(p.player.output_under(show, ringing))
                        .get(key)
                        .copied(),
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
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.2).abs() < 1e-6,
            "not HTP across classes"
        );
        assert!((out[&(1, Attribute::Pan)] - 10.0).abs() < 1e-6);
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
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.9).abs() < 1e-6,
            "highest dimmer"
        );
        assert!(
            (out[&(1, Attribute::Pan)] - 50.0).abs() < 1e-6,
            "latest pan"
        );
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
        assert!((out[&(1, Attribute::Dimmer)] - 1.0).abs() < 1e-6);
        assert!(
            (out[&(2, Attribute::Dimmer)] - 0.7).abs() < 1e-6,
            "fell through to Show"
        );
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
        assert!((out[&(1, Attribute::Dimmer)] - 0.5).abs() < 1e-6);
        assert!(
            !out.contains_key(&(3, Attribute::Dimmer)),
            "nothing holds it"
        );
    }

    /// r[verify playback.playback-master]
    #[test]
    fn a_playback_master_scales_only_that_entrys_intensity() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        let look = pb.push(
            Class::Look,
            player(vec![cue(&[dimmer(1, 1.0), pan(1, 10.0)])], &show),
        );
        pb.push(Class::Show, player(vec![cue(&[dimmer(2, 1.0)])], &show));
        pb.set_master(look, 0.5);
        let out = pb.output(&show, &HashSet::new());
        assert!((out[&(1, Attribute::Dimmer)] - 0.5).abs() < 1e-6);
        assert!(
            (out[&(1, Attribute::Pan)] - 10.0).abs() < 1e-6,
            "dimmer only"
        );
        assert!(
            (out[&(2, Attribute::Dimmer)] - 1.0).abs() < 1e-6,
            "the other list untouched"
        );
    }

    /// A master is applied before the fold, so a Song list pulled to
    /// nothing lets the Look list beneath it show — the master is on the
    /// entry's contribution, not on the merged output.
    /// r[verify playback.playback-master]
    #[test]
    fn a_playback_master_is_applied_inside_the_fold() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(Class::Look, player(vec![cue(&[dimmer(1, 0.6)])], &show));
        let a = pb.push(Class::Look, player(vec![cue(&[dimmer(1, 1.0)])], &show));
        pb.set_master(a, 0.2);
        let out = pb.output(&show, &HashSet::new());
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.6).abs() < 1e-6,
            "HTP within the class sees the scaled value"
        );
    }

    /// r[verify playback.temp-and-pause] - pause, resume, go back
    #[test]
    fn transport_keys_land_on_the_song_list() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(Class::Look, player(vec![cue(&[dimmer(9, 1.0)])], &show));
        pb.push(
            Class::Song,
            player(
                vec![
                    cue(&[dimmer(1, 0.2)]),
                    cue(&[dimmer(1, 0.5)]),
                    cue(&[dimmer(1, 0.9)]),
                ],
                &show,
            ),
        );
        pb.song_mut().unwrap().player.go(&show);
        assert_eq!(pb.song_mut().unwrap().player.current_index(), Some(1));

        pb.transport(Transport::GoBack, &show);
        assert_eq!(
            pb.song_mut().unwrap().player.current_index(),
            Some(0),
            "went back one"
        );
        assert_eq!(
            pb.get(0).unwrap().player.current_index(),
            Some(0),
            "the look list did not move"
        );

        pb.transport(Transport::TogglePause, &show);
        assert!(pb.song_mut().unwrap().player.is_paused());
        pb.transport(Transport::TogglePause, &show);
        assert!(
            !pb.song_mut().unwrap().player.is_paused(),
            "pressed again, it resumes"
        );

        pb.transport(Transport::Load(2), &show);
        pb.song_mut().unwrap().player.go(&show);
        assert_eq!(
            pb.song_mut().unwrap().player.current_index(),
            Some(2),
            "GO after load goes to the loaded cue"
        );
    }

    #[test]
    fn transport_without_a_song_list_is_a_no_op() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(Class::Look, player(vec![cue(&[dimmer(9, 1.0)])], &show));
        pb.transport(Transport::GoBack, &show);
        pb.transport(Transport::TogglePause, &show);
        assert!(pb.song_mut().is_none());
    }

    /// A key nothing sets falls to its default; a key a cue sets is
    /// the cue's.
    /// r[verify playback.defaults]
    #[test]
    fn a_key_nothing_sets_falls_to_its_default() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(Class::Song, player(vec![cue(&[dimmer(1, 0.7)])], &show));
        let defaults = HashMap::from([
            ((1, Attribute::Dimmer), 0.0),
            ((1, Attribute::Zoom), 0.4),
            ((2, Attribute::Zoom), 0.4),
        ]);
        let out = pb.output_with_defaults(&show, &HashSet::new(), &defaults);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.7).abs() < 1e-6,
            "the cue's"
        );
        assert!(
            (out[&(1, Attribute::Zoom)] - 0.4).abs() < 1e-6,
            "the default"
        );
        assert!(
            (out[&(2, Attribute::Zoom)] - 0.4).abs() < 1e-6,
            "a fixture no cue touches"
        );
        assert!(
            !pb.output(&show, &HashSet::new())
                .contains_key(&(1, Attribute::Zoom)),
            "without defaults nothing holds it"
        );
    }

    /// Before the first GO, a list's output is cue zero: every default.
    /// r[verify playback.defaults] - cue zero
    #[test]
    fn cue_zero_is_the_defaults() {
        let groups = Vec::new();
        let show = show(&groups);
        let mut pb = Playbacks::new();
        pb.push(Class::Song, CuePlayer::new(vec![cue(&[dimmer(1, 0.7)])]));
        let defaults = HashMap::from([((1, Attribute::Dimmer), 0.0), ((1, Attribute::Zoom), 0.4)]);
        let out = pb.output_with_defaults(&show, &HashSet::new(), &defaults);
        assert!(out[&(1, Attribute::Dimmer)].abs() < 1e-6);
        assert!((out[&(1, Attribute::Zoom)] - 0.4).abs() < 1e-6);
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

    /// Folding twice at one clock gives one answer, and going back to
    /// an earlier clock gives the earlier answer.
    ///
    /// The output is a pure function of the stack, the rig, the show
    /// clock and the operator's state. That is what makes the stack
    /// inspectable: a frame can be re-resolved offline to answer "why
    /// was that light on" — which is only true if resolving it again
    /// does not change it, and if nothing has cached a value from a
    /// frame that has been and gone.
    ///
    /// r[verify playback.output-is-pure]
    #[test]
    fn folding_the_same_frame_twice_gives_the_same_answer() {
        use crate::step::{Speed, Step, Timing};

        let groups = vec![Group {
            name: "Pars".into(),
            chans: vec![1, 2],
        }];
        let show = show(&groups);

        // Something that moves, so a cached value would show up.
        let chase = Cue {
            name: "chase".into(),
            recipes: vec![
                crate::recipe::Recipe {
                    target: crate::selection::Selection::Group("Pars".into()),
                    steps: vec![
                        Step::new(vec![crate::recipe::RecipeApply::Dimmer(1.0)]),
                        Step::new(vec![crate::recipe::RecipeApply::Dimmer(0.0)]),
                    ],
                    timing: Timing {
                        speed: Speed::Bpm(60.0),
                        measure: 4.0,
                        ..Default::default()
                    },
                    tricks: Vec::new(),
                    stack: false,
                    ..Default::default()
                }
                .into(),
            ],
            ..Default::default()
        };

        let mut pb = Playbacks::default();
        let mut player = CuePlayer::new(vec![chase]);
        player.go(&show);
        // A quarter of the way round: inside the first step.
        player.tick(1.0);
        pb.push(Class::Show, player);

        let ringing = HashSet::new();
        let once = pb.output(&show, &ringing);
        let twice = pb.output(&show, &ringing);
        assert_eq!(once, twice, "resolving the same frame twice moved it");

        // Move on, then come back: the earlier clock resolves to the
        // earlier frame, because nothing kept the later one.
        // Three quarters round, which is the *other* step — so a value
        // held over from the last frame would be visible.
        if let Some(p) = pb.of_class(Class::Show) {
            p.tick(2.0);
        }
        let later = pb.output(&show, &ringing);
        assert_ne!(later, once, "a moving chase did not move");

        if let Some(p) = pb.of_class(Class::Show) {
            p.set_clock(1.0);
        }
        let back = pb.output(&show, &ringing);
        assert_eq!(back, once, "going back to a clock gave a different frame");
    }
}
