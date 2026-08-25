//! Real-window wgpu renderer for `src/bin/live.rs`. All the actual
//! rendering — shader, pipelines, the two-pass draw — lives in
//! `live_pipeline.rs` (`LivePipeline`), shared with
//! `live_headless_renderer.rs`. This file owns only what a window
//! specifically needs on top of that: the surface, its swapchain
//! configuration/resize handling, and acquiring/presenting a frame.

pub use crate::live_pipeline::{DEFAULT_AMBIENT, DEFAULT_HAZE};
use crate::live_pipeline::LivePipeline;
use crate::camera::Camera;
use crate::mesh::MeshBuilder;
use winit::window::Window;

pub struct LiveRenderer {
    pipeline: LivePipeline,
    adapter: wgpu::Adapter,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    depth_view: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl LiveRenderer {
    pub fn new(window: std::sync::Arc<Window>, ambient: f32, haze: f32) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let (width, height) = (size.width.max(1), size.height.max(1));

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(window)?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|e| anyhow::anyhow!("no wgpu adapter available: {e}"))?;

        let info = adapter.get_info();
        eprintln!("ignition-viz (live): using adapter {} ({:?})", info.name, info.backend);

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ignition-live-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
            ..Default::default()
        }))?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);

        let pipeline = LivePipeline::new(device, queue, surface_format, ambient, haze);
        let depth_view = pipeline.make_depth_view(width, height);
        let mut renderer = Self { pipeline, adapter, surface, surface_format, depth_view, width, height };
        renderer.configure_surface(width, height);
        Ok(renderer)
    }

    fn configure_surface(&mut self, width: u32, height: u32) {
        let caps = self.surface.get_capabilities(&self.adapter);
        self.surface.configure(
            &self.pipeline.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.surface_format,
                width,
                height,
                present_mode: caps.present_modes.first().copied().unwrap_or(wgpu::PresentMode::Fifo),
                alpha_mode: caps.alpha_modes[0],
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
                color_space: wgpu::SurfaceColorSpace::default(),
            },
        );
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.configure_surface(width, height);
        self.depth_view = self.pipeline.make_depth_view(width, height);
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }

    pub fn ambient_mut(&mut self) -> &mut f32 {
        &mut self.pipeline.ambient
    }

    pub fn haze_mut(&mut self) -> &mut f32 {
        &mut self.pipeline.haze
    }

    pub fn render(&self, mesh: &MeshBuilder, camera: &Camera) -> anyhow::Result<()> {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            // Transient — skip this frame, the next `request_redraw` retries.
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Outdated => return Ok(()),
            wgpu::CurrentSurfaceTexture::Lost => anyhow::bail!("surface lost"),
            wgpu::CurrentSurfaceTexture::Validation => anyhow::bail!("surface validation error"),
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.pipeline.render_frame(mesh, camera, &view, &self.depth_view);
        self.pipeline.queue.present(frame);
        Ok(())
    }
}
