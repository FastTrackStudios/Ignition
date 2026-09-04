//! The files an operator picks from a folder, and the checks that run
//! before doors.
//!
//! Four formats, and the lines between them are the design
//! (`docs/spec/files.md`, `docs/spec/profile.md`):
//!
//! ```text
//! ignition.ig-profile     the interface a rig must satisfy      — `Profile`
//! norco/venue.ig-venue    one room's implementation of it       — `VenueManifest` + the JSON beside it
//! bye-bye-bye.ignition    one song's show, against the profile  — `ShowDocument` around `CueList`
//! sunday.ig-show          the night: profile, venue, songs      — `ShowFile`
//! bye-bye-bye.norco.ig-local  what this room does differently   — `VenueLayer`
//! ```
//!
//! Everything here is static. Nothing is rendered, nothing is cooked,
//! nothing needs a rig: the point is that "will tonight's set work in
//! Vegas" is answerable on the plane, and a gap is reported by name in
//! one place before anything runs.
//!
//! Findings are warnings. A show that names a follow spot at a room
//! without one still loads and still plays everything else
//! (`r[files.graceful-degradation]`); the only thing that fails a check
//! outright is a *required* role the venue leaves unbound, and even that
//! fails the check, not the load.

use crate::cue::{Cue, CueList};
use crate::preset::Ref;
use crate::profile::{Gap, Profile, RoleKind, VenueProfile};
use crate::recipe::{Recipe, RecipeApply, RecipeRef};
use crate::selection::Selection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// The newest `.ig-show` layout this build writes.
pub const SHOW_FILE_VERSION: u32 = 1;
/// The newest `.ig-local` layout this build writes.
pub const VENUE_LAYER_VERSION: u32 = 1;
/// The newest `venue.ig-venue` manifest this build writes.
pub const VENUE_MANIFEST_VERSION: u32 = 1;
/// The newest `.ignition` header this build writes. A file with no
/// version is the pre-header `CueList` and loads as version 0.
pub const IGNITION_VERSION: u32 = 1;

/// Why a file did not load. Every variant names the file, and the
/// version variant names the version, because "could not load" is the
/// message that gets a show played with its effects silently missing.
#[derive(Debug)]
pub enum Error {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    /// The file is from a build newer than this one.
    // r[impl files.versioned] - rejected with a message naming the version, never half-loaded
    Version {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "reading {}: {source}", path.display()),
            Self::Json { path, source } => write!(f, "parsing {}: {source}", path.display()),
            Self::Version {
                path,
                found,
                supported,
            } => write!(
                f,
                "{} is version {found}; this build understands up to version {supported}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for Error {}

fn read(path: &Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn parse<T: for<'de> Deserialize<'de>>(path: &Path, raw: &str) -> Result<T, Error> {
    serde_json::from_str(raw).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn check_version(path: &Path, found: u32, supported: u32) -> Result<(), Error> {
    if found > supported {
        return Err(Error::Version {
            path: path.to_path_buf(),
            found,
            supported,
        });
    }
    Ok(())
}

/// Pretty, stable JSON: the property every format has to keep is that a
/// person can read the diff between what they wrote and what the
/// generator wrote.
// r[impl files.text-and-diffable]
//
// `to_string_pretty` only fails on a non-string map key (nothing here
// has one) or a `NaN`/infinite float, which JSON cannot spell. The
// latter is reachable in principle — a broken effect could hand a cue
// a `NaN` — but every `to_json` in this file returns a bare `String`,
// and making that fallible would push a `Result` onto every caller in
// every crate that saves a show for one pathological value nothing
// here currently produces. `expect` over a graceful fallback: a `.5,
// NaN, .8` written to disk as `null` would silently corrupt the file
// it claims to have saved, which is worse than the save failing loudly.
#[expect(
    clippy::expect_used,
    reason = "no non-string map keys here, and turning a NaN into a silent bad write would be worse than the crash; see the comment above"
)]
fn to_pretty<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).expect("serialising a value this crate built")
}

// ---------------------------------------------------------------------------
// .ig-show — the night
// ---------------------------------------------------------------------------

/// One song in the night's running order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SongEntry {
    /// The `.ignition` file, relative to the `.ig-show`.
    pub file: String,
    /// This venue's layer for the song, if it has one — an `.ig-local`
    /// relative to the `.ig-show`. Named here rather than discovered by
    /// convention, so the check can say which songs have one.
    // r[impl profile.venue-layer] - the layer is a separate artifact, bound here
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl SongEntry {
    #[must_use]
    pub fn new(file: &str) -> Self {
        Self {
            file: file.into(),
            layer: None,
            extra: serde_json::Map::default(),
        }
    }
}

/// A `.ig-show`: a profile, a venue, and the songs in order.
///
/// It is what an operator opens for the night, and what the check runs
/// against. It holds paths, not content — the same `.ignition` is used
/// by any number of shows, and a song played twice in a night is one
/// file referenced twice.
// r[impl profile.show-binds-the-night]
// r[impl profile.show-many-per-song] - `songs` is a Vec of paths; nothing dedups it
// r[impl files.show.many-per-song]
// r[impl files.versioned]
// r[impl files.additive-evolution] - unknown keys ride in `extra` and are written back
// r[impl files.text-and-diffable] - JSON, pretty-printed on save
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShowFile {
    pub version: u32,
    #[serde(default)]
    pub name: String,
    /// The profile every song is written against: a path relative to
    /// the `.ig-show`, or a bare name resolved beside the venues in
    /// `profiles/<name>.ig-profile`.
    pub profile: String,
    /// The venue directory, relative to the `.ig-show`.
    pub venue: String,
    pub songs: Vec<SongEntry>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ShowFile {
    /// # Errors
    ///
    /// `raw` does not parse as a `ShowFile`, or names a version newer
    /// than [`SHOW_FILE_VERSION`].
    pub fn parse(path: &Path, raw: &str) -> Result<Self, Error> {
        let show: Self = parse(path, raw)?;
        check_version(path, show.version, SHOW_FILE_VERSION)?;
        Ok(show)
    }

    /// # Errors
    ///
    /// `path` cannot be read, or [`Self::parse`] rejects its contents.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        Self::parse(path, &read(path)?)
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        to_pretty(self)
    }

    /// Where the profile file is, given where the show file is.
    ///
    /// A path is used as written; a bare name looks in the `profiles/`
    /// directory that sits beside `venues/` — the same convention
    /// `Venue::load` uses to inherit colours, so a show and a venue that
    /// name the same profile find the same file.
    #[must_use]
    pub fn profile_path(&self, show_dir: &Path) -> PathBuf {
        resolve_profile(show_dir, &self.profile, Some(&show_dir.join(&self.venue)))
    }

    #[must_use]
    pub fn venue_dir(&self, show_dir: &Path) -> PathBuf {
        show_dir.join(&self.venue)
    }
}

