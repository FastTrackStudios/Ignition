//! `viz --bench <frames>`: the studio's render route, timed.
//!
//! The studio does not run `App::run`. It hands Bevy a wgpu device that
//! Blitz made, steps the app once per paint, and composites the target
//! texture — see `embedded.rs`. A benchmark of the windowed `App` would
//! measure a different pipeline (its own device, pipelined rendering,
//! a swapchain), so this one builds the same `EmbeddedViz` on a device
//! made the way Blitz makes one — the same features, WebGPU's default
//! limits — and pumps it by hand. Every frame is waited for on the GPU,
//! so a frame's time is the whole frame and not just the CPU's half of
//! it: the studio has no pipelining either, and its present blocks on
//! exactly this work.
//!
//! What it prints is what a change to the renderer is judged against:
//! the `r[viz.performance-budget]` numbers.

use crate::app::VizConfig;
use crate::dmx::DmxUniverses;
use crate::embedded::{EmbeddedViz, HostGpu};
use crate::gdtf_geometry::GdtfLibrary;
use crate::playback::Playback;
use crate::spawn::{BeamEmitter, FixtureSpill};
use bevy::diagnostic::{DiagnosticsStore, EntityCountDiagnosticsPlugin};
use bevy::light::{VolumetricFog, VolumetricLight};
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use std::time::{Duration, Instant};
use wgpu::PollType;

/// What one benchmark run measured.
#[derive(Debug, Clone)]
pub struct BenchReport {
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    /// Wall time per frame, CPU step plus a wait for the GPU.
    pub avg_ms: f64,
    pub p50_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    /// Time spent in `EmbeddedViz::render` alone — the main-world
    /// update, extraction and command encoding, before the GPU is
    /// waited for.
    pub cpu_avg_ms: f64,
    pub fps: f64,
    pub counts: SceneCounts,
    /// Every Bevy diagnostic that had a value, `(path, average)`.
    pub diagnostics: Vec<(String, f64)>,
}

impl BenchReport {
    /// How long the GPU kept working after the CPU step returned —
    /// what a present would have waited for.
    #[must_use]
    pub fn gpu_tail_ms(&self) -> f64 {
        (self.avg_ms - self.cpu_avg_ms).max(0.0)
    }
}

/// How much of everything the scene carried, read from the world after
/// the run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SceneCounts {
    pub emitters: usize,
    pub spill_lights: usize,
    pub spot_lights_visible: usize,
    pub shadowed_spots: usize,
    pub volumetric_lights: usize,
    pub mesh_entities: usize,
    pub mesh_assets: usize,
    pub standard_materials: usize,
    pub fog_steps: u32,
    pub gpu_timestamps: bool,
    /// Render pipelines in Bevy's cache at the end of the run — a count
    /// that keeps climbing between runs of different length is a
    /// pipeline being compiled per frame, and with synchronous
    /// compilation each one is a stall.
    pub pipelines: usize,
}

impl BenchReport {
    /// The report as `--bench` prints it.
    #[must_use]
    pub fn render(&self) -> String {
        // `write!` into the `String` rather than `push_str(&format!(...))`
        // — same output, one allocation fewer per line. `String`'s
        // `Write` impl never actually fails, so the `Result` is dropped
        // rather than propagated.
        use std::fmt::Write as _;
        let mut out = String::new();
        let c = &self.counts;
        let _ = writeln!(
            out,
            "bench {}x{} over {} frames: avg {:.2} ms  p50 {:.2}  p99 {:.2}  max {:.2}  => {:.1} fps (cpu step {:.2} ms, gpu after it {:.2} ms)",
            self.width,
            self.height,
            self.frames,
            self.avg_ms,
            self.p50_ms,
            self.p99_ms,
            self.max_ms,
            self.fps,
            self.cpu_avg_ms,
            self.gpu_tail_ms()
        );
        // The studio presents on vsync, so what it needs is each half
        // of the frame under the refresh interval on its own: the CPU
        // step, which is what blocks the next paint, and the GPU work,
        // which is what the present waits for.
        let budget = 1000.0 / 120.0;
        let verdict = if self.cpu_avg_ms <= budget && self.gpu_tail_ms() <= budget {
            "holds 120 fps"
        } else if self.cpu_avg_ms > budget {
            "CPU-bound below 120 fps"
        } else {
            "GPU-bound below 120 fps"
        };
        let _ = writeln!(
            out,
            "verdict: {verdict} ({budget:.2} ms per frame at 120 Hz; pipelines {})",
            self.counts.pipelines
        );
        let _ = writeln!(
            out,
            "scene: emitters {}  spill lights {} ({} lit)  shadowed spots {}  volumetric lights {}  mesh entities {}  mesh assets {}  standard materials {}  fog steps {}  gpu timestamps {}",
            c.emitters,
            c.spill_lights,
            c.spot_lights_visible,
            c.shadowed_spots,
            c.volumetric_lights,
            c.mesh_entities,
            c.mesh_assets,
            c.standard_materials,
            c.fog_steps,
            if c.gpu_timestamps { "yes" } else { "no" }
        );
        for (path, value) in &self.diagnostics {
            let _ = writeln!(out, "  {path}: {value:.3}");
        }
        out
    }
}

