//! The default profile, checked against a real venue's real binding.
//!
//! A unit test can only prove the machinery works on data written to
//! make it work. This proves the thing that actually matters: that the
//! profile Ignition ships is implementable by the rig Ignition was built
//! against, using names Norco already had.
//!
//! It is also the test that would have caught the profile being an
//! aspiration. When first written, Norco failed every required role — it
//! owned a key light and called it `Front Wash`, and nothing said so.

use ignition_core::profile::{Profile, VenueProfile};

fn load<T: serde::de::DeserializeOwned>(path: &str) -> Option<T> {
    // The data lives in the repo, not the crate, so a runner without it
    // skips rather than fails.
    let text = std::fs::read_to_string(format!("../../{path}")).ok()?;
    serde_json::from_str(&text).ok()
}

#[test]
fn norco_implements_the_default_profile() {
    let Some(profile): Option<Profile> = load("data/profiles/ignition.ig-profile") else {
        return;
    };
    let Some(venue): Option<VenueProfile> = load("data/venues/norco/profile.json") else {
        return;
    };

    assert_eq!(venue.profile, profile.name);

    let gaps = profile.gaps(&venue);
    let blocking: Vec<String> = gaps
        .iter()
        .filter(|g| g.required)
        .map(|g| g.to_string())
        .collect();
    assert!(
        blocking.is_empty(),
        "Norco does not implement {}:\n  {}",
        profile.name,
        blocking.join("\n  ")
    );
    assert!(profile.satisfied_by(&venue));
}

/// Every name the binding points at must exist at the venue.
///
/// The check above proves Norco *claims* every role. This proves the
/// claims are true — a binding naming a group the venue does not have
/// would satisfy the profile and light nothing, which is a worse failure
/// than an honest gap because it passes.
#[test]
fn every_binding_points_at_something_real() {
    let Some(venue): Option<VenueProfile> = load("data/venues/norco/profile.json") else {
        return;
    };
    let Some(groups): Option<Vec<serde_json::Value>> = load("data/venues/norco/groups.json") else {
        return;
    };
    let Some(palettes): Option<serde_json::Value> = load("data/venues/norco/palettes.json") else {
        return;
    };

    let names: Vec<&str> = groups
        .iter()
        .filter_map(|g| g.get("label")?.as_str())
        .collect();
    // Only the plain `Group("...")` bindings can be checked this way;
    // an expression is checked by resolving it, which needs a rig.
    for (role, selection) in &venue.groups {
        if let ignition_core::Selection::Group(name) = selection {
            assert!(
                names.contains(&name.as_str()),
                "role {role:?} binds to group {name:?}, which Norco does not have"
            );
        }
    }

    let focus: Vec<&str> = palettes
        .get("focus")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|f| f.get("name")?.as_str()).collect())
        .unwrap_or_default();
    for (role, name) in &venue.focus {
        assert!(
            focus.contains(&name.as_str()),
            "role {role:?} binds to focus point {name:?}, which Norco does not have"
        );
    }
}

/// The venue's own blocking grid points at focus points it really has.
///
/// Areas are venue-owned rather than profile-declared — how many a stage
/// has is a property of the stage — so nothing above checks them, and
/// they are the most likely place for a name that merely *looks* right:
/// half of Norco's areas resolve to focus points called `Vocal ...`
/// rather than `Downstage ...`.
#[test]
fn norcos_areas_point_at_real_focus_points() {
    let Some(areas): Option<serde_json::Value> = load("data/venues/norco/areas.json") else {
        return;
    };
    let Some(palettes): Option<serde_json::Value> = load("data/venues/norco/palettes.json") else {
        return;
    };
    let focus: Vec<&str> = palettes
        .get("focus")
        .and_then(|f| f.as_array())
        .map(|a| a.iter().filter_map(|f| f.get("name")?.as_str()).collect())
        .unwrap_or_default();

    let grid = areas
        .get("areas")
        .and_then(|a| a.as_object())
        .expect("areas.json has an areas map");
    assert!(!grid.is_empty(), "Norco declares no areas");
    for (area, target) in grid {
        let target = target.as_str().unwrap_or_default();
        assert!(
            focus.contains(&target),
            "area {area:?} points at focus point {target:?}, which Norco does not have"
        );
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
    assert!(focus.contains(&"Vocal"), "no portable way to find the talent");
}
