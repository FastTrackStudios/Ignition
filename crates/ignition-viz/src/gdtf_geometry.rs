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
//! Shapes come from three sources, in order: a real 3D model file the
//! profile ships (`Model::file` -> `models/gltf/<file>.glb`, falling back
//! to `models/3ds/<file>.3ds`, decoded by `gdtf_mesh.rs`), one of the
//! spec's own standard primitive meshes (`PrimitiveType::Base/Yoke/Head/
//! Scanner/Conventional/...`, embedded from `assets/gdtf-primitives/`),
//! or a procedural Cube/Cylinder/Sphere. Whatever the source, the mesh is
//! scaled to the `<Model>`'s declared Length/Width/Height — the spec
//! (gdtf-spec.md, "3D Models") says those dimensions always govern, no
//! matter how the file itself is scaled. Materials and textures in the
//! files are ignored: the body is drawn in the venue's fixture material.

use bevy::math::{Mat3, Quat, Vec3};
use bevy::prelude::*;
use gdtf::geometry::{AnyGeometry, Geometry};
use gdtf::model::PrimitiveType;
// `Name` is aliased because Bevy's prelude has one too, and this module
// uses both — the GDTF one to read a node's declared name, Bevy's to
// label the entity spawned for it.
use gdtf::GdtfFile;
use gdtf::values::{Matrix, Name as GdtfName};
use gdtf::{Model3Detail, Model3Format, ResourceMap};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::gdtf_mesh::{RawMesh, parse_3ds, parse_glb};

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
    /// True for a `<Beam>` node: the spec's own marker for where the
    /// light leaves the fixture ("the position of the fixture's light
    /// output (usually the position of the lens)"), firing along the
    /// node's -Z. Only these become emitters — a `<Geometry>` with no
    /// model (a DMX socket, a power inlet) also draws nothing, and used
    /// to be mistaken for one.
    pub is_beam: bool,
    pub children: Vec<GdtfNode>,
}

impl GdtfNode {
    /// Every `<Beam>` node's pose in the fixture root's frame, in
    /// document order: where each emitter sits and which way it fires
    /// with every joint at rest.
    pub fn beam_poses(&self) -> Vec<(Vec3, Quat)> {
        let mut out = Vec::new();
        self.collect_beams(Vec3::ZERO, Quat::IDENTITY, &mut out);
        out
    }

    fn collect_beams(&self, parent_pos: Vec3, parent_rot: Quat, out: &mut Vec<(Vec3, Quat)>) {
        let pos = parent_pos + parent_rot * self.local_pos;
        let rot = parent_rot * self.local_rot;
        if self.is_beam {
            out.push((pos, rot));
        }
        for c in &self.children {
            c.collect_beams(pos, rot, out);
        }
    }

    /// Where a bar's cells hang: the lowest node every `<Beam>` in the
    /// tree sits under, as its index in pre-order (the order
    /// `spawn_gdtf_tree` spawns entities in), with each cell's pose in
    /// that node's own frame. `None` for a tree with no beam at all.
    ///
    /// The lowest common ancestor rather than a direct parent because a
    /// file is free to group its cells — the Ultra Bar (and every
    /// generated bar that borrows its geometry) nests twelve cells in
    /// pairs, two levels down. A bar's one emitter is spawned under this
    /// node so the strip is carried by the same transform tree as the
    /// cells it stands in for: whatever joint or display scale moves
    /// the cells moves the strip.
    // r[impl viz.one-emitter-tree] - a bar's strip hangs where its cells hang
    pub fn bar_cells(&self) -> Option<(usize, Vec<(Vec3, Quat)>)> {
        let mut paths: Vec<Vec<usize>> = Vec::new();
        self.collect_beam_paths(&mut Vec::new(), &mut paths);
        let mut prefix = paths.first()?.clone();
        for path in &paths[1..] {
            let common = prefix.iter().zip(path).take_while(|(a, b)| a == b).count();
            prefix.truncate(common);
        }
        let mut node = self;
        let mut index = 0usize;
        for &child in &prefix {
            index += 1 + node.children[..child]
                .iter()
                .map(GdtfNode::len)
                .sum::<usize>();
            node = &node.children[child];
        }
        let mut cells = Vec::new();
        for c in &node.children {
            c.collect_beams(Vec3::ZERO, Quat::IDENTITY, &mut cells);
        }
        if node.is_beam {
            cells.insert(0, (Vec3::ZERO, Quat::IDENTITY));
        }
        Some((index, cells))
    }

    fn collect_beam_paths(&self, path: &mut Vec<usize>, out: &mut Vec<Vec<usize>>) {
        if self.is_beam {
            out.push(path.clone());
        }
        for (i, c) in self.children.iter().enumerate() {
            path.push(i);
            c.collect_beam_paths(path, out);
            path.pop();
        }
    }

    /// Nodes in this subtree, itself included.
    fn len(&self) -> usize {
        1 + self.children.iter().map(GdtfNode::len).sum::<usize>()
    }

    /// Scales the whole tree about the root — every placement and every
    /// drawn part — for a profile the venue wants shown larger than
    /// life. Emitters move with their parts, so a beam still leaves the
    /// lens.
    // r[impl viz.gdtf-aliases] - a display scale is applied to the tree, not the spec
    pub fn scale_by(&mut self, factor: f32) {
        self.local_pos *= factor;
        match &mut self.shape {
            GdtfShape::Box { size } => *size *= factor,
            GdtfShape::Cylinder { height, radius } => {
                *height *= factor;
                *radius *= factor;
            }
            GdtfShape::Mesh(mesh) => {
                if let Some(p) =
                    mesh.attribute_mut(Mesh::ATTRIBUTE_POSITION)
                        .and_then(|a| match a {
                            bevy::mesh::VertexAttributeValues::Float32x3(v) => Some(v),
                            _ => None,
                        })
                {
                    for v in p.iter_mut() {
                        v[0] *= factor;
                        v[1] *= factor;
                        v[2] *= factor;
                    }
                }
            }
            GdtfShape::None => {}
        }
        for c in &mut self.children {
            c.scale_by(factor);
        }
    }

    /// The axis-aligned bounds of everything this tree draws, in the
    /// fixture root's frame with every joint at rest — the assembled
    /// fixture's physical box. `None` when nothing is drawn.
    pub fn assembled_bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut acc: Option<(Vec3, Vec3)> = None;
        self.collect_bounds(Vec3::ZERO, Quat::IDENTITY, &mut acc);
        acc
    }

    fn collect_bounds(&self, parent_pos: Vec3, parent_rot: Quat, acc: &mut Option<(Vec3, Vec3)>) {
        let pos = parent_pos + parent_rot * self.local_pos;
        let rot = parent_rot * self.local_rot;
        let corners: Option<[Vec3; 8]> = match &self.shape {
            GdtfShape::Box { size } => Some(box_corners(-*size * 0.5, *size * 0.5)),
            // Laid over onto Z, the way `spawn_gdtf_tree` draws it.
            GdtfShape::Cylinder { height, radius } => Some(box_corners(
                Vec3::new(-radius, -radius, -height * 0.5),
                Vec3::new(*radius, *radius, height * 0.5),
            )),
            GdtfShape::Mesh(mesh) => mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .and_then(|a| a.as_float3())
                .map(|p| {
                    p.iter().map(|v| Vec3::from(*v)).fold(
                        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
                        |(lo, hi), v| (lo.min(v), hi.max(v)),
                    )
                })
                .map(|(lo, hi)| box_corners(lo, hi)),
            GdtfShape::None => None,
        };
        if let Some(corners) = corners {
            for c in corners {
                let w = pos + rot * c;
                *acc = Some(match *acc {
                    Some((lo, hi)) => (lo.min(w), hi.max(w)),
                    None => (w, w),
                });
            }
        }
        for c in &self.children {
            c.collect_bounds(pos, rot, acc);
        }
    }
}

