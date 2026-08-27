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
use bevy_ecs::{
    component::Component,
    entity::Entity,
    query::With,
    resource::Resource,
    schedule::IntoScheduleConfigs,
    system::{Commands, Query, Res, ResMut},
    world::World,
};
use bevy_math::UVec3;
use bevy_reflect::Reflect;
use bevy_render::{
    Render, RenderApp, RenderStartup, RenderSystems,
    camera::ExtractedCamera,
    diagnostic::RecordDiagnostics,
    extract_component::{ExtractComponent, ExtractComponentPlugin},
    render_resource::{
        binding_types::{texture_3d, texture_storage_3d, uniform_buffer},
        *,
    },
    renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
    texture::{CachedTexture, TextureCache},
    view::ExtractedView,
};
use bevy_shader::Shader;
use bevy_utils::prelude::default;

use bevy_core_pipeline::schedule::{Core3d, Core3dSystems};

use crate::{MeshPipelineViewLayoutKey, MeshPipelineViewLayouts, MeshViewBindGroup, ViewKeyCache};

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
}

impl Default for FroxelVolumetrics {
    fn default() -> Self {
        Self {
            dimensions: UVec3::new(160, 90, 64),
            near: 0.5,
            far: 60.0,
        }
    }
}

/// The grid's own uniform, mirroring `FroxelGrid` in the shader.
#[derive(Clone, Copy, ShaderType)]
pub struct FroxelUniform {
    dimensions: UVec3,
    /// Advanced every frame so the sample point inside each froxel moves;
    /// the temporal pass turns that motion into resolution.
    jitter: f32,
    near: f32,
    far: f32,
    scattering: f32,
    absorption: f32,
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
    shader: Handle<Shader>,
}

/// The injection pipeline for one view, specialised on that view's
/// layout key the way every other pipeline in the crate is.
#[derive(Component)]
pub struct ViewFroxelPipeline(CachedComputePipelineId);

/// The grid pair for one view: what the injection writes, and what the
/// integration accumulates into for the apply pass to read.
#[derive(Component)]
pub struct ViewFroxelTextures {
    pub scattering: CachedTexture,
    pub integrated: CachedTexture,
}

#[derive(Component)]
pub struct ViewFroxelBindGroups {
    inject: BindGroup,
    integrate: BindGroup,
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

pub fn init_froxel_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mesh_view_layouts: Res<MeshPipelineViewLayouts>,
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

    commands.insert_resource(FroxelPipelines {
        mesh_view_layouts: mesh_view_layouts.clone(),
        inject_layout,
        integrate_layout,
        integrate,
        shader: load_embedded_asset!(world, "froxel.wgsl"),
    });
}

/// The injection pipeline has to be specialised per view, because the
/// view layout it binds is.
pub fn prepare_froxel_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    pipelines: Res<FroxelPipelines>,
    view_key_cache: Res<ViewKeyCache>,
    views: Query<(Entity, &ExtractedView), With<FroxelVolumetrics>>,
) {
    for (entity, view) in &views {
        let Some(view_key) = view_key_cache.get(&view.retained_view_entity) else {
            continue;
        };
        let view_layout = pipelines
            .mesh_view_layouts
            .get_view_layout(MeshPipelineViewLayoutKey::from(*view_key));

        let id = pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some("froxel_inject_pipeline".into()),
            layout: vec![
                view_layout.main_layout.clone(),
                pipelines.inject_layout.clone(),
            ],
            shader: pipelines.shader.clone(),
            ..default()
        });
        commands.entity(entity).insert(ViewFroxelPipeline(id));
    }
}

pub fn prepare_froxel_textures(
    mut commands: Commands,
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

        commands.entity(entity).insert(ViewFroxelTextures {
            scattering: texture_cache.get(&render_device, descriptor("froxel_scattering_grid")),
            integrated: texture_cache.get(&render_device, descriptor("froxel_integrated_grid")),
        });
    }
}

pub fn prepare_froxel_uniforms(
    mut commands: Commands,
    mut buffer: ResMut<FroxelUniformBuffer>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    views: Query<(Entity, &FroxelVolumetrics)>,
) {
    let Some(mut writer) = buffer
        .0
        .get_writer(views.iter().len(), &render_device, &render_queue)
    else {
        return;
    };

    for (entity, froxels) in &views {
        let offset = writer.write(&FroxelUniform {
            dimensions: froxels.dimensions,
            // Temporal accumulation lands in a later pass; until then
            // the sample sits at the froxel's centre.
            jitter: 0.0,
            near: froxels.near,
            far: froxels.far,
            // The medium, until the pass reads it from `FogVolume`.
            scattering: 0.01,
            absorption: 0.01,
        });
        commands
            .entity(entity)
            .insert(ViewFroxelUniformOffset(offset));
    }
}

pub fn prepare_froxel_bind_groups(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipelines: Res<FroxelPipelines>,
    froxel_uniforms: Res<FroxelUniformBuffer>,
    views: Query<(Entity, &ViewFroxelTextures)>,
) {
    let Some(froxel_binding) = froxel_uniforms.0.binding() else {
        return;
    };

    for (entity, textures) in &views {
        let inject = render_device.create_bind_group(
            "froxel_inject_bind_group",
            &pipeline_cache.get_bind_group_layout(&pipelines.inject_layout),
            &BindGroupEntries::sequential((
                froxel_binding.clone(),
                &textures.scattering.default_view,
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

        commands
            .entity(entity)
            .insert(ViewFroxelBindGroups { inject, integrate });
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
    )>,
    pipelines: Res<FroxelPipelines>,
    pipeline_cache: Res<PipelineCache>,
    mut ctx: RenderContext,
) {
    let (froxels, bind_groups, inject_pipeline, view_bind_group, froxel_offset) = view.into_inner();

    let (Some(inject), Some(integrate)) = (
        pipeline_cache.get_compute_pipeline(inject_pipeline.0),
        pipeline_cache.get_compute_pipeline(pipelines.integrate),
    ) else {
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
        pass.set_bind_group(1, &bind_groups.inject, &[froxel_offset.0]);
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

    encoder.pop_debug_group();
    time_span.end(encoder);
}
