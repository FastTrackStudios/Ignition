//! Minimal CPU-side mesh building: boxes, cones and quads combined into one
//! vertex/index buffer per scene. Object counts here are small (hundreds),
//! so there is no instancing — every object gets its own baked-in-place
//! triangles.

use crate::obj_mesh::ObjMesh;
use glam::{Quat, Vec3};

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 3],
}

/// A live-mode-only light source — a lit fixture's contribution to the
/// scene beyond its own mesh. `live_renderer.rs` uploads these into a
/// storage buffer the fragment shader reads to illuminate *other* geometry
/// (walls, floor) the way a real light does; `renderer.rs` (headless
/// `shot`) never populates or reads this, so regression screenshots are
/// unaffected. `color` is already dimmer-scaled (see `scene.rs`) — the
/// shader doesn't do its own intensity math.
#[derive(Debug, Clone, Copy)]
pub struct PointLight {
    pub position: Vec3,
    pub color: [f32; 3],
}

#[derive(Default)]
pub struct MeshBuilder {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Additively-blended, unlit geometry — beam cones for lit fixtures.
    /// Drawn in a second pass in `live_renderer.rs` after the main opaque
    /// pass, with depth write off so overlapping beams blend rather than
    /// z-fight; `renderer.rs`'s headless path never draws this list.
    pub glow_vertices: Vec<Vertex>,
    pub glow_indices: Vec<u32>,
    pub lights: Vec<PointLight>,
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

    /// A cylinder centred at `center`, its axis along local `-Z` (the
    /// fixture-hang direction) after `rot`, `height` tall, `radius` wide,
    /// with flat caps at each end.
    pub fn add_cylinder(
        &mut self,
        center: Vec3,
        rot: Quat,
        height: f32,
        radius: f32,
        color: [f32; 3],
        segments: u32,
    ) {
        let axis = Vec3::NEG_Z;
        let up = Vec3::Y;
        let u = axis.cross(up).normalize();
        let v = axis.cross(u).normalize();
        let half = height * 0.5;
        let front = axis * half; // local -Z end (the lens/business end)
        let back = -front;

        let mut ring = |offset: Vec3| -> u32 {
            let start = self.vertices.len() as u32;
            for i in 0..segments {
                let theta = (i as f32 / segments as f32) * std::f32::consts::TAU;
                let local = offset + u * theta.cos() * radius + v * theta.sin() * radius;
                let normal_local = (u * theta.cos() + v * theta.sin()).normalize();
                self.vertices.push(Vertex {
                    position: (center + rot * local).into(),
                    normal: (rot * normal_local).into(),
                    color,
                });
            }
            start
        };
        let front_ring = ring(front);
        let back_ring = ring(back);
        for i in 0..segments {
            let j = (i + 1) % segments;
            let a = front_ring + i;
            let b = front_ring + j;
            let c = back_ring + j;
            let d = back_ring + i;
            self.indices.extend_from_slice(&[a, b, c, a, c, d]);
        }

        // Caps.
        let mut cap = |offset: Vec3, ring_start: u32, outward: Vec3, flip: bool| {
            let center_idx = self.vertices.len() as u32;
            self.vertices.push(Vertex {
                position: (center + rot * offset).into(),
                normal: (rot * outward).into(),
                color,
            });
            for i in 0..segments {
                let j = (i + 1) % segments;
                let (a, b) = if flip { (j, i) } else { (i, j) };
                self.indices
                    .extend_from_slice(&[center_idx, ring_start + a, ring_start + b]);
            }
        };
        cap(front, front_ring, axis, false);
        cap(back, back_ring, -axis, true);
    }

    /// A fixture marker: a small housing cube at `center`, plus a cone
    /// pointing down the local `-Z` axis (the direction Augment3d fixtures
    /// hang, per `norco-venue-reference.md`) after `rot` is applied,
    /// standing in for the beam until real DMX-driven beams exist. Kept as
    /// the fallback shape for any fixture type without a dedicated builder
    /// in `scene.rs`.
    pub fn add_fixture(&mut self, center: Vec3, rot: Quat, color: [f32; 3]) {
        self.add_box(center, rot, Vec3::splat(0.18), color);
        self.add_cone(center, rot, Vec3::NEG_Z, 0.3, 0.09, color, 12);
    }

    /// A par can: a squat cylindrical housing plus a short beam stub, both
    /// sized proportionally to the real fixture rather than one fixed size
    /// for every fixture type. `diameter`/`depth` are the housing's real
    /// dimensions (metres); the beam stub scales off `diameter` so a small
    /// par doesn't carry the same spike as a large one.
    pub fn add_par_can(&mut self, center: Vec3, rot: Quat, diameter: f32, depth: f32, color: [f32; 3]) {
        self.add_cylinder(center, rot, depth, diameter * 0.5, color, 16);
        let beam_len = (diameter * 1.6).max(0.15);
        let front = rot * (Vec3::NEG_Z * (depth * 0.5));
        self.add_cone(center + front, rot, Vec3::NEG_Z, beam_len, diameter * 0.22, color, 12);
    }

