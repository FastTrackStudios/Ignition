//! Beams in the air, drawn analytically: no grid, no march steps and no
//! dependence on earlier frames, so no blocks at any pan or tilt speed or
//! strobe rate.
//!
//! Add [`BeamsPlugin`], put a [`BeamCamera`] on a 3D camera and a [`Beam`]
//! on each fixture's emitter. Each beam is one instanced cone hull that
//! only decides which pixels run; each of those pixels intersects its view
//! ray with the exact cone, cuts the segment at the first surface in the
//! depth buffer and integrates single scattering along it in closed form.
//! `README.md` has the design, the tiers, what was ported from ASLS Studio
//! and what is not built yet.
//!
//! The mathematics is in [`math`], on the CPU and tested;
//! `shaders/scatter.wesl` is the same arithmetic for the GPU.

pub mod beam;
pub mod gobo;
pub mod math;
pub mod quality;
pub mod render;

use bevy::{
    app::{App, Plugin, Update},
    asset::{AssetServer, Assets, Handle, embedded_asset},
    camera::Camera3d,
    core_pipeline::{Core3dSystems, core_3d::prepare_core_3d_depth_textures, schedule::Core3d},
    ecs::{
        component::Component,
        resource::Resource,
        schedule::IntoScheduleConfigs,
        system::{Query, Res},
    },
    image::Image,
    log::info,
    render::{
        ExtractSchedule, GpuResourceAppExt, Render, RenderApp, RenderStartup, RenderSystems,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        render_resource::SpecializedRenderPipelines,
    },
    shader::load_shader_library,
    time::{Real, Time},
};

pub use beam::{Beam, Gobo};
pub use gobo::{GoboLayer, gobo_image};
pub use quality::{AirResolution, BeamTier, GoboInAir, Governor, Quality};

/// Draws every [`Beam`] in the air for every camera with a [`BeamCamera`].
#[derive(Default)]
pub struct BeamsPlugin;

/// The gobo texture every beam's [`Gobo::slot`] indexes: an array with
/// mips, as [`gobo_image`] builds. The plugin starts it as one open layer.
#[derive(Resource, Clone, Debug)]
pub struct BeamGobos(pub Handle<Image>);

/// The haze the beams shine through. Uniform across the room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Haze {
    /// How much haze is in the air, 0 (none: nothing shows) to 1.
    pub density: f32,
    /// Light scattered per metre at full density. With the beams' real
    /// candela and the camera's real exposure this is the one number that
    /// says how bright the air is.
    pub scattering_at_full: f32,
    /// Henyey-Greenstein `g`: how much brighter a beam is looked into than
    /// seen from the side.
    pub anisotropy: f32,
}

impl Default for Haze {
    fn default() -> Self {
        Self {
            density: 1.0,
            scattering_at_full: 0.02,
            anisotropy: 0.3,
        }
    }
}

impl Haze {
    /// Light scattered per metre of path.
    #[must_use]
    pub fn scattering(&self) -> f32 {
        self.density.clamp(0.0, 1.0) * self.scattering_at_full.max(0.0)
    }

    /// Light lost per metre of path. Haze absorbs almost nothing, so this
    /// is what it scatters.
    #[must_use]
    pub fn extinction(&self) -> f32 {
        self.scattering()
    }
}

/// How the summed beams' highlights are shaped before they join the
/// picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Highlights {
    /// The brightness (exposed, brightest channel) past which a beam
    /// starts to pale towards white.
    pub white_from: f32,
    /// How fast it pales past that.
    pub white_rate: f32,
    /// Also compress the brightest channel to below 1. For a camera with
    /// no tonemapper after the beams; leave off when there is one.
    pub compress: bool,
    /// The dither's amplitude, as a fraction of the display's range.
    /// Below one 8-bit step, so a faint beam end fades instead of banding.
    pub dither: f32,
}