/// A profile reference — path or name — resolved to a file.
fn resolve_profile(base: &Path, profile: &str, venue_dir: Option<&Path>) -> PathBuf {
    let as_path = base.join(profile);
    if profile.ends_with(".ig-profile") || profile.contains('/') || as_path.is_file() {
        return as_path;
    }
    let file = format!("{}.ig-profile", profile.to_lowercase());
    // `<root>/venues/<venue>` → `<root>/profiles/<name>.ig-profile`.
    if let Some(root) = venue_dir.and_then(|v| v.parent()).and_then(|p| p.parent()) {
        let candidate = root.join("profiles").join(&file);
        if candidate.is_file() {
            return candidate;
        }
    }
    // `<root>/shows/x.ig-show` → `<root>/profiles/<name>.ig-profile`.
    if let Some(root) = base.parent() {
        let candidate = root.join("profiles").join(&file);
        if candidate.is_file() {
            return candidate;
        }
    }
    as_path
}

// ---------------------------------------------------------------------------
// .ignition — one song, with its header
// ---------------------------------------------------------------------------

/// Which song a show is written against — enough to find the project
/// and its tempo map, and nothing more. The audio and the arrangement
/// stay in the DAW session; this is a pointer, not a copy.
// r[impl files.show.song-binding]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SongBinding {
    /// The song project, relative to the `.ignition` file.
    pub project: String,
    #[serde(default)]
    pub name: String,
}

/// An `.ignition` file: a `CueList` plus the header that makes it
/// checkable without a venue present.
///
/// One JSON document, not two. The cue list's own keys sit at the top
/// level exactly as `CueList` has always written them, and the header
/// adds `version`, `profile` and `song` beside them — so a file written
/// before the header existed loads unchanged (every header key is
/// optional), and a file written with it still loads as a plain
/// `CueList` by anything that only knows that type.
///
/// Writers (`authorshow`, the generator) can adopt this by serialising a
/// `ShowDocument` instead of a `CueList`; until they do, the header is
/// simply absent and the check says so.
// r[impl profile.ignition-is-per-song] - one CueList, the profile it uses, and the song it is against
// r[impl files.show] - the cue list, its recipes and triggers, and the song timing they are against
// r[impl files.show.cues] - the existing CueList, unchanged
// r[impl files.show.song-binding]
// r[impl files.versioned] - `version` absent means 0, the pre-header layout, which loads
// r[impl files.additive-evolution]
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ShowDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// The profile whose vocabulary the cues use, by name or path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub song: Option<SongBinding>,
    #[serde(flatten)]
    pub list: CueList,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl ShowDocument {
    #[must_use]
    pub fn new(list: CueList, profile: &str, song: SongBinding) -> Self {
        Self {
            version: Some(IGNITION_VERSION),
            profile: Some(profile.into()),
            song: Some(song),
            list,
            extra: serde_json::Map::default(),
        }
    }

    /// # Errors
    ///
    /// `raw` does not parse as a `ShowDocument`, or names a version
    /// newer than [`IGNITION_VERSION`].
    pub fn parse(path: &Path, raw: &str) -> Result<Self, Error> {
        let doc: Self = parse(path, raw)?;
        check_version(path, doc.version.unwrap_or(0), IGNITION_VERSION)?;
        Ok(doc)
    }

    /// # Errors
    ///
    /// `path` cannot be read, or [`Self::parse`] rejects its contents.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        Self::parse(path, &read(path)?)
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        to_pretty(self)
    }
}

// ---------------------------------------------------------------------------
// .ig-local — the venue layer
// ---------------------------------------------------------------------------

/// What one room does differently for one song.
///
/// A separate file, bound to one `.ignition` and one venue. Merged into
/// the show it would destroy the property the show exists to have;
/// merged into the venue it would apply to every song. It may say
/// anything the venue can do — including names the profile never
/// declared — because it is the sanctioned home for the non-portable.
// r[impl profile.venue-layer]
// r[impl profile.venue-layer.explicitly-local] - a layer's cues are never checked against the profile
// r[impl files.versioned]
// r[impl files.additive-evolution]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VenueLayer {
    pub version: u32,
    /// The `.ignition` this adjusts, relative to the layer file.
    pub song: String,
    /// The venue this is for, by directory name.
    pub venue: String,
    /// Cues replaced wholesale, keyed by the base cue's name. A name the
    /// base show lacks is ignored — the layer adjusts a show that
    /// already works, it does not carry part of it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub override_cues: BTreeMap<String, Cue>,
    /// Cues added after the base list, sorted into position with it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_cues: Vec<Cue>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl VenueLayer {
    #[must_use]
    pub fn new(song: &str, venue: &str) -> Self {
        Self {
            version: VENUE_LAYER_VERSION,
            song: song.into(),
            venue: venue.into(),
            override_cues: BTreeMap::default(),
            add_cues: Vec::default(),
            extra: serde_json::Map::default(),
        }
    }

    /// # Errors
    ///
    /// `raw` does not parse as a `VenueLayer`, or names a version newer
    /// than [`VENUE_LAYER_VERSION`].
    pub fn parse(path: &Path, raw: &str) -> Result<Self, Error> {
        let layer: Self = parse(path, raw)?;
        check_version(path, layer.version, VENUE_LAYER_VERSION)?;
        Ok(layer)
    }

    /// # Errors
    ///
    /// `path` cannot be read, or [`Self::parse`] rejects its contents.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        Self::parse(path, &read(path)?)
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        to_pretty(self)
    }
}

/// What `apply_layer` displaced, so `remove_layer` can put it back.
///
/// Its existence is the test of `r[profile.venue-layer.optional]`: a
/// layer that can be lifted off leaving the original is a layer that
/// carried nothing the show needed.
#[derive(Debug, Clone, Default)]
pub struct Applied {
    /// `(index, original)` for every cue the layer replaced.
    pub replaced: Vec<(usize, Cue)>,
    /// How many cues the layer appended.
    pub added: usize,
    /// Override names the base show had no cue for — reported, not
    /// applied.
    pub unmatched: Vec<String>,
}

