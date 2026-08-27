//! The haze, marched at a fraction of the picture's size.
//!
//! Bevy's volumetric fog raymarches every pixel of the view it is on:
//! `step_count` samples per pixel, each looping over every volumetric
//! light whose cone reaches that cluster. At 5120x1440 that was most of
//! the frame by a wide margin, and it scales with pixels for a picture
//! that is, by nature, soft — haze has no edges of its own.
//!
//! So the fog is not on the main camera. A second **haze camera** on its
//! own render layer sees only black twins of the room's occluders and
//! renders the fog over them into a small HDR texture; a full-screen
//! composite in the main camera's transparent pass adds that texture in
//! and dims the scene by the transmittance the twins wrote into its
//! alpha. Every part of it is a Bevy feature used as shipped — render
//! layers, render-to-texture, `Material`, the fog itself — which is what
//! keeps it out of the engine's internals.
//!
//! Halving the haze camera's size cuts the fog to a quarter; a third,
//! to a ninth. The picture is upsampled bilinearly. What that costs is
//! sharpness where a shaft meets a wall, and at the studio's viewport
//! that edge is already softened by bloom.
//!
//! Every fixture's spill light is volumetric, so the fog is the one
//! thing that puts light in the air: with the hazers off nothing shows
//! there, and as they run every shaft appears in it. `HazeLevel` is the
//! hazers' settled output; `drive_haze` writes it into the fog volume
//! and the occluders' extinction every frame.

use crate::spawn::VizSettings;
use bevy::asset::embedded_asset;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{ClearColorConfig, Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::Image;
use bevy::light::{FogVolume, NotShadowCaster, ShadowFilteringMethod, VolumetricFog};
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::mesh::MeshVertexBufferLayoutRef;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor,
    SpecializedMeshPipelineError, TextureFormat,
};
use bevy::shader::ShaderRef;

const OCCLUDER_SHADER: &str = "embedded://ignition_viz/haze_occluder.wgsl";
const COMPOSITE_SHADER: &str = "embedded://ignition_viz/haze_composite.wgsl";

/// The render layer the haze camera sees: occluder twins and volumetric
/// lights, nothing else.
pub const HAZE_LAYER: usize = 1;

/// On the main camera: "march the haze for this view at `1/scale` of
/// its size, with `fog_steps` samples per pixel". At a scale of one the
/// fog goes on the camera itself; above it, on a haze camera.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HazeView {
    pub fog_steps: u32,
    pub scale: u32,
}

/// The haze camera itself, and which main camera it follows.
#[derive(Component)]
pub struct HazeCamera {
    pub main: Entity,
    pub image: Handle<Image>,
    pub size: UVec2,
}

/// The full-screen composite quad, a child of the main camera.
#[derive(Component)]
pub struct HazeComposite;

/// How much haze is in the room right now, as a fraction of a normally
/// hazed room: the hazers' output, settled with the lag real haze has.
/// The operator's `--haze` dial multiplies it (`spawn::haze_density`).
// r[impl viz.haze-is-volumetric] - the haze in the room is the hazers' settled output
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct HazeLevel {
    pub level: f32,
}

impl Default for HazeLevel {
    /// A room opens already hazed to its residual — the hazers ran
    /// before doors — rather than clear.
    fn default() -> Self {
        Self {
            level: HAZE_RESIDUAL,
        }
    }
}

/// What is left in the air with every hazer off: a room that has been
/// hazed does not clear, and a show that never touches its hazers is
/// lit in that residual rather than in clean air. A venue with no hazer
/// patched at all is taken as normally hazed.
pub const HAZE_RESIDUAL: f32 = 0.35;

/// Seconds for the haze to close most of the gap toward a hazer that
/// has just come up: fluid takes a while to fill a room.
pub const HAZE_RISE_SECONDS: f32 = 4.0;

/// Seconds for it to fall most of the way once the hazers stop. Slower
/// than the rise: haze lingers.
pub const HAZE_FALL_SECONDS: f32 = 12.0;

