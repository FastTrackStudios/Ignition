//! IGNITION PATCH: froxel volumetrics.
//!
//! Bevy's volumetric fog raymarches in screen space: every pixel walks
//! the ray itself, every frame, from scratch. That is why it is
//! expensive, why running it at reduced resolution is asked for
//! ([bevy#16701]), and why a unified froxel system is on the wishlist
//! and unstaffed ([bevy#18151]).
//!
//! This is that system, in the shape both issues describe. A
//! frustum-aligned voxel grid — froxels — is lit **once per froxel**
//! rather than once per pixel per step, integrated along its depth
//! once per column, and sampled per pixel. Reduced resolution is not a
//! setting bolted on afterwards: the grid *is* the reduced-resolution
//! representation, and its dimensions are the quality dial.
//!
//! Written to be offered upstream, so the API surface is Bevy's own
//! ([`FroxelVolumetrics`] on the camera) rather than anything specific
//! to the application that needed it first.
//!
//! [bevy#16701]: https://github.com/bevyengine/bevy/issues/16701
//! [bevy#18151]: https://github.com/bevyengine/bevy/issues/18151

use bevy_app::{App, Plugin};
use bevy_asset::{Handle, embedded_asset, load_embedded_asset};
use bevy_camera::Camera3d;
use bevy_diagnostic::FrameCount;
use bevy_ecs::{
    component::Component,
    entity::Entity,
    query::With,
    resource::Resource,
    schedule::IntoScheduleConfigs,
    system::{Commands, Query, Res, ResMut},
    world::World,
};
use bevy_light::FogVolume;
use bevy_math::{Mat4, UVec3, UVec4, Vec3, Vec4};
use bevy_reflect::Reflect;
use bevy_render::{
    Render, RenderApp, RenderStartup, RenderSystems,
    camera::ExtractedCamera,
    diagnostic::RecordDiagnostics,
    extract_component::{ExtractComponent, ExtractComponentPlugin},
    render_asset::RenderAssets,
    render_resource::{
        binding_types::{
            sampler, texture_3d, texture_depth_2d, texture_storage_3d, uniform_buffer,
        },
        *,
    },
    renderer::{RenderAdapter, RenderContext, RenderDevice, RenderQueue, ViewQuery},
    texture::{CachedTexture, FallbackImage, GpuImage, TextureCache},
    view::{
        ExtractedView, ViewDepthTexture, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms,
    },
};
use bevy_shader::{Shader, ShaderDefVal};
use bevy_transform::components::GlobalTransform;
use bevy_utils::prelude::default;

use bevy_core_pipeline::{
    FullscreenShader,
    core_3d::prepare_core_3d_depth_textures,
    schedule::{Core3d, Core3dSystems},
};

use crate::{
    MeshPipelineKey, MeshPipelineViewLayoutKey, MeshPipelineViewLayouts, MeshViewBindGroup,
    ViewKeyCache,
};

/// The grid's storage format: in-scattered light in RGB, extinction in
/// A. Sixteen-bit float because the light is HDR and the grid is read
/// with filtering.
const GRID_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Froxel volumetrics on a camera.
///
/// The dimensions are the quality dial and the answer to "render
/// volumetric fog at lower resolution": there is no separate resolution
/// setting because the grid is already the reduced-resolution
/// representation. Halving `dimensions.xy` quarters the lighting work
/// without the fog becoming a blurry copy of a full-resolution image,
/// which is what downsampling a screen-space march gives you.
#[derive(Component, Clone, Copy, Debug, Reflect, ExtractComponent)]
pub struct FroxelVolumetrics {
    /// Froxels across, up and deep.
    ///
    /// The default is the one Frostbite and Unreal converged on and
    /// costs about a million froxels — against two and a half million
    /// pixels at 1080p, each of which would otherwise walk the whole
    /// light list once per step.
    pub dimensions: UVec3,
    /// Where the grid begins and ends in view space, in metres.
    ///
    /// Not the camera's near plane: a grid starting at 0.1 m spends
    /// most of its slices on air nobody is looking through. Depth
    /// slices are distributed exponentially between the two, so the
    /// near froxels are the small ones.
    pub near: f32,
    pub far: f32,
    /// How many points inside each froxel are lit and averaged.
    ///
    /// One relies entirely on the temporal blend to resolve a beam
    /// thinner than a froxel, which takes frames a still does not have.
    pub samples: u32,
    /// How much of last frame's grid survives into this one, 0..1.
    ///
    /// A beam's bright core is thinner than a froxel, so one sample a
    /// frame renders it as a dashed line; the history is what turns
    /// those samples into a beam. Too much and it smears behind a
    /// moving mover, which is why the caller owns the number.
    pub history_weight: f32,
    /// The exponent the fog's distance attenuation falls off with, two
    /// being the physical inverse square.
    ///
    /// Lower carries a beam further than physics does. A look decision
    /// rather than a wrong one — a real shaft is visible well past
    /// where its own falloff says it should be — and it touches the fog
    /// alone, never a surface.
    pub dilution: f32,
}

