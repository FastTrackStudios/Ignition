//! A dark room, three trusses of moving heads and the beams in the air.
//!
//! ```text
//! cargo run --release --example truss                      # a window
//! cargo run --release --example truss -- --headless --out /tmp/beams
//! cargo run --release --example truss -- --bench --out /tmp/beams/bench.json
//! ```
//!
//! Options: `--beams N` (72), `--size WxH` (1920x1080), `--tier
//! low|medium|high|ultra` (high), `--half` (the air target at half
//! resolution), `--scene NAME` (one of the scenes in `rig.rs`; headless
//! renders them all without it), `--frames N` (leave a window after N
//! frames).
//!
//! Headless renders each scene's frames to PNG with no window and no
//! display. The bench measures the beam pass's GPU time per scene and
//! writes it, with the machine's load average, as JSON.

#![expect(
    clippy::arithmetic_side_effects,
    clippy::suboptimal_flops,
    reason = "arithmetic on floats and glam vectors: the first lint cannot tell them from integers, and a fused multiply-add would only obscure the arithmetic"
)]
#![expect(
    clippy::needless_pass_by_value,
    reason = "Bevy hands a system its parameters by value"
)]

mod rig;

use std::{fmt::Write as _, path::PathBuf, time::Duration};

use bevy::{
    app::{AppExit, ScheduleRunnerPlugin},
    camera::{Exposure, Hdr, RenderTarget},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    diagnostic::{DiagnosticPath, DiagnosticsStore},
    prelude::*,
    render::{
        RenderPlugin,
        diagnostic::RenderDiagnosticsPlugin,
        render_resource::{TextureFormat, TextureUsages},
        view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    },
    window::{ExitCondition, WindowResolution},
    winit::WinitPlugin,
};
use ignition_beams::{
    AirResolution, Beam, BeamCamera, BeamGobos, BeamGovernor, BeamTier, BeamsPlugin, GoboLayer,
    Governor, Quality, gobo_image, math::peak_candela,
};
use rig::{Fixture, Kind, Scene, look, rig, trusses};

/// What the example was asked to do.
#[derive(Resource, Clone, Debug, PartialEq)]
struct Options {
    mode: Mode,
    beams: u16,
    size: UVec2,
    quality: Quality,
    scene: Option<Scene>,
    out: PathBuf,
    frames: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Window,
    Headless,
    Bench,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Window,
            beams: 72,
            size: UVec2::new(1920, 1080),
            quality: Quality {
                resolution: AirResolution::Full,
                tier: BeamTier::High,
            },
            scene: None,
            out: PathBuf::from("target/tmp/beams"),
            frames: None,
        }
    }
}

/// `WxH`, both positive.
fn parse_size(text: &str) -> Option<UVec2> {
    let (width, height) = text.split_once('x')?;
    let size = UVec2::new(width.parse().ok()?, height.parse().ok()?);
    (size.x > 0 && size.y > 0).then_some(size)
}

