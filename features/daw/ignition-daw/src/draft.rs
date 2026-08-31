//! Keeping a person's edits across a regeneration.
//!
//! The generated list is a draft. The parts that make it a show — the
//! blackout before the last chorus, the lift four bars into the second
//! verse — are added by hand, and a regenerator that threw them away
//! would teach people not to touch the draft. Two sidecars keep the
//! list re-derivable without that cost:
//!
//! * `<song>.edits.json` — an [`Edits`]: which cues to keep verbatim
//!   from the existing file, and any written inline.
//!
//! Positions used to be a second sidecar, `<song>.positions.json`,
//! because `Cue.at` carried only the resolved bar. It now carries the
//! relative [`Position`](ignition_core::music::Position) itself, so the
//! show file is self-describing and [`reposition`] resolves it against
//! whatever arrangement is loaded. [`reposition_from_sidecar`] still
//! reads an old file's sidecar into it.

use ignition_core::cue::Positions;
use ignition_core::music::SongMap;
use ignition_core::{Cue, CueList};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What to keep when the draft is regenerated.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Edits {
    /// Names of cues in the existing show file to keep verbatim over
    /// the regenerated ones.
    #[serde(default)]
    pub keep: Vec<String>,
    /// Cues written here directly, by name. Kept over both the
    /// generated cue and the existing file's.
    #[serde(default)]
    pub cues: BTreeMap<String, Cue>,
}

impl Edits {
    pub fn load(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn is_empty(&self) -> bool {
        self.keep.is_empty() && self.cues.is_empty()
    }
}

/// Lays a person's edits over a freshly generated list.
///
/// A kept cue replaces the generated cue of the same name, or is added
/// if the generator no longer makes one. Its `at` is whatever it was —
/// [`reposition`] afterwards moves it with its section like any other.
/// Returns the names that were kept, and those asked for but found
/// nowhere, so the caller can say so.
// r[impl song.generate.is-a-draft] - regeneration does not destroy edits
pub fn merge(fresh: &mut CueList, existing: Option<&CueList>, edits: &Edits) -> Merged {
    let mut merged = Merged::default();
    let mut place = |cue: Cue| {
        match fresh.cues.iter_mut().find(|c| c.name == cue.name) {
            Some(slot) => *slot = cue.clone(),
            None => fresh.cues.push(cue.clone()),
        }
        merged.kept.push(cue.name);
    };
    for name in &edits.keep {
        match existing.and_then(|l| l.cues.iter().find(|c| &c.name == name)) {
            Some(cue) => place(cue.clone()),
            None => merged.missing.push(name.clone()),
        }
    }
    for (name, cue) in &edits.cues {
        let mut cue = cue.clone();
        cue.name = name.clone();
        place(cue);
    }
    fresh.sort_by_position();
    merged
}

/// What [`merge`] did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Merged {
    pub kept: Vec<String>,
    pub missing: Vec<String>,
}

/// Resolves every position in a list against `song` — the load-time
/// half of `r[song.relative-position]`. Returns the names whose section
/// the arrangement no longer has.
// r[impl song.relative-position.resolved-on-load]
pub fn reposition(list: &mut CueList, song: &SongMap) -> Vec<String> {
    list.resolve_positions(song)
}