impl Default for FroxelVolumetrics {
    fn default() -> Self {
        Self {
            dimensions: UVec3::new(160, 90, 64),
            near: 0.5,
            far: 60.0,
            samples: 1,
            history_weight: 0.9,
            dilution: 2.0,
        }
    }
}

/// Local 1x1x1 space to the density texture's UVW space, as
/// `render.rs` defines it for the march.
static UVW_FROM_LOCAL: Mat4 = Mat4::from_cols(
    bevy_math::vec4(1.0, 0.0, 0.0, 0.0),
    bevy_math::vec4(0.0, 1.0, 0.0, 0.0),
    bevy_math::vec4(0.0, 0.0, 1.0, 0.0),
    bevy_math::vec4(0.5, 0.5, 0.5, 1.0),
);

/// The grid's own uniform, mirroring `FroxelGrid` in the shader.
#[derive(Clone, Copy, ShaderType)]
pub struct FroxelUniform {
    /// Froxels across, up and deep, and the frame number in `w`.
    dimensions: UVec4,
    /// `near`, `far`, `scattering`, `absorption`.
    range: Vec4,
    /// `density`, `light_intensity`, `asymmetry`, `history_weight`.
    medium: Vec4,
    /// `samples`, `has_density_texture`, `has_volume`, `dilution`.
    flags: Vec4,
    /// The density texture's offset in `xyz`; `w` is spare.
    density_offset: Vec4,
    prev_clip_from_world: Mat4,
    uvw_from_world: Mat4,
}

#[derive(Resource)]
pub struct FroxelPipelines {
    /// Bevy's own view bindings — the lights, the clusters, the shadow
    /// atlas — as group 0 of the injection, exactly as the screen-space
    /// march has them. Reusing the layout rather than restating it is
    /// what lets the injection run the *same* lighting code; see the
    /// compute-visibility patch in `render/mesh_view_bindings.rs`.
    mesh_view_layouts: MeshPipelineViewLayouts,
    inject_layout: BindGroupLayoutDescriptor,
    integrate_layout: BindGroupLayoutDescriptor,
    integrate: CachedComputePipelineId,
    apply_layout: BindGroupLayoutDescriptor,
    apply_sampler: Sampler,
    apply_shader: Handle<Shader>,
    shader: Handle<Shader>,
}

/// The injection pipeline for one view, specialised on that view's
/// layout key the way every other pipeline in the crate is, and the
/// apply pipeline, specialised on the target's format.
#[derive(Component)]
pub struct ViewFroxelPipeline {
    inject: CachedComputePipelineId,
    apply: CachedRenderPipelineId,
}

/// The grid pair for one view: what the injection writes, and what the
/// integration accumulates into for the apply pass to read.
#[derive(Component)]
pub struct ViewFroxelTextures {
    /// This frame's injection target, and last frame's, which the
    /// injection blends against. They swap every frame.
    pub scattering: CachedTexture,
    pub history: CachedTexture,
    pub integrated: CachedTexture,
}

/// Last frame's view-projection for a view, kept so the history can be
/// reprojected through the camera's motion.
#[derive(Component)]
pub struct ViewFroxelPrevious(pub Mat4);

#[derive(Component)]
pub struct ViewFroxelBindGroups {
    inject: BindGroup,
    integrate: BindGroup,
    apply: BindGroup,
}

#[derive(Resource, Default)]
pub struct FroxelUniformBuffer(pub DynamicUniformBuffer<FroxelUniform>);

#[derive(Component)]
pub struct ViewFroxelUniformOffset(u32);

