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
use crate::gdtf_geometry::GdtfLibrary;
use crate::output::DmxOutput;
use crate::playback::{Playback, operator_keys, tick_playback};
use crate::spawn::{
    BeamStyle, CanvasClock, DmxRes, GdtfLibraryRes, LiveDmx, VenueRes, VizSettings, apply_ambient,
    resolve_live_dmx, spawn_venue, update_beams, update_canvas_videos, update_fixture_bodies,
    update_live_fixtures,
};
use crate::video::export::{self, ExportRequest, FrameSchedule};
use crate::view::ViewPreset;
use crate::{DmxUniverses, Venue, dmx};
use bevy::app::SubApps;
use bevy::asset::{AssetPlugin, RenderAssetUsages};
use bevy::camera::{Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::Image;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::{
    Extent3d, PollType, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::time::TimeUpdateStrategy;
use bevy::window::ExitCondition;
use bevy::winit::WinitPlugin;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

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
    /// Whether to draw the props layer (people, drum kit, speakers,
    /// mics). On by default now that the people and the kit are real
    /// models rather than the placeholder boxes that used to clutter
    /// every shot without adding information worth the noise.
    pub show_props: bool,
    /// An explicit eye/target pair, overriding `view`'s framing. What
    /// `--eye`/`--look` set, for inspecting one corner of the room.
    pub camera: Option<(Vec3, Vec3)>,
    /// Draw the operator overlay — the cue list with cooked status.
    /// Always on in a window; `--overlay` turns it on for a snapshot
    /// too, which is how a still can carry the cue context that makes it
    /// mean something.
    pub overlay: bool,
    /// Draw the frame-rate readout in the corner.
    ///
    /// Separate from `overlay` because the studio wants this without the
    /// cue list — it already draws the cue list in its own sidebar, and
    /// two of them on one screen is worse than none.
    pub fps: bool,
    /// Room objects to leave out — see `VizSettings::exclude`.
    pub exclude: Vec<String>,
    /// Global exposure — see `VizSettings::exposure`.
    pub exposure: f32,
    /// What the venue's screens display — see
    /// `VizSettings::screen_content`.
    pub screen_content: Option<String>,
    /// Per-canvas sources — `--canvas main=clips/city.png`. A canvas
    /// with no entry falls back to `screen_content`.
    pub canvas_content: std::collections::HashMap<String, String>,
    /// Where each canvas's crop is centred in its source, 0..=1; see
    /// `VizSettings::canvas_focus`.
    pub canvas_focus: std::collections::HashMap<String, f32>,
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
    /// How much GPU to spend per pixel — see `RenderQuality`.
    pub quality: RenderQuality,
    /// Whether DMX leaves the socket. The transmitter is bound either
    /// way, from the venue's config, so the switch is instant; this is
    /// its starting position. Off by default: a visualizer on a laptop
    /// should not take over a rig until asked.
    // r[impl dmx.output-toggle] - the starting position of the switch
    pub output: bool,
    /// Attach the loopback sink — see `output::LoopbackSink` for why
    /// this is a verification path and not the normal one.
    pub loopback: bool,
    /// Whether a lit fixture's housing glows its own colour — see
    /// `VizSettings::body_glow`. Off by default: the real fixtures are
    /// black.
    pub body_glow: bool,
}

/// The per-pixel cost dials: what a still can afford and a live view
/// cannot.
///
/// The volumetric pass raymarches every pixel `fog_steps` times, and
/// MSAA multiplies every fragment by its sample count. Together they
/// were most of the frame in the studio, where the picture has to
/// arrive every 8 ms; a snapshot has all day and keeps the old numbers
/// so stills do not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderQuality {
    /// `VolumetricFog::step_count`.
    pub fog_steps: u32,
    /// Whether the camera multisamples. Bloom and fog blur beam edges
    /// enough that turning it off is hard to see and easy to measure.
    pub msaa: bool,
    /// The haze is marched at `1/fog_scale` of the picture's size — see
    /// `haze.rs`. `1` marches it on the camera itself, at full size,
    /// which is what every existing snapshot was made with; `0` picks
    /// the scale from the picture's size so the haze camera stays
    /// within `haze::HAZE_PIXEL_BUDGET` pixels whatever the viewport.
    pub fog_scale: u32,
}

