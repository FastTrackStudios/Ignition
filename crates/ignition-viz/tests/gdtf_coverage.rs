//! Every fixture patched in every venue under `data/venues/` resolves to
//! a GDTF profile in the workspace's own library (`data/gdtf` plus
//! `data/gdtf/generated`, via `aliases.json` where the console's model
//! string is not the profile's name), and that profile has something real
//! to draw.

use ignition_viz::gdtf_geometry::{GdtfLibrary, GdtfNode, GdtfShape};
use ignition_viz::venue::Venue;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// A decoded 3DS/primitive mesh, or a GLB `bevy_gltf` will load — both
/// are the profile's real geometry.
fn meshes(node: &GdtfNode) -> usize {
    let own = usize::from(matches!(
        node.shape,
        GdtfShape::Mesh(_) | GdtfShape::Gltf { .. }
    ));
    own.saturating_add(node.children.iter().map(meshes).sum::<usize>())
}

/// r[verify viz.gdtf-meshes] - every venue fixture has a real mesh to draw
/// r[verify viz.gdtf-aliases] - exact name or alias covers every venue model
#[test]
fn every_venue_fixture_resolves_to_a_profile_with_a_real_mesh() {
    let venues = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/venues"));
    let library = GdtfLibrary::load_default();
    assert!(
        !library.is_empty(),
        "no GDTF profiles loaded from data/gdtf"
    );

    let mut models = BTreeSet::new();
    for entry in std::fs::read_dir(&venues).expect("data/venues").flatten() {
        if !entry.path().join("fixtures.json").is_file() {
            continue;
        }
        let venue =
            Venue::load(entry.path()).unwrap_or_else(|e| panic!("{}: {e}", entry.path().display()));
        for f in &venue.fixtures {
            let Some(model) = f.model.clone() else {
                continue;
            };
            models.insert((f.manufacturer.clone().unwrap_or_default(), model));
        }
    }
    assert!(
        !models.is_empty(),
        "no venues found under {}",
        venues.display()
    );

    let mut failures = Vec::new();
    for (manufacturer, model) in &models {
        match library.find(manufacturer, model) {
            None => failures.push(format!("{manufacturer:?} {model:?}: no profile")),
            Some(fixture) if meshes(&fixture.root) == 0 => failures.push(format!(
                "{manufacturer:?} {model:?} -> {:?}: no real mesh",
                fixture.fixture_type_name
            )),
            Some(fixture) => println!(
                "{manufacturer:?} {model:?} -> {:?}",
                fixture.fixture_type_name
            ),
        }
    }
    assert!(
        failures.is_empty(),
        "unresolved fixtures:\n{}",
        failures.join("\n")
    );
}

/// r[verify viz.gdtf-aliases] - a generated profile beats a downloaded one of the same name
#[test]
fn generated_profile_wins_over_downloaded_of_same_name() {
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gdtf"));
    let top = GdtfLibrary::load_dir(&dir).expect("load data/gdtf");
    let generated = GdtfLibrary::load_dir(&dir.join("generated")).expect("load generated");
    assert!(top.len() >= generated.len());
    // Every generated name resolves in the merged library to the
    // generated file's own (first) DMX mode name.
    for name in [
        "150W LED Beam Moving Head Light",
        "RockStrip 252",
        "Uking Par",
    ] {
        let g = generated.find("", name).expect(name);
        let m = top.find("", name).expect(name);
        assert_eq!(g.dmx_mode_name, m.dmx_mode_name, "{name}");
    }
}