pub struct FroxelVolumetricsPlugin;

impl Plugin for FroxelVolumetricsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "froxel.wgsl");
        embedded_asset!(app, "froxel_integrate.wgsl");
        embedded_asset!(app, "froxel_apply.wgsl");

        app.register_type::<FroxelVolumetrics>()
            .add_plugins(ExtractComponentPlugin::<FroxelVolumetrics>::default());

        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };

        render_app
            .init_resource::<FroxelUniformBuffer>()
            // After the mesh pipeline, because the view layout this
            // borrows does not exist until it has run.
            .add_systems(
                RenderStartup,
                init_froxel_pipelines.after(crate::MeshPipelineSystems),
            )
            .add_systems(
                Render,
                (
                    prepare_froxel_pipelines.in_set(RenderSystems::Prepare),
                    prepare_froxel_textures.in_set(RenderSystems::PrepareResources),
                    prepare_froxel_uniforms.in_set(RenderSystems::PrepareResources),
                    prepare_froxel_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                    prepare_view_depth_textures_for_froxels
                        .in_set(RenderSystems::Prepare)
                        .before(prepare_core_3d_depth_textures),
                ),
            )
            .add_systems(
                Core3d,
                froxel_volumetrics
                    .after(Core3dSystems::MainPass)
                    .before(Core3dSystems::EarlyPostProcess),
            );
    }
}

/// The apply pass reads the depth buffer, so it has to be bindable.
///
/// The screen-space march asks for the same thing in
/// `render::prepare_view_depth_textures_for_volumetric_fog`; a camera
/// running froxels instead of the march would otherwise get a depth
/// texture it cannot sample.
pub fn prepare_view_depth_textures_for_froxels(
    mut cameras: Query<&mut Camera3d>,
    froxels: Query<&FroxelVolumetrics>,
) {
    if froxels.is_empty() {
        return;
    }

    for mut camera in cameras.iter_mut() {
        camera.depth_texture_usages.0 |= TextureUsages::TEXTURE_BINDING.bits();
    }
}

pub fn init_froxel_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mesh_view_layouts: Res<MeshPipelineViewLayouts>,
    render_device: Res<RenderDevice>,
    world: &World,
) {
    // Group 0 for both passes: the view, the grid's own uniform, and
    // the grid itself. Declared `COMPUTE` here — which is the whole
    // reason this can be a compute pass at all. Bevy's *own* view bind
    // group is fragment-only and cannot be bound to a compute pipeline,
    // but visibility belongs to the layout, not to the buffers.
    let inject_layout = BindGroupLayoutDescriptor::new(
        "froxel_inject_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<FroxelUniform>(true),
                texture_storage_3d(GRID_FORMAT, StorageTextureAccess::WriteOnly),
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                // The fog volume's density texture. Always bound, so
                // the layout does not vary with the scene; the uniform
                // says whether it means anything.
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );

    let apply_layout = BindGroupLayoutDescriptor::new(
        "froxel_apply_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<FroxelUniform>(true),
                texture_3d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                texture_depth_2d(),
            ),
        ),
    );

    let integrate_layout = BindGroupLayoutDescriptor::new(
        "froxel_integrate_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                uniform_buffer::<FroxelUniform>(true),
                texture_3d(TextureSampleType::Float { filterable: true }),
                texture_storage_3d(GRID_FORMAT, StorageTextureAccess::WriteOnly),
            ),
        ),
    );

    let integrate = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("froxel_integrate_pipeline".into()),
        layout: vec![integrate_layout.clone()],
        shader: load_embedded_asset!(world, "froxel_integrate.wgsl"),
        ..default()
    });

    // Clamped rather than repeated: a beam at the edge of the frustum
    // should fade off the side of the grid, not wrap round to the other
    // one.
    let apply_sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("froxel_grid_sampler"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        mipmap_filter: MipmapFilterMode::Nearest,
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        ..default()
    });

    commands.insert_resource(FroxelPipelines {
        mesh_view_layouts: mesh_view_layouts.clone(),
        apply_layout,
        apply_sampler,
        apply_shader: load_embedded_asset!(world, "froxel_apply.wgsl"),
        inject_layout,
        integrate_layout,
        integrate,
        shader: load_embedded_asset!(world, "froxel.wgsl"),
    });
}