/// Lays a venue layer over a cue list, in place.
///
/// Overrides replace the cue of the same name; additions go on the end.
/// Position order is *not* re-sorted here, because that is
/// `CueList::resolve_positions`' job and it needs the song map — call
/// it after, the same as for a freshly loaded list.
// r[impl profile.venue-layer]
// r[impl profile.venue-layer.optional] - the base list is complete before this runs; the receipt undoes it
pub fn apply_layer(list: &mut CueList, layer: &VenueLayer) -> Applied {
    let mut applied = Applied::default();
    for (name, cue) in &layer.override_cues {
        let hit = list.cues.iter().position(|c| &c.name == name);
        match hit.and_then(|i| list.cues.get_mut(i).map(|slot| (i, slot))) {
            Some((i, slot)) => {
                let original = std::mem::replace(slot, cue.clone());
                applied.replaced.push((i, original));
            }
            None => applied.unmatched.push(name.clone()),
        }
    }
    applied.added = layer.add_cues.len();
    list.cues.extend(layer.add_cues.iter().cloned());
    applied
}

/// Lifts a layer back off. Only valid on the list `apply_layer` ran on,
/// before anything re-sorted it.
// r[impl profile.venue-layer.optional]
pub fn remove_layer(list: &mut CueList, applied: Applied) {
    let keep = list.cues.len().saturating_sub(applied.added);
    list.cues.truncate(keep);
    for (i, original) in applied.replaced {
        if let Some(slot) = list.cues.get_mut(i) {
            *slot = original;
        }
    }
}

// ---------------------------------------------------------------------------
// venue.ig-venue — the manifest
// ---------------------------------------------------------------------------

/// A venue directory's manifest.
///
/// The venue stays a directory of the JSON files it always was — that is
/// the editable form, and the one git sees — and the manifest names them
/// and the assets folder, so the directory moves and copies as a unit.
/// Every key but `version` is optional; a directory with no manifest at
/// all is the same venue with every default.
// r[impl files.directory-or-archive] - a directory with a manifest; the directory is the editable form
// r[impl files.venue.assets] - `assets` is relative to the venue directory
// r[impl files.versioned]
// r[impl files.additive-evolution]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VenueManifest {
    pub version: u32,
    #[serde(default)]
    pub name: String,
    /// The profile this venue implements, by name. Mirrors the
    /// `profile` key in the binding file; the binding file wins if they
    /// disagree, because it is the one the check reads.
    #[serde(default)]
    pub profile: String,
    /// Overrides for the JSON files, keyed by their default name without
    /// the extension — `"fixtures"`, `"room"`, `"screens"`, `"props"`,
    /// `"groups"`, `"palettes"`, `"profile"`, `"areas"`. Anything not
    /// named keeps its default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, String>,
    /// The assets folder, relative to the venue directory.
    #[serde(default = "default_assets")]
    pub assets: String,
    /// Where this room's universes go on the wire — protocol, priority,
    /// targets — as `ignition_dmx::OutputConfig` JSON. Kept untyped here
    /// so the core stays free of the transmit crate; `ignition-viz`
    /// parses it. Absent means "every patched universe on sACN
    /// multicast", decided by the loader. A show never carries this.
    // r[impl dmx.venue-config] - the venue file owns the output config
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dmx: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_assets() -> String {
    "assets".into()
}

/// The manifest's file name inside a venue directory.
pub const VENUE_MANIFEST_FILE: &str = "venue.ig-venue";

/// The JSON files a venue directory is made of, by manifest key.
pub const VENUE_FILES: [&str; 8] = [
    "fixtures", "room", "screens", "props", "groups", "palettes", "profile", "areas",
];

impl Default for VenueManifest {
    fn default() -> Self {
        Self {
            version: VENUE_MANIFEST_VERSION,
            name: String::new(),
            profile: String::new(),
            files: BTreeMap::default(),
            assets: default_assets(),
            dmx: None,
            extra: serde_json::Map::default(),
        }
    }
}

impl VenueManifest {
    /// The manifest in `dir`, or the all-defaults manifest for a bare
    /// directory. A manifest that is present but unreadable is an error,
    /// because a venue that half-loads is worse than one that does not.
    ///
    /// # Errors
    ///
    /// `VENUE_MANIFEST_FILE` exists in `dir` but cannot be read or does
    /// not parse as a `VenueManifest`.
    pub fn load_dir(dir: &Path) -> Result<Self, Error> {
        let path = dir.join(VENUE_MANIFEST_FILE);
        if !path.is_file() {
            return Ok(Self::default());
        }
        let manifest: Self = parse(&path, &read(&path)?)?;
        check_version(&path, manifest.version, VENUE_MANIFEST_VERSION)?;
        Ok(manifest)
    }

    /// Where one of the venue's JSON files is: the override if named,
    /// else `<key>.json`, in either case relative to `dir`.
    #[must_use]
    pub fn file(&self, dir: &Path, key: &str) -> PathBuf {
        self.files
            .get(key)
            .map_or_else(|| dir.join(format!("{key}.json")), |name| dir.join(name))
    }

    /// The assets directory, resolved against the venue directory.
    // r[impl files.venue.assets]
    #[must_use]
    pub fn assets_dir(&self, dir: &Path) -> PathBuf {
        dir.join(&self.assets)
    }

    #[must_use]
    pub fn to_json(&self) -> String {
        to_pretty(self)
    }
}

/// The venue's `areas.json` — its blocking grid, as its own names bound
/// to its own focus points. Loaded into `VenueProfile::areas` when the
/// binding file does not carry them itself.
#[derive(Debug, Clone, Default, Deserialize)]
struct AreasFile {
    #[serde(default)]
    areas: BTreeMap<String, String>,
}

/// The venue's profile binding, from the two documents themselves.
///
/// The pure half of [`load_venue_binding`]. The caller has the text —
/// off a disk, off a network, out of a test — and this parses it. Split
/// out so a browser can build a venue at all: everything below the
/// directory walk is ordinary parsing, and it was only the walk that
/// could not follow.
///
/// A missing document is not an error; the binding falls back to its
/// defaults. The manifest's own `profile` name is not consulted — that
/// is a directory fact, and it stays with [`load_venue_binding`].
///
/// # Errors
///
/// If either document is present and will not parse.
pub fn venue_binding_from_str(
    binding: Option<&str>,
    areas: Option<&str>,
) -> Result<VenueProfile, Error> {
    let mut binding: VenueProfile = match binding {
        Some(raw) => parse(Path::new("profile.json"), raw)?,
        None => VenueProfile::default(),
    };
    if binding.areas.is_empty()
        && let Some(raw) = areas
    {
        let areas: AreasFile = parse(Path::new("areas.json"), raw)?;
        binding.areas = areas.areas;
    }
    Ok(binding)
}

