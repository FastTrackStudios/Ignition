//! A Live set that is a whole night, read into song maps.
//!
//! A recital or a gig is often one Ableton Live set with every song laid
//! end to end, marked the AbleSet way (see `ableton::setlist`): a locator
//! per song, `SONG END` where it stops, section cards on a `SECTIONS`
//! track. [`LiveSet`] reads that through studio's `ableton` adapter into
//! one [`SongMap`] per song — the same shape a REAPER project's regions
//! become — so a show written against "4 bars into Chorus 2" resolves
//! against the set as it is today (`r[song.map.imported]`).
//!
//! It also answers the question a playhead in the *set* asks: given a
//! beat of the arrangement, which song is that, and where in it. Every
//! song starts on its own bar 1 (its count-in), whatever bar of the set
//! it happens to sit at, and a time-signature change starts a new bar —
//! the same rule a musician counts by.

use crate::{Bars, Section, SongMap, TempoMap, TempoPoint, TimeSignature};
use ableton::AbletonLiveSet;
use ableton::setlist;
use ableton::tempo::TempoMap as LiveTempo;
use anyhow::Result;
use std::path::Path;

/// A beat of the set where the signature changes, and the signature.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Meter {
    at: f64,
    signature: TimeSignature,
}

impl Meter {
    /// Quarter-note beats per bar.
    fn beats_per_bar(self) -> f64 {
        f64::from(self.signature.numerator.max(1)) * 4.0
            / f64::from(self.signature.denominator.max(1))
    }
}

/// One song of the night.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSetSong {
    /// The song's locator, as written (`Heart isn't Ready`).
    pub name: String,
    /// `heart-isnt-ready`.
    pub slug: String,
    /// Where it starts and ends on the set, in beats.
    pub start_beats: f64,
    pub end_beats: f64,
    /// Its shape: tempo and sections, positioned from its own bar 1.
    pub song: SongMap,
    meters: Vec<Meter>,
}

impl LiveSetSong {
    /// Whether a beat of the set falls inside this song.
    #[must_use]
    pub fn contains(&self, set_beat: f64) -> bool {
        set_beat >= self.start_beats && set_beat < self.end_beats
    }

    /// The signature in force at a beat of the set.
    fn meter_at(&self, set_beat: f64) -> Meter {
        self.meters
            .iter()
            .rev()
            .find(|m| m.at <= set_beat + 1e-9)
            .or_else(|| self.meters.first())
            .copied()
            .unwrap_or(Meter {
                at: 0.0,
                signature: TimeSignature::default(),
            })
    }

    /// A beat of the set as a bar and beat of this song (bar 1 at the
    /// song's start). Before the start is bar 1, beat 1.
    #[must_use]
    pub fn position(&self, set_beat: f64) -> Bars {
        let target = set_beat.max(self.start_beats);
        let mut cursor = self.start_beats;
        let mut bar: u32 = 1;
        let changes = self
            .meters
            .iter()
            .map(|m| m.at)
            .filter(|&at| at > self.start_beats + 1e-9 && at <= target + 1e-9);
        for change in changes {
            let per_bar = self.meter_at(cursor).beats_per_bar();
            let span = change - cursor;
            // A partial bar before a change is still a bar.
            bar = bar.saturating_add(whole_bars(span, per_bar, true));
            cursor = change;
        }
        let per_bar = self.meter_at(cursor).beats_per_bar();
        let span = target - cursor;
        let whole = whole_bars(span, per_bar, false);
        let beat = f64::from(whole).mul_add(-per_bar, span) + 1.0;
        Bars::new(bar.saturating_add(whole), beat)
    }
}

/// How many bars of `per_bar` beats fit in `span`; `ceil` counts a
/// partial bar as one.
fn whole_bars(span: f64, per_bar: f64, ceil: bool) -> u32 {
    let ratio = span / per_bar;
    let n = if ceil {
        (ratio - 1e-9).ceil()
    } else {
        (ratio + 1e-9).floor()
    };
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a bar count of a song: non-negative and far below u32::MAX"
    )]
    let n = n.max(0.0) as u32;
    n
}

/// A Live set, read as a night of songs.
pub struct LiveSet {
    tempo: LiveTempo,
    songs: Vec<LiveSetSong>,
}

impl LiveSet {
    /// Reads a `.als` file.
    ///
    /// # Errors
    ///
    /// The file cannot be read or does not parse as a Live set.
    pub fn read(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let set = ableton::read_live_set(path)
            .map_err(|e| anyhow::anyhow!("reading the Live set {}: {e}", path.display()))?;
        Ok(Self::of(&set))
    }

    /// The night in a parsed set.
    #[must_use]
    pub fn of(set: &AbletonLiveSet) -> Self {
        let tempo = LiveTempo::of(set);
        let mut meters: Vec<Meter> = set
            .time_signature_automation
            .iter()
            .map(|(at, ts)| Meter {
                at: *at,
                signature: TimeSignature {
                    numerator: u32::from(ts.numerator),
                    denominator: u32::from(ts.denominator),
                },
            })
            .collect();
        if meters.first().is_none_or(|m| m.at > 0.0) {
            meters.insert(
                0,
                Meter {
                    at: 0.0,
                    signature: TimeSignature {
                        numerator: u32::from(set.time_signature.numerator),
                        denominator: u32::from(set.time_signature.denominator),
                    },
                },
            );
        }
        let cards = setlist::sections(set);
        let songs = setlist::songs(set)
            .into_iter()
            .map(|range| {
                let mut song = LiveSetSong {
                    name: range.name.clone(),
                    slug: range.slug.clone(),
                    start_beats: range.start_beats,
                    end_beats: range.end_beats,
                    song: SongMap {
                        name: range.name.clone(),
                        tempo: TempoMap::default(),
                        sections: Vec::new(),
                    },
                    meters: meters.clone(),
                };
                song.song = song_map(&song, &setlist::sections_of(&cards, &range), &tempo);
                song
            })
            .collect();
        Self { tempo, songs }
    }

