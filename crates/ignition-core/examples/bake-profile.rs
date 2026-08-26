//! Writes the built-in effects library — and the busking programming
//! beside it — into a profile file.
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
//! Only the baked keys are replaced: `effects`, `effect_notes`,
//! `bundles`, and the busking programming — `looks`, `macros`, `pages`,
//! `protected`, `speed_routing`. Everything else in the file is
//! hand-authored and must survive, which is why this rewrites keys
//! rather than serialising a `Profile` back out — a round trip would
//! quietly drop any field the struct does not model yet.

// r[impl effects.library.profile-ships-it] - bakes the Rust library into the shipped profile file
// r[impl effects.bundle] - the bundles ship in the profile beside the library
// r[impl profile.looks] - and the looks
// r[impl profile.macros] - and the macros
// r[impl profile.pages] - and the pages
// r[impl profile.protected-roles] - and the protected roles
// r[impl profile.speed-routing] - and the speed routing
// r[impl files.additive-evolution] - only the baked keys are rewritten, every other field survives
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: bake-profile <profile.ig-profile>")?;
    let text = std::fs::read_to_string(&path)?;
    let mut doc: serde_json::Value = serde_json::from_str(&text)?;

    let library = ignition_core::effects::library();
    let notes = ignition_core::effects::notes();
    let bundles = ignition_core::effects::bundles();
    let busking = ignition_core::macros::shipped();
    let (count, bundled) = (library.len(), bundles.len());
    let (looks, macros, pages) = (
        busking.looks.len(),
        busking.macros.len(),
        busking.pages.len(),
    );
    doc["effects"] = serde_json::to_value(&library)?;
    doc["effect_notes"] = serde_json::to_value(&notes)?;
    doc["bundles"] = serde_json::to_value(&bundles)?;
    doc["looks"] = serde_json::to_value(&busking.looks)?;
    doc["macros"] = serde_json::to_value(&busking.macros)?;
    doc["pages"] = serde_json::to_value(&busking.pages)?;
    doc["protected"] = serde_json::to_value(&busking.protected)?;
    doc["speed_routing"] = serde_json::to_value(&busking.speed_routing)?;

    std::fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&doc)?))?;
    println!(
        "baked {count} effects, their notes, {bundled} bundles, {looks} looks, {macros} macros and {pages} pages into {path}"
    );
    Ok(())
}
