//! The venue's console show, as desk banks.
//!
//! Riverside came with a myDMX 5 show, translated into
//! `data/shows/riverside-desk.json` with every scene named
//! `"<BANK> · <scene>"`. Live shows those scenes bank by bank beside the
//! profile's looks, so a night at that room can be run the way its old
//! desk ran it with the profile's busking layered on top. A desk scene
//! is a cue in a playback — the Show-class one, the base of the stack —
//! and firing one is `Command::DeskScene(index)`.

// Nothing is dead here; it is mounted when `main.rs` hosts `live::Views`
// (and its stylesheet, `live::LIVE_CSS`). Until the integrator wires
// that, the crate root does not reach these items. Remove once mounted.

// r[impl studio.live.desk-scenes] - the console show surfaces as banks of cues

use ignition_core::CueList;

/// One scene of the desk: its index in the cue list — what `DeskScene`
/// fires — and the name without its bank prefix.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Scene {
    pub index: usize,
    pub name: String,
}

/// A bank of the old desk: what one page of its buttons held.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Bank {
    pub name: String,
    pub scenes: Vec<Scene>,
}

/// The separator the translation put between bank and scene.
const SEP: &str = " · ";

/// Split a cue name into its bank and scene. A name with no separator
/// is a bank of its own — "Blackout" is one.
#[must_use]
pub fn split(name: &str) -> (&str, &str) {
    match name.split_once(SEP) {
        Some((bank, scene)) => (bank.trim(), scene.trim()),
        None => (name.trim(), name.trim()),
    }
}

/// Group a cue list into banks, in order of first appearance, each
/// bank's scenes in list order.
#[must_use]
pub fn banks(list: &CueList) -> Vec<Bank> {
    let mut out: Vec<Bank> = Vec::new();
    for (index, cue) in list.cues.iter().enumerate() {
        let (bank, scene) = split(&cue.name);
        let scene = Scene {
            index,
            name: scene.to_string(),
        };
        match out.iter_mut().find(|b| b.name == bank) {
            Some(b) => b.scenes.push(scene),
            None => out.push(Bank {
                name: bank.to_string(),
                scenes: vec![scene],
            }),
        }
    }
    out
}

/// Where a venue's desk show lives, if it has one: the venue directory's
/// name under `data/shows/<venue>-desk.json`.
#[must_use]
pub fn path_for_venue(venue_dir: &str) -> Option<std::path::PathBuf> {
    let name = std::path::Path::new(venue_dir).file_name()?.to_str()?;
    let path = std::path::PathBuf::from(format!("data/shows/{name}-desk.json"));
    path.exists().then_some(path)
}

/// The desk show for a venue, as banks. Empty when the venue came with
/// no desk — most do not.
pub fn load(venue_dir: &str) -> Vec<Bank> {
    let Some(path) = path_for_venue(venue_dir) else {
        return Vec::new();
    };
    match load_list(&path) {
        Ok(list) => banks(&list),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "desk show does not load");
            Vec::new()
        }
    }
}

/// The desk cue list itself — what the engine plays.
///
/// # Errors
///
/// The file cannot be read, or its JSON does not parse as a `CueList`.
pub fn load_list(path: &std::path::Path) -> anyhow::Result<CueList> {
    let raw = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn riverside() -> CueList {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/shows/riverside-desk.json"
        );
        load_list(std::path::Path::new(path)).expect("riverside desk show")
    }

    /// r[verify studio.live.desk-scenes]
    #[test]
    fn riverside_groups_into_its_desk_banks() {
        let banks = banks(&riverside());
        let names: Vec<&str> = banks.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"FIRST"), "{names:?}");
        assert!(names.contains(&"Main Scenes"), "{names:?}");
        assert!(names.contains(&"Movers"), "{names:?}");
        // Every cue is in exactly one bank, and indices are the list's.
        let total: usize = banks.iter().map(|b| b.scenes.len()).sum();
        assert_eq!(total, 154);
        let mut seen: Vec<usize> = banks
            .iter()
            .flat_map(|b| b.scenes.iter().map(|s| s.index))
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..154).collect::<Vec<_>>());
        // The prefix is stripped from the scene's own name.
        let first = banks.iter().find(|b| b.name == "FIRST").unwrap();
        assert!(first.scenes.iter().all(|s| !s.name.contains(SEP)));
    }

    #[test]
    fn a_name_without_a_bank_is_its_own_bank() {
        assert_eq!(split("Blackout"), ("Blackout", "Blackout"));
        assert_eq!(split("FX  2 · 3 Sweep"), ("FX  2", "3 Sweep"));
    }

    #[test]
    fn only_a_venue_with_a_desk_has_banks() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let banks = {
            let _guard = std::env::current_dir().unwrap();
            // Paths in this module are cwd-relative like the studio's
            // others; resolve through the workspace root explicitly here.
            let path = std::path::Path::new(root).join("data/shows/riverside-desk.json");
            super::banks(&load_list(&path).unwrap())
        };
        assert!(!banks.is_empty());
        assert!(path_for_venue("/nowhere/venues/nothing").is_none());
    }
}