/// The injection pipeline has to be specialised per view, because the
/// view layout it binds is.
/// Whether the injection may read clustered decals — the same test the
/// mesh pipeline makes, so the two agree about the layout.
fn decal_shader_defs(
    render_device: &RenderDevice,
    render_adapter: &RenderAdapter,
) -> Vec<ShaderDefVal> {
    if crate::decal::clustered::clustered_decals_are_usable(render_device, render_adapter) {
        vec!["CLUSTERED_DECALS_ARE_USABLE".into()]
    } else {
        vec![]
    }
}

pub fn prepare_froxel_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    pipelines: Res<FroxelPipelines>,
    view_key_cache: Res<ViewKeyCache>,
    fullscreen_shader: Res<FullscreenShader>,
    render_device: Res<RenderDevice>,
    render_adapter: Res<RenderAdapter>,
    views: Query<(Entity, &ExtractedView, &ViewTarget), With<FroxelVolumetrics>>,
) {
    let decal_defs = decal_shader_defs(&render_device, &render_adapter);

    for (entity, view, target) in &views {
        let Some(view_key) = view_key_cache.get(&view.retained_view_entity) else {
            continue;
        };
        let view_layout = pipelines
            .mesh_view_layouts
            .get_view_layout(MeshPipelineViewLayoutKey::from(*view_key));

        // The shadow sampling the injection calls is a chain of
        // `#ifdef`s on the filter method, and with none of them defined
        // it returns 0 — "to make it obvious that something is wrong",
        // as upstream puts it. Compiled without these defs the froxel
        // path draws every beam fully shadowed, which is to say black.
        // The view's key already carries the method; this reads it the
        // same way `MeshPipeline::specialize` does.
        let mut shader_defs = decal_defs.clone();
        let filter = view_key.intersection(MeshPipelineKey::SHADOW_FILTER_METHOD_RESERVED_BITS);
        if filter == MeshPipelineKey::SHADOW_FILTER_METHOD_GAUSSIAN {
            shader_defs.push("SHADOW_FILTER_METHOD_GAUSSIAN".into());
        } else if filter == MeshPipelineKey::SHADOW_FILTER_METHOD_TEMPORAL {
            shader_defs.push("SHADOW_FILTER_METHOD_TEMPORAL".into());
        } else {
            shader_defs.push("SHADOW_FILTER_METHOD_HARDWARE_2X2".into());
        }

        let inject = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("froxel_inject_pipeline".into()),
            // Group 1 is Bevy's binding-array group, bound so the
            // injection can read the clustered decals: a gobo is a
            // stencil at the gate, and the fog wants it as much as the
            // wall does. That pushes our own bindings to group 2.
            layout: vec![
                view_layout.main_layout.clone(),
                view_layout.binding_array_layout.clone(),
                pipelines.inject_layout.clone(),
            ],
            shader: pipelines.shader.clone(),
            shader_defs,
            ..default()
        });

        let apply = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some("froxel_apply_pipeline".into()),
            layout: vec![pipelines.apply_layout.clone()],
            vertex: fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: pipelines.apply_shader.clone(),
                targets: vec![Some(ColorTargetState {
                    format: target.main_texture_format(),
                    // The fog contributes `src` and lets through
                    // `1 - src_alpha` of what is behind it, which is
                    // exactly what the integrate pass's transmittance
                    // means.
                    blend: Some(BlendState {
                        color: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                        alpha: BlendComponent {
                            src_factor: BlendFactor::One,
                            dst_factor: BlendFactor::OneMinusSrcAlpha,
                            operation: BlendOperation::Add,
                        },
                    }),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        });

        commands
            .entity(entity)
            .insert(ViewFroxelPipeline { inject, apply });
    }
}

