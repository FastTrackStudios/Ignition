//! The real show and the real room, baked into the binary.
//!
//! This used to be a six-channel demo built by hand, because the phone
//! had no data source. It still has none — there is no vox link to a
//! running console — but "no live source" and "no real data" are
//! different problems, and only the first one is true. The show file and
//! the venue's patch and groups are in the repo, they are the same files
//! the desk opens, and `include_str!` puts them in the app bundle.
//!
//! So the phone shows the actual NSYNC cue list against the actual Room
//! 138 rig: forty-three cues, forty fixtures, the groups the profile
//! binds its roles through. Nothing here is invented, and a change to
//! `data/songs/bye-bye-bye.json` reaches the phone on the next build.
//!
//! Baked rather than read from disk because an iOS app has no working
//! directory to speak of and no `data/` beside it. The cost is that the
//! files are fixed at build time, which is right for now — when the vox
//! link lands, this module is the only one that changes.

use ignition_core::selection::{FixtureInfo, Rig, Roles};
use ignition_core::{ChanId, CueList, Group, PatchEntry, Placement, Quat, Selection, Vec3};
use ignition_core::{Palettes, Show, SpeedMasters};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The show the desk opens on, as shipped.
const SHOW_JSON: &str = include_str!("../../../data/songs/bye-bye-bye.json");
/// Room 138's patch and groups — the venue the studio defaults to.
const PATCH_JSON: &str = include_str!("../../../data/venues/room138-cbu/patch.json");
const GROUPS_JSON: &str = include_str!("../../../data/venues/room138-cbu/groups.json");
/// Fixture names and tags, for a patch list that reads as a rig rather
/// than as a column of numbers.
const FIXTURES_JSON: &str = include_str!("../../../data/venues/room138-cbu/fixtures.json");
/// The venue's role bindings and its named colours — what turns a show
/// written against `Role("Key")` into fixtures, and `"Turquoise"` into a
/// triple.
const PROFILE_JSON: &str = include_str!("../../../data/venues/room138-cbu/profile.json");
const PALETTES_JSON: &str = include_str!("../../../data/venues/room138-cbu/palettes.json");
/// The shipped profile, for the colours a venue inherits rather than
/// declares. Room 138's own palette carries its focus points and an
/// empty colour list — the twenty-eight named colours the show says
/// (`"Gold"`, `"Turquoise"`) are the profile's, and the desk's venue
/// loader folds them in on load. Without this the phone resolved every
/// colour to nothing and showed levels with no hue.
const PROFILE_COLORS_JSON: &str = include_str!("../../../data/profiles/ignition.ig-profile");

