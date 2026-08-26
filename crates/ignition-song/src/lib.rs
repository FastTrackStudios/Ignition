//! Reading a song's shape out of a DAW project.
//!
//! The project file is the source of truth for tempo, time signature
//! and section boundaries — not a second copy maintained by hand. Move
//! a section in the DAW and the lighting moves with it, because the
//! lighting never knew where it was in seconds.
//!
//! Parsing goes through `daw::file` rather than reading the text here.
//! That is not politeness about layering: the project format carries
//! fractional tempo, ruler lanes, tempo envelopes and region GUIDs, and
//! a hand-rolled reader gets three of those wrong before anyone
//! notices. (It also means fixes land once — the fractional-tempo bug
//! this hit on day one was a `as i32` in the shared parser, not here.)
//!
//! Today the format is REAPER's. The `.daw` format is the target, and
//! it goes through the same crate.

pub mod transport;
#[cfg(feature = "play")]
pub use transport::SongTransport;
pub use transport::{SourceTransport, TapClock, TransportSource};

pub mod draft;
pub mod timecode;
pub use draft::{Edits, merge, reposition, reposition_from_sidecar};

pub mod chart;
pub mod generate;
pub mod lint;
pub mod mib;
pub use mib::{set_class_timing, set_mib};
pub mod hits;
pub mod lyrics;
pub use generate::{Kind, Roles, generate};

use anyhow::{Context, Result};
use daw::file::{ReaperProject, parse_rpp_file};
use ignition_core::{Bars, Section, SongMap, TempoMap, TempoPoint, TimeSignature};

/// Reads a song map from a project file on disk.
// r[impl song.map.imported]
pub fn load(path: impl AsRef<std::path::Path>) -> Result<SongMap> {
    let path = path.as_ref();
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    from_rpp(&text, &name)
}

/// Reads a song map from project text.
// r[impl song.map.imported]
pub fn from_rpp(text: &str, name: &str) -> Result<SongMap> {
    let parsed = parse_rpp_file(text).map_err(|e| anyhow::anyhow!("parsing project: {e:?}"))?;
    let project = ReaperProject::from_rpp_project(&parsed)
        .map_err(|e| anyhow::anyhow!("reading project: {e}"))?;
    Ok(song_map(&project, name))
}

/// The tempo map, from the project's header tempo.
///
/// A tempo *envelope* — a song that changes tempo — is not read yet;
/// `TempoMap` already holds a list of points, so it is a matter of
/// walking `project.tempo_envelope` rather than a change of shape.
// r[impl song.tempo-map] - fractional tempo and signature read from the project header (single point today)
fn tempo_map(project: &ReaperProject) -> TempoMap {
    match project.properties.tempo {
        Some((bpm, numerator, denominator, _flags)) => TempoMap::constant(
            bpm.max(1.0),
            TimeSignature {
                numerator: numerator.max(1) as u32,
                denominator: denominator.max(1) as u32,
            },
        ),
        None => TempoMap::default(),
    }
}

/// Sections, from the project's regions.
///
/// Regions rather than markers: a region has an end, and a section's
/// *length* is the thing lighting cares about — "the chorus is eight
/// bars" is a statement about a region.
// r[impl song.map]
// r[impl song.map.sections-from-regions]
// r[impl song.map.bar-boundaries] - a fractional section loads unrounded; reporting it is not yet built
fn song_map(project: &ReaperProject, name: &str) -> SongMap {
    let tempo = tempo_map(project);
    let mut sections: Vec<Section> = project
        .markers_regions
        .regions
        .iter()
        .filter_map(|region| {
            let end = region.end_position?;
            let start = tempo.position_at(region.position);
            let bars = bars_between(&tempo, region.position, end);
            (bars > 0.0).then(|| Section {
                name: region.name.clone(),
                start,
                bars,
            })
        })
        .collect();
    sections.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    SongMap {
        name: name.to_string(),
        tempo,
        sections,
    }
}

/// How many bars lie between two times.
fn bars_between(tempo: &TempoMap, from_seconds: f64, to_seconds: f64) -> f64 {
    let from = tempo.position_at(from_seconds);
    let to = tempo.position_at(to_seconds);
    let per_bar = tempo.at(from).time_signature.numerator.max(1) as f64;
    let flat = |p: Bars| (p.bar as f64 - 1.0) + (p.beat - 1.0) / per_bar;
    flat(to) - flat(from)
}

