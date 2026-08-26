//! The hit chart — hits a human curated, and the ideas they belong to.
//!
//! [`crate::hits`] detects; this reads what somebody *decided*. The
//! difference matters more than it sounds. The detector finds a thousand
//! onsets in a three-minute song and is right about nearly all of them,
//! which is exactly the problem: a hi-hat is a real onset and wants no
//! light cue. Deciding which handful of hits carry the song is a musical
//! judgement, and the place to make it is a MIDI editor with the track
//! playing, not a threshold.
//!
//! So the chart lives in the project, on a track called `HITS`, and the
//! detector's job is downgraded from author to first draft.
//!
//! # The schema
//!
//! Pitch is the hit's class, which REAPER shows by name because the
//! track carries `MIDINOTENAMES`:
//!
//! ```text
//! 48  Kick        50  Snare
//! 60  Low Hit     72  Medium Hit      84  High Hit
//! 96  Connected   ← a long note spanning the hits that form one idea
//! ```
//!
//! Velocity is how hard, within the class.
//!
//! # Why grouping is the interesting part
//!
//! A list of hits can only ever produce a light that flashes on hits.
//! Knowing three hits are *one idea* is what lets a programmer — or
//! something writing cues automatically — say "three hits, so throw them
//! left, centre, right across the stage". That is a phrase being played,
//! not three unrelated events that happen to be close together, and no
//! amount of onset detection recovers it: the information is in what the
//! band meant, not in the waveform.
//!
//! A `Connected` note is drawn by hand and so is loose at both ends —
//! it usually starts a little before its first hit. Membership is
//! therefore by overlap, not by exact position.

use crate::SongMap;
use anyhow::{Context, Result};
use daw::file::{ReaperProject, parse_rpp_file};
use ignition_core::Bars;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The track a chart is read from.
pub const TRACK: &str = "HITS";

/// The `Connected` marker pitch.
const CONNECTED: u8 = 96;

/// How big a hit is.
///
/// These are **intensity tiers, not instruments**, despite two of them
/// being named after drums. The names come from the MIDI note names on
/// the track, and are kept so the code says what REAPER shows — but
/// `Kick` does not mean "every kick drum in the song" and never did.
/// Cody uses it for a soft accent, placed where one is wanted, which the
/// chart bears out: forty-six of them in nineteen bars with long
/// stretches carrying none, against a snare that runs two to the bar for
/// fifty-six bars straight.
///
/// Reading them as a drum transcription is the mistake to avoid. It
/// leads to lighting a `Kick` low and heavy because that is where a bass
/// drum sits, when what was actually asked for is the gentlest thing in
/// the vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitClass {
    /// The softest tier — a light touch, not a bass drum.
    Kick,
    /// A light accent. In this chart it runs as the backbeat.
    Snare,
    /// The band hits: the whole group landing together, whatever played
    /// it.
    Low,
    Medium,
    High,
}

impl HitClass {
    /// The pitch this class is written at.
    pub fn pitch(self) -> u8 {
        match self {
            HitClass::Kick => 48,
            HitClass::Snare => 50,
            HitClass::Low => 60,
            HitClass::Medium => 72,
            HitClass::High => 84,
        }
    }

    fn from_pitch(pitch: u8) -> Option<Self> {
        Some(match pitch {
            48 => HitClass::Kick,
            50 => HitClass::Snare,
            60 => HitClass::Low,
            72 => HitClass::Medium,
            84 => HitClass::High,
            _ => return None,
        })
    }

    /// The name REAPER shows for this pitch.
    pub fn label(self) -> &'static str {
        match self {
            HitClass::Kick => "Kick",
            HitClass::Snare => "Snare",
            HitClass::Low => "Low Hit",
            HitClass::Medium => "Medium Hit",
            HitClass::High => "High Hit",
        }
    }

    /// How much of the rig this class is worth, 0..1.
    ///
    /// The two soft tiers are deliberately small. They are accents, not
    /// events — a snare on the backbeat of nearly every bar, a soft hit
    /// wherever one is wanted — and anything that read as a "hit" on
    /// them would be a rig that never stops flashing. What they are for
    /// is pulse. The band hits are what land.
    pub fn weight(self) -> f32 {
        match self {
            HitClass::Kick => 0.08,
            HitClass::Snare => 0.16,
            HitClass::Low => 0.30,
            HitClass::Medium => 0.55,
            HitClass::High => 0.85,
        }
    }
}

/// One charted hit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChartHit {
    pub at: Bars,
    pub class: HitClass,
    /// MIDI velocity, 1..=127.
    pub velocity: u8,
    /// Which [`Group`] this hit belongs to, if any.
    pub group: Option<usize>,
}

