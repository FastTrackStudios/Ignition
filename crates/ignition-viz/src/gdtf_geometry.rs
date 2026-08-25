//! GDTF 3D Geometry-tree import — real fixture geometry (yoke/head/beam as
//! separate transformable nodes, each with a real primitive shape and
//! dimensions from the manufacturer's own published profile) instead of
//! `fixture_profile.rs`'s single QLC+ placeholder mesh plus a heuristic
//! Z-split (`fixture_profile.rs::MOVING_HEAD_SPLIT_Z` — a vertex-histogram
//! guess, since the placeholder mesh has no real yoke/head boundary to
//! read). This is the piece every prior GDTF/OFL slice in
//! `docs/research/lighting-console-landscape.md` flagged as "biggest
//! value, not started."
//!
//! Required patching the vendored `gdtf` crate (`crates/gdtf-vendored`,
//! see its `PATCH-NOTES.md`) — the upstream `Matrix` type (every
//! `<Geometry>` node's `Position`) has no public accessor for its values
//! at all, so reading a geometry tree's actual placement was impossible
//! through the published API.
//!
//! Real GDTF files use `<Axis>` geometry nodes for pan/tilt joints (per
//! the official spec example — `mvrdevelopment/spec/examples/geometry.md`
//! "Basic Moving Head": `Base(Geometry) -> Yoke(Axis) -> Head(Axis) ->
//! Beam(Beam)`), and a `<DMXChannel>`'s `Geometry` attribute names which
//! node a Pan/Tilt/other attribute actually rotates — not always the
//! whole fixture, and not always "the mesh's upper half" the way the
//! heuristic split assumes. That association is what this module reads
//! (`is_pan`/`is_tilt` on each node) so the live pan/tilt composition in
//! `scene.rs`/`fixture_profile.rs` can eventually target the *real*
//! joint instead of a guess, the same way `Shape::Mesh::split_z` does for
//! the placeholder mesh today.
//!
//! Scope of this slice: the Geometry tree + `PrimitiveType` fallback
//! shapes (box/cylinder, sized from the real `<Model>` dimensions) — not
//! real referenced 3D model files (glTF/3DS `Model::file`). Most
//! community-contributed GDTF files (including the one this module is
//! tested against) use primitive fallbacks rather than authoring real
//! meshes, so this alone is real, useful coverage; real-mesh import is a
//! separate, larger follow-on (a glTF parser, `zip`-embedded-resource
//! reading via `ResourceMap::read_model_mesh`) not started here.

use gdtf::geometry::{AnyGeometry, Geometry};
use gdtf::model::PrimitiveType;
use gdtf::values::{Matrix, Name};
use gdtf::GdtfFile;
use glam::{Quat, Vec3};
use std::fs::File;
use std::path::Path;

/// A real fixture-shape node, one per `<Geometry>`/`<Axis>`/`<Beam>` XML
/// element, in the mesh's own local frame (matches `ObjMesh`'s convention:
/// local -Z is the beam/aim direction — GDTF's own coordinate convention
/// turned out to already agree, translations run consistently along -Z
/// from base to beam in every real example checked, so no axis remap is
/// applied here the way the QLC+ COLLADA import needed one).
pub struct GdtfNode {
    pub name: String,
    pub shape: GdtfShape,
    pub local_pos: Vec3,
    pub local_rot: Quat,
    /// True if a `<DMXChannel>` with a `Pan` attribute names this node in
    /// its `Geometry` reference — a live pan reading should rotate this
    /// node (and everything under it), not the whole fixture or a
    /// guessed split point.
    pub is_pan: bool,
    /// Same as `is_pan`, for `Tilt`.
    pub is_tilt: bool,
    pub children: Vec<GdtfNode>,
}

pub enum GdtfShape {
    Box { size: Vec3 },
    Cylinder { height: f32, radius: f32 },
    /// A `<Beam>` node (the light-exit point) or any geometry with no
    /// resolvable primitive (an `Undefined`/real-mesh-only model this
    /// slice doesn't import) — present in the tree for its transform and
    /// pan/tilt association, nothing drawn for it directly.
    None,
}

pub struct GdtfFixture {
    pub fixture_type_name: String,
    pub dmx_mode_name: String,
    pub root: GdtfNode,
}