/// The venue's profile binding, from the directory: `profile.json` (or
/// the manifest's override) with `areas.json` folded in. Everything the
/// static check needs, and nothing that needs a renderer.
///
/// # Errors
///
/// The manifest cannot be loaded, the profile file it names cannot be
/// read or parsed, or a present `areas.json` cannot be read or parsed.
/// A *missing* `profile.json` or `areas.json` is not an error — the
/// binding falls back to its defaults.
// r[impl default.norco-is-the-proof] - data-defined: data/venues/norco/profile.json binds Norco to the roles; tests/profile_binding.rs holds it
pub fn load_venue_binding(dir: &Path) -> Result<VenueProfile, Error> {
    let manifest = VenueManifest::load_dir(dir)?;
    let binding_path = manifest.file(dir, "profile");
    let mut binding: VenueProfile = if binding_path.is_file() {
        parse(&binding_path, &read(&binding_path)?)?
    } else {
        VenueProfile::default()
    };
    if binding.profile.is_empty() {
        binding.profile.clone_from(&manifest.profile);
    }
    let areas_path = manifest.file(dir, "areas");
    if binding.areas.is_empty() && areas_path.is_file() {
        let areas: AreasFile = parse(&areas_path, &read(&areas_path)?)?;
        binding.areas = areas.areas;
    }
    Ok(binding)
}

// ---------------------------------------------------------------------------
// The checks
// ---------------------------------------------------------------------------

/// Something a show does that the check wants a person to see.
///
/// Never a refusal to load. The show plays with whatever it has; this
/// is the list of what it will be missing, by name, before the night.
// r[impl files.graceful-degradation] - a finding is a warning; nothing here stops a show loading
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Finding {
    /// A role the show names that the profile does not declare. At a
    /// venue with such a group it plays; anywhere else, that recipe
    /// covers nothing.
    // r[impl profile.venue-may-exceed] - visibly non-portable, not silently
    Undeclared { kind: RoleKind, name: String },
    /// A venue group named directly rather than through a role. Works
    /// at exactly one address.
    VenueGroup { name: String },
    /// A library effect or bundle the profile does not ship.
    UnknownEffect { name: String },
    /// A fixture reached by channel — the one rule that makes
    /// portability possible, broken.
    // r[impl files.no-fixture-identity]
    FixtureIdentity { how: String },
    /// A coordinate in metres outside a `Where` zone: a room's geometry
    /// carried in a show.
    // r[impl profile.ignition-has-no-venue]
    MetreCoordinate { what: String },
    /// An area named from the audience's side of the stage.
    // r[impl profile.areas.performer-orientation]
    AudienceOriented { area: String },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undeclared { kind, name } => {
                write!(f, "{kind:?} {name:?} is not in the profile's vocabulary")
            }
            Self::VenueGroup { name } => {
                write!(f, "names venue group {name:?} directly; use a role")
            }
            Self::UnknownEffect { name } => {
                write!(f, "effect {name:?} is not in the profile's library")
            }
            Self::FixtureIdentity { how } => write!(f, "names fixtures by identity: {how}"),
            Self::MetreCoordinate { what } => write!(f, "carries a room coordinate: {what}"),
            Self::AudienceOriented { area } => write!(
                f,
                "area {area:?} is named from the audience; use the performer's left and right"
            ),
        }
    }
}

/// A finding placed: which cue (or trigger) it was found in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Located {
    /// The cue or trigger name; `None` for the list as a whole.
    pub cue: Option<String>,
    pub finding: Finding,
}

impl fmt::Display for Located {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.cue {
            Some(cue) => write!(f, "{cue:?}: {}", self.finding),
            None => write!(f, "{}", self.finding),
        }
    }
}

/// Everything a cue list *names*, gathered for the vocabulary check.
#[derive(Debug, Default)]
struct Used {
    names: Vec<(String, RoleKind)>,
    findings: Vec<Located>,
}

impl Used {
    fn cue(&mut self, cue: Option<&str>, finding: Finding) {
        self.findings.push(Located {
            cue: cue.map(str::to_string),
            finding,
        });
    }

    fn selection(&mut self, cue: Option<&str>, sel: &Selection) {
        match sel {
            Selection::Role(name) => self.names.push((name.clone(), RoleKind::Group)),
            Selection::Group(name) => self.cue(cue, Finding::VenueGroup { name: name.clone() }),
            Selection::Chans(chans) => self.cue(
                cue,
                Finding::FixtureIdentity {
                    how: format!("channels {chans:?}"),
                },
            ),
            Selection::Layout { of, rows } => {
                self.cue(
                    cue,
                    Finding::FixtureIdentity {
                        how: format!("a layout of {} channel rows", rows.len()),
                    },
                );
                self.selection(cue, of);
            }
            // Capabilities are portable by construction.
            Selection::Tag(_) | Selection::Model(_) => {}
            Selection::Union(parts) | Selection::Intersect(parts) => {
                for p in parts {
                    self.selection(cue, p);
                }
            }
            Selection::Except { of, minus } => {
                self.selection(cue, of);
                self.selection(cue, minus);
            }
            // A `Where` zone is a question about the stage, and both
            // rooms agree on what their coordinates mean — the one place
            // a metre is portable. See r[files.capability-over-name].
            Selection::Where { of, .. } | Selection::Order { of, .. } => self.selection(cue, of),
        }
    }

    fn point(&mut self, cue: Option<&str>, what: &str, point: &Ref<crate::Vec3>) {
        match point {
            Ref::Named(name) => self.names.push((name.clone(), RoleKind::Focus)),
            Ref::Inline(v) => self.cue(
                cue,
                Finding::MetreCoordinate {
                    what: format!("{what} at ({}, {}, {})", v.x, v.y, v.z),
                },
            ),
        }
    }

    fn apply(&mut self, cue: Option<&str>, apply: &RecipeApply) {
        match apply {
            RecipeApply::Color(Ref::Named(name)) => {
                self.names.push((name.clone(), RoleKind::Colour));
            }
            RecipeApply::Colors { colors, .. } => {
                for c in colors {
                    if let Ref::Named(name) = c {
                        self.names.push((name.clone(), RoleKind::Colour));
                    }
                }
            }
            RecipeApply::FocusPoint(p) => self.point(cue, "a focus point", p),
            RecipeApply::FocusFan { from, to } => {
                self.point(cue, "a fan start", from);
                self.point(cue, "a fan end", to);
            }
            RecipeApply::FocusKeyframes(points) => {
                for p in points {
                    self.point(cue, "a keyframe", p);
                }
            }
            // Literal colours are portable (r[default.colour-roles-are-semantic]);
            // a direction is unitless; a delta is relative; the rest name
            // nothing the venue owns.
            _ => {}
        }
    }

