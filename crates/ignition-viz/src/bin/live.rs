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

use ignition_core::{CueList, CuePlayer};
use ignition_viz::live_renderer::{DEFAULT_AMBIENT, DEFAULT_HAZE};
use ignition_viz::show::tick_and_apply;
use ignition_viz::{build_scene, dmx, Camera, DmxUniverses, LiveHeadlessRenderer, LiveRenderer, Venue};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

struct App {
    venue: Venue,
    dmx: DmxUniverses,
    window: Option<Arc<Window>>,
    renderer: Option<LiveRenderer>,
    ambient: f32,
    haze: f32,
    /// A loaded cue list (`--cuelist`) being programmed/played back — press
    /// Space to GO to the next cue, same convention as a real console. Ticks
    /// forward every redraw via `tick_and_apply` so fades render smoothly;
    /// `None` when no `--cuelist` was passed, in which case this binary
    /// behaves exactly as before this feature existed (purely a passive
    /// sACN/Art-Net receiver).
    cue_player: Option<CuePlayer>,
    last_tick: Instant,
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
        self.last_tick = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                }
            }
            // Space = GO, matching a real lighting console's convention —
            // advances the loaded cue list one step, fading into it over
            // that cue's own `fade_secs` rather than snapping.
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state == ElementState::Pressed && !event.repeat && event.logical_key == Key::Named(NamedKey::Space) {
                    if let Some(player) = &mut self.cue_player {
                        player.go();
                        println!(
                            "GO -> cue {} {:?}",
                            player.current_index().map(|i| i + 1).unwrap_or(0),
                            player.current_name()
                        );
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last_tick).as_secs_f32();
                self.last_tick = now;
                if let Some(player) = &mut self.cue_player {
                    tick_and_apply(&self.dmx, &self.venue, player, dt);
                }

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

fn load_cue_list(path: &PathBuf) -> anyhow::Result<CueList> {
    let raw = std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("parsing {} as a cue list: {e}", path.display()))
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
    let mut cuelist_path: Option<PathBuf> = None;
    let mut cue_index: Option<usize> = None;
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
            // A programmed show: `--cuelist` loads a JSON `CueList`
            // (`ignition_core::cue`) — in the windowed mode, press Space to
            // GO through it live; with `--snapshot`, pass `--cue N` to jump
            // straight to the end of cue N's fade and capture that moment
            // headlessly (no keyboard/window needed to test a show).
            "--cuelist" => cuelist_path = Some(PathBuf::from(args.next().expect("--cuelist needs a path"))),
            "--cue" => cue_index = Some(args.next().expect("--cue needs a 0-based cue index").parse().unwrap()),
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

    let mut cue_player = match &cuelist_path {
        Some(path) => {
            let list = load_cue_list(path)?;
            println!("loaded cue list {:?}: {} cues", list.name, list.cues.len());
            Some(CuePlayer::new(list.cues))
        }
        None => None,
    };

    if let Some(out_path) = snapshot {
        if let Some(player) = &mut cue_player {
            let index = cue_index.unwrap_or(0);
            player.jump_to_end_of(index);
            println!("cue -> {} {:?}", index + 1, player.current_name());
            ignition_viz::show::apply_cue_output(&dmx, &venue, &player.output());
        }
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

    if cue_player.is_some() {
        println!("cue list loaded — press Space in the window to GO to the next cue");
    }

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app =
        App { venue, dmx, window: None, renderer: None, ambient, haze, cue_player, last_tick: Instant::now() };
    event_loop.run_app(&mut app)?;
    Ok(())
}