/// The features the studio asks Blitz for on Bevy's behalf (see the studio's
/// `main.rs`): what Vello itself wants, plus what bloom's `Rg11b10Ufloat`
/// buffer and Bevy's format probing need.
///
/// Without the first of those every frame's bloom texture fails validation
/// and the frame is thrown away — quietly, at four hundred "frames" a
/// second.
pub const STUDIO_FEATURES: wgpu::Features = wgpu::Features::CLEAR_TEXTURE
    .union(wgpu::Features::PIPELINE_CACHE)
    .union(wgpu::Features::RG11B10UFLOAT_RENDERABLE)
    .union(wgpu::Features::FLOAT32_FILTERABLE)
    .union(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES);

/// WebGPU's default limits, which is what Vello asks for and so what the
/// studio's device has.
///
/// Bevy reads them as "GPU preprocessing only": transforms are built on
/// the GPU, culling and draw encoding stay on the CPU.
///
/// Raising the storage limits and adding `INDIRECT_FIRST_INSTANCE |
/// IMMEDIATES` unlocks Bevy's full GPU culling — and was measured *slower*
/// here, 7.7 ms to 13.8 ms of CPU a frame on the benchmark, so the studio
/// keeps the defaults.
#[must_use]
pub fn studio_limits() -> wgpu::Limits {
    wgpu::Limits::default()
}

/// A device made the way Blitz makes the one the studio hands to Bevy:
///
/// default WebGPU limits and `STUDIO_FEATURES` — plus `TIMESTAMP_QUERY`
/// when the adapter has it, so Bevy's render diagnostics can put a GPU
/// time on each pass. That one extra feature changes nothing about how
/// the frame is drawn.
///
/// # Errors
///
/// If no adapter or device matching the studio's own feature/limit
/// requirements is available on this machine.
pub fn headless_gpu() -> anyhow::Result<HostGpu> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    #[allow(unused_mut, reason = "the solari feature adds to it")]
    let mut wanted = STUDIO_FEATURES
        | wgpu::Features::TIMESTAMP_QUERY
        | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    #[cfg(feature = "solari")]
    {
        wanted |= crate::solari::required_features();
    }
    #[allow(unused_mut, reason = "the solari feature adds to it")]
    let mut limits = studio_limits();
    #[cfg(feature = "solari")]
    {
        limits = crate::solari::widen_limits(limits, &adapter.limits());
    }
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("ignition bench (blitz-shaped)"),
        required_features: adapter.features() & wanted,
        required_limits: limits,
        // Ray queries are behind wgpu's experimental gate; the token is
        // `unsafe` because wgpu does not promise validation for them.
        #[cfg(feature = "solari")]
        experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
        ..Default::default()
    }))?;
    let info = adapter.get_info();
    println!(
        "bench: {} ({:?}, {:?}) features {:?}",
        info.name,
        info.backend,
        info.device_type,
        device.features() & wanted
    );
    Ok(HostGpu {
        instance,
        adapter,
        device,
        queue,
    })
}

