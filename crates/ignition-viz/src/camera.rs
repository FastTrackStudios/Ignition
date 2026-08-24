use glam::{Mat4, Vec3};

pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub fov_y_deg: f32,
    pub aspect: f32,
    pub z_near: f32,
    pub z_far: f32,
}

impl Camera {
    pub fn view_proj(&self) -> Mat4 {
        let view = glam::camera::rh::view::look_at_mat4(self.eye, self.target, self.up);
        let proj = glam::camera::rh::proj::directx::perspective(
            self.fov_y_deg.to_radians(),
            self.aspect,
            self.z_near,
            self.z_far,
        );
        proj * view
    }

    /// Auto-frame a "front of house" view: eye near the back of the
    /// audience at roughly standing height, looking toward the rig at
    /// somewhere around truss height — the way an operator actually sees
    /// the room, not a top-down look that stares straight through the
    /// ceiling. Works for any venue's bounds (min/max over every object's
    /// centre), not hardcoded to Norco's numbers.
    pub fn frame_house_view(min: Vec3, max: Vec3, aspect: f32) -> Self {
        let size = max - min;
        let center = (min + max) * 0.5;
        let eye = Vec3::new(
            center.x + size.x * 0.15,
            min.y + size.y * 0.06,
            min.z + size.z * 0.28,
        );
        let target = Vec3::new(center.x, center.y, min.z + size.z * 0.55);
        let extent = size.length().max(4.0);
        Self {
            eye,
            target,
            up: Vec3::Z,
            fov_y_deg: 60.0,
            aspect,
            z_near: 0.1,
            z_far: extent * 4.0 + 10.0,
        }
    }

    /// Standing mid-stage, looking back into the house — the performer's-eye
    /// view. The previous version put `eye` at `max.y - size.y*0.10`, which
    /// on the Norco data lands inside/against the upstage wall's booth and
    /// column clutter (a wall a few centimetres from the lens fills the
    /// frame). Pulled well clear of that wall and raised above prop height.
    pub fn frame_stage_view(min: Vec3, max: Vec3, aspect: f32) -> Self {
        let size = max - min;
        let center = (min + max) * 0.5;
        let eye = Vec3::new(
            center.x,
            max.y - size.y * 0.35,
            min.z + size.z * 0.55,
        );
        let target = Vec3::new(center.x, min.y + size.y * 0.15, min.z + size.z * 0.22);
        let extent = size.length().max(4.0);
        Self {
            eye,
            target,
            up: Vec3::Z,
            fov_y_deg: 65.0,
            aspect,
            z_near: 0.1,
            z_far: extent * 4.0 + 10.0,
        }
    }

    /// A close-up on a specific set of points (e.g. the screens' centres),
    /// viewed from the audience side so the front face is visible. `points`
    /// should be world-space centres; `pad` is extra framing margin in
    /// metres on top of the tight bounding box.
    pub fn frame_points(points: &[Vec3], view_from_neg_y: bool, pad: f32, aspect: f32) -> Self {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for &p in points {
            min = min.min(p);
            max = max.max(p);
        }
        let center = (min + max) * 0.5;
        // The diagonal of the *actual* points, not a padded bounding box —
        // padding every axis before measuring inflates the distance badly
        // for a flat/linear cluster (e.g. three fixtures in a row, same
        // y/z: two of the three axes have zero real spread, so padding
        // them dominates the "extent" and pushes the camera much further
        // back than the cluster warrants). `pad` is added once, in metres,
        // to the final distance instead.
        let spread = (max - min).length();
        let distance = spread.max(0.5) * 0.9 + pad;
        let sign = if view_from_neg_y { -1.0 } else { 1.0 };
        // Elevation offset direction depends on how high the cluster
        // already sits, not a fixed sign: an overhead rig (near a ~3.5m
        // ceiling) has no headroom for an elevated vantage, so look up at
        // it from below; a floor-level cluster has no "below" to speak of
        // (the floor is right there), so look down at it from above
        // instead. Either way the offset is a small fixed distance, not
        // scaled by distance — a large-but-tight cluster shouldn't be able
        // to push the eye through the ceiling or the floor.
        let elevation = if center.z > 1.5 { -0.6 } else { 0.6 };
        let eye_z = (center.z + elevation).max(0.3);
        let eye = Vec3::new(center.x, center.y + sign * distance, eye_z);
        let extent = distance + spread;
        Self {
            eye,
            target: center,
            up: Vec3::Z,
            fov_y_deg: 50.0,
            aspect,
            z_near: 0.05,
            z_far: extent * 4.0 + 10.0,
        }
    }

    /// Straight-down plan view — pair with excluding the ceiling from the
    /// scene (`shot --exclude Ceiling`) or it just renders the roof.
    pub fn frame_top_view(min: Vec3, max: Vec3, aspect: f32) -> Self {
        let size = max - min;
        let center = (min + max) * 0.5;
        let extent = size.length().max(4.0);
        let eye = Vec3::new(center.x, center.y, max.z + extent * 0.6);
        Self {
            eye,
            target: Vec3::new(center.x, center.y, min.z),
            up: Vec3::Y,
            fov_y_deg: 45.0,
            aspect,
            z_near: 0.1,
            z_far: extent * 4.0 + 10.0,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub light_dir: [f32; 4],
    pub camera_pos: [f32; 4],
}

impl CameraUniform {
    pub fn from_camera(camera: &Camera) -> Self {
        Self {
            view_proj: camera.view_proj().to_cols_array_2d(),
            light_dir: [-0.4, 0.5, -0.75, 0.0],
            camera_pos: [camera.eye.x, camera.eye.y, camera.eye.z, 1.0],
        }
    }
}
