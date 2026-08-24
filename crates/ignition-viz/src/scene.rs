use crate::fixture_profile::add_typed_fixture;
use crate::mesh::MeshBuilder;
use crate::venue::Venue;

const ROOM_COLOR: [f32; 3] = [0.42, 0.44, 0.48];
// Ceiling: black tile, dark grey grid lines (shader.wgsl brightens the
// seams instead of darkening them here — multiplying an already-near-
// black colour darker doesn't show up).
const CEILING_COLOR: [f32; 3] = [0.045, 0.045, 0.05];
// Columns: stonemason-style ashlar block texture (shader.wgsl), warm-toned
// (R > B) so the shader can key stone vs. the cool-toned wall grey without
// a separate material flag. Black cap added as a second, small box on top
// — see the room loop below.
const COLUMN_COLOR: [f32; 3] = [0.58, 0.53, 0.46];
const COLUMN_CAP_COLOR: [f32; 3] = [0.05, 0.05, 0.05];
// Pillars ("the smaller pole beam coming out of" each column): black wood,
// distinct from the stone column and from PROP_COLOR.
const PILLAR_COLOR: [f32; 3] = [0.09, 0.07, 0.06];
// Audience floor: real wood, plank-textured (shader.wgsl). Was on the
// stage floor in the first pass — corrected 2026-08-24: the wood belongs
// to the audience, the stage itself is dark/black plywood.
const FLOOR_COLOR: [f32; 3] = [0.26, 0.17, 0.11];
// Stage floor: black-painted plywood — the shader gives this a coarser
// sheet-seam pattern (4x8ft panels) instead of the audience's narrow
// board planks. Kept a hair above pure black so the sheet seams and grain
// stay visible instead of crushing to flat black.
const STAGE_FLOOR_COLOR: [f32; 3] = [0.055, 0.05, 0.045];
// A flat-black panel — a TV that's off, which is the right default for a
// mapping surface with no content assigned yet (see
// docs/research/projection-mapping-resolume.md).
const SCREEN_COLOR: [f32; 3] = [0.02, 0.02, 0.03];
// Was [0.55, 0.40, 0.28] (orange-brown, read as the same colour as Mover
// fixtures), then [0.42, 0.38, 0.34] (desaturated grey-brown). Purple —
// nothing else in the scene (room/floor grey, screens/wash blue, movers
// orange) is anywhere near it, so props are unambiguous at a glance.
const PROP_COLOR: [f32; 3] = [0.48, 0.30, 0.62];

