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
    /// The set's library, relative to the show; default
    /// `../library/<show>.json`.
    #[serde(default)]
    library: Option<String>,
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
    /// The song's file: read again when it changes while the show runs.
    pub path: PathBuf,
}

/// The pools a show plays from before any library is laid over them:
/// the room's palettes and the profile's effects, looks and tricks.
#[derive(Clone)]
pub struct Base {
    pub palettes: ignition_core::Palettes,
    pub library: std::collections::BTreeMap<String, ignition_core::Recipe>,
    pub looks: std::collections::BTreeMap<String, ignition_core::profile::Look>,
    pub tricks: std::collections::BTreeMap<String, Vec<ignition_core::Trick>>,
}

/// When a song's file and the set's library were last written: a change
/// of either is the show edited under the renderer.
pub type Stamp = (Option<std::time::SystemTime>, Option<std::time::SystemTime>);

/// How a room looks to the camera, from its manifest's `look`:
/// `haze` (0 a clear room, 1 normally hazed) and `exposure` (stops on
/// the stage exposure) and `output` (what its rig delivers of nominal). A
/// room that says nothing gets the renderer's own.
#[must_use]
pub fn venue_look(venue_dir: &Path) -> (Option<f32>, Option<f32>, Option<f32>) {
    let look = std::fs::read_to_string(venue_dir.join("venue.ig-venue"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|doc| doc.get("look").cloned());
    let number = |key: &str| {
        look.as_ref()
            .and_then(|l| l.get(key))
            .and_then(serde_json::Value::as_f64)
            .map(|v| {
                #[expect(
                    clippy::as_conversions,
                    clippy::cast_possible_truncation,
                    reason = "a dial between 0 and a few"
                )]
                let v = v as f32;
                v
            })
    };
    (number("haze"), number("exposure"), number("output"))
}

/// The room's folder, for what the running room writes back into it
/// (a camera view saved from the pane).
pub static VENUE_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// The room's own fill with every fixture off (`look.ambient`, where one
/// is a dim worklight): a room with no other light source says a small
/// number, so dark is dark.
#[must_use]
pub fn venue_ambient(venue_dir: &Path) -> Option<f32> {
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(venue_dir.join("venue.ig-venue")).ok()?).ok()?;
    let v = doc.get("look")?.get("ambient")?.as_f64()?;
    v.to_string().parse::<f32>().ok()
}

impl NightSong {
    /// The song's cue list as its file has it now, resolved against the
    /// set; `None` when the file does not read (a half-made edit): the
    /// list in hand keeps playing.
    #[must_use]
    pub fn reload(&self) -> Option<CueList> {
        let raw = std::fs::read_to_string(&self.path).ok()?;
        let mut list: CueList = serde_json::from_str(&raw).ok()?;
        for problem in list.resolve_positions(&self.live.song) {
            tracing::warn!(song = %self.live.name, "{problem}");
        }
        Some(list)
    }
}

/// The night, in running order.
pub struct Night {
    pub name: String,
    pub venue: PathBuf,
    pub set: LiveSet,
    pub sets: Vec<SetDoc>,
    pub songs: Vec<NightSong>,
    /// The set's library file (it need not exist).
    pub library: PathBuf,
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
                path,
            });
        }
        let library = dir.join(doc.library.unwrap_or_else(|| {
            let stem = show.file_stem().and_then(|s| s.to_str()).unwrap_or("set");
            format!("../library/{stem}.json")
        }));
        Ok(Self {
            library,
            name: doc.name,
            venue: dir.join(doc.venue),
            set,
            sets: doc.sets,
            songs,
        })
    }

    /// The library a song plays from: its own overrides (`library` in
    /// its file) over the set's. Empty when neither is there.
    #[must_use]
    pub fn library_for(&self, song: &NightSong) -> ignition_core::library::Library {
        use ignition_core::library::Library;
        let set: Library = std::fs::read_to_string(&self.library)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let own: Library = std::fs::read_to_string(&song.path)
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|doc| doc.get("library").cloned())
            .and_then(|lib| serde_json::from_value(lib).ok())
            .unwrap_or_default();
        own.over(&set)
    }

    /// When the song's file and the library were last written.
    #[must_use]
    pub fn stamp(&self, song: &NightSong) -> Stamp {
        let at = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        (at(&song.path), at(&self.library))
    }

    /// The lit song a beat of the set falls in.
    #[must_use]
    pub fn song_at(&self, set_beat: f64) -> Option<usize> {
        self.songs.iter().position(|s| s.live.contains(set_beat))
    }
}
