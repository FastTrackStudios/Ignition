//! Running the visualizer inside somebody else's window.
//!
//! `run_windowed` owns a winit event loop; `run_snapshot` owns nothing
//! and renders one frame. This is the third case: the host owns the
//! window, the event loop *and the wgpu device*, and the visualizer is
//! a texture it composites.
//!
//! That last part is the whole trick. Bevy normally creates its own
//! `wgpu::Device`, and two devices cannot share a texture without
//! external-memory extensions nobody wants to write. `RenderCreation::
//! manual` lets Bevy be handed an existing device instead — so the host
//! creates one, Bevy renders into a texture on it, and the host samples
//! that texture directly. No copy, no readback, no second GPU context.
//!
//! It only works because the two halves agree on a wgpu version: Bevy
//! 0.19.1 and the Dioxus 0.8 line are both on wgpu 29, so cargo unifies
//! them to one crate and one `Device` type. A major-version split either
//! side of this and the approach is dead.

use crate::app::{VizConfig, VizPlugin, camera_bundle};
use crate::dmx::DmxUniverses;
use crate::gdtf_geometry::GdtfLibrary;
use crate::playback::Playback;
use bevy::app::{App, PluginsState};
use bevy::asset::AssetPlugin;
use bevy::camera::RenderTarget;
use bevy::image::Image;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::renderer::WgpuWrapper;
use bevy::render::renderer::{
    RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue,
};
use bevy::render::settings::RenderCreation;
use bevy::render::texture::GpuImage;
use bevy::render::{RenderApp, RenderPlugin};
use bevy::window::{ExitCondition, WindowPlugin};
use bevy::winit::WinitPlugin;
use std::sync::{Arc, Mutex};
use wgpu::{TextureFormat, TextureUsages};

/// A visualizer driven by a host application.
pub struct EmbeddedViz {
    app: App,
    target: Handle<Image>,
    size: (u32, u32),
    /// The last texture actually handed out. Kept so a resize — which
    /// costs a frame or two while the render world allocates the new
    /// target — shows the previous frame rather than a black flash.
    last_good: Option<wgpu::Texture>,
}