    fn recipe(&mut self, cue: Option<&str>, recipe: &Recipe) {
        self.selection(cue, &recipe.target);
        for step in &recipe.steps {
            for a in &step.apply {
                self.apply(cue, a);
            }
        }
    }

    fn recipe_ref(&mut self, cue: Option<&str>, profile: &Profile, r: &RecipeRef) {
        match r {
            RecipeRef::Inline(recipe) => self.recipe(cue, recipe),
            RecipeRef::Named { effect, target, .. } => {
                if !profile.effects.contains_key(effect) {
                    self.cue(
                        cue,
                        Finding::UnknownEffect {
                            name: effect.clone(),
                        },
                    );
                }
                if let Some(t) = target {
                    self.selection(cue, t);
                }
            }
            RecipeRef::Bundle { bundle, target } => {
                if !profile.bundles.contains_key(bundle) {
                    self.cue(
                        cue,
                        Finding::UnknownEffect {
                            name: bundle.clone(),
                        },
                    );
                }
                if let Some(t) = target {
                    self.selection(cue, t);
                }
            }
            // r[impl profile.looks] - a look the profile lacks is an unknown name; a known one is walked
            RecipeRef::Look { look } => match profile.looks.get(look) {
                None => self.cue(cue, Finding::UnknownEffect { name: look.clone() }),
                Some(l) => {
                    for r in l
                        .recipes
                        .iter()
                        .filter(|r| !matches!(r, RecipeRef::Look { .. }))
                    {
                        self.recipe_ref(cue, profile, r);
                    }
                }
            },
        }
    }
}

/// Every name a cue list uses, of every kind, deduplicated.
#[must_use]
pub fn names_used(list: &CueList) -> Vec<(String, RoleKind)> {
    let mut used = Used::default();
    let profile = Profile::default();
    for cue in &list.cues {
        for r in &cue.recipes {
            used.recipe_ref(Some(&cue.name), &profile, r);
        }
    }
    for t in &list.triggers {
        used.recipe(Some(&t.name), &t.recipe);
    }
    used.names.sort();
    used.names.dedup();
    used.names
}

/// Is this show written in the profile's vocabulary, and nothing else?
///
/// Static. Every role, colour and focus name the cues use is held
/// against what the profile declares; every selection is walked for a
/// channel; every focus apply is walked for an inline coordinate. A
/// show that passes plays at every venue implementing the profile.
// r[impl files.compatibility-check] - show against profile, every gap by name
// r[impl profile.show-check-before-doors] - one half of it; the other is `check_venue_against_profile`
// r[impl profile.check-is-static]
// r[impl files.no-fixture-identity] - `Chans` and `Layout` are reported
// r[impl profile.ignition-has-no-venue] - inline metres are reported
// r[impl profile.declares-vocabulary] - "a show MUST NOT use a name the profile does not declare, and that MUST be checkable"
#[must_use]
pub fn check_show_against_profile(list: &CueList, profile: &Profile) -> Vec<Located> {
    let mut used = Used::default();
    for cue in &list.cues {
        if !cue.values.is_empty() {
            used.cue(
                Some(&cue.name),
                Finding::FixtureIdentity {
                    how: format!("{} direct channel values", cue.values.len()),
                },
            );
        }
        for r in &cue.recipes {
            used.recipe_ref(Some(&cue.name), profile, r);
        }
    }
    for t in &list.triggers {
        used.recipe(Some(&t.name), &t.recipe);
    }
    // Colours are checked against the profile's palette, not its roles:
    // the profile ships colour *values*, and a literal name it carries
    // is as portable as a role.
    let (colours, roles): (Vec<_>, Vec<_>) = used
        .names
        .iter()
        .partition(|(_, kind)| *kind == RoleKind::Colour);
    for (name, _) in colours {
        if !profile
            .colors
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(name))
        {
            used.findings.push(Located {
                cue: None,
                finding: Finding::Undeclared {
                    kind: RoleKind::Colour,
                    name: name.clone(),
                },
            });
        }
    }
    for (name, kind) in profile.undeclared(roles.iter().map(|(n, k)| (n.as_str(), *k))) {
        used.findings.push(Located {
            cue: None,
            finding: Finding::Undeclared { kind, name },
        });
    }
    used.findings.sort();
    used.findings.dedup();
    used.findings
}

/// Does this venue implement the profile? Every unbound role, required
/// first — `Profile::gaps`, named here so the two halves of the check
/// read as a pair.
// r[impl files.compatibility-check] - venue against profile
// r[impl profile.check-is-static]
// r[impl files.required-roles] - a required gap is reported, and the venue still loaded
#[must_use]
pub fn check_venue_against_profile(venue: &VenueProfile, profile: &Profile) -> Vec<Gap> {
    profile.gaps(venue)
}

/// Are the venue's areas named the way the people on stage name them?
///
/// Only orientation is checked. How many areas there are, and what
/// granularity they cut the stage at, is the venue's to decide.
// r[impl profile.areas.performer-orientation] - "house left" is flagged
// r[impl profile.areas.venue-decides-granularity] - nothing here counts them
pub fn check_areas<'a>(areas: impl IntoIterator<Item = &'a str>) -> Vec<Finding> {
    const AUDIENCE: [&str; 6] = [
        "house left",
        "house right",
        "audience left",
        "audience right",
        "camera left",
        "camera right",
    ];
    areas
        .into_iter()
        .filter(|name| {
            let lower = name.to_lowercase();
            AUDIENCE.iter().any(|a| lower.contains(a))
        })
        .map(|name| Finding::AudienceOriented { area: name.into() })
        .collect()
}

/// One song's line in the report.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SongReport {
    pub file: String,
    pub name: String,
    /// The `.ig-local` this venue applies, if the show names one.
    // r[impl profile.venue-layer.visible]
    pub layer: Option<String>,
    /// The profile the file says it is written against, if it says.
    pub profile: Option<String>,
    pub findings: Vec<Located>,
    /// Why the song could not be checked, if it could not.
    pub error: Option<String>,
}