    /// A moving head: a small yoke base plus an offset head housing, sized
    /// from the real fixture's footprint/height rather than a fixed marker.
    /// `base_size` is the mounting bracket/yoke box (roughly square in X/Y,
    /// thin in Z); `head_size` is the moving head housing; `drop` is how far
    /// below the mounting point the head sits (its own height plus the
    /// yoke's reach). All local -Z is "down" from the mount, matching the
    /// fixture's stored hang orientation.
    pub fn add_moving_head(
        &mut self,
        center: Vec3,
        rot: Quat,
        base_size: Vec3,
        head_size: Vec3,
        drop: f32,
        color: [f32; 3],
    ) {
        self.add_box(center, rot, base_size, color);
        let head_center = center + rot * (Vec3::NEG_Z * drop);
        self.add_box(head_center, rot, head_size, color);
        let beam_len = (head_size.z.max(head_size.x) * 1.4).max(0.12);
        let front = rot * (Vec3::NEG_Z * (head_size.z * 0.5));
        self.add_cone(
            head_center + front,
            rot,
            Vec3::NEG_Z,
            beam_len,
            head_size.x.min(head_size.y) * 0.28,
            color,
            10,
        );
    }

    /// An elongated bar/batten fixture (an LED strip in a metal housing) —
    /// just a box sized to the real fixture's (length, width, height). No
    /// beam cone: a wash bar reads as a light source along its whole
    /// length, not a single point.
    pub fn add_bar(&mut self, center: Vec3, rot: Quat, length: f32, width: f32, height: f32, color: [f32; 3]) {
        self.add_box(center, rot, Vec3::new(length, width, height), color);
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
        build_cone(&mut self.vertices, &mut self.indices, origin, rot, local_axis, length, radius, color, segments);
    }

    /// A beam-cone light glow — same shape as `add_cone`, but appended to
    /// the separate additively-blended `glow_vertices`/`glow_indices` list
    /// instead of the main opaque geometry. See `PointLight` for why this
    /// list exists.
    pub fn add_glow_cone(
        &mut self,
        origin: Vec3,
        rot: Quat,
        local_axis: Vec3,
        length: f32,
        radius: f32,
        color: [f32; 3],
        segments: u32,
    ) {
        build_cone(
            &mut self.glow_vertices,
            &mut self.glow_indices,
            origin,
            rot,
            local_axis,
            length,
            radius,
            color,
            segments,
        );
    }

    /// Registers a live light source for `live_renderer.rs`'s point-light
    /// pass — see `PointLight`.
    pub fn add_light(&mut self, position: Vec3, color: [f32; 3]) {
        self.lights.push(PointLight { position, color });
    }
}

fn build_cone(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    rot: Quat,
    local_axis: Vec3,
    length: f32,
    radius: f32,
    color: [f32; 3],
    segments: u32,
) {
    {
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

        let base_start = vertices.len() as u32;
        for i in 0..segments {
            let theta = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let local = u * theta.cos() * radius + v * theta.sin() * radius;
            let p = origin + rot * local;
            let normal = rot * local.normalize();
            vertices.push(Vertex {
                position: p.into(),
                normal: normal.into(),
                color,
            });
        }
        let tip_normal = rot * axis;
        let tip_idx = vertices.len() as u32;
        vertices.push(Vertex {
            position: tip.into(),
            normal: tip_normal.into(),
            color,
        });
        for i in 0..segments {
            let a = base_start + i;
            let b = base_start + (i + 1) % segments;
            indices.extend_from_slice(&[a, b, tip_idx]);
        }
        let base_normal = rot * (-axis);
        let base_center_idx = vertices.len() as u32;
        vertices.push(Vertex {
            position: base_center.into(),
            normal: base_normal.into(),
            color,
        });
        for i in 0..segments {
            let a = base_start + i;
            let b = base_start + (i + 1) % segments;
            indices.extend_from_slice(&[base_center_idx, b, a]);
        }
    }
}

impl MeshBuilder {
    /// Bakes an imported mesh (a real fixture shape, see
    /// `assets/qlc-meshes/`) into the scene at `center`, rotated by `rot`
    /// and uniformly scaled by `scale` — the asset's own local space is
    /// already in this crate's convention (Z-up, local -Z = beam/hang
    /// direction) via the conversion step, so no extra basis change is
    /// needed here, only placement.
    pub fn add_mesh_asset(&mut self, center: Vec3, rot: Quat, scale: f32, obj: &ObjMesh, color: [f32; 3]) {
        let base = self.vertices.len() as u32;
        for (p, n) in obj.positions.iter().zip(&obj.normals) {
            self.vertices.push(Vertex {
                position: (center + rot * (*p * scale)).into(),
                normal: (rot * *n).into(),
                color,
            });
        }
        for tri in &obj.triangles {
            self.indices
                .extend_from_slice(&[base + tri[0], base + tri[1], base + tri[2]]);
        }
    }
}