/// A tempo point, for callers building a map by hand.
pub fn point(bar: u32, bpm: f64) -> TempoPoint {
    TempoPoint {
        at: Bars::bar(bar),
        bpm,
        time_signature: TimeSignature::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real project this was built against. Skipped rather than
    /// failed when it is not on this machine — it lives in Downloads,
    /// not in the repo, and a test that fails on a colleague's checkout
    /// teaches people to ignore failures.
    fn song() -> Option<SongMap> {
        let path = concat!(env!("HOME"), "/Downloads/Bye Bye Bye/Bye Bye Bye.RPP");
        std::path::Path::new(path)
            .exists()
            .then(|| load(path).expect("the project parses"))
    }

    #[test]
    /// r[verify song.tempo-map] - fractional tempo
    fn reads_the_songs_fractional_tempo() {
        let Some(song) = song() else { return };
        let point = song.tempo.at(Bars::START);
        assert!(
            (point.bpm - 86.28).abs() < 1e-9,
            "got {} — a truncated tempo is the bug this exists to catch",
            point.bpm
        );
        assert_eq!(point.time_signature.numerator, 4);
    }

    /// Every section of this song is a whole number of bars, which is
    /// the evidence that arranging in bars is how the material is
    /// actually shaped rather than a convenience.
    #[test]
    /// r[verify song.map.bar-boundaries]
    fn every_section_is_a_whole_number_of_bars() {
        let Some(song) = song() else { return };
        assert_eq!(song.sections.len(), 14, "{:?}", names(&song));
        for section in &song.sections {
            let rounded = section.bars.round();
            assert!(
                (section.bars - rounded).abs() < 0.01,
                "{} is {} bars",
                section.name,
                section.bars
            );
            assert!(
                section.bars >= 1.0,
                "{} is {} bars",
                section.name,
                section.bars
            );
        }
        assert_eq!(total_bars(&song).round(), 74.0);
    }

    #[test]
    /// r[verify song.map]
    /// r[verify song.map.sections-from-regions]
    fn sections_arrive_in_order_with_the_expected_shape() {
        let Some(song) = song() else { return };
        assert_eq!(
            names(&song),
            [
                "Count-In",
                "IN A",
                "IN B",
                "VS 1",
                "PRE",
                "CH 1",
                "Break",
                "VS 2",
                "PRE",
                "CH 2",
                "BR",
                "Breakdown",
                "CH 3",
                "Outro"
            ]
        );
        let chorus = song.section("CH 1").expect("CH 1");
        assert_eq!(chorus.start, Bars::bar(23));
        assert_eq!(chorus.bars.round(), 8.0);
    }

    #[test]
    fn a_position_resolves_to_the_section_containing_it() {
        let Some(song) = song() else { return };
        assert_eq!(song.section_at(Bars::bar(23)).unwrap().name, "CH 1");
        assert_eq!(song.section_at(Bars::bar(30)).unwrap().name, "CH 1");
        assert_eq!(song.section_at(Bars::bar(31)).unwrap().name, "Break");
    }

    /// The shipped show is self-describing: every cue and trigger
    /// carries its relative position, and re-resolving them against the
    /// same arrangement lands on the bars the file already caches. This
    /// is the load path a player takes.
    /// r[verify song.relative-position.resolved-on-load]
    /// r[verify song.relative-position]
    #[test]
    fn the_shipped_show_resolves_to_its_own_cached_bars() {
        let Some(song) = song() else { return };
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/songs");
        let raw = std::fs::read_to_string(format!("{dir}/bye-bye-bye.json")).unwrap();
        let mut list: ignition_core::CueList = serde_json::from_str(&raw).unwrap();
        let before: Vec<_> = list
            .cues
            .iter()
            .map(|c| (c.name.clone(), c.position()))
            .collect();
        let triggers_before: Vec<_> = list.triggers.iter().map(|t| t.bars()).collect();
        assert!(
            list.cues
                .iter()
                .all(|c| !matches!(c.at, Some(ignition_core::music::Position::Absolute(_)))),
            "every cue is placed relative to a section"
        );
        let unresolved = crate::reposition(&mut list, &song);
        assert!(unresolved.is_empty(), "{unresolved:?}");
        let after: Vec<_> = list
            .cues
            .iter()
            .map(|c| (c.name.clone(), c.position()))
            .collect();
        assert_eq!(after, before);
        let triggers_after: Vec<_> = list.triggers.iter().map(|t| t.bars()).collect();
        assert_eq!(triggers_after, triggers_before);
        // And the second PRE is addressed as the second PRE.
        let pre2 = list.cues.iter().find(|c| c.name == "PRE 2").unwrap();
        assert_eq!(
            pre2.at,
            Some(ignition_core::music::Position::nth("PRE", 1, 0))
        );
    }

    fn names(song: &SongMap) -> Vec<String> {
        song.sections.iter().map(|s| s.name.clone()).collect()
    }

    fn total_bars(song: &SongMap) -> f64 {
        song.sections.iter().map(|s| s.bars).sum()
    }
}