/// A fixture the venue file has no hang for — origin, upright.
const UNPLACED: Placement = Placement {
    position: Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
    orientation: Quat {
        w: 1.0,
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
};

/// A patch row as the venue file writes it.
///
/// Its own struct rather than `PatchEntry`: the venue's rows carry a
/// footprint and a provenance the phone has no use for, and borrowing
/// the console's type here would mean widening that type to fit a file
/// format. The conversion is one function and it is below.
#[derive(Debug, Clone, Deserialize)]
struct PatchRow {
    chan: ChanId,
    #[serde(default)]
    manufacturer: String,
    model: String,
    #[serde(default)]
    universe: Option<u16>,
    #[serde(default)]
    address: Option<u16>,
}

/// The name and hang the fixtures file carries for a channel.
#[derive(Debug, Clone, Deserialize)]
struct FixtureRow {
    chan: ChanId,
    name: String,
    position: Vec3,
    /// The mount orientation. The venue file also carries `eulers`; the
    /// quaternion is the one the domain reads.
    quat: Quat,
}

/// One venue group record — `target`/`label`/`channels`, where the
/// channels are strings so a run can be written `"1-16"`.
#[derive(Debug, Clone, Deserialize)]
struct GroupRow {
    label: String,
    channels: Vec<String>,
}

/// `"1-16"` and `"7"` both mean a run of channels.
fn parse_run(s: &str) -> Vec<ChanId> {
    match s.split_once('-') {
        Some((a, b)) => match (a.trim().parse::<ChanId>(), b.trim().parse::<ChanId>()) {
            (Ok(a), Ok(b)) if a <= b => (a..=b).collect(),
            _ => Vec::new(),
        },
        None => s
            .trim()
            .parse::<ChanId>()
            .map(|c| vec![c])
            .unwrap_or_default(),
    }
}

/// The shipped cue list.
///
/// A parse failure here is a broken build, not a runtime condition: the
/// file is compiled in, so if it does not parse it did not parse on the
/// developer's machine either.
pub fn cues() -> CueList {
    serde_json::from_str(SHOW_JSON).expect("the shipped show parses")
}

/// The venue's patch, as the console's own `PatchEntry`.
pub fn patch() -> Vec<PatchEntry> {
    let rows: Vec<PatchRow> = serde_json::from_str(PATCH_JSON).expect("the shipped patch parses");
    let fixtures: Vec<FixtureRow> =
        serde_json::from_str(FIXTURES_JSON).expect("the shipped fixtures parse");
    rows.into_iter()
        .map(|r| {
            let named = fixtures.iter().find(|f| f.chan == r.chan);
            PatchEntry {
                chan: r.chan,
                // The fixture's own name where it has one — `Tower 2
                // Light 4` says more than the model does — and the model
                // otherwise.
                fixture_type: named.map(|f| f.name.clone()).unwrap_or_else(|| {
                    if r.manufacturer.is_empty() {
                        r.model.clone()
                    } else {
                        format!("{} {}", r.manufacturer, r.model)
                    }
                }),
                // The real hang, not a placeholder: the file has it, and
                // a patch list that knows where a fixture is can later
                // draw one.
                placement: named
                    .map(|f| Placement {
                        position: f.position,
                        orientation: f.quat,
                    })
                    .unwrap_or(UNPLACED),
                dmx: match (r.universe, r.address) {
                    (Some(universe), Some(start_channel)) => Some(ignition_proto::DmxAddress {
                        universe,
                        start_channel,
                    }),
                    _ => None,
                },
            }
        })
        .collect()
}

/// The venue's groups, in file order.
pub fn groups() -> Vec<Group> {
    let rows: Vec<GroupRow> = serde_json::from_str(GROUPS_JSON).expect("the shipped groups parse");
    rows.into_iter()
        .map(|g| Group {
            name: g.label,
            chans: g.channels.iter().flat_map(|s| parse_run(s)).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything baked in parses, and is the size the room actually is.
    ///
    /// The point of the assertions being specific: a silent truncation —
    /// a patch that parsed to three rows because the file format moved —
    /// would otherwise look like a working app with a short list.
    #[test]
    fn the_shipped_show_and_room_parse() {
        let cues = cues();
        assert!(
            cues.cues.len() > 20,
            "the show has {} cues; the shipped one has 43",
            cues.cues.len()
        );
        assert!(!cues.triggers.is_empty(), "the show lost its triggers");

        let patch = patch();
        assert_eq!(patch.len(), 40, "Room 138 has forty fixtures");
        assert!(
            patch.iter().all(|p| p.dmx.is_some()),
            "every fixture in this room is patched"
        );
        assert!(
            patch.iter().any(|p| p.fixture_type.starts_with("Tower")),
            "the patch lost the fixtures' names: {:?}",
            patch.first().map(|p| &p.fixture_type)
        );
        assert!(
            patch.iter().any(|p| p.placement.position.z > 0.1),
            "every fixture came through at the origin; the hang was lost"
        );

        let groups = groups();
        assert!(groups.len() > 10, "the venue ships more groups than that");
        let towers = groups
            .iter()
            .find(|g| g.name == "Towers")
            .expect("the Towers group");
        assert_eq!(towers.chans.len(), 24, "four towers of six");
    }

    /// A run and a single both read.
    #[test]
    fn a_channel_run_expands() {
        assert_eq!(parse_run("7"), vec![7]);
        assert_eq!(parse_run("3-6"), vec![3, 4, 5, 6]);
        assert!(parse_run("").is_empty());
        assert!(parse_run("6-3").is_empty(), "a backwards run is not a run");
    }
}

// ── Cooking the show ─────────────────────────────────────────────────

/// The venue's role bindings, as the resolver wants them.
///
/// `Roles` is one method — "what does this room call its Key?" — so the
/// binding table is a map of role name to selection and this is the
/// whole implementation. It is what makes the shipped show, which names
/// roles and never a group, resolve to Room 138's actual fixtures.
// r[impl profile.resolution-by-role]
pub struct VenueRoles(BTreeMap<String, Selection>);

impl Roles for VenueRoles {
    fn role(&self, name: &str) -> Option<&Selection> {
        self.0.get(name)
    }
}

/// `profile.json`'s `groups` table: role name -> `{"Group": "..."}`.
#[derive(Debug, Clone, Deserialize)]
struct ProfileFile {
    #[serde(default)]
    groups: BTreeMap<String, BTreeMap<String, String>>,
}

pub fn roles() -> VenueRoles {
    let p: ProfileFile = serde_json::from_str(PROFILE_JSON).expect("the shipped profile parses");
    VenueRoles(
        p.groups
            .into_iter()
            .filter_map(|(role, binding)| {
                binding
                    .get("Group")
                    .map(|g| (role, Selection::Group(g.clone())))
            })
            .collect(),
    )
}

/// Just the `colors` list out of the profile.
#[derive(Debug, Clone, Deserialize)]
struct ProfileColors {
    #[serde(default)]
    colors: Vec<ignition_core::ColorPreset>,
}

/// The venue's palette, with the profile's colours folded in.
///
/// The same order the desk's loader uses: the venue's own entries win,
/// and anything it does not name falls back to the profile's. A room
/// that has never been given a palette still speaks the whole
/// vocabulary the show is written in.
// r[impl color.scope.fallback-order]
pub fn palettes() -> Palettes {
    let mut p: Palettes = serde_json::from_str(PALETTES_JSON).expect("the shipped palettes parse");
    let profile: ProfileColors =
        serde_json::from_str(PROFILE_COLORS_JSON).expect("the shipped profile parses");
    for c in profile.colors {
        if !p.colors.iter().any(|own| own.name == c.name) {
            p.colors.push(c);
        }
    }
    p
}

/// The rig, from the patch — chan, model and where each fixture hangs.
///
/// A `Rig` is what a selection asks "which of these is furthest stage
/// left", so a show that orders its wash by height needs this and not
/// just a channel list.
pub fn rig() -> Rig {
    let rows: Vec<PatchRow> = serde_json::from_str(PATCH_JSON).expect("the shipped patch parses");
    let placed = patch();
    Rig::new(
        rows.into_iter()
            .map(|r| FixtureInfo {
                chan: r.chan,
                placement: placed
                    .iter()
                    .find(|p| p.chan == r.chan)
                    .map(|p| p.placement.clone()),
                manufacturer: r.manufacturer,
                // The *model*, not the fixture's name: a colour preset is
                // scoped by model when it resolves onto emitters, and
                // `Tower 2 Light 4` is not a model.
                model: r.model,
                tags: Vec::new(),
            })
            .collect(),
    )
}

/// Everything the cue player needs to cook a cue into levels.
///
/// The phone builds the same `Show` the desk does, from the same files —
/// groups, palettes, rig, role bindings and the shipped effect library.
/// That is why the cue list here can show the levels the rig would
/// actually take rather than a plausible-looking mock.
pub struct Cooked {
    /// Every patched channel, in patch order — what the meter row draws.
    pub channels: Vec<ChanId>,
    pub groups: Vec<Group>,
    pub palettes: Palettes,
    pub rig: Rig,
    pub roles: VenueRoles,
    pub speeds: SpeedMasters,
    pub library: BTreeMap<String, ignition_core::Recipe>,
    /// The shipped bundles — several effects taken as one.
    pub bundles: BTreeMap<String, ignition_core::profile::Bundle>,
}

impl Cooked {
    pub fn load() -> Self {
        Self {
            channels: patch().iter().map(|p| p.chan).collect(),
            groups: groups(),
            palettes: palettes(),
            rig: rig(),
            roles: roles(),
            // 120 until a transport says otherwise; the phone has no
            // clock of its own yet.
            speeds: [("Song".to_string(), 120.0), ("Tap".to_string(), 120.0)]
                .into_iter()
                .collect(),
            library: ignition_core::effects::library(),
            bundles: ignition_core::effects::bundles(),
        }
    }

    pub fn show(&self) -> Show<'_> {
        Show {
            palettes: &self.palettes,
            speeds: &self.speeds,
            roles: &self.roles,
            library: &self.library,
            bundles: &self.bundles,
            ..Show::new(&self.groups, &self.rig)
        }
    }
}

#[cfg(test)]
mod cook_tests {
    use super::*;
    use ignition_core::CuePlayer;

    /// The shipped show cooks to real levels on the shipped rig.
    ///
    /// This is the assertion that matters, and the one the app was
    /// missing: `bye-bye-bye.json` targets roles and never a group, so
    /// without the venue's bindings every recipe resolves to nothing and
    /// the meters sit at zero while the cue list scrolls happily. A
    /// parse test would not have noticed.
    #[test]
    fn taking_a_cue_lights_the_rig() {
        let cooked = Cooked::load();
        let list = cues();
        let mut player = CuePlayer::new(list.cues.clone());

        // Walk into the first chorus, taking each cue and letting its
        // fade land.
        let chorus = list
            .cues
            .iter()
            .position(|c| c.name == "CH 1")
            .expect("the show has a first chorus");
        for i in 0..=chorus {
            player.go(&cooked.show());
            player.tick(list.cues[i].fade_secs.max(0.1));
        }

        let out = player.output(&cooked.show());
        let lit: Vec<_> = out
            .iter()
            .filter(|((_, attr), v)| *attr == ignition_core::Attribute::Dimmer && **v > 0.05)
            .collect();
        assert!(
            lit.len() >= 8,
            "the chorus lit {} fixtures of forty; the roles are not resolving",
            lit.len()
        );

        // And the colour resolved too — a role that binds but a palette
        // that does not would give levels with no hue.
        assert!(
            out.keys()
                .any(|(_, a)| matches!(a, ignition_core::Attribute::ColorAdd { .. })),
            "no colour reached the rig: {:?}",
            out.keys()
                .map(|(_, a)| format!("{a:?}"))
                .collect::<std::collections::BTreeSet<_>>()
        );
    }
}
