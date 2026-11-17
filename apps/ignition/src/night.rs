//! A night: the `.ig-show` and the Live set its songs are songs of.

use anyhow::{Context, Result};
use ignition_core::CueList;
use ignition_daw::ableton::{LiveSet, LiveSetSong};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The parts of a `.ig-show` the runner reads. Unknown keys are the
/// show file's business (`r[files.additive-evolution]`).
#[derive(Debug, Deserialize)]
struct ShowDoc {
    #[serde(default)]
    name: String,
    venue: String,
    songs: Vec<SongDoc>,
    #[serde(default)]
    sets: Vec<SetDoc>,
}

#[derive(Debug, Deserialize)]
struct SongDoc {
    file: String,
    /// The song's locator in the Live set.
    locator: String,
    /// What the night calls it.
    #[serde(default)]
    song: String,
    #[serde(default)]
    set: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetDoc {
    pub name: String,
    #[serde(default = "lit")]
    pub lit: bool,
    #[serde(default)]
    pub songs: Vec<String>,
}

const fn lit() -> bool {
    true
}

/// One lit song: where it is on the set, and its cues.
pub struct NightSong {
    pub title: String,
    pub set: String,
    pub live: LiveSetSong,
    pub list: CueList,
}

/// The night, in running order.
pub struct Night {
    pub name: String,
    pub venue: PathBuf,
    pub set: LiveSet,
    pub sets: Vec<SetDoc>,
    pub songs: Vec<NightSong>,
}

impl Night {
    /// Reads the show and the Live set, and matches each song to its
    /// range of the set. Each cue list's relative positions are resolved
    /// against the set as it is now (`r[song.relative-position.resolved-on-load]`);
    /// what does not resolve is warned and plays at its stored bar.
    ///
    /// # Errors
    ///
    /// Either file cannot be read, or a song names a locator the set
    /// does not have.
    pub fn load(show: &Path, live_set: &Path) -> Result<Self> {
        let raw =
            std::fs::read_to_string(show).with_context(|| format!("reading {}", show.display()))?;
        let doc: ShowDoc =
            serde_json::from_str(&raw).with_context(|| format!("parsing {}", show.display()))?;
        let dir = show.parent().unwrap_or_else(|| Path::new("."));
        let set = LiveSet::read(live_set)?;
        let mut songs = Vec::new();
        for entry in doc.songs {
            let live = set
                .song(&entry.locator)
                .with_context(|| {
                    format!(
                        "{}: the Live set has no song {:?}",
                        live_set.display(),
                        entry.locator
                    )
                })?
                .clone();
            let path = dir.join(&entry.file);
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let mut list: CueList = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", path.display()))?;
            for problem in list.resolve_positions(&live.song) {
                tracing::warn!(song = %live.name, "{problem}");
            }
            songs.push(NightSong {
                title: if entry.song.is_empty() {
                    live.name.clone()
                } else {
                    entry.song
                },
                set: entry.set,
                live,
                list,
            });
        }
        Ok(Self {
            name: doc.name,
            venue: dir.join(doc.venue),
            set,
            sets: doc.sets,
            songs,
        })
    }

    /// The lit song a beat of the set falls in.
    #[must_use]
    pub fn song_at(&self, set_beat: f64) -> Option<usize> {
        self.songs.iter().position(|s| s.live.contains(set_beat))
    }
}
