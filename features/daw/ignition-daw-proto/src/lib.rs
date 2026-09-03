//! Musical time — where a cue sits in a song.
//!
//! The leaf of the `daw` feature, and the contract between a DAW project
//! and everything downstream of it: bars and beats, the tempo map, the
//! section list, and a position written relative to a section rather
//! than to a bar number.
//!
//! See `docs/domain/musical-time-cues.md`. The short version: a cue at
//! 61.196 s breaks when the tempo changes, when a section is cut, or
//! when the band takes the last chorus twice. A cue at bar 22 does not.
//!
//! Nothing here reads a clock or a file. It converts between musical
//! position and seconds, and holds the map a song is shaped by; who
//! supplies the position each frame is somebody else's problem.
//!
//! This was `ignition_core::music` until the `daw` feature was carved
//! out. It is the only part of the musical layer the lighting domain
//! needs, which is why it is the part that sits below both:
//! `ignition-core` depends on this crate, and so does every backend
//! that produces a song map.

use serde::{Deserialize, Serialize};

/// A position in a song, in bars and beats, counted from 1 the way
/// musicians count.
///
/// `Bars { bar: 22, beat: 1.0 }` is "the downbeat of bar 22". `beat` is
/// fractional so a hit on the *and* of 3 is `beat: 3.5` rather than a
/// separate tick unit nobody says out loud.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
// r[impl song.position]
// r[impl song.position.never-seconds] - the stored form is bars and beats
pub struct Bars {
    pub bar: u32,
    #[serde(default = "one_beat")]
    pub beat: f64,
}

#[must_use]
const fn one_beat() -> f64 {
    1.0
}

impl Bars {
    pub const START: Self = Self { bar: 1, beat: 1.0 };

    #[must_use]
    pub const fn bar(bar: u32) -> Self {
        Self { bar, beat: 1.0 }
    }

    #[must_use]
    pub const fn new(bar: u32, beat: f64) -> Self {
        Self { bar, beat }
    }
}

impl PartialOrd for Bars {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        (self.bar, self.beat).partial_cmp(&(other.bar, other.beat))
    }
}

/// Beats per bar and what a beat is worth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    pub numerator: u32,
    pub denominator: u32,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self {
            numerator: 4,
            denominator: 4,
        }
    }
}

/// One tempo, from a musical position onward.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
// r[impl song.tempo-map] - fractional bpm with a time signature per point
pub struct TempoPoint {
    pub at: Bars,
    /// Fractional on purpose. A project written at 86.28 BPM read as 86
    /// is nine milliseconds of error per bar — two thirds of a second
    /// across a three-minute song, which is well past where a cue stops
    /// landing. (That was a real bug in the RPP parser, fixed upstream.)
    pub bpm: f64,
    #[serde(default)]
    pub time_signature: TimeSignature,
}

/// How a song's musical time relates to seconds.
///
/// A list rather than a single tempo from the start, even though most
/// songs have one entry: a song that ritards is then a data problem
/// rather than a redesign.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl song.tempo-map] - an ordered list, never a single number
pub struct TempoMap {
    points: Vec<TempoPoint>,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self::constant(120.0, TimeSignature::default())
    }
}

impl TempoMap {
    #[must_use]
    pub fn constant(bpm: f64, time_signature: TimeSignature) -> Self {
        Self {
            points: vec![TempoPoint {
                at: Bars::START,
                bpm,
                time_signature,
            }],
        }
    }

    /// Points are sorted on the way in, so callers can hand over
    /// whatever order the project file listed them in.
    #[must_use]
    pub fn new(mut points: Vec<TempoPoint>) -> Self {
        points.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
        if points.is_empty() {
            return Self::default();
        }
        Self { points }
    }

    #[must_use]
    pub fn points(&self) -> &[TempoPoint] {
        &self.points
    }