/// Sets up the subscriber for a bench run.
///
/// The embedded path disables Bevy's `LogPlugin` because a host owns the
/// subscriber; here there is no host, so a render error would otherwise
/// vanish. `RUST_LOG` filters it, warnings by default. `IGNITION_TRACE_SPANS
/// =1` with a `bevy/trace` build prints every span as it closes, with its
/// busy time — a poor man's profiler that needs no extra crate: aggregate
/// the lines by span name.
fn init_bench_tracing() {
    let span_events = if std::env::var("IGNITION_TRACE_SPANS").is_ok_and(|v| v == "1") {
        tracing_subscriber::fmt::format::FmtSpan::CLOSE
    } else {
        tracing_subscriber::fmt::format::FmtSpan::NONE
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_span_events(span_events)
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .try_init();
}

/// Renders `frames` frames through the studio's embedded route and
/// reports how long each took. `warmup` frames run first and are not
/// counted — pipelines compile and assets upload on the first few.
///
/// # Panics
///
/// Never in practice: the only panic in this path is the GPU-poll wait,
/// which only fires if the device itself is lost (see `EmbeddedViz::step`).
///
/// # Errors
///
/// If the snapshot at `snapshot` (when given) fails to encode or write,
/// or `--snapshot`'s own GPU-device setup fails.
pub fn run_bench(
    config: VizConfig,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    frames: u32,
    warmup: u32,
    snapshot: Option<&std::path::Path>,
) -> anyhow::Result<BenchReport> {
    let (width, height) = (config.width, config.height);
    init_bench_tracing();
    let gpu = headless_gpu()?;
    let device = gpu.device.clone();
    let gpu_timestamps = device.features().contains(wgpu::Features::TIMESTAMP_QUERY);
    let mut viz = EmbeddedViz::new_with(
        config,
        DmxUniverses::default(),
        playback,
        gdtf,
        gpu,
        |app| {
            app.add_plugins((
                RenderDiagnosticsPlugin,
                EntityCountDiagnosticsPlugin::default(),
            ));
        },
    );
    // As in `EmbeddedViz::step`: a `PollError` means the GPU device
    // itself is gone, not a data problem the bench could recover from.
    #[expect(
        clippy::expect_used,
        reason = "a lost GPU device has no fallback; see EmbeddedViz::step"
    )]
    let wait = || {
        device
            .poll(PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })
            .expect("polling the bench device");
    };
    for _ in 0..warmup {
        viz.render(width, height);
        wait();
    }
    let mut total = Vec::with_capacity(crate::num::usize_of_u32(frames));
    let mut cpu = Vec::with_capacity(crate::num::usize_of_u32(frames));
    for _ in 0..frames {
        let start = Instant::now();
        viz.render(width, height);
        cpu.push(start.elapsed().as_secs_f64() * 1e3);
        wait();
        total.push(start.elapsed().as_secs_f64() * 1e3);
    }
    // `IGNITION_BENCH_DUMP=1`: every frame's time in order, for seeing
    // whether the tail is a rhythm (something periodic in the frame) or
    // a scatter (the driver, the OS).
    if std::env::var("IGNITION_BENCH_DUMP").is_ok_and(|v| v == "1") {
        for (i, (t, c)) in total.iter().zip(&cpu).enumerate() {
            println!("frame {i}: {t:.2} ms (cpu {c:.2})");
        }
    }
    let mut sorted = total.clone();
    sorted.sort_by(f64::total_cmp);
    // `saturating_sub(1)` rather than `- 1`: an empty `sorted` (zero
    // frames requested) would otherwise underflow before the percentile
    // is ever read. `.get()` over `[]` for the same reason — the index
    // is a rounded float, not a loop counter over this same slice, so it
    // is data as far as `docs/ops/clippy.md`'s indexing rule is
    // concerned. Falling back to 0.0 only fires on that empty-run case;
    // every real bench has at least one frame and hits neither fallback.
    let pct = |p: f64| {
        let last = sorted.len().saturating_sub(1);
        let idx = crate::num::usize_of_f64((crate::num::f64_of_usize(last) * p).round());
        sorted.get(idx).copied().unwrap_or(0.0)
    };
    let avg_ms = total.iter().sum::<f64>() / crate::num::f64_of_usize(total.len().max(1));
    let cpu_avg_ms = cpu.iter().sum::<f64>() / crate::num::f64_of_usize(cpu.len().max(1));

    if let Some(path) = snapshot {
        write_snapshot(&mut viz, width, height, &wait, path);
    }

    let mut counts = scene_counts(viz.app_mut().world_mut());
    counts.gpu_timestamps = gpu_timestamps;
    counts.pipelines = viz.pipeline_count();
    let diagnostics = {
        let world = viz.app_mut().world();
        let store = world.resource::<DiagnosticsStore>();
        let mut rows: Vec<(String, f64)> = store
            .iter()
            .filter_map(|d| d.average().map(|v| (d.path().as_str().to_string(), v)))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    };
    Ok(BenchReport {
        width,
        height,
        frames,
        avg_ms,
        p50_ms: pct(0.5),
        p99_ms: pct(0.99),
        max_ms: sorted.last().copied().unwrap_or(0.0),
        cpu_avg_ms,
        fps: if avg_ms > 0.0 { 1e3 / avg_ms } else { 0.0 },
        counts,
        diagnostics,
    })
}

