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
    let dir = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/gdtf/generated"
    ));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return; // nothing generated yet
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("gdtf") {
            continue;
        }
        let file = File::open(&path).expect("open generated gdtf");
        let parsed =
            GdtfFile::new(file).unwrap_or_else(|e| panic!("{}: parse failed: {e}", path.display()));
        let fixture_type = parsed
            .description
            .fixture_types
            .first()
            .unwrap_or_else(|| panic!("{}: no fixture type", path.display()));
        let name = fixture_type
            .name
            .as_deref()
            .map(std::string::ToString::to_string);
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
            let mode_name = mode
                .name
                .as_deref()
                .map(std::string::ToString::to_string)
                .unwrap_or_default();
            let imported = import_channel_map(&path, Some(&mode_name)).unwrap_or_else(|e| {
                panic!(
                    "{}: mode {mode_name:?} failed to import: {e}",
                    path.display()
                )
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

/// The assembled fixture — every part at the file's own placement, every
/// joint at rest — is the size the spec's listing says the product is.
/// The generator used to fit the spec's box onto the *sum* of the parts'
/// heights, though a head sits inside its yoke and the yoke inside the
/// base's drop, which shrank every mini mover to two thirds of itself
/// and squashed a metre-long bar to six centimetres.
// r[verify viz.gdtf-generated] - a generated profile is its listing's size when assembled
#[test]
fn generated_profiles_assemble_to_their_specs_physical_size() {
    use ignition_viz::gdtf_geometry::import_geometry;
    let dir = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/gdtf/generated"
    ));
    // (file, spec width/length/height in metres — see data/fixtures).
    // Profiles without a pigtail: the cable stub is left out of the
    // listing's dimensions and of the generator's fit, but
    // `assembled_bounds` draws everything.
    let pinned: [(&str, [f32; 3]); 2] = [
        (
            "ZKYMZL@Mini_Gobo_Moving_Head_Light_11ch@ignition.gdtf",
            [0.159, 0.146, 0.247],
        ),
        (
            "Rockville@RockStrip_252@ignition.gdtf",
            [1.020, 0.067, 0.065],
        ),
    ];
    for (file, spec) in pinned {
        let path = dir.join(file);
        if !path.exists() {
            continue;
        }
        let fixture = import_geometry(&path, None).expect("imports");
        let (lo, hi) = fixture.root.assembled_bounds().expect("draws something");
        let extent = hi - lo;
        // The listing's width/length are not axis-tagged: the longer
        // goes on the fixture's longer horizontal axis.
        let mut horizontal = [extent.x, extent.y];
        horizontal.sort_by(|a, b| b.total_cmp(a));
        let mut want = [spec[0], spec[1]];
        want.sort_by(|a, b| b.total_cmp(a));
        for (got, want) in horizontal.iter().zip(want) {
            assert!(
                (got - want).abs() < 0.003,
                "{file}: horizontal extent {got} should be {want} (extent {extent:?})"
            );
        }
        assert!(
            (extent.z - spec[2]).abs() < 0.003,
            "{file}: height {} should be {} (extent {extent:?})",
            extent.z,
            spec[2]
        );
    }
}

/// The TY-30's one emitter sits on its head's lens face and fires out of
/// it — not floating below the head where the unscaled placement left
/// it once the parts shrank.
// r[verify viz.emitter-at-beam-node] - a mover's emitter is on its lens
#[test]
fn the_mini_gobo_movers_emitter_is_on_the_head() {
    use ignition_viz::gdtf_geometry::import_geometry;
    let path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/gdtf/generated/ZKYMZL@Mini_Gobo_Moving_Head_Light_11ch@ignition.gdtf"
    ));
    if !path.exists() {
        return;
    }
    let fixture = import_geometry(&path, None).expect("imports");
    let beams = fixture.root.beam_poses();
    assert_eq!(beams.len(), 1, "one <Beam> node, so one emitter");
    let (pos, rot) = beams[0];
    let (lo, _) = fixture.root.assembled_bounds().unwrap();
    assert!(
        (pos.z - lo.z).abs() < 0.01,
        "emitter z {} sits on the lens face z {}",
        pos.z,
        lo.z
    );
    assert!(
        (rot * bevy::math::Vec3::NEG_Z).z < -0.99,
        "fires straight out of the lens at rest"
    );
}