/// The level the room is heading for: the hardest-working hazer, never
/// below the residual, and a full room where no hazer is patched.
// r[impl viz.haze-is-volumetric] - no hazer patched reads as a hazed room
pub fn hazer_target(hazer_outputs: &[f32]) -> f32 {
    if hazer_outputs.is_empty() {
        return 1.0;
    }
    hazer_outputs
        .iter()
        .fold(HAZE_RESIDUAL, |acc, o| acc.max(o.clamp(0.0, 1.0)))
}

impl HazeLevel {
    /// One frame of settling toward `target`: a first-order lag with
    /// the rise and fall constants above.
    // r[impl viz.haze-is-volumetric] - haze builds and lingers
    pub fn settle(&mut self, target: f32, dt: f32) {
        let tau = if target > self.level {
            HAZE_RISE_SECONDS
        } else {
            HAZE_FALL_SECONDS
        };
        let k = 1.0 - (-dt.max(0.0) / tau).exp();
        self.level += (target - self.level) * k;
    }
}

/// A black twin, so its original is not twinned again and the twin
/// itself never is.
#[derive(Component)]
pub struct HazeTwin;

/// The extinction the twins write — one material for every twin, so the
/// haze camera draws the whole room in one batch per mesh.
#[derive(Resource)]
struct OccluderMaterialHandle(Handle<HazeOccluderMaterial>);

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct HazeOccluderMaterial {
    /// x = extinction per metre.
    #[uniform(0)]
    pub params: Vec4,
}

impl Material for HazeOccluderMaterial {
    fn fragment_shader() -> ShaderRef {
        OCCLUDER_SHADER.into()
    }
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct HazeCompositeMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub haze: Handle<Image>,
}

impl Material for HazeCompositeMaterial {
    fn fragment_shader() -> ShaderRef {
        COMPOSITE_SHADER.into()
    }

    /// Transparent pass, no depth write — the routing `Add` buys. The
    /// blend itself is replaced below.
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    /// `out = fog + scene * transmittance`: the haze camera's colour is
    /// added, and what was already there is scaled by the alpha its
    /// occluders wrote.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut()
            && let Some(Some(target)) = fragment.targets.first_mut()
        {
            target.blend = Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::SrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
            });
        }
        Ok(())
    }
}

pub struct HazePlugin;

impl Plugin for HazePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "haze_occluder.wgsl");
        embedded_asset!(app, "haze_composite.wgsl");
        app.add_plugins((
            MaterialPlugin::<HazeOccluderMaterial>::default(),
            MaterialPlugin::<HazeCompositeMaterial>::default(),
        ))
        .init_resource::<HazeLevel>()
        .add_systems(Startup, occluder_material)
        .add_systems(Update, (spawn_haze_cameras, sort_occluders, drive_haze))
        .add_systems(
            PostUpdate,
            follow_main_camera.before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// The haze's extinction per metre, from the same dials the fog volume
/// is spawned with — see `spawn::VOLUMETRIC_HAZE_SCALE`.
fn occluder_material(
    mut commands: Commands,
    settings: Res<VizSettings>,
    mut materials: ResMut<Assets<HazeOccluderMaterial>>,
) {
    let extinction =
        crate::spawn::haze_extinction_per_metre(settings.haze, HazeLevel::default().level);
    let handle = materials.add(HazeOccluderMaterial {
        params: Vec4::new(extinction, 0.0, 0.0, 0.0),
    });
    commands.insert_resource(OccluderMaterialHandle(handle));
}

/// Bevy's per-pixel ray-start jitter on the haze camera, in metres. At
/// 128 steps across a thirty-metre house a step is a quarter of a metre,
/// and a wide cone seen along its axis shows every step as a ring; a
/// jitter of about one step turns the rings into grain the bilinear
/// upsample and bloom smooth away, and it changes with the frame
/// (interleaved gradient noise on `frame_count`), so what is left
/// averages out over a few frames. `IGNITION_FOG_JITTER` overrides it.
pub const FOG_JITTER_METRES: f32 = 0.3;

fn fog_jitter() -> f32 {
    std::env::var("IGNITION_FOG_JITTER")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(FOG_JITTER_METRES)
}

/// How many pixels the haze camera may have when the scale is chosen
/// automatically (`HazeView::scale == 0`). The fog's cost is this times
/// the step count, so it is the one number that sets the haze's share
/// of the frame whatever the viewport: 1280x720 at a 2560-wide studio,
/// 1280x360 at 5120 — both under the budget with 128 steps on the
/// reference GPU.
pub const HAZE_PIXEL_BUDGET: u32 = 640 * 960;

/// The scale for `main`: the smallest whole divisor that brings the
/// haze camera under `HAZE_PIXEL_BUDGET`; a given scale is used as is.
pub fn haze_scale(main: UVec2, scale: u32) -> u32 {
    if scale > 0 {
        return scale;
    }
    let pixels = f64::from(main.x) * f64::from(main.y);
    ((pixels / f64::from(HAZE_PIXEL_BUDGET)).sqrt().ceil() as u32).max(1)
}

/// The haze camera's target: `main / scale`, never below one pixel.
pub fn haze_size(main: UVec2, scale: u32) -> UVec2 {
    let scale = haze_scale(main, scale);
    UVec2::new(main.x.div_ceil(scale).max(1), main.y.div_ceil(scale).max(1))
}

fn haze_target(images: &mut Assets<Image>, size: UVec2) -> Handle<Image> {
    // Float, so the shafts keep their HDR headroom for the main
    // camera's bloom; no sRGB view, this is radiance.
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba16Float, None);
    image.sampler = bevy::image::ImageSampler::linear();
    images.add(image)
}

