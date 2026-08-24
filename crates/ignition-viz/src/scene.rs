use crate::mesh::MeshBuilder;
use crate::venue::Venue;

const ROOM_COLOR: [f32; 3] = [0.42, 0.44, 0.48];
const FLOOR_COLOR: [f32; 3] = [0.30, 0.31, 0.34];
const SCREEN_COLOR: [f32; 3] = [0.20, 0.45, 0.85];
const PROP_COLOR: [f32; 3] = [0.55, 0.40, 0.28];

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
        if skip(&g.name) {
            continue;
        }
        mesh.add_box(g.position.to_glam(), g.orientation(), g.size.to_glam(), PROP_COLOR);
    }

    for f in &venue.fixtures {
        if skip(&f.name) {
            continue;
        }
        mesh.add_fixture(f.position.to_glam(), f.orientation(), f.kind().color());
    }

    mesh
}
