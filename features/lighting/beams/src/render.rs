//! The render world's half: the beams' buffer, the two pipelines and the
//! pass.
//!
//! Each view with a [`BeamCamera`] gets, after its main pass and before
//! any post-processing: every beam's hull drawn additively into an
//! `Rgba16Float` air target, then that target added to the view's own
//! (`shaders/beams.wesl`, `shaders/composite.wesl`).

use bevy::{
    asset::{AssetId, Handle},
    camera::{Camera3d, visibility::InheritedVisibility},
    ecs::{
        component::Component,
        entity::Entity,
        query::With,
        resource::Resource,
        system::{Commands, Query, Res, ResMut},
    },
    image::Image,
    math::{Mat4, UVec2, Vec4},
    render::{
        Extract,
        camera::ExtractedCamera,
        diagnostic::RecordDiagnostics,
        render_asset::RenderAssets,
        render_resource::{
            BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
            BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId, ColorTargetState,
            ColorWrites, DynamicUniformBuffer, Extent3d, Face, FragmentState, LoadOp, Operations,
            PipelineCache, PrimitiveState, RenderPassColorAttachment, RenderPassDescriptor,
            RenderPipelineDescriptor, SamplerBindingType, ShaderStages, ShaderType,
            SpecializedRenderPipeline, SpecializedRenderPipelines, StorageBuffer, StoreOp,
            TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
            VertexState,
            binding_types::{
                sampler, storage_buffer_read_only, texture_2d, texture_2d_array, texture_depth_2d,
                uniform_buffer,
            },
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        texture::{CachedTexture, GpuImage, TextureCache},
        view::{ExtractedView, Msaa, ViewDepthStencilTexture, ViewTarget},
    },
    shader::{Shader, ShaderDefVal},
    transform::components::GlobalTransform,
};

use crate::{
    BeamCamera, BeamGobos,
    beam::{Beam, GpuBeam},
    quality::{AirResolution, BeamTier, GoboInAir},
};

/// The name of the pass in Bevy's render diagnostics: its GPU time is the
/// diagnostic `render/ignition_beams/elapsed_gpu`, in milliseconds.
pub const PASS_NAME: &str = "ignition_beams";

/// The format the beams are summed in: half floats, so many faint beams
/// add up without banding and bright ones are not clipped before the
/// roll-off.
const AIR_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// The hull's vertices: 24 segments of twelve (`shaders/beams.wesl`).
const HULL_VERTICES: u32 = 288;

/// Every lit beam this frame, ready for the buffer.
#[derive(Resource, Default)]
pub struct ExtractedBeams {
    beams: Vec<GpuBeam>,
    gobos: Option<AssetId<Image>>,
}

/// Copies the lit, visible beams and the gobo texture's id out of the
/// main world.
pub fn extract_beams(
    mut extracted: ResMut<ExtractedBeams>,
    beams: Extract<Query<(&Beam, &GlobalTransform, Option<&InheritedVisibility>)>>,
    gobos: Extract<Option<Res<BeamGobos>>>,
) {
    extracted.beams.clear();
    extracted.beams.extend(
        beams
            .iter()
            .filter(|(_, _, visible)| visible.is_none_or(|visible| visible.get()))
            .filter_map(|(beam, transform, _)| beam.to_gpu(transform)),
    );
    extracted.gobos = gobos.as_ref().map(|gobos| gobos.0.id());
}

/// What every beam shares for one view: `BeamView` in the shaders.
#[derive(ShaderType, Clone, Copy)]
pub struct BeamViewUniform {
    clip_from_world: Mat4,
    world_from_clip: Mat4,
    size: Vec4,
    medium: Vec4,
    look: Vec4,
}

/// The beams' storage buffer and the views' uniforms.
#[derive(Resource, Default)]
pub struct BeamBuffers {
    beams: StorageBuffer<Vec<GpuBeam>>,
    views: DynamicUniformBuffer<BeamViewUniform>,
    count: u32,
}

/// The shaders, loaded by the plugin (`lib.rs`).
#[derive(Resource, Clone)]
pub struct BeamShaders {
    pub air: Handle<Shader>,
    pub composite: Handle<Shader>,
}

/// The two bind group layouts, and the shaders to build pipelines from.
#[derive(Resource)]
pub struct BeamPipeline {
    air_layout: BindGroupLayoutDescriptor,
    composite_layout: BindGroupLayoutDescriptor,
    shaders: BeamShaders,
}

/// Builds the layouts once the render device exists.
pub fn init_beam_pipeline(mut commands: Commands, shaders: Res<BeamShaders>) {
    let air_layout = BindGroupLayoutDescriptor::new(
        "ignition_beams_air_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX_FRAGMENT,
            (
                uniform_buffer::<BeamViewUniform>(true),
                storage_buffer_read_only::<Vec<GpuBeam>>(false),
                texture_depth_2d().visibility(ShaderStages::FRAGMENT),
                texture_2d_array(TextureSampleType::Float { filterable: true })
                    .visibility(ShaderStages::FRAGMENT),
                sampler(SamplerBindingType::Filtering).visibility(ShaderStages::FRAGMENT),
            ),
        ),
    );
    let composite_layout = BindGroupLayoutDescriptor::new(
        "ignition_beams_composite_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                uniform_buffer::<BeamViewUniform>(true),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_depth_2d(),
            ),
        ),
    );
    commands.insert_resource(BeamPipeline {
        air_layout,
        composite_layout,
        shaders: shaders.clone(),
    });
}

