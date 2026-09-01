//! Readers for the 3D model files a GDTF profile can carry. Two formats,
//! because the spec (gdtf-spec.md, "3D Models") requires a reader to
//! accept both: `models/gltf/<file>.glb` (preferred by the spec, and by
//! most recent uploads) and `models/3ds/<file>.3ds` (what the older half
//! of GDTF Share, and the spec's own standard-primitive meshes, are
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
    pub fn is_empty(&self) -> bool {
        self.indices.len() < 3
    }

    /// Axis-aligned bounds, or `None` for an empty mesh.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.positions.iter().map(|p| Vec3::from(*p));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Appends `other`'s triangles, offsetting its indices.
    fn append(&mut self, other: RawMesh) {
        let base = self.positions.len() as u32;
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
            .extend(other.indices.into_iter().map(|i| i + base));
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

/// Decodes a 3D Studio `.3ds` file: the classic chunk tree
/// `4D4D > 3D3D > 4000 (name) > 4100 > 4110 vertices / 4120 faces /
/// 4160 local axes`. Every mesh object in the file is merged into one
/// [`RawMesh`].
///
/// 3DS stores vertices already in world space — the `0x4160` local-axis
/// matrix is the object's pivot frame for editors, not a transform to
/// apply — so it is parsed (to be well-formed about it) and then ignored,
/// the same choice lib3ds and every viewer make. 3DS is Z-up, matching
/// GDTF, so no axis remap. Units are whatever the author used; the
/// caller scales to the `<Model>` dimensions regardless (the spec says
/// the dimensions always govern).
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
            let rest = &object[(name_end + 1).min(object.len())..];
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
                let count = u16_at(data, 0)? as usize;
                mesh.positions.reserve(count);
                for i in 0..count {
                    let at = 2 + i * 12;
                    mesh.positions.push([
                        f32_at(data, at)?,
                        f32_at(data, at + 4)?,
                        f32_at(data, at + 8)?,
                    ]);
                }
            }
            CHUNK_FACES => {
                let count = u16_at(data, 0)? as usize;
                mesh.indices.reserve(count * 3);
                for i in 0..count {
                    // a, b, c, flags — the flags word is edge visibility.
                    let at = 2 + i * 8;
                    for k in 0..3 {
                        mesh.indices.push(u16_at(data, at + k * 2)? as u32);
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
    let n = mesh.positions.len() as u32;
    if mesh.indices.iter().any(|i| *i >= n) {
        anyhow::bail!("3DS: face index out of range");
    }
    Ok(mesh)
}

/// The chunk starting at `at`: `(id, body)` with the 6-byte header
/// stripped, or `None` if it doesn't fit.
fn chunk(bytes: &[u8], at: usize) -> Option<(u16, &[u8])> {
    let id = u16_at(bytes, at).ok()?;
    let len = u32_at(bytes, at + 2).ok()? as usize;
    if len < 6 || at + len > bytes.len() {
        return None;
    }
    Some((id, &bytes[at + 6..at + len]))
}

/// Iterates the sibling chunks packed back-to-back in `bytes`.
fn chunks(bytes: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut at = 0;
    std::iter::from_fn(move || {
        let (id, body) = chunk(bytes, at)?;
        at += body.len() + 6;
        Some((id, body))
    })
}

fn u16_at(b: &[u8], at: usize) -> anyhow::Result<u16> {
    let s = b
        .get(at..at + 2)
        .ok_or_else(|| anyhow::anyhow!("3DS: truncated chunk"))?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], at: usize) -> anyhow::Result<u32> {
    let s = b
        .get(at..at + 4)
        .ok_or_else(|| anyhow::anyhow!("3DS: truncated chunk"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
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
// r[impl viz.gdtf-meshes] - the GLB's extent is read from its header; bevy_gltf draws it
pub fn glb_bounds(bytes: &[u8]) -> anyhow::Result<Option<(Vec3, Vec3)>> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| anyhow::anyhow!("GLB: {e}"))?;
    let doc = &gltf.document;
    let roots: Vec<gltf::Node> = match doc.default_scene() {
        Some(scene) => scene.nodes().collect(),
        None => doc.scenes().flat_map(|s| s.nodes()).collect(),
    };
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
pub fn y_up_to_z_up(p: Vec3) -> Vec3 {
    Vec3::new(p.x, -p.z, p.y)
}

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
fn vec3_of(v: &serde_json::Value) -> Option<Vec3> {
    let a = v.as_array()?;
    if a.len() < 3 {
        return None;
    }
    Some(Vec3::new(
        a[0].as_f64()? as f32,
        a[1].as_f64()? as f32,
        a[2].as_f64()? as f32,
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
            v.extend(((body.len() + 6) as u32).to_le_bytes());
            v.extend_from_slice(body);
            v
        }
        let mut vbody = (verts.len() as u16).to_le_bytes().to_vec();
        for v in verts {
            for c in v {
                vbody.extend(c.to_le_bytes());
            }
        }
        let mut fbody = (faces.len() as u16).to_le_bytes().to_vec();
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