impl RenderQuality {
    /// What a headless still renders at: the quality every existing
    /// snapshot was made with.
    pub const STILL: Self = Self {
        fog_steps: 192,
        msaa: true,
        fog_scale: 1,
    };

    /// What a window or the studio renders at. `IGNITION_FOG_STEPS`
    /// overrides the step count, for comparing on a given GPU without a
    /// rebuild.
    pub fn live() -> Self {
        let fog_steps = std::env::var("IGNITION_FOG_STEPS")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|n| *n > 0)
            // 128 rather than 64: at 64 the raymarch quantises a mover's
            // cone into rings and dots, and a 1.7-degree beam vanishes
            // outright for whole frames. 128 is the fewest that keeps
            // every shaft a solid shaft; the frame budget is spent on
            // that before anything else, and the haze camera's pixel
            // budget is what pays for it.
            .unwrap_or(128);
        // By pixel budget: the fog's cost is the haze camera's pixels
        // times the steps, and a viewport twice as wide should not cost
        // twice as much haze. `IGNITION_FOG_SCALE` forces a scale.
        let fog_scale = std::env::var("IGNITION_FOG_SCALE")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(0);
        Self {
            fog_steps,
            msaa: false,
            fog_scale,
        }
    }

    /// The same dials, with the raymarch made fine enough for the rig
    /// in front of it. A narrow shaft — a mini gobo mover's 11 degrees,
    /// a beam fixture's 2 — is a few decimetres across for most of its
    /// length, and at a live step count the fog samples cross it only
    /// once or twice per pixel: the shaft comes out as a string of dots
    /// and rings instead of a solid cone. Only a rig that actually cuts
    /// such a shaft pays for the finer march; a room of pars keeps the
    /// live count.
    pub fn for_rig(self, venue: &Venue, gdtf: Option<&GdtfLibrary>) -> Self {
        // An explicit `IGNITION_FOG_STEPS` is the operator comparing
        // counts on their own GPU; the rig does not get to overrule it.
        let forced = std::env::var("IGNITION_FOG_STEPS")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|n| *n > 0);
        // The raise for a narrow shaft is a full-size dial: on the haze
        // camera it doubles the fog's cost for a beam the pixel budget
        // has already made a couple of pixels wide, and 128 steps keep
        // that beam solid — see `haze.rs`.
        let steps = match (forced, self.fog_scale) {
            (Some(forced), _) => forced,
            (None, 1) => fog_steps_for(self.fog_steps, narrowest_shaft_full_angle_deg(venue, gdtf)),
            (None, _) => self.fog_steps,
        };
        Self {
            fog_steps: steps,
            msaa: self.msaa,
            fog_scale: self.fog_scale,
        }
    }
}

/// Full beam angles below this need the finer march — see
/// `RenderQuality::for_rig`.
pub const NARROW_BEAM_FULL_ANGLE_DEG: f32 = 15.0;

/// The step count a narrow shaft needs to read as a solid cone in the
/// studio viewport: twice the live count, which is where the dots go.
pub const NARROW_BEAM_FOG_STEPS: u32 = 256;

/// `base`, raised to `NARROW_BEAM_FOG_STEPS` when the rig's narrowest
/// shaft-cutting beam is under `NARROW_BEAM_FULL_ANGLE_DEG`. Never
/// lowered: a still's count is already above it.
pub fn fog_steps_for(base: u32, narrowest_shaft_full_angle_deg: Option<f32>) -> u32 {
    match narrowest_shaft_full_angle_deg {
        Some(angle) if angle < NARROW_BEAM_FULL_ANGLE_DEG => base.max(NARROW_BEAM_FOG_STEPS),
        _ => base,
    }
}

