//! The one live-mode render pipeline, shared by `live_renderer.rs`
//! (windowed) and `live_headless_renderer.rs` (render-to-PNG). Everything
//! that doesn't depend on *where* the frame ends up — shader compilation,
//! bind group layouts, both render pipelines, and the two-pass draw itself
//! (opaque + additive glow) — lives here exactly once. Only what genuinely
//! differs stays in the two callers: acquiring a device/queue (windowed
//! needs a surface-compatible adapter; headless doesn't), and the final
//! target (present a surface texture vs. read back an offscreen one to a
//! PNG).

use crate::camera::{Camera, CameraUniform};
use crate::mesh::{MeshBuilder, PointLight, Vertex};
use wgpu::util::DeviceExt;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// 4x MSAA — the single biggest "why does this look worse than ASLS"
/// fix found by actually reading their renderer setup: `antialias: true`
/// is a one-line default in `THREE.WebGLRenderer`, but wgpu doesn't
/// antialias by default at all, so every straight edge is aliased/blocky.
/// Both `live_renderer.rs` and `live_headless_renderer.rs` render into a
/// multisampled colour target at this sample count and resolve to the
/// caller's final (single-sample) target — see `render_frame`'s
/// `resolve_view` parameter.
pub const SAMPLE_COUNT: u32 = 4;

/// Storage buffers can't be zero-sized on every backend — pad an empty
/// light list to one dummy (position far away, black) entry rather than
/// special-casing "no lights" at the call site.
const MIN_LIGHTS: usize = 1;