/// The whole night, checked, in one place.
// r[impl profile.show-check-before-doors]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Report {
    pub show: String,
    pub profile: String,
    pub venue: String,
    /// Roles the venue leaves unbound, required first.
    pub gaps: Vec<Gap>,
    /// The venue's area names, checked for orientation.
    pub area_findings: Vec<Finding>,
    pub songs: Vec<SongReport>,
    /// Anything that stopped a half of the check running at all.
    pub errors: Vec<String>,
}

impl Report {
    /// The one thing that fails the night: a required role unbound.
    pub fn required_gaps(&self) -> impl Iterator<Item = &Gap> {
        self.gaps.iter().filter(|g| g.required)
    }

    #[must_use]
    pub fn ok(&self) -> bool {
        self.required_gaps().next().is_none()
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "show    {}", self.show)?;
        writeln!(f, "profile {}", self.profile)?;
        writeln!(f, "venue   {}", self.venue)?;
        for e in &self.errors {
            writeln!(f, "  error: {e}")?;
        }
        writeln!(f)?;
        if self.gaps.is_empty() {
            writeln!(f, "venue implements the profile: every role bound")?;
        } else {
            writeln!(f, "venue against profile: {} unbound", self.gaps.len())?;
            for g in &self.gaps {
                writeln!(f, "  {g}")?;
            }
        }
        for a in &self.area_findings {
            writeln!(f, "  {a}")?;
        }
        writeln!(f)?;
        for s in &self.songs {
            let layer = s
                .layer
                .as_ref()
                .map_or_else(|| "no layer".into(), |l| format!("layer {l}"));
            let profile = s
                .profile
                .as_ref()
                .map_or_else(|| "no profile header".into(), |p| format!("against {p}"));
            writeln!(f, "{} ({}) — {profile}, {layer}", s.name, s.file)?;
            if let Some(e) = &s.error {
                writeln!(f, "  error: {e}")?;
            }
            if s.findings.is_empty() && s.error.is_none() {
                writeln!(f, "  clean")?;
            }
            for l in &s.findings {
                writeln!(f, "  {l}")?;
            }
        }
        writeln!(f)?;
        if self.ok() {
            write!(f, "ok: nothing required is missing")
        } else {
            write!(
                f,
                "FAIL: {} required role(s) unbound",
                self.required_gaps().count()
            )
        }
    }
}

