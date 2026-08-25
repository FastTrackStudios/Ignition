//! Live windowed visualizer — the actual "emulated rig" this project's
//! DMX work exists for: opens a real window, redraws continuously, and
//! every frame reads whatever sACN/Art-Net packets have arrived
//! (`ignition_viz::dmx`) to move/colour the fixtures. `fixtures.json`
//! (`venue::Venue`) still supplies every fixture's fixed mount pose exactly
//! as `shot` uses it — this binary only adds a live layer on top, it does
//! not replace the static venue data. Camera is fixed at the house view
//! for now; there's no fly-around yet (see `docs/research/lighting-
//! console-landscape.md` for what's phased for later).
//!
//! Run from the repo root: `cargo run -p ignition-viz --bin live -- --venue data/venues/norco`
//!
//! No sACN/Art-Net source on the network yet? Every fixture just renders at
//! its static default (dimmer 1, no live colour/pan/tilt) — this binary
//! doesn't require a console to be running, it only *responds* to one when
//! present.
//!
//! `--snapshot <path>` renders one frame headlessly (via
//! `LiveHeadlessRenderer` — the same point-light + beam-cone pipeline as
//! the real window, no window/display required) and exits, instead of
//! opening a window. This is what proves the live DMX/lighting work
//! actually looks like in an environment with no display attached — the
//! same role `shot` plays for the static venue model, just with live data
//! behind it. Waits `--warm-up-ms` (default 300) after starting the DMX
//! listeners before capturing, so a source that's already sending has time
//! to be received.

use ignition_viz::live_renderer::{DEFAULT_AMBIENT, DEFAULT_HAZE};
use ignition_viz::{build_scene, dmx, Camera, DmxUniverses, LiveHeadlessRenderer, LiveRenderer, Venue};
use std::path::PathBuf;
use std::sync::Arc;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

struct App {
    venue: Venue,
    dmx: DmxUniverses,
    window: Option<Arc<Window>>,
    renderer: Option<LiveRenderer>,
    ambient: f32,
    haze: f32,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Ignition — live rig")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 1000.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("failed to create window"));
        let renderer =
            LiveRenderer::new(window.clone(), self.ambient, self.haze).expect("failed to init live renderer");
        window.request_redraw();
        self.window = Some(window);
        self.renderer = Some(renderer);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(renderer) = &self.renderer else { return };
                let mesh = build_scene(&self.venue, &[], false, Some(&self.dmx));
                let (min, max) = self.venue.bounds();
                let camera = Camera::frame_house_view(min, max, renderer.aspect());
                if let Err(e) = renderer.render(&mesh, &camera) {
                    eprintln!("ignition-viz (live): render error: {e}");
                }
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() -> anyhow::Result<()> {
    let mut venue_dir = PathBuf::from("data/venues/norco");
    let mut max_universe = 4u16;
    let mut snapshot: Option<PathBuf> = None;
    let mut warm_up_ms = 300u64;
    let mut view = "house".to_string();
    let mut width = 1600u32;
    let mut height = 1000u32;
    let mut ambient = DEFAULT_AMBIENT;
    let mut haze = DEFAULT_HAZE;
    let mut snapshot_time: Option<f32> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--venue" => venue_dir = PathBuf::from(args.next().expect("--venue needs a path")),
            "--max-universe" => {
                max_universe = args.next().expect("--max-universe needs a number").parse().unwrap()
            }
            "--snapshot" => snapshot = Some(PathBuf::from(args.next().expect("--snapshot needs a path"))),
            "--warm-up-ms" => {
                warm_up_ms = args.next().expect("--warm-up-ms needs a number").parse().unwrap()
            }
            "--view" => view = args.next().expect("--view needs house|stage|top"),
            "--width" => width = args.next().expect("--width needs a number").parse().unwrap(),
            "--height" => height = args.next().expect("--height needs a number").parse().unwrap(),
            // Visualizer settings, same idea as QLC+/grandMA3's 3D-view
            // ambient/haze sliders — 0 ambient + real haze is the default
            // (see live_renderer::{DEFAULT_AMBIENT, DEFAULT_HAZE}) so the
            // room only shows what its fixtures actually light, the way
            // the real stage looks in the dark.
            "--ambient" => ambient = args.next().expect("--ambient needs a number 0..1").parse().unwrap(),
            "--haze" => haze = args.next().expect("--haze needs a number, e.g. 1.6").parse().unwrap(),
            // Phase of the beam haze's drifting turbulence to render, for
            // --snapshot (a single frame has no "next frame" to animate
            // through, so this is the only way to see the effect at a
            // moment other than t=0). Defaults to the real wall-clock time
            // of day so repeated snapshots aren't all identical.
            "--time" => snapshot_time = Some(args.next().expect("--time needs seconds, e.g. 12.5").parse().unwrap()),
            other => eprintln!("ignition-live: ignoring unknown argument {other}"),
        }
    }

    let venue = Venue::load(&venue_dir)?;
    println!(
        "loaded venue {:?}: {} fixtures, {} room objects",
        venue_dir,
        venue.fixtures.len(),
        venue.room.len()
    );

    let dmx = DmxUniverses::new();
    dmx::spawn_sacn_listener(dmx.clone(), max_universe);
    dmx::spawn_artnet_listener(dmx.clone());

    if let Some(out_path) = snapshot {
        std::thread::sleep(std::time::Duration::from_millis(warm_up_ms));
        let mesh = build_scene(&venue, &[], false, Some(&dmx));
        println!(
            "scene: {} vertices, {} indices, {} glow vertices, {} lights",
            mesh.vertices.len(),
            mesh.indices.len(),
            mesh.glow_vertices.len(),
            mesh.lights.len()
        );
        let (min, max) = venue.bounds();
        let aspect = width as f32 / height as f32;
        let camera = match view.as_str() {
            "stage" => Camera::frame_stage_view(min, max, aspect),
            "top" => Camera::frame_top_view(min, max, aspect),
            "house" => Camera::frame_house_view(min, max, aspect),
            other => anyhow::bail!("unknown --view {other}; use house, stage, or top"),
        };
        let renderer = LiveHeadlessRenderer::new(ambient, haze)?;
        let time_secs = snapshot_time.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| (d.as_secs_f32()) % 3600.0)
                .unwrap_or(0.0)
        });
        renderer.render_to_png(&mesh, &camera, width, height, time_secs, &out_path)?;
        println!("wrote {} (haze phase t={:.1}s)", out_path.display(), time_secs);
        return Ok(());
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { venue, dmx, window: None, renderer: None, ambient, haze };
    event_loop.run_app(&mut app)?;
    Ok(())
}
