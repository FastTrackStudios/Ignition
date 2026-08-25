//! wgpu 3D visualizer, in two modes: `src/bin/shot.rs` (headless
//! render-to-PNG, no display, no live data — regression screenshots) and
//! `src/bin/live.rs` (a real window, redrawn continuously, driven by live
//! DMX via `dmx.rs` — an actual emulated rig you can program lights
//! against). `fixtures.json` (`venue.rs`) is the fixture's fixed *mount*
//! pose in both modes; `dmx.rs` + `channel_map.rs` supply the *live*
//! dimmer/colour/pan/tilt composed on top of it in `live` mode only.

pub mod camera;
pub mod channel_map;
pub mod dmx;
pub mod fixture_profile;
pub mod gdtf_geometry;
pub mod gdtf_import;
pub mod live_headless_renderer;
pub mod live_pipeline;
pub mod live_renderer;
pub mod mesh;
pub mod obj_mesh;
pub mod ofl_import;
pub mod renderer;
pub mod scene;
pub mod show;
pub mod venue;

pub use camera::Camera;
pub use dmx::{DmxUniverses, ResolvedAttributes};
pub use live_headless_renderer::LiveHeadlessRenderer;
pub use live_renderer::LiveRenderer;
pub use renderer::HeadlessRenderer;
pub use scene::build_scene;
pub use venue::Venue;