/// Reads a `.gdtf` file and builds the real geometry tree for one DMX
/// mode. `mode_name`, when `None`, picks the file's first mode — same
/// convention as `gdtf_import::import_channel_map`.
pub fn import_geometry(path: &Path, mode_name: Option<&str>) -> anyhow::Result<GdtfFixture> {
    let file = File::open(path)?;
    let gdtf = GdtfFile::new(file).map_err(|e| anyhow::anyhow!("failed to parse GDTF file: {e}"))?;

    let fixture_type = gdtf
        .description
        .fixture_types
        .first()
        .ok_or_else(|| anyhow::anyhow!("GDTF file defines no fixture types"))?;

    let mode = match mode_name {
        Some(name) => fixture_type
            .dmx_modes
            .iter()
            .find(|m| m.name.as_deref().map(|n| n.to_string()).as_deref() == Some(name))
            .ok_or_else(|| anyhow::anyhow!("GDTF file has no DMX mode named {name:?}"))?,
        None => fixture_type
            .dmx_modes
            .first()
            .ok_or_else(|| anyhow::anyhow!("GDTF file's fixture type defines no DMX modes"))?,
    };

    // Which geometry attributes are Pan/Tilt, and which node's name each
    // one targets — read straight from this mode's own DMXChannel list
    // (`ch.geometry`, `logical_channels[0].attribute`), the real
    // manufacturer-declared association, not a guess.
    let mut pan_targets = Vec::new();
    let mut tilt_targets = Vec::new();
    for ch in &mode.dmx_channels {
        let Some(logical) = ch.logical_channels.first() else { continue };
        let attr_name = logical.attribute.to_string();
        let geometry_name = ch.geometry.to_string();
        match attr_name.as_str() {
            "Pan" => pan_targets.push(geometry_name),
            "Tilt" => tilt_targets.push(geometry_name),
            _ => {}
        }
    }

    // The mode's own `Geometry` attribute names which top-level tree this
    // mode actually uses (a file can define more than one geometry tree —
    // see the spec's "More Complex Fixture Type" example); fall back to
    // the first declared tree if the mode doesn't say.
    let root_geometry = match &mode.geometry {
        Some(name) => fixture_type
            .geometries
            .iter()
            .find(|g| g.name().map(|n| n.to_string()).as_deref() == Some(name.to_string().as_str()))
            .ok_or_else(|| anyhow::anyhow!("DMX mode's start geometry {name:?} not found"))?,
        None => fixture_type
            .geometries
            .first()
            .ok_or_else(|| anyhow::anyhow!("fixture type defines no geometry trees"))?,
    };

    let root = build_node(root_geometry, fixture_type, &pan_targets, &tilt_targets);
    let fixture_type_name = fixture_type.name.as_deref().unwrap_or(&fixture_type.short_name).to_string();
    let dmx_mode_name = mode.name.as_deref().unwrap_or("").to_string();
    Ok(GdtfFixture { fixture_type_name, dmx_mode_name, root })
}

fn build_node(
    g: &Geometry,
    fixture_type: &gdtf::fixture_type::FixtureType,
    pan_targets: &[String],
    tilt_targets: &[String],
) -> GdtfNode {
    let name = g.name().map(Name::to_string).unwrap_or_default();
    let (local_pos, local_rot) = decompose_matrix(geometry_position(g));
    let shape = geometry_shape(g, fixture_type);
    let is_pan = pan_targets.iter().any(|t| t == &name);
    let is_tilt = tilt_targets.iter().any(|t| t == &name);
    let children = g.children().iter().map(|c| build_node(c, fixture_type, pan_targets, tilt_targets)).collect();
    GdtfNode { name, shape, local_pos, local_rot, is_pan, is_tilt, children }
}

/// `Geometry::position` isn't part of the `AnyGeometry` trait (each of
/// the ~18 concrete node types — Generic/Axis/Beam/Laser/Display/... —
/// carries it as its own field, not a shared one), so this has to match
/// every variant. All of them share the same `position: Matrix` field
/// shape in practice.
fn geometry_position(g: &Geometry) -> Matrix {
    match g {
        Geometry::Generic(x) => x.position,
        Geometry::Axis(x) => x.position,
        Geometry::FilterBeam(x) => x.position,
        Geometry::FilterColor(x) => x.position,
        Geometry::FilterGobo(x) => x.position,
        Geometry::FilterShaper(x) => x.position,
        Geometry::Beam(x) => x.position,
        Geometry::MediaServerLayer(x) => x.position,
        Geometry::MediaServerCamera(x) => x.position,
        Geometry::MediaServerMaster(x) => x.position,
        Geometry::Display(x) => x.position,
        Geometry::Reference(x) => x.position,
        Geometry::Laser(x) => x.position,
        Geometry::WiringObject(x) => x.position,
        Geometry::Inventory(x) => x.position,
        Geometry::Structure(x) => x.position,
        Geometry::Support(x) => x.position,
        Geometry::Magnet(x) => x.position,
    }
}

