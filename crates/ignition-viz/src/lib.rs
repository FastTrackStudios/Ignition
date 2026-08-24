//! wgpu 3D visualizer. This first pass is deliberately headless — see
//! `docs/research/lighting-console-landscape.md` §7.1 for the open question
//! (how a wgpu surface composes with Dioxus/Blitz) that a real window is
//! blocked on, and `src/bin/shot.rs` for the screenshot CLI this module
//! exists to serve today: render a venue to a PNG with no display attached.

pub mod camera;
pub mod fixture_profile;
pub mod mesh;
pub mod obj_mesh;
pub mod renderer;
pub mod scene;
pub mod venue;

pub use camera::Camera;
pub use renderer::HeadlessRenderer;
pub use scene::build_scene;
pub use venue::Venue;
