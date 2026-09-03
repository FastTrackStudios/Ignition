//! Hits, on the musical grid.
//!
//! [`hit_detect_dsp`] finds hits in seconds, because that is all an
//! audio file knows. A light cue is written in bars, because that is
//! what survives a tempo change, a loop, or a seek. This is the join,
//! and it is the same job [`crate::lyrics`] does for an `.lrc`.
//!
//! Snapping matters more here than it does for lyrics. A detected onset
//! carries about 20 ms of window latency and a few more of human timing,
//! so its raw seconds are close to the grid but never on it. Left alone,
//! a chase built from those hits drifts a few milliseconds either side
//! of the beat all song — visible on a hard bump, and worse than being
//! wrong in a consistent direction. Snapped, a hit either *is* the
//! eighth or it is not one.
//!
//! Eighths by default. Sixteenths find hi-hat noise and no more real
//! accents; quarters miss the off-beat stabs that most pop choruses are
//! built on.

use crate::SongMap;
use anyhow::{Context, Result};
use hit_detect_dsp::{Analysis, Band, Config, analyze};
use ignition_daw_proto::Bars;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// One hit, placed on the grid.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
// r[impl song.hits.detected]
pub struct Hit {
    /// The grid position it snapped to — the authoritative time.
    pub at: Bars,
    /// Where it actually was, in seconds, before snapping. Kept so a
    /// suspicious hit can be checked against the recording rather than
    /// argued about.
    pub secs: f32,
    /// 0..1 against the strongest hit in the song.
    pub strength: f32,
    /// Which band it lives in — `Low` is usually a kick, `High` usually
    /// a cymbal or snare crack. See [`hit_detect_dsp::Band`].
    pub band: HitBand,
    /// The dynamic indicator at this moment, 0..1.
    pub dynamics: f32,
}

/// [`hit_detect_dsp::Band`], as something that serialises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitBand {
    Low,
    Mid,
    High,
}

impl From<Band> for HitBand {
    fn from(band: Band) -> Self {
        match band {
            Band::Low => Self::Low,
            Band::Mid => Self::Mid,
            Band::High => Self::High,
        }
    }
}

/// Every hit in a song, plus the dynamic curve behind them.
#[derive(Debug, Clone, Serialize, Deserialize)]
// r[impl song.hits.detected] - per-bar dynamics curve
pub struct Hits {
    pub song: String,
    /// Grid divisions per beat the hits were snapped to — 2 is eighths.
    pub grid: u32,
    pub hits: Vec<Hit>,
    /// The dynamic indicator sampled once per bar, which is the
    /// resolution a *cue* is written at. The per-frame curve stays in
    /// the analysis; a show does not need 17,000 numbers to know the
    /// second chorus is bigger than the first.
    pub dynamics_by_bar: Vec<f32>,
}

impl Hits {
    /// Hits at or after `from` and before `to`.
    pub fn between(&self, from: Bars, to: Bars) -> impl Iterator<Item = &Hit> {
        self.hits.iter().filter(move |h| h.at >= from && h.at < to)
    }

    /// The strongest hit in a bar range, if any — what an accent cue is
    /// hung on.
    #[must_use]
    pub fn strongest(&self, from: Bars, to: Bars) -> Option<&Hit> {
        self.between(from, to)
            .max_by(|a, b| a.strength.total_cmp(&b.strength))
    }

    /// The dynamic indicator for a bar, 0..1.
    #[must_use]
    pub fn dynamics_at_bar(&self, bar: u32) -> f32 {
        let index = usize::try_from(bar.saturating_sub(1)).unwrap_or(usize::MAX);
        self.dynamics_by_bar.get(index).copied().unwrap_or(0.0)
    }
}