    /// The tempo in force at a position.
    #[must_use]
    pub fn at(&self, position: Bars) -> TempoPoint {
        // Points are sorted, so the last one at or before the position
        // is the one in force. `rev().find()` rather than
        // `take_while().next_back()` because `TakeWhile` is not
        // double-ended.
        let default = TempoPoint {
            at: Bars::START,
            bpm: 120.0,
            time_signature: TimeSignature::default(),
        };
        self.points
            .iter()
            .rev()
            .find(|p| p.at <= position)
            .copied()
            .or_else(|| self.points.first().copied())
            .unwrap_or(default)
    }

    /// Seconds from the start of the song to a musical position.
    ///
    /// Walks the tempo points so a map with several tempos accumulates
    /// correctly rather than pretending the last one always applied.
    // r[impl song.tempo-map] - accumulates segment by segment
    // r[impl song.position.never-seconds] - seconds are derived at the moment of use, never stored
    #[must_use]
    pub fn seconds_at(&self, position: Bars) -> f64 {
        let mut seconds = 0.0;
        for (i, point) in self.points.iter().enumerate() {
            if point.at > position {
                break;
            }
            // This point runs until the next one, or until `position`.
            let until = i
                .checked_add(1)
                .and_then(|j| self.points.get(j))
                .map_or(position, |next| next.at.min_position(position));
            let delta = beats_between(point, point.at, until) * 60.0 / point.bpm;
            seconds += delta;
        }
        seconds
    }

    /// The musical position at a number of seconds — the inverse of
    /// `seconds_at`, and what a transport's playhead becomes.
    // r[impl song.tempo-map] - the inverse
    // r[impl song.position.never-seconds] - a seconds source is converted on the way in
    #[must_use]
    pub fn position_at(&self, seconds: f64) -> Bars {
        let mut elapsed = 0.0;
        for (i, point) in self.points.iter().enumerate() {
            let next = i.checked_add(1).and_then(|j| self.points.get(j));
            let segment_seconds = next.map_or(f64::INFINITY, |n| {
                beats_between(point, point.at, n.at) * 60.0 / point.bpm
            });
            if seconds < elapsed + segment_seconds {
                let into_beats = (seconds - elapsed) * point.bpm / 60.0;
                return advance(point, point.at, into_beats);
            }
            elapsed += segment_seconds;
        }
        Bars::START
    }
}

impl Bars {
    /// The earlier of two positions.
    #[must_use]
    fn min_position(self, other: Self) -> Self {
        if self < other { self } else { other }
    }
}

/// Beats between two positions, in one tempo point's time signature.
#[must_use]
fn beats_between(point: &TempoPoint, from: Bars, to: Bars) -> f64 {
    let per_bar = f64::from(point.time_signature.numerator.max(1));
    let flat = |p: Bars| (f64::from(p.bar) - 1.0).mul_add(per_bar, p.beat - 1.0);
    (flat(to) - flat(from)).max(0.0)
}

/// A position `beats` further on from `from`, in one point's signature.
// r[impl song.tempo-map] - invertible to within a nanobeat
#[must_use]
fn advance(point: &TempoPoint, from: Bars, beats: f64) -> Bars {
    let per_bar = f64::from(point.time_signature.numerator.max(1));
    let flat = (f64::from(from.bar) - 1.0).mul_add(per_bar, from.beat - 1.0 + beats);
    // Snap before splitting into bar and beat. A downbeat arrived at by
    // dividing seconds lands on 235.999999999 beats as often as on 236,
    // and the floor below turns the first into "bar 59, beat 5" — a
    // position that does not exist, one bar early, right on the section
    // boundary where every cue sits. A nanobeat is 0.7 microseconds at
    // this tempo.
    let flat = (flat * 1e9).round() / 1e9;
    let bar = (flat / per_bar).floor();
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the range check above ensures bar is in [0, u32::MAX)"
    )]
    let bar_u32 = if bar >= 0.0 && bar < f64::from(u32::MAX) {
        bar as u32
    } else {
        0
    };
    Bars {
        bar: bar_u32.saturating_add(1),
        beat: flat - bar * per_bar + 1.0,
    }
}

