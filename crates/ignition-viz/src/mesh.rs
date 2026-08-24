//! Minimal CPU-side mesh building: boxes, cones and quads combined into one
//! vertex/index buffer per scene. Object counts here are small (hundreds),
//! so there is no instancing — every object gets its own baked-in-place
//! triangles.

use glam::{Quat, Vec3};

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
}

#[derive(Default)]
pub struct MeshBuilder {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshBuilder {
    fn push_quad(&mut self, p: [Vec3; 4], normal: Vec3, color: [f32; 3]) {
        let base = self.vertices.len() as u32;
        for v in p {
            self.vertices.push(Vertex {
                position: v.into(),
                normal: normal.into(),
                color,
            });
        }
        self.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// A box centred at `center`, rotated by `rot`, with full extents `size`
    /// (not half-extents — matches the extracted venue data's `size` field).
    /// Extents that are ~0 (a wall's thickness axis) get a 2cm floor so the
    /// slab stays visible.
    pub fn add_box(&mut self, center: Vec3, rot: Quat, size: Vec3, color: [f32; 3]) {
        let h = (size * 0.5).max(Vec3::splat(0.01));
        let corners = |sx: f32, sy: f32, sz: f32| center + rot * Vec3::new(sx * h.x, sy * h.y, sz * h.z);
        let faces: [(Vec3, [Vec3; 4]); 6] = [
            (
                rot * Vec3::X,
                [
                    corners(1.0, -1.0, -1.0),
                    corners(1.0, 1.0, -1.0),
                    corners(1.0, 1.0, 1.0),
                    corners(1.0, -1.0, 1.0),
                ],
            ),
            (
                rot * Vec3::NEG_X,
                [
                    corners(-1.0, -1.0, 1.0),
                    corners(-1.0, 1.0, 1.0),
                    corners(-1.0, 1.0, -1.0),
                    corners(-1.0, -1.0, -1.0),
                ],
            ),
            (
                rot * Vec3::Y,
                [
                    corners(-1.0, 1.0, -1.0),
                    corners(-1.0, 1.0, 1.0),
                    corners(1.0, 1.0, 1.0),
                    corners(1.0, 1.0, -1.0),
                ],
            ),
            (
                rot * Vec3::NEG_Y,
                [
                    corners(-1.0, -1.0, 1.0),
                    corners(-1.0, -1.0, -1.0),
                    corners(1.0, -1.0, -1.0),
                    corners(1.0, -1.0, 1.0),
                ],
            ),
            (
                rot * Vec3::Z,
                [
                    corners(-1.0, -1.0, 1.0),
                    corners(1.0, -1.0, 1.0),
                    corners(1.0, 1.0, 1.0),
                    corners(-1.0, 1.0, 1.0),
                ],
            ),
            (
                rot * Vec3::NEG_Z,
                [
                    corners(-1.0, 1.0, -1.0),
                    corners(1.0, 1.0, -1.0),
                    corners(1.0, -1.0, -1.0),
                    corners(-1.0, -1.0, -1.0),
                ],
            ),
        ];
        for (normal, quad) in faces {
            self.push_quad(quad, normal, color);
        }
    }

    /// A single-sided quad — used for screens, facing `+Z` in local space
    /// before `rot` is applied.
    pub fn add_quad(&mut self, center: Vec3, rot: Quat, size: Vec3, color: [f32; 3]) {
        let hx = size.x.max(0.01) * 0.5;
        let hy = size.y.max(0.01) * 0.5;
        let normal = rot * Vec3::Z;
        let p = [
            center + rot * Vec3::new(-hx, -hy, 0.0),
            center + rot * Vec3::new(hx, -hy, 0.0),
            center + rot * Vec3::new(hx, hy, 0.0),
            center + rot * Vec3::new(-hx, hy, 0.0),
        ];
        self.push_quad(p, normal, color);
    }

    /// A fixture marker: a small housing cube at `center`, plus a cone
    /// pointing down the local `-Z` axis (the direction Augment3d fixtures
    /// hang, per `norco-venue-reference.md`) after `rot` is applied,
    /// standing in for the beam until real DMX-driven beams exist.
    pub fn add_fixture(&mut self, center: Vec3, rot: Quat, color: [f32; 3]) {
        self.add_box(center, rot, Vec3::splat(0.18), color);
        self.add_cone(center, rot, Vec3::NEG_Z, 0.9, 0.10, color, 12);
    }

    /// A cone from `origin`, extending `length` along `local_axis` (rotated
    /// by `rot`), base radius `radius`.
    pub fn add_cone(
        &mut self,
        origin: Vec3,
        rot: Quat,
        local_axis: Vec3,
        length: f32,
        radius: f32,
        color: [f32; 3],
        segments: u32,
    ) {
        let tip_local = local_axis.normalize() * length;
        let tip = origin + rot * tip_local;
        let base_center = origin;

        // Build an orthonormal basis for the base circle, perpendicular to
        // local_axis.
        let axis = local_axis.normalize();
        let up = if axis.abs_diff_eq(Vec3::Y, 1e-3) {
            Vec3::X
        } else {
            Vec3::Y
        };
        let u = axis.cross(up).normalize();
        let v = axis.cross(u).normalize();

        let base_start = self.vertices.len() as u32;
        for i in 0..segments {
            let theta = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let local = u * theta.cos() * radius + v * theta.sin() * radius;
            let p = origin + rot * local;
            let normal = rot * local.normalize();
            self.vertices.push(Vertex {
                position: p.into(),
                normal: normal.into(),
                color,
            });
        }
        let tip_normal = rot * axis;
        let tip_idx = self.vertices.len() as u32;
        self.vertices.push(Vertex {
            position: tip.into(),
            normal: tip_normal.into(),
            color,
        });
        for i in 0..segments {
            let a = base_start + i;
            let b = base_start + (i + 1) % segments;
            self.indices.extend_from_slice(&[a, b, tip_idx]);
        }
        let base_normal = rot * (-axis);
        let base_center_idx = self.vertices.len() as u32;
        self.vertices.push(Vertex {
            position: base_center.into(),
            normal: base_normal.into(),
            color,
        });
        for i in 0..segments {
            let a = base_start + i;
            let b = base_start + (i + 1) % segments;
            self.indices.extend_from_slice(&[base_center_idx, b, a]);
        }
    }
}