/// Analyses an audio file and places its hits on `song`'s grid.
///
/// `grid` is divisions per beat: 2 for eighths, 4 for sixteenths.
///
/// # Errors
///
/// Returns an error if the audio file cannot be decoded.
// r[impl song.hits.detected]
pub fn detect(audio: impl AsRef<Path>, song: &SongMap, grid: u32) -> Result<Hits> {
    let path = audio.as_ref();
    let (samples, sample_rate) =
        fts_sample::load_mono_f32(path, None, fts_sample::ResampleQuality::default())
            .with_context(|| format!("decoding {}", path.display()))?;

    let analysis = analyze(&samples, &Config::for_rate(f64::from(sample_rate)));
    Ok(place(&analysis, song, grid))
}

/// Places a finished analysis on the grid.
// r[impl song.hits.detected]
// r[impl song.hits.grid-snapped] - eighths by default, per-bar dynamics at the midpoint
#[must_use]
pub fn place(analysis: &Analysis, song: &SongMap, grid: u32) -> Hits {
    let grid = grid.max(1);
    let hits = analysis
        .hits
        .iter()
        .map(|hit| Hit {
            at: snap(song.tempo.position_at(hit.secs), song, grid),
            secs: narrow_secs(hit.secs),
            strength: hit.strength,
            band: hit.band.into(),
            dynamics: analysis.dynamics_at(hit.secs),
        })
        .collect();

    // One dynamics value per bar, taken at the bar's midpoint rather
    // than its downbeat: a downbeat sample lands on whatever transient
    // happens to be there, and a bar is being asked to report its own
    // level, not its first moment's.
    let bars = last_bar(song);
    let dynamics_by_bar = (1..=bars)
        .map(|bar| {
            let start = song.tempo.seconds_at(Bars::bar(bar));
            let end = song.tempo.seconds_at(Bars::bar(bar.saturating_add(1)));
            analysis.dynamics_at((start + end) * 0.5)
        })
        .collect();

    Hits {
        song: song.name.clone(),
        grid,
        hits,
        dynamics_by_bar,
    }
}

/// A detected onset's second count, narrowed for storage.
///
/// A song is minutes long, so this is far below where an `f32`'s 24-bit
/// mantissa would start losing whole milliseconds — the field exists so
/// a suspicious hit can be checked by ear, not for sample-accurate
/// resynthesis.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "a song's duration in seconds is far below f32's precision limit; see the doc comment"
)]
const fn narrow_secs(value: f64) -> f32 {
    value as f32
}

/// A non-negative bar count, floored from a beat total.
///
/// `total_beats` only goes negative if a hit is placed before the
/// tempo point that governs it, which `snap` never does — but the
/// value is data-derived, so this clamps rather than trusting that.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to a non-negative range before the cast; see the doc comment"
)]
fn bars_of(value: f64) -> u32 {
    if value.is_nan() || value <= 0.0 {
        0
    } else {
        value.min(f64::from(u32::MAX)) as u32
    }
}

/// Rounds a position to the nearest grid division.
///
/// Done in beats-from-the-start rather than within the bar, so a hit
/// that snaps forward off the end of a bar lands on the next downbeat
/// instead of on "beat 5", a position that does not exist.
// r[impl song.hits.grid-snapped]
fn snap(position: Bars, song: &SongMap, grid: u32) -> Bars {
    let seconds = song.tempo.seconds_at(position);
    let point = song.tempo.at(position);
    let beats_per_bar = f64::from(point.time_signature.numerator.max(1));
    let division = 60.0 / point.bpm / f64::from(grid);
    if division <= 0.0 {
        return position;
    }
    // Snap against the start of the tempo point's own bar, so a tempo
    // change does not shift the grid for everything after it.
    let origin = song.tempo.seconds_at(Bars::bar(point.at.bar));
    let steps = ((seconds - origin) / division).round();
    let total_beats = steps / f64::from(grid);
    let bar = point
        .at
        .bar
        .saturating_add(bars_of((total_beats / beats_per_bar).floor()));
    let beat = total_beats.rem_euclid(beats_per_bar) + 1.0;
    Bars::new(bar, beat)
}