/// One named stretch of a song — a verse, a chorus, a break.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub start: Bars,
    /// Length in bars. Whole numbers in practice, because that is how
    /// the material is shaped — every section of the song this was
    /// built against is an exact bar count.
    pub bars: f64,
}

impl Section {
    /// The first position after this section.
    #[must_use]
    pub fn end(&self, map: &TempoMap) -> Bars {
        let point = map.at(self.start);
        advance(
            &point,
            self.start,
            self.bars * f64::from(point.time_signature.numerator.max(1)),
        )
    }
}

/// A song's shape: its tempo map and its sections.
///
/// Imported from the project file rather than authored twice — moving a
/// section in the DAW moves the lighting with it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl song.map]
pub struct SongMap {
    pub name: String,
    pub tempo: TempoMap,
    pub sections: Vec<Section>,
}

impl SongMap {
    /// The section containing a position, if any.
    #[must_use]
    pub fn section_at(&self, position: Bars) -> Option<&Section> {
        self.sections
            .iter()
            .rev()
            .find(|s| s.start <= position)
            .filter(|s| position < s.end(&self.tempo))
    }

    #[must_use]
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.iter().find(|s| s.name == name)
    }

    /// The `ordinal`th section with this name, counting from 0.
    ///
    /// `section("PRE")` finds the first pre-chorus and leaves the second
    /// unreachable; this is how the second one is named.
    // r[impl song.relative-position.duplicate-names]
    #[must_use]
    pub fn section_nth(&self, name: &str, ordinal: usize) -> Option<&Section> {
        self.sections.iter().filter(|s| s.name == name).nth(ordinal)
    }

    /// Which occurrence of its own name a section is, counting from 0.
    fn ordinal_of(&self, index: usize) -> usize {
        self.sections.get(index).map_or(0, |section| {
            let name = &section.name;
            self.sections.get(..index).map_or(0, |before| {
                before.iter().filter(|s| &s.name == name).count()
            })
        })
    }
}

/// Where a cue sits, as the author wrote it.
///
/// A show stores this, not the bar it resolves to: "4 bars into `CH 1`"
/// survives cutting two bars from the verse, and bar 27 does not. The
/// resolved [`Bars`] is what the engine runs on and is derived on load.
///
/// The JSON is deliberately compatible with a bare [`Bars`]: a file
/// that says `{"bar": 22, "beat": 1.0}` still loads, as `Absolute`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
// r[impl song.relative-position] - the stored form is relative to a section
pub enum Position {
    /// A bar number. What the older files say, and what a position
    /// outside any section has to be.
    Absolute(Bars),
    /// The last bar of the `ordinal`th section named `section` — where a
    /// build lands and a stab goes. Listed before `Relative` because the
    /// untagged decoder tries variants in order and this one carries a
    /// marker field the other does not.
    LastBar {
        section: String,
        #[serde(default)]
        ordinal: usize,
        /// Always `true`; it is what tells the two relative forms apart
        /// in JSON.
        last_bar: bool,
    },
    /// `bars` whole bars and a `beat` into the `ordinal`th section named
    /// `section`.
    Relative {
        section: String,
        #[serde(default)]
        ordinal: usize,
        #[serde(default)]
        bars: u32,
        #[serde(default = "one_beat")]
        beat: f64,
    },
}

impl From<Bars> for Position {
    fn from(bars: Bars) -> Self {
        Self::Absolute(bars)
    }
}

impl Position {
    /// `bars` into the first section with this name.
    #[must_use]
    pub fn at(section: &str, bars: u32) -> Self {
        Self::nth(section, 0, bars)
    }