/// Objects whose `name` contains any of these substrings are left out of the
/// scene — e.g. `["Ceiling"]` for a top-down plan view that would otherwise
/// just render the roof. `show_props` is a separate on/off switch for the
/// whole props layer (drum kit, speakers, mics, ...) — off by default per
/// the operator's 2026-08-24 call: with only placeholder box/purple-tint
/// geometry, props were cluttering every shot without adding information
/// worth the visual noise while the fixture layout itself is still being
/// iterated on. Not deleted — `--show-props` in `shot` brings it back.
pub fn build_scene(venue: &Venue, exclude: &[String], show_props: bool) -> MeshBuilder {
    let mut mesh = MeshBuilder::default();
    let skip = |name: &str| exclude.iter().any(|e| name.contains(e.as_str()));

    for g in &venue.room {
        if skip(&g.name) {
            continue;
        }
        let color = if g.name == "Ceiling" {
            CEILING_COLOR
        } else if g.name.starts_with("Stage") {
            STAGE_FLOOR_COLOR
        } else if g.name.starts_with("Floor") {
            FLOOR_COLOR
        } else if g.name.starts_with("Column") {
            COLUMN_COLOR
        } else {
            ROOM_COLOR
        };
        let pos = g.position.to_glam();
        let rot = g.orientation();
        let size = g.size.to_glam();
        // Walls and risers ("Face - ...") are pivoted at their BASE in the
        // source data, not their centre — confirmed by cross-checking
        // wall heights against the real ceiling height (room.json's
        // Ceiling object): e.g. Wall - Upstage's base z (0.1524) + its
        // height (3.302) lands on 3.4544, the ceiling, to four decimal
        // places, for every wall checked. `add_box` always centres on the
        // point it's given, so shift up by half the height first — left
        // uncorrected, every wall/riser floats too low, leaving a gap at
        // the top (the dark band between wall and ceiling visible in
        // every render before this fix) and the wrong footprint anywhere
        // that gap intersects something else (reported: the flare walls
        // clipping through the TVs mounted on them).
        let center = if g.name.starts_with("Wall") || g.name.starts_with("Face") {
            pos + rot * glam::Vec3::Z * (size.z * 0.5)
        } else {
            pos
        };
        mesh.add_box(center, rot, size, color);
        if g.name.starts_with("Column") {
            // A black capstone on top of the column — a second box rather
            // than trying to two-tone a single add_box call (it only takes
            // one colour for the whole box).
            let cap_height = 0.06;
            let cap_center = center + rot * glam::Vec3::Z * (size.z * 0.5 + cap_height * 0.5);
            let cap_size = glam::Vec3::new(size.x * 1.02, size.y * 1.02, cap_height);
            mesh.add_box(cap_center, rot, cap_size, COLUMN_CAP_COLOR);
        }
    }

    // Pillars ("the smaller pole beam coming out of" each column) are an
    // architectural detail like the columns themselves, not set-dressing —
    // rendered unconditionally, unlike the rest of props.json below.
    for g in venue.props.iter().filter(|g| g.name.starts_with("Pillar")) {
        if skip(&g.name) {
            continue;
        }
        mesh.add_box(g.position.to_glam(), g.orientation(), g.size.to_glam(), PILLAR_COLOR);
    }

    for g in &venue.screens {
        if skip(&g.name) {
            continue;
        }
        let pos = g.position.to_glam();
        let rot = g.orientation();
        let size = g.size.to_glam();
        // Same base-pivot convention as walls (see above) — a TV's
        // position is its bottom edge, not its centre. The quad's local Y
        // is its height axis (add_quad), so shift along the rotated local
        // Y instead of world Z.
        let center = pos + rot * glam::Vec3::Y * (size.y * 0.5);
        mesh.add_quad(center, rot, size, SCREEN_COLOR);
        // A thin backing box so the screen reads as an object from any angle,
        // not just face-on.
        let backing_size = glam::Vec3::new(g.size.x, g.size.y, 0.05);
        mesh.add_box(center, rot, backing_size, [0.08, 0.08, 0.10]);
    }

    if show_props {
        for g in &venue.props {
            // People have no dedicated shape yet — as a plain AABB box a
            // standing person is just a "tall box" with no readable
            // silhouette, easy to mistake for a fixture marker. Hidden
            // even when the rest of the props layer is on; bring back once
            // there's a real human model to draw instead. Pillars are
            // rendered unconditionally above, not here — they're an
            // architectural detail, not set-dressing.
            if skip(&g.name) || g.name.starts_with("Person") || g.name.starts_with("Pillar") {
                continue;
            }
            mesh.add_box(g.position.to_glam(), g.orientation(), g.size.to_glam(), PROP_COLOR);
        }
    }

    for f in &venue.fixtures {
        // Unpatched channels (e.g. Norco's phantom 19/98) have no real
        // position — the live patch reports (0,0,0) for them, which would
        // otherwise render as a stray fixture marker at the room's origin.
        // See docs/domain/norco-patch-and-groups.md.
        if skip(&f.name) || !f.patched {
            continue;
        }
        let manufacturer = f.manufacturer.as_deref().unwrap_or("");
        let model = f.model.as_deref().unwrap_or("");
        add_typed_fixture(
            &mut mesh,
            f.position.to_glam(),
            f.orientation(),
            manufacturer,
            model,
            f.kind().color(),
        );
    }

    mesh
}