pub fn prepare_froxel_textures(
    mut commands: Commands,
    frame_count: Res<FrameCount>,
    mut texture_cache: ResMut<TextureCache>,
    render_device: Res<RenderDevice>,
    views: Query<(Entity, &FroxelVolumetrics), With<ExtractedCamera>>,
) {
    for (entity, froxels) in &views {
        let size = Extent3d {
            width: froxels.dimensions.x.max(1),
            height: froxels.dimensions.y.max(1),
            depth_or_array_layers: froxels.dimensions.z.max(1),
        };
        let descriptor = |label| TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D3,
            format: GRID_FORMAT,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };

        // Two injection grids, swapped every frame: this frame writes
        // one and reads the other, which is last frame's. Same shape as
        // TAA's history pair, and for the same reason.
        let a = texture_cache.get(&render_device, descriptor("froxel_scattering_grid_a"));
        let b = texture_cache.get(&render_device, descriptor("froxel_scattering_grid_b"));
        let (scattering, history) = if frame_count.0.is_multiple_of(2) {
            (a, b)
        } else {
            (b, a)
        };

        commands.entity(entity).insert(ViewFroxelTextures {
            scattering,
            history,
            integrated: texture_cache.get(&render_device, descriptor("froxel_integrated_grid")),
        });
    }
}

pub fn prepare_froxel_uniforms(
    mut commands: Commands,
    mut buffer: ResMut<FroxelUniformBuffer>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    frame_count: Res<FrameCount>,
    views: Query<(
        Entity,
        &FroxelVolumetrics,
        &ExtractedView,
        Option<&ViewFroxelPrevious>,
    )>,
    fog_volumes: Query<(&FogVolume, &GlobalTransform)>,
) {
    let Some(mut writer) = buffer
        .0
        .get_writer(views.iter().len(), &render_device, &render_queue)
    else {
        return;
    };

    // The medium, from the scene's fog volume. The march supports a
    // volume per region; a grid is one frustum, so this takes the
    // first — a venue has one body of haze in it.
    let medium = fog_volumes.iter().next();
    // The volume's own space, whether or not it carries a texture:
    // being *inside* it is what decides where there is haze at all.
    let (uvw_from_world, has_volume) = match medium {
        Some((_, transform)) => (UVW_FROM_LOCAL * transform.to_matrix().inverse(), 1),
        None => (Mat4::IDENTITY, 0),
    };
    let (density_texture_offset, has_density_texture) = match medium {
        Some((fog, _)) if fog.density_texture.is_some() => (fog.density_texture_offset, 1),
        _ => (Vec3::ZERO, 0),
    };

    for (entity, froxels, view, previous) in &views {
        let clip_from_world = view
            .clip_from_world
            .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());

        let offset = writer.write(&FroxelUniform {
            dimensions: froxels.dimensions.extend(frame_count.0),
            range: bevy_math::vec4(
                froxels.near,
                froxels.far,
                medium.map_or(0.3, |(fog, _)| fog.scattering),
                medium.map_or(0.3, |(fog, _)| fog.absorption),
            ),
            medium: bevy_math::vec4(
                medium.map_or(0.0, |(fog, _)| fog.density_factor),
                medium.map_or(1.0, |(fog, _)| fog.light_intensity),
                medium.map_or(0.0, |(fog, _)| fog.scattering_asymmetry),
                previous.map_or(0.0, |_| froxels.history_weight.clamp(0.0, 0.99)),
            ),
            flags: bevy_math::vec4(
                froxels.samples.clamp(1, 16) as f32,
                has_density_texture as f32,
                has_volume as f32,
                froxels.dilution.clamp(0.0, 2.0),
            ),
            density_offset: density_texture_offset.extend(0.0),
            prev_clip_from_world: previous.map_or(clip_from_world, |p| p.0),
            uvw_from_world,
        });

        commands
            .entity(entity)
            .insert(ViewFroxelPrevious(clip_from_world));
        commands
            .entity(entity)
            .insert(ViewFroxelUniformOffset(offset));
    }
}