/// Writes the last rendered frame to `path`, the same target and quality
/// the studio would have shown, so `--snapshot` gives something to look
/// at over what the numbers bought. `wait` is the caller's GPU-poll
/// closure, run between render steps while the async write catches up.
fn write_snapshot(
    viz: &mut EmbeddedViz,
    width: u32,
    height: u32,
    wait: &impl Fn(),
    path: &std::path::Path,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let image = viz.target();
    // Owned: the observer outlives this call.
    let out: std::path::PathBuf = path.into();
    viz.app_mut()
        .world_mut()
        .spawn(Screenshot::image(image))
        .observe(save_to_disk(out));
    // `checked_add` rather than `+`: the deadline is ten seconds out and
    // never actually near `Instant`'s ceiling, but the fallback keeps
    // the wait from panicking instead of just timing out if it ever
    // were.
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(10))
        .unwrap_or_else(Instant::now);
    while !path.exists() && Instant::now() < deadline {
        viz.render(width, height);
        wait();
        std::thread::sleep(Duration::from_millis(10));
    }
    if !path.exists() {
        eprintln!("bench: snapshot never reached {}", path.display());
    }
}

/// Counts what is in the scene right now.
pub fn scene_counts(world: &mut World) -> SceneCounts {
    let emitters = world.query::<&BeamEmitter>().iter(world).count();
    let spill_lights = world
        .query_filtered::<(), With<FixtureSpill>>()
        .iter(world)
        .count();
    let mut spots = world.query::<(&SpotLight, &ViewVisibility, Option<&VolumetricLight>)>();
    let mut spot_lights_visible: usize = 0;
    let mut shadowed_spots: usize = 0;
    let mut volumetric_lights: usize = 0;
    for (light, visible, volumetric) in spots.iter(world) {
        if !visible.get() {
            continue;
        }
        spot_lights_visible = spot_lights_visible.saturating_add(1);
        if light.shadow_maps_enabled {
            shadowed_spots = shadowed_spots.saturating_add(1);
        }
        if volumetric.is_some() {
            volumetric_lights = volumetric_lights.saturating_add(1);
        }
    }
    let mesh_entities = world
        .query_filtered::<(), (With<Mesh3d>, With<ViewVisibility>)>()
        .iter(world)
        .count();
    let mesh_assets = world.resource::<Assets<Mesh>>().len();
    let standard_materials = world.resource::<Assets<StandardMaterial>>().len();
    let fog_steps = world
        .query::<&VolumetricFog>()
        .iter(world)
        .map(|f| f.step_count)
        .max()
        .unwrap_or(0);
    SceneCounts {
        emitters,
        spill_lights,
        spot_lights_visible,
        shadowed_spots,
        volumetric_lights,
        mesh_entities,
        mesh_assets,
        standard_materials,
        fog_steps,
        gpu_timestamps: false,
        pipelines: 0,
    }
}