fn box_corners(lo: Vec3, hi: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(lo.x, lo.y, lo.z),
        Vec3::new(hi.x, lo.y, lo.z),
        Vec3::new(lo.x, hi.y, lo.z),
        Vec3::new(hi.x, hi.y, lo.z),
        Vec3::new(lo.x, lo.y, hi.z),
        Vec3::new(hi.x, lo.y, hi.z),
        Vec3::new(lo.x, hi.y, hi.z),
        Vec3::new(hi.x, hi.y, hi.z),
    ]
}

pub enum GdtfShape {
    Box {
        size: Vec3,
    },
    Cylinder {
        height: f32,
        radius: f32,
    },
    /// A real triangle mesh — decoded from the profile's own model file
    /// or from one of the spec's standard primitives — already scaled to
    /// the `<Model>` dimensions and in the node's local Z-up frame.
    /// Cloned into the asset store each time the fixture is spawned.
    Mesh(Mesh),
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
    /// The first `<Beam>` node's `BeamAngle` (full angle, degrees), when
    /// the file states one above zero. The profile is the single source
    /// of truth for how wide a fixture's light is: a patch's own angle
    /// is only consulted when no profile resolves at all.
    // r[impl viz.profile-optics] - the profile's <Beam> angles are the emitter's
    pub beam_angle_deg: Option<f32>,
    /// The same node's `FieldAngle` (full, degrees) when it states one
    /// wider than the beam angle — the 10% edge, where the beam angle is
    /// the 50% edge. `None` when the file repeats the beam angle or
    /// leaves the default: the visualizer then assumes twice the beam,
    /// which is where an LED par's field typically lands.
    pub field_angle_deg: Option<f32>,
}

impl GdtfFixture {
    /// Beam and field angles as full angles in degrees — the field
    /// assumed at twice the beam when the file has no wider one. `None`
    /// when the file states no beam angle.
    // r[impl viz.profile-optics] - field is 2x beam when the file has none
    pub fn optics(&self) -> Option<(f32, f32)> {
        let beam = self.beam_angle_deg?;
        Some((beam, self.field_angle_deg.unwrap_or(beam * 2.0)))
    }
}

/// The first `<Beam>` in the tree with a positive `BeamAngle`, with its
/// `FieldAngle` when that is genuinely wider.
fn first_beam_angles(g: &Geometry) -> Option<(f32, Option<f32>)> {
    if let Geometry::Beam(b) = g
        && b.beam_angle > 0.1
    {
        let field = (b.field_angle > b.beam_angle + 0.1).then_some(b.field_angle as f32);
        return Some((b.beam_angle as f32, field));
    }
    g.children().iter().find_map(first_beam_angles)
}

/// Reads a `.gdtf` file and builds the real geometry tree for one DMX
/// mode. `mode_name`, when `None`, picks the file's first mode — same
/// convention as `gdtf_import::import_channel_map`.
pub fn import_geometry(path: &Path, mode_name: Option<&str>) -> anyhow::Result<GdtfFixture> {
    let file = File::open(path)?;
    let mut gdtf =
        GdtfFile::new(file).map_err(|e| anyhow::anyhow!("failed to parse GDTF file: {e}"))?;
    let GdtfFile {
        description,
        resources,
    } = &mut gdtf;

    let fixture_type = description
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
        let Some(logical) = ch.logical_channels.first() else {
            continue;
        };
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

    let mut models = ModelCache {
        resources,
        loaded: HashMap::new(),
    };
    let root = build_node(
        root_geometry,
        fixture_type,
        &pan_targets,
        &tilt_targets,
        &mut models,
    );
    let fixture_type_name = fixture_type
        .name
        .as_deref()
        .unwrap_or(&fixture_type.short_name)
        .to_string();
    let dmx_mode_name = mode.name.as_deref().unwrap_or("").to_string();
    let (beam_angle_deg, field_angle_deg) = match first_beam_angles(root_geometry) {
        Some((beam, field)) => (Some(beam), field),
        None => (None, None),
    };
    Ok(GdtfFixture {
        fixture_type_name,
        dmx_mode_name,
        root,
        beam_angle_deg,
        field_angle_deg,
    })
}

fn build_node(
    g: &Geometry,
    fixture_type: &gdtf::fixture_type::FixtureType,
    pan_targets: &[String],
    tilt_targets: &[String],
    models: &mut ModelCache<'_>,
) -> GdtfNode {
    let name = g.name().map(GdtfName::to_string).unwrap_or_default();
    let (local_pos, local_rot) = decompose_matrix(geometry_position(g));
    let shape = geometry_shape(g, fixture_type, models);
    let is_pan = pan_targets.iter().any(|t| t == &name);
    let is_tilt = tilt_targets.iter().any(|t| t == &name);
    let children = g
        .children()
        .iter()
        .map(|c| build_node(c, fixture_type, pan_targets, tilt_targets, models))
        .collect();
    GdtfNode {
        name,
        shape,
        local_pos,
        local_rot,
        is_pan,
        is_tilt,
        is_beam: matches!(g, Geometry::Beam(_)),
        children,
    }
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

/// The model files of one `.gdtf`, decoded at most once each: a file's
/// geometries routinely share a Model (every cell of a bar, both arms
/// of a yoke), and the zip is read through a seeking cursor, so this
/// keeps the decode per distinct file rather than per node.
struct ModelCache<'a> {
    resources: &'a mut ResourceMap,
    loaded: HashMap<String, Option<RawMesh>>,
}

impl ModelCache<'_> {
    // r[impl viz.gdtf-meshes] - a profile's own glb/3ds is what gets drawn
    fn get(&mut self, file: &str) -> Option<RawMesh> {
        if let Some(hit) = self.loaded.get(file) {
            return hit.clone();
        }
        let raw = read_model_file(self.resources, file);
        self.loaded.insert(file.to_string(), raw.clone());
        raw
    }
}

/// Reads `models/gltf/<file>.glb`, or failing that `models/3ds/<file>.3ds`
/// — the spec prefers glTF, and a file that ships both is meant to be
/// the same model twice. A missing or undecodable file is reported and
/// yields `None`, so the node falls back to a box of the declared size
/// rather than taking the fixture down with it.
fn read_model_file(resources: &mut ResourceMap, file: &str) -> Option<RawMesh> {
    let attempts: [(Model3Format, fn(&[u8]) -> anyhow::Result<RawMesh>); 2] = [
        (Model3Format::Gltf, parse_glb),
        (Model3Format::Max3ds, parse_3ds),
    ];
    let mut last_err = None;
    for (format, decode) in attempts {
        let mut bytes = Vec::new();
        let read = resources
            .read_model_mesh(file, format, Model3Detail::Default)
            .map_err(|e| anyhow::anyhow!("{e}"))
            .and_then(|mut r| r.read_to_end(&mut bytes).map_err(Into::into));
        if read.is_err() {
            continue;
        }
        match decode(&bytes) {
            Ok(raw) if !raw.is_empty() => return Some(raw),
            Ok(_) => last_err = Some(anyhow::anyhow!("{format:?} model has no triangles")),
            Err(e) => last_err = Some(e),
        }
    }
    match last_err {
        Some(e) => eprintln!("viz: GDTF model {file:?}: {e}; drawing a box instead"),
        None => eprintln!("viz: GDTF model {file:?} has no glb or 3ds in the file; drawing a box"),
    }
    None
}

