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

/// The beam-glow pass's own vertex format — deliberately *not* `Vertex`.
///
/// `Vertex` carries a pre-shaded colour, which is all opaque geometry
/// needs; a beam needs the shader to evaluate its brightness per-fragment
/// from where the fragment sits inside the beam, which means shipping the
/// beam's own parameters down with every vertex. These are exactly ASLS
/// Studio's five per-instance beam attributes (`beam.vertex.glsl`'s
/// `wpos`/`direction`/`color`/`intensity`/`angle`) plus the beam-local
/// position their vertex shader derives from the un-transformed cylinder
/// (`vPosition`) — the difference being that Ignition bakes real
/// world-space cone geometry on the CPU (see this module's header) rather
/// than widening one shared cylinder per instance on the GPU, so the
/// per-beam values ride per-vertex instead of per-instance. Same values
/// reach the fragment shader either way.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GlowVertex {
    /// World-space position of this vertex on the cone's surface.
    pub position: [f32; 3],
    /// Outward radial normal of the cone wall, world space. The glow
    /// shader's `anglePower` term uses it one-sided (`max(dot(v, n), 0)`),
    /// which is what makes a beam's *near* wall draw and its far wall
    /// contribute nothing — ASLS gets the same one-sidedness out of the
    /// sign of a screen-space derived normal; doing it from a real
    /// outward normal is the same result without depending on which way
    /// the rasteriser's Y axis runs.
    pub normal: [f32; 3],
    /// Position relative to the beam's origin (the fixture's lens), in
    /// world *scale* but beam-local *origin* — so `length()` of it is the
    /// real distance travelled down the beam, which is what the falloff
    /// curve is a function of. ASLS's `vPosition`.
    pub beam_local: [f32; 3],
    /// The beam's colour, already dimmer-scaled by `scene.rs`, and NOT
    /// pre-attenuated: all falloff happens per-fragment now. (ASLS keeps
    /// colour and dimmer as separate attributes and multiplies them in
    /// the shader; multiplying on the CPU is the same product.)
    pub color: [f32; 3],
    /// World-space beam aim direction, normalized. ASLS's `direction`.
    pub direction: [f32; 3],
    /// World-space position of the fixture's lens — where the beam starts.
    /// ASLS's `wpos`/`beamPos`; used for the view-alignment term, which is
    /// per-beam (not per-fragment), so it must come from the beam's origin
    /// rather than this vertex's own position.
    pub origin: [f32; 3],
    /// The real fixture's beam angle in degrees. ASLS's `angle.x` — it is
    /// the quadratic term of the falloff curve, so a wide wash genuinely
    /// dies off over a shorter distance than a tight spot.
    pub angle_deg: f32,
    /// Keeps the struct 16-byte aligned so `bytemuck::cast_slice` and the
    /// vertex layout agree on stride without an implicit tail pad.
    pub _pad: f32,
}

/// A live-mode-only light source — a lit fixture's contribution to the
/// scene beyond its own mesh. `live_pipeline.rs` uploads these into a
/// storage buffer the fragment shader reads to illuminate *other* geometry
/// (walls, floor) the way a real light does; `renderer.rs` (headless
/// `shot`) never populates or reads this, so regression screenshots are
/// unaffected. `color` is already dimmer-scaled (see `scene.rs`) — the
/// shader doesn't do its own intensity math.
///
/// Modelled as a real cone-angled spotlight, not an omnidirectional point
/// light — confirmed against ASLS Studio's own visualizer source
/// (`docs/research/lighting-console-landscape.md`'s Slice 7): a real
/// fixture's spill only falls within roughly its own beam angle, and an
/// omnidirectional light was lighting surfaces a real fixture never would
/// (e.g. the wall directly behind a fixture aimed the other way).
#[derive(Debug, Clone, Copy)]
pub struct PointLight {
    pub position: Vec3,
    pub color: [f32; 3],
    /// Normalized aim direction — world space, matching the beam cone's
    /// own axis (`fixture_profile.rs::emit_light_and_beam`).
    pub direction: Vec3,
    /// Half-angle of the light's cone, in degrees (the real fixture's beam
    /// angle) — matches the beam cone geometry's own spread.
    pub cone_half_angle_deg: f32,
}

#[derive(Default)]
pub struct MeshBuilder {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Additively-blended, unlit geometry — beam cones for lit fixtures.
    /// Drawn in a second pass in `live_renderer.rs` after the main opaque
    /// pass, with depth write off so overlapping beams blend rather than
    /// z-fight; `renderer.rs`'s headless path never draws this list.
    pub glow_vertices: Vec<GlowVertex>,
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

