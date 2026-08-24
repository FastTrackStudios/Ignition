//! Placeholder engine binary. Right now it does exactly one thing: prove the
//! Norco venue extract in `data/venues/norco/` parses, as the first checkpoint
//! toward the Phase 0 spike in
//! `docs/research/lighting-console-landscape.md` §9.
//!
//! Run from the repo root: `cargo run -p ignition-engine`.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug, Deserialize)]
struct FixtureRecord {
    chan: u32,
    tags: Vec<String>,
    patched: bool,
}

fn main() {
    let path = "data/venues/norco/fixtures.json";
    let raw = fs::read_to_string(path).unwrap_or_else(|e| {
        panic!("expected to run from the repo root; couldn't read {path}: {e}")
    });
    let fixtures: Vec<FixtureRecord> = serde_json::from_str(&raw).expect("valid fixtures.json");

    let mut by_tag: BTreeMap<String, u32> = BTreeMap::new();
    for f in &fixtures {
        let key = if f.tags.is_empty() {
            "(untagged)".to_string()
        } else {
            f.tags.join(",")
        };
        *by_tag.entry(key).or_default() += 1;
    }

    let unpatched: Vec<u32> = fixtures.iter().filter(|f| !f.patched).map(|f| f.chan).collect();

    println!(
        "Norco venue extract: {} channels ({} patched, {} unpatched: {unpatched:?})",
        fixtures.len(),
        fixtures.len() - unpatched.len(),
        unpatched.len()
    );
    for (tag, count) in &by_tag {
        println!("  {count:>3}  {tag}");
    }

    let screens: Value = serde_json::from_str(
        &fs::read_to_string("data/venues/norco/screens.json").expect("screens.json"),
    )
    .expect("valid screens.json");
    println!(
        "Norco venue extract: {} mappable screens (TVs)",
        screens.as_array().map(|a| a.len()).unwrap_or(0)
    );
}