/// The spec's own standard-primitive meshes (see
/// `assets/gdtf-primitives/LICENSE-NOTICE.txt`), decoded once per
/// process. Cube/Cylinder/Sphere/Pigtail have no file: they are made
/// procedurally in [`geometry_shape`].
// r[impl viz.gdtf-meshes] - the spec's standard primitives are real meshes too
fn standard_primitive(pt: PrimitiveType) -> Option<&'static RawMesh> {
    static MESHES: OnceLock<HashMap<PrimitiveType, RawMesh>> = OnceLock::new();
    let meshes = MESHES.get_or_init(|| {
        macro_rules! embed {
            ($($pt:ident => $file:literal),* $(,)?) => {{
                let mut m = HashMap::new();
                $(
                    match parse_3ds(include_bytes!(concat!(
                        "../assets/gdtf-primitives/", $file
                    ))) {
                        Ok(raw) if !raw.is_empty() => {
                            m.insert(PrimitiveType::$pt, raw);
                        }
                        Ok(_) => eprintln!("viz: GDTF primitive {} is empty", $file),
                        Err(e) => eprintln!("viz: GDTF primitive {}: {e}", $file),
                    }
                )*
                m
            }};
        }
        embed! {
            Base => "primitivetype_base.3ds",
            Yoke => "primitivetype_yoke.3ds",
            Head => "primitivetype_head.3ds",
            Scanner => "primitivetype_scanner.3ds",
            Conventional => "primitivetype_conventional.3ds",
            Base1_1 => "primitivetype_base_1.1.3ds",
            Scanner1_1 => "primitivetype_scanner_1.1.3ds",
            Conventional1_1 => "primitivetype_conventional_1.1.3ds",
        }
    });
    meshes.get(&pt)
}

fn geometry_shape(
    g: &Geometry,
    fixture_type: &gdtf::fixture_type::FixtureType,
    models: &mut ModelCache<'_>,
) -> GdtfShape {
    // A <Beam> node is the light-exit point, not a physical housing —
    // never draw a primitive for it, regardless of whether it links a
    // Model (real files often do, for a lens/glass mesh): the emitter
    // spawned at its transform is what shows there.
    if matches!(g, Geometry::Beam(_)) {
        return GdtfShape::None;
    }
    let Some(model) = g.model(fixture_type) else {
        return GdtfShape::None;
    };
    // Spec, "3D Models": Length is the X extent, Width the Y extent,
    // Height the Z extent — GDTF is Z-up, the same as this project.
    let size = Vec3::new(model.length as f32, model.width as f32, model.height as f32);

    // A referenced model file wins over the primitive type: the spec lets
    // a profile set both, with the primitive as the fallback for readers
    // that can't decode the file.
    // (`File=""` is how most authoring tools write "no file".)
    let file = model.file.as_deref().filter(|f| !f.is_empty());
    if let Some(file) = file {
        if let Some(raw) = models.get(file) {
            return GdtfShape::Mesh(raw_to_mesh(&raw, size));
        }
    }

    match model.primitive_type {
        PrimitiveType::Cylinder => GdtfShape::Cylinder {
            height: size.z,
            radius: size.x * 0.5,
        },
        // A pigtail is the fixture's cable tail: a thin stub is all it
        // needs to be, and the spec ships no mesh for it.
        PrimitiveType::Pigtail => GdtfShape::Cylinder {
            height: size.z,
            radius: size.x.min(size.y) * 0.5,
        },
        PrimitiveType::Sphere => GdtfShape::Mesh(scaled_procedural(
            Sphere::new(0.5)
                .mesh()
                .ico(3)
                .unwrap_or_else(|_| Sphere::new(0.5).mesh().uv(16, 8)),
            size,
        )),
        PrimitiveType::Cube => GdtfShape::Box { size },
        PrimitiveType::Undefined if file.is_none() => GdtfShape::None,
        pt => match standard_primitive(pt) {
            Some(raw) => GdtfShape::Mesh(raw_to_mesh(raw, size)),
            // Undefined-with-a-file that failed to decode, or a primitive
            // whose mesh didn't embed: a box of the declared size rather
            // than an invisible gap where the part should be.
            None => GdtfShape::Box { size },
        },
    }
}

/// A unit Bevy primitive, scaled so its bounds match `size`.
fn scaled_procedural(mesh: Mesh, size: Vec3) -> Mesh {
    mesh.scaled_by(size.max(Vec3::splat(0.005)))
}

/// Scale factors that fit `raw`'s bounds to `size`, per axis. An axis
/// the mesh has no extent on (a flat plate) or the model declares as
/// zero borrows the other axes' ratio, so a mesh authored in millimetres
/// against a metre-sized model still comes out the right size.
fn fit_scale(raw: &RawMesh, size: Vec3) -> Vec3 {
    let Some((lo, hi)) = raw.bounds() else {
        return Vec3::ONE;
    };
    let extent = hi - lo;
    let ratio = |i: usize| -> Option<f32> {
        (extent[i] > 1e-6 && size[i] > 1e-6).then(|| size[i] / extent[i])
    };
    let fallback = (0..3).filter_map(ratio).next().unwrap_or(1.0);
    Vec3::new(
        ratio(0).unwrap_or(fallback),
        ratio(1).unwrap_or(fallback),
        ratio(2).unwrap_or(fallback),
    )
}

/// Builds the Bevy mesh for a decoded model, scaled to the `<Model>`
/// dimensions about the origin (the origin is the part's pivot — spec:
/// "drawn around its own suspension point" — so it is not recentred).
/// A model without normals is flat-shaded.
// r[impl viz.gdtf-meshes] - scaled to the Model's declared dimensions
pub fn raw_to_mesh(raw: &RawMesh, size: Vec3) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};

    let scale = fit_scale(raw, size);
    let positions: Vec<[f32; 3]> = raw
        .positions
        .iter()
        .map(|p| (Vec3::from(*p) * scale).to_array())
        .collect();
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_indices(Indices::U32(raw.indices.clone()));
    if raw.normals.len() == raw.positions.len() && !raw.normals.is_empty() {
        // Non-uniform scale bends normals: inverse-transpose of a
        // diagonal is 1/scale.
        let inv = Vec3::ONE / scale;
        let normals: Vec<[f32; 3]> = raw
            .normals
            .iter()
            .map(|n| (Vec3::from(*n) * inv).normalize_or_zero().to_array())
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    } else {
        mesh.duplicate_vertices();
        mesh.compute_flat_normals();
    }
    mesh
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
    let rotation = Quat::from_mat3(&Mat3::from_cols(col0, col1, col2));
    (translation, rotation)
}

/// A directory of `.gdtf` files, keyed by the fixture-type name each one
/// declares, so a venue fixture can be matched to a real manufacturer
/// profile by its patch strings.
///
/// Loaded once at startup and shared: a rig has 71 fixtures and perhaps
/// four distinct types, so parsing per fixture would be re-reading the
/// same zip dozens of times.
#[derive(Default)]
pub struct GdtfLibrary {
    by_type: HashMap<String, GdtfFixture>,
    /// `aliases.json` next to the profiles: normalized venue model string
    /// -> normalized fixture-type name.
    aliases: HashMap<String, String>,
}