/// The full beam angle of the narrowest patched fixture bright enough
/// per-direction to cut a shaft (see `SHAFT_CANDELA_THRESHOLD`), if any.
///
/// The angle is the profile's where one resolves, the patch's only
/// otherwise — the same rule the emitters follow.
pub fn narrowest_shaft_full_angle_deg(venue: &Venue, gdtf: Option<&GdtfLibrary>) -> Option<f32> {
    use crate::fixture_profile::{
        LUMENS_PER_WATT, SHAFT_CANDELA_THRESHOLD, peak_candela, power_watts,
    };
    use crate::spawn::fixture_optics;
    venue
        .fixtures
        .iter()
        .filter(|f| f.patched)
        .filter_map(|f| {
            let manufacturer = f.manufacturer.as_deref().unwrap_or("");
            let model = f.model.as_deref().unwrap_or("");
            let profile = gdtf.and_then(|lib| lib.find(manufacturer, model));
            let half = fixture_optics(f, profile).beam_half_deg;
            let lumens = power_watts(manufacturer, model) * LUMENS_PER_WATT;
            (peak_candela(lumens, half) >= SHAFT_CANDELA_THRESHOLD).then_some(half * 2.0)
        })
        .min_by(|a, b| a.total_cmp(b))
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
    /// The transmitter, taken on `build` for the same reason — a
    /// `Sender` owns threads and sockets and is not cloneable. `None`
    /// means a viz that sends nothing (a snapshot, an export).
    pub output: Mutex<Option<DmxOutput>>,
}

/// Binds the venue's output config, enabled per `config.output`, with
/// the loopback sink when asked for. Errors land on the resource, to be
/// shown; nothing here can fail the launch.
// r[impl dmx.venue-config] - bound from the venue, at load
pub fn bind_output(config: &VizConfig, dmx: &DmxUniverses) -> DmxOutput {
    let source = "Ignition".to_string();
    let output = DmxOutput::bind(&config.venue.output_config(), &source, config.output);
    if crate::output::loopback_requested(config.loopback) {
        tracing::info!("dmx output: loopback sink attached");
        output.with_sink(Box::new(crate::output::LoopbackSink(dmx.clone())))
    } else {
        output
    }
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
                overlay: self.config.overlay,
                fps: self.config.fps,
                exclude: self.config.exclude.clone(),
                beam_style: self.config.beam_style,
                body_glow: self.config.body_glow,
                exposure: self.config.exposure,
                screen_content: self.config.screen_content.clone(),
                canvas_content: self.config.canvas_content.clone(),
                canvas_focus: self.config.canvas_focus.clone(),
                assets_dir: self.config.assets_dir.clone(),
            })
            // Free-running by default, which is what the standalone
            // binary wants; a host driving the visualizer from a
            // transport overwrites it every frame. See `CanvasClock`.
            .init_resource::<CanvasClock>()
            .insert_resource(GdtfLibraryRes(
                self.gdtf.lock().expect("gdtf library lock").take(),
            ))
            .add_plugins((BeamPlugin, crate::haze::HazePlugin))
            // Room for the whole rig's lights before the GPU clustering
            // buffers have to grow. Bevy resizes them when they overflow
            // and says so — "the scene lighting may have been corrupted
            // for a few frames" — which on a venue with seventy-odd
            // fixtures happens on the first busy cue. Sized up front, so
            // the corruption never happens rather than happening once.
            .init_resource::<LiveDmx>()
            .insert_resource(
                self.output
                    .lock()
                    .expect("dmx output lock")
                    .take()
                    .unwrap_or_default(),
            )
            .add_systems(Startup, widen_light_clusters)
            // Frame timing for the overlay's FPS readout. Bevy's own
            // plugin rather than a hand-rolled delta average, because it
            // keeps a smoothed history — a per-frame reciprocal flickers
            // too much to read, and the question is whether the rig
            // *holds* a rate, not what one frame did.
            .add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_venue)
            // Playback runs before the fixture update so a beam reflects
            // this frame's cue state, not last frame's.
            .add_systems(
                Update,
                (
                    apply_ambient,
                    operator_keys,
                    tick_playback,
                    // The frame goes out from the same bytes the next
                    // system decodes — after the encoder, before the
                    // picture.
                    crate::output::send_output,
                    resolve_live_dmx,
                    update_live_fixtures,
                    update_fixture_bodies,
                )
                    .chain(),
            )
            .add_systems(Update, crate::props::pose_new_characters)
            // Not in the chain above: a canvas frame does not depend on
            // anything a fixture did this frame, and holding up the rig
            // for a texture upload is the one thing the whole video path
            // is built to avoid.
            .add_systems(Update, update_canvas_videos)
            // Camera targeting covers both readouts, so it is gated on
            // either being on — not on `overlay`, or an fps-only studio
            // would spawn the text and then draw it nowhere.
            .add_systems(
                Update,
                crate::overlay::target_overlay_camera
                    .run_if(|s: Res<VizSettings>| s.overlay || s.fps),
            )
            .add_systems(
                Update,
                crate::overlay::update_overlay
                    .after(crate::overlay::target_overlay_camera)
                    .run_if(|s: Res<VizSettings>| s.overlay),
            )
            .add_systems(
                Startup,
                crate::overlay::spawn_overlay.run_if(|s: Res<VizSettings>| s.overlay),
            )
            .add_systems(
                Update,
                crate::overlay::update_fps.run_if(|s: Res<VizSettings>| s.fps),
            )
            .add_systems(
                Startup,
                crate::overlay::spawn_fps.run_if(|s: Res<VizSettings>| s.fps),
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
    let output = bind_output(&config, &dmx);

    match config.snapshot.clone() {
        Some(path) => run_snapshot(config, dmx, playback, gdtf, &path),
        None => run_windowed(config, dmx, playback, gdtf, output),
    }
}

