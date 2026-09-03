//! A minimal OBJ loader for the converted QLC+ fixture meshes in
//! `assets/qlc-meshes/`.
//!
//! Only handles exactly what those files contain — `v`/`vn`/triangle `f a//a
//! b//b c//c` lines, position and normal always at the same index (the
//! converter that produced these files guarantees that) — not a
//! general-purpose OBJ parser.

use bevy::asset::RenderAssetUsages;
use bevy::math::Vec3;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

use crate::num::{u32_of_usize, usize_of_u32};

pub struct ObjMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
}

impl ObjMesh {
    /// Converts to a Bevy mesh, optionally keeping only the part on one
    /// side of `split_z` (the mesh's own local Z, pre-scale) so a moving
    /// head's yoke and head become two meshes and the head can be a child
    /// entity that tilts under the body.
    ///
    /// A triangle is assigned by its centroid rather than clipped, same
    /// as the pre-Bevy renderer: at these mesh densities a whole-triangle
    /// split is invisible, and clipping would mean retriangulating for no
    /// gain.
    #[must_use]
    pub fn to_bevy_mesh(&self, split: Option<(f32, bool)>) -> Mesh {
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for tri in &self.triangles {
            if let Some((split_z, keep_above)) = split {
                // A position index the converter never actually produces
                // out of range reads as `0.0` here rather than panicking —
                // the mesh is data (see `parse`'s comment on face lines),
                // and a wrong centroid just puts one stray triangle on the
                // wrong side of the split, not a crash.
                let centroid_z = tri
                    .iter()
                    .map(|&i| self.positions.get(usize_of_u32(i)).map_or(0.0, |p| p.z))
                    .sum::<f32>()
                    / 3.0;
                if (centroid_z >= split_z) != keep_above {
                    continue;
                }
            }
            // A position index past the end of the vertex list is
            // malformed mesh data — skip the whole triangle rather than
            // indexing into it and panicking, and rather than pushing a
            // partial triangle that would corrupt the index buffer.
            if tri
                .iter()
                .any(|&i| self.positions.get(usize_of_u32(i)).is_none())
            {
                continue;
            }
            for &i in tri {
                let idx = usize_of_u32(i);
                let Some(&pos) = self.positions.get(idx) else {
                    continue;
                };
                indices.push(u32_of_usize(positions.len()));
                positions.push(pos.into());
                // The converter that produced these files guarantees the
                // normal index matches the position index; a mesh with no
                // normals falls back to +Z rather than panicking.
                let n = self.normals.get(idx).copied().unwrap_or(Vec3::Z);
                normals.push(n.into());
            }
        }

        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_indices(Indices::U32(indices))
    }
}

impl ObjMesh {
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut triangles = Vec::new();

        for line in text.lines() {
            let mut it = line.split_ascii_whitespace();
            match it.next() {
                Some("v") => {
                    let (x, y, z) = three_floats(it);
                    positions.push(Vec3::new(x, y, z));
                }
                Some("vn") => {
                    let (x, y, z) = three_floats(it);
                    normals.push(Vec3::new(x, y, z));
                }
                Some("f") => {
                    // A converted mesh is data, same as a venue or show
                    // file, so a malformed face (a token with no digits
                    // before `//`, or one that doesn't parse) drops the
                    // face rather than taking the whole load down; only
                    // a triangulated face — exactly three usable vertex
                    // indices — is a face this loader draws at all.
                    let idx: Option<Vec<u32>> = it
                        .map(|tok| {
                            let first = tok.split("//").next().unwrap_or(tok);
                            first.parse::<u32>().ok()?.checked_sub(1)
                        })
                        .collect();
                    if let Some(&[a, b, c]) = idx.as_deref() {
                        triangles.push([a, b, c]);
                    }
                }
                _ => {}
            }
        }

        Self {
            positions,
            normals,
            triangles,
        }
    }

    /// Bounding-box half-extent along the largest axis — used to derive a
    /// uniform scale factor toward a target real-world size.
    // `Vec3`'s `Sub`/`Mul` are float component-wise ops with no integer
    // overflow to guard against — `arithmetic_side_effects` fires on any
    // operator-overloaded type, not just the primitive integers the lint
    // is really about (see docs/ops/clippy.md and `view.rs`'s identical
    // suppression).
    #[must_use]
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "Vec3 arithmetic is float, component-wise, and cannot panic or overflow"
    )]
    pub fn max_half_extent(&self) -> f32 {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &p in &self.positions {
            min = min.min(p);
            max = max.max(p);
        }
        ((max - min) * 0.5).max_element()
    }
}

/// A `v`/`vn` line's three numbers. A missing or unparsable component —
/// the file is data, not something this loader gets to assume is
/// well-formed — reads as `0.0` rather than dropping the vertex, since a
/// mesh with one zeroed vertex is still recognisable and a missing one
/// would misalign every index after it.
fn three_floats<'a>(mut it: impl Iterator<Item = &'a str>) -> (f32, f32, f32) {
    let mut next = || it.next().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    (next(), next(), next())
}