/// The options the arguments ask for.
///
/// # Errors
/// When an argument is not one of the example's, or its value is missing
/// or cannot be read.
fn parse_options(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--headless" => options.mode = Mode::Headless,
            "--bench" => options.mode = Mode::Bench,
            "--half" => options.quality.resolution = AirResolution::Half,
            "--beams" => {
                options.beams = value()?.parse().map_err(|e| format!("--beams: {e}"))?;
            }
            "--frames" => {
                options.frames = Some(value()?.parse().map_err(|e| format!("--frames: {e}"))?);
            }
            "--size" => {
                let text = value()?;
                options.size =
                    parse_size(&text).ok_or_else(|| format!("--size: not WxH: {text}"))?;
            }
            "--tier" => {
                let name = value()?;
                options.quality.tier = BeamTier::named(&name)
                    .ok_or_else(|| format!("--tier: no tier called {name}"))?;
            }
            "--scene" => {
                let name = value()?;
                options.scene = Some(
                    Scene::named(&name)
                        .ok_or_else(|| format!("--scene: no scene called {name}"))?,
                );
            }
            "--out" => options.out = PathBuf::from(value()?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(options)
}

/// Makes the directory the headless modes write into: the captures' own,
/// or the one the bench's file goes in.
///
/// # Errors
/// When the directory cannot be made.
fn make_room_for_output(options: &Options) -> Result<(), String> {
    let directory = match options.mode {
        Mode::Window => return Ok(()),
        Mode::Headless => Some(options.out.as_path()),
        Mode::Bench => options.out.parent(),
    };
    let Some(directory) = directory.filter(|path| !path.as_os_str().is_empty()) else {
        return Ok(());
    };
    std::fs::create_dir_all(directory).map_err(|e| format!("{}: {e}", directory.display()))
}

/// One frame to capture: a scene at a moment.
#[derive(Clone, Debug, PartialEq)]
struct Shot {
    name: String,
    scene: Scene,
    t: f32,
}

/// The moment every scene is first captured at.
const SHOT_TIME: f32 = 2.0;

/// The frames to capture for a scene. The moving and strobing ones get
/// three consecutive frames at 60 a second: with no history in the
/// renderer each must be as clean as a still.
fn shots(scene: Scene) -> Vec<Shot> {
    let frames: u8 = match scene {
        Scene::SidePan | Scene::Strobe => 3,
        Scene::Gobos => 2,
        _ => 1,
    };
    (0..frames)
        .map(|frame| Shot {
            name: if frames > 1 {
                format!("{}-{frame}", scene.name())
            } else {
                scene.name().to_owned()
            },
            scene,
            t: SHOT_TIME + f32::from(frame) / 60.0,
        })
        .collect()
}

/// What the rig is doing now.
#[derive(Resource, Clone, Copy, Debug)]
struct Stage {
    scene: Scene,
    t: f32,
}

#[derive(Component)]
struct Rigged(Fixture);

/// The room's exposure: indoors, a little under Bevy's own indoor preset.
const EV100: f32 = 7.6;

fn main() -> Result<(), String> {
    let options = parse_options(std::env::args().skip(1))?;
    make_room_for_output(&options)?;
    let mut app = App::new();
    let plugins = DefaultPlugins.set(RenderPlugin {
        // A capture must not be taken while a pipeline is still compiling.
        synchronous_pipeline_compilation: options.mode != Mode::Window,
        ..default()
    });
    if options.mode == Mode::Window {
        app.add_plugins(plugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Ignition beams".into(),
                resolution: WindowResolution::new(options.size.x, options.size.y),
                ..default()
            }),
            ..default()
        }));
    } else {
        app.add_plugins(
            plugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                })
                .disable::<WinitPlugin>(),
        )
        .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::ZERO));
    }
    app.add_plugins((BeamsPlugin, RenderDiagnosticsPlugin))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(Stage {
            scene: options.scene.unwrap_or(Scene::Show),
            t: SHOT_TIME,
        })
        .add_systems(Startup, (build_room, build_rig, build_gobos));
    match options.mode {
        Mode::Window => {
            app.insert_resource(FrameLimit(options.frames))
                .add_systems(Startup, window_camera)
                .add_systems(Update, (run_clock, leave_after_frames, apply_stage).chain());
        }
        Mode::Headless => {
            let all = options
                .scene
                .map_or_else(|| Scene::ALL.to_vec(), |scene| vec![scene]);
            app.insert_resource(Capture::new(
                all.into_iter().flat_map(shots).collect(),
                options.out.clone(),
            ))
            .add_systems(Startup, offscreen_camera)
            .add_systems(Update, (drive_capture, apply_stage).chain());
        }
        Mode::Bench => {
            app.insert_resource(Bench::new(&options))
                .add_systems(Startup, offscreen_camera)
                .add_systems(Update, (drive_bench, apply_stage).chain());
        }
    }
    app.insert_resource(options);
    match app.run() {
        AppExit::Success => Ok(()),
        AppExit::Error(code) => Err(format!("exited with {code}")),
    }
}

// ── The room ──────────────────────────────────────────────────────────

