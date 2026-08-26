//! Decoders for the 3D model files a GDTF profile can carry — the raw
//! triangle soup behind `gdtf_geometry.rs`'s real-mesh shapes. Two
//! formats, because the spec (gdtf-spec.md, "3D Models") requires a
//! reader to accept both: `models/gltf/<file>.glb` (preferred by the
//! spec, and by most recent uploads) and `models/3ds/<file>.3ds` (what
//! the older half of GDTF Share, and the spec's own standard-primitive
//! meshes, are authored in).
//!
//! Both decode into the same [`RawMesh`] in GDTF's own frame — metres,
//! right-handed, Z-up, the fixture hanging with its beam along -Z — so
//! `gdtf_geometry.rs` only has to scale to the `<Model>` dimensions and
//! hand the result to Bevy. 3DS is Z-up natively; glTF is Y-up, so its
//! vertices are remapped `(x, y, z) -> (x, -z, y)` on the way in.
//!
//! Neither decoder reads materials or textures: a fixture body is drawn
//! in the venue's own body material (`spawn.rs`), the same as the
//! primitive fallbacks, and the lens is the emitter, not a mesh.

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
            CHUNK_LOCAL_AXES => {
                // 3x3 axes + origin, 12 floats. Validated for shape only.
                if data.len() < 48 {
                    anyhow::bail!("3DS: truncated local-axes chunk");
                }
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

/// Decodes a binary glTF 2.0 file into one [`RawMesh`], walking every
/// node of the default scene (or all scenes, if none is marked default)
/// and baking each node's world transform into its vertices. Only the
/// embedded BIN buffer is read: the spec forbids external buffers and
/// extensions in a GDTF model, and a file that does need them decodes to
/// whatever primitives don't.
///
/// glTF is Y-up; the result is remapped to GDTF's Z-up.
pub fn parse_glb(bytes: &[u8]) -> anyhow::Result<RawMesh> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| anyhow::anyhow!("GLB: {e}"))?;
    let blob = gltf.blob.as_deref();
    let doc = &gltf.document;

    let mut out = RawMesh::default();
    let roots: Vec<gltf::Node> = match doc.default_scene() {
        Some(scene) => scene.nodes().collect(),
        None => doc.scenes().flat_map(|s| s.nodes()).collect(),
    };
    for node in roots {
        walk_node(&node, Mat4::IDENTITY, blob, &mut out);
    }

    // Y-up -> Z-up: (x, y, z) -> (x, -z, y).
    for p in &mut out.positions {
        *p = [p[0], -p[2], p[1]];
    }
    for n in &mut out.normals {
        *n = [n[0], -n[2], n[1]];
    }
    Ok(out)
}

fn walk_node(node: &gltf::Node, parent: Mat4, blob: Option<&[u8]>, out: &mut RawMesh) {
    let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => blob,
                gltf::buffer::Source::Uri(_) => None,
            });
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let mut part = RawMesh::default();
            part.positions = positions
                .map(|p| world.transform_point3(Vec3::from(p)).to_array())
                .collect();
            if let Some(normals) = reader.read_normals() {
                let normal_mat = world.inverse().transpose();
                part.normals = normals
                    .map(|n| {
                        normal_mat
                            .transform_vector3(Vec3::from(n))
                            .normalize_or_zero()
                            .to_array()
                    })
                    .collect();
                if part.normals.len() != part.positions.len() {
                    part.normals.clear();
                }
            }
            part.indices = match reader.read_indices() {
                Some(idx) => idx.into_u32().collect(),
                None => (0..part.positions.len() as u32).collect(),
            };
            let n = part.positions.len() as u32;
            if part.indices.iter().any(|i| *i >= n) {
                continue;
            }
            out.append(part);
        }
    }
    for child in node.children() {
        walk_node(&child, world, blob, out);
    }
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
