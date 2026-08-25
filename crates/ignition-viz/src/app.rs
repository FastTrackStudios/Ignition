//! The one place that owns a `bevy::App`.
//!
//! Two entry points over one `VizPlugin`: `run_windowed` opens a window
//! and presents frames until the operator closes it, and `run_snapshot`
//! renders a fixed number of frames into an offscreen image, writes a PNG
//! and returns. The scene, the systems and the resources are identical —
//! only the render target and who drives the loop differ. That is why
//! there is a single `viz` binary now where there used to be `shot` and
//! `live`: those were two binaries because they were two separately
//! hand-written wgpu pipelines, not two different scenes.
//!
//! The snapshot path follows Bevy's own `externally_driven_headless_
//! renderer` example: no window, no winit, the update loop pumped by
//! hand, and the device polled after each frame so the capture has
//! actually happened before the next one is asked for.

use crate::beam::BeamPlugin;
use crate::playback::{go_on_space, tick_playback, Playback};
use crate::spawn::{
    apply_ambient, spawn_venue, update_beams, update_fixture_bodies, update_live_fixtures,
    BeamStyle, DmxRes, GdtfLibraryRes, VenueRes, VizSettings,
};
use crate::gdtf_geometry::GdtfLibrary;
use crate::view::ViewPreset;
use crate::{dmx, DmxUniverses, Venue};
use bevy::app::SubApps;
use bevy::asset::{AssetPlugin, RenderAssetUsages};
use bevy::camera::{Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::Image;
use bevy::light::VolumetricFog;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, PollType, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::render::RenderPlugin;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Everything the CLI can set. A plain struct, so `bin/viz.rs` stays
/// argument parsing and nothing else.
#[derive(Clone)]
pub struct VizConfig {
    pub venue: Venue,
    pub view: ViewPreset,
    pub width: u32,
    pub height: u32,
    pub haze: f32,
    pub ambient: f32,
    /// Whether to draw the props layer (drum kit, speakers, mics). Off by
    /// default: as placeholder boxes they clutter every shot without
    /// adding information worth the noise.
    pub show_props: bool,
    /// Room objects to leave out — see `VizSettings::exclude`.
    pub exclude: Vec<String>,
    /// Global exposure — see `VizSettings::exposure`.
    pub exposure: f32,
    /// What the venue's screens display — see
    /// `VizSettings::screen_content`.
    pub screen_content: Option<String>,
    /// Root directory the asset server loads from.
    pub assets_dir: String,
    /// How beams are drawn — see `BeamStyle`.
    pub beam_style: BeamStyle,
    /// Highest sACN/Art-Net universe to listen on.
    pub max_universe: u16,
    /// Render one frame to this path and exit, instead of opening a
    /// window.
    pub snapshot: Option<PathBuf>,
    /// How many frames to run before capturing. Bevy's first frames
    /// upload assets and warm pipelines, and a live rig also needs its
    /// first DMX packets to arrive; grabbing frame 0 catches a half-built
    /// scene and a dark rig.
    pub settle_frames: u32,
}

/// The visualizer itself: resources, the venue spawn, and the per-frame
/// live update. Deliberately free of any window or camera setup, so the
/// windowed and headless paths can each supply their own render target
/// and otherwise share everything.
pub struct VizPlugin {
    pub config: VizConfig,
    pub dmx: DmxUniverses,
    /// Taken on `build`, since `Plugin::build` only gets `&self` and a
    /// parsed GDTF library is neither cloneable nor cheap. A `Mutex`
    /// rather than a `Cell` because a `Plugin` must be `Sync`.
    pub gdtf: Mutex<Option<GdtfLibrary>>,
}

impl Plugin for VizPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClearColor(Color::BLACK))
            .insert_resource(VenueRes(self.config.venue.clone()))
            .insert_resource(DmxRes(self.dmx.clone()))
            .insert_resource(VizSettings {
                haze: self.config.haze,
                ambient: self.config.ambient,
                show_props: self.config.show_props,
                exclude: self.config.exclude.clone(),
                beam_style: self.config.beam_style,
                exposure: self.config.exposure,
                screen_content: self.config.screen_content.clone(),
            })
            .insert_resource(GdtfLibraryRes(self.gdtf.lock().expect("gdtf library lock").take()))
            .add_plugins(BeamPlugin)
            .add_systems(Startup, spawn_venue)
            // Playback runs before the fixture update so a beam reflects
            // this frame's cue state, not last frame's.
            .add_systems(
                Update,
                (
                    apply_ambient,
                    go_on_space,
                    tick_playback,
                    update_live_fixtures,
                    update_fixture_bodies,
                )
                    .chain(),
            )
            // After transform propagation, because a beam's world pose is
            // whatever the joints `update_live_fixtures` just moved ended
            // up producing — see `update_beams`.
            .add_systems(
                PostUpdate,
                update_beams.after(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// `playback` is inserted as a resource rather than carried on the plugin
/// because `Plugin::build` only gets `&self`, and the players are not
/// cloneable — they hold live fade state.
pub fn run(config: VizConfig, playback: Playback, gdtf: Option<GdtfLibrary>) {
    let dmx = DmxUniverses::new();
    dmx::spawn_sacn_listener(dmx.clone(), config.max_universe);
    dmx::spawn_artnet_listener(dmx.clone());

    match config.snapshot.clone() {
        Some(path) => run_snapshot(config, dmx, playback, gdtf, &path),
        None => run_windowed(config, dmx, playback, gdtf),
    }
}

fn run_windowed(config: VizConfig, dmx: DmxUniverses, playback: Playback, gdtf: Option<GdtfLibrary>) {
    let (min, max) = config.venue.bounds();
    let view = config.view;
    let (width, height) = (config.width, config.height);
    let assets_dir = config.assets_dir.clone();

    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: assets_dir.clone(),
            ..default()
        }).set(WindowPlugin {
            primary_window: Some(Window {
                title: "Ignition — visualizer".into(),
                resolution: (width, height).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(VizPlugin { config, dmx, gdtf: Mutex::new(gdtf) })
        .insert_resource(playback)
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(camera_bundle(view, min, max));
        })
        .run();
}

/// No window, no winit — an offscreen image target, the loop pumped by
/// hand, and the GPU polled between frames.
fn run_snapshot(
    config: VizConfig,
    dmx: DmxUniverses,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    path: &Path,
) {
    let (min, max) = config.venue.bounds();
    let view = config.view;
    let (width, height) = (config.width, config.height);
    let settle_frames = config.settle_frames.max(1);

    let assets_dir = config.assets_dir.clone();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(AssetPlugin { file_path: assets_dir, ..default() })
            .set(WindowPlugin {
                // A lot of Bevy still expects the plugin to be present,
                // just with nothing to show.
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                // Otherwise the first frames render with pipelines that
                // have not finished compiling, and a snapshot can catch
                // a scene mid-warm-up.
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins(VizPlugin { config, dmx, gdtf: Mutex::new(gdtf) })
    .insert_resource(playback);

    let mut target = Image::new_uninit(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    target.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let target: RenderTarget = app.world_mut().resource_mut::<Assets<Image>>().add(target).into();

    let camera_target = target.clone();
    app.add_systems(Startup, move |mut commands: Commands| {
        // `RenderTarget` is its own component — adding it to the camera
        // is what points it at the offscreen image instead of a window.
        commands.spawn((camera_bundle(view, min, max), camera_target.clone()));
    });

    // No `run()`: the schedule runner is replaced by the loop below, so
    // the app has to be finished and cleaned up by hand.
    app.finish();
    app.cleanup();
    let mut subapps: SubApps = std::mem::take(app.sub_apps_mut());

    let step = |subapps: &mut SubApps| {
        subapps.update();
        // Wait for the frame to actually finish on the GPU, so a capture
        // requested this frame is complete before the next one starts.
        subapps
            .main
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait { submission_index: None, timeout: None })
            .expect("polling the render device");
    };

    for _ in 0..settle_frames {
        step(&mut subapps);
    }
    let out = path.to_path_buf();
    subapps
        .main
        .world_mut()
        .spawn(Screenshot::image(target.as_image().expect("snapshot target is an image").clone()))
        .observe(save_to_disk(out));
    // The capture is queued on the render world and the PNG encode runs
    // on a task pool thread, so the app has to keep stepping for a few
    // frames afterwards, and then outlive the encode itself — dropping
    // `subapps` takes the task pools down with it.
    for _ in 0..8 {
        step(&mut subapps);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !path.exists() && std::time::Instant::now() < deadline {
        step(&mut subapps);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !path.exists() {
        eprintln!("viz: snapshot never reached {}", path.display());
    }
}

/// The camera every mode uses: HDR so bloom has something above white to
/// work with, a tonemapper that desaturates as it clips, and the preset's
/// own framing.
fn camera_bundle(view: ViewPreset, min: Vec3, max: Vec3) -> impl Bundle {
    (
        Camera3d::default(),
        // Without HDR the additive beam material clips at 1.0 and there
        // is nothing left above white for bloom to find — which is most
        // of what makes a beam read as light rather than as a shape.
        Hdr,
        Projection::Perspective(PerspectiveProjection {
            fov: view.fov_y_deg().to_radians(),
            near: 0.05,
            far: view.far(min, max),
            ..default()
        }),
        // Desaturates toward white as it clips, which is what stops a
        // saturated red wash from flattening into one flat red — the
        // exact failure the hand-written ACES pass produced.
        Tonemapping::TonyMcMapface,
        Bloom::NATURAL,
        view.transform(min, max),
        // Only does anything when there is a `FogVolume` in the scene,
        // so it costs nothing in `BeamStyle::Shader` and there is no
        // reason to make the camera's shape depend on the beam style.
        VolumetricFog {
            // No environment map, so nothing should be lit by one.
            ambient_intensity: 0.0,
            // Well above the default 64. A church auditorium is ~20m
            // deep and a beam crosses most of it, so the default step
            // spacing lands visibly wide and every shaft comes out in
            // stair-steps. Cost is per-pixel raymarching, which is worth
            // it here: beams *are* the picture.
            step_count: 192,
            // No jitter. It offsets each ray's start depth to trade
            // banding for noise, and Bevy's own docs say it is meant for
            // use *with* temporal antialiasing — which resolves that
            // noise across frames. There is no TAA here, so all it did
            // was speckle every lit surface. The step count above is
            // what buys smoothness instead.
            jitter: 0.0,
            ..default()
        },
    )
}
