//! The Bevy 3D visualizer, plus the venue/DMX plumbing that feeds it.
//!
//! Layering, which the pre-Bevy version did not really have and paid for:
//! the modules below the renderer know nothing about Bevy's ECS. `venue`
//! loads the extracted venue JSON, `dmx` receives sACN/Art-Net,
//! `channel_map` resolves a universe against a fixture's personality, and
//! `show` plays cues and effects. `spawn` is the one place that turns any
//! of that into entities, and `app` is the one place that owns a
//! `bevy::App`.
//!
//! `src/bin/viz.rs` is the only binary — windowed by default, or
//! `--snapshot <path>` for a single headless frame. It replaces the
//! `shot`/`live` pair, which were two binaries only because they were two
//! separately hand-written wgpu pipelines.

pub mod app;
pub mod beam;
pub mod channel_map;
pub mod dmx;
pub mod embedded;
pub mod fixture_profile;
pub mod gdtf_geometry;
pub mod gdtf_import;
pub mod obj_mesh;
pub mod ofl_import;
pub mod overlay;
pub mod playback;
pub mod props;
pub mod show;
pub mod spawn;
pub mod venue;
pub mod view;

pub use app::{VizConfig, run};
pub use dmx::{DmxUniverses, ResolvedAttributes};
pub use venue::Venue;
pub use view::ViewPreset;
