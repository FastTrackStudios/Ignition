//! Every profile `tools/make_gdtf.py` wrote into `data/gdtf/generated`
//! opens with the same parser the visualizer uses, names its fixture
//! type, and imports a channel map for every mode it declares.

use gdtf::GdtfFile;
use ignition_viz::gdtf_import::import_channel_map;
use std::fs::File;
use std::path::PathBuf;

/// r[verify viz.gdtf-generated] - generated profiles import like real ones
#[test]
fn every_generated_profile_imports_in_every_mode() {
    let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/gdtf/generated"));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return; // nothing generated yet
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("gdtf") {
            continue;
        }
        let file = File::open(&path).expect("open generated gdtf");
        let parsed = GdtfFile::new(file)
            .unwrap_or_else(|e| panic!("{}: parse failed: {e}", path.display()));
        let fixture_type = parsed
            .description
            .fixture_types
            .first()
            .unwrap_or_else(|| panic!("{}: no fixture type", path.display()));
        let name = fixture_type.name.as_deref().map(|n| n.to_string());
        assert!(
            name.as_deref().is_some_and(|n| !n.trim().is_empty()),
            "{}: fixture type has no name",
            path.display()
        );
        assert!(
            !fixture_type.dmx_modes.is_empty(),
            "{}: fixture type declares no DMX modes",
            path.display()
        );
        for mode in &fixture_type.dmx_modes {
            let mode_name = mode.name.as_deref().map(|n| n.to_string()).unwrap_or_default();
            let imported = import_channel_map(&path, Some(&mode_name)).unwrap_or_else(|e| {
                panic!("{}: mode {mode_name:?} failed to import: {e}", path.display())
            });
            assert_eq!(imported.fixture_type_name, name.clone().unwrap());
            assert!(
                imported.channel_map.footprint > 0,
                "{}: mode {mode_name:?} has an empty footprint",
                path.display()
            );
        }
    }
}
