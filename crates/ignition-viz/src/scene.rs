use crate::fixture_profile::add_typed_fixture;
use crate::mesh::MeshBuilder;
use crate::venue::Venue;

const ROOM_COLOR: [f32; 3] = [0.42, 0.44, 0.48];
const FLOOR_COLOR: [f32; 3] = [0.30, 0.31, 0.34];
const SCREEN_COLOR: [f32; 3] = [0.20, 0.45, 0.85];
// Was [0.55, 0.40, 0.28] — read as orange next to the fixture markers'
// own orange (Mover) colour, which is exactly the mix-up reported: props
// and moving heads were hard to tell apart at a glance. Desaturated and
// cooled toward neutral grey-brown.
const PROP_COLOR: [f32; 3] = [0.42, 0.38, 0.34];

/// Objects whose `name` contains any of these substrings are left out of the
/// scene — e.g. `["Ceiling"]` for a top-down plan view that would otherwise
/// just render the roof.
pub fn build_scene(venue: &Venue, exclude: &[String]) -> MeshBuilder {
    let mut mesh = MeshBuilder::default();
    let skip = |name: &str| exclude.iter().any(|e| name.contains(e.as_str()));

    for g in &venue.room {
        if skip(&g.name) {
            continue;
        }
        let is_floor_like = g.name.starts_with("Floor")
            || g.name.starts_with("Stage")
            || g.name == "Ceiling";
        let color = if is_floor_like { FLOOR_COLOR } else { ROOM_COLOR };
        mesh.add_box(g.position.to_glam(), g.orientation(), g.size.to_glam(), color);
    }

    for g in &venue.screens {
        if skip(&g.name) {
            continue;
        }
        mesh.add_quad(g.position.to_glam(), g.orientation(), g.size.to_glam(), SCREEN_COLOR);
        // A thin backing box so the screen reads as an object from any angle,
        // not just face-on.
        let backing_size = glam::Vec3::new(g.size.x, g.size.y, 0.05);
        mesh.add_box(g.position.to_glam(), g.orientation(), backing_size, [0.08, 0.08, 0.10]);
    }

    for g in &venue.props {
        // People and pillars have no dedicated shape yet — as plain AABB
        // boxes they're tall, undifferentiated, and easy to mistake for
        // fixture markers (a standing person's bounding box is just a
        // "tall box" with no readable silhouette). Hidden unconditionally
        // for now rather than left in looking wrong; bring back once
        // there's a real human/architectural model to draw instead.
        let is_placeholder_only = g.name.starts_with("Person") || g.name.starts_with("Pillar");
        if skip(&g.name) || is_placeholder_only {
            continue;
        }
        mesh.add_box(g.position.to_glam(), g.orientation(), g.size.to_glam(), PROP_COLOR);
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
