//! The studio's picture froze when its window was resized: the resize
//! re-pointed every camera — the haze camera included — at the new main
//! target, and the haze composite then sampled the texture it was
//! rendering into, a validation panic on the render thread the main
//! thread waited on forever. This drives the same embedded path
//! headless: render, resize, render — and must come back with a texture
//! of the new size.

use std::time::Duration;

/// r[verify studio.windows.visualizer-anywhere] - a resize keeps rendering
#[test]
fn a_resize_keeps_rendering() {
    let Ok(gpu) = ignition_viz::bench::headless_gpu() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let venue = ignition_viz::venue::Venue::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/venues/norco"
    ))
    .expect("norco loads");
    let config = ignition_viz::VizConfig::headless(venue, 640, 360);
    let playback = ignition_viz::playback::Playback::default();
    let device = gpu.device.clone();
    let mut viz = ignition_viz::embedded::EmbeddedViz::new_with(
        config,
        Default::default(),
        playback,
        Some(ignition_viz::gdtf_geometry::GdtfLibrary::load_default()),
        gpu,
        |_| {},
    );
    let wait = || {
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(Duration::from_secs(10)),
            })
            .expect("device poll");
    };
    let mut first = None;
    for _ in 0..12 {
        first = viz.render(640, 360);
        wait();
    }
    let first = first.expect("a texture at the first size");
    assert_eq!((first.width(), first.height()), (640, 360));

    // The resize. A frozen renderer never posts the new target, so the
    // loop below would keep handing back the old one.
    let mut last = None;
    for _ in 0..30 {
        last = viz.render(800, 450);
        wait();
        if let Some(t) = &last
            && (t.width(), t.height()) == (800, 450)
        {
            break;
        }
    }
    let last = last.expect("a texture after the resize");
    assert_eq!(
        (last.width(), last.height()),
        (800, 450),
        "the renderer never posted the resized target"
    );
}