fn run_windowed(
    config: VizConfig,
    dmx: DmxUniverses,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    output: DmxOutput,
) {
    let (min, max) = config.venue.bounds();
    let view = config.view;
    let free_camera = config.camera;
    let quality = config.quality.for_rig(&config.venue, gdtf.as_ref());
    let (width, height) = (config.width, config.height);
    let assets_dir = config.assets_dir.clone();

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: assets_dir.clone(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Ignition — visualizer".into(),
                        resolution: (width, height).into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(VizPlugin {
            config,
            dmx,
            gdtf: Mutex::new(gdtf),
            output: Mutex::new(Some(output)),
        })
        .insert_resource(playback)
        .add_systems(Startup, move |mut commands: Commands| {
            commands.spawn(camera_bundle(view, free_camera, min, max, quality));
        })
        .run();
}

/// A headless app rendering into an offscreen image target, finished
/// and cleaned up, its sub-apps handed back for a loop to pump by hand.
/// Shared by the snapshot and the export.
struct Headless {
    subapps: SubApps,
    target: RenderTarget,
}

impl Headless {
    fn build(
        config: VizConfig,
        dmx: DmxUniverses,
        playback: Playback,
        gdtf: Option<GdtfLibrary>,
    ) -> Self {
        let (min, max) = config.venue.bounds();
        let view = config.view;
        let free_camera = config.camera;
        let quality = config.quality.for_rig(&config.venue, gdtf.as_ref());
        let (width, height) = (config.width, config.height);

        let assets_dir = config.assets_dir.clone();
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: assets_dir,
                    ..default()
                })
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
        .add_plugins(VizPlugin {
            config,
            dmx,
            gdtf: Mutex::new(gdtf),
            // A still or an export sends nothing: there is no rig to
            // drive from a frame rendered offline.
            output: Mutex::new(None),
        })
        .insert_resource(playback);

        let mut target = Image::new_uninit(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        target.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
        let target: RenderTarget = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(target)
            .into();

        let camera_target = target.clone();
        app.add_systems(Startup, move |mut commands: Commands| {
            // `RenderTarget` is its own component — adding it to the camera
            // is what points it at the offscreen image instead of a window.
            commands.spawn((
                camera_bundle(view, free_camera, min, max, quality),
                camera_target.clone(),
            ));
        });

        // No `run()`: the schedule runner is replaced by the caller's
        // loop, so the app has to be finished and cleaned up by hand.
        app.finish();
        app.cleanup();
        let subapps: SubApps = std::mem::take(app.sub_apps_mut());
        Self { subapps, target }
    }

    /// One frame, waited for on the GPU so a capture requested this
    /// frame is complete before the next one starts.
    fn step(&mut self) {
        self.subapps.update();
        self.subapps
            .main
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("polling the render device");
    }

    fn target_image(&self) -> Handle<Image> {
        self.target
            .as_image()
            .expect("offscreen target is an image")
            .clone()
    }
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
    let settle_frames = config.settle_frames.max(1);
    let mut headless = Headless::build(config, dmx, playback, gdtf);
    for _ in 0..settle_frames {
        headless.step();
    }
    let out = path.to_path_buf();
    let image = headless.target_image();
    headless
        .subapps
        .main
        .world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(out));
    // The capture is queued on the render world and the PNG encode runs
    // on a task pool thread, so the app has to keep stepping for a few
    // frames afterwards, and then outlive the encode itself — dropping
    // `subapps` takes the task pools down with it.
    for _ in 0..8 {
        headless.step();
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !path.exists() && std::time::Instant::now() < deadline {
        headless.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !path.exists() {
        eprintln!("viz: snapshot never reached {}", path.display());
    }
}

/// Renders the show to a video file, frame by frame against the song's
/// clock: Bevy's time is stepped by exactly one frame per update
/// (`TimeUpdateStrategy::ManualDuration`), so the playback advances the
/// same amount every frame however long the GPU took, and each frame is
/// captured and handed to the sink before the next is started. The
/// playback should already be located at the export's first bar
/// (`Playback::load` with `bar`) and carry the song's BPM.
///
/// Like `run`, this owns the thread until the export is finished.
// r[impl viz.export] - offline, frame by frame, at a chosen size
pub fn run_export(
    config: VizConfig,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    request: &ExportRequest,
) -> anyhow::Result<()> {
    let bpm = playback.speeds.get("Song").copied().unwrap_or(120.0) as f64;
    let schedule = FrameSchedule::new(request, bpm);
    let (width, height) = (config.width, config.height);
    let settle_frames = config.settle_frames.max(1);
    let mut sink = export::open_sink(request, width, height)?;
    let frame_count = schedule.frame_count();
    println!(
        "exporting bars {}..{} at {} fps: {} frames of {}x{}",
        schedule.from_bar, schedule.to_bar, schedule.fps, frame_count, width, height
    );

    let dmx = DmxUniverses::new();
    let mut headless = Headless::build(config, dmx, playback, gdtf);
    // Warm up with the clock stopped, so the first exported frame is
    // frame 0 of the song range, not settle_frames later.
    headless
        .subapps
        .main
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    for _ in 0..settle_frames {
        headless.step();
    }

    let (tx, rx) = std::sync::mpsc::channel::<Image>();
    let dt = Duration::from_secs_f64(schedule.dt());
    for frame in schedule.frames() {
        // Frame 0 renders the located moment; every later frame first
        // advances the clock by one dt.
        let advance = if frame.index == 0 { Duration::ZERO } else { dt };
        let image = headless.target_image();
        let world = headless.subapps.main.world_mut();
        world.insert_resource(TimeUpdateStrategy::ManualDuration(advance));
        let tx = tx.clone();
        world
            .spawn(Screenshot::image(image))
            .observe(move |captured: On<ScreenshotCaptured>| {
                let _ = tx.send(captured.image.clone());
            });
        // The capture completes on the render world a frame or two
        // later; keep stepping (with the clock held) until it lands so
        // frames reach the sink in order.
        let mut captured = None;
        for _ in 0..32 {
            headless.step();
            headless
                .subapps
                .main
                .world_mut()
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
            if let Ok(image) = rx.try_recv() {
                captured = Some(image);
                break;
            }
        }
        let Some(image) = captured else {
            anyhow::bail!("frame {} was never captured", frame.index);
        };
        let rgba = image
            .try_into_dynamic()
            .map_err(|e| anyhow::anyhow!("frame {}: {e:?}", frame.index))?
            .to_rgba8();
        sink.push(&frame, rgba.as_raw(), rgba.width(), rgba.height())?;
        if frame.index % schedule.fps == 0 {
            println!(
                "  frame {}/{} — bar {} beat {:.2}",
                frame.index, frame_count, frame.position.bar, frame.position.beat
            );
        }
    }
    sink.finish()?;
    println!("export finished: {}", request.path.display());
    Ok(())
}

/// The camera every mode uses: HDR so bloom has something above white to
/// work with, a tonemapper that desaturates as it clips, and the preset's
/// own framing.
/// Grows the clustering buffers past what this rig will ever need.
///
/// A Startup system rather than an inserted resource because the plugin
/// builds `GlobalClusterSettings` from the device's own capabilities —
/// whether storage buffers and GPU clustering are supported at all — and
/// replacing it wholesale would mean deciding those here, wrongly, on
/// hardware this code cannot see.
fn widen_light_clusters(mut settings: ResMut<bevy::light::cluster::GlobalClusterSettings>) {
    if let Some(gpu) = settings.gpu_clustering.as_mut() {
        gpu.initial_z_slice_list_capacity = gpu.initial_z_slice_list_capacity.max(4096);
        gpu.initial_index_list_capacity = gpu.initial_index_list_capacity.max(16384);
    }
}

pub(crate) fn camera_bundle(
    view: ViewPreset,
    free: Option<(Vec3, Vec3)>,
    min: Vec3,
    max: Vec3,
    quality: RenderQuality,
) -> impl Bundle {
    (
        Camera3d::default(),
        if quality.msaa {
            Msaa::default()
        } else {
            Msaa::Off
        },
        // Where shadow level-of-detail is measured from. Bevy warns when
        // point or spot lights exist and nothing declares one — which is
        // every frame here, since a lighting rig is nothing but point and
        // spot lights. Putting it on the camera is the intended answer:
        // detail should fall off with distance from the viewer.
        bevy::camera::ShadowLodOrigin,
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
        match free {
            Some((eye, target)) => ViewPreset::free_transform(eye, target),
            None => view.transform(min, max),
        },
        // The haze: marched on this camera at full size for a still, or
        // on a camera of its own at a fraction of the size for a live
        // view — `haze.rs` reads this and sets up whichever. The step
        // count is per pixel; a still goes well above the default 64
        // because a beam crossing a 20 m room at the default spacing
        // comes out in stair-steps, and a live view pays for fewer
        // pixels instead. Neither jitters: Bevy's jitter is meant for
        // use with temporal antialiasing, and without it all it did
        // was speckle every lit surface.
        crate::haze::HazeView {
            fog_steps: quality.fog_steps,
            scale: quality.fog_scale,
        },
    )
}

#[cfg(test)]
mod quality_tests {
    use super::*;

    /// The dots inside a narrow shaft are the raymarch crossing it a
    /// sample or two per pixel; the finer march is paid only by a rig
    /// that has such a shaft.
    #[test]
    fn narrow_shafts_raise_the_live_fog_steps_and_nothing_else_does() {
        let live = RenderQuality::live().fog_steps;
        assert_eq!(
            fog_steps_for(live, None),
            live,
            "a rig with no shaft keeps the live count"
        );
        assert_eq!(
            fog_steps_for(live, Some(25.0)),
            live,
            "a par rig keeps the live count"
        );
        assert_eq!(
            fog_steps_for(live, Some(11.0)),
            NARROW_BEAM_FOG_STEPS,
            "an 11-degree mover doubles it"
        );
        assert_eq!(
            fog_steps_for(RenderQuality::STILL.fog_steps, Some(1.72)),
            NARROW_BEAM_FOG_STEPS.max(RenderQuality::STILL.fog_steps),
            "a still is never made coarser"
        );
    }
}
