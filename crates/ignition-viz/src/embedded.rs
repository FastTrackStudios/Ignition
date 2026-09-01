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

use crate::app::{CameraSpec, VizConfig, VizPlugin, spawn_camera};
use crate::dmx::DmxUniverses;
use crate::gdtf_geometry::GdtfLibrary;
use crate::playback::Playback;
use bevy::app::{App, PluginsState};
use bevy::asset::{AssetId, AssetPlugin};
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

/// Where the render thread leaves the target texture for the host.
///
/// With pipelined rendering the render world lives on its own thread
/// and is not reachable from here, so a system over there looks the
/// target up in `RenderAssets<GpuImage>` every frame and posts the
/// `wgpu::Texture` — a cheap `Arc` handle — into this mailbox. The host
/// reads the last one posted. `wanted` is what the main thread wants
/// posted, which changes only on a resize.
#[derive(Resource, Clone, Default)]
pub struct TargetMailbox(Arc<Mutex<Mailbox>>);

#[derive(Default)]
struct Mailbox {
    wanted: Option<AssetId<Image>>,
    texture: Option<(AssetId<Image>, wgpu::Texture)>,
    /// The programme camera's target, while a host pane shows it —
    /// see `EmbeddedViz::programme_texture`.
    programme_wanted: Option<AssetId<Image>>,
    programme: Option<(AssetId<Image>, wgpu::Texture)>,
    pipelines: usize,
}

/// Render-world side: posts the wanted targets' textures once they exist.
fn post_target_texture(
    mailbox: Res<TargetMailbox>,
    images: Res<RenderAssets<GpuImage>>,
    pipelines: Res<bevy::render::render_resource::PipelineCache>,
) {
    let mut mailbox = mailbox.0.lock().expect("target mailbox");
    mailbox.pipelines = pipelines.pipelines().count();
    if let Some(wanted) = mailbox.wanted
        && mailbox.texture.as_ref().is_none_or(|(id, _)| *id != wanted)
        && let Some(image) = images.get(wanted)
    {
        mailbox.texture = Some((wanted, (*image.texture).clone()));
    }
    if let Some(wanted) = mailbox.programme_wanted
        && mailbox
            .programme
            .as_ref()
            .is_none_or(|(id, _)| *id != wanted)
        && let Some(image) = images.get(wanted)
    {
        mailbox.programme = Some((wanted, (*image.texture).clone()));
    }
}

