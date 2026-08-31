//! The Bevy 3D visualizer, plus the venue/DMX plumbing that feeds it.
//!
//! Layering, which the pre-Bevy version did not really have and paid for:
//! the modules below the renderer know nothing about Bevy's ECS. `venue`
//! loads the extracted venue JSON, `dmx` receives sACN/Art-Net,
//! `channel_map` resolves a universe against a fixture's personality, and
//! `show` plays cues and effects, and `video` decodes screen-canvas
//! clips on threads of its own. `spawn` is the one place that turns any
//! of that into entities, and `app` is the one place that owns a
//! `bevy::App`.
//!
//! `src/bin/viz.rs` is the only binary — windowed by default, or
//! `--snapshot <path>` for a single headless frame. It replaces the
//! `shot`/`live` pair, which were two binaries only because they were two
//! separately hand-written wgpu pipelines.

// A Bevy system *is* a long argument list of `Query`s, and a `Query` with
// a filter — `Query<(Entity, &A, &B), (Added<A>, Without<C>)>` — is what
// clippy calls a complex type. Both lints fire on the ECS's own idiom
// rather than on anything this crate chose, and rewriting a system
// signature to satisfy them makes it harder to read, not easier. Scoped
// to this crate deliberately: the domain crates get no such licence, and
// a long argument list in `ignition-core` is still a design smell there.
#![allow(clippy::type_complexity, clippy::too_many_arguments)]

pub mod app;
pub mod bench;
pub mod budget;
pub mod camera;
pub mod canvas;
pub mod canvas_material;
pub mod channel_map;
pub mod dmx;
pub mod embedded;
pub mod fixture_profile;
pub mod gdtf_assets;
pub mod gdtf_geometry;
pub mod gdtf_import;
pub mod gdtf_mesh;
pub mod gizmos;
pub mod gobo;
pub mod haze;
pub mod haze_texture;
pub mod obj_mesh;
pub mod ofl_import;
pub mod output;
pub mod overlay;
pub mod picking;
pub mod playback;
pub mod preview;
pub mod props;
pub mod show;
#[cfg(feature = "solari")]
pub mod solari;
pub mod spawn;
pub mod venue;
pub mod video;
pub mod view;

pub use app::{Grade, RenderQuality, VizConfig, run, run_export};
pub use camera::{
    ActiveCamera, CameraCommand, CameraPreset, CameraSetup, CameraState, CameraTarget, Cameras,
};
pub use dmx::{DmxUniverses, ResolvedAttributes};
pub use output::{DmxOutput, OutputSummary};
pub use spawn::CanvasClock;
pub use venue::Venue;
pub use view::ViewPreset;
