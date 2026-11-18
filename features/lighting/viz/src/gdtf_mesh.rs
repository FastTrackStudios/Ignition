//! Readers for the 3D model files a GDTF profile can carry.
//!
//! Two formats, because the spec (gdtf-spec.md, "3D Models") requires a
//! reader to accept both: `models/gltf/<file>.glb` (preferred by the spec,
//! and by most recent uploads) and `models/3ds/<file>.3ds` (what the older
//! half of GDTF Share, and the spec's own standard-primitive meshes, are
//! authored in).
//!
//! The two are handled differently. 3DS has no Bevy loader, so
//! [`parse_3ds`] decodes it here into a [`RawMesh`] in GDTF's own frame
//! — metres, right-handed, Z-up, the fixture hanging with its beam along
//! -Z — and `gdtf_geometry.rs` scales that to the `<Model>` dimensions
//! and hands it to Bevy. 3DS carries no materials, so the body is drawn
//! in the venue's own body material (`spawn.rs`).
//!
//! GLB is Bevy's own format: `bevy_gltf` loads it, materials and node
//! hierarchy included, through the `gdtf://` asset source in
//! `gdtf_assets.rs`. All this module reads from a GLB is its extent
//! ([`glb_bounds`]) — from the accessor `min`/`max` the format requires —
//! so the importer can size it and stand a box in for it while it loads.
//! glTF is Y-up; the extent is remapped `(x, y, z) -> (x, -z, y)` here
//! and the spawned scene gets the same rotation as a transform.

use bevy::math::{Mat4, Vec3};

/// A decoded model: indexed triangles, positions in metres, Z-up.
/// `normals` is empty when the file carried none (always, for 3DS); the
/// caller flat-shades those.
#[derive(Debug, Default, Clone)]
pub struct RawMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl RawMesh {
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.indices.len() < 3
    }

    /// Axis-aligned bounds, or `None` for an empty mesh.
    #[must_use]
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.positions.iter().map(|p| Vec3::from(*p));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Appends `other`'s triangles, offsetting its indices.
    fn append(&mut self, other: Self) {
        // Saturating rather than exact: a mesh with more than u32::MAX
        // vertices already can't be indexed by this format, so the clamp
        // never actually fires for a real file — it just keeps the
        // conversion audited instead of an inline `as`.
        let base = crate::num::u32_of_usize(self.positions.len());
        // Normals are all-or-nothing: if either side lacks them the
        // combined mesh is flat-shaded later, so don't keep a half set.
        if self.normals.len() == self.positions.len()
            && other.normals.len() == other.positions.len()
        {
            self.normals.extend(other.normals);
        } else {
            self.normals.clear();
        }
        self.positions.extend(other.positions);
        self.indices
            .extend(other.indices.into_iter().map(|i| i.saturating_add(base)));
    }
}

// ---------------------------------------------------------------------
// 3DS
// ---------------------------------------------------------------------

const CHUNK_MAIN: u16 = 0x4D4D;
const CHUNK_EDITOR: u16 = 0x3D3D;
const CHUNK_OBJECT: u16 = 0x4000;
const CHUNK_TRIMESH: u16 = 0x4100;
const CHUNK_VERTICES: u16 = 0x4110;
const CHUNK_FACES: u16 = 0x4120;
const CHUNK_LOCAL_AXES: u16 = 0x4160;