/// A dark floor, a dark back wall and the three trusses.
fn build_room(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dark = |materials: &mut Assets<StandardMaterial>, value: f32| {
        materials.add(StandardMaterial {
            base_color: Color::linear_rgb(value, value, value),
            perceptual_roughness: 0.9,
            ..default()
        })
    };
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::new(16.0, 14.0)))),
        MeshMaterial3d(dark(&mut materials, 0.07)),
        Transform::from_xyz(0.0, 0.0, 2.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::new(Vec3::Z, Vec2::new(16.0, 5.0)))),
        MeshMaterial3d(dark(&mut materials, 0.09)),
        Transform::from_xyz(0.0, 5.0, rig::WALL_Z),
    ));
    let steel = dark(&mut materials, 0.02);
    for (from, to) in trusses() {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(from.distance(to), 0.3, 0.3))),
            MeshMaterial3d(steel.clone()),
            Transform::from_translation(from.midpoint(to)),
        ));
    }
}

/// Every mover: a beam in the air, a spot light for the pool it lands in,
/// and a dark head behind the lens.
fn build_rig(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    options: Res<Options>,
) {
    let head = meshes.add(Cuboid::new(0.3, 0.3, 0.4));
    let body = materials.add(StandardMaterial {
        base_color: Color::linear_rgb(0.015, 0.015, 0.015),
        perceptual_roughness: 0.8,
        ..default()
    });
    for fixture in rig(options.beams) {
        commands
            .spawn((
                Rigged(fixture),
                Beam::default(),
                SpotLight {
                    shadow_maps_enabled: false,
                    ..default()
                },
                Transform::from_translation(fixture.position),
                Visibility::default(),
            ))
            .with_child((
                Mesh3d(head.clone()),
                MeshMaterial3d(body.clone()),
                // Behind the lens: the beam shines along −Z.
                Transform::from_xyz(0.0, 0.0, 0.22),
            ));
    }
}

/// The gobo wheel: slot 0 open, then dots, bars and a pinwheel.
fn build_gobos(mut images: ResMut<Assets<Image>>, mut gobos: ResMut<BeamGobos>) {
    const SIZE: u16 = 256;
    let dots = |x: f32, y: f32| {
        (0..6_u8)
            .map(|i| f32::from(i) * core::f32::consts::FRAC_PI_3)
            .map(|angle| Vec2::new(angle.cos(), angle.sin()) * 0.58)
            .chain([Vec2::ZERO])
            .any(|centre| centre.distance(Vec2::new(x, y)) < 0.19)
    };
    let bars = |x: f32, y: f32| x.hypot(y) < 0.9 && (y * 3.0 + 0.25).rem_euclid(1.0) < 0.5;
    let pinwheel = |x: f32, y: f32| {
        let radius = x.hypot(y);
        let sector = (y.atan2(x) * 6.0 / core::f32::consts::TAU + radius * 0.8).rem_euclid(1.0);
        radius > 0.12 && radius < 0.92 && sector < 0.5
    };
    let layers = [
        GoboLayer::open(SIZE),
        GoboLayer::draw(SIZE, dots),
        GoboLayer::draw(SIZE, bars),
        GoboLayer::draw(SIZE, pinwheel),
    ];
    if let Some(image) = gobo_image(&layers) {
        gobos.0 = images.add(image);
    }
}

/// What every camera in the example carries.
fn camera_bundle(options: &Options, scene: Scene) -> impl Bundle {
    let (eye, target) = scene.camera();
    (
        Camera3d::default(),
        Hdr,
        Msaa::Off,
        Exposure { ev100: EV100 },
        Tonemapping::ReinhardLuminance,
        // The beams dither themselves before the tonemapper; this is the
        // tonemapper's own, for what it compresses near white.
        DebandDither::Enabled,
        Projection::Perspective(PerspectiveProjection {
            fov: 55.0_f32.to_radians(),
            ..default()
        }),
        Transform::from_translation(eye).looking_at(target, Vec3::Y),
        BeamCamera {
            quality: options.quality,
            ..default()
        },
    )
}

/// The window's camera, its quality governed by the frame time.
fn window_camera(mut commands: Commands, options: Res<Options>, stage: Res<Stage>) {
    commands.spawn((
        camera_bundle(&options, stage.scene),
        BeamGovernor(Governor::new(1000.0 / 60.0, options.quality)),
    ));
}

/// The image the headless modes render into.
#[derive(Resource)]
struct Offscreen(Handle<Image>);

