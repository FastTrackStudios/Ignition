//! Writes the built-in effects library into a profile file.
//!
//! The library is authored in Rust — fifteen recipes of nested step
//! tables is a lot of punctuation to get right by eye, and these are
//! worth testing — but it *ships* as data, so a venue or a person can
//! add their own without touching the crate. This is the bridge.
//!
//! ```text
//! cargo run -p ignition-core --example bake-profile -- data/profiles/ignition.ig-profile
//! ```
//!
//! An example rather than a binary because it needs `serde_json`, which
//! `ignition-core` carries as a dev-dependency only: the crate is the
//! pure model and should not gain a JSON dependency to ship a tool. An
//! example gets dev-dependencies and says "tooling, not product" in the
//! same move.
//!
//! Only the `effects`, `effect_notes` and `bundles` maps are replaced.
//! Everything else in the file is hand-authored and must survive, which
//! is why this rewrites three keys rather than serialising a `Profile`
//! back out — a round trip would quietly drop any field the struct does
//! not model yet.

// r[impl effects.library.profile-ships-it] - bakes the Rust library into the shipped profile file
// r[impl effects.bundle] - the bundles ship in the profile beside the library
// r[impl files.additive-evolution] - only the `effects`, `effect_notes` and `bundles` keys are rewritten, every other field survives
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: bake-profile <profile.ig-profile>")?;
    let text = std::fs::read_to_string(&path)?;
    let mut doc: serde_json::Value = serde_json::from_str(&text)?;

    let library = ignition_core::effects::library();
    let notes = ignition_core::effects::notes();
    let bundles = ignition_core::effects::bundles();
    let (count, bundled) = (library.len(), bundles.len());
    doc["effects"] = serde_json::to_value(&library)?;
    doc["effect_notes"] = serde_json::to_value(&notes)?;
    doc["bundles"] = serde_json::to_value(&bundles)?;

    std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&doc)?))?;
    println!("baked {count} effects, their notes and {bundled} bundles into {path}");
    Ok(())
}