/// The host's wgpu objects, in the order `RenderCreation::manual` wants
/// them.
///
/// Taken as plain wgpu types rather than the host's own handle struct so
/// this crate does not depend on Dioxus, Blitz or any other host — the
/// only contract is "same wgpu".
pub struct HostGpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl EmbeddedViz {
    /// Builds the visualizer on the host's device.
    pub fn new(
        config: VizConfig,
        dmx: DmxUniverses,
        playback: Playback,
        gdtf: Option<GdtfLibrary>,
        gpu: HostGpu,
    ) -> Self {
        let (min, max) = config.venue.bounds();
        let view = config.view;
        let free_camera = config.camera;
        let size = (config.width.max(1), config.height.max(1));
        let assets_dir = config.assets_dir.clone();

        let adapter_info = gpu.adapter.get_info();
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: assets_dir,
                    ..default()
                })
                .set(WindowPlugin {
                    // The host owns the window. A lot of Bevy still
                    // expects the plugin to be present, just with
                    // nothing to show.
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: RenderCreation::manual(
                        RenderDevice::from(gpu.device),
                        RenderQueue(Arc::new(WgpuWrapper::new(gpu.queue))),
                        RenderAdapterInfo(WgpuWrapper::new(adapter_info)),
                        RenderAdapter(Arc::new(WgpuWrapper::new(gpu.adapter))),
                        RenderInstance(Arc::new(WgpuWrapper::new(gpu.instance))),
                    ),
                    // The host is presenting every frame; a pipeline
                    // still compiling would show as a hole in its UI
                    // rather than as one bad screenshot.
                    synchronous_pipeline_compilation: true,
                    ..default()
                })
                // The host's event loop is the only event loop.
                .disable::<WinitPlugin>()
                // Pipelined rendering calls `remove_sub_app(RenderApp)`
                // and drives it on its own thread — which means the
                // render world's `RenderAssets<GpuImage>`, where the
                // target texture lives, is no longer reachable from
                // here. Embedding needs to reach in and hand that
                // texture to the host every frame, so the pipelining has
                // to go. Costs a frame of latency-hiding we do not have
                // anyway, since the host controls when we render.
                .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
                // The host already installed a tracing subscriber, and a
                // process gets one. Left in, Bevy tries to install its
                // own, fails, and reports the failure at ERROR — so the
                // loudest line in a clean startup is the log system
                // complaining about the log system.
                .disable::<bevy::log::LogPlugin>(),
        )
        .add_plugins(VizPlugin {
            config,
            dmx,
            gdtf: Mutex::new(gdtf),
        })
        .insert_resource(playback);

        let target = add_target(&mut app, size);
        let camera_target: RenderTarget = target.clone().into();
        app.add_systems(Startup, move |mut commands: Commands| {
            commands.spawn((
                camera_bundle(view, free_camera, min, max),
                camera_target.clone(),
            ));
        });

        let mut viz = Self {
            app,
            target,
            size,
            last_good: None,
        };
        // Bevy allocates the target's GPU image in the render world, one
        // frame behind the main world, and pipelines compile on the
        // first draw. Without a few frames here the host's first paint
        // gets `None` and — since it only repaints on demand — may never
        // ask again.
        for _ in 0..3 {
            viz.step();
        }
        viz
    }

    /// One frame, once the plugins are ready. `false` while still
    /// starting up.
    fn step(&mut self) -> bool {
        match self.app.plugins_state() {
            PluginsState::Cleaned => {}
            PluginsState::Ready => {
                // What `run()` would have done.
                self.app.finish();
                self.app.cleanup();
            }
            other => {
                tracing::debug!(?other, "viz.embed: plugins not ready");
                return false;
            }
        }
        self.app.update();
        true
    }

    /// Advances one frame and returns the texture the host should
    /// composite.
    ///
    /// `None` until the render world has actually allocated the target,
    /// which takes a frame or two — the host draws nothing rather than
    /// a black hole.
    pub fn render(&mut self, width: u32, height: u32) -> Option<wgpu::Texture> {
        if width > 0 && height > 0 && (width, height) != self.size {
            self.resize(width, height);
        }
        if !self.step() {
            return self.last_good.clone();
        }
        // Hold the previous frame's texture while a new target warms up.
        // The render world allocates a `GpuImage` a frame behind the main
        // world, so a resize otherwise shows as a black flash for as
        // long as that takes.
        match self.texture() {
            Some(texture) => {
                self.last_good = Some(texture.clone());
                Some(texture)
            }
            None => self.last_good.clone(),
        }
    }

    /// The current target texture, without stepping.
    pub fn texture(&self) -> Option<wgpu::Texture> {
        let Some(render_app) = self.app.get_sub_app(RenderApp) else {
            tracing::debug!("viz.embed: no RenderApp sub-app");
            return None;
        };
        let Some(images) = render_app.world().get_resource::<RenderAssets<GpuImage>>() else {
            tracing::debug!("viz.embed: no RenderAssets<GpuImage>");
            return None;
        };
        if images.get(&self.target).is_none() {
            let present: Vec<String> = images.iter().map(|(id, _)| format!("{id:?}")).collect();
            tracing::debug!(
                want = ?self.target.id(),
                have = ?present,
                "viz.embed: target not in RenderAssets"
            );
        }
        images
            .get(&self.target)
            // `bevy_render`'s `Texture` is a newtype over the wgpu one
            // and derefs to it; cloning through the deref is what hands
            // the host the same GPU resource rather than a copy.
            .map(|gpu_image| (*gpu_image.texture).clone())
    }

    /// Re-creates the target at a new size and re-points the camera.
    ///
    /// A resize is a new `Image` asset rather than a mutation, because
    /// the render world caches a `GpuImage` per handle and there is no
    /// supported way to swap the texture under one. Dropping the old
    /// handle is what frees the old GPU texture — `self.target` holds
    /// the only strong reference besides the camera's, which is replaced
    /// below.
    ///
    /// This did not work the first time it was written, and the reason
    /// is worth keeping: the target was built with `Image::new_uninit`,
    /// which never gets extracted into the render world at all. That
    /// looked like "Bevy cannot re-point a render target", and it is
    /// not — it is `new_target_texture` or nothing, at any point in the
    /// app's life.
    fn resize(&mut self, width: u32, height: u32) {
        self.size = (width, height);
        let target = add_target(&mut self.app, self.size);
        let render_target: RenderTarget = target.clone().into();
        let mut cameras = self
            .app
            .world_mut()
            .query_filtered::<Entity, With<Camera>>();
        let entities: Vec<Entity> = cameras.iter(self.app.world()).collect();
        for entity in entities {
            self.app
                .world_mut()
                .entity_mut(entity)
                .insert(render_target.clone());
        }
        self.target = target;
    }

    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }
}

/// An offscreen colour target sized to the host's viewport.
fn add_target(app: &mut App, (width, height): (u32, u32)) -> Handle<Image> {
    // `new_target_texture` rather than a hand-rolled `new_uninit`: it
    // sets the usage flags *and* backs the image with real data, which
    // is what gets it extracted into the render world at all. An uninit
    // image never appears in `RenderAssets<GpuImage>`, so the host's
    // viewport stays black with no error anywhere — which is exactly the
    // way this failed the first time.
    let mut target = Image::new_target_texture(
        width,
        height,
        TextureFormat::Rgba8Unorm,
        // The sRGB view is what makes the colours right when Blitz
        // samples it; without it the viewport reads washed out.
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    // `new_target_texture` sets RENDER_ATTACHMENT | TEXTURE_BINDING |
    // COPY_DST, which is enough for a target nobody reads back. Bevy's
    // own `render_to_texture` pass copies *out* of it, so without
    // COPY_SRC every frame fails wgpu validation and the viewport stays
    // black — loudly, but only once you have a texture at all.
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    app.world_mut().resource_mut::<Assets<Image>>().add(target)
}
