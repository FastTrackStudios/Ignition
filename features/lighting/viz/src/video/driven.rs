//! An offline render the caller drives: frame `i` is whatever the caller
//! puts the show at before it is drawn, at a fixed frame time.
//!
//! `app::run_export` walks a song at one constant tempo from its own
//! schedule. A night's songs have tempo maps (ramps, meter changes) that
//! live in the Live set, not here — so the runner, which knows the set,
//! says where each frame is ([`Driver::position`]), and this loop only
//! guarantees the frame arithmetic: Bevy's clock steps exactly `1/fps`
//! per frame however long the GPU takes, and every frame is captured and
//! handed to [`Driver::frame`] in order before the next one starts.

use crate::app::{AUTO_EXPOSURE_PREROLL_SECS, Headless, VizConfig};
use crate::dmx::DmxUniverses;
use crate::gdtf_geometry::GdtfLibrary;
use crate::playback::Playback;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::time::TimeUpdateStrategy;
use std::time::{Duration, Instant};

/// Captures that may be outstanding before the loop waits for the GPU.
const IN_FLIGHT: u32 = 4;

/// Polls the render device: waiting for every submission when `wait`,
/// else only collecting what has finished (the readbacks' callbacks).
// A `PollError` means the device itself is gone (see `Headless::step`).
#[expect(
    clippy::expect_used,
    reason = "a lost GPU device has no fallback; see Headless::step"
)]
fn poll(headless: &Headless, wait: bool) {
    use bevy::render::renderer::RenderDevice;
    use wgpu::PollType;
    let kind = if wait {
        PollType::Wait {
            submission_index: None,
            timeout: None,
        }
    } else {
        PollType::Poll
    };
    headless
        .subapps
        .main
        .world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(kind)
        .expect("polling the render device");
}

/// What drives a [`run_driven`] render.
pub trait Driver {
    /// Puts the show where frame `index` is. Called once per frame
    /// before it is drawn (and repeatedly with index 0 for the pre-roll).
    fn position(&mut self, playback: &mut Playback, index: u32);

    /// Frame `index`, captured: tightly packed RGBA8 at `width`×`height`,
    /// handed over (no copy).
    ///
    /// # Errors
    ///
    /// The frame cannot be written (an encoder that went away, a full
    /// disk); the render stops.
    fn frame(&mut self, index: u32, rgba: Vec<u8>, width: u32, height: u32) -> anyhow::Result<()>;

    /// The render is done: where its time went.
    fn finished(&mut self, stats: &Stats) {
        let _ = stats;
    }
}

/// Where a render's time went.
#[derive(Debug, Clone, Copy, Default)]
pub struct Stats {
    pub frames: u32,
    /// Bevy updates run (one a frame, and one to fetch the last).
    pub updates: u32,
    /// In Bevy's updates: the main world, extraction, the render's encoding.
    pub step: Duration,
    /// Polling the GPU (waiting on it only past `IN_FLIGHT` captures).
    pub wait: Duration,
    /// Handing frames to the driver (its encoder).
    pub hand_off: Duration,
    /// From the first frame to the last (the pre-roll not counted).
    pub total: Duration,
}