    /// A beam-cone light glow — appended to the separate
    /// additively-blended `glow_vertices`/`glow_indices` list instead of
    /// the main opaque geometry (see `PointLight` for why that list
    /// exists). Unlike `add_cone`'s decorative housing-stub shape (base at
    /// `origin`, tapering to a point — fine for a small fixed-length
    /// ~0.15m stub, never really scrutinised), this is a real frustum:
    /// narrow where the beam exits the fixture, flaring out to `radius` at
    /// the far end — the correct physical shape for a spotlight beam.
    ///
    /// This carries *no* baked brightness: the geometry is the beam's
    /// envelope and nothing more. Every falloff term — down-the-beam
    /// distance, view alignment, the soft silhouette edge, haze density —
    /// is evaluated per-fragment by `live_shader.wgsl`'s `fs_glow`, a
    /// direct port of ASLS Studio's `beam.fragment.glsl`. `angle_deg` is
    /// the real fixture's beam angle, which that curve needs as its
    /// quadratic term.
    #[allow(clippy::too_many_arguments)]
    pub fn add_glow_cone(
        &mut self,
        origin: Vec3,
        rot: Quat,
        local_axis: Vec3,
        length: f32,
        radius: f32,
        color: [f32; 3],
        angle_deg: f32,
        segments: u32,
    ) {
        build_glow_cone(
            &mut self.glow_vertices,
            &mut self.glow_indices,
            origin,
            rot,
            local_axis,
            length,
            radius,
            color,
            angle_deg,
            segments,
        );
    }

