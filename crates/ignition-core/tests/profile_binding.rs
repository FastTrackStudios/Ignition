//! The default profile, checked against every real venue's real binding.
//!
//! A unit test can only prove the machinery works on data written to
//! make it work. This proves the thing that actually matters: that the
//! profile Ignition ships is implementable by the rigs Ignition is
//! actually run on, using names those rooms already had.
//!
//! It is also the test that would have caught the profile being an
//! aspiration. When first written, Norco failed every required role — it
//! owned a key light and called it `Front Wash`, and nothing said so.
//!
//! Every check here runs over **every** directory under `data/venues/`
//! rather than over Norco alone. A second room is exactly when a
//! hand-listed venue stops being maintained, and a venue that is not
//! checked is a venue whose binding is an aspiration again.

use ignition_core::profile::{Profile, VenueProfile};

fn load<T: serde::de::DeserializeOwned>(path: &str) -> Option<T> {
    // The data lives in the repo, not the crate, so a runner without it
    // skips rather than fails.
    let text = std::fs::read_to_string(format!("../../{path}")).ok()?;
    serde_json::from_str(&text).ok()
}

/// Every venue directory that has been bound to a profile.
///
/// A venue with no `profile.json` is a room somebody is still setting
/// up, not a failure — `Venue::load` treats it the same way — so it is
/// skipped rather than failed.
fn bound_venues() -> Vec<(String, VenueProfile)> {
    let Ok(dir) = std::fs::read_dir("../../data/venues") else {
        return Vec::new();
    };
    let mut out: Vec<(String, VenueProfile)> = dir
        .filter_map(|e| {
            let name = e.ok()?.file_name().to_string_lossy().into_owned();
            let venue: VenueProfile = load(&format!("data/venues/{name}/profile.json"))?;
            Some((name, venue))
        })
        .collect();
    // Stable order, so a failure names the same venue every run.
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The `label`s in a venue's `groups.json`.
fn group_names(venue: &str) -> Vec<String> {
    let groups: Vec<serde_json::Value> =
        load(&format!("data/venues/{venue}/groups.json")).unwrap_or_default();
    groups
        .iter()
        .filter_map(|g| Some(g.get("label")?.as_str()?.to_string()))
        .collect()
}

/// The `name`s in a venue's `palettes.json` focus list.
fn focus_names(venue: &str) -> Vec<String> {
    let palettes: serde_json::Value =
        load(&format!("data/venues/{venue}/palettes.json")).unwrap_or_default();
    palettes
        .get("focus")
        .and_then(|f| f.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|f| Some(f.get("name")?.as_str()?.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// r[verify default.norco-is-the-proof]
#[test]
fn every_venue_implements_the_default_profile() {
    let Some(profile): Option<Profile> = load("data/profiles/ignition.ig-profile") else {
        return;
    };
    let venues = bound_venues();
    assert!(!venues.is_empty(), "no venue is bound to a profile at all");

    for (name, venue) in &venues {
        assert_eq!(
            venue.profile, profile.name,
            "{name} claims profile {:?}, not {:?}",
            venue.profile, profile.name
        );
        let blocking: Vec<String> = profile
            .gaps(venue)
            .iter()
            .filter(|g| g.required)
            .map(|g| g.to_string())
            .collect();
        assert!(
            blocking.is_empty(),
            "{name} does not implement {}:\n  {}",
            profile.name,
            blocking.join("\n  ")
        );
        assert!(
            profile.satisfied_by(venue),
            "{name} does not satisfy the profile"
        );
    }
}

/// Every name a binding points at must exist at the venue it belongs to.
///
/// The check above proves each room *claims* every role. This proves the
/// claims are true — a binding naming a group the venue does not have
/// would satisfy the profile and light nothing, which is a worse failure
/// than an honest gap because it passes.
#[test]
fn every_binding_points_at_something_real() {
    for (name, venue) in bound_venues() {
        let groups = group_names(&name);
        // Only the plain `Group("...")` bindings can be checked this way;
        // an expression is checked by resolving it, which needs a rig.
        for (role, selection) in &venue.groups {
            if let ignition_core::Selection::Group(group) = selection {
                assert!(
                    groups.contains(group),
                    "at {name}, role {role:?} binds to group {group:?}, \
                     which that venue does not have"
                );
            }
        }

        let focus = focus_names(&name);
        for (role, point) in &venue.focus {
            assert!(
                focus.contains(point),
                "at {name}, role {role:?} binds to focus point {point:?}, \
                 which that venue does not have"
            );
        }
    }
}

/// Each venue's own blocking grid points at focus points it really has.
///
/// Areas are venue-owned rather than profile-declared — how many a stage
/// has is a property of the stage — so nothing above checks them, and
/// they are the most likely place for a name that merely *looks* right:
/// half of Norco's areas resolve to focus points called `Vocal ...`
/// rather than `Downstage ...`.
#[test]
fn every_venues_areas_point_at_real_focus_points() {
    for (name, _) in bound_venues() {
        let Some(areas): Option<serde_json::Value> =
            load(&format!("data/venues/{name}/areas.json"))
        else {
            // A room with no blocking grid can still be programmed
            // through the profile's focus roles.
            continue;
        };
        let focus = focus_names(&name);
        let grid = areas
            .get("areas")
            .and_then(|a| a.as_object())
            .unwrap_or_else(|| panic!("{name}: areas.json has no areas map"));
        assert!(!grid.is_empty(), "{name} declares an empty blocking grid");
        for (area, target) in grid {
            let target = target.as_str().unwrap_or_default();
            assert!(
                focus.iter().any(|f| f == target),
                "at {name}, area {area:?} points at focus point {target:?}, \
                 which that venue does not have"
            );
        }
    }
}

/// The default profile declares no areas, and that is the design rather
/// than an omission: a club has three and a stadium has fifteen, so a
/// central list either excludes the large room or burdens the small one.
#[test]
fn the_default_profile_leaves_areas_to_the_venue() {
    let Some(profile): Option<Profile> = load("data/profiles/ignition.ig-profile") else {
        return;
    };
    assert!(
        profile.vocabulary(ignition_core::RoleKind::Area).is_empty(),
        "the default profile declared areas; the venue owns those"
    );
    // ...and the portable question is still answerable, through focus.
    let focus = profile.vocabulary(ignition_core::RoleKind::Focus);
    assert!(
        focus.contains(&"Vocal"),
        "no portable way to find the talent"
    );
}