/// Renders `frames` frames at `fps`, the show placed by `driver` before
/// each one.
///
/// Before frame 0 the show is held at frame 0's moment while the clock
/// runs (`AUTO_EXPOSURE_PREROLL_SECS`), so exposure and the temporal
/// passes have settled when the first frame is written — exactly as
/// `run_export` does.
///
/// # Errors
///
/// A frame is never captured, cannot be converted, or the driver refuses it.
// r[impl viz.export] - offline, frame by frame, the clock a caller's
pub fn run_driven(
    config: VizConfig,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    fps: u32,
    frames: u32,
    driver: &mut dyn Driver,
) -> anyhow::Result<()> {
    let fps = fps.max(1);
    let mut config = config;
    // A video adapts like an eye, not a still (see `run_export`).
    config.quality.instant_adaptation = false;
    let settle_frames = config.settle_frames.max(1);
    let mut headless = Headless::build(config, DmxUniverses::new(), playback, gdtf);
    let dt = Duration::from_secs_f64(1.0 / f64::from(fps));
    let preroll = settle_frames.max(crate::num::u32_of_f32(
        (AUTO_EXPOSURE_PREROLL_SECS * crate::num::f32_of_u32(fps)).ceil(),
    ));
    for _ in 0..preroll {
        let world = headless.subapps.main.world_mut();
        if let Some(mut playback) = world.get_resource_mut::<Playback>() {
            driver.position(&mut playback, 0);
        }
        world.insert_resource(TimeUpdateStrategy::ManualDuration(dt));
        headless.step();
    }

    // Pipelined: one update per frame. A capture asked for before update
    // `k` shows what update `k - 1` drew (measured: a snap positioned on
    // frame 60 and captured in the same update came out as frame 59's
    // picture), so update `k` places frame `k` and asks for frame `k - 1`;
    // one held update at the end asks for the last. The readback lands a
    // frame or two later, in order or not: frames wait in `ready` until
    // their turn. The clock moves exactly one `dt` per update, as before.
    let began = Instant::now();
    let (tx, rx) = std::sync::mpsc::channel::<(u32, Image)>();
    let mut ready = std::collections::BTreeMap::<u32, Image>::new();
    let mut next = 0_u32;
    let mut update = 0_u32;
    let mut stats = Stats::default();
    while next < frames {
        anyhow::ensure!(
            update <= frames.saturating_add(256),
            "frame {next} was never captured"
        );
        let target = headless.target_image();
        let world = headless.subapps.main.world_mut();
        if update < frames {
            if let Some(mut playback) = world.get_resource_mut::<Playback>() {
                driver.position(&mut playback, update);
            }
        }
        let advance = if update == 0 || update >= frames {
            Duration::ZERO
        } else {
            dt
        };
        world.insert_resource(TimeUpdateStrategy::ManualDuration(advance));
        if let Some(index) = update.checked_sub(1).filter(|i| *i < frames) {
            let tx = tx.clone();
            world.spawn(Screenshot::image(target)).observe(
                move |captured: On<ScreenshotCaptured>| {
                    let _ = tx.send((index, captured.image.clone()));
                },
            );
        }
        // The update, then the GPU polled without waiting: the readbacks
        // finish while the next frame is being built. Only when more than
        // [`IN_FLIGHT`] captures are outstanding does the loop wait for
        // the GPU, so the queue never runs away from it.
        let t = Instant::now();
        headless.subapps.update();
        stats.step = stats.step.saturating_add(t.elapsed());
        let asked = update.min(frames);
        let outstanding = asked
            .saturating_sub(next)
            .saturating_sub(u32::try_from(ready.len()).unwrap_or(u32::MAX));
        let t = Instant::now();
        poll(&headless, outstanding > IN_FLIGHT);
        stats.wait = stats.wait.saturating_add(t.elapsed());
        stats.updates = stats.updates.saturating_add(1);
        update = update.saturating_add(1);
        while let Ok((index, image)) = rx.try_recv() {
            ready.insert(index, image);
        }
        while let Some(mut image) = ready.remove(&next) {
            let t = Instant::now();
            let (width, height) = (image.width(), image.height());
            let packed = matches!(
                image.texture_descriptor.format,
                TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
            ) && usize::try_from(u64::from(width) * u64::from(height) * 4).ok()
                == image.data.as_ref().map(Vec::len);
            let rgba = match image.data.take() {
                Some(bytes) if packed => bytes,
                other => {
                    image.data = other;
                    image
                        .try_into_dynamic()
                        .map_err(|e| anyhow::anyhow!("frame {next}: {e:?}"))?
                        .to_rgba8()
                        .into_raw()
                }
            };
            driver.frame(next, rgba, width, height)?;
            stats.hand_off = stats.hand_off.saturating_add(t.elapsed());
            next = next.saturating_add(1);
        }
    }
    stats.frames = frames;
    stats.total = began.elapsed();
    driver.finished(&stats);
    Ok(())
}
