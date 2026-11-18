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

//! The gate that has to be green before `channel_map.rs` can be deleted.
//!
//! That table is DMX truth today, and a model with no row in it is a
//! model with no channel map, which is a fixture that never lights —
//! twenty-four of Room 138's forty went dark for exactly that reason.
//! Replacing it with a directory of documents is only safe if the
//! directory covers at least as much, so this test asks the two
//! questions the table answered:
//!
//! 1. does every fixture patched in every shipped venue resolve to a
//!    fixture type at all, and
//! 2. does that type have a mode whose footprint fits the room's own
//!    address spacing?
//!
//! The second is the one that matters and the one nobody can fake: the
//! gap between two consecutive fixtures in a real patch is a measured
//! fact about the rig, and a footprint wider than the gap means two
//! fixtures share bytes.
//!
//! Failures accumulate and are reported together. One run should tell
//! you every fixture that needs a document, not the first one.

use ignition_fixture::Library;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every venue directory that has a `fixtures.json`, the way
/// `ignition-viz`'s own coverage test walks them — so a venue added
/// later is checked without anyone remembering to add it here.
fn venues() -> Vec<(String, Vec<Value>)> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(repo().join("data/venues")) else {
        return out;
    };
    let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for dir in dirs {
        let path = dir.join("fixtures.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(Value::Array(fixtures)) = serde_json::from_str::<Value>(&text) else {
            panic!("{} is not an array of fixtures", path.display());
        };
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push((name, fixtures));
    }
    out
}

fn string(fixture: &Value, key: &str) -> String {
    fixture
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn number(fixture: &Value, key: &str) -> Option<u32> {
    fixture.get(key).and_then(Value::as_u64).map(|n| n as u32)
}

/// r[verify files.venue.fixtures] - every patched fixture has a type the
/// library knows.
#[test]
fn every_venue_fixture_resolves_to_a_fixture_type() {
    let library = Library::load_default();
    assert!(
        !library.types().is_empty(),
        "the fixture library is empty — data/fixtures did not load from {}",
        repo().display()
    );
    assert!(
        library.rejected().is_empty(),
        "documents that would not load: {:#?}",
        library.rejected()
    );

    let mut missing: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (venue, fixtures) in venues() {
        for fixture in &fixtures {
            // An unpatched fixture is a prop or a spare; it has no bytes
            // and needs no channel map.
            if fixture.get("patched").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let manufacturer = string(fixture, "manufacturer");
            let model = string(fixture, "model");
            if model.is_empty() {
                continue;
            }
            let found = library
                .find(&manufacturer, &model)
                .filter(|doc| !doc.modes.is_empty());
            if found.is_none() {
                missing
                    .entry(format!("{manufacturer} / {model}"))
                    .or_default()
                    .push(venue.clone());
            }
        }
    }

    assert!(
        missing.is_empty(),
        "these patched models have no fixture type with a mode in data/fixtures.\n\
         Each one is a fixture that would not light. Write the document, or add \
         an alias to data/gdtf/aliases.json.\n{missing:#?}"
    );
}

/// r[verify files.venue.fixtures] - and its footprint fits the room.
///
/// The venue never records which *mode* a fixture is in. The room does,
/// implicitly, in the gap it left between one fixture's address and the
/// next: a rig patched at 1, 8, 15 is telling you those fixtures are
/// seven channels wide. A type with no mode narrow enough to fit that
/// gap is a type that would overrun its neighbour.
#[test]
fn some_mode_of_every_type_fits_the_gap_the_room_left() {
    let library = Library::load_default();
    let mut problems: Vec<String> = Vec::new();

    for (venue, fixtures) in venues() {
        // Address spacing is only meaningful within one universe.
        let mut by_universe: BTreeMap<u32, Vec<(u32, String, String, u32)>> = BTreeMap::new();
        for fixture in &fixtures {
            if fixture.get("patched").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let (Some(universe), Some(address)) =
                (number(fixture, "universe"), number(fixture, "address"))
            else {
                continue;
            };
            by_universe.entry(universe).or_default().push((
                address,
                string(fixture, "manufacturer"),
                string(fixture, "model"),
                number(fixture, "chan").unwrap_or(0),
            ));
        }

        for (universe, mut patched) in by_universe {
            patched.sort_by_key(|(address, _, _, _)| *address);
            for window in patched.windows(2) {
                let [(address, manufacturer, model, chan), (next, _, _, _)] = window else {
                    continue;
                };
                // Two fixtures on the same address is multipatch, not a
                // spacing claim — r[files.venue.multipatch].
                let gap = next.saturating_sub(*address);
                if gap == 0 {
                    continue;
                }
                let Some(doc) = library.find(manufacturer, model) else {
                    continue; // the test above is what reports this
                };
                // Not "some mode could fit" but "the mode actually
                // chosen fits" — the resolution the engine will really
                // do, model string first and the gap second.
                let chosen = doc.mode_for(model, u16::try_from(gap).ok());
                let footprint = chosen.map_or(0, |mode| doc.channel_map(mode).0.footprint);
                let fits = footprint > 0 && u32::from(footprint) <= gap;
                if !fits {
                    let widths: Vec<String> = doc
                        .modes
                        .keys()
                        .map(|mode| format!("{mode}={}", doc.channel_map(mode).0.footprint))
                        .collect();
                    problems.push(format!(
                        "{venue} chan {chan} ({manufacturer} / {model}) at {universe}.{address}: \
                         the room left {gap} channels before {universe}.{next}, but `{}` resolved \
                         to mode {chosen:?} which is {footprint} wide — modes are {}",
                        doc.console_name,
                        widths.join(", ")
                    ));
                }
            }
        }
    }

    assert!(
        problems.is_empty(),
        "a fixture type whose narrowest mode overruns the next fixture's address \
         would bleed into it on the wire:\n{}",
        problems.join("\n")
    );
}

/// Every document in the library says something addressable.
///
/// A document with modes but no readable channel in any of them is
/// research that has not landed yet, and it is better to know that here
/// than at the rig.
#[test]
fn every_document_yields_a_channel_map() {
    let library = Library::load_default();
    let mut problems: Vec<String> = Vec::new();
    for doc in library.types() {
        if doc.modes.is_empty() {
            problems.push(format!("{}: no modes at all", doc.console_name));
            continue;
        }
        for mode in doc.modes.keys() {
            let (map, complaints) = doc.channel_map(mode);
            if map.footprint == 0 {
                problems.push(format!("{} mode {mode}: footprint 0", doc.console_name));
            }
            for complaint in complaints {
                problems.push(format!(
                    "{} mode {} channel {}: {}",
                    doc.console_name, complaint.mode, complaint.channel, complaint.message
                ));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
