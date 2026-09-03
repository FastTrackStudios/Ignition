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

// r[impl files.venue] - smoke check only: reads fixtures.json and screens.json from the venue directory
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "data/venues/norco/fixtures.json";
    let raw = fs::read_to_string(path)?;
    let fixtures: Vec<FixtureRecord> = serde_json::from_str(&raw)?;

    let mut by_tag: BTreeMap<String, u32> = BTreeMap::new();
    for f in &fixtures {
        let key = if f.tags.is_empty() {
            "(untagged)".to_string()
        } else {
            f.tags.join(",")
        };
        by_tag
            .entry(key)
            .and_modify(|c| *c = c.saturating_add(1))
            .or_insert(1);
    }

    let unpatched: Vec<u32> = fixtures
        .iter()
        .filter(|f| !f.patched)
        .map(|f| f.chan)
        .collect();

    let patched_count = fixtures.len().saturating_sub(unpatched.len());
    println!(
        "Norco venue extract: {} channels ({} patched, {} unpatched: {unpatched:?})",
        fixtures.len(),
        patched_count,
        unpatched.len()
    );
    for (tag, count) in &by_tag {
        println!("  {count:>3}  {tag}");
    }

    let screens: Value =
        serde_json::from_str(&fs::read_to_string("data/venues/norco/screens.json")?)?;
    println!(
        "Norco venue extract: {} mappable screens (TVs)",
        screens.as_array().map_or(0, Vec::len)
    );
    Ok(())
}
