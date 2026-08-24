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

use ignition_viz::{build_scene, dmx, Camera, DmxUniverses, LiveRenderer, Venue};
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
        let renderer = LiveRenderer::new(window.clone()).expect("failed to init live renderer");
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
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--venue" => venue_dir = PathBuf::from(args.next().expect("--venue needs a path")),
            "--max-universe" => {
                max_universe = args.next().expect("--max-universe needs a number").parse().unwrap()
            }
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

    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App { venue, dmx, window: None, renderer: None };
    event_loop.run_app(&mut app)?;
    Ok(())
}
