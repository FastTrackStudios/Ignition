//! A minimal OBJ loader for the converted QLC+ fixture meshes in
//! `assets/qlc-meshes/`. Only handles exactly what those files contain —
//! `v`/`vn`/triangle `f a//a b//b c//c` lines, position and normal always
//! at the same index (the converter that produced these files guarantees
//! that) — not a general-purpose OBJ parser.

use bevy::asset::RenderAssetUsages;
use bevy::math::Vec3;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

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
    pub fn to_bevy_mesh(&self, split: Option<(f32, bool)>) -> Mesh {
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for tri in &self.triangles {
            if let Some((split_z, keep_above)) = split {
                let centroid_z = tri
                    .iter()
                    .map(|&i| self.positions[i as usize].z)
                    .sum::<f32>()
                    / 3.0;
                if (centroid_z >= split_z) != keep_above {
                    continue;
                }
            }
            for &i in tri {
                indices.push(positions.len() as u32);
                positions.push(self.positions[i as usize].into());
                // The converter that produced these files guarantees the
                // normal index matches the position index; a mesh with no
                // normals falls back to +Z rather than panicking.
                let n = self.normals.get(i as usize).copied().unwrap_or(Vec3::Z);
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
                    let idx: Vec<u32> = it
                        .map(|tok| {
                            let first = tok.split("//").next().unwrap();
                            first.parse::<u32>().unwrap() - 1
                        })
                        .collect();
                    assert_eq!(idx.len(), 3, "obj_mesh only supports triangulated faces");
                    triangles.push([idx[0], idx[1], idx[2]]);
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

fn three_floats<'a>(mut it: impl Iterator<Item = &'a str>) -> (f32, f32, f32) {
    let x = it.next().unwrap().parse().unwrap();
    let y = it.next().unwrap().parse().unwrap();
    let z = it.next().unwrap().parse().unwrap();
    (x, y, z)
}