/// Gives every main camera that asks for one a haze camera and a
/// composite quad.
fn spawn_haze_cameras(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut composites: ResMut<Assets<HazeCompositeMaterial>>,
    cameras: Query<(Entity, &HazeView, &Camera, &Transform, &Projection), Added<HazeView>>,
) {
    for (main, view, camera, transform, projection) in &cameras {
        if view.scale == 1 {
            // Full size: the fog on the camera itself, as Bevy ships it
            // and as every still was made.
            commands.entity(main).insert(VolumetricFog {
                ambient_intensity: 0.0,
                step_count: view.fog_steps,
                jitter: 0.0,
                ..default()
            });
            continue;
        }
        let size = haze_size(camera.physical_viewport_size().unwrap_or(UVec2::new(64, 64)), view.scale);
        let image = haze_target(&mut images, size);
        commands.spawn((
            HazeCamera {
                main,
                image: image.clone(),
                size,
            },
            Camera3d::default(),
            Camera {
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Hdr,
            Msaa::Off,
            Tonemapping::None,
            RenderLayers::layer(HAZE_LAYER),
            ShadowFilteringMethod::Hardware2x2,
            VolumetricFog {
                ambient_intensity: 0.0,
                step_count: view.fog_steps,
                jitter: fog_jitter(),
                ..default()
            },
            *transform,
            projection.clone(),
            Name::new("Haze camera"),
        ));
        // A quad just past the near plane, big enough to fill any field
        // of view, a child of the camera so it rides along. Nearest
        // thing in the transparent pass, so it is drawn last.
        commands.spawn((
            HazeComposite,
            Mesh3d(meshes.add(Rectangle::new(200.0, 200.0))),
            MeshMaterial3d(composites.add(HazeCompositeMaterial { haze: image })),
            Transform::from_xyz(0.0, 0.0, -0.1),
            bevy::camera::visibility::NoFrustumCulling,
            NotShadowCaster,
            ChildOf(main),
            Name::new("Haze composite"),
        ));
    }
}

/// Keeps each haze camera on its main camera's pose, projection and
/// size. A resize is a new target image — the render world caches one
/// GPU texture per handle, so the image is replaced, not resized.
fn follow_main_camera(
    mut images: ResMut<Assets<Image>>,
    mut composites: ResMut<Assets<HazeCompositeMaterial>>,
    mains: Query<(&HazeView, &Camera, &Transform, Ref<Projection>, &Children), Without<HazeCamera>>,
    mut hazes: Query<
        (
            &mut HazeCamera,
            &mut Transform,
            &mut Projection,
            &mut RenderTarget,
            &mut VolumetricFog,
        ),
        Without<HazeView>,
    >,
    quads: Query<&MeshMaterial3d<HazeCompositeMaterial>, With<HazeComposite>>,
) {
    for (mut haze, mut transform, mut projection, mut target, mut fog) in &mut hazes {
        let Ok((view, camera, main_transform, main_projection, children)) = mains.get(haze.main)
        else {
            continue;
        };
        if *transform != *main_transform {
            *transform = *main_transform;
        }
        if main_projection.is_changed() {
            *projection = (*main_projection).clone();
        }
        if fog.step_count != view.fog_steps {
            fog.step_count = view.fog_steps;
        }
        let Some(main_size) = camera.physical_viewport_size() else {
            continue;
        };
        let wanted = haze_size(main_size, view.scale);
        if wanted == haze.size {
            continue;
        }
        let image = haze_target(&mut images, wanted);
        *target = RenderTarget::Image(image.clone().into());
        for child in children.iter() {
            if let Ok(material) = quads.get(child)
                && let Some(mut m) = composites.get_mut(&material.0)
            {
                m.haze = image.clone();
            }
        }
        haze.image = image;
        haze.size = wanted;
    }
}

/// Sorts every new opaque mesh into what it does for the light: a
/// fixture housing casts no shadow and blocks no shaft; everything else
/// gets a black twin on the haze layer: same mesh, same skin, a child with an identity transform
/// so it is wherever its original is. Only `StandardMaterial` meshes —
/// a beam cone or the composite quad occludes nothing — and nothing
/// under a fixture: a par's housing is a hand's width across and hangs
/// above every beam, and five hundred of them would double what the
/// haze camera culls and draws for a silhouette no shaft ever shows.
fn sort_occluders(
    mut commands: Commands,
    material: Option<Res<OccluderMaterialHandle>>,
    added: Query<
        (Entity, &Mesh3d, Option<&SkinnedMesh>),
        (
            Added<Mesh3d>,
            With<MeshMaterial3d<StandardMaterial>>,
            Without<HazeTwin>,
        ),
    >,
    parents: Query<&ChildOf>,
    fixtures: Query<(), With<crate::spawn::Fixture>>,
) {
    let Some(material) = material else { return };
    let under_a_fixture = |mut entity: Entity| loop {
        if fixtures.contains(entity) {
            return true;
        }
        match parents.get(entity) {
            Ok(parent) => entity = parent.parent(),
            Err(_) => return false,
        }
    };
    for (entity, mesh, skin) in &added {
        if under_a_fixture(entity) {
            // Nor does a housing cast a shadow: twelve shadow views a
            // frame each culled and drew five hundred fixture meshes
            // for silhouettes that land on the ceiling they hang from.
            // That was most of the render thread's frame.
            // r[impl viz.performance-budget] - fixture bodies cast no shadow
            commands.entity(entity).insert(NotShadowCaster);
            continue;
        }
        let mut twin = commands.spawn((
            HazeTwin,
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(material.0.clone()),
            Transform::IDENTITY,
            RenderLayers::layer(HAZE_LAYER),
            NotShadowCaster,
            ChildOf(entity),
        ));
        if let Some(skin) = skin {
            twin.insert(skin.clone());
        }
    }
}

/// Writes the room's haze into the renderer: the fog volume's density
/// and the occluder twins' extinction, both from the same dial and the
/// same hazer level, so the composite dims the room exactly as much as
/// the fog lights it. Runs after the fixtures decode the frame's DMX,
/// which is where the level moves.
// r[impl viz.haze-is-volumetric] - one density, fog and occluders alike
fn drive_haze(
    level: Res<HazeLevel>,
    settings: Res<VizSettings>,
    material: Option<Res<OccluderMaterialHandle>>,
    mut materials: ResMut<Assets<HazeOccluderMaterial>>,
    mut fogs: Query<&mut FogVolume>,
) {
    if !level.is_changed() && !settings.is_changed() {
        return;
    }
    let density = crate::spawn::haze_density(settings.haze, level.level);
    for mut fog in &mut fogs {
        if fog.density_factor != density {
            fog.density_factor = density;
        }
    }
    let extinction = crate::spawn::haze_extinction_per_metre(settings.haze, level.level);
    if let Some(handle) = material
        && let Some(mut m) = materials.get_mut(&handle.0)
        && m.params.x != extinction
    {
        m.params.x = extinction;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No hazer patched is a hazed room; hazers off is the residual, not
    /// clean air; the hardest-working hazer sets the level.
    /// r[verify viz.haze-is-volumetric]
    #[test]
    fn the_hazers_set_the_level_and_a_room_never_fully_clears() {
        assert_eq!(hazer_target(&[]), 1.0);
        assert_eq!(hazer_target(&[0.0, 0.0]), HAZE_RESIDUAL);
        assert_eq!(hazer_target(&[0.2, 0.9]), 0.9);
        assert_eq!(hazer_target(&[3.0]), 1.0);
    }

    /// Haze builds over seconds and lingers longer than it took to
    /// build; it never overshoots.
    /// r[verify viz.haze-is-volumetric]
    #[test]
    fn haze_builds_with_a_lag_and_lingers() {
        let mut h = HazeLevel { level: 0.0 };
        h.settle(1.0, HAZE_RISE_SECONDS);
        let after_one_tau = h.level;
        assert!((after_one_tau - 0.632).abs() < 0.01, "{after_one_tau}");
        for _ in 0..100 {
            h.settle(1.0, 1.0);
        }
        assert!(h.level <= 1.0 && h.level > 0.999);
        h.settle(0.0, HAZE_RISE_SECONDS);
        assert!(h.level > 1.0 - 0.632, "falls slower than it rose: {}", h.level);
        h.settle(0.0, 1000.0);
        assert!(h.level.abs() < 1e-3);
        // A zero or negative frame does nothing.
        let before = h.level;
        h.settle(1.0, 0.0);
        assert_eq!(h.level, before);
    }

    /// The dial multiplies the hazers: no haze on the dial, or a room
    /// with none in it, is no density at all.
    /// r[verify viz.haze-is-volumetric]
    #[test]
    fn density_is_dial_times_hazer_level() {
        use crate::spawn::haze_density;
        assert_eq!(haze_density(0.0, 1.0), 0.0);
        assert_eq!(haze_density(1.6, 0.0), 0.0);
        assert!((haze_density(1.6, 0.5) - haze_density(1.6, 1.0) * 0.5).abs() < 1e-7);
        assert!(haze_density(1.6, 1.0) > 0.0);
    }

    /// r[verify viz.performance-budget]
    #[test]
    fn the_haze_camera_is_a_fraction_of_the_view_and_never_nothing() {
        assert_eq!(haze_size(UVec2::new(5120, 1440), 2), UVec2::new(2560, 720));
        assert_eq!(haze_size(UVec2::new(2560, 1440), 3), UVec2::new(854, 480));
        assert_eq!(haze_size(UVec2::new(1, 1), 4), UVec2::new(1, 1));
    }

    /// Auto scale: the studio's two viewports land under the pixel
    /// budget at the smallest whole scale, and a small window is not
    /// shrunk at all.
    /// r[verify viz.performance-budget]
    #[test]
    fn the_automatic_scale_keeps_the_haze_under_its_pixel_budget() {
        for (w, h) in [(5120, 1440), (2560, 1440), (1920, 1080), (640, 360)] {
            let size = haze_size(UVec2::new(w, h), 0);
            assert!(size.x * size.y <= HAZE_PIXEL_BUDGET, "{w}x{h} -> {size}");
            let scale = haze_scale(UVec2::new(w, h), 0);
            if scale > 1 {
                let coarser = UVec2::new(w.div_ceil(scale - 1), h.div_ceil(scale - 1));
                assert!(coarser.x * coarser.y > HAZE_PIXEL_BUDGET, "{w}x{h} could use {}", scale - 1);
            }
        }
        assert_eq!(haze_scale(UVec2::new(640, 360), 0), 1);
        assert_eq!(haze_scale(UVec2::new(2560, 1440), 0), 3);
        assert_eq!(haze_scale(UVec2::new(5120, 1440), 0), 4);
    }
}