impl ChartHit {
    /// What a cue uses — the class weight, and nothing else.
    ///
    /// Velocity is deliberately **not** applied. The chart carries it,
    /// because it is real data somebody may want later, but the values
    /// in it are an artefact of how the notes were entered rather than a
    /// decision: they drift downward through the song for no musical
    /// reason, so scaling by them made later hits quietly weaker than
    /// identical earlier ones. A class already says how big a hit is.
    /// If a hit needs to be bigger, it should be a bigger class.
    pub fn intensity(&self) -> f32 {
        self.class.weight()
    }
}

/// A run of hits that form one musical idea — one `Connected` note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub start: Bars,
    pub end: Bars,
    /// Indices into [`HitChart::hits`], in time order.
    pub members: Vec<usize>,
}

/// Everything charted for one song.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HitChart {
    pub hits: Vec<ChartHit>,
    pub groups: Vec<Group>,
}

impl HitChart {
    /// The hits in a group, in time order.
    pub fn members(&self, group: &Group) -> impl Iterator<Item = &ChartHit> {
        group.members.iter().filter_map(|i| self.hits.get(*i))
    }

    /// Hits belonging to no group — the isolated ones.
    pub fn ungrouped(&self) -> impl Iterator<Item = &ChartHit> {
        self.hits.iter().filter(|h| h.group.is_none())
    }

    /// Whether anything was charted at all.
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }
}

/// Reads the chart from a project's `HITS` track.
///
/// An absent track is not an error — most projects have no chart, and a
/// show without one is still a show.
pub fn read(project: impl AsRef<Path>, song: &SongMap) -> Result<HitChart> {
    let path = project.as_ref();
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let parsed = parse_rpp_file(&text).map_err(|e| anyhow::anyhow!("parsing project: {e:?}"))?;
    let project = ReaperProject::from_rpp_project(&parsed)
        .map_err(|e| anyhow::anyhow!("reading project: {e}"))?;

    let Some(track) = project
        .tracks
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(TRACK))
    else {
        return Ok(HitChart::default());
    };

    let notes: Vec<Note> = track
        .items
        .iter()
        .flat_map(|item| {
            let offset = item.position;
            item.takes
                .iter()
                .filter_map(|take| take.source.as_ref())
                .filter_map(|source| source.midi_data.as_ref())
                .flat_map(move |midi| notes_of(midi, offset))
        })
        .collect();
    Ok(assemble(notes, song))
}

/// One decoded note-on/note-off pair, in seconds from the project start.
#[derive(Debug, Clone, Copy)]
struct Note {
    start_qn: f64,
    end_qn: f64,
    pitch: u8,
    velocity: u8,
}

/// Pairs note-ons with note-offs.
///
/// Deltas are cumulative and per-*event*, not per-note, so the running
/// clock has to advance on every event including the offs — dropping
/// them would leave every note after the first at the wrong position.
fn notes_of(
    midi: &daw::file::types::item::MidiSource,
    item_offset_secs: f64,
) -> Vec<Note> {
    let ppq = f64::from(midi.ticks_per_qn.max(1));
    let _ = item_offset_secs;
    let mut clock = 0u64;
    let mut open: std::collections::HashMap<u8, Vec<(u64, u8)>> = Default::default();
    let mut notes = Vec::new();

    for event in &midi.events {
        clock += u64::from(event.delta_ticks);
        let [status, pitch, velocity] = event.bytes[..] else {
            continue;
        };
        match status & 0xF0 {
            0x90 if velocity > 0 => open.entry(pitch).or_default().push((clock, velocity)),
            0x80 | 0x90 => {
                // A note-off matches the *oldest* open note of that
                // pitch. Same pitch retriggered before release is rare
                // here but free to handle correctly.
                if let Some(stack) = open.get_mut(&pitch)
                    && !stack.is_empty()
                {
                    let (start, velocity) = stack.remove(0);
                    notes.push(Note {
                        start_qn: start as f64 / ppq,
                        end_qn: clock as f64 / ppq,
                        pitch,
                        velocity,
                    });
                }
            }
            _ => {}
        }
    }
    notes.sort_by(|a, b| a.start_qn.total_cmp(&b.start_qn));
    notes
}

/// Turns decoded notes into hits and the groups spanning them.
fn assemble(notes: Vec<Note>, song: &SongMap) -> HitChart {
    let at = |qn: f64| -> Bars {
        // Quarter notes from the project start, through the tempo map,
        // so a tempo change moves the chart with the music.
        song.tempo.position_at(qn_to_secs(qn, song))
    };

    let mut hits: Vec<ChartHit> = notes
        .iter()
        .filter_map(|n| {
            HitClass::from_pitch(n.pitch).map(|class| ChartHit {
                at: at(n.start_qn),
                class,
                velocity: n.velocity,
                group: None,
            })
        })
        .collect();

    let mut groups = Vec::new();
    for span in notes.iter().filter(|n| n.pitch == CONNECTED) {
        let (start, end) = (at(span.start_qn), at(span.end_qn));
        // Membership by overlap. A `Connected` note is drawn by hand and
        // usually starts a little before the hit it opens on, so an
        // exact-position test would find nothing at all.
        let members: Vec<usize> = hits
            .iter()
            .enumerate()
            .filter(|(_, h)| h.at >= start && h.at < end)
            .map(|(i, _)| i)
            .collect();
        if members.is_empty() {
            continue;
        }
        let index = groups.len();
        for member in &members {
            hits[*member].group = Some(index);
        }
        groups.push(Group {
            start,
            end,
            members,
        });
    }

    HitChart { hits, groups }
}

