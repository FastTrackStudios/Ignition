//! Headless (no window, no surface) render-to-PNG using the *live*
//! pipeline (`live_pipeline.rs`'s `LivePipeline` — the same point lights +
//! additive beam-cone pass `live_renderer.rs`'s window uses), instead of
//! `renderer.rs`'s plain headless pipeline.
//!
//! Exists for exactly one reason: proving what the live DMX/lighting work
//! actually looks like without a real display attached, the same way
//! `HeadlessRenderer`/`shot` proves the static venue model without one.
//! `src/bin/live.rs`'s `--snapshot <path>` flag is the CLI entry point —
//! run a short DMX warm-up, build one frame of the scene, render it here,
//! write a PNG, exit.

use crate::camera::Camera;
use crate::live_pipeline::LivePipeline;
use crate::mesh::MeshBuilder;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub struct LiveHeadlessRenderer {
    pipeline: LivePipeline,
}

impl LiveHeadlessRenderer {
    /// See `live_pipeline::{DEFAULT_AMBIENT, DEFAULT_HAZE}` for the
    /// reasoning behind those defaults.
    pub fn new(ambient: f32, haze: f32) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            ..Default::default()
        }))
        .map_err(|e| anyhow::anyhow!("no wgpu adapter available: {e}"))?;

        let info = adapter.get_info();
        eprintln!("ignition-viz (live headless): using adapter {} ({:?})", info.name, info.backend);

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("ignition-live-headless-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
            ..Default::default()
        }))?;

        let pipeline = LivePipeline::new(device, queue, COLOR_FORMAT, ambient, haze);
        Ok(Self { pipeline })
    }

    pub fn render_to_png(
        &self,
        mesh: &MeshBuilder,
        camera: &Camera,
        width: u32,
        height: u32,
        time_secs: f32,
        out_path: &std::path::Path,
    ) -> anyhow::Result<()> {
        let device = &self.pipeline.device;

        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("color-target"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = self.pipeline.make_depth_view(width, height);
        let msaa_view = self.pipeline.make_msaa_color_view(width, height, COLOR_FORMAT);

        self.pipeline.render_frame(mesh, camera, &msaa_view, &color_view, &depth_view, time_secs);

        // Readback: rows must be padded to a 256-byte alignment.
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: (padded_bytes_per_row * height) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("ignition-live-headless-readback-encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &color_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.pipeline.queue.submit(Some(encoder.finish()));

        let slice = output_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None })?;
        rx.recv()??;

        let data = slice.get_mapped_range()?;
        let mut pixels = vec![0u8; (unpadded_bytes_per_row * height) as usize];
        for row in 0..height {
            let src_start = (row * padded_bytes_per_row) as usize;
            let src = &data[src_start..src_start + unpadded_bytes_per_row as usize];
            let dst_start = (row * unpadded_bytes_per_row) as usize;
            pixels[dst_start..dst_start + unpadded_bytes_per_row as usize].copy_from_slice(src);
        }
        drop(data);
        output_buffer.unmap();

        let img = image::RgbaImage::from_raw(width, height, pixels)
            .ok_or_else(|| anyhow::anyhow!("pixel buffer size mismatch"))?;
        img.save(out_path)?;
        Ok(())
    }
}