impl Default for Highlights {
    fn default() -> Self {
        Self {
            white_from: 1.0,
            white_rate: 0.6,
            compress: false,
            dither: 1.0 / 255.0,
        }
    }
}

/// Put on a 3D camera to have it see beams in the air.
///
/// The camera must have `Msaa::Off`: the beams read its depth buffer, and a
/// multisampled camera gets no beams.
#[derive(Component, Clone, Debug, PartialEq, ExtractComponent)]
#[extract_app(RenderApp)]
#[require(Camera3d)]
pub struct BeamCamera {
    pub quality: Quality,
    pub haze: Haze,
    pub highlights: Highlights,
    /// How far the highest tiers' gobo samples are jittered inside their
    /// stretch of the ray, 0 (evenly spaced, the default) to 1. The
    /// samples are prefiltered, so they need no jitter to hide their
    /// spacing, and jitter shows as a fine screen pattern in a gobo's
    /// shaft; it is here for a gobo with detail finer than its mips keep.
    pub gobo_jitter: f32,
}

impl Default for BeamCamera {
    fn default() -> Self {
        Self {
            quality: Quality::default(),
            haze: Haze::default(),
            highlights: Highlights::default(),
            gobo_jitter: 0.0,
        }
    }
}

/// Put beside a [`BeamCamera`] to have its quality follow the measured
/// frame time.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BeamGovernor(pub Governor);

/// Feeds each governor the last frame's real time and applies its answer.
#[expect(
    clippy::needless_pass_by_value,
    reason = "Bevy hands a system its parameters by value"
)]
fn govern_beam_quality(
    time: Res<Time<Real>>,
    mut cameras: Query<(&mut BeamCamera, &mut BeamGovernor)>,
) {
    let frame_ms = time.delta_secs() * 1000.0;
    if frame_ms <= 0.0 {
        return;
    }
    for (mut camera, mut governor) in &mut cameras {
        let quality = governor.0.observe(frame_ms);
        if quality != camera.quality {
            info!(
                from = ?camera.quality,
                to = ?quality,
                average_ms = governor.0.average_ms(),
                budget_ms = governor.0.budget_ms,
                "beams: quality changed"
            );
            camera.quality = quality;
        }
    }
}

impl Plugin for BeamsPlugin {
    fn build(&self, app: &mut App) {
        load_shader_library!(app, "shaders/scatter.wesl");
        embedded_asset!(app, "shaders/beams.wesl");
        embedded_asset!(app, "shaders/composite.wesl");

        let open = gobo_image(&[GoboLayer::open(4)])
            .map(|image| app.world_mut().resource_mut::<Assets<Image>>().add(image))
            .unwrap_or_default();
        app.insert_resource(BeamGobos(open))
            .add_plugins(ExtractComponentPlugin::<BeamCamera>::default())
            .add_systems(Update, govern_beam_quality);

        let assets = app.world().resource::<AssetServer>();
        let shaders = render::BeamShaders {
            air: assets.load("embedded://ignition_beams/shaders/beams.wesl"),
            composite: assets.load("embedded://ignition_beams/shaders/composite.wesl"),
        };
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(shaders)
            .init_resource::<render::ExtractedBeams>()
            .init_gpu_resource::<render::BeamBuffers>()
            .init_gpu_resource::<SpecializedRenderPipelines<render::BeamPipeline>>()
            .add_systems(RenderStartup, render::init_beam_pipeline)
            .add_systems(ExtractSchedule, render::extract_beams)
            .add_systems(
                Render,
                (
                    render::allow_depth_sampling
                        .in_set(RenderSystems::Prepare)
                        .before(prepare_core_3d_depth_textures),
                    render::prepare_beam_pipelines.in_set(RenderSystems::Prepare),
                    render::prepare_beam_buffers.in_set(RenderSystems::PrepareResources),
                ),
            )
            .add_systems(
                Core3d,
                render::draw_beams
                    .after(Core3dSystems::MainPass)
                    .before(Core3dSystems::EarlyPostProcess),
            );
    }
}
