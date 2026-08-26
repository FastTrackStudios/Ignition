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
    pub children: Vec<GdtfNode>,
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
    Ok(GdtfFixture {
        fixture_type_name,
        dmx_mode_name,
        root,
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
        let aliases = load_aliases(&dir.join("aliases.json"));
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
fn load_aliases(path: &Path) -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    match serde_json::from_str::<HashMap<String, String>>(&text) {
        Ok(map) => map
            .into_iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .map(|(k, v)| (normalize(&k), normalize(&v)))
            .collect(),
        Err(e) => {
            eprintln!("viz: ignoring {}: {e}", path.display());
            HashMap::new()
        }
    }
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
pub fn spawn_gdtf_tree(
    commands: &mut Commands,
    parent: Entity,
    node: &GdtfNode,
    meshes: &mut Assets<Mesh>,
    material: &Handle<StandardMaterial>,
    emitters: &mut Vec<Entity>,
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

    match node.shape {
        GdtfShape::Box { size } => {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::from_size(size.max(Vec3::splat(0.005))))),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                ChildOf(id),
            ));
        }
        GdtfShape::Cylinder { height, radius } => {
            // Bevy's cylinder stands on +Y; this project's world and
            // GDTF's own geometry are Z-up, so it is laid over.
            commands.spawn((
                Mesh3d(meshes.add(Cylinder::new(radius.max(0.005), height.max(0.005)))),
                MeshMaterial3d(material.clone()),
                Transform::from_rotation(Quat::from_rotation_arc(Vec3::Y, Vec3::Z)),
                ChildOf(id),
            ));
        }
        // Already in the node's Z-up frame and at the model's size: the
        // body material is the venue's, never the file's.
        GdtfShape::Mesh(ref mesh) => {
            commands.spawn((
                Mesh3d(meshes.add(mesh.clone())),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                ChildOf(id),
            ));
        }
        // A `<Beam>` node is the light-exit point, not a housing: nothing
        // is drawn for it, but its transform is exactly where a beam
        // starts and which way it points, which is what the caller wants
        // back. A file may declare more than one (a multi-lens fixture).
        GdtfShape::None => emitters.push(id),
    }

    for child in &node.children {
        spawn_gdtf_tree(commands, id, child, meshes, material, emitters);
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
                spawn_gdtf_tree(
                    &mut commands,
                    root,
                    &fixture.root,
                    &mut meshes,
                    &material,
                    &mut emitters,
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
}