fn geometry_shape(g: &Geometry, fixture_type: &gdtf::fixture_type::FixtureType) -> GdtfShape {
    // A <Beam> node is the light-exit point, not a physical housing —
    // never draw a primitive for it, regardless of whether it links a
    // Model (real files sometimes do, for a lens/glass mesh this slice
    // doesn't render).
    if matches!(g, Geometry::Beam(_)) {
        return GdtfShape::None;
    }
    let Some(model) = g.model(fixture_type) else { return GdtfShape::None };
    // Width/Length/Height -> X/Y/Z: matches this project's own Z-up
    // convention (height = vertical), consistent with how `fixture_profile
    // .rs`'s other real-world dimension comments (e.g. the Uking Par's
    // 180x180x100mm) already read real spec sheets.
    let size = Vec3::new(model.width as f32, model.length as f32, model.height as f32);
    match model.primitive_type {
        PrimitiveType::Cylinder => GdtfShape::Cylinder { height: size.z, radius: size.x * 0.5 },
        PrimitiveType::Undefined if model.file.is_none() => GdtfShape::None,
        // Undefined-with-a-file means a real 3D model is referenced —
        // not imported this slice (see module doc); fall back to a box
        // from the model's own declared dimensions rather than nothing,
        // so the fixture doesn't just have an invisible gap where its
        // real mesh would be.
        _ => GdtfShape::Box { size },
    }
}

/// GDTF's `Matrix` is row-major with translation in column 3 of the first
/// three rows (confirmed against every real example in the spec's own
/// geometry.md — e.g. `{1,0,0,0}{0,1,0,0}{0,0,1,-0.225}{0,0,0,1}` is a
/// pure Z-translation, translation value as the last element of row 2).
fn decompose_matrix(m: Matrix) -> (Vec3, Quat) {
    let r = m.rows();
    let translation = Vec3::new(r[0][3] as f32, r[1][3] as f32, r[2][3] as f32);
    let col0 = Vec3::new(r[0][0] as f32, r[1][0] as f32, r[2][0] as f32);
    let col1 = Vec3::new(r[0][1] as f32, r[1][1] as f32, r[2][1] as f32);
    let col2 = Vec3::new(r[0][2] as f32, r[1][2] as f32, r[2][2] as f32);
    let rotation = Quat::from_mat3(&glam::Mat3::from_cols(col0, col1, col2));
    (translation, rotation)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real GDTF file matching the spec's own "Basic Moving Head" example
    /// verbatim (Base -> Yoke(Axis) -> Head(Axis) -> Beam), hand-built
    /// from `mvrdevelopment/spec/examples/geometry.md` rather than a
    /// manufacturer download (GDTF Share requires a registered account —
    /// see `docs/research/lighting-console-landscape.md`) but conforming
    /// to the same schema the real `gdtf` parser validates against, with
    /// added DMXChannel Pan/Tilt entries pointing at Yoke/Head so the
    /// pan/tilt-association logic has something real to resolve.
    const MOVING_HEAD: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets/gdtf-samples/basic-moving-head.gdtf");

    #[test]
    fn imports_the_real_yoke_head_beam_hierarchy() {
        let fixture = import_geometry(Path::new(MOVING_HEAD), None).expect("parses the sample file");
        assert_eq!(fixture.root.name, "Base");
        assert_eq!(fixture.root.children.len(), 1);

        let yoke = &fixture.root.children[0];
        assert_eq!(yoke.name, "Yoke");
        assert!(yoke.is_pan, "Yoke should be the Pan target");
        assert!(!yoke.is_tilt);
        // Base -> Yoke translation matches the spec example exactly.
        assert!((yoke.local_pos.z - (-0.225)).abs() < 0.001);

        let head = &yoke.children[0];
        assert_eq!(head.name, "Head");
        assert!(head.is_tilt, "Head should be the Tilt target");
        assert!(!head.is_pan);
        assert!((head.local_pos.z - (-0.100)).abs() < 0.001);

        let beam = &head.children[0];
        assert_eq!(beam.name, "Beam");
        assert!(matches!(beam.shape, GdtfShape::None), "a Beam node has no drawable primitive");
        assert!((beam.local_pos.z - (-0.150)).abs() < 0.001);
    }

    #[test]
    fn primitive_shapes_carry_real_dimensions() {
        let fixture = import_geometry(Path::new(MOVING_HEAD), None).unwrap();
        // Base is PrimitiveType Cube with real Width/Length/Height.
        match fixture.root.shape {
            GdtfShape::Box { size } => {
                assert!(size.x > 0.0 && size.y > 0.0 && size.z > 0.0);
            }
            _ => panic!("expected Base to resolve to a Box primitive"),
        }
    }
}
