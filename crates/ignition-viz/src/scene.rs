use crate::fixture_profile::add_typed_fixture;
use crate::mesh::MeshBuilder;
use crate::venue::Venue;

const ROOM_COLOR: [f32; 3] = [0.42, 0.44, 0.48];
// Ceiling: light, matching a real acoustic drop-ceiling tile.
const CEILING_COLOR: [f32; 3] = [0.80, 0.80, 0.78];
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
            // People and pillars have no dedicated shape yet — as plain
            // AABB boxes they're tall, undifferentiated, and easy to
            // mistake for fixture markers (a standing person's bounding
            // box is just a "tall box" with no readable silhouette).
            // Hidden even when the rest of the props layer is on; bring
            // back once there's a real human/architectural model to draw
            // instead.
            let is_placeholder_only = g.name.starts_with("Person") || g.name.starts_with("Pillar");
            if skip(&g.name) || is_placeholder_only {
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