    /// `bars` into the `ordinal`th section with this name.
    #[must_use]
    pub fn nth(section: &str, ordinal: usize, bars: u32) -> Self {
        Self::Relative {
            section: section.to_string(),
            ordinal,
            bars,
            beat: 1.0,
        }
    }

    /// The last bar of the first section with this name.
    #[must_use]
    pub fn last_bar(section: &str) -> Self {
        Self::LastBar {
            section: section.to_string(),
            ordinal: 0,
            last_bar: true,
        }
    }

    /// The bar this lands on in a given arrangement, or `None` when the
    /// arrangement has no such section — a cue for a section the song no
    /// longer has is dropped, not placed at bar 1.
    // r[impl song.relative-position.resolved-on-load] - resolution is a function of the map, called by the loader
    // r[impl song.relative-position.duplicate-names] - the ordinal picks the occurrence
    #[must_use]
    pub fn resolve(&self, song: &SongMap) -> Option<Bars> {
        match self {
            Self::Absolute(bars) => Some(*bars),
            Self::Relative {
                section,
                ordinal,
                bars,
                beat,
            } => {
                let s = song.section_nth(section, *ordinal)?;
                Some(Bars::new(s.start.bar.saturating_add(*bars), *beat))
            }
            Self::LastBar {
                section, ordinal, ..
            } => {
                let s = song.section_nth(section, *ordinal)?;
                let section_end_bar = s.bars.ceil();
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "the range check ensures section_end_bar is in [0, u32::MAX)"
                )]
                let section_end_u32 =
                    if section_end_bar >= 0.0 && section_end_bar < f64::from(u32::MAX) {
                        section_end_bar as u32
                    } else {
                        0
                    };
                Some(Bars::bar(
                    s.start
                        .bar
                        .saturating_add(section_end_u32.max(1).saturating_sub(1)),
                ))
            }
        }
    }

    /// The relative form of an absolute position — which section it
    /// falls in, and how far in. A position outside every section stays
    /// absolute, because there is nothing to be relative to.
    ///
    /// This is how positions that arrive absolute — a charted hit, a
    /// hand-placed cue — are written down so they move with the section.
    #[must_use]
    pub fn relative_to(song: &SongMap, at: Bars) -> Self {
        let Some(index) = song
            .sections
            .iter()
            .rposition(|s| s.start <= at && at < s.end(&song.tempo))
        else {
            return Self::Absolute(at);
        };
        let Some(section) = song.sections.get(index) else {
            return Self::Absolute(at);
        };
        Self::Relative {
            section: section.name.clone(),
            ordinal: song.ordinal_of(index),
            bars: at.bar.saturating_sub(section.start.bar),
            beat: at.beat,
        }
    }
}

#[cfg(test)]
mod tests {
    // Test code: the lines below and the test body may use `as` conversions freely,
    // without #[allow] on each one. This is the only place in this crate where that
    // is allowed.
    use super::*;

    /// The real song this was built against: 86.28 BPM, 4/4.
    fn bye_bye_bye() -> TempoMap {
        TempoMap::constant(86.28, TimeSignature::default())
    }

    #[test]
    fn a_bar_is_four_beats_of_the_tempo() {
        let map = bye_bye_bye();
        let bar_seconds = 4.0 * 60.0 / 86.28;
        assert!((map.seconds_at(Bars::bar(2)) - bar_seconds).abs() < 1e-9);
        assert!((map.seconds_at(Bars::bar(3)) - bar_seconds * 2.0).abs() < 1e-9);
    }

    /// The section starts read out of the project's regions, in bars,
    /// against the seconds REAPER wrote. If the tempo were truncated to
    /// 86 these would be out by up to two thirds of a second — which is
    /// how the parser bug was found.
    /// r[verify song.tempo-map] - fractional bpm
    #[test]
    fn real_section_starts_land_on_their_bars() {
        let map = bye_bye_bye();
        for (bar, seconds, name) in [
            (3u32, 5.563_282_336_578_58, "IN A"),
            (11, 27.816_411_682_892_91, "VS 1"),
            (23, 61.196_105_702_364_39, "CH 1"),
            (61, 166.898_470_097_357_43, "CH 3"),
            (73, 200.278_164_116_828_92, "Outro"),
        ] {
            let got = map.seconds_at(Bars::bar(bar));
            assert!(
                (got - seconds).abs() < 0.001,
                "{name}: bar {bar} is {got}s, project says {seconds}s"
            );
        }
    }