/// The last bar the song's sections reach.
///
/// `Section::end` is the first position *after* the section, so a
/// two-bar song ends at bar 3 beat 1 — one past the last bar there is.
/// A section ending on a downbeat therefore gives back the bar before
/// it; one ending mid-bar (which the sections of a real arrangement do
/// not, but nothing forbids) reaches into the bar it stops in, and that
/// bar counts.
fn last_bar(song: &SongMap) -> u32 {
    song.sections.last().map_or(1, |s| {
        let end = s.end(&song.tempo);
        if (end.beat - 1.0).abs() < 1e-6 {
            end.bar.saturating_sub(1).max(1)
        } else {
            end.bar
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_daw_proto::{TempoMap, TimeSignature};

    /// A small frame count as a float, for building the synthetic
    /// dynamics ramp fixtures below. Bounded by the fixture size, far
    /// under where an `f32` loses integer precision.
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "fixture frame counts here are small; see the doc comment"
    )]
    const fn small_i32_as_f32(n: i32) -> f32 {
        n as f32
    }

    fn song(bpm: f64) -> SongMap {
        SongMap {
            name: "test".into(),
            tempo: TempoMap::constant(
                bpm,
                TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
            ),
            sections: Vec::new(),
        }
    }

    /// At 120 bpm a beat is 0.5 s and an eighth 0.25 s. A hit 20 ms
    /// early — the window latency the detector always carries — must
    /// land on the beat, not just before it.
    #[test]
    /// r[verify song.hits.grid-snapped]
    fn a_late_or_early_hit_snaps_to_the_eighth() {
        let song = song(120.0);
        // 1.5 s is beat 4 of bar 1; test it arriving 20 ms either side.
        for offset in [-0.02, 0.02] {
            let at = snap(song.tempo.position_at(1.5 + offset), &song, 2);
            assert_eq!(at.bar, 1, "offset {offset}");
            assert!((at.beat - 4.0).abs() < 1e-6, "offset {offset}: {at:?}");
        }
    }

    /// The case that makes snapping in absolute beats worth the trouble:
    /// a hit just before the end of a bar snaps *forwards* onto the next
    /// downbeat, not onto a beat 5 that does not exist.
    #[test]
    /// r[verify song.hits.grid-snapped] - snaps in beats from the song start, across the barline
    fn snapping_forward_crosses_the_barline() {
        let song = song(120.0);
        // Bar 2 starts at 2.0 s; arrive 20 ms early.
        let at = snap(song.tempo.position_at(1.98), &song, 2);
        assert_eq!(at.bar, 2);
        assert!((at.beat - 1.0).abs() < 1e-6, "{at:?}");
    }

    /// Eighths, not sixteenths: an off-beat stab is a real musical
    /// position and must survive snapping rather than collapse onto the
    /// beat before it.
    #[test]
    /// r[verify song.hits.grid-snapped] - eighths keep the off-beat
    fn the_off_beat_survives() {
        let song = song(120.0);
        // 0.25 s past the downbeat of bar 1 is the "and" of beat 1.
        let at = snap(song.tempo.position_at(0.25), &song, 2);
        assert_eq!(at.bar, 1);
        assert!((at.beat - 1.5).abs() < 1e-6, "{at:?}");
    }

    /// Every part of a detected hit survives being placed, and the
    /// dynamics curve is one number per bar.
    ///
    /// Each field is here because something downstream needs it and
    /// nothing else carries it: the snapped position is what a cue is
    /// written at, the raw seconds are how a suspicious hit is checked
    /// against the recording rather than argued about, the strength is
    /// against the whole song rather than the recent past, and the
    /// per-bar dynamics are what lets a show know the second chorus is
    /// bigger than the first without carrying seventeen thousand
    /// numbers to say so.
    ///
    /// The bar curve is sampled at each bar's *midpoint*, which the
    /// ramp below is built to catch: a downbeat sample would read 0.0
    /// for bar 1 where the midpoint reads the middle of the bar.
    ///
    /// r[verify song.hits.detected]
    #[test]
    fn a_placed_hit_keeps_its_seconds_strength_band_and_level() {
        use hit_detect_dsp::Hit as Onset;

        // Two bars at 120 bpm: 2 s each, 4 s in all.
        let mut song = song(120.0);
        song.sections = vec![ignition_daw_proto::Section {
            name: "VS".into(),
            start: Bars::bar(1),
            bars: 2.0,
        }];

        // A dynamics ramp from 0 to 1 across four seconds, one frame
        // per 10 ms, so the value at a moment is that moment's fraction
        // of the song.
        let frame_rate = 100.0;
        let frames = 400;
        let analysis = Analysis {
            hits: vec![
                // 20 ms early on beat 4 of bar 1 — the window latency
                // every detector carries.
                Onset {
                    secs: 1.48,
                    strength: 1.0,
                    band: Band::Low,
                },
                Onset {
                    secs: 3.02,
                    strength: 0.25,
                    band: Band::High,
                },
            ],
            dynamics: (0..frames)
                .map(|i| small_i32_as_f32(i) / small_i32_as_f32(frames.saturating_sub(1)))
                .collect(),
            frame_rate,
        };

        let placed = place(&analysis, &song, 2);
        assert_eq!(placed.song, "test");
        assert_eq!(placed.grid, 2);
        assert_eq!(placed.hits.len(), 2);

        let first = placed.hits[0];
        assert_eq!(first.at, Bars::new(1, 4.0), "the hit did not snap");
        assert!(
            (first.secs - 1.48).abs() < 1e-6,
            "the raw second was lost to the snap: {}",
            first.secs
        );
        assert!((first.strength - 1.0).abs() < f32::EPSILON);
        assert_eq!(first.band, HitBand::Low);
        assert!(
            (first.dynamics - 0.37).abs() < 0.02,
            "the level was not read at the hit's own moment: {}",
            first.dynamics
        );

        let second = placed.hits[1];
        assert_eq!(second.at, Bars::new(2, 3.0));
        assert_eq!(second.band, HitBand::High);
        assert!((0.0..=1.0).contains(&second.strength));
        assert!(
            second.dynamics > first.dynamics,
            "the later, louder moment read quieter"
        );

        // One value per bar, at the midpoint: bar 1's middle is 1 s
        // into a four-second song, bar 2's is 3 s.
        assert_eq!(placed.dynamics_by_bar.len(), 2);
        assert!(
            (placed.dynamics_by_bar[0] - 0.25).abs() < 0.02,
            "bar 1: {:?}",
            placed.dynamics_by_bar
        );
        assert!(
            (placed.dynamics_by_bar[1] - 0.75).abs() < 0.02,
            "bar 2: {:?}",
            placed.dynamics_by_bar
        );
    }

    #[test]
    fn strongest_picks_the_biggest_hit_in_range() {
        let hits = Hits {
            song: "t".into(),
            grid: 2,
            hits: vec![
                Hit {
                    at: Bars::bar(1),
                    secs: 0.0,
                    strength: 0.3,
                    band: HitBand::Low,
                    dynamics: 0.5,
                },
                Hit {
                    at: Bars::bar(2),
                    secs: 2.0,
                    strength: 0.9,
                    band: HitBand::High,
                    dynamics: 0.6,
                },
                Hit {
                    at: Bars::bar(9),
                    secs: 9.0,
                    strength: 1.0,
                    band: HitBand::Low,
                    dynamics: 0.9,
                },
            ],
            dynamics_by_bar: Vec::new(),
        };
        let found = hits.strongest(Bars::bar(1), Bars::bar(5)).expect("a hit");
        // The 1.0 at bar 9 is outside the range and must not win.
        assert_eq!(found.at.bar, 2);
    }
}