    /// Every song of the set, in arrangement order.
    #[must_use]
    pub fn songs(&self) -> &[LiveSetSong] {
        &self.songs
    }

    /// A song by its locator name or slug (case-insensitive).
    #[must_use]
    pub fn song(&self, name: &str) -> Option<&LiveSetSong> {
        let slug = setlist::slug(name);
        self.songs
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name) || s.slug == slug)
    }

    /// The song a beat of the set falls in, if any.
    #[must_use]
    pub fn song_at(&self, set_beat: f64) -> Option<&LiveSetSong> {
        self.songs.iter().find(|s| s.contains(set_beat))
    }

    /// Seconds from the top of the set at a beat.
    #[must_use]
    pub fn seconds_at(&self, set_beat: f64) -> f64 {
        self.tempo.seconds_at(set_beat)
    }

    /// The tempo at a beat of the set.
    #[must_use]
    pub fn bpm_at(&self, set_beat: f64) -> f64 {
        self.tempo.bpm_at(set_beat)
    }

    /// The beat of the set at a time from its top — the inverse of
    /// [`Self::seconds_at`], by bisection (the map is monotonic).
    #[must_use]
    pub fn beat_at(&self, seconds: f64) -> f64 {
        let (mut lo, mut hi) = (0.0_f64, 1.0_f64);
        while self.seconds_at(hi) < seconds && hi < 1e7 {
            hi *= 2.0;
        }
        for _ in 0..64 {
            let mid = 0.5 * (lo + hi);
            if self.seconds_at(mid) < seconds {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        0.5 * (lo + hi)
    }
}

/// A song's map: its tempo steps and its section cards, from its bar 1.
fn song_map(song: &LiveSetSong, cards: &[setlist::Section], tempo: &LiveTempo) -> SongMap {
    let mut points = vec![TempoPoint {
        at: Bars::START,
        bpm: tempo.bpm_at(song.start_beats),
        time_signature: song.meter_at(song.start_beats).signature,
    }];
    let mut changes: Vec<f64> = tempo
        .steps()
        .into_iter()
        .map(|(at, _)| at)
        .chain(song.meters.iter().map(|m| m.at))
        .filter(|&at| at > song.start_beats && at < song.end_beats)
        .collect();
    changes.sort_by(f64::total_cmp);
    changes.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    for at in changes {
        points.push(TempoPoint {
            at: song.position(at),
            bpm: tempo.bpm_at(at),
            time_signature: song.meter_at(at).signature,
        });
    }
    let sections = cards
        .iter()
        .map(|card| Section {
            name: card.name.clone(),
            start: song.position(card.start_beats),
            bars: (card.end_beats - card.start_beats)
                / song.meter_at(card.start_beats).beats_per_bar(),
        })
        .collect();
    SongMap {
        name: song.name.clone(),
        tempo: TempoMap::new(points),
        sections,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(start: f64, end: f64, meters: &[(f64, u32)]) -> LiveSetSong {
        LiveSetSong {
            name: "x".into(),
            slug: "x".into(),
            start_beats: start,
            end_beats: end,
            song: SongMap {
                name: "x".into(),
                tempo: TempoMap::default(),
                sections: Vec::new(),
            },
            meters: meters
                .iter()
                .map(|&(at, numerator)| Meter {
                    at,
                    signature: TimeSignature {
                        numerator,
                        denominator: 4,
                    },
                })
                .collect(),
        }
    }

    #[test]
    fn a_song_counts_from_its_own_start_not_the_sets_bar_grid() {
        // Hey! starts on beat 2673 of JT's recital: not a bar line of the
        // set, but bar 1 of the song.
        let s = song(2673.0, 3037.0, &[(0.0, 4)]);
        assert_eq!(s.position(2673.0), Bars::new(1, 1.0));
        assert_eq!(s.position(2677.0), Bars::new(2, 1.0));
        assert_eq!(s.position(2679.5), Bars::new(2, 3.5));
    }

    #[test]
    fn a_signature_change_starts_a_bar() {
        // Hey!'s one 5/4 bar at 2965, back to 4/4 at 2970.
        let s = song(2673.0, 3037.0, &[(0.0, 4), (2965.0, 5), (2970.0, 4)]);
        assert_eq!(s.position(2965.0), Bars::new(74, 1.0));
        assert_eq!(s.position(2969.0), Bars::new(74, 5.0));
        assert_eq!(s.position(2970.0), Bars::new(75, 1.0));
        assert_eq!(s.position(2974.0), Bars::new(76, 1.0));
    }

    #[test]
    fn a_partial_bar_before_a_change_is_a_bar() {
        // 6 beats of 4/4, then 3/4: the 2-beat stub is bar 2.
        let s = song(0.0, 100.0, &[(0.0, 4), (6.0, 3)]);
        assert_eq!(s.position(6.0), Bars::new(3, 1.0));
        assert_eq!(s.position(9.0), Bars::new(4, 1.0));
    }
}