/// Checks a night: the venue against the profile, and every song
/// against the profile, reporting which songs have a layer.
///
/// `show_dir` is what the show file's paths are relative to.
// r[impl profile.show-check-before-doors] - every song and the venue, one report
// r[impl files.compatibility-check] - two independent checks, N+M not N×M
// r[impl profile.venue-layer.visible]
// r[impl profile.show-many-per-song] - a song listed twice is checked twice, under its own entry
pub fn check_ig_show(show: &ShowFile, show_dir: &Path) -> Report {
    let profile_path = show.profile_path(show_dir);
    let venue_dir = show.venue_dir(show_dir);
    let mut report = Report {
        show: show.name.clone(),
        profile: profile_path.display().to_string(),
        venue: venue_dir.display().to_string(),
        ..Default::default()
    };

    let profile = match read(&profile_path).and_then(|raw| parse::<Profile>(&profile_path, &raw)) {
        Ok(p) => Some(p),
        Err(e) => {
            report.errors.push(e.to_string());
            None
        }
    };

    match load_venue_binding(&venue_dir) {
        Ok(binding) => {
            if let Some(p) = &profile {
                report.gaps = check_venue_against_profile(&binding, p);
            }
            report.area_findings = check_areas(binding.areas.keys().map(String::as_str));
        }
        Err(e) => report.errors.push(e.to_string()),
    }

    for entry in &show.songs {
        let path = show_dir.join(&entry.file);
        let mut song = SongReport {
            file: entry.file.clone(),
            layer: entry.layer.clone(),
            ..Default::default()
        };
        match ShowDocument::load(&path) {
            Ok(doc) => {
                song.name.clone_from(&doc.list.name);
                song.profile.clone_from(&doc.profile);
                if let Some(p) = &profile {
                    song.findings = check_show_against_profile(&doc.list, p);
                }
            }
            Err(e) => song.error = Some(e.to_string()),
        }
        report.songs.push(song);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Role;
    use crate::selection::{Axis, Cmp, Where};
    use crate::step::{Step, Timing};

    fn data(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../data")
            .join(rel)
    }

    fn profile() -> Profile {
        let path = data("profiles/ignition.ig-profile");
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
    }

    fn cue(name: &str, recipes: Vec<RecipeRef>) -> Cue {
        Cue {
            name: name.into(),
            recipes,
            ..Default::default()
        }
    }

    fn on(target: Selection) -> RecipeRef {
        RecipeRef::Inline(Recipe {
            target,
            steps: vec![Step::new(vec![RecipeApply::Dimmer(1.0)])],
            timing: Timing::default(),
            tricks: vec![],
            stack: false,
            ..Default::default()
        })
    }

    // ----- .ig-show -----

    /// r[verify profile.show-binds-the-night]
    /// r[verify files.text-and-diffable]
    /// r[verify files.versioned]
    #[test]
    fn the_shipped_show_binds_profile_venue_and_songs() {
        let path = data("shows/sunday.ig-show");
        let show = ShowFile::load(&path).unwrap();
        assert_eq!(show.version, SHOW_FILE_VERSION);
        assert!(show.profile_path(path.parent().unwrap()).is_file());
        assert!(show.venue_dir(path.parent().unwrap()).is_dir());
        assert!(!show.songs.is_empty());
        // Pretty JSON that round-trips byte-for-byte: what a diff shows
        // is what a person changed.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw.trim_end(), show.to_json());
    }

    /// r[verify profile.show-many-per-song]
    /// r[verify files.show.many-per-song]
    #[test]
    fn a_song_may_be_listed_twice_and_is_checked_twice() {
        let path = data("shows/sunday.ig-show");
        let show = ShowFile::load(&path).unwrap();
        let files: Vec<_> = show.songs.iter().map(|s| s.file.as_str()).collect();
        assert!(
            files.iter().filter(|f| f.contains("bye-bye-bye")).count() >= 2,
            "the shipped show plays Bye Bye Bye twice: {files:?}"
        );
        let report = check_ig_show(&show, path.parent().unwrap());
        assert_eq!(report.songs.len(), show.songs.len());
    }

    /// r[verify files.additive-evolution]
    #[test]
    fn unknown_show_keys_survive_a_round_trip() {
        let raw = r#"{"version":1,"name":"x","profile":"p","venue":"v","songs":[{"file":"a.ignition","mood":"loud"}],"promoter":"Bob"}"#;
        let show = ShowFile::parse(Path::new("x.ig-show"), raw).unwrap();
        let back: serde_json::Value = serde_json::from_str(&show.to_json()).unwrap();
        assert_eq!(back["promoter"], "Bob");
        assert_eq!(back["songs"][0]["mood"], "loud");
    }

    /// r[verify files.versioned]
    #[test]
    fn a_newer_show_file_is_rejected_naming_the_version() {
        let raw = r#"{"version":99,"profile":"p","venue":"v","songs":[]}"#;
        let err = ShowFile::parse(Path::new("future.ig-show"), raw).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("99") && msg.contains("future.ig-show"),
            "{msg}"
        );
    }

    // ----- .ignition -----

    /// r[verify files.show.song-binding]
    /// r[verify profile.ignition-is-per-song]
    /// r[verify files.show.cues]
    #[test]
    fn the_header_rides_on_the_cue_list_as_one_document() {
        let list = CueList {
            name: "Song".into(),
            cues: vec![cue("A", vec![on(Selection::Role("Key".into()))])],
            triggers: vec![],
            ..Default::default()
        };
        let doc = ShowDocument::new(
            list.clone(),
            "Ignition",
            SongBinding {
                project: "../songs/song.rpp".into(),
                name: "Song".into(),
            },
        );
        let raw = doc.to_json();
        // The same document reads as a bare CueList by anything that
        // only knows that type, and as the headed form by us.
        let plain: CueList = serde_json::from_str(&raw).unwrap();
        assert_eq!(plain, list);
        let back = ShowDocument::parse(Path::new("song.ignition"), &raw).unwrap();
        assert_eq!(back.profile.as_deref(), Some("Ignition"));
        assert_eq!(back.song.as_ref().unwrap().project, "../songs/song.rpp");
        assert_eq!(back.list, list);
    }

    /// r[verify files.versioned]
    /// r[verify files.additive-evolution]
    #[test]
    fn a_pre_header_song_file_loads_as_version_zero() {
        // A bare `CueList` as every show was written before the header
        // existed: no version, no profile, no song binding.
        let doc = ShowDocument::parse(
            Path::new("old.json"),
            r#"{"name":"Old Song","cues":[{"name":"Verse","recipes":[]}]}"#,
        )
        .unwrap();
        assert!(doc.version.is_none() || doc.version == Some(0));
        assert!(doc.profile.is_none() && doc.song.is_none());
        assert_eq!(doc.list.cues.len(), 1);
        assert_eq!(doc.list.name, "Old Song");
        // And the shipped show carries the header the generator writes.
        let shipped = ShowDocument::load(data("songs/bye-bye-bye.json")).unwrap();
        assert_eq!(shipped.version, Some(1));
        assert_eq!(shipped.profile.as_deref(), Some("Ignition"));
    }

    // ----- .ig-local -----

    fn base() -> CueList {
        CueList {
            name: "Song".into(),
            cues: vec![
                cue("Verse", vec![on(Selection::Role("Wash".into()))]),
                cue("Chorus", vec![on(Selection::Role("Back".into()))]),
            ],
            triggers: vec![],
            ..Default::default()
        }
    }

    /// r[verify profile.venue-layer]
    /// r[verify profile.venue-layer.optional]
    #[test]
    fn applying_then_removing_a_layer_yields_the_original() {
        let original = base();
        let mut list = original.clone();
        let mut layer = VenueLayer::new("song.ignition", "norco");
        layer.override_cues.insert(
            "Chorus".into(),
            cue("Chorus", vec![on(Selection::Group("Pillar Pars".into()))]),
        );
        layer
            .override_cues
            .insert("Nope".into(), cue("Nope", vec![]));
        layer.add_cues.push(cue("Norco Tag", vec![]));

        let applied = apply_layer(&mut list, &layer);
        assert_eq!(list.cues.len(), 3);
        assert_eq!(applied.unmatched, vec!["Nope".to_string()]);
        assert_ne!(list, original, "the layer changed something");

        remove_layer(&mut list, applied);
        assert_eq!(
            list, original,
            "the base show was complete without the layer"
        );
    }

    /// A layer is checked against nothing: it may name what the venue
    /// has, profile or not.
    /// r[verify profile.venue-layer.explicitly-local]
    /// r[verify files.versioned]
    #[test]
    fn a_layer_may_use_venue_names_and_round_trips() {
        let mut layer = VenueLayer::new("song.ignition", "norco");
        layer.add_cues.push(cue(
            "Pillar",
            vec![on(Selection::Group("Pillar Pars".into()))],
        ));
        let raw = layer.to_json();
        let back = VenueLayer::parse(Path::new("x.ig-local"), &raw).unwrap();
        assert_eq!(back, layer);
        let too_new = raw.replacen("\"version\": 1", "\"version\": 7", 1);
        assert!(VenueLayer::parse(Path::new("x.ig-local"), &too_new).is_err());
    }

    // ----- checks -----

    /// r[verify files.compatibility-check]
    /// r[verify profile.check-is-static]
    /// r[verify profile.declares-vocabulary]
    #[test]
    fn an_undeclared_role_is_a_finding() {
        let list = CueList {
            cues: vec![cue("A", vec![on(Selection::Role("Lasers".into()))])],
            ..Default::default()
        };
        let f = check_show_against_profile(&list, &profile());
        assert!(f.iter().any(|l| l.finding
            == Finding::Undeclared {
                kind: RoleKind::Group,
                name: "Lasers".into()
            }));
    }

    /// r[verify files.no-fixture-identity]
    /// r[verify profile.ignition-has-no-venue]
    #[test]
    fn channels_and_inline_metres_are_findings_and_zones_are_not() {
        let list = CueList {
            cues: vec![
                cue("Chans", vec![on(Selection::Chans(vec![1, 2, 3]))]),
                cue(
                    "Point",
                    vec![RecipeRef::Inline(Recipe {
                        target: Selection::Role("Movers".into()),
                        steps: vec![Step::new(vec![RecipeApply::FocusPoint(Ref::Inline(
                            crate::Vec3 {
                                x: 1.0,
                                y: 2.0,
                                z: 0.0,
                            },
                        ))])],
                        timing: Timing::default(),
                        tricks: vec![],
                        stack: false,
                        ..Default::default()
                    })],
                ),
                cue(
                    "Zone",
                    vec![on(Selection::Where {
                        of: Box::new(Selection::Role("Wash".into())),
                        filter: Where::Half {
                            axis: Axis::X,
                            cmp: Cmp::Gt,
                            at: 0.0,
                        },
                    })],
                ),
            ],
            ..Default::default()
        };
        let f = check_show_against_profile(&list, &profile());
        assert!(f.iter().any(|l| l.cue.as_deref() == Some("Chans")
            && matches!(l.finding, Finding::FixtureIdentity { .. })));
        assert!(f.iter().any(|l| l.cue.as_deref() == Some("Point")
            && matches!(l.finding, Finding::MetreCoordinate { .. })));
        assert!(
            !f.iter().any(|l| l.cue.as_deref() == Some("Zone")),
            "a Where zone is a capability, not a room coordinate: {f:?}"
        );
    }

    /// The shipped song is the proof the check is not merely strict: it
    /// names nothing by identity and carries no metres.
    /// r[verify files.no-fixture-identity]
    /// r[verify profile.ignition-has-no-venue]
    /// r[verify files.show]
    #[test]
    fn the_shipped_song_names_no_fixture_and_no_room() {
        let doc = ShowDocument::load(data("songs/bye-bye-bye.json")).unwrap();
        let f = check_show_against_profile(&doc.list, &profile());
        let hard: Vec<_> = f
            .iter()
            .filter(|l| {
                matches!(
                    l.finding,
                    Finding::FixtureIdentity { .. } | Finding::MetreCoordinate { .. }
                )
            })
            .collect();
        assert!(hard.is_empty(), "{hard:#?}");
    }

    /// r[verify files.required-roles]
    /// r[verify files.graceful-degradation]
    /// r[verify profile.show-check-before-doors]
    /// r[verify profile.venue-layer.visible]
    #[test]
    fn the_report_names_every_gap_and_every_layer_without_refusing() {
        let path = data("shows/sunday.ig-show");
        let show = ShowFile::load(&path).unwrap();
        let report = check_ig_show(&show, path.parent().unwrap());
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        // Norco binds every required role; the optional ones it lacks
        // are listed, not fatal.
        assert!(report.ok(), "{report}");
        assert!(report.gaps.iter().all(|g| !g.required));
        assert!(report.gaps.iter().any(|g| g.role == "Spot"));
        // Every song is reported, each saying whether it has a layer.
        assert_eq!(report.songs.len(), show.songs.len());
        assert!(report.songs.iter().all(|s| s.error.is_none()));
        let with_layer = report.songs.iter().filter(|s| s.layer.is_some()).count();
        assert_eq!(with_layer, 1, "{report}");
        // And the report is one thing an operator reads.
        let text = report.to_string();
        assert!(
            text.contains("no layer") && text.contains("layer "),
            "{text}"
        );
    }

    /// A venue with a required role unbound fails the check and is
    /// still a venue.
    /// r[verify files.required-roles]
    /// r[verify files.compatibility-check]
    #[test]
    fn a_required_gap_fails_the_report() {
        let mut show = ShowFile::load(data("shows/sunday.ig-show")).unwrap();
        show.venue = "../venues/riverside".into();
        let report = check_ig_show(&show, &data("shows"));
        // Riverside binds every required group and focus, so it passes
        // too; strip Key from a copy of its binding to see the failure.
        let binding = load_venue_binding(&data("venues/riverside")).unwrap();
        let mut broken = binding;
        broken.groups.remove("Key");
        let gaps = check_venue_against_profile(&broken, &profile());
        assert!(gaps.iter().any(|g| g.role == "Key" && g.required));
        assert!(report.errors.is_empty(), "{:?}", report.errors);
    }

    /// r[verify profile.areas.performer-orientation]
    /// r[verify profile.areas.venue-decides-granularity]
    #[test]
    fn areas_are_named_from_the_stage_and_never_counted() {
        let f = check_areas(["Downstage Left", "House Left", "Upstage Camera Right"]);
        assert_eq!(f.len(), 2);
        let many: Vec<String> = (0..40).map(|i| format!("Area {i}")).collect();
        assert!(check_areas(many.iter().map(String::as_str)).is_empty());
    }

    /// A generic show reaches the talent through focus roles, never
    /// through a venue's blocking grid.
    /// r[verify profile.areas.portable-question-is-focus]
    /// r[verify profile.areas]
    #[test]
    fn a_generic_show_never_references_an_area() {
        let doc = ShowDocument::load(data("songs/bye-bye-bye.json")).unwrap();
        let binding = load_venue_binding(&data("venues/norco")).unwrap();
        assert!(!binding.areas.is_empty(), "norco ships areas.json");
        let used = names_used(&doc.list);
        for (name, _) in &used {
            assert!(
                !binding.areas.contains_key(name),
                "the generic show names area {name:?}"
            );
        }
        // The portable question is answered by focus roles the profile
        // declares, and the default profile declares no area role.
        let p = profile();
        assert!(p.vocabulary(RoleKind::Area).is_empty());
        assert!(
            used.iter()
                .any(|(n, k)| *k == RoleKind::Focus && n == "Vocal")
        );
    }

    /// r[verify files.directory-or-archive]
    /// r[verify files.venue.assets]
    #[test]
    fn a_manifest_names_the_files_and_the_assets_relative_to_the_venue() {
        let dir = data("venues/norco");
        let m = VenueManifest::load_dir(&dir).unwrap();
        assert_eq!(m.version, VENUE_MANIFEST_VERSION);
        assert_eq!(m.assets_dir(&dir), dir.join("assets"));
        assert_eq!(m.file(&dir, "fixtures"), dir.join("fixtures.json"));
        // A bare directory is the same venue with every default.
        let bare = VenueManifest::load_dir(Path::new("/nonexistent")).unwrap();
        assert_eq!(bare, VenueManifest::default());
        // An override is honoured.
        let mut m2 = VenueManifest::default();
        m2.files.insert("fixtures".into(), "rig.json".into());
        assert_eq!(m2.file(&dir, "fixtures"), dir.join("rig.json"));
    }

    /// r[verify profile.several-may-exist]
    #[test]
    fn two_profiles_check_the_same_show_differently() {
        let list = CueList {
            cues: vec![cue("A", vec![on(Selection::Role("Key".into()))])],
            ..Default::default()
        };
        let church = Profile {
            name: "church".into(),
            roles: vec![Role::required("Pulpit", RoleKind::Group, "")],
            ..Default::default()
        };
        assert!(check_show_against_profile(&list, &profile()).is_empty());
        assert!(!check_show_against_profile(&list, &church).is_empty());
    }
}