/// Decodes a 3D Studio `.3ds` file: the classic chunk tree `4D4D > 3D3D >
/// 4000 (name) > 4100 > 4110 vertices / 4120 faces / 4160 local axes`.
///
/// Every mesh object in the file is merged into one [`RawMesh`].
///
/// 3DS stores vertices already in world space — the `0x4160` local-axis
/// matrix is the object's pivot frame for editors, not a transform to
/// apply — so it is parsed (to be well-formed about it) and then ignored,
/// the same choice lib3ds and every viewer make. 3DS is Z-up, matching
/// GDTF, so no axis remap. Units are whatever the author used; the
/// caller scales to the `<Model>` dimensions regardless (the spec says
/// the dimensions always govern).
///
/// # Errors
///
/// If `bytes` isn't a well-formed 3DS file (too short, wrong root
/// chunk, or missing the geometry chunks a mesh needs).
pub fn parse_3ds(bytes: &[u8]) -> anyhow::Result<RawMesh> {
    let (id, body) = chunk(bytes, 0).ok_or_else(|| anyhow::anyhow!("3DS: file too short"))?;
    if id != CHUNK_MAIN {
        anyhow::bail!("3DS: not a 3DS file (main chunk 0x{id:04X})");
    }
    let mut out = RawMesh::default();
    for (id, editor) in chunks(body) {
        if id != CHUNK_EDITOR {
            continue;
        }
        for (id, object) in chunks(editor) {
            if id != CHUNK_OBJECT {
                continue;
            }
            // Object chunk: a NUL-terminated name, then sub-chunks.
            let name_end = object.iter().position(|b| *b == 0).unwrap_or(object.len());
            let start = name_end.saturating_add(1).min(object.len());
            let rest = object.get(start..).unwrap_or(&[]);
            for (id, trimesh) in chunks(rest) {
                if id == CHUNK_TRIMESH {
                    out.append(parse_trimesh(trimesh)?);
                }
            }
        }
    }
    Ok(out)
}

fn parse_trimesh(body: &[u8]) -> anyhow::Result<RawMesh> {
    let mut mesh = RawMesh::default();
    for (id, data) in chunks(body) {
        match id {
            CHUNK_VERTICES => {
                let count = usize::from(u16_at(data, 0)?);
                mesh.positions.reserve(count);
                for i in 0..count {
                    // Saturating: `count` is a u16, so `i * 12` never
                    // comes close to overflowing usize on any real
                    // target — this is the audited shape rather than a
                    // bound that ever actually clamps.
                    let at = 2usize.saturating_add(i.saturating_mul(12));
                    mesh.positions.push([
                        f32_at(data, at)?,
                        f32_at(data, at.saturating_add(4))?,
                        f32_at(data, at.saturating_add(8))?,
                    ]);
                }
            }
            CHUNK_FACES => {
                let count = usize::from(u16_at(data, 0)?);
                mesh.indices.reserve(count.saturating_mul(3));
                for i in 0..count {
                    // a, b, c, flags — the flags word is edge visibility.
                    let at = 2usize.saturating_add(i.saturating_mul(8));
                    for k in 0..3usize {
                        mesh.indices
                            .push(u32::from(u16_at(data, at.saturating_add(k.saturating_mul(2)))?));
                    }
                }
                // Sub-chunks (material groups, smoothing) follow the face
                // list inside this same chunk; nothing here needs them.
            }
            CHUNK_LOCAL_AXES
                // 3x3 axes + origin, 12 floats. Validated for shape only.
                if data.len() < 48 => {
                    anyhow::bail!("3DS: truncated local-axes chunk");
                }
            _ => {}
        }
    }
    let n = crate::num::u32_of_usize(mesh.positions.len());
    if mesh.indices.iter().any(|i| *i >= n) {
        anyhow::bail!("3DS: face index out of range");
    }
    Ok(mesh)
}

/// The chunk starting at `at`: `(id, body)` with the 6-byte header
/// stripped, or `None` if it doesn't fit.
fn chunk(bytes: &[u8], at: usize) -> Option<(u16, &[u8])> {
    let id = u16_at(bytes, at).ok()?;
    let len_field = u32_at(bytes, at.checked_add(2)?).ok()?;
    // Widening a u32 length into usize: exact on every target this crate
    // ships for. `unwrap_or(usize::MAX)` only matters on a hypothetical
    // 16-bit usize, where it falls through the `len < 6` and bounds
    // checks below like any other malformed length would.
    let len = usize::try_from(len_field).unwrap_or(usize::MAX);
    if len < 6 {
        return None;
    }
    let end = at.checked_add(len)?;
    if end > bytes.len() {
        return None;
    }
    let body_start = at.checked_add(6)?;
    bytes.get(body_start..end).map(|body| (id, body))
}

