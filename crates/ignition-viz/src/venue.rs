//! Loads the extracted venue JSON (`data/venues/<name>/*.json`) — see
//! `docs/domain/norco-venue-reference.md` for what each file contains.

// The maths types come from Bevy's re-export rather than a direct `glam`
// dependency, so the tree only ever has one `Vec3` — see the `bevy` entry
// in the root Cargo.toml. These local `Vec3`/`Quat` structs stay, because
// they are the JSON's field layout (`{x, y, z}`, `{w, x, y, z}`) and
// Deserialize impls, not a maths type.
use bevy::math::{EulerRot, Quat as Rotation, Vec3 as Point};
use ignition_core::show_file::VenueManifest;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub fn to_vec3(self) -> Point {
        Point::new(self.x, self.y, self.z)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Quat {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Quat {
    pub fn to_quat(self) -> Rotation {
        Rotation::from_xyzw(self.x, self.y, self.z, self.w).normalize()
    }
}

fn euler_to_quat(e: Vec3) -> Rotation {
    Rotation::from_euler(
        EulerRot::ZYX,
        e.z.to_radians(),
        e.y.to_radians(),
        e.x.to_radians(),
    )
}

#[derive(Debug, Clone, Deserialize)]
// r[impl files.venue.fixtures] - type, position, orientation, patch and tags per fixture
pub struct FixtureRecord {
    pub chan: Option<u32>,
    pub name: String,
    pub tags: Vec<String>,
    #[serde(default = "default_patched")]
    pub patched: bool,
    #[serde(default)]
    pub manufacturer: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub position: Vec3,
    pub eulers: Vec3,
    pub quat: Quat,
    pub size: Vec3,
    /// Which sACN/Art-Net universe this fixture is patched to — same field
    /// the live Eos pull already writes (`docs/domain/norco-patch-and-
    /// groups.md`). `None` for a fixture with no live-patch data.
    #[serde(default)]
    pub universe: Option<u16>,
    /// 1-based start DMX address within `universe`. Eos's live pull calls
    /// this `address`; `global_address` (also present in the extracted
    /// data) is the same value expressed net-wide rather than per-universe
    /// and isn't needed here.
    #[serde(default)]
    pub address: Option<u16>,
    /// The real fixture's beam spread, in degrees — sizes the live beam
    /// cone's radius in `live` mode (`scene.rs`). `None` (no field in the
    /// source data, or a non-beam fixture like a bar) falls back to a
    /// generic default.
    #[serde(default)]
    pub beam_angle_deg: Option<f32>,
    /// Further DMX addresses this one fixture also drives — four house
    /// pars on one address are one fixture in the show and four on the
    /// wire. Written with exactly the bytes the primary address gets.
    // r[impl files.venue.multipatch] - the extra addresses of a multipatched fixture
    #[serde(default)]
    pub mirrors: Vec<ignition_proto::DmxAddress>,
}

fn default_patched() -> bool {
    true
}

impl FixtureRecord {
    /// The mounting orientation — the *hang*, never the aim. See
    /// `docs/domain/norco-venue-reference.md`.
    pub fn orientation(&self) -> Rotation {
        self.quat.to_quat()
    }

    /// This fixture's live DMX address, if the venue data has both pieces —
    /// `None` for anything not DMX-controlled (or missing patch data).
    pub fn dmx_address(&self) -> Option<ignition_proto::DmxAddress> {
        Some(ignition_proto::DmxAddress {
            universe: self.universe?,
            start_channel: self.address?,
        })
    }

    /// This fixture's real hung position and mount orientation, in
    /// `ignition_core`'s f64 `Placement` shape — what
    /// `ignition_core::focus` solves Pan/Tilt against.
    ///
    /// The **raw** mount quaternion, deliberately. This used to compose
    /// `fixture_profile::beam_pre_rotate` in, to compensate for the
    /// renderer aiming beams along `rot * pan * tilt * pre_rotate` — a
    /// mesh-authoring correction that had leaked onto the aim joint. That
    /// was a compensating error for a real bug, and not even an exact
    /// one: it applied the correction *before* pan where the renderer
    /// applied it *after* tilt, and a 180-degree flip does not commute
    /// with pan. Focus solves came out right only when pan happened to be
    /// near zero.
    ///
    /// The renderer now aims along `mount * pan * tilt * -Z`, which is
    /// what `focus.rs` always documented itself as targeting, so there is
    /// nothing left to compensate for. `spawn.rs`'s
    /// `aim_convention_tests` pins the two together.
    ///
    /// A plain unit cast for both fields (this crate's own `Vec3`/`Quat`
    /// are `f32`, matching Bevy's maths types; `ignition_proto`'s are
    /// `f64`, matching the rest of the wire-contract types) — no
    /// coordinate remap, same convention both sides already agree on
    /// (`to_vec3()`/`to_quat()`'s own doc comments).
    pub fn placement(&self) -> ignition_proto::Placement {
        let effective_rot = self.orientation();
        ignition_proto::Placement {
            position: ignition_proto::Vec3 {
                x: self.position.x as f64,
                y: self.position.y as f64,
                z: self.position.z as f64,
            },
            orientation: ignition_proto::Quat {
                w: effective_rot.w as f64,
                x: effective_rot.x as f64,
                y: effective_rot.y as f64,
                z: effective_rot.z as f64,
            },
        }
    }

    pub fn kind(&self) -> FixtureKind {
        if self
            .tags
            .iter()
            .any(|t| t.contains("Yoke") || t.contains("Mover"))
        {
            FixtureKind::Mover
        } else if self.tags.iter().any(|t| t.contains("Wash")) {
            FixtureKind::Wash
        } else {
            FixtureKind::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureKind {
    Wash,
    Mover,
    Other,
}

impl FixtureKind {
    pub fn color(self) -> [f32; 3] {
        match self {
            FixtureKind::Wash => [0.25, 0.75, 0.95],
            FixtureKind::Mover => [0.95, 0.55, 0.15],
            FixtureKind::Other => [0.65, 0.65, 0.70],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
// r[impl files.venue.room]
// r[impl files.venue.screens] - position, size, orientation and canvas per panel
pub struct GeometryRecord {
    pub name: String,
    pub position: Vec3,
    pub eulers: Vec3,
    pub size: Vec3,
    /// Which canvas a screen shows a piece of.
    ///
    /// Screens sharing a canvas each show the slice matching where they
    /// physically are, so one image spans several panels. `None` means
    /// the screen is its own canvas and shows the whole source — which
    /// is what every screen did before this existed. Ignored for
    /// anything that is not a screen.
    #[serde(default)]
    pub canvas: Option<String>,
}

impl GeometryRecord {
    pub fn orientation(&self) -> Rotation {
        euler_to_quat(self.eulers)
    }

    /// The canvas this screen belongs to — its own name when it is not
    /// part of a group.
    // r[impl files.venue.canvases]
    pub fn canvas_name(&self) -> &str {
        self.canvas.as_deref().unwrap_or(&self.name)
    }
}

/// One `channels` array entry from Eos's export — most groups use its
/// range-string shorthand (`"1-48"`, or a bare `"50"` for one channel),
/// but some (e.g. Norco's "Pars Odd") are exported as a plain JSON array
/// of individual channel numbers instead — Eos apparently picks whichever
/// is more compact for a given selection. `untagged` accepts either shape
/// per-element rather than requiring the whole array to agree.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ChannelListEntry {
    Range(String),
    Chan(u32),
}

/// One entry from Eos's own exported `groups.json` — the live rig's real
/// group list (112 of them for Norco: "Pars", "Movers", "OH Movers", ...
/// see `docs/domain/norco-patch-and-groups.md`). `Venue::groups()` expands
/// `channels` into the plain `ChanId` lists `ignition_core::recipe`'s
/// `RecipeTarget::Group` actually needs.
#[derive(Debug, Clone, Deserialize)]
pub struct GroupRecord {
    #[allow(dead_code)] // Eos's own numeric group ID — kept for round-tripping, not read yet.
    pub target: String,
    pub label: String,
    pub channels: Vec<ChannelListEntry>,
}

fn parse_channel_ranges(entries: &[ChannelListEntry]) -> Vec<u32> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            ChannelListEntry::Chan(v) => out.push(*v),
            ChannelListEntry::Range(r) => {
                if let Some((a, b)) = r.split_once('-') {
                    if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                        out.extend(a..=b);
                    }
                } else if let Ok(v) = r.trim().parse::<u32>() {
                    out.push(v);
                }
            }
        }
    }
    out
}

#[derive(Clone)]
// r[impl files.venue] - fixtures, room, screens, props, groups, palettes and profile binding
// r[impl files.vocabulary] - groups, palettes and canvases are the venue's vocabulary
// r[impl profile.venue-declares-what-it-implements] - `profile` names the profile, and may be empty
pub struct Venue {
    pub fixtures: Vec<FixtureRecord>,
    pub room: Vec<GeometryRecord>,
    pub screens: Vec<GeometryRecord>,
    pub props: Vec<GeometryRecord>,
    /// Raw Eos group records — empty for a venue with no `groups.json`
    /// (not every extracted venue will have one). Use `groups()` for the
    /// resolved `ignition_core::Group` form recipes actually target.
    pub group_records: Vec<GroupRecord>,
    /// The venue's colour and focus palettes — empty for a venue with no
    /// `palettes.json`. Palettes belong to the room rather than to a
    /// show, so every show loaded against this venue means the same thing
    /// by "House Blue".
    pub palettes: ignition_core::Palettes,
    /// Which profile this venue implements, and how — empty for a venue
    /// with no `profile.json`.
    ///
    /// A venue without one still works: it can be programmed directly
    /// against its own group names. What it gives up is being *checked*,
    /// and being able to run a show written against roles.
    pub profile: ignition_core::profile::VenueProfile,
    /// Every fixture's address and channel layout, resolved once on
    /// first use — see `PatchTable`.
    pub patch: std::sync::OnceLock<PatchTable>,
}

impl Venue {
    /// Where every fixture sits on the wire, built once.
    ///
    /// Resolving a fixture's channel map means lowercasing its
    /// manufacturer and model and walking a match; doing that per
    /// fixture per attribute per frame was a measurable slice of the
    /// studio's frame, for an answer that cannot change after load.
    pub fn patch(&self) -> &PatchTable {
        self.patch.get_or_init(|| PatchTable::build(self))
    }
}

/// One patched fixture: its start address and its personality's layout.
#[derive(Debug, Clone)]
pub struct Patch {
    /// The primary address — what the read side (`dmx.rs::resolve`)
    /// decodes the fixture from.
    pub address: ignition_proto::DmxAddress,
    /// Every further address the fixture is multipatched to.
    // r[impl files.venue.multipatch]
    pub mirrors: Vec<ignition_proto::DmxAddress>,
    pub map: ignition_proto::ChannelMap,
    /// The output-side profile: emitters, wheel, preference, space, defaults.
    pub profile: crate::fixture_profile::FixtureProfile,
}

impl Patch {
    /// The primary address followed by every mirror — where a frame's
    /// bytes for this fixture go.
    // r[impl files.venue.multipatch] - the patch expands to all addresses
    pub fn addresses(&self) -> impl Iterator<Item = &ignition_proto::DmxAddress> {
        std::iter::once(&self.address).chain(self.mirrors.iter())
    }
}

/// The venue's patch, indexed the two ways the visualizer needs it: by
/// position in `Venue::fixtures` (what an entity carries) and by
/// console channel (what a cue targets).
#[derive(Debug, Clone, Default)]
pub struct PatchTable {
    entries: Vec<Option<Patch>>,
    by_chan: std::collections::HashMap<u32, usize>,
}

impl PatchTable {
    fn build(venue: &Venue) -> Self {
        let entries = venue
            .fixtures
            .iter()
            .map(|f| {
                let manufacturer = f.manufacturer.as_deref().unwrap_or("");
                let model = f.model.as_deref().unwrap_or("");
                let address = f.dmx_address()?;
                let profile = crate::channel_map::profile_for(manufacturer, model)?;
                Some(Patch {
                    address,
                    mirrors: f.mirrors.clone(),
                    map: profile.map.clone(),
                    profile,
                })
            })
            .collect();
        let by_chan = venue
            .fixtures
            .iter()
            .enumerate()
            .filter_map(|(i, f)| f.chan.map(|c| (c, i)))
            .collect();
        Self { entries, by_chan }
    }

    /// The patch for the fixture at `index` in `Venue::fixtures`, if it
    /// has an address and a known layout.
    pub fn get(&self, index: usize) -> Option<&Patch> {
        self.entries.get(index).and_then(|p| p.as_ref())
    }

    /// The patch for console channel `chan`.
    pub fn by_chan(&self, chan: u32) -> Option<&Patch> {
        self.by_chan.get(&chan).and_then(|i| self.get(*i))
    }

    /// Every patched `(chan, patch)`.
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Patch)> {
        self.by_chan
            .iter()
            .filter_map(|(chan, i)| Some((*chan, self.get(*i)?)))
    }

    /// The rest value of every attribute of every patched fixture — the
    /// floor a released attribute falls to and what cue zero establishes.
    // r[impl playback.defaults] - the defaults map for the patched rig
    pub fn defaults(&self) -> std::collections::HashMap<(u32, ignition_proto::Attribute), f32> {
        self.iter()
            .flat_map(|(chan, patch)| {
                patch
                    .profile
                    .defaults
                    .iter()
                    .map(move |(attr, value)| ((chan, attr.clone()), *value))
            })
            .collect()
    }
}

/// Fills in any colour the venue did not define itself from its profile.
///
/// The venue always wins. A room whose fixtures render `Deep Blue`
/// differently says so once, and every show using that name is right
/// there without knowing anything happened.
// r[impl default.colour-defaults-ship] - profile colour defaults inherited unless the venue overrides
fn inherit_colors(
    mut palettes: ignition_core::Palettes,
    venue_dir: &Path,
    binding: &ignition_core::profile::VenueProfile,
) -> ignition_core::Palettes {
    if binding.profile.is_empty() {
        return palettes;
    }
    let path = venue_dir.parent().and_then(|p| p.parent()).map(|root| {
        root.join("profiles")
            .join(format!("{}.ig-profile", binding.profile.to_lowercase()))
    });
    let Some(path) = path else {
        return palettes;
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        // A missing profile is not fatal: the venue still has whatever
        // colours it defined, and the compatibility check is the thing
        // that reports the profile being absent.
        tracing::debug!(path = %path.display(), "venue: no profile file to inherit colours from");
        return palettes;
    };
    let Ok(profile) = serde_json::from_str::<ignition_core::profile::Profile>(&raw) else {
        tracing::warn!(path = %path.display(), "venue: profile did not parse; colours not inherited");
        return palettes;
    };

    let mut inherited = 0usize;
    for color in profile.colors.iter().chain(binding.colors.iter()) {
        if !palettes.colors.iter().any(|c| c.name == color.name) {
            palettes.colors.push(color.clone());
            inherited += 1;
        }
    }
    tracing::debug!(inherited, "venue: colours inherited from profile");

    // Splits inherit by the same rule. They are read from the profile
    // file's own `splits` key rather than through `Profile`, which does
    // not carry them yet: a split is palette vocabulary, and this is the
    // one place the profile is opened as a palette.
    // r[impl color.multi] - profile split defaults inherited unless the venue overrides
    #[derive(serde::Deserialize, Default)]
    struct ProfileSplits {
        #[serde(default)]
        splits: Vec<ignition_core::preset::ColorSplit>,
    }
    let profile_splits = serde_json::from_str::<ProfileSplits>(&raw).unwrap_or_default();
    let mut inherited = 0usize;
    for split in profile_splits.splits {
        if !palettes.splits.iter().any(|s| s.name == split.name) {
            palettes.splits.push(split);
            inherited += 1;
        }
    }
    tracing::debug!(inherited, "venue: colour splits inherited from profile");
    alias_focus(palettes, binding)
}

/// Publishes each bound focus role under the profile's name for it.
///
/// The same move as inheriting colours, in the other direction: rather
/// than adding what the venue lacks, this adds a second *name* for what
/// it already has. Norco's lead position is `Vocal Centre`; the profile
/// calls that job `Vocal`; after this the palette answers to both, and a
/// show written against roles resolves with no special case anywhere in
/// the lookup path.
///
/// An alias never overwrites. A venue that genuinely has its own point
/// called `Stage` keeps it, because the venue is the authority on its
/// own room and a profile role is a request, not a claim.
// r[impl profile.venue-binds] - focus roles bound to the venue's own points
fn alias_focus(
    mut palettes: ignition_core::Palettes,
    binding: &ignition_core::profile::VenueProfile,
) -> ignition_core::Palettes {
    let mut added = 0usize;
    for (role, own_name) in &binding.focus {
        if palettes.focus.iter().any(|f| &f.name == role) {
            continue;
        }
        let Some(target) = palettes
            .focus
            .iter()
            .find(|f| &f.name == own_name)
            .map(|f| f.target)
        else {
            // A binding naming a point the venue does not have. Not
            // fatal — the role simply stays unresolved, and the static
            // check is where that gets reported by name.
            tracing::warn!(
                role,
                own_name,
                "venue: focus binding points at a missing palette entry"
            );
            continue;
        };
        palettes.focus.push(ignition_core::FocusPointPreset {
            name: role.clone(),
            target,
        });
        added += 1;
    }
    tracing::debug!(added, "venue: focus roles aliased onto the palette");
    palettes
}

impl Venue {
    // r[impl files.venue] - the seven JSON files under data/venues/<name>/
    // r[impl profile.venue-declares-what-it-implements] - profile.json is optional
    // r[impl profile.venue-binds] - the binding loads with the venue, not with any show
    pub fn load(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let dir = dir.as_ref();
        // A directory with a `venue.ig-venue` manifest, or a bare
        // directory of the seven JSON files — the manifest only names
        // them and the assets folder, so a bare directory is the same
        // venue with every default. The directory stays the editable
        // form either way; an archive is packaging, not format.
        // r[impl files.directory-or-archive] - the manifest is optional and the directory is the format
        // r[impl files.versioned] - a manifest from a newer build is refused by name
        let manifest = VenueManifest::load_dir(dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        let file = |key: &str| manifest.file(dir, key);
        let read = |key: &str| -> anyhow::Result<String> {
            std::fs::read_to_string(file(key))
                .map_err(|e| anyhow::anyhow!("reading {}: {e}", file(key).display()))
        };
        // groups.json is optional — a venue extract without one (or an
        // older extract, predating this field) just has no named groups
        // to recipe-target by name; `RecipeTarget::Chans` still works.
        let group_records = match std::fs::read_to_string(file("groups")) {
            Ok(raw) => serde_json::from_str(&raw)?,
            Err(_) => Vec::new(),
        };
        // palettes.json is optional for the same reason groups.json is —
        // a show can always write its colours and points out inline.
        let palettes = match std::fs::read_to_string(file("palettes")) {
            Ok(raw) => serde_json::from_str(&raw)?,
            Err(_) => ignition_core::Palettes::default(),
        };
        // profile.json is optional like the rest. A venue that has not
        // been bound to a profile yet is a venue somebody is still
        // setting up, not an error. `areas.json` — the blocking grid —
        // is folded in beside it; both are the venue's own vocabulary.
        let profile = ignition_core::show_file::load_venue_binding(dir)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        // Colours declared by the profile are inherited unless the venue
        // overrides them, which is what keeps implementing a room cheap:
        // bind the groups, which genuinely differ per rig, and inherit
        // the colours, which mostly do not. Found by convention next to
        // the venue rather than configured — a venue names its profile
        // and the profile lives in `profiles/` beside `venues/`, so
        // there is nothing to keep in sync.
        let palettes = inherit_colors(palettes, dir, &profile);
        Ok(Self {
            palettes,
            profile,
            patch: Default::default(),
            fixtures: serde_json::from_str(&read("fixtures")?)?,
            room: serde_json::from_str(&read("room")?)?,
            screens: serde_json::from_str(&read("screens")?)?,
            props: serde_json::from_str(&read("props")?)?,
            group_records,
        })
    }

    /// Where a venue directory's assets live — room models, fixture
    /// geometry — `assets` under the venue unless its manifest says
    /// otherwise, and always resolved against the venue directory so
    /// the directory moves and copies as a unit. Read from the manifest
    /// rather than stored, so a venue built in memory has no path to
    /// carry.
    // r[impl files.venue.assets] - resolved relative to the venue, never absolute
    pub fn assets_dir(dir: impl AsRef<Path>) -> std::path::PathBuf {
        let dir = dir.as_ref();
        VenueManifest::load_dir(dir)
            .unwrap_or_default()
            .assets_dir(dir)
    }

    /// The venue's real groups, resolved into `ignition_core::Group`'s
    /// plain `(name, chans)` shape — what `ignition_core::recipe`'s
    /// `RecipeTarget::Group` actually resolves against.
    // r[impl files.vocabulary] - named groups
    pub fn groups(&self) -> Vec<ignition_core::Group> {
        self.group_records
            .iter()
            .map(|g| ignition_core::Group {
                name: g.label.clone(),
                chans: parse_channel_ranges(&g.channels),
            })
            .collect()
    }

    /// The patched rig in the flat shape `ignition_core::selection`
    /// resolves against — position, model and tags per channel, which is
    /// what makes `Selection::Tag`/`Model` and the spatial filters and
    /// orders answerable. Built once by the loader, not per frame.
    // r[impl files.venue.fixtures] - position, model and tags per channel, for spatial and capability selection
    pub fn rig(&self) -> ignition_core::Rig {
        ignition_core::Rig::new(
            self.fixtures
                .iter()
                .filter_map(|f| {
                    Some(ignition_core::FixtureInfo {
                        chan: f.chan?,
                        placement: Some(f.placement()),
                        manufacturer: f.manufacturer.clone().unwrap_or_default(),
                        model: f.model.clone().unwrap_or_default(),
                        tags: f.tags.clone(),
                    })
                })
                .collect(),
        )
    }

    /// A patched channel's real `Placement`, for `ignition_core::recipe`'s
    /// Focus Point expansion — `None` for a channel with no matching
    /// fixture (an unpatched or out-of-range `chan`).
    pub fn placement_of(&self, chan: u32) -> Option<ignition_proto::Placement> {
        self.fixtures
            .iter()
            .find(|f| f.chan == Some(chan))
            .map(|f| f.placement())
    }

    /// Axis-aligned bounds over every object's centre — used to auto-frame
    /// the default camera regardless of which venue is loaded.
    pub fn bounds(&self) -> (Point, Point) {
        let mut min = Point::splat(f32::INFINITY);
        let mut max = Point::splat(f32::NEG_INFINITY);
        let mut visit = |p: Point| {
            min = min.min(p);
            max = max.max(p);
        };
        for f in &self.fixtures {
            visit(f.position.to_vec3());
        }
        // Side annexes (sound booth, storage closet, alcove) sit far from
        // the main room — at Norco, ~10m past the audience back wall —
        // and including them badly distorts every auto-framed camera
        // (frame_house_view etc.), which assumes `bounds()` describes the
        // performance space. Excluded from framing; still rendered
        // normally, just not used to size/aim the default cameras.
        let is_annex = |name: &str| {
            name.contains("Alcove") || name.contains("Closet") || name.contains("Booth")
        };
        for g in self
            .room
            .iter()
            .filter(|g| !is_annex(&g.name))
            .chain(&self.screens)
            .chain(&self.props)
        {
            visit(g.position.to_vec3());
        }
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_eos_range_strings_and_bare_numbers_into_channel_lists() {
        // Real shapes straight from Norco's groups.json: range strings
        // ("1-3", "80-82") and a bare single-channel string ("50") — plus
        // the plain-integer shape Eos uses for some groups (e.g. "Pars
        // Odd"), all in the one array `ChannelListEntry::untagged` accepts.
        let entries = vec![
            ChannelListEntry::Range("1-3".to_string()),
            ChannelListEntry::Range("50".to_string()),
            ChannelListEntry::Chan(7),
            ChannelListEntry::Range("80-82".to_string()),
        ];
        assert_eq!(
            parse_channel_ranges(&entries),
            vec![1, 2, 3, 50, 7, 80, 81, 82]
        );
    }

    #[test]
    fn channel_list_entry_deserializes_both_real_json_shapes() {
        let ranges: Vec<ChannelListEntry> = serde_json::from_str(r#"["1-48", "50-53"]"#).unwrap();
        assert!(matches!(ranges[0], ChannelListEntry::Range(_)));
        let nums: Vec<ChannelListEntry> = serde_json::from_str(r#"[1, 3, 5, 7]"#).unwrap();
        assert!(matches!(nums[0], ChannelListEntry::Chan(1)));
    }

    #[test]
    fn groups_resolves_eos_group_records_into_ignition_core_groups() {
        let venue = Venue {
            fixtures: vec![],
            room: vec![],
            screens: vec![],
            props: vec![],
            palettes: Default::default(),
            profile: Default::default(),
            patch: Default::default(),
            group_records: vec![GroupRecord {
                target: "1".to_string(),
                label: "Pars".to_string(),
                channels: vec![ChannelListEntry::Range("1-3".to_string())],
            }],
        };
        let groups = venue.groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Pars");
        assert_eq!(groups[0].chans, vec![1, 2, 3]);
    }

    #[test]
    fn placement_of_finds_the_real_fixture_and_converts_units() {
        let record: FixtureRecord = serde_json::from_value(serde_json::json!({
            "chan": 7,
            "name": "Test",
            "tags": [],
            "position": {"x": 1.5, "y": -2.0, "z": 3.25},
            "eulers": {"x": 0.0, "y": 0.0, "z": 0.0},
            "quat": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0},
            "size": {"x": 0.2, "y": 0.2, "z": 0.2},
        }))
        .expect("valid fixture record");
        let venue = Venue {
            fixtures: vec![record],
            room: vec![],
            screens: vec![],
            props: vec![],
            palettes: Default::default(),
            profile: Default::default(),
            patch: Default::default(),
            group_records: vec![],
        };

        let placement = venue.placement_of(7).expect("fixture on channel 7 exists");
        assert!((placement.position.x - 1.5).abs() < 0.001);
        assert!((placement.position.y - (-2.0)).abs() < 0.001);
        assert!((placement.position.z - 3.25).abs() < 0.001);
        assert!(
            venue.placement_of(999).is_none(),
            "no fixture on channel 999"
        );
    }

    /// r[verify files.venue.multipatch] - a record's mirrors reach the patch table
    /// r[verify playback.defaults] - the rig's defaults come from each fixture's profile
    #[test]
    fn mirrors_and_defaults_come_through_the_patch_table() {
        let record: FixtureRecord = serde_json::from_value(serde_json::json!({
            "chan": 9,
            "name": "House par",
            "tags": [],
            "manufacturer": "Uking",
            "model": "Par",
            "position": {"x": 0.0, "y": 0.0, "z": 0.0},
            "eulers": {"x": 0.0, "y": 0.0, "z": 0.0},
            "quat": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0},
            "size": {"x": 0.2, "y": 0.2, "z": 0.2},
            "universe": 1,
            "address": 1,
            "mirrors": [{"universe": 1, "start_channel": 200}, {"universe": 2, "start_channel": 8}],
        }))
        .expect("valid fixture record");
        let venue = Venue {
            fixtures: vec![record],
            room: vec![],
            screens: vec![],
            props: vec![],
            palettes: Default::default(),
            profile: Default::default(),
            patch: Default::default(),
            group_records: vec![],
        };
        let patch = venue.patch().by_chan(9).unwrap();
        let addresses: Vec<_> = patch
            .addresses()
            .map(|a| (a.universe, a.start_channel))
            .collect();
        assert_eq!(addresses, vec![(1, 1), (1, 200), (2, 8)]);
        let defaults = venue.patch().defaults();
        assert_eq!(defaults[&(9, ignition_proto::Attribute::Dimmer)], 0.0);
        assert_eq!(defaults[&(9, ignition_proto::Attribute::Strobe)], 0.0);
    }

    fn data(rel: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data")
            .join(rel)
    }

    /// r[verify files.directory-or-archive]
    /// r[verify files.venue.assets]
    /// r[verify files.venue]
    #[test]
    fn a_venue_directory_loads_with_its_manifest_and_assets_relative_to_it() {
        let dir = data("venues/norco");
        let venue = Venue::load(&dir).expect("norco loads");
        assert_eq!(Venue::assets_dir(&dir), dir.join("assets"));
        assert!(!venue.fixtures.is_empty());
        assert_eq!(venue.profile.profile, "Ignition");
        // areas.json is the venue's own blocking grid, folded in beside
        // the binding.
        assert!(venue.profile.areas.contains_key("Downstage Left"));
    }

    /// The manifest only names things: a directory without one is the
    /// same venue with every default.
    /// r[verify files.directory-or-archive]
    /// r[verify files.additive-evolution]
    #[test]
    fn a_bare_directory_still_loads_and_an_override_is_honoured() {
        let src = data("venues/riverside");
        let tmp = std::env::temp_dir().join(format!("ig-venue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        for f in ["room", "screens", "props", "profile", "groups", "palettes"] {
            std::fs::copy(src.join(format!("{f}.json")), tmp.join(format!("{f}.json"))).unwrap();
        }
        // No manifest, fixtures under the default name.
        std::fs::copy(src.join("fixtures.json"), tmp.join("fixtures.json")).unwrap();
        let bare = Venue::load(&tmp).expect("a bare directory loads");
        assert_eq!(Venue::assets_dir(&tmp), tmp.join("assets"));
        // A manifest renaming fixtures and the assets folder, with a key
        // this build has never heard of.
        std::fs::rename(tmp.join("fixtures.json"), tmp.join("rig.json")).unwrap();
        std::fs::write(
            tmp.join(ignition_core::show_file::VENUE_MANIFEST_FILE),
            r#"{"version":1,"name":"tmp","files":{"fixtures":"rig.json"},"assets":"models","surveyed":"2026-08"}"#,
        )
        .unwrap();
        let with = Venue::load(&tmp).expect("a manifest directory loads");
        assert_eq!(with.fixtures.len(), bare.fixtures.len());
        assert_eq!(Venue::assets_dir(&tmp), tmp.join("models"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