/// Defaults per the operator's own call: a real dark venue has no ambient
/// fill light at all — everything you see is either a fixture's beam or
/// its spill — and enough haze in the air that beams read as visible
/// shafts of light, the way the stage actually looks, rather than as
/// invisible cones that only show up as a flat colour where they happen to
/// hit a surface.
pub const DEFAULT_AMBIENT: f32 = 0.0;
pub const DEFAULT_HAZE: f32 = 1.6;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuPointLight {
    position: [f32; 4],
    color: [f32; 4],
    /// xyz = normalized aim direction, w = cos(cone half-angle) — precomputed
    /// on the CPU side so the shader does a plain `dot()` comparison instead
    /// of a `cos()` call per light per fragment.
    direction_cos_angle: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuSettings {
    ambient: f32,
    haze: f32,
    /// Seconds since the renderer was created — animates the beam haze's
    /// turbulence (`fs_glow`'s `fogging()`) so it drifts instead of
    /// sitting frozen. `render_frame`'s caller supplies it each frame
    /// (`LiveRenderer`'s window loop uses a real clock; `--snapshot`'s
    /// single frame just passes 0.0, which is fine — a static snapshot
    /// has no "next frame" for drift to matter).
    time: f32,
    _pad1: f32,
}

pub struct LivePipeline {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    glow_pipeline: wgpu::RenderPipeline,
    camera_bind_group_layout: wgpu::BindGroupLayout,
    lights_bind_group_layout: wgpu::BindGroupLayout,
    pub ambient: f32,
    pub haze: f32,
}

impl LivePipeline {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, color_format: wgpu::TextureFormat, ambient: f32, haze: f32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ignition-live-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("live_shader.wgsl").into()),
        });

        let camera_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let lights_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lights-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ignition-live-pipeline-layout"),
            bind_group_layouts: &[Some(&camera_bind_group_layout), Some(&lights_bind_group_layout)],
            immediate_size: 0,
        });

        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3],
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ignition-live-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(vertex_layout.clone())],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLE_COUNT, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });

        // The beam-cone pass: additive blend, depth-tested against the
        // opaque geometry above (a beam doesn't glow through a wall) but no
        // depth *write* (overlapping beams blend into each other instead of
        // the nearer one hiding the farther).
        let glow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ignition-live-glow-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Some(vertex_layout)],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_glow"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::REPLACE,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLE_COUNT, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });

        Self { device, queue, pipeline, glow_pipeline, camera_bind_group_layout, lights_bind_group_layout, ambient, haze }
    }

    /// Depth attachment's `sample_count` must match the pipeline's own
    /// (`SAMPLE_COUNT`) — wgpu requires every attachment in a render pass
    /// to agree on multisampling.
    pub fn make_depth_view(&self, width: u32, height: u32) -> wgpu::TextureView {
        let depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("live-depth-target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        depth_texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// A multisampled colour target to render into — the counterpart to
    /// `make_depth_view`, same `SAMPLE_COUNT`. Callers resolve this down to
    /// their real final target (a surface texture, or an offscreen texture
    /// for PNG readback) via `render_frame`'s `resolve_view` parameter —
    /// wgpu resolves MSAA automatically as part of ending a render pass
    /// that names a `resolve_target`, no separate blit needed.
    pub fn make_msaa_color_view(&self, width: u32, height: u32, format: wgpu::TextureFormat) -> wgpu::TextureView {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("live-msaa-color-target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: SAMPLE_COUNT,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        texture.create_view(&wgpu::TextureViewDescriptor::default())
    }

    /// Builds and submits the full two-pass frame (opaque + additive glow).
    /// Callers own acquiring the views and whatever happens to the result
    /// after submission (present vs. readback) — this is everything in
    /// between that's identical either way.
    ///
    /// `msaa_view` (from `make_msaa_color_view`) is what both passes
    /// actually render into; `resolve_view` is the caller's real final
    /// target (a surface texture, or an offscreen texture for PNG
    /// readback) — resolved into on whichever pass runs last (the glow
    /// pass when there's glow geometry, the opaque pass otherwise), since
    /// resolving mid-sequence would discard the following pass's additive
    /// blending against the multisampled buffer.
    pub fn render_frame(
        &self,
        mesh: &MeshBuilder,
        camera: &Camera,
        msaa_view: &wgpu::TextureView,
        resolve_view: &wgpu::TextureView,
        depth_view: &wgpu::TextureView,
        time_secs: f32,
    ) {
        let device = &self.device;

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("live-vertices"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("live-indices"),
            contents: bytemuck::cast_slice(&mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let uniform = CameraUniform::from_camera(camera);
        let camera_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("live-camera-uniform"),
            contents: bytemuck::bytes_of(&uniform),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let settings = GpuSettings { ambient: self.ambient, haze: self.haze, time: time_secs, _pad1: 0.0 };
        let settings_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("live-settings-uniform"),
            contents: bytemuck::bytes_of(&settings),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("live-camera-bg"),
            layout: &self.camera_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: camera_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: settings_buffer.as_entire_binding() },
            ],
        });

        let mut gpu_lights: Vec<GpuPointLight> = mesh
            .lights
            .iter()
            .map(|l: &PointLight| GpuPointLight {
                position: [l.position.x, l.position.y, l.position.z, 1.0],
                color: [l.color[0], l.color[1], l.color[2], 1.0],
                direction_cos_angle: [
                    l.direction.x,
                    l.direction.y,
                    l.direction.z,
                    l.cone_half_angle_deg.to_radians().cos(),
                ],
            })
            .collect();
        while gpu_lights.len() < MIN_LIGHTS {
            gpu_lights.push(GpuPointLight {
                position: [0.0, 0.0, -1000.0, 1.0],
                color: [0.0, 0.0, 0.0, 0.0],
                direction_cos_angle: [0.0, 0.0, -1.0, -1.0],
            });
        }
        let lights_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("live-lights"),
            contents: bytemuck::cast_slice(&gpu_lights),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let lights_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("live-lights-bg"),
            layout: &self.lights_bind_group_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: lights_buffer.as_entire_binding() }],
        });

        let has_glow = !mesh.glow_indices.is_empty();

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ignition-live-encoder") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ignition-live-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa_view,
                    // Resolve now only if there's no glow pass to follow —
                    // resolving mid-sequence would throw away the
                    // multisampled buffer the glow pass needs to blend
                    // into. See `render_frame`'s doc comment.
                    resolve_target: if has_glow { None } else { Some(resolve_view) },
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.04, g: 0.045, b: 0.06, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &camera_bind_group, &[]);
            pass.set_bind_group(1, &lights_bind_group, &[]);
            pass.set_vertex_buffer(0, vertex_buffer.slice(..));
            pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
        }

        if has_glow {
            let glow_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("live-glow-vertices"),
                contents: bytemuck::cast_slice(&mesh.glow_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let glow_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("live-glow-indices"),
                contents: bytemuck::cast_slice(&mesh.glow_indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ignition-live-glow-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: msaa_view,
                    resolve_target: Some(resolve_view),
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.glow_pipeline);
            pass.set_bind_group(0, &camera_bind_group, &[]);
            pass.set_bind_group(1, &lights_bind_group, &[]);
            pass.set_vertex_buffer(0, glow_vertex_buffer.slice(..));
            pass.set_index_buffer(glow_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.glow_indices.len() as u32, 0, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
    }
}