impl GdtfLibrary {
    /// Reads every `.gdtf` in `dir` and in each of its immediate
    /// subdirectories (`data/gdtf` holds downloaded manufacturer files,
    /// `data/gdtf/generated` the ones `tools/make_gdtf.py` writes), plus
    /// `dir/aliases.json` if present. A file that fails to parse is
    /// reported and skipped rather than failing the whole load — one bad
    /// download should not stop the visualizer opening.
    ///
    /// Two profiles with the same fixture-type name resolve
    /// deterministically: files are visited in sorted order, top level
    /// first, then each subdirectory, and a later file replaces an
    /// earlier one — so a generated profile in a subdirectory always wins
    /// over a downloaded one of the same name at the top level.
    pub fn load_dir(dir: &Path) -> anyhow::Result<Self> {
        let mut by_type = HashMap::new();
        let entries = std::fs::read_dir(dir)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", dir.display()))?;
        let mut files = Vec::new();
        let mut subdirs = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                subdirs.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("gdtf") {
                files.push(path);
            }
        }
        files.sort();
        subdirs.sort();
        for sub in subdirs {
            let Ok(entries) = std::fs::read_dir(&sub) else {
                continue;
            };
            let mut nested: Vec<_> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("gdtf"))
                .collect();
            nested.sort();
            files.extend(nested);
        }
        for path in files {
            match import_geometry(&path, None) {
                Ok(fixture) => {
                    println!(
                        "loaded GDTF {:?} (mode {:?}) from {}",
                        fixture.fixture_type_name,
                        fixture.dmx_mode_name,
                        path.display()
                    );
                    by_type.insert(normalize(&fixture.fixture_type_name), fixture);
                }
                Err(e) => eprintln!("viz: skipping {}: {e}", path.display()),
            }
        }
        let (aliases, display_scale) = load_aliases(&dir.join("aliases.json"));
        for (name, factor) in display_scale {
            if let Some(fixture) = by_type.get_mut(&name) {
                fixture.root.scale_by(factor);
            } else {
                eprintln!("viz: aliases.json _display_scale names no profile: {name:?}");
            }
        }
        Ok(Self { by_type, aliases })
    }

    /// The workspace's own library: `data/gdtf` relative to the current
    /// directory (how `viz --venue data/venues/norco` addresses data), or
    /// failing that relative to the crate's own location, as the venue
    /// tests do. Returns an empty library, never an error, when neither
    /// exists — a checkout without profiles still gets placeholder
    /// meshes.
    pub fn load_default() -> Self {
        let candidates = [
            PathBuf::from("data/gdtf"),
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/gdtf"),
        ];
        for dir in candidates {
            if !dir.is_dir() {
                continue;
            }
            match Self::load_dir(&dir) {
                Ok(lib) => return lib,
                Err(e) => eprintln!("viz: {e}"),
            }
        }
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_type.len()
    }

    /// Matches a patched fixture's manufacturer/model against the loaded
    /// profiles, in order:
    ///
    /// 1. the model string equals a fixture-type name (normalized —
    ///    generated profiles carry the console name verbatim);
    /// 2. `aliases.json` maps the model string to a fixture-type name;
    /// 3. a normalized substring in either direction, as a last resort —
    ///    Eos model strings carry DMX-mode suffixes and vary in
    ///    punctuation between channels of the same physical fixture, the
    ///    same leniency `fixture_profile::shape_for` uses.
    // r[impl viz.gdtf-aliases] - exact name, then aliases.json, then fuzzy
    pub fn find(&self, manufacturer: &str, model: &str) -> Option<&GdtfFixture> {
        let model_key = normalize(model);
        if let Some(fixture) = self.by_type.get(&model_key) {
            return Some(fixture);
        }
        if let Some(fixture) = self
            .aliases
            .get(&model_key)
            .and_then(|name| self.by_type.get(name))
        {
            return Some(fixture);
        }
        let needle = normalize(&format!("{manufacturer} {model}"));
        // Sorted so the fuzzy fallback is stable across runs, not
        // whichever HashMap bucket comes first.
        let mut keys: Vec<&String> = self.by_type.keys().collect();
        keys.sort();
        keys.into_iter().find_map(|key| {
            let hit = needle.contains(key.as_str())
                || key.contains(&model_key)
                || (!model_key.is_empty() && model_key.contains(key.as_str()));
            hit.then(|| &self.by_type[key])
        })
    }
}

/// `{"<venue model string>": "<fixture type name>"}`, both sides
/// normalized on load. Missing file: no aliases. Malformed file: reported
/// and treated as none.
/// Also reads `"_display_scale": {"<fixture type name>": factor}` —
/// profiles a venue wants drawn larger than life (a mini mover that
/// reads as a toy at true size from the back of the room). Applied to
/// the imported tree, never to the profile's own dimensions.
// r[impl viz.gdtf-aliases] - the alias file, and the display scales beside it
fn load_aliases(path: &Path) -> (HashMap<String, String>, HashMap<String, f32>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (HashMap::new(), HashMap::new());
    };
    parse_aliases(&text).unwrap_or_else(|e| {
        eprintln!("viz: ignoring {}: {e}", path.display());
        (HashMap::new(), HashMap::new())
    })
}

#[allow(clippy::type_complexity)]
fn parse_aliases(text: &str) -> anyhow::Result<(HashMap<String, String>, HashMap<String, f32>)> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let Some(map) = value.as_object() else {
        anyhow::bail!("aliases.json is not an object");
    };
    let mut aliases = HashMap::new();
    let mut scales = HashMap::new();
    for (k, v) in map {
        if k == "_display_scale" {
            let Some(entries) = v.as_object() else {
                anyhow::bail!("_display_scale is not an object");
            };
            for (name, factor) in entries {
                let Some(factor) = factor.as_f64() else {
                    anyhow::bail!("_display_scale {name:?} is not a number");
                };
                if factor <= 0.0 {
                    anyhow::bail!("_display_scale {name:?} must be positive");
                }
                scales.insert(normalize(name), factor as f32);
            }
        } else if k.starts_with('_') {
            continue;
        } else if let Some(target) = v.as_str() {
            aliases.insert(normalize(k), normalize(target));
        } else {
            anyhow::bail!("alias {k:?} is not a string");
        }
    }
    Ok((aliases, scales))
}

/// Lowercased, with everything that is not alphanumeric removed — so
/// "Mac Aura XB", "mac-aura-xb" and "MAC_AuraXB" all compare equal.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Spawns `fixture`'s real Geometry tree as an entity hierarchy under
/// `parent`, and returns the entities that emit light.
///
/// A GDTF Geometry tree *is* a transform hierarchy — Base carries Yoke
/// carries Head carries Beam, each with its own local placement — so it
/// maps onto parent/child entities directly, and the kinematic chain
/// comes free: rotating the Yoke entity moves the head, the beam and
/// everything under them, because Bevy already propagates transforms.
/// The pre-Bevy version of this walked the tree every frame recomposing
/// world matrices by hand to bake triangles.
///
/// Joints are marked with `PanJoint`/`TiltJoint` from the file's own
/// `<DMXChannel>` `Geometry` associations, so a live reading rotates the
/// node the manufacturer says it rotates — not a guessed split point the
/// way the QLC+ placeholder mesh needs.
///
/// `nodes` receives every node's entity in pre-order — the order
/// `GdtfNode::bar_cells` counts in — so a caller can hang something under
/// a particular node of the file's tree after the fact.
/// Mesh assets shared across every fixture of a type.
///
/// A profile's tree is one `GdtfNode` per model, and every fixture of
/// that type is spawned from the same tree — so the mesh a node carries
/// is the same mesh for all forty-eight pars, and adding it to
/// `Assets<Mesh>` once per fixture made forty-eight GPU copies that the
/// engine could not batch. Keyed on the node's own address (the library
/// outlives the spawn) and the split, so the same model split the same
/// way is one asset and one instanced draw.
// r[impl viz.performance-budget] - one mesh asset per fixture model
pub struct SharedMeshes<'a> {
    pub assets: &'a mut Assets<Mesh>,
    cache: HashMap<(usize, u64), Handle<Mesh>>,
}

impl<'a> SharedMeshes<'a> {
    pub fn new(assets: &'a mut Assets<Mesh>) -> Self {
        Self {
            assets,
            cache: HashMap::new(),
        }
    }

    /// The handle for `key`, building the mesh with `build` only the
    /// first time it is asked for.
    pub fn get_or_add(&mut self, key: (usize, u64), build: impl FnOnce() -> Mesh) -> Handle<Mesh> {
        if let Some(handle) = self.cache.get(&key) {
            return handle.clone();
        }
        let handle = self.assets.add(build());
        self.cache.insert(key, handle.clone());
        handle
    }