/// Iterates the sibling chunks packed back-to-back in `bytes`.
fn chunks(bytes: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let (id, body) = chunk(bytes, at)?;
        // Saturating: a chunk's own bounds check in `chunk()` above
        // already keeps `at` inside `bytes.len()`, so this never actually
        // clamps for a well-formed stream of sibling chunks.
        at = at.saturating_add(body.len()).saturating_add(6);
        Some((id, body))
    })
}

fn u16_at(b: &[u8], at: usize) -> anyhow::Result<u16> {
    let end = at.checked_add(2);
    let s = end
        .and_then(|end| b.get(at..end))
        .ok_or_else(|| anyhow::anyhow!("3DS: truncated chunk"))?;
    let bytes: [u8; 2] = s
        .try_into()
        .map_err(|_| anyhow::anyhow!("3DS: truncated chunk"))?;
    Ok(u16::from_le_bytes(bytes))
}

fn u32_at(b: &[u8], at: usize) -> anyhow::Result<u32> {
    let end = at.checked_add(4);
    let s = end
        .and_then(|end| b.get(at..end))
        .ok_or_else(|| anyhow::anyhow!("3DS: truncated chunk"))?;
    let bytes: [u8; 4] = s
        .try_into()
        .map_err(|_| anyhow::anyhow!("3DS: truncated chunk"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn f32_at(b: &[u8], at: usize) -> anyhow::Result<f32> {
    Ok(f32::from_bits(u32_at(b, at)?))
}

// ---------------------------------------------------------------------
// GLB
// ---------------------------------------------------------------------

/// The bounds of a binary glTF 2.0 file's default scene (or of every
/// scene, if none is marked default), in GDTF's Z-up frame, without
/// decoding a single vertex.
///
/// The mesh itself is not read here any more: `bevy_gltf` loads it —
/// materials, tangents and node hierarchy included — through the
/// `gdtf://` asset source (`gdtf_assets.rs`). What the importer still
/// needs synchronously is the model's extent, to scale it to the
/// `<Model>` dimensions and to place the box that stands in for it until
/// the load lands. glTF requires every `POSITION` accessor to carry its
/// `min`/`max`, so that comes straight out of the JSON header: each
/// primitive's box, transformed by its node's world matrix, unioned.
///
/// `None` when the file has no positioned primitive at all.
///
/// # Errors
///
/// If `bytes` isn't a well-formed GLB/glTF document.
// r[impl viz.gdtf-meshes] - the GLB's extent is read from its header; bevy_gltf draws it
pub fn glb_bounds(bytes: &[u8]) -> anyhow::Result<Option<(Vec3, Vec3)>> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| anyhow::anyhow!("GLB: {e}"))?;
    let doc = &gltf.document;
    let roots: Vec<gltf::Node> = doc.default_scene().map_or_else(
        || doc.scenes().flat_map(|s| s.nodes()).collect(),
        |scene| scene.nodes().collect(),
    );
    let mut acc: Option<(Vec3, Vec3)> = None;
    for node in roots {
        walk_bounds(&node, Mat4::IDENTITY, &mut acc);
    }
    // Y-up -> Z-up: (x, y, z) -> (x, -z, y).
    Ok(acc.map(|(lo, hi)| {
        let corners = [
            Vec3::new(lo.x, lo.y, lo.z),
            Vec3::new(hi.x, lo.y, lo.z),
            Vec3::new(lo.x, hi.y, lo.z),
            Vec3::new(hi.x, hi.y, lo.z),
            Vec3::new(lo.x, lo.y, hi.z),
            Vec3::new(hi.x, lo.y, hi.z),
            Vec3::new(lo.x, hi.y, hi.z),
            Vec3::new(hi.x, hi.y, hi.z),
        ];
        corners
            .into_iter()
            .map(y_up_to_z_up)
            .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), p| (lo.min(p), hi.max(p)))
    }))
}

/// glTF's frame into GDTF's: `(x, y, z) -> (x, -z, y)`. The same
/// permutation `gdtf_assets::GLTF_TO_GDTF` applies to the spawned scene.
#[must_use]
pub fn y_up_to_z_up(p: Vec3) -> Vec3 {
    Vec3::new(p.x, -p.z, p.y)
}