/// A visualizer driven by a host application.
pub struct EmbeddedViz {
    app: App,
    target: Handle<Image>,
    mailbox: TargetMailbox,
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
        Self::new_with(config, dmx, playback, gdtf, gpu, |_| {})
    }

    /// `new`, with a hook that sees the `App` after every plugin is
    /// added and before the first frame — where `--bench` hangs the
    /// diagnostics plugins off the *same* construction the studio uses,
    /// so what it measures is the studio's route and not a cousin of it.
    pub fn new_with(
        config: VizConfig,
        dmx: DmxUniverses,
        playback: Playback,
        gdtf: Option<GdtfLibrary>,
        gpu: HostGpu,
        configure: impl FnOnce(&mut App),
    ) -> Self {
        let quality = config.quality.for_rig(&config.venue, gdtf.as_ref());
        let spec = CameraSpec::new(&config, quality);
        let size = (config.width.max(1), config.height.max(1));
        let assets_dir = config.assets_dir.clone();
        // Bound here, on the host's universes, so the studio's OUTPUT
        // key drives the same transmitter the windowed viz would.
        let output = crate::app::bind_output(&config, &dmx);

        let adapter_info = gpu.adapter.get_info();
        let mut app = App::new();
        app.add_plugins(crate::gdtf_assets::GdtfSourcePlugin);
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
                // Pipelined rendering stays on. It moves the render
                // world to a thread of its own, so a host paint costs
                // the main world's update plus the wait for the
                // *previous* frame's render — the two halves overlap
                // instead of adding up. The price is that the render
                // world is no longer reachable from here, which is
                // what `TargetMailbox` is for.
                // r[impl viz.performance-budget] - the studio renders on a thread of its own
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
            output: Mutex::new(Some(output)),
        })
        .insert_resource(playback);

        // The host takes selections and sends them as `Command::Select`,
        // so a click in the viewport travels the same road as a tile.
        app.insert_resource(crate::picking::SelectionRoute::Host);
        // The host owns the keyboard and drains the cue commands itself
        // (`Command::Camera`, and `camera …` beside `macro …`).
        {
            let mut active = app
                .world_mut()
                .resource_mut::<crate::camera::ActiveCamera>();
            active.keys = false;
            active.host_drains_cues = true;
        }
        configure(&mut app);
        let mailbox = TargetMailbox::default();
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.insert_resource(mailbox.clone()).add_systems(
                bevy::render::Render,
                post_target_texture.after(bevy::render::RenderSystems::PrepareAssets),
            );
        }
        let target = add_target(&mut app, size);
        mailbox.0.lock().expect("target mailbox").wanted = Some(target.id());
        let camera_target: RenderTarget = target.clone().into();
        app.add_systems(
            Startup,
            move |mut commands: Commands,
                  mut curves: ResMut<
                Assets<bevy::post_process::auto_exposure::AutoExposureCompensationCurve>,
            >| {
                let camera = spawn_camera(&mut commands, &mut curves, spec);
                commands.entity(camera).insert(camera_target.clone());
            },
        );

        let mut viz = Self {
            app,
            target,
            mailbox,
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
        // r[impl studio.profiling] - the main-world update, on its own
        let _span = tracing::info_span!(target: "ignition::profile", "viz.step").entered();
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
        // The whole visualizer frame: resize, main-world update,
        // extraction and command encoding. With pipelined rendering the
        // GPU keeps working after this returns — what it costs shows up
        // as `blitz.render`'s own self time, when the submit waits.
        // r[impl studio.profiling] - one visualizer frame
        let _span = tracing::info_span!(target: "ignition::profile", "viz.render").entered();
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

    /// Whether a host pane shows the programme camera, and at what
    /// size. `None` takes the programme camera down (unless a canvas
    /// still samples it) so a lone viewport pays nothing for it.
    // r[impl viz.programme-view] - the host switches the second camera on and off
    pub fn set_programme(&mut self, size: Option<(u32, u32)>) {
        let world = self.app.world_mut();
        let mut view = world.resource_mut::<crate::camera::ProgrammeView>();
        let wants = size.is_some();
        if let Some((w, h)) = size
            && (w, h) != view.size
            && w > 0
            && h > 0
        {
            view.size = (w, h);
            // A new size is a new image; the camera follows it next frame.
            if let Some(camera) = view.camera.take() {
                world.commands().entity(camera).despawn();
            }
            let target = world
                .resource_mut::<Assets<Image>>()
                .add(crate::camera::camera_target((w, h)));
            let mut view = world.resource_mut::<crate::camera::ProgrammeView>();
            view.target = target;
        }
        let mut view = world.resource_mut::<crate::camera::ProgrammeView>();
        if view.host_wants != wants {
            view.host_wants = wants;
        }
        let id = view.target.id();
        self.mailbox
            .0
            .lock()
            .expect("target mailbox")
            .programme_wanted = wants.then_some(id);
    }

    /// The programme camera's texture, once it has rendered — `None`
    /// until then, or while nothing wants the programme.
    // r[impl viz.programme-view] - the second texture the host composites
    pub fn programme_texture(&self) -> Option<wgpu::Texture> {
        let wanted = self
            .app
            .world()
            .get_resource::<crate::camera::ProgrammeView>()
            .filter(|v| v.host_wants)?
            .target
            .id();
        let mailbox = self.mailbox.0.lock().expect("target mailbox");
        match &mailbox.programme {
            Some((id, texture)) if *id == wanted => Some(texture.clone()),
            _ => None,
        }
    }

    /// The current target texture, without stepping: whatever the
    /// render thread last posted for the current target. `None` until
    /// it has allocated one, which takes a frame or two.
    pub fn texture(&self) -> Option<wgpu::Texture> {
        let mailbox = self.mailbox.0.lock().expect("target mailbox");
        match &mailbox.texture {
            // `bevy_render`'s `Texture` is a newtype over the wgpu one;
            // what was posted is the same GPU resource, not a copy.
            Some((id, texture)) if *id == self.target.id() => Some(texture.clone()),
            _ => {
                tracing::debug!(want = ?self.target.id(), "viz.embed: target not posted yet");
                None
            }
        }
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
        // Only the cameras that drew into the old target follow it: the
        // haze camera has a target of its own, and pointing it at the
        // main image made its composite quad sample the texture it was
        // rendering into — a wgpu validation panic on the render
        // thread, which with pipelined rendering the main thread then
        // waits on forever. That was "the visualizer freezes when I
        // resize it".
        // r[impl studio.windows.visualizer-anywhere] - a resize re-points only the main target's cameras
        let old = self.target.id();
        let mut cameras = self
            .app
            .world_mut()
            .query_filtered::<(Entity, &RenderTarget), With<Camera>>();
        let entities: Vec<Entity> = cameras
            .iter(self.app.world())
            .filter(|(_, t)| matches!(t, RenderTarget::Image(img) if img.handle.id() == old))
            .map(|(e, _)| e)
            .collect();
        for entity in entities {
            self.app
                .world_mut()
                .entity_mut(entity)
                .insert(render_target.clone());
        }
        self.mailbox.0.lock().expect("target mailbox").wanted = Some(target.id());
        self.target = target;
    }

    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// The image the camera renders into.
    pub fn target(&self) -> Handle<Image> {
        self.target.clone()
    }

    /// The host's pointer, in pixels of the target texture — what
    /// `picking.rs` turns into `bevy_picking` input on the next frame.
    /// `None` for a pointer that has left the viewport.
    // r[impl studio.program.pick-and-gizmos] - pointer input arrives from the host
    pub fn pointer(
        &mut self,
        position: Option<(f32, f32)>,
        primary: bool,
        shift: bool,
        ctrl: bool,
    ) {
        let next = crate::picking::HostPointer {
            position: position.map(|(x, y)| Vec2::new(x, y)),
            primary,
            shift,
            ctrl,
        };
        let mut pointer = self
            .app
            .world_mut()
            .resource_mut::<crate::picking::HostPointer>();
        if *pointer != next {
            *pointer = next;
        }
    }

    /// A selection the viewport asked for since the last take — the
    /// channels clicked, or an empty list for a click that cleared it.
    pub fn take_selection(&mut self) -> Option<Vec<ignition_proto::ChanId>> {
        self.app
            .world_mut()
            .resource_mut::<crate::picking::SelectionRequest>()
            .take()
    }

    /// One of the programmer's overlays, or the master switch — see
    /// `gizmos::ProgramOverlays`.
    pub fn overlays_mut(&mut self) -> Mut<'_, crate::gizmos::ProgramOverlays> {
        self.app
            .world_mut()
            .resource_mut::<crate::gizmos::ProgramOverlays>()
    }

    /// The group the Library is hovering, outlined in the room.
    pub fn highlight_group(&mut self, name: Option<String>) {
        let mut highlight = self
            .app
            .world_mut()
            .resource_mut::<crate::gizmos::HighlightGroup>();
        if highlight.0 != name {
            highlight.0 = name;
        }
    }

    /// How many render pipelines the render world had compiled as of
    /// the last frame it rendered.
    pub fn pipeline_count(&self) -> usize {
        self.mailbox.0.lock().expect("target mailbox").pipelines
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
