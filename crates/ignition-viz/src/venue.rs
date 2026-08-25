//! Loads the extracted venue JSON (`data/venues/<name>/*.json`) — see
//! `docs/domain/norco-venue-reference.md` for what each file contains.

// The maths types come from Bevy's re-export rather than a direct `glam`
// dependency, so the tree only ever has one `Vec3` — see the `bevy` entry
// in the root Cargo.toml. These local `Vec3`/`Quat` structs stay, because
// they are the JSON's field layout (`{x, y, z}`, `{w, x, y, z}`) and
// Deserialize impls, not a maths type.
use bevy::math::{EulerRot, Quat as Rotation, Vec3 as Point};
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
pub struct GeometryRecord {
    pub name: String,
    pub position: Vec3,
    pub eulers: Vec3,
    pub size: Vec3,
}

impl GeometryRecord {
    pub fn orientation(&self) -> Rotation {
        euler_to_quat(self.eulers)
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
}

impl Venue {
    pub fn load(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let dir = dir.as_ref();
        let read = |name: &str| -> anyhow::Result<String> {
            std::fs::read_to_string(dir.join(name))
                .map_err(|e| anyhow::anyhow!("reading {}: {e}", dir.join(name).display()))
        };
        // groups.json is optional — a venue extract without one (or an
        // older extract, predating this field) just has no named groups
        // to recipe-target by name; `RecipeTarget::Chans` still works.
        let group_records = match std::fs::read_to_string(dir.join("groups.json")) {
            Ok(raw) => serde_json::from_str(&raw)?,
            Err(_) => Vec::new(),
        };
        // palettes.json is optional for the same reason groups.json is —
        // a show can always write its colours and points out inline.
        let palettes = match std::fs::read_to_string(dir.join("palettes.json")) {
            Ok(raw) => serde_json::from_str(&raw)?,
            Err(_) => ignition_core::Palettes::default(),
        };
        Ok(Self {
            palettes,
            fixtures: serde_json::from_str(&read("fixtures.json")?)?,
            room: serde_json::from_str(&read("room.json")?)?,
            screens: serde_json::from_str(&read("screens.json")?)?,
            props: serde_json::from_str(&read("props.json")?)?,
            group_records,
        })
    }

    /// The venue's real groups, resolved into `ignition_core::Group`'s
    /// plain `(name, chans)` shape — what `ignition_core::recipe`'s
    /// `RecipeTarget::Group` actually resolves against.
    pub fn groups(&self) -> Vec<ignition_core::Group> {
        self.group_records
            .iter()
            .map(|g| ignition_core::Group {
                name: g.label.clone(),
                chans: parse_channel_ranges(&g.channels),
            })
            .collect()
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
}
