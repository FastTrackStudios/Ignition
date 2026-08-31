//! Lyrics on the song's timeline.
//!
//! An `.lrc` is timed in seconds, because it was written for a media
//! player that only knows seconds. Everything else here is timed in
//! bars, because that is what survives a tempo change, a loop, or a seek
//! into the middle of a chorus. So the job of this module is the
//! conversion: read the file, put every line where it falls musically,
//! and hand back something a lyric screen can be driven from by the same
//! clock that drives the lights.
//!
//! What LRC gives is one timestamp per line. That is coarse — a line is
//! ~2.9 s in "Bye Bye Bye" — and it is nowhere near syllable sync. But
//! it gives the two things that make syllable sync tractable later: the
//! *text*, so alignment is a forced-alignment problem against known
//! words rather than transcription, and an *anchor* every few seconds,
//! so a repeated chorus cannot slide onto the wrong repeat. Words and
//! syllables get filled in against those anchors; this is the scaffold.

use anyhow::{Context, Result};
use ignition_daw_proto::{Bars, SongMap};
use keyflow_proto::lrc;
use std::path::Path;

/// One lyric line, placed on the song.
#[derive(Debug, Clone, PartialEq)]
// r[impl song.lyrics] - Bars position, original seconds kept, blank lines kept
pub struct LyricLine {
    /// Where the line lands musically. The authoritative time.
    pub at: Bars,
    /// The same instant in seconds, kept because the LRC said so and
    /// round-tripping through bars and back would only add error.
    pub secs: f32,
    /// Empty means "clear the screen" — LRC marks the end of a line, and
    /// of the last line before an instrumental, with a timed blank. Held
    /// rather than dropped, or the last line of a verse would hang on the
    /// TVs through the whole break.
    pub text: String,
    /// Per-word times where the file had them (enhanced LRC). Empty for
    /// the ordinary line-level kind.
    pub words: Vec<LyricWord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LyricWord {
    pub at: Bars,
    pub secs: f32,
    pub text: String,
}

/// Lyrics for a song, on that song's timeline.
#[derive(Debug, Clone, Default)]
pub struct Lyrics {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub lines: Vec<LyricLine>,
}

impl Lyrics {
    /// The line showing at `position`, if any.
    ///
    /// A line shows until the next one starts — including the blank ones,
    /// which is how the screen clears.
    // r[impl song.lyrics] - a line holds until the next one
    pub fn line_at(&self, position: Bars) -> Option<&LyricLine> {
        let index = match self.lines.binary_search_by(|l| {
            l.at.partial_cmp(&position)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };
        let line = &self.lines[index];
        (!line.text.is_empty()).then_some(line)
    }
}

/// Reads an `.lrc` and places it on `song`'s timeline.
// r[impl song.lyrics]
pub fn load(path: impl AsRef<Path>, song: &SongMap) -> Result<Lyrics> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading lyrics from {}", path.display()))?;
    Ok(place(&lrc::parse(&text), song))
}

/// Places parsed LRC lines on a song's timeline.
///
/// The file's own `[offset:]` has already been folded into its times by
/// the parser, so nothing is applied here — applying it twice is the
/// classic way to end up a beat early on every line.
// r[impl song.lyrics] - lines and words through the tempo map; `[offset:]` applied once, by the parser
pub fn place(parsed: &lrc::Lrc, song: &SongMap) -> Lyrics {
    let bars_at = |secs: f32| song.tempo.position_at(secs as f64);
    Lyrics {
        title: parsed.title.clone(),
        artist: parsed.artist.clone(),
        lines: parsed
            .lines
            .iter()
            .map(|line| LyricLine {
                at: bars_at(line.start),
                secs: line.start,
                text: line.text.clone(),
                words: line
                    .words
                    .iter()
                    .map(|w| LyricWord {
                        at: bars_at(w.start),
                        secs: w.start,
                        text: w.text.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_daw_proto::{TempoMap, TimeSignature};

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

    /// The whole point: a second-timed file comes out bar-timed. At 120
    /// bpm a bar is 2 s, so 8 s is the top of bar 5.
    #[test]
    /// r[verify song.lyrics]
    fn seconds_become_bars() {
        let parsed = lrc::parse("[00:08.00]Hey, hey\n");
        let placed = place(&parsed, &song(120.0));
        assert_eq!(placed.lines[0].at.bar, 5);
        // Beat 1.0 is the downbeat here — beats are 1-based, as they are
        // on a console and in a chart.
        assert!(
            (placed.lines[0].at.beat - 1.0).abs() < 1e-3,
            "{:?}",
            placed.lines[0].at
        );
    }

    /// A line holds until the next one starts, so a screen driven off
    /// `line_at` shows it for its whole duration rather than one frame.
    #[test]
    /// r[verify song.lyrics] - a line holds until the next
    fn a_line_holds_until_the_next_one() {
        let parsed = lrc::parse("[00:08.00]first\n[00:16.00]second\n");
        let placed = place(&parsed, &song(120.0));
        assert_eq!(placed.line_at(Bars::bar(6)).unwrap().text, "first");
        assert_eq!(placed.line_at(Bars::bar(9)).unwrap().text, "second");
    }

    /// Before the first line there is nothing to show — an intro must be
    /// blank, not showing the last line of the song.
    #[test]
    /// r[verify song.lyrics]
    fn nothing_shows_before_the_first_line() {
        let parsed = lrc::parse("[00:08.00]first\n");
        let placed = place(&parsed, &song(120.0));
        assert!(placed.line_at(Bars::bar(1)).is_none());
    }

    /// The blank LRC lines clear the screen. Dropped instead of kept,
    /// the last line of a verse would hang through the instrumental.
    #[test]
    /// r[verify song.lyrics] - a timed blank is kept
    fn a_blank_line_clears_the_screen() {
        let parsed = lrc::parse("[00:08.00]first\n[00:16.00]\n");
        let placed = place(&parsed, &song(120.0));
        assert_eq!(placed.lines.len(), 2);
        assert_eq!(placed.line_at(Bars::bar(6)).unwrap().text, "first");
        assert!(placed.line_at(Bars::bar(9)).is_none());
    }
}
