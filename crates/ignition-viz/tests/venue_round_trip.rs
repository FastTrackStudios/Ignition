// Integration test: `clippy.toml`'s test allowances only reach
// `#[cfg(test)]` modules, so the panic set is lifted here instead.
// See docs/ops/clippy.md.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "integration test — see docs/ops/clippy.md"
)]

//! Editing a venue must not cost it anything.
//!
//! The patch page writes `fixtures.json` back, and the failure that
//! matters is not a crash — it is a save that silently drops a field.
//! A venue edited by a build that predates some key must come out with
//! that key intact (`r[files.additive-evolution]`), an address must move
//! all three of the fields an address is made of, and a re-hang must
//! write both spellings of the orientation or the loader will ignore it.

use ignition_viz::venue::Venue;
use serde_json::Value;

fn norco() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/venues/norco")
}

/// Copy the shipped venue somewhere writable.
fn scratch(name: &str) -> Option<std::path::PathBuf> {
    let source = norco();
    if !source.join("fixtures.json").exists() {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("ignition-venue-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    for entry in std::fs::read_dir(&source).ok()?.flatten() {
        let path = entry.path();
        if path.is_file() {
            let _ = std::fs::copy(&path, dir.join(entry.file_name()));
        }
    }
    Some(dir)
}

fn fixtures_json(dir: &std::path::Path) -> Vec<Value> {
    let raw = std::fs::read_to_string(dir.join("fixtures.json")).expect("fixtures.json");
    serde_json::from_str(&raw).expect("an array of fixtures")
}

/// r[verify files.additive-evolution] - a save keeps what it did not understand
#[test]
fn saving_a_venue_keeps_every_key_it_did_not_understand() {
    let Some(dir) = scratch("keys") else {
        return;
    };
    // Add a key no build knows about, the way a newer build would.
    let mut raw = fixtures_json(&dir);
    raw[0]["invented_later"] = serde_json::json!({"by": "a newer build"});
    std::fs::write(
        dir.join("fixtures.json"),
        serde_json::to_string_pretty(&raw).unwrap(),
    )
    .unwrap();

    let venue = Venue::load(&dir).expect("the venue loads");
    venue.save_fixtures(&dir).expect("and saves");

    let after = fixtures_json(&dir);
    assert_eq!(
        after[0].get("invented_later"),
        Some(&serde_json::json!({"by": "a newer build"})),
        "a key this build never modelled was dropped by a save"
    );
    // And the keys it does model are all still there.
    for key in ["chan", "name", "tags", "position", "eulers", "quat", "size"] {
        assert!(after[0].get(key).is_some(), "{key} went missing");
    }
}

/// r[verify patch.writes-the-venue] - load, save, load is the same venue
#[test]
fn a_venue_round_trips_through_a_save_unchanged() {
    let Some(dir) = scratch("round-trip") else {
        return;
    };
    let before = Venue::load(&dir).expect("the venue loads");
    before.save_fixtures(&dir).expect("and saves");
    let after = Venue::load(&dir).expect("and loads again");
    assert_eq!(
        before.fixtures, after.fixtures,
        "a save that changes nothing changed something"
    );
}

/// r[verify patch.writes-the-venue] - an address is three fields
#[test]
fn moving_a_fixture_moves_its_global_address_too() {
    let Some(dir) = scratch("address") else {
        return;
    };
    let mut venue = Venue::load(&dir).expect("the venue loads");
    let chan = venue.fixtures[0].chan;
    venue.fixtures[0].set_address(Some(ignition_proto::DmxAddress {
        universe: 3,
        start_channel: 17,
    }));
    venue.save_fixtures(&dir).expect("saves");

    let raw = fixtures_json(&dir);
    let moved = raw
        .iter()
        .find(|f| f.get("chan").and_then(Value::as_u64) == chan.map(u64::from))
        .expect("the fixture is still in the file");
    assert_eq!(moved["universe"], 3);
    assert_eq!(moved["address"], 17);
    // (3 - 1) * 512 + 17. A stale value here is a file that disagrees
    // with itself, and nothing would notice until something read the
    // wrong one.
    assert_eq!(moved["global_address"], 1041);
}

/// r[verify patch.unpatched] - unpatching keeps the fixture
#[test]
fn unpatching_takes_the_address_and_leaves_the_fixture() {
    let Some(dir) = scratch("unpatch") else {
        return;
    };
    let mut venue = Venue::load(&dir).expect("the venue loads");
    let before = venue.fixtures.len();
    let name = venue.fixtures[0].name.clone();
    venue.fixtures[0].set_address(None);
    venue.save_fixtures(&dir).expect("saves");

    let venue = Venue::load(&dir).expect("loads again");
    assert_eq!(venue.fixtures.len(), before, "a fixture was deleted");
    let fixture = &venue.fixtures[0];
    assert_eq!(fixture.name, name, "and it is still the same one");
    assert!(!fixture.patched);
    assert!(fixture.dmx_address().is_none());
    // Still in the room, still in groups, still selectable.
    assert!(venue.patch().get(0).is_none(), "and it has no bytes");
}

/// r[verify patch.orientation-is-whole] - both spellings, or the loader
/// ignores the edit.
#[test]
fn re_hanging_a_fixture_writes_the_angles_and_the_quaternion() {
    let Some(dir) = scratch("orientation") else {
        return;
    };
    let mut venue = Venue::load(&dir).expect("the venue loads");
    let turned = bevy::math::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    venue.fixtures[0].set_orientation(turned);
    venue.save_fixtures(&dir).expect("saves");

    let raw = fixtures_json(&dir);
    let eulers = &raw[0]["eulers"];
    // Ninety degrees about Z, in both places. If only the quaternion
    // moved, the file would read as a fixture hung one way and drawn
    // another; if only the angles moved, `orientation()` would ignore
    // the edit entirely, which is the bug `aimwash.rs` documents.
    assert!(
        (eulers["z"].as_f64().unwrap() - 90.0).abs() < 0.01,
        "the angles did not follow the quaternion: {eulers}"
    );

    let venue = Venue::load(&dir).expect("loads again");
    let back = venue.fixtures[0].orientation();
    assert!(
        back.angle_between(turned) < 0.001,
        "the hang did not survive the round trip"
    );
}