    /// Registers a live light source for `live_renderer.rs`'s point-light
    /// pass — see `PointLight`.
    pub fn add_light(&mut self, position: Vec3, color: [f32; 3], direction: Vec3, cone_half_angle_deg: f32) {
        self.lights.push(PointLight { position, color, direction: direction.normalize(), cone_half_angle_deg });
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

/// How many rings of vertices a glow cone gets along its length. Two —
/// the near ring at the lens and the far ring where the beam lands — is
/// all the *shape* needs, because a frustum's wall is planar between
/// them and `GlowVertex::beam_local` interpolates exactly across it. The
/// six rings this used to have existed only to approximate a falloff
/// curve in baked vertex colours; the curve is evaluated per-fragment
/// now, so the extra rings bought nothing. (ASLS's own beam cylinder is
/// `BEAM_SEGMENTS = 1`, i.e. the same two rings, for the same reason.)
const GLOW_CONE_RING_STEPS: u32 = 1;

/// A beam-glow frustum — a narrow ring at `origin` flaring to `radius`
/// at the far end, `length` away along `local_axis`.
///
/// Pure envelope geometry: no brightness is baked anywhere in here. Each
/// vertex carries the beam's own parameters (see `GlowVertex`) and
/// `fs_glow` does all the shading per-fragment. `angle_deg` is the real
/// fixture's beam angle, which that shader's falloff curve needs.
#[allow(clippy::too_many_arguments)]
fn build_glow_cone(
    vertices: &mut Vec<GlowVertex>,
    indices: &mut Vec<u32>,
    origin: Vec3,
    rot: Quat,
    local_axis: Vec3,
    length: f32,
    radius: f32,
    color: [f32; 3],
    angle_deg: f32,
    segments: u32,
) {
    let axis = local_axis.normalize();
    let up = if axis.abs_diff_eq(Vec3::Y, 1e-3) { Vec3::X } else { Vec3::Y };
    let u = axis.cross(up).normalize();
    let v = axis.cross(u).normalize();

    // A small non-zero near radius (rather than a true point) so the near
    // end reads as a lens-sized aperture up close instead of a degenerate
    // vertex fan — ASLS does the same thing with a fixed 9cm
    // `BEAM_TOP_RADIUS` on their cylinder's top cap.
    let near_radius = (radius * 0.08).max(0.02);

    let world_direction = (rot * axis).normalize();

    let mut ring = |dist: f32, ring_radius: f32| -> u32 {
        let start = vertices.len() as u32;
        let offset = axis * dist;
        for i in 0..segments {
            let theta = (i as f32 / segments as f32) * std::f32::consts::TAU;
            let local = offset + u * theta.cos() * ring_radius + v * theta.sin() * ring_radius;
            let normal_local = (u * theta.cos() + v * theta.sin()).normalize();
            // `beam_local` is the vertex offset from the lens in world
            // *scale* (rotated, not translated) — so its length is the
            // real metres travelled down the beam, whatever the fixture's
            // orientation.
            let beam_local = rot * local;
            vertices.push(GlowVertex {
                position: (origin + beam_local).into(),
                normal: (rot * normal_local).into(),
                beam_local: beam_local.into(),
                color,
                direction: world_direction.into(),
                origin: origin.into(),
                angle_deg,
                _pad: 0.0,
            });
        }
        start
    };

    let mut prev_ring: Option<u32> = None;
    for step in 0..=GLOW_CONE_RING_STEPS {
        let t = step as f32 / GLOW_CONE_RING_STEPS as f32;
        let dist = t * length;
        let ring_radius = near_radius + (radius - near_radius) * t;
        let this_ring = ring(dist, ring_radius);
        if let Some(prev) = prev_ring {
            for i in 0..segments {
                let j = (i + 1) % segments;
                let a = prev + i;
                let b = prev + j;
                let c = this_ring + j;
                let d = this_ring + i;
                indices.extend_from_slice(&[a, b, c, a, c, d]);
            }
        }
        prev_ring = Some(this_ring);
    }

    // No end caps. `fs_glow` shades one-sided off the wall's outward
    // normal, and a cap's normal points down the beam — it would only
    // ever contribute when staring straight into the fixture, where it
    // reads as a flat disc rather than light. The wall alone covers the
    // beam's whole silhouette.
    let _ = prev_ring;
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

    /// Like `add_mesh_asset`, but each triangle is rotated by `base_rot` or
    /// `head_rot` depending on which side of `split_z` (the mesh's own
    /// local, unscaled Z — see `ObjMesh`) its centroid falls on — a
    /// yoke/head split for moving-head fixtures so a live tilt reading
    /// only rotates the head, not the whole fixture body (see
    /// `fixture_profile.rs`'s `Shape::Mesh::split_z` and
    /// `docs/research/lighting-console-landscape.md`'s Slice 10). Emits
    /// fresh vertices per triangle instead of reusing `obj`'s shared
    /// position/normal indices — a vertex straddling the split boundary
    /// would otherwise need two different rotated positions depending on
    /// which triangle it's part of, which shared indexing can't express.
    /// This mesh is small (~1000 vertices) so the duplication cost is
    /// negligible.
    /// `outer_rot` (mount + live pan) applies to every vertex, base and
    /// head alike — a real yoke rotates on its mount when panning. `tilt`
    /// applies only to head-side vertices, and pivots around `split_z`
    /// (transformed into `pre_rotate`'s frame) instead of the mesh's own
    /// local origin — the neck the head actually tilts from, not the
    /// fixture's base anchor point far below it. `pre_rotate` is the same
    /// mesh-convention fix `add_mesh_asset` folds into its caller's `rot`;
    /// it has to be applied explicitly here (before the pivot subtraction)
    /// since the pivot itself needs to live in the same rotated space as
    /// the vertices it's offsetting.
    #[allow(clippy::too_many_arguments)]
    pub fn add_mesh_asset_split(
        &mut self,
        center: Vec3,
        outer_rot: Quat,
        tilt: Quat,
        pre_rotate: Quat,
        scale: f32,
        obj: &ObjMesh,
        split_z: f32,
        color: [f32; 3],
    ) {
        let pivot_pre = pre_rotate * (Vec3::new(0.0, 0.0, split_z) * scale);
        for tri in &obj.triangles {
            let centroid_z = (obj.positions[tri[0] as usize].z
                + obj.positions[tri[1] as usize].z
                + obj.positions[tri[2] as usize].z)
                / 3.0;
            let is_head = centroid_z >= split_z;
            let base_idx = self.vertices.len() as u32;
            for &vi in tri {
                let p_pre = pre_rotate * (obj.positions[vi as usize] * scale);
                let n_pre = pre_rotate * obj.normals[vi as usize];
                let (p_final, n_final) = if is_head {
                    (pivot_pre + tilt * (p_pre - pivot_pre), tilt * n_pre)
                } else {
                    (p_pre, n_pre)
                };
                self.vertices.push(Vertex {
                    position: (center + outer_rot * p_final).into(),
                    normal: (outer_rot * n_final).into(),
                    color,
                });
            }
            self.indices.extend_from_slice(&[base_idx, base_idx + 1, base_idx + 2]);
        }
    }
}
