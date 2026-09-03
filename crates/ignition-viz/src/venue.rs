//! Loads the extracted venue JSON (`data/venues/<name>/*.json`) — see
//! `docs/domain/norco-venue-reference.md` for what each file contains.

// The maths types come from Bevy's re-export rather than a direct `glam`
// dependency, so the tree only ever has one `Vec3` — see the `bevy` entry
// in the root Cargo.toml. These local `Vec3`/`Quat` structs stay, because
// they are the JSON's field layout (`{x, y, z}`, `{w, x, y, z}`) and
// Deserialize impls, not a maths type.
use bevy::math::{EulerRot, Quat as Rotation, Vec3 as Point};
use ignition_core::show_file::VenueManifest;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    #[must_use]
    pub const fn to_vec3(self) -> Point {
        Point::new(self.x, self.y, self.z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quat {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Quat {
    #[must_use]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// r[impl files.venue.fixtures] - type, position, orientation, patch and tags per fixture
// r[impl patch.writes-the-venue] - the same record reads and writes
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mirrors: Vec<ignition_proto::DmxAddress>,
    /// The operator's own word for this fixture — "SL truss 3", "the one
    /// behind the drum riser". Distinct from `name`, which is the
    /// venue's. Has been in these files since they were written; the
    /// patch sheet is the first thing to show it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// The gel in front of it, if it is a conventional.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gel: String,
    /// The address expressed net-wide rather than per-universe, as the
    /// Eos pull writes it.
    ///
    /// **Derived**, and the reason it is modelled rather than left to
    /// `extra`: it is `(universe - 1) * 512 + address`, so an edit that
    /// moved the address and wrote this back unchanged would leave the
    /// file disagreeing with itself. [`Self::set_address`] is the only
    /// way to move a fixture, and it recomputes this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_address: Option<u32>,
    /// Everything this build does not know about, kept so it can be
    /// written back unchanged (`r[files.additive-evolution]`).
    #[serde(flatten)]
    pub extra: std::collections::BTreeMap<String, serde_json::Value>,
}

const fn default_patched() -> bool {
    true
}

impl FixtureRecord {
    /// The mounting orientation — the *hang*, never the aim. See
    /// `docs/domain/norco-venue-reference.md`.
    #[must_use]
    pub fn orientation(&self) -> Rotation {
        self.quat.to_quat()
    }

    /// Move this fixture on the wire.
    ///
    /// The only way to change an address, because an address is three
    /// fields and not one: `universe`, `address`, and the derived
    /// `global_address` the Eos pull also writes. Setting the first two
    /// by hand and leaving the third is a venue file that disagrees with
    /// itself, and nothing would notice until something read the wrong
    /// one.
    // r[impl patch.writes-the-venue] - an address is one edit, not three
    pub fn set_address(&mut self, address: Option<ignition_proto::DmxAddress>) {
        if let Some(address) = address {
            self.universe = Some(address.universe);
            self.address = Some(address.start_channel);
            // 512 slots per universe, universes 1-based.
            self.global_address = Some(
                u32::from(address.universe)
                    .saturating_sub(1)
                    .saturating_mul(512)
                    .saturating_add(u32::from(address.start_channel)),
            );
            self.patched = true;
        } else {
            // Unpatching keeps the fixture: it is still in the room,
            // still in groups, still selectable — it just has no bytes
            // (`r[patch.unpatched]`).
            self.universe = None;
            self.address = None;
            self.global_address = None;
            self.patched = false;
        }
    }

    /// Re-hang this fixture.
    ///
    /// The only way to change an orientation, and for the same reason as
    /// [`Self::set_address`]: the hang is stored twice, as Euler angles
    /// and as a quaternion, and [`Self::orientation`] reads only the
    /// quaternion. Writing the angles alone is silently ignored — a bug
    /// this project has already shipped once, documented at the top of
    /// `crates/ignition-viz/src/bin/aimwash.rs`.
    // r[impl patch.orientation-is-whole] - both spellings, or neither
    pub fn set_orientation(&mut self, rotation: Rotation) {
        let rotation = rotation.normalize();
        self.quat = Quat {
            w: rotation.w,
            x: rotation.x,
            y: rotation.y,
            z: rotation.z,
        };
        let (z, y, x) = rotation.to_euler(EulerRot::ZYX);
        self.eulers = Vec3 {
            x: x.to_degrees(),
            y: y.to_degrees(),
            z: z.to_degrees(),
        };
    }

    /// This fixture's live DMX address, if the venue data has both pieces —
    /// `None` for anything not DMX-controlled (or missing patch data).
    #[must_use]
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
    #[must_use]
    pub fn placement(&self) -> ignition_proto::Placement {
        let effective_rot = self.orientation();
        ignition_proto::Placement {
            position: ignition_proto::Vec3 {
                x: f64::from(self.position.x),
                y: f64::from(self.position.y),
                z: f64::from(self.position.z),
            },
            orientation: ignition_proto::Quat {
                w: f64::from(effective_rot.w),
                x: f64::from(effective_rot.x),
                y: f64::from(effective_rot.y),
                z: f64::from(effective_rot.z),
            },
        }
    }

    #[must_use]
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
    #[must_use]
    pub const fn color(self) -> [f32; 3] {
        match self {
            Self::Wash => [0.25, 0.75, 0.95],
            Self::Mover => [0.95, 0.55, 0.15],
            Self::Other => [0.65, 0.65, 0.70],
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
    #[must_use]
    pub fn orientation(&self) -> Rotation {
        euler_to_quat(self.eulers)
    }

    /// The canvas this screen belongs to — its own name when it is not
    /// part of a group.
    // r[impl files.venue.canvases]
    #[must_use]
    pub fn canvas_name(&self) -> &str {
        self.canvas.as_deref().unwrap_or(&self.name)
    }
}

/// One `channels` array entry from Eos's export.
///
/// Most groups use its range-string shorthand (`"1-48"`, or a bare `"50"`
/// for one channel), but some (e.g. Norco's "Pars Odd") are exported as a
/// plain JSON array of individual channel numbers instead — Eos
/// apparently picks whichever is more compact for a given selection.
///
/// `untagged` accepts either shape per-element rather than requiring the
/// whole array to agree.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ChannelListEntry {
    Range(String),
    Chan(u32),
}

/// One entry from Eos's own exported `groups.json` — the live rig's real
/// group list (112 of them for Norco: "Pars", "Movers", "OH Movers", ...
///
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

// `Default` is an empty room: no fixtures, no geometry, no patch.
// It is what the studio falls back to when a venue will not load —
// see `Viewport` in `apps/ignition-studio/src/main.rs`. An empty room
// draws as an empty room, which is recoverable; a panic is not.
#[derive(Clone, Default)]
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
    /// Channels a venue-local layer is overriding, if the room has one.
    ///
    /// Empty for a room with no layer, which is every room by default:
    /// the base venue must be complete and playable on its own
    /// (`r[patch.venue-layer.optional]`). The patch sheet marks these
    /// rows, because a room behaving differently from its own file is a
    /// fact somebody needs before the night rather than during it.
    // r[impl patch.venue-layer.visible] - which fixtures are overridden
    #[allow(
        clippy::struct_field_names,
        reason = "it is the overridden channels, and any shorter name reads as something else"
    )]
    pub overridden: Vec<u32>,
    /// Where this room's universes go on the wire, from the manifest's
    /// `dmx` key. `None` when the manifest has none; `output_config()`
    /// is the answer either way.
    // r[impl dmx.venue-config] - the room's network lives with the room
    pub dmx: Option<ignition_dmx::OutputConfig>,
}

/// Replace a file's contents without ever leaving it half-written.
///
/// A venue file is read at startup and a truncated one is a room that
/// will not open, so the write goes to a sibling temporary and is
/// renamed over the original — a rename within a directory is atomic on
/// every filesystem this runs on. The temporary is removed if the rename
/// fails, so a failed save does not litter the venue.
pub(crate) fn write_atomically(path: &Path, contents: &str) -> anyhow::Result<()> {
    let Some(dir) = path.parent() else {
        anyhow::bail!("{} has no directory to write into", path.display());
    };
    let temporary = path.with_extension("json.writing");
    std::fs::create_dir_all(dir)?;
    if let Err(error) = std::fs::write(&temporary, contents) {
        let _ = std::fs::remove_file(&temporary);
        anyhow::bail!("writing {}: {error}", temporary.display());
    }
    if let Err(error) = std::fs::rename(&temporary, path) {
        let _ = std::fs::remove_file(&temporary);
        anyhow::bail!("replacing {}: {error}", path.display());
    }
    Ok(())
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

    /// Every universe a patched fixture — primary or mirror — lands on,
    /// ascending.
    pub fn patched_universes(&self) -> Vec<u16> {
        let mut universes: Vec<u16> = self
            .fixtures
            .iter()
            .filter(|f| f.patched)
            .flat_map(|f| {
                f.dmx_address()
                    .into_iter()
                    .chain(f.mirrors.iter().copied())
                    .map(|a| a.universe)
            })
            .collect();
        universes.sort_unstable();
        universes.dedup();
        universes
    }

    /// The output config to bind: the manifest's, or — for a venue
    /// whose manifest says nothing — every patched universe on sACN
    /// multicast at the default priority, which is what a rig with no
    /// configuration expects to hear.
    // r[impl dmx.venue-config] - absent config falls back to sACN multicast per patched universe
    pub fn output_config(&self) -> ignition_dmx::OutputConfig {
        self.dmx.as_ref().map_or_else(
            || default_output_config(&self.patched_universes()),
            Clone::clone,
        )
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
    /// The fixture type this model resolved to, by console name. Empty
    /// when the hand-written fallback table answered.
    pub fixture_type: String,
    /// The fixture type's mode this patch resolved to — what the room
    /// said, in the model string or in the address spacing it left.
    /// `legacy` for a model answered by the table rather than a
    /// document.
    pub mode: String,
    /// How the chart was come by: `manual`, `listing` or `guess`
    /// (`r[patch.type-confidence]`). A patch sheet shows this, because
    /// "the channel order on this fixture is a guess" is something an
    /// operator wants to know before the fixture does something
    /// unexpected.
    pub confidence: String,
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

/// How many channels the room left before the next fixture, per fixture.
///
/// The venue records an address and never a mode, but it recorded the
/// mode anyway: a rig patched at 1, 8, 15 is saying those fixtures are
/// seven channels wide. This is that fact, read back out — for each
/// patched fixture, the distance to the next distinct address above it
/// in the same universe, or `None` for the last one in its universe.
///
/// Two fixtures on the *same* address are multipatch
/// (`r[files.venue.multipatch]`), not a spacing claim, so an equal
/// address is skipped rather than reported as a gap of zero.
fn address_gaps(venue: &Venue) -> Vec<Option<u16>> {
    // Addresses per universe, sorted and deduplicated, so "the next one
    // above" is a scan of a short sorted list rather than of the rig.
    let mut by_universe: std::collections::HashMap<u16, Vec<u16>> =
        std::collections::HashMap::new();
    for address in venue.fixtures.iter().filter_map(FixtureRecord::dmx_address) {
        by_universe
            .entry(address.universe)
            .or_default()
            .push(address.start_channel);
    }
    for addresses in by_universe.values_mut() {
        addresses.sort_unstable();
        addresses.dedup();
    }
    venue
        .fixtures
        .iter()
        .map(|f| {
            let address = f.dmx_address()?;
            let addresses = by_universe.get(&address.universe)?;
            let next = addresses
                .iter()
                .copied()
                .find(|a| *a > address.start_channel)?;
            Some(next.saturating_sub(address.start_channel))
        })
        .collect()
}

impl PatchTable {
    // r[impl patch.type-is-data] - the channel map comes from a document
    // r[impl patch.type-modes] - and the room's own spacing picks the mode
    fn build(venue: &Venue) -> Self {
        let gaps = address_gaps(venue);
        let entries = venue
            .fixtures
            .iter()
            .enumerate()
            .map(|(index, f)| {
                let manufacturer = f.manufacturer.as_deref().unwrap_or("");
                let model = f.model.as_deref().unwrap_or("");
                let address = f.dmx_address()?;
                let gap = gaps.get(index).copied().flatten();
                let found = crate::fixture_library::profile_for(manufacturer, model, gap)?;
                Some(Patch {
                    address,
                    mirrors: f.mirrors.clone(),
                    map: found.profile.map.clone(),
                    profile: found.profile,
                    fixture_type: found.fixture_type,
                    mode: found.mode,
                    confidence: found.confidence,
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
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Patch> {
        self.entries.get(index).and_then(|p| p.as_ref())
    }

    /// The patch for console channel `chan`.
    #[must_use]
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
    #[must_use]
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
    // Splits inherit by the same rule as colours below. They are read
    // from the profile file's own `splits` key rather than through
    // `Profile`, which does not carry them yet: a split is palette
    // vocabulary, and this is the one place the profile is opened as a
    // palette.
    // r[impl color.multi] - profile split defaults inherited unless the venue overrides
    #[derive(serde::Deserialize, Default)]
    struct ProfileSplits {
        #[serde(default)]
        splits: Vec<ignition_core::preset::ColorSplit>,
    }
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
            inherited = inherited.saturating_add(1);
        }
    }
    tracing::debug!(inherited, "venue: colours inherited from profile");

    let profile_splits = serde_json::from_str::<ProfileSplits>(&raw).unwrap_or_default();
    let mut inherited = 0usize;
    for split in profile_splits.splits {
        if !palettes.splits.iter().any(|s| s.name == split.name) {
            palettes.splits.push(split);
            inherited = inherited.saturating_add(1);
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
        added = added.saturating_add(1);
    }
    tracing::debug!(added, "venue: focus roles aliased onto the palette");
    palettes
}

/// sACN multicast, priority 100, for each of `universes` — built as
/// JSON and parsed, so the shape is the one the manifest would carry.
///
/// # Panics
///
/// Never in practice: the JSON is built to `OutputConfig`'s own shape a
/// couple of lines below.
#[must_use]
// The JSON built above always matches `OutputConfig`'s own shape — this
// isn't a file read off disk, it's a literal this function just wrote —
// so `from_value` failing here would mean the two shapes drifted apart
// at compile time, not a bad runtime input.
#[expect(
    clippy::expect_used,
    reason = "the JSON is built two lines above to OutputConfig's own shape; see the comment above"
)]
pub fn default_output_config(universes: &[u16]) -> ignition_dmx::OutputConfig {
    let entries: serde_json::Map<String, serde_json::Value> = universes
        .iter()
        .map(|u| {
            (
                u.to_string(),
                serde_json::json!({ "sacn": { "priority": 100 } }),
            )
        })
        .collect();
    serde_json::from_value(serde_json::json!({ "universes": entries }))
        .expect("the default output config is well-formed")
}

impl Venue {
    // r[impl files.venue] - the seven JSON files under data/venues/<name>/
    // r[impl profile.venue-declares-what-it-implements] - profile.json is optional
    // r[impl profile.venue-binds] - the binding loads with the venue, not with any show
    /// # Errors
    ///
    /// If any of the venue's JSON files is missing or fails to parse.
    /// Forget the resolved patch, so the next read rebuilds it.
    ///
    /// The patch table is resolved once and cached, which is right for a
    /// venue that never changes and wrong the moment one does. Every
    /// edit that touches an address, a model or the patched flag has to
    /// call this, or the room goes on being addressed the way it was
    /// before the edit.
    pub fn repatch(&mut self) {
        self.patch = std::sync::OnceLock::default();
    }

    /// The fixture on `chan`, if the room has one.
    ///
    /// By channel rather than by index deliberately: an index is a
    /// position in a file that inserting a fixture changes, and an edit
    /// that arrived a frame late would then land on the wrong light.
    #[must_use]
    pub fn by_chan_mut(&mut self, chan: u32) -> Option<&mut FixtureRecord> {
        self.fixtures.iter_mut().find(|f| f.chan == Some(chan))
    }

    /// The lowest channel number nothing is using.
    #[must_use]
    pub fn next_free_chan(&self) -> u32 {
        let mut used: Vec<u32> = self.fixtures.iter().filter_map(|f| f.chan).collect();
        used.sort_unstable();
        let mut next = 1_u32;
        for chan in used {
            if chan == next {
                next = next.saturating_add(1);
            }
        }
        next
    }

    /// The lowest address in `universe` with `footprint` free channels
    /// after it (`r[patch.address]`).
    ///
    /// `None` when the universe cannot hold another one — a real answer,
    /// and better than offering an address that would collide.
    #[must_use]
    pub fn next_free_address(&self, universe: u16, footprint: u16) -> Option<u16> {
        if footprint == 0 || footprint > 512 {
            return None;
        }
        let patch = self.patch();
        // What each of the 512 slots holds, by fixture index.
        let mut taken = [false; 512];
        for (index, fixture) in self.fixtures.iter().enumerate() {
            let Some(address) = fixture.dmx_address() else {
                continue;
            };
            if address.universe != universe {
                continue;
            }
            let width = patch.get(index).map_or(1, |p| p.map.footprint).max(1);
            for offset in 0..width {
                let slot = address
                    .start_channel
                    .saturating_add(offset)
                    .saturating_sub(1);
                if let Some(cell) = taken.get_mut(usize::from(slot)) {
                    *cell = true;
                }
            }
        }
        let mut run = 0_u16;
        for (index, occupied) in taken.iter().enumerate() {
            if *occupied {
                run = 0;
                continue;
            }
            run = run.saturating_add(1);
            if run >= footprint {
                let slot = u16::try_from(index).unwrap_or(u16::MAX).saturating_add(1);
                return Some(slot.saturating_sub(footprint).saturating_add(1));
            }
        }
        None
    }

    /// Write the fixtures back to the venue directory.
    ///
    /// Only `fixtures.json`, because that is the only file the patch
    /// edits: the room, the screens, the props and the palettes are
    /// untouched by patching and rewriting them would put unrelated
    /// churn in a diff somebody has to review.
    ///
    /// The same text form the file was read in
    /// (`r[files.text-and-diffable]`), with every key this build did not
    /// understand written back unchanged
    /// (`r[files.additive-evolution]`) — a venue edited by the studio
    /// and one edited by hand have to be the same kind of artifact.
    ///
    /// Writes through a temporary file in the same directory and renames
    /// over the original, so a crash mid-write cannot leave a venue
    /// half-written. A half-written `fixtures.json` is a room that will
    /// not open.
    ///
    /// # Errors
    ///
    /// If the directory cannot be written, or the records will not
    /// serialise.
    // r[impl patch.writes-the-venue] - back to the venue's own files
    pub fn save_fixtures(&self, dir: impl AsRef<Path>) -> anyhow::Result<()> {
        let dir = dir.as_ref();
        let manifest = VenueManifest::load_dir(dir).map_err(|e| anyhow::anyhow!("{e}"))?;
        let path = manifest.file(dir, "fixtures");
        let json = serde_json::to_string_pretty(&self.fixtures)?;
        write_atomically(&path, &format!("{json}\n"))
    }

    /// Read a venue from its directory.
    ///
    /// # Errors
    ///
    /// If the manifest names a version this build does not know, if a
    /// required file (`fixtures`, `room`, `screens`, `props`) is missing
    /// or malformed, or if the manifest's `dmx` block will not parse. The
    /// optional files — groups, palettes, profile, areas — are absent on
    /// a room somebody is still setting up, which is not an error.
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
        // The manifest's `dmx` block, typed here rather than in the
        // core so the core never links the transmit crate. A block
        // that is present but malformed is a load error: a venue that
        // silently falls back to multicast is the "silently not
        // sending" the spec warns about.
        // r[impl dmx.venue-config] - parsed from the manifest, refused if malformed
        let dmx = manifest
            .dmx
            .clone()
            .map(serde_json::from_value::<ignition_dmx::OutputConfig>)
            .transpose()
            .map_err(|e| anyhow::anyhow!("{}: bad `dmx` block: {e}", dir.display()))?;
        let mut venue = Self {
            palettes,
            profile,
            patch: std::sync::OnceLock::default(),
            overridden: Vec::new(),
            dmx,
            fixtures: serde_json::from_str(&read("fixtures")?)?,
            room: serde_json::from_str(&read("room")?)?,
            screens: serde_json::from_str(&read("screens")?)?,
            props: serde_json::from_str(&read("props")?)?,
            group_records,
        };
        // The room's own local changes, last, so they sit over
        // everything the base venue said (`r[patch.venue-layer]`). A
        // missing layer is the normal case and not an error; a malformed
        // one *is*, because applying half of somebody's changes and
        // dropping the rest is worse than refusing to open.
        // r[impl patch.venue-layer] - laid over the room it belongs to
        // r[impl patch.venue-layer.optional] - and absent by default
        if let Some(layer) = crate::venue_layer::VenueLayer::load(dir)? {
            venue.overridden = layer.apply(&mut venue);
            if !venue.overridden.is_empty() {
                tracing::info!(
                    venue = %dir.display(),
                    fixtures = venue.overridden.len(),
                    "a venue-local layer is overriding this room"
                );
            }
        }
        Ok(venue)
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
            .map(FixtureRecord::placement)
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

    /// The room's real extent: every room surface's full size, not the
    /// centre `bounds()` frames cameras on. A beam runs to a wall, not
    /// to the middle of one, so this is what sizes a throw. Falls back
    /// to `bounds()` for a venue with no room geometry.
    // r[impl viz.beam-reach] - throws are sized by the room's surfaces
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Vec3 arithmetic is float, component-wise, and cannot panic or overflow"
    )]
    pub fn room_extent(&self) -> (Point, Point) {
        if self.room.is_empty() {
            return self.bounds();
        }
        let mut min = Point::splat(f32::INFINITY);
        let mut max = Point::splat(f32::NEG_INFINITY);
        for g in &self.room {
            let half = g.size.to_vec3().abs() * 0.5;
            let centre = g.position.to_vec3();
            min = min.min(centre - half);
            max = max.max(centre + half);
        }
        (min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r[verify viz.beam-reach] - the room extent is the surfaces' full size
    #[test]
    fn the_room_extent_reaches_the_far_side_of_every_surface() {
        let surface = |name: &str, p: [f32; 3], s: [f32; 3]| -> GeometryRecord {
            serde_json::from_value(serde_json::json!({
                "name": name,
                "position": {"x": p[0], "y": p[1], "z": p[2]},
                "eulers": {"x": 0.0, "y": 0.0, "z": 0.0},
                "size": {"x": s[0], "y": s[1], "z": s[2]},
            }))
            .expect("record")
        };
        let venue = Venue {
            fixtures: vec![],
            room: vec![
                surface("Floor", [0.0, -5.0, 0.0], [10.0, 20.0, 0.0]),
                surface("Ceiling", [0.0, -5.0, 3.0], [10.0, 20.0, 0.0]),
            ],
            screens: vec![],
            props: vec![],
            group_records: vec![],
            palettes: ignition_core::Palettes::default(),
            profile: ignition_core::profile::VenueProfile::default(),
            patch: std::sync::OnceLock::default(),
            overridden: Vec::new(),
            dmx: None,
        };
        let (min, max) = venue.room_extent();
        assert_eq!(min, Point::new(-5.0, -15.0, 0.0));
        assert_eq!(max, Point::new(5.0, 5.0, 3.0));
        // Where `bounds()` stops at the centres.
        let (bmin, bmax) = venue.bounds();
        assert_eq!((bmin.x, bmax.y), (0.0, -5.0));
    }

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
        let nums: Vec<ChannelListEntry> = serde_json::from_str(r"[1, 3, 5, 7]").unwrap();
        assert!(matches!(nums[0], ChannelListEntry::Chan(1)));
    }

    #[test]
    fn groups_resolves_eos_group_records_into_ignition_core_groups() {
        let venue = Venue {
            fixtures: vec![],
            room: vec![],
            screens: vec![],
            props: vec![],
            palettes: ignition_core::Palettes::default(),
            profile: ignition_core::profile::VenueProfile::default(),
            patch: std::sync::OnceLock::default(),
            overridden: Vec::new(),
            dmx: None,
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
            palettes: ignition_core::Palettes::default(),
            profile: ignition_core::profile::VenueProfile::default(),
            patch: std::sync::OnceLock::default(),
            overridden: Vec::new(),
            dmx: None,
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
            palettes: ignition_core::Palettes::default(),
            profile: ignition_core::profile::VenueProfile::default(),
            patch: std::sync::OnceLock::default(),
            overridden: Vec::new(),
            dmx: None,
            group_records: vec![],
        };
        let patch = venue.patch().by_chan(9).unwrap();
        let addresses: Vec<_> = patch
            .addresses()
            .map(|a| (a.universe, a.start_channel))
            .collect();
        assert_eq!(addresses, vec![(1, 1), (1, 200), (2, 8)]);
        let defaults = venue.patch().defaults();
        assert!((defaults[&(9, ignition_proto::Attribute::Dimmer)] - (0.0)).abs() < 1e-6);
        assert!((defaults[&(9, ignition_proto::Attribute::Strobe)] - (0.0)).abs() < 1e-6);
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

    /// r[verify dmx.venue-config] - the manifest's `dmx` block round-trips into the venue, typed
    #[test]
    fn the_venue_manifest_carries_the_output_config() {
        use ignition_core::show_file::VenueManifest;
        let json = serde_json::json!({
            "version": 1,
            "name": "Room",
            "dmx": { "universes": {
                "1": { "sacn": { "priority": 120 }, "artnet": { "net": 0, "subnet": 0, "universe": 0 } },
                "3": { "sacn": { "priority": 100 } }
            } }
        });
        let manifest: VenueManifest = serde_json::from_value(json).expect("parses");
        let config: ignition_dmx::OutputConfig =
            serde_json::from_value(manifest.dmx.clone().expect("dmx kept")).expect("typed");
        assert_eq!(config.universes.len(), 2);
        assert_eq!(
            config.universes[&1].sacn.as_ref().map(|s| s.priority),
            Some(120)
        );
        assert!(config.universes[&1].artnet.is_some());
        assert!(config.universes[&3].artnet.is_none());
        // And back out through the manifest unchanged.
        let again: VenueManifest =
            serde_json::from_str(&serde_json::to_string(&manifest).unwrap()).unwrap();
        assert_eq!(again.dmx, manifest.dmx);
        // `dmx` is a named field, not something `extra` swallowed.
        assert!(!manifest.extra.contains_key("dmx"));
    }

    /// r[verify dmx.venue-config] - Norco's file names four universes on both protocols
    #[test]
    fn norco_declares_four_universes_on_sacn_and_artnet() {
        let Ok(venue) = Venue::load(data("venues/norco")) else {
            return;
        };
        let config = venue.output_config();
        assert_eq!(
            config.universes.keys().copied().collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        for (n, u) in &config.universes {
            assert_eq!(u.sacn.as_ref().map(|s| s.priority), Some(100), "U{n} sACN");
            assert_eq!(
                u.artnet.as_ref().map(|a| a.universe),
                Some(crate::num::u8_of_u16(*n - 1)),
                "U{n} Art-Net"
            );
        }
    }

    /// r[verify dmx.venue-config] - no block means every patched universe on sACN multicast
    #[test]
    fn a_venue_without_a_dmx_block_falls_back_to_sacn_on_every_patched_universe() {
        let record = |universe: u16, address: u16| -> FixtureRecord {
            serde_json::from_value(serde_json::json!({
                "chan": address, "name": "p", "tags": [], "patched": true,
                "manufacturer": "uking", "model": "par",
                "position": {"x": 0.0, "y": 0.0, "z": 0.0},
                "eulers": {"x": 0.0, "y": 0.0, "z": 0.0},
                "quat": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0},
                "size": {"x": 0.2, "y": 0.2, "z": 0.2},
                "universe": universe, "address": address,
            }))
            .expect("record")
        };
        let venue = Venue {
            fixtures: vec![record(2, 1), record(2, 10), record(5, 1)],
            room: vec![],
            screens: vec![],
            props: vec![],
            group_records: vec![],
            palettes: ignition_core::Palettes::default(),
            profile: ignition_core::profile::VenueProfile::default(),
            patch: std::sync::OnceLock::default(),
            overridden: Vec::new(),
            dmx: None,
        };
        let config = venue.output_config();
        assert_eq!(config.universes.keys().copied().collect::<Vec<_>>(), [2, 5]);
        for u in config.universes.values() {
            let sacn = u.sacn.as_ref().expect("sACN");
            assert_eq!(sacn.priority, 100);
            assert!(sacn.multicast, "multicast by default");
            assert!(u.artnet.is_none());
        }
    }
}