pub fn prepare_froxel_bind_groups(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fog_volumes: Query<&FogVolume>,
    images: Res<RenderAssets<GpuImage>>,
    fallback: Res<FallbackImage>,
    pipeline_cache: Res<PipelineCache>,
    pipelines: Res<FroxelPipelines>,
    froxel_uniforms: Res<FroxelUniformBuffer>,
    view_uniforms: Res<ViewUniforms>,
    views: Query<(Entity, &ViewFroxelTextures, &ViewDepthTexture)>,
) {
    let (Some(froxel_binding), Some(view_binding)) = (
        froxel_uniforms.0.binding(),
        view_uniforms.uniforms.binding(),
    ) else {
        return;
    };

    // The density texture if the volume has one and it has finished
    // uploading, and a flat fallback otherwise — the binding is always
    // filled so the layout stays the same shape.
    let density = fog_volumes
        .iter()
        .find_map(|fog| fog.density_texture.as_ref())
        .and_then(|handle| images.get(handle));
    let density_texture = density.map_or(&fallback.d3.texture_view, |image| &image.texture_view);
    let density_sampler = density.map_or(&fallback.d3.sampler, |image| &image.sampler);

    for (entity, textures, depth) in &views {
        let inject = render_device.create_bind_group(
            "froxel_inject_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipelines.inject_layout),
            &BindGroupEntries::sequential((
                froxel_binding.clone(),
                &textures.scattering.default_view,
                &textures.history.default_view,
                &pipelines.apply_sampler,
                density_texture,
                density_sampler,
            )),
        );

        let integrate = render_device.create_bind_group(
            "froxel_integrate_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipelines.integrate_layout),
            &BindGroupEntries::sequential((
                froxel_binding.clone(),
                &textures.scattering.default_view,
                &textures.integrated.default_view,
            )),
        );

        let apply = render_device.create_bind_group(
            "froxel_apply_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipelines.apply_layout),
            &BindGroupEntries::sequential((
                view_binding.clone(),
                froxel_binding.clone(),
                &textures.integrated.default_view,
                &pipelines.apply_sampler,
                depth.view(),
            )),
        );

        commands.entity(entity).insert(ViewFroxelBindGroups {
            inject,
            integrate,
            apply,
        });
    }
}

/// Light the grid, then integrate it along its depth.
///
/// Two dispatches for the whole screen's volumetrics, against a
/// fullscreen pass that walks the clustered light list once per pixel
/// per step.
pub fn froxel_volumetrics(
    view: ViewQuery<(
        &FroxelVolumetrics,
        &ViewFroxelBindGroups,
        &ViewFroxelPipeline,
        &MeshViewBindGroup,
        &ViewFroxelUniformOffset,
        &ViewUniformOffset,
        &ViewTarget,
    )>,
    pipelines: Res<FroxelPipelines>,
    pipeline_cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (froxels, bind_groups, view_pipelines, view_bind_group, froxel_offset, view_offset, target) =
        view.into_inner();

    let (Some(inject), Some(integrate), Some(apply)) = (
        pipeline_cache.get_compute_pipeline(view_pipelines.inject),
        pipeline_cache.get_compute_pipeline(pipelines.integrate),
        pipeline_cache.get_render_pipeline(view_pipelines.apply),
    ) else {
        // The first frames run before the pipelines finish compiling.
        return;
    };

    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "froxel_volumetrics");

    let encoder = ctx.command_encoder();
    encoder.push_debug_group("froxel_volumetrics");

    let dimensions = froxels.dimensions.max(UVec3::ONE);
    {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("froxel_inject"),
            timestamp_writes: None,
        });
        pass.set_pipeline(inject);
        pass.set_bind_group(0, &view_bind_group.main, &view_bind_group.main_offsets);
        pass.set_bind_group(1, &view_bind_group.binding_array, &[]);
        pass.set_bind_group(2, &bind_groups.inject, &[froxel_offset.0]);
        pass.dispatch_workgroups(
            dimensions.x.div_ceil(4),
            dimensions.y.div_ceil(4),
            dimensions.z.div_ceil(4),
        );
    }

    {
        // One invocation per column, walking Z — the reason there is no
        // prefix sum per pixel later on.
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("froxel_integrate"),
            timestamp_writes: None,
        });
        pass.set_pipeline(integrate);
        pass.set_bind_group(0, &bind_groups.integrate, &[froxel_offset.0]);
        pass.dispatch_workgroups(dimensions.x.div_ceil(8), dimensions.y.div_ceil(8), 1);
    }

    // Composite the grid over the frame. A fullscreen triangle and one
    // filtered read per pixel, where the screen-space march walked the
    // clustered light list once per pixel per step.
    {
        // Blended onto the frame rather than ping-ponged through a
        // post-process copy: the pipeline's blend state is what
        // composites it, so the existing colour has to be loaded, not
        // cleared.
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("froxel_apply"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: target.main_texture_view(),
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(apply);
        pass.set_bind_group(
            0,
            &bind_groups.apply,
            &[view_offset.offset, froxel_offset.0],
        );
        pass.draw(0..3, 0..1);
    }

    encoder.pop_debug_group();
    time_span.end(encoder);
}