/// Which pipeline: one per tier for the air pass, one per target format
/// and way of compositing.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeamPipelineKey {
    Air(BeamTier),
    Composite {
        format: TextureFormat,
        resolution: AirResolution,
        compress: bool,
    },
}

/// Adds source to destination, leaving the destination's alpha alone.
const ADDITIVE: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::Zero,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
};

/// The conditions `shaders/beams.wesl` is compiled under for a tier.
fn air_shader_defs(tier: BeamTier) -> Vec<ShaderDefVal> {
    let samples = match tier.gobo() {
        GoboInAir::Steps(samples) => samples,
        GoboInAir::None | GoboInAir::Soft => 1,
    };
    vec![
        ShaderDefVal::UInt("PIECES".into(), u32::from(tier.pieces())),
        ShaderDefVal::Bool("GOBO_SOFT".into(), tier.gobo() == GoboInAir::Soft),
        ShaderDefVal::Bool(
            "GOBO_STEPS".into(),
            matches!(tier.gobo(), GoboInAir::Steps(_)),
        ),
        ShaderDefVal::UInt("GOBO_SAMPLES".into(), u32::from(samples)),
    ]
}

impl SpecializedRenderPipeline for BeamPipeline {
    type Key = BeamPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        match key {
            BeamPipelineKey::Air(tier) => {
                let shader_defs = air_shader_defs(tier);
                RenderPipelineDescriptor {
                    label: Some("ignition_beams_air_pipeline".into()),
                    layout: vec![self.air_layout.clone()],
                    vertex: VertexState {
                        shader: self.shaders.air.clone(),
                        shader_defs: shader_defs.clone(),
                        ..Default::default()
                    },
                    // Back faces only: one fragment per beam per pixel,
                    // from outside the cone or from in it.
                    primitive: PrimitiveState {
                        cull_mode: Some(Face::Front),
                        ..Default::default()
                    },
                    fragment: Some(FragmentState {
                        shader: self.shaders.air.clone(),
                        shader_defs,
                        targets: vec![Some(ColorTargetState {
                            format: AIR_FORMAT,
                            blend: Some(ADDITIVE),
                            write_mask: ColorWrites::COLOR,
                        })],
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            }
            BeamPipelineKey::Composite {
                format,
                resolution,
                compress,
            } => {
                let shader_defs = vec![
                    ShaderDefVal::Bool("HALF_RES".into(), resolution == AirResolution::Half),
                    ShaderDefVal::Bool("COMPRESS".into(), compress),
                ];
                RenderPipelineDescriptor {
                    label: Some("ignition_beams_composite_pipeline".into()),
                    layout: vec![self.composite_layout.clone()],
                    vertex: VertexState {
                        shader: self.shaders.composite.clone(),
                        shader_defs: shader_defs.clone(),
                        ..Default::default()
                    },
                    fragment: Some(FragmentState {
                        shader: self.shaders.composite.clone(),
                        shader_defs,
                        targets: vec![Some(ColorTargetState {
                            format,
                            blend: Some(ADDITIVE),
                            write_mask: ColorWrites::COLOR,
                        })],
                        ..Default::default()
                    }),
                    ..Default::default()
                }
            }
        }
    }
}

/// A view's two pipelines for this frame.
#[derive(Component)]
pub struct ViewBeamPipelines {
    air: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
}

/// A view's air target and its place in the uniform buffer.
#[derive(Component)]
pub struct ViewBeamTarget {
    air: CachedTexture,
    uniform_offset: u32,
}

/// Lets the beams read the depth the main pass wrote.
pub fn allow_depth_sampling(mut cameras: Query<&mut Camera3d, With<BeamCamera>>) {
    for mut camera in &mut cameras {
        camera.depth_texture_usages.0 |= TextureUsages::TEXTURE_BINDING.bits();
    }
}

/// Picks each view's pipelines for its quality and target.
pub fn prepare_beam_pipelines(
    mut commands: Commands,
    pipeline_cache: Res<PipelineCache>,
    mut pipelines: ResMut<SpecializedRenderPipelines<BeamPipeline>>,
    pipeline: Res<BeamPipeline>,
    views: Query<(Entity, &ExtractedView, &BeamCamera)>,
) {
    for (entity, view, camera) in &views {
        let air = pipelines.specialize(
            &pipeline_cache,
            &pipeline,
            BeamPipelineKey::Air(camera.quality.tier),
        );
        let composite = pipelines.specialize(
            &pipeline_cache,
            &pipeline,
            BeamPipelineKey::Composite {
                format: view.target_format,
                resolution: camera.quality.resolution,
                compress: camera.highlights.compress,
            },
        );
        commands
            .entity(entity)
            .insert(ViewBeamPipelines { air, composite });
    }
}

/// The size of a view's air target.
fn air_size(view: UVec2, resolution: AirResolution) -> UVec2 {
    match resolution {
        AirResolution::Full => view,
        AirResolution::Half => (view / 2).max(UVec2::ONE),
    }
}

/// What a view's beams are told about it.
fn view_uniform(
    view: &ExtractedView,
    exposure: f32,
    camera: &BeamCamera,
    size: UVec2,
) -> BeamViewUniform {
    let clip_from_world = view
        .clip_from_world
        .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());
    let air = air_size(size, camera.quality.resolution).as_vec2();
    let full = size.as_vec2();
    BeamViewUniform {
        clip_from_world,
        world_from_clip: clip_from_world.inverse(),
        size: Vec4::new(air.x, air.y, full.x, full.y),
        medium: Vec4::new(
            camera.haze.scattering(),
            camera.haze.extinction(),
            camera.haze.anisotropy,
            exposure,
        ),
        look: Vec4::new(
            camera.gobo_jitter.clamp(0.0, 1.0),
            camera.highlights.white_rate,
            camera.highlights.white_from,
            camera.highlights.dither,
        ),
    }
}

/// Writes the beams' buffer, each view's uniform, and finds each view its
/// air target.
pub fn prepare_beam_buffers(
    mut commands: Commands,
    mut buffers: ResMut<BeamBuffers>,
    mut texture_cache: ResMut<TextureCache>,
    extracted: Res<ExtractedBeams>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    views: Query<(Entity, &ExtractedView, &ExtractedCamera, &BeamCamera)>,
) {
    buffers.count = u32::try_from(extracted.beams.len()).unwrap_or(u32::MAX);
    buffers.beams.get_mut().clone_from(&extracted.beams);
    buffers.beams.write_buffer(&device, &queue);

    buffers.views.clear();
    for (entity, view, extracted_camera, camera) in &views {
        let Some(size) = extracted_camera.physical_viewport_size else {
            continue;
        };
        let uniform_offset =
            buffers
                .views
                .push(&view_uniform(view, extracted_camera.exposure, camera, size));
        let air = air_size(size, camera.quality.resolution);
        let air = texture_cache.get(
            &device,
            TextureDescriptor {
                label: Some("ignition_beams_air"),
                size: Extent3d {
                    width: air.x,
                    height: air.y,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: AIR_FORMAT,
                usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
        );
        commands.entity(entity).insert(ViewBeamTarget {
            air,
            uniform_offset,
        });
    }
    buffers.views.write_buffer(&device, &queue);
}

/// Draws a view's beams into its air target, then adds that to the view.
pub fn draw_beams(
    view: ViewQuery<(
        &ViewTarget,
        &ViewDepthStencilTexture,
        &ViewBeamPipelines,
        &ViewBeamTarget,
        &Msaa,
    )>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<BeamPipeline>,
    buffers: Res<BeamBuffers>,
    extracted: Res<ExtractedBeams>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let (view_target, depth, pipelines, target, msaa) = view.into_inner();
    if buffers.count == 0 || *msaa != Msaa::Off {
        return;
    }
    let (
        Some(air_pipeline),
        Some(composite_pipeline),
        Some(beams),
        Some(views),
        Some(depth_view),
        Some(gobos),
    ) = (
        pipeline_cache.get_render_pipeline(pipelines.air),
        pipeline_cache.get_render_pipeline(pipelines.composite),
        buffers.beams.binding(),
        buffers.views.binding(),
        depth.attachment.depth_stencil_views().depth_only_view(),
        extracted.gobos.and_then(|gobos| images.get(gobos)),
    )
    else {
        return;
    };

    let air_bind_group = ctx.render_device().create_bind_group(
        "ignition_beams_air_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline.air_layout),
        &BindGroupEntries::sequential((
            views.clone(),
            beams,
            depth_view,
            &gobos.texture_view,
            &gobos.sampler,
        )),
    );
    let composite_bind_group = ctx.render_device().create_bind_group(
        "ignition_beams_composite_bind_group",
        &pipeline_cache.get_bind_group_layout(&pipeline.composite_layout),
        &BindGroupEntries::sequential((views, &target.air.default_view, depth_view)),
    );

    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), PASS_NAME);
    {
        let mut air_pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("ignition_beams_air_pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: &target.air.default_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Default::default()),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        air_pass.set_pipeline(air_pipeline);
        air_pass.set_bind_group(0, &air_bind_group, &[target.uniform_offset]);
        air_pass.draw(0..HULL_VERTICES, 0..buffers.count);
    }
    {
        let mut composite_pass =
            ctx.command_encoder()
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("ignition_beams_composite_pass"),
                    color_attachments: &[Some(RenderPassColorAttachment {
                        view: view_target.main_texture_view(),
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
        composite_pass.set_pipeline(composite_pipeline);
        composite_pass.set_bind_group(0, &composite_bind_group, &[target.uniform_offset]);
        composite_pass.draw(0..3, 0..1);
    }
    time_span.end(ctx.command_encoder());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_air_target_is_never_empty() {
        assert_eq!(
            air_size(UVec2::new(2560, 1440), AirResolution::Full),
            UVec2::new(2560, 1440)
        );
        assert_eq!(
            air_size(UVec2::new(2560, 1440), AirResolution::Half),
            UVec2::new(1280, 720)
        );
        assert_eq!(air_size(UVec2::new(1, 1), AirResolution::Half), UVec2::ONE);
    }

    #[test]
    fn every_tier_names_every_condition_the_shader_reads() {
        for tier in BeamTier::ALL {
            let defs = air_shader_defs(tier);
            let names: Vec<String> = defs
                .iter()
                .map(|def| match def {
                    ShaderDefVal::Bool(name, _)
                    | ShaderDefVal::Int(name, _)
                    | ShaderDefVal::UInt(name, _) => name.to_string(),
                })
                .collect();
            assert_eq!(names, ["PIECES", "GOBO_SOFT", "GOBO_STEPS", "GOBO_SAMPLES"]);
        }
    }
}