    /// How many distinct meshes have been shared so far.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

pub fn spawn_gdtf_tree(
    commands: &mut Commands,
    parent: Entity,
    node: &GdtfNode,
    meshes: &mut SharedMeshes<'_>,
    material: &Handle<StandardMaterial>,
    emitters: &mut Vec<Entity>,
    nodes: &mut Vec<Entity>,
) {
    let mut entity = commands.spawn((
        Transform {
            translation: node.local_pos,
            rotation: node.local_rot,
            scale: Vec3::ONE,
        },
        Visibility::default(),
        Name::new(node.name.clone()),
        ChildOf(parent),
    ));
    if node.is_pan {
        entity.insert(PanJoint);
    }
    if node.is_tilt {
        entity.insert(TiltJoint);
    }
    let id = entity.id();
    nodes.push(id);

    match node.shape {
        GdtfShape::Box { size } => {
            let key = (node as *const GdtfNode as usize, 0);
            commands.spawn((
                Mesh3d(meshes.get_or_add(key, || {
                    Cuboid::from_size(size.max(Vec3::splat(0.005))).into()
                })),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                ChildOf(id),
            ));
        }
        GdtfShape::Cylinder { height, radius } => {
            // Bevy's cylinder stands on +Y; this project's world and
            // GDTF's own geometry are Z-up, so it is laid over.
            let key = (node as *const GdtfNode as usize, 0);
            commands.spawn((
                Mesh3d(meshes.get_or_add(key, || {
                    Cylinder::new(radius.max(0.005), height.max(0.005)).into()
                })),
                MeshMaterial3d(material.clone()),
                Transform::from_rotation(Quat::from_rotation_arc(Vec3::Y, Vec3::Z)),
                ChildOf(id),
            ));
        }
        // Already in the node's Z-up frame and at the model's size: the
        // body material is the venue's, never the file's.
        GdtfShape::Mesh(ref mesh) => {
            let key = (node as *const GdtfNode as usize, 0);
            commands.spawn((
                Mesh3d(meshes.get_or_add(key, || mesh.clone())),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                ChildOf(id),
            ));
        }
        // Nothing to draw — a `<Beam>` node, or a socket/inlet with no
        // model. Only the former is a light source, decided below.
        GdtfShape::None => {}
    }
    // A `<Beam>` node is the light-exit point, not a housing: nothing is
    // drawn for it, but its transform is exactly where a beam starts and
    // which way it points, which is what the caller wants back. A file
    // may declare more than one (a multi-lens fixture). Every model-less
    // `<Geometry>` used to be pushed here too, which put four emitters on
    // a par's DMX/power sockets — up at the pigtail, above the head —
    // and lit the room from there instead of from the lens.
    // r[impl viz.emitter-at-beam-node] - only a <Beam> node emits
    if node.is_beam {
        emitters.push(id);
    }

    for child in &node.children {
        spawn_gdtf_tree(commands, id, child, meshes, material, emitters, nodes);
    }
}

/// The node a live Pan reading rotates, per the fixture's own file.
#[derive(Component)]
pub struct PanJoint;

/// The node a live Tilt reading rotates, per the fixture's own file.
#[derive(Component)]
pub struct TiltJoint;

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
    const MOVING_HEAD: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/gdtf-samples/basic-moving-head.gdtf"
    );

    #[test]
    fn imports_the_real_yoke_head_beam_hierarchy() {
        let fixture =
            import_geometry(Path::new(MOVING_HEAD), None).expect("parses the sample file");
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
        assert!(
            matches!(beam.shape, GdtfShape::None),
            "a Beam node has no drawable primitive"
        );
        assert!((beam.local_pos.z - (-0.150)).abs() < 0.001);
    }

    /// `data/gdtf/`, the real manufacturer downloads the venues patch
    /// against. Tests that need it skip when the checkout doesn't have
    /// it (a shallow clone, or a CI job without the data tree).
    fn real_gdtf_files() -> Vec<std::path::PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/gdtf");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut files: Vec<_> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("gdtf"))
            .collect();
        files.sort();
        files
    }

    fn count_shapes(node: &GdtfNode, meshes: &mut usize, boxes: &mut usize, drawn: &mut usize) {
        match node.shape {
            GdtfShape::Mesh(_) => {
                *meshes += 1;
                *drawn += 1;
            }
            GdtfShape::Box { .. } => {
                *boxes += 1;
                *drawn += 1;
            }
            GdtfShape::Cylinder { .. } => *drawn += 1,
            GdtfShape::None => {}
        }
        for c in &node.children {
            count_shapes(c, meshes, boxes, drawn);
        }
    }

    fn mesh_bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .expect("positions");
        positions.iter().map(|p| Vec3::from(*p)).fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        )
    }

    /// The lowest drawn face of the tree in the root frame — for a par
    /// hanging lens-down, the lens.
    fn lowest_drawn_z(root: &GdtfNode) -> f32 {
        root.assembled_bounds().expect("draws something").0.z
    }

    /// The pars: one emitter, at the lens, firing out of it. Both the
    /// Chauvet download and the generated Uking Par that borrows its
    /// geometry carry four model-less socket nodes under the pigtail
    /// (Power IN/OUT, DMX IN/OUT) — none of those may emit.
    // r[verify viz.emitter-at-beam-node] - a par lights from its lens, not its sockets
    #[test]
    fn a_pars_emitter_is_at_the_lens_and_fires_out_of_it() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/gdtf");
        for file in [
            "Chauvet@SlimPAR_Pro_Q_USB@Version_1.gdtf",
            "generated/U_King@Uking_Par@ignition.gdtf",
        ] {
            let path = root.join(file);
            if !path.exists() {
                continue;
            }
            let fixture = import_geometry(&path, None).expect("parses");
            let beams = fixture.root.beam_poses();
            assert_eq!(beams.len(), 1, "{file}: exactly one <Beam> node emits");
            let (pos, rot) = beams[0];
            let fwd = rot * Vec3::NEG_Z;
            assert!(
                fwd.abs_diff_eq(Vec3::NEG_Z, 1e-4),
                "{file}: the beam fires straight out of the lens, got {fwd:?}"
            );
            // The lens face is the lowest drawn face of the head; the
            // emitter sits within a few centimetres of it (the Chauvet
            // file puts it 2cm inside the glass, the generated par 2cm
            // proud of it), never up at the yoke or the pigtail.
            let lens_z = lowest_drawn_z(&fixture.root);
            assert!(
                (pos.z - lens_z).abs() < 0.03,
                "{file}: emitter z {} should be at the lens face z {lens_z}",
                pos.z
            );
            assert!(
                pos.z < -0.2,
                "{file}: emitter z {} is up the body, not at the lens",
                pos.z
            );
        }
    }

    // r[verify viz.emitter-at-beam-node] - model-less sockets are not emitters
    #[test]
    fn a_model_less_geometry_node_is_not_an_emitter() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/gdtf/Chauvet@SlimPAR_Pro_Q_USB@Version_1.gdtf");
        if !path.exists() {
            return;
        }
        let fixture = import_geometry(&path, None).expect("parses");
        fn walk(n: &GdtfNode, out: &mut Vec<(String, bool, bool)>) {
            out.push((
                n.name.clone(),
                n.is_beam,
                matches!(n.shape, GdtfShape::None),
            ));
            n.children.iter().for_each(|c| walk(c, out));
        }
        let mut nodes = Vec::new();
        walk(&fixture.root, &mut nodes);
        let undrawn: Vec<_> = nodes.iter().filter(|(_, _, none)| *none).collect();
        assert!(
            undrawn.len() >= 5,
            "the file has the pigtail's four sockets plus the Beam undrawn: {undrawn:?}"
        );
        let beams: Vec<_> = nodes.iter().filter(|(_, b, _)| *b).collect();
        assert_eq!(beams.len(), 1);
        assert_eq!(beams[0].0, "Beam");
    }

    // r[verify viz.gdtf-aliases] - a display scale grows the drawn tree, emitter included
    #[test]
    fn a_display_scale_grows_the_whole_tree_and_keeps_the_emitter_on_the_lens() {
        let (aliases, scales) = parse_aliases(
            r#"{"_comment": "x", "Par": "Uking Par", "_display_scale": {"Mini Gobo Moving Head Light 11ch": 1.5}}"#,
        )
        .unwrap();
        assert_eq!(aliases.get("par").map(String::as_str), Some("ukingpar"));
        assert_eq!(scales.get("minigobomovingheadlight11ch"), Some(&1.5));

        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../data/gdtf/generated/ZKYMZL@Mini_Gobo_Moving_Head_Light_11ch@ignition.gdtf",
        );
        if !path.exists() {
            return;
        }
        let mut fixture = import_geometry(&path, None).unwrap();
        let (lo0, hi0) = fixture.root.assembled_bounds().unwrap();
        fixture.root.scale_by(1.5);
        let (lo, hi) = fixture.root.assembled_bounds().unwrap();
        assert!(((hi - lo) - (hi0 - lo0) * 1.5).abs().max_element() < 1e-4);
        let (beam, _) = fixture.root.beam_poses()[0];
        assert!(
            (beam.z - lo.z).abs() < 0.015,
            "the emitter scaled with the head: {} vs {}",
            beam.z,
            lo.z
        );
    }

    /// The library as shipped: the Norco floor movers are drawn at 1.5x.
    // r[verify viz.gdtf-aliases] - the shipped display scale is applied on load
    #[test]
    fn the_shipped_library_scales_the_mini_gobo_movers() {
        let library = GdtfLibrary::load_default();
        if library.is_empty() {
            return;
        }
        let fixture = library
            .find("Riukoe", "Mini Gobo Moving Head Light 11ch")
            .expect("resolves");
        let (lo, hi) = fixture.root.assembled_bounds().unwrap();
        assert!(
            ((hi.z - lo.z) - 0.247 * 1.5).abs() < 0.005,
            "height {}",
            hi.z - lo.z
        );
    }

    // r[impl viz.gdtf-meshes] - the 3DS chunk walk, on a buffer built by hand
    #[test]
    fn parses_a_hand_built_3ds_chunk_tree() {
        use crate::gdtf_mesh::test_support::build_3ds;
        let verts = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 3.0],
        ];
        let faces = [[0, 1, 2], [0, 2, 3]];
        let bytes = build_3ds("tri", &verts, &faces);
        let raw = parse_3ds(&bytes).expect("parses");
        assert_eq!(raw.positions.len(), 4);
        assert_eq!(raw.indices, vec![0, 1, 2, 0, 2, 3]);
        assert!(raw.normals.is_empty(), "3DS carries no normals");
        let (lo, hi) = raw.bounds().unwrap();
        assert_eq!((lo, hi), (Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0)));

        // Scaled to a Model's dimensions, and flat-shaded on the way in.
        let mesh = raw_to_mesh(&raw, Vec3::new(0.5, 0.5, 0.5));
        let (lo, hi) = mesh_bounds(&mesh);
        assert!(
            (hi - lo - Vec3::splat(0.5)).abs().max_element() < 1e-5,
            "{lo:?} {hi:?}"
        );
        assert!(mesh.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());

        assert!(
            parse_3ds(b"\x00\x00\x06\x00\x00\x00").is_err(),
            "not a 3DS file"
        );
        assert!(parse_3ds(&bytes[..bytes.len() - 5]).is_err(), "truncated");
    }

    // r[impl viz.gdtf-meshes] - a real manufacturer GLB decodes to triangles
    #[test]
    fn decodes_a_real_glb_from_a_gdtf_zip() {
        let Some(path) = real_gdtf_files()
            .into_iter()
            .find(|p| p.to_string_lossy().contains("SlimPAR_Quad_12"))
        else {
            eprintln!("skipping: data/gdtf has no SlimPAR Quad 12 profile");
            return;
        };
        let mut gdtf = GdtfFile::new(File::open(&path).unwrap()).unwrap();
        let mut bytes = Vec::new();
        gdtf.resources
            .read_model_mesh("Body", Model3Format::Gltf, Model3Detail::Default)
            .expect("the file ships models/gltf/Body.glb")
            .read_to_end(&mut bytes)
            .unwrap();
        let raw = parse_glb(&bytes).expect("decodes");
        assert!(!raw.is_empty());
        assert_eq!(raw.indices.len() % 3, 0);
        assert!(
            raw.indices
                .iter()
                .all(|i| (*i as usize) < raw.positions.len())
        );
        let (lo, hi) = raw.bounds().unwrap();
        assert!(
            (hi - lo).min_element() > 0.0,
            "a body has volume: {lo:?} {hi:?}"
        );

        // And through the whole import it lands as a Mesh shape, sized to
        // the Model, not as the box fallback.
        let fixture = import_geometry(&path, None).unwrap();
        let (mut meshes, mut boxes, mut drawn) = (0, 0, 0);
        count_shapes(&fixture.root, &mut meshes, &mut boxes, &mut drawn);
        assert!(
            meshes >= 1,
            "expected real meshes, got {boxes} boxes and {drawn} drawn"
        );
    }

    // r[impl viz.gdtf-meshes] - every shipped profile has something real to draw
    #[test]
    fn every_real_gdtf_file_draws_at_least_one_mesh() {
        let files = real_gdtf_files();
        if files.is_empty() {
            eprintln!("skipping: no data/gdtf directory");
            return;
        }
        for path in files {
            let fixture =
                import_geometry(&path, None).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let (mut meshes, mut boxes, mut drawn) = (0, 0, 0);
            count_shapes(&fixture.root, &mut meshes, &mut boxes, &mut drawn);
            assert!(drawn >= 1, "{}: nothing drawable", path.display());
            // A file that ships a model file, or names one of the spec's
            // mesh primitives, must come out as a real mesh — a box there
            // means a decode fell over. (One that only uses Cube/Cylinder
            // is correctly all boxes and cylinders.)
            let gdtf = GdtfFile::new(File::open(&path).unwrap()).unwrap();
            let expects_mesh = gdtf.description.fixture_types[0].models.iter().any(|m| {
                m.file.as_deref().is_some_and(|f| !f.is_empty())
                    || standard_primitive(m.primitive_type).is_some()
                    || m.primitive_type == PrimitiveType::Sphere
            });
            assert!(
                meshes >= 1 || !expects_mesh,
                "{}: {boxes} boxes and no real mesh",
                path.display()
            );
        }
    }

    // r[impl viz.gdtf-meshes] - Yoke and Head are the spec's meshes, not boxes
    #[test]
    fn standard_primitives_resolve_to_real_meshes() {
        for pt in [
            PrimitiveType::Base,
            PrimitiveType::Base1_1,
            PrimitiveType::Yoke,
            PrimitiveType::Head,
            PrimitiveType::Scanner,
            PrimitiveType::Scanner1_1,
            PrimitiveType::Conventional,
            PrimitiveType::Conventional1_1,
        ] {
            let raw = standard_primitive(pt).unwrap_or_else(|| panic!("{pt:?} has no mesh"));
            assert!(
                raw.indices.len() >= 12,
                "{pt:?} is too small to be a real shape"
            );
            let size = Vec3::new(0.3, 0.2, 0.1);
            let mesh = raw_to_mesh(raw, size);
            let (lo, hi) = mesh_bounds(&mesh);
            assert!(
                (hi - lo - size).abs().max_element() < 1e-4,
                "{pt:?} not scaled to the Model: {lo:?} {hi:?}"
            );
        }
        assert!(standard_primitive(PrimitiveType::Cube).is_none());
    }

    #[test]
    fn primitive_shapes_carry_real_dimensions() {
        let fixture = import_geometry(Path::new(MOVING_HEAD), None).unwrap();
        // Base is PrimitiveType Base, Length/Width/Height 0.3/0.3/0.15:
        // the spec's Base mesh, scaled to exactly that.
        match &fixture.root.shape {
            GdtfShape::Mesh(mesh) => {
                let (lo, hi) = mesh_bounds(mesh);
                let size = hi - lo;
                assert!(
                    (size - Vec3::new(0.3, 0.3, 0.15)).abs().max_element() < 1e-4,
                    "{size:?}"
                );
            }
            _ => panic!("expected Base to resolve to the spec's Base mesh"),
        }
    }

    /// The point of spawning the tree as entities rather than baking it:
    /// rotating the joint the *file* names as the tilt axis moves the
    /// beam and everything else downstream of it, and leaves the base
    /// alone — with no code here composing a single world matrix. Bevy's
    /// transform propagation is the kinematic chain.
    #[test]
    fn tilting_the_files_own_joint_moves_the_beam_and_not_the_base() {
        use bevy::MinimalPlugins;
        use bevy::app::App;

        let fixture = import_geometry(Path::new(MOVING_HEAD), None).unwrap();

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::transform::TransformPlugin);

        // No `AssetPlugin`: this test is about the transform hierarchy,
        // so the mesh store and the material handle are throwaway locals
        // rather than real assets registered in the world.
        let material = Handle::<StandardMaterial>::default();
        let (root, emitters, base) = {
            let world = app.world_mut();
            let root = world
                .spawn((Transform::default(), Visibility::default()))
                .id();
            let mut emitters = Vec::new();
            let mut queue = bevy::ecs::world::CommandQueue::default();
            {
                let mut commands = Commands::new(&mut queue, world);
                let mut meshes = Assets::<Mesh>::default();
                let mut nodes = Vec::new();
                spawn_gdtf_tree(
                    &mut commands,
                    root,
                    &fixture.root,
                    &mut SharedMeshes::new(&mut meshes),
                    &material,
                    &mut emitters,
                    &mut nodes,
                );
            }
            queue.apply(world);
            let base = *world
                .entity(root)
                .get::<Children>()
                .unwrap()
                .first()
                .unwrap();
            (root, emitters, base)
        };
        assert_eq!(
            emitters.len(),
            1,
            "the sample file declares exactly one Beam node"
        );
        {
            // The tree really is drawn, not just transformed: Base, Yoke
            // and Head each resolve to a primitive and get a mesh child,
            // while the Beam node contributes a transform and no mesh.
            let world = app.world_mut();
            let drawn = world.query::<&Mesh3d>().iter(world).count();
            assert_eq!(drawn, 3, "Base, Yoke and Head should each draw a primitive");
            let pans = world
                .query_filtered::<Entity, With<PanJoint>>()
                .iter(world)
                .count();
            let tilts = world
                .query_filtered::<Entity, With<TiltJoint>>()
                .iter(world)
                .count();
            assert_eq!(
                (pans, tilts),
                (1, 1),
                "the file names one pan joint and one tilt joint"
            );
        }
        let beam = emitters[0];

        app.update();
        let world = app.world();
        let idle_beam = world
            .entity(beam)
            .get::<GlobalTransform>()
            .unwrap()
            .translation();
        let idle_base = world
            .entity(base)
            .get::<GlobalTransform>()
            .unwrap()
            .translation();
        // Base -> Yoke(-0.225) -> Head(-0.100) -> Beam(-0.150), all on Z.
        assert!((idle_beam.z - (-0.475)).abs() < 0.001, "{idle_beam:?}");

        // Rotate whichever entity the file marked as the tilt joint.
        let tilt_joint = app
            .world_mut()
            .query_filtered::<Entity, With<TiltJoint>>()
            .iter(app.world())
            .next()
            .expect("the sample file names a tilt joint");
        app.world_mut()
            .entity_mut(tilt_joint)
            .get_mut::<Transform>()
            .unwrap()
            .rotate_x(std::f32::consts::FRAC_PI_2);
        app.update();

        let world = app.world();
        let tilted_beam = world
            .entity(beam)
            .get::<GlobalTransform>()
            .unwrap()
            .translation();
        let tilted_base = world
            .entity(base)
            .get::<GlobalTransform>()
            .unwrap()
            .translation();
        assert!(
            idle_beam.distance(tilted_beam) > 0.01,
            "a 90 degree tilt should move the beam: {idle_beam:?} -> {tilted_beam:?}"
        );
        assert!(
            idle_base.distance(tilted_base) < 1e-6,
            "the base sits above the tilt joint and must not move"
        );
        let _ = root;
    }

    /// Spawns a profile's tree under a root at `mount` in a headless app
    /// with transform propagation and nothing else, and returns the app
    /// with the emitters and the pre-order node entities.
    fn spawn_in_app(
        fixture: &GdtfFixture,
        mount: Transform,
    ) -> (bevy::app::App, Vec<Entity>, Vec<Entity>) {
        use bevy::MinimalPlugins;
        use bevy::app::App;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::transform::TransformPlugin);
        let material = Handle::<StandardMaterial>::default();
        let world = app.world_mut();
        let root = world.spawn((mount, Visibility::default())).id();
        let mut emitters = Vec::new();
        let mut nodes = Vec::new();
        let mut queue = bevy::ecs::world::CommandQueue::default();
        {
            let mut commands = Commands::new(&mut queue, world);
            let mut meshes = Assets::<Mesh>::default();
            spawn_gdtf_tree(
                &mut commands,
                root,
                &fixture.root,
                &mut SharedMeshes::new(&mut meshes),
                &material,
                &mut emitters,
                &mut nodes,
            );
        }
        queue.apply(world);
        (app, emitters, nodes)
    }

    /// The first `<Beam>`'s world pose composed by hand from the file's
    /// tree: mount, then each node's local pose, with the pan and tilt
    /// joints turned by `pan` and `tilt`. This is the arithmetic the
    /// entity hierarchy is supposed to do for us.
    fn expected_beam_pose(
        node: &GdtfNode,
        parent: Transform,
        pan: Quat,
        tilt: Quat,
    ) -> Option<Transform> {
        let mut local = Transform {
            translation: node.local_pos,
            rotation: node.local_rot,
            scale: Vec3::ONE,
        };
        if node.is_pan {
            local.rotation = pan;
        }
        if node.is_tilt {
            local.rotation = tilt;
        }
        let world = parent * local;
        if node.is_beam {
            return Some(world);
        }
        node.children
            .iter()
            .find_map(|c| expected_beam_pose(c, world, pan, tilt))
    }

    fn set_joints(app: &mut bevy::app::App, pan: Quat, tilt: Quat) {
        let world = app.world_mut();
        let pans: Vec<Entity> = world
            .query_filtered::<Entity, With<PanJoint>>()
            .iter(world)
            .collect();
        for e in pans {
            world.entity_mut(e).get_mut::<Transform>().unwrap().rotation = pan;
        }
        let tilts: Vec<Entity> = world
            .query_filtered::<Entity, With<TiltJoint>>()
            .iter(world)
            .collect();
        for e in tilts {
            world.entity_mut(e).get_mut::<Transform>().unwrap().rotation = tilt;
        }
    }

    fn assert_same_pose(actual: &GlobalTransform, expected: Transform, what: &str) {
        let (_, rot, pos) = actual.to_scale_rotation_translation();
        assert!(
            pos.distance(expected.translation) < 1e-4,
            "{what}: emitter at {pos:?}, beam node at {:?}",
            expected.translation
        );
        assert!(
            rot.angle_between(expected.rotation) < 1e-4,
            "{what}: emitter aims {:?}, beam node {:?}",
            rot * Vec3::NEG_Z,
            expected.rotation * Vec3::NEG_Z
        );
    }

    /// A par: the emitter's world transform is the beam node's, through
    /// the mount alone.
    // r[verify viz.one-emitter-tree] - a par's emitter is its <Beam> node, mounted
    #[test]
    fn a_pars_emitter_world_transform_is_its_beam_nodes() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/gdtf/generated/U_King@36_LED_Par_Can@ignition.gdtf");
        if !path.exists() {
            return;
        }
        let fixture = import_geometry(&path, None).unwrap();
        let mount = Transform {
            translation: Vec3::new(1.0, -8.3, 3.25),
            rotation: Quat::from_rotation_x(72.4f32.to_radians()),
            scale: Vec3::ONE,
        };
        let (mut app, emitters, _) = spawn_in_app(&fixture, mount);
        assert_eq!(emitters.len(), 1);
        app.update();
        let expected =
            expected_beam_pose(&fixture.root, mount, Quat::IDENTITY, Quat::IDENTITY).unwrap();
        let actual = app
            .world()
            .entity(emitters[0])
            .get::<GlobalTransform>()
            .unwrap();
        assert_same_pose(actual, expected, "par");
    }

    /// A mover at pan 90 / tilt 45: mount x pan joint x tilt joint x
    /// beam-node local, and nothing else.
    // r[verify viz.one-emitter-tree] - a mover's emitter follows its joints
    #[test]
    fn a_movers_emitter_follows_mount_pan_and_tilt_through_the_tree() {
        let fixture = import_geometry(Path::new(MOVING_HEAD), None).unwrap();
        let mount = Transform {
            translation: Vec3::new(-2.0, 1.5, 0.0),
            rotation: Quat::from_rotation_x(core::f32::consts::PI),
            scale: Vec3::ONE,
        };
        let pan = Quat::from_rotation_z(90f32.to_radians());
        let tilt = Quat::from_rotation_x(45f32.to_radians());
        let (mut app, emitters, _) = spawn_in_app(&fixture, mount);
        set_joints(&mut app, pan, tilt);
        app.update();
        let expected = expected_beam_pose(&fixture.root, mount, pan, tilt).unwrap();
        let actual = app
            .world()
            .entity(emitters[0])
            .get::<GlobalTransform>()
            .unwrap();
        assert_same_pose(actual, expected, "mover at pan 90 / tilt 45");
        // And it genuinely moved: the aim is neither the mount's -Z nor
        // the rest pose's.
        let rest =
            expected_beam_pose(&fixture.root, mount, Quat::IDENTITY, Quat::IDENTITY).unwrap();
        assert!(
            (expected.rotation * Vec3::NEG_Z).dot(rest.rotation * Vec3::NEG_Z) < 0.9,
            "the joints turned the beam"
        );
    }

    /// A display-scaled mover: the scale is baked into the tree's
    /// placements, so the emitter still lands on the (scaled) lens.
    // r[verify viz.one-emitter-tree] - a scaled mover's emitter is still its beam node
    #[test]
    fn a_scaled_movers_emitter_is_still_its_beam_node() {
        let mut fixture = import_geometry(Path::new(MOVING_HEAD), None).unwrap();
        let rest_unscaled = expected_beam_pose(
            &fixture.root,
            Transform::IDENTITY,
            Quat::IDENTITY,
            Quat::IDENTITY,
        )
        .unwrap();
        fixture.root.scale_by(1.5);
        let mount = Transform::from_translation(Vec3::new(0.5, 0.5, 2.0));
        let pan = Quat::from_rotation_z(90f32.to_radians());
        let tilt = Quat::from_rotation_x(45f32.to_radians());
        let (mut app, emitters, _) = spawn_in_app(&fixture, mount);
        set_joints(&mut app, pan, tilt);
        app.update();
        let expected = expected_beam_pose(&fixture.root, mount, pan, tilt).unwrap();
        let actual = app
            .world()
            .entity(emitters[0])
            .get::<GlobalTransform>()
            .unwrap();
        assert_same_pose(actual, expected, "scaled mover");
        let rest_scaled = expected_beam_pose(
            &fixture.root,
            Transform::IDENTITY,
            Quat::IDENTITY,
            Quat::IDENTITY,
        )
        .unwrap();
        assert!(
            (rest_scaled.translation - rest_unscaled.translation * 1.5)
                .abs()
                .max_element()
                < 1e-5,
            "the lens moved out by the display scale"
        );
    }

    /// A bar's cells all hang from one node, and that node is what the
    /// strip emitter is spawned under.
    // r[verify viz.one-emitter-tree] - a bar's cells share one parent node
    #[test]
    fn a_bars_cells_are_siblings_under_one_node() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../data/gdtf/American_DJ@Ultra_Bar_12@Close_-_Needs_Work_on_Strobes_and_Programs.gdtf",
        );
        if !path.exists() {
            return;
        }
        let fixture = import_geometry(&path, None).unwrap();
        let (parent_index, cells) = fixture.root.bar_cells().expect("a bar");
        assert_eq!(cells.len(), 12);
        let (mut app, emitters, nodes) = spawn_in_app(&fixture, Transform::IDENTITY);
        app.update();
        let parent = nodes[parent_index];
        let is_under = |world: &World, mut e: Entity| -> bool {
            for _ in 0..16 {
                let Some(c) = world.entity(e).get::<ChildOf>() else {
                    return false;
                };
                if c.parent() == parent {
                    return true;
                }
                e = c.parent();
            }
            false
        };
        for e in &emitters {
            assert!(
                is_under(app.world(), *e),
                "every cell hangs under the one node"
            );
        }
        // The Ultra Bar pairs its cells two levels down, all under the
        // root: the lowest node over all twelve is the root itself, and
        // no cell is a direct child of it.
        assert_eq!(parent_index, 0);
        assert!(fixture.root.children.iter().all(|c| !c.is_beam));
        // The cells' poses are in that node's frame: composed with its
        // world pose they land on the entities.
        let world = app.world();
        let ancestor = *world.entity(parent).get::<GlobalTransform>().unwrap();
        for (e, (pos, _)) in emitters.iter().zip(&cells) {
            let actual = world
                .entity(*e)
                .get::<GlobalTransform>()
                .unwrap()
                .translation();
            assert!(actual.distance(ancestor.transform_point(*pos)) < 1e-5);
        }
        // A par is not a bar.
        let par = import_geometry(Path::new(MOVING_HEAD), None).unwrap();
        assert!(par.root.bar_cells().is_some_and(|(_, c)| c.len() == 1));
    }

    /// The profile's angles, as the visualizer reads them.
    // r[verify viz.profile-optics] - the generated pars carry beam and field
    #[test]
    fn the_shipped_pars_carry_the_datasheets_angles() {
        let library = GdtfLibrary::load_default();
        if library.is_empty() {
            return;
        }
        let par = library.find("Uking", "Par").expect("resolves");
        assert_eq!(
            par.optics(),
            Some((30.0, 60.0)),
            "36-LED par: 30 beam, field assumed 2x"
        );
        let slim = library
            .find("Chauvet", "SlimPAR Tri 7 IRC 7ch")
            .expect("resolves");
        assert_eq!(
            slim.optics(),
            Some((20.0, 34.0)),
            "SlimPAR: the manual's 20/34"
        );
        let beam = library
            .find("Betopper", "150W LED Beam Moving Head Light")
            .expect("resolves");
        let (b, f) = beam.optics().unwrap();
        assert!(
            (b - 1.72).abs() < 1e-3 && f > b && f < 4.0,
            "beam fixture {b}/{f}"
        );
    }
}