/// Quarter notes from the project start, as seconds.
fn qn_to_secs(qn: f64, song: &SongMap) -> f64 {
    // Walk the tempo map rather than assuming one tempo: quarter notes
    // are musical time and seconds are not, which is the whole reason
    // the map exists.
    let mut remaining = qn;
    let mut seconds = 0.0;
    let points = song.tempo.points();
    for (i, point) in points.iter().enumerate() {
        let next = points.get(i + 1);
        let span_qn = match next {
            Some(next) => {
                let beats = beats_between(point.at, next.at, point.time_signature.numerator);
                beats * f64::from(point.time_signature.denominator) / 4.0
            }
            None => f64::INFINITY,
        };
        let take = remaining.min(span_qn);
        seconds += take * 60.0 / point.bpm;
        remaining -= take;
        if remaining <= 0.0 {
            break;
        }
    }
    seconds
}

/// Beats between two positions at a fixed time signature.
fn beats_between(from: Bars, to: Bars, beats_per_bar: u32) -> f64 {
    let per_bar = f64::from(beats_per_bar.max(1));
    let flat = |b: Bars| f64::from(b.bar) * per_bar + b.beat;
    flat(to) - flat(from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::{TempoMap, TimeSignature};

    fn song() -> SongMap {
        SongMap {
            name: "t".into(),
            tempo: TempoMap::constant(120.0, TimeSignature { numerator: 4, denominator: 4 }),
            sections: Vec::new(),
        }
    }

    fn note(start_qn: f64, len_qn: f64, pitch: u8, velocity: u8) -> Note {
        Note {
            start_qn,
            end_qn: start_qn + len_qn,
            pitch,
            velocity,
        }
    }

    /// The whole point: three hits under one `Connected` note come back
    /// as one idea, which is what lets a programmer throw them left,
    /// centre and right instead of flashing three times in one place.
    #[test]
    fn a_connected_note_gathers_its_hits() {
        let chart = assemble(
            vec![
                // Drawn a little early, as a hand-drawn note is.
                note(3.9, 1.7, CONNECTED, 48),
                note(4.0, 0.1, 84, 96),
                note(4.5, 0.1, 84, 96),
                note(5.0, 0.1, 84, 96),
            ],
            &song(),
        );
        assert_eq!(chart.groups.len(), 1);
        assert_eq!(chart.groups[0].members.len(), 3);
        assert!(chart.hits.iter().all(|h| h.group == Some(0)));
    }

    /// Hits outside every span stay ungrouped rather than being swept
    /// into the nearest one — an isolated stab is its own idea.
    #[test]
    fn hits_outside_a_span_stay_ungrouped() {
        let chart = assemble(
            vec![
                note(3.9, 1.2, CONNECTED, 48),
                note(4.0, 0.1, 84, 96),
                note(9.0, 0.1, 72, 96),
            ],
            &song(),
        );
        assert_eq!(chart.ungrouped().count(), 1);
        assert_eq!(chart.ungrouped().next().unwrap().class, HitClass::Medium);
    }

    /// Pitches outside the schema are somebody else's notes and must not
    /// become hits — the track is a place a person works, and a stray
    /// note should be ignored rather than lighting the room.
    #[test]
    fn unknown_pitches_are_ignored() {
        let chart = assemble(vec![note(0.0, 0.1, 42, 100)], &song());
        assert!(chart.is_empty());
    }

    /// Velocity is carried but not applied. The values in a hand-entered
    /// chart drift for reasons that are not musical, and scaling by them
    /// made late hits weaker than identical early ones.
    #[test]
    fn velocity_does_not_change_intensity() {
        let soft = ChartHit { at: Bars::bar(1), class: HitClass::High, velocity: 40, group: None };
        let hard = ChartHit { at: Bars::bar(1), class: HitClass::High, velocity: 127, group: None };
        assert_eq!(soft.intensity(), hard.intensity());
        assert!((hard.intensity() - HitClass::High.weight()).abs() < 1e-6);
    }

    /// A kick is on nearly every beat and a snare on every backbeat, so
    /// both must stay well under a band hit — otherwise the rig never
    /// stops flashing and nothing reads as an accent.
    #[test]
    fn pulse_classes_stay_under_the_band_hits() {
        assert!(HitClass::Kick.weight() < HitClass::Snare.weight());
        assert!(HitClass::Snare.weight() < HitClass::Low.weight());
        assert!(HitClass::Snare.weight() < HitClass::High.weight() * 0.25);
    }
}