    /// r[verify song.tempo-map]
    #[test]
    fn position_and_seconds_are_inverses() {
        let map = bye_bye_bye();
        for bar in [1u32, 2, 23, 60, 74] {
            let seconds = map.seconds_at(Bars::bar(bar));
            let back = map.position_at(seconds);
            assert_eq!(back.bar, bar, "{seconds}s");
            assert!((back.beat - 1.0).abs() < 1e-6);
        }
    }

    /// r[verify song.position]
    #[test]
    fn a_fractional_beat_is_partway_into_the_bar() {
        let map = bye_bye_bye();
        let beat = 60.0 / 86.28;
        // The "and" of 3 in bar 5 is two and a half beats past its
        // downbeat.
        let got = map.seconds_at(Bars::new(5, 3.5)) - map.seconds_at(Bars::bar(5));
        assert!((got - beat * 2.5).abs() < 1e-9, "{got}");
    }

    /// r[verify song.tempo-map]
    #[test]
    fn a_tempo_change_accumulates_rather_than_rewriting_history() {
        let map = TempoMap::new(vec![
            TempoPoint {
                at: Bars::bar(1),
                bpm: 120.0,
                time_signature: TimeSignature::default(),
            },
            TempoPoint {
                at: Bars::bar(3),
                bpm: 60.0,
                time_signature: TimeSignature::default(),
            },
        ]);
        // Two bars at 120 = 4s; then two bars at 60 = 8s.
        assert!((map.seconds_at(Bars::bar(3)) - 4.0).abs() < 1e-9);
        assert!((map.seconds_at(Bars::bar(5)) - 12.0).abs() < 1e-9);
        // ...and the inverse still works across the change.
        assert_eq!(map.position_at(12.0).bar, 5);
    }

    #[test]
    fn a_section_knows_where_it_ends() {
        let map = bye_bye_bye();
        let chorus = Section {
            name: "CH 1".into(),
            start: Bars::bar(23),
            bars: 8.0,
        };
        assert_eq!(chorus.end(&map), Bars::bar(31));
    }

    /// r[verify song.map]
    #[test]
    fn a_song_map_finds_the_section_a_position_is_in() {
        let song = SongMap {
            name: "Bye Bye Bye".into(),
            tempo: bye_bye_bye(),
            sections: vec![
                Section {
                    name: "VS 1".into(),
                    start: Bars::bar(11),
                    bars: 8.0,
                },
                Section {
                    name: "PRE".into(),
                    start: Bars::bar(19),
                    bars: 4.0,
                },
                Section {
                    name: "CH 1".into(),
                    start: Bars::bar(23),
                    bars: 8.0,
                },
            ],
        };
        assert_eq!(song.section_at(Bars::bar(11)).unwrap().name, "VS 1");
        assert_eq!(song.section_at(Bars::new(22, 4.9)).unwrap().name, "PRE");
        assert_eq!(song.section_at(Bars::bar(23)).unwrap().name, "CH 1");
        // Past the end of the last section is not in any of them.
        assert!(song.section_at(Bars::bar(31)).is_none());
        // ...and neither is anything before the first.
        assert!(song.section_at(Bars::bar(1)).is_none());
    }