// `Mat4`'s `Mul` is float, component-wise, and cannot panic or overflow —
// `arithmetic_side_effects` fires on any operator-overloaded type, not
// just the primitive integers the lint is really about (see
// docs/ops/clippy.md and `fixture_profile.rs::rotated_z_extent`'s
// identical suppression).
#[expect(
    clippy::arithmetic_side_effects,
    reason = "Mat4 arithmetic is float, component-wise, and cannot panic or overflow"
)]
fn walk_bounds(node: &gltf::Node, parent: Mat4, acc: &mut Option<(Vec3, Vec3)>) {
    let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let Some(positions) = prim.get(&gltf::Semantic::Positions) else {
                continue;
            };
            let (Some(lo), Some(hi)) = (
                positions.min().and_then(|v| vec3_of(&v)),
                positions.max().and_then(|v| vec3_of(&v)),
            ) else {
                continue;
            };
            for corner in [
                Vec3::new(lo.x, lo.y, lo.z),
                Vec3::new(hi.x, lo.y, lo.z),
                Vec3::new(lo.x, hi.y, lo.z),
                Vec3::new(hi.x, hi.y, lo.z),
                Vec3::new(lo.x, lo.y, hi.z),
                Vec3::new(hi.x, lo.y, hi.z),
                Vec3::new(lo.x, hi.y, hi.z),
                Vec3::new(hi.x, hi.y, hi.z),
            ] {
                let w = world.transform_point3(corner);
                *acc = Some(match *acc {
                    Some((lo, hi)) => (lo.min(w), hi.max(w)),
                    None => (w, w),
                });
            }
        }
    }
    for child in node.children() {
        walk_bounds(&child, world, acc);
    }
}

/// An accessor `min`/`max` JSON array as a point.
fn vec3_of(value: &serde_json::Value) -> Option<Vec3> {
    let array = value.as_array()?;
    let first = array.first()?.as_f64()?;
    let second = array.get(1)?.as_f64()?;
    let third = array.get(2)?.as_f64()?;
    Some(Vec3::new(
        crate::num::f32_of_f64(first),
        crate::num::f32_of_f64(second),
        crate::num::f32_of_f64(third),
    ))
}

#[cfg(test)]
pub(crate) mod test_support {
    /// Builds a minimal but well-formed 3DS byte buffer: one object
    /// holding the given vertices and faces, with a local-axes chunk so
    /// the 0x4160 path is exercised too.
    pub fn build_3ds(name: &str, verts: &[[f32; 3]], faces: &[[u16; 3]]) -> Vec<u8> {
        fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
            let mut v = id.to_le_bytes().to_vec();
            v.extend(crate::num::u32_of_usize(body.len().saturating_add(6)).to_le_bytes());
            v.extend_from_slice(body);
            v
        }
        let mut vbody = crate::num::u16_of_usize(verts.len()).to_le_bytes().to_vec();
        for v in verts {
            for c in v {
                vbody.extend(c.to_le_bytes());
            }
        }
        let mut fbody = crate::num::u16_of_usize(faces.len()).to_le_bytes().to_vec();
        for f in faces {
            for i in f {
                fbody.extend(i.to_le_bytes());
            }
            fbody.extend(7u16.to_le_bytes());
        }
        let mut axes = Vec::new();
        for v in [1f32, 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0.] {
            axes.extend(v.to_le_bytes());
        }
        let mut trimesh = chunk(super::CHUNK_VERTICES, &vbody);
        trimesh.extend(chunk(super::CHUNK_FACES, &fbody));
        trimesh.extend(chunk(super::CHUNK_LOCAL_AXES, &axes));
        let mut object = name.as_bytes().to_vec();
        object.push(0);
        object.extend(chunk(super::CHUNK_TRIMESH, &trimesh));
        let editor = chunk(super::CHUNK_OBJECT, &object);
        chunk(super::CHUNK_MAIN, &chunk(super::CHUNK_EDITOR, &editor))
    }
}