fn offscreen_camera(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    options: Res<Options>,
    stage: Res<Stage>,
) {
    let mut image = Image::new_target_texture(
        options.size.x,
        options.size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let handle = images.add(image);
    commands.spawn((
        camera_bundle(&options, stage.scene),
        RenderTarget::Image(handle.clone().into()),
    ));
    commands.insert_resource(Offscreen(handle));
}

// ── The rig, each frame ───────────────────────────────────────────────

/// Points every fixture and the camera where the stage says.
fn apply_stage(
    stage: Res<Stage>,
    mut fixtures: Query<(&Rigged, &mut Transform, &mut Beam, &mut SpotLight)>,
    mut cameras: Query<&mut Transform, (With<Camera3d>, Without<Rigged>)>,
) {
    for (Rigged(fixture), mut transform, mut beam, mut light) in &mut fixtures {
        let now = look(stage.scene, fixture, stage.t);
        let optics = fixture.kind.optics();
        *transform = Transform::from_translation(fixture.position).looking_at(now.aim, Vec3::Z);
        *beam = Beam {
            beam_angle: (optics.beam_deg * now.zoom).to_radians(),
            field_angle: (optics.field_deg * now.zoom).to_radians(),
            lens_radius: optics.lens_radius,
            flux: optics.flux,
            colour: now.colour,
            dimmer: now.dimmer,
            gobo: now.gobo,
            range: now.range,
            // A beam fixture's shaft is its whole point: let it run hotter
            // before its own knee.
            knee: if fixture.kind == Kind::Beam { 4.0 } else { 3.0 },
            ..Beam::default()
        };
        let (profile, tan_edge) = beam.profile();
        *light = SpotLight {
            color: Color::LinearRgba(now.colour),
            // Bevy's spot light takes lumens as if it shone all round.
            intensity: peak_candela(optics.flux * now.dimmer, &profile, tan_edge)
                * 4.0
                * core::f32::consts::PI,
            range: beam.range,
            radius: optics.lens_radius,
            inner_angle: beam.beam_angle * 0.5,
            outer_angle: tan_edge.atan(),
            shadow_maps_enabled: false,
            ..default()
        };
    }
    let (eye, target) = stage.scene.camera();
    for mut transform in &mut cameras {
        *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    }
}

/// The window's clock: real time.
fn run_clock(time: Res<Time>, mut stage: ResMut<Stage>) {
    stage.t += time.delta_secs();
}

#[derive(Resource)]
struct FrameLimit(Option<u32>);

fn leave_after_frames(
    mut frames: Local<u32>,
    limit: Res<FrameLimit>,
    mut exit: MessageWriter<AppExit>,
) {
    *frames = frames.saturating_add(1);
    if limit.0.is_some_and(|limit| *frames >= limit) {
        exit.write(AppExit::Success);
    }
}

// ── Headless captures ─────────────────────────────────────────────────

/// Frames to wait before the first capture (shaders, the gobo texture) and
/// after each change of stage (the transforms reach the renderer).
const FIRST_SETTLE: u8 = 12;
const SETTLE: u8 = 4;

/// The captures still to take.
#[derive(Resource)]
struct Capture {
    shots: Vec<Shot>,
    out: PathBuf,
    next: usize,
    wait: u8,
    staged: bool,
    saved: usize,
}

impl Capture {
    const fn new(shots: Vec<Shot>, out: PathBuf) -> Self {
        Self {
            shots,
            out,
            next: 0,
            wait: FIRST_SETTLE,
            staged: false,
            saved: 0,
        }
    }

    /// The file a shot is written to.
    fn path_of(&self, shot: &Shot) -> PathBuf {
        self.out.join(format!("{}.png", shot.name))
    }
}

/// Stages each shot, lets it settle, captures it, and leaves when the last
/// file is written.
fn drive_capture(
    mut commands: Commands,
    mut capture: ResMut<Capture>,
    mut stage: ResMut<Stage>,
    offscreen: Res<Offscreen>,
    mut exit: MessageWriter<AppExit>,
) {
    if capture.wait > 0 {
        capture.wait -= 1;
        return;
    }
    let Some(shot) = capture.shots.get(capture.next).cloned() else {
        if capture.saved >= capture.shots.len() {
            let missing = capture
                .shots
                .iter()
                .filter(|shot| !capture.path_of(shot).exists())
                .count();
            if missing == 0 {
                info!(frames = capture.saved, out = %capture.out.display(), "beams: captures written");
                exit.write(AppExit::Success);
            } else {
                error!(missing, out = %capture.out.display(), "beams: captures not written");
                exit.write(AppExit::error());
            }
        }
        return;
    };
    if !capture.staged {
        *stage = Stage {
            scene: shot.scene,
            t: shot.t,
        };
        capture.staged = true;
        capture.wait = SETTLE;
        return;
    }
    let path = capture.path_of(&shot);
    commands
        .spawn(Screenshot::image(offscreen.0.clone()))
        .observe(save_to_disk(path))
        .observe(|_: On<ScreenshotCaptured>, mut capture: ResMut<Capture>| {
            capture.saved = capture.saved.saturating_add(1);
        });
    capture.staged = false;
    capture.next = capture.next.saturating_add(1);
    capture.wait = SETTLE;
}

// ── The bench ─────────────────────────────────────────────────────────

/// Frames run before measuring a scene, and frames measured.
const BENCH_WARM: u16 = 40;
const BENCH_FRAMES: u16 = 240;

/// One scene's measured GPU time for the beam pass, in milliseconds.
#[derive(Clone, Debug, PartialEq)]
struct Measured {
    scene: Scene,
    median_ms: f64,
    mean_ms: f64,
    max_ms: f64,
    load_average: String,
}

#[derive(Resource)]
struct Bench {
    scenes: Vec<Scene>,
    header: String,
    out: PathBuf,
    index: usize,
    frame: u16,
    samples: Vec<f64>,
    results: Vec<Measured>,
}

impl Bench {
    fn new(options: &Options) -> Self {
        let scenes = options.scene.map_or_else(
            || {
                vec![
                    Scene::Show,
                    Scene::Alias,
                    Scene::Overlap,
                    Scene::AtCamera,
                    Scene::InsideWash,
                ]
            },
            |scene| vec![scene],
        );
        Self {
            scenes,
            header: format!(
                "\"width\": {}, \"height\": {}, \"beams\": {}, \"tier\": \"{}\", \"air\": \"{}\"",
                options.size.x,
                options.size.y,
                options.beams,
                options.quality.tier.name(),
                match options.quality.resolution {
                    AirResolution::Full => "full",
                    AirResolution::Half => "half",
                },
            ),
            out: options.out.clone(),
            index: 0,
            frame: 0,
            samples: Vec::new(),
            results: Vec::new(),
        }
    }
}

/// The median, the mean and the largest of `samples`; `None` when there
/// are none. The median is the one to quote: the GPU is shared, and another
/// program's frame landing inside the pass lifts the mean and the largest.
fn summarise(samples: &[f64]) -> Option<(f64, f64, f64)> {
    let count = u32::try_from(samples.len())
        .ok()
        .filter(|&count| count > 0)?;
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted.get(sorted.len() / 2).copied()?;
    let mean = sorted.iter().sum::<f64>() / f64::from(count);
    Some((median, mean, sorted.last().copied()?))
}

/// The bench's results as JSON.
fn bench_json(header: &str, results: &[Measured]) -> String {
    let mut json = format!("{{{header}, \"scenes\": [");
    for (i, result) in results.iter().enumerate() {
        let comma = if i == 0 { "" } else { "," };
        // Writing to a `String` cannot fail.
        let _ = write!(
            json,
            "{comma}\n  {{\"scene\": \"{}\", \"gpu_ms_median\": {:.4}, \"gpu_ms_mean\": {:.4}, \"gpu_ms_max\": {:.4}, \"loadavg\": \"{}\"}}",
            result.scene.name(),
            result.median_ms,
            result.mean_ms,
            result.max_ms,
            result.load_average,
        );
    }
    json.push_str("\n]}\n");
    json
}

/// Runs each scene with its clock moving, reads the beam pass's GPU time
/// from Bevy's render diagnostics, and writes the lot when done.
fn drive_bench(
    mut bench: ResMut<Bench>,
    mut stage: ResMut<Stage>,
    diagnostics: Res<DiagnosticsStore>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(scene) = bench.scenes.get(bench.index).copied() else {
        let json = bench_json(&bench.header, &bench.results);
        match std::fs::write(&bench.out, json) {
            Ok(()) => {
                info!(out = %bench.out.display(), scenes = bench.results.len(), "beams: bench written");
                exit.write(AppExit::Success);
            }
            Err(error) => {
                error!(out = %bench.out.display(), %error, "beams: bench not written");
                exit.write(AppExit::error());
            }
        }
        return;
    };
    *stage = Stage {
        scene,
        t: SHOT_TIME + f32::from(bench.frame) / 60.0,
    };
    bench.frame = bench.frame.saturating_add(1);
    if bench.frame <= BENCH_WARM {
        return;
    }
    let gpu_time = DiagnosticPath::new("render/ignition_beams/elapsed_gpu");
    if let Some(ms) = diagnostics
        .get(&gpu_time)
        .and_then(bevy::diagnostic::Diagnostic::value)
    {
        bench.samples.push(ms);
    }
    if bench.frame < BENCH_WARM.saturating_add(BENCH_FRAMES) {
        return;
    }
    let (median_ms, mean_ms, max_ms) =
        summarise(&bench.samples).unwrap_or((f64::NAN, f64::NAN, f64::NAN));
    let load_average = std::fs::read_to_string("/proc/loadavg")
        .map(|text| {
            text.split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    bench.results.push(Measured {
        scene,
        median_ms,
        mean_ms,
        max_ms,
        load_average,
    });
    bench.samples.clear();
    bench.frame = 0;
    bench.index = bench.index.saturating_add(1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(text: &str) -> impl Iterator<Item = String> {
        text.split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>()
            .into_iter()
    }

    #[test]
    fn no_arguments_is_a_window_on_the_show() {
        assert_eq!(parse_options(args("")), Ok(Options::default()));
    }

    #[test]
    fn arguments_set_what_they_name() {
        let options = parse_options(args(
            "--bench --beams 200 --size 5120x1440 --tier ultra --half --scene alias --out x.json",
        ))
        .unwrap();
        assert_eq!(options.mode, Mode::Bench);
        assert_eq!(options.beams, 200);
        assert_eq!(options.size, UVec2::new(5120, 1440));
        assert_eq!(options.quality.tier, BeamTier::Ultra);
        assert_eq!(options.quality.resolution, AirResolution::Half);
        assert_eq!(options.scene, Some(Scene::Alias));
        assert_eq!(options.out, PathBuf::from("x.json"));
    }

    #[test]
    fn a_bad_argument_says_which() {
        assert!(
            parse_options(args("--tier froxels"))
                .unwrap_err()
                .contains("froxels")
        );
        assert!(
            parse_options(args("--size big"))
                .unwrap_err()
                .contains("big")
        );
        assert!(
            parse_options(args("--beams"))
                .unwrap_err()
                .contains("--beams")
        );
        assert!(
            parse_options(args("--what"))
                .unwrap_err()
                .contains("--what")
        );
    }

    #[test]
    fn moving_scenes_get_consecutive_frames() {
        let pan = shots(Scene::SidePan);
        assert_eq!(pan.len(), 3);
        assert!((pan[1].t - pan[0].t - 1.0 / 60.0).abs() < 1.0e-6);
        assert_eq!(shots(Scene::Overlap).len(), 1);
        assert_eq!(shots(Scene::Overlap)[0].name, "overlap");
    }

    #[test]
    fn a_summary_is_the_median_the_mean_and_the_largest() {
        assert_eq!(summarise(&[]), None);
        assert_eq!(
            summarise(&[3.0, 1.0, 2.0, 10.0, 4.0]),
            Some((3.0, 4.0, 10.0))
        );
    }

    #[test]
    fn the_bench_writes_json_a_reader_can_parse() {
        let json = bench_json(
            "\"beams\": 70",
            &[Measured {
                scene: Scene::Show,
                median_ms: 1.0,
                mean_ms: 1.25,
                max_ms: 2.5,
                load_average: "5.1 5.0 4.9".into(),
            }],
        );
        assert!(
            json.contains(
                "\"scene\": \"show\", \"gpu_ms_median\": 1.0000, \"gpu_ms_mean\": 1.2500"
            )
        );
        assert!(json.starts_with("{\"beams\": 70, \"scenes\": ["));
        assert!(json.trim_end().ends_with("]}"));
    }
}