    fn arrangement(verse_bars: f64) -> SongMap {
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "test: range check ensures verse_bars is in [0, u32::MAX)"
        )]
        let verse_bars_u32 = if verse_bars >= 0.0 && verse_bars < f64::from(u32::MAX) {
            verse_bars as u32
        } else {
            0
        };
        SongMap {
            name: "test".into(),
            tempo: bye_bye_bye(),
            sections: vec![
                Section {
                    name: "VS 1".into(),
                    start: Bars::bar(1),
                    bars: verse_bars,
                },
                Section {
                    name: "PRE".into(),
                    start: Bars::bar({
                        #[expect(
                            clippy::arithmetic_side_effects,
                            reason = "test: constant addition"
                        )]
                        {
                            1 + verse_bars_u32
                        }
                    }),
                    bars: 4.0,
                },
                Section {
                    name: "CH 1".into(),
                    start: Bars::bar({
                        #[expect(
                            clippy::arithmetic_side_effects,
                            reason = "test: constant addition"
                        )]
                        {
                            5 + verse_bars_u32
                        }
                    }),
                    bars: 8.0,
                },
                Section {
                    name: "PRE".into(),
                    start: Bars::bar({
                        #[expect(
                            clippy::arithmetic_side_effects,
                            reason = "test: constant addition"
                        )]
                        {
                            13 + verse_bars_u32
                        }
                    }),
                    bars: 4.0,
                },
            ],
        }
    }

    /// r[verify song.relative-position]
    /// r[verify song.relative-position.resolved-on-load]
    #[test]
    fn a_relative_position_moves_with_its_section() {
        let four_in = Position::at("CH 1", 4);
        assert_eq!(four_in.resolve(&arrangement(8.0)), Some(Bars::bar(17)));
        // Cut two bars from the verse: the same position, two bars earlier.
        assert_eq!(four_in.resolve(&arrangement(6.0)), Some(Bars::bar(15)));
        // The last bar of the pre-chorus.
        assert_eq!(
            Position::last_bar("PRE").resolve(&arrangement(8.0)),
            Some(Bars::bar(12))
        );
        // A section the arrangement does not have resolves to nothing.
        assert_eq!(Position::at("BR", 0).resolve(&arrangement(8.0)), None);
    }

    /// r[verify song.relative-position.duplicate-names]
    #[test]
    fn the_second_pre_is_reachable_by_ordinal() {
        let song = arrangement(8.0);
        assert_eq!(song.section_nth("PRE", 0).unwrap().start, Bars::bar(9));
        assert_eq!(song.section_nth("PRE", 1).unwrap().start, Bars::bar(21));
        assert!(song.section_nth("PRE", 2).is_none());
        assert_eq!(
            Position::nth("PRE", 1, 0).resolve(&song),
            Some(Bars::bar(21))
        );
        // ...and an absolute bar in the second PRE reads back as it.
        assert_eq!(
            Position::relative_to(&song, Bars::new(22, 3.5)),
            Position::Relative {
                section: "PRE".into(),
                ordinal: 1,
                bars: 1,
                beat: 3.5
            }
        );
        // Outside every section there is nothing to be relative to.
        assert_eq!(
            Position::relative_to(&song, Bars::bar(40)),
            Position::Absolute(Bars::bar(40))
        );
    }

    /// r[verify song.relative-position] - the older absolute form still loads
    #[test]
    fn positions_round_trip_through_json_and_accept_a_bare_bar() {
        let absolute: Position = serde_json::from_str(r#"{"bar":22,"beat":1.0}"#).unwrap();
        assert_eq!(absolute, Position::Absolute(Bars::bar(22)));
        let bare: Position = serde_json::from_str(r#"{"bar":22}"#).unwrap();
        assert_eq!(bare, Position::Absolute(Bars::bar(22)));
        for p in [
            Position::at("CH 1", 4),
            Position::nth("PRE", 1, 0),
            Position::last_bar("Breakdown"),
            Position::Absolute(Bars::new(3, 2.5)),
        ] {
            let json = serde_json::to_string(&p).unwrap();
            let back: Position = serde_json::from_str(&json).unwrap();
            assert_eq!(back, p, "{json}");
        }
    }
}
