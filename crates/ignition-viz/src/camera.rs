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

    /// Standing on the downstage lip, looking back into the house — the
    /// operator's-eye view flipped: what the performers/screens face.
    pub fn frame_stage_view(min: Vec3, max: Vec3, aspect: f32) -> Self {
        let size = max - min;
        let center = (min + max) * 0.5;
        let eye = Vec3::new(
            center.x - size.x * 0.10,
            max.y - size.y * 0.10,
            min.z + size.z * 0.30,
        );
        let target = Vec3::new(center.x, min.y + size.y * 0.15, min.z + size.z * 0.35);
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