/// An older show plus its `<song>.positions.json`: the sidecar's
/// positions are written into the list and resolved. After this the
/// list carries them itself and the sidecar is not needed again.
// r[impl song.relative-position.resolved-on-load]
// r[impl files.additive-evolution] - an old file and its sidecar still load
pub fn reposition_from_sidecar(
    list: &mut CueList,
    positions: &Positions,
    song: &SongMap,
) -> Vec<String> {
    positions.apply(list, song)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real project this was built against. Skipped rather than
    /// failed when it is not on this machine — it lives in Downloads,
    /// not in the repo, and a test that fails on a colleague's checkout
    /// teaches people to ignore failures.
    fn project_song() -> Option<ignition_core::SongMap> {
        let path = concat!(env!("HOME"), "/Downloads/Bye Bye Bye/Bye Bye Bye.RPP");
        std::path::Path::new(path)
            .exists()
            .then(|| ignition_daw_reaper::load(path).expect("the project parses"))
    }

    /// The shipped show is self-describing: every cue and trigger
    /// carries its relative position, and re-resolving them against the
    /// same arrangement lands on the bars the file already caches. This
    /// is the load path a player takes.
    /// r[verify song.relative-position.resolved-on-load]
    /// r[verify song.relative-position]
    #[test]
    fn the_shipped_show_resolves_to_its_own_cached_bars() {
        let Some(song) = project_song() else { return };
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../data/songs");
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
        let unresolved = reposition(&mut list, &song);
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
    use ignition_core::music::Position;
    use ignition_core::{Bars, Section, TempoMap};

    fn cue(name: &str, bar: u32, fade: f32) -> Cue {
        Cue {
            name: name.into(),
            fade_secs: fade,
            at: Some(Bars::bar(bar).into()),
            ..Default::default()
        }
    }

    fn list(cues: Vec<Cue>) -> CueList {
        CueList {
            name: "t".into(),
            cues,
            triggers: Vec::new(),
            ..Default::default()
        }
    }

    /// r[verify song.generate.is-a-draft]
    #[test]
    fn regenerating_keeps_the_edited_cues_from_a_file_on_disk() {
        // Last week's file, with the chorus hand-tuned and a cue added.
        let existing = list(vec![
            cue("VS 1", 11, 2.0),
            cue("CH 1", 23, 0.05),
            cue("· blackout", 60, 0.0),
        ]);
        let dir = std::env::temp_dir().join(format!("ignition-draft-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let show = dir.join("show.json");
        std::fs::write(&show, serde_json::to_string(&existing).unwrap()).unwrap();
        let edits_path = dir.join("show.edits.json");
        std::fs::write(&edits_path, r#"{ "keep": ["CH 1", "· blackout", "gone"] }"#).unwrap();

        // This week's draft: the generator moved on.
        let mut fresh = list(vec![cue("VS 1", 11, 3.0), cue("CH 1", 23, 0.25)]);
        let edits = Edits::load(&edits_path).unwrap();
        let existing: CueList =
            serde_json::from_str(&std::fs::read_to_string(&show).unwrap()).unwrap();
        let merged = merge(&mut fresh, Some(&existing), &edits);
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(merged.kept, ["CH 1", "· blackout"]);
        assert_eq!(merged.missing, ["gone"]);
        let names: Vec<&str> = fresh.cues.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["VS 1", "CH 1", "· blackout"]);
        // The regenerated verse won; the edited chorus was kept verbatim.
        assert_eq!(fresh.cues[0].fade_secs, 3.0);
        assert_eq!(fresh.cues[1].fade_secs, 0.05);
    }

    #[test]
    fn inline_edits_win_over_everything() {
        let mut fresh = list(vec![cue("CH 1", 23, 0.25)]);
        let existing = list(vec![cue("CH 1", 23, 0.05)]);
        let mut edits = Edits {
            keep: vec!["CH 1".into()],
            cues: BTreeMap::new(),
        };
        edits.cues.insert("CH 1".into(), cue("CH 1", 23, 1.5));
        merge(&mut fresh, Some(&existing), &edits);
        assert_eq!(fresh.cues.len(), 1);
        assert_eq!(fresh.cues[0].fade_secs, 1.5);
    }

    /// r[verify song.relative-position.resolved-on-load]
    #[test]
    fn a_kept_cue_still_moves_with_its_section() {
        let song = |chorus_at: u32| SongMap {
            name: "t".into(),
            tempo: TempoMap::default(),
            sections: vec![Section {
                name: "CH 1".into(),
                start: Bars::bar(chorus_at),
                bars: 8.0,
            }],
        };
        let mut l = list(vec![cue("CH 1", 23, 0.05), cue("· lift", 27, 1.0)]);
        // An old file: absolute bars plus a sidecar.
        let positions = Positions::of(&l, &song(23));
        assert_eq!(positions.cues["· lift"], Position::at("CH 1", 4));
        let unresolved = reposition_from_sidecar(&mut l, &positions, &song(21));
        assert!(unresolved.is_empty());
        assert_eq!(l.cues[0].position(), Some(Bars::bar(21)));
        assert_eq!(l.cues[1].position(), Some(Bars::bar(25)));
        // And now the file itself says where it is, relative.
        assert_eq!(l.cues[1].at, Some(Position::at("CH 1", 4)));
        let unresolved = reposition(&mut l, &song(30));
        assert!(unresolved.is_empty());
        assert_eq!(l.cues[1].position(), Some(Bars::bar(34)));
    }
}
