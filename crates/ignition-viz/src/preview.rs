//! Preview stills for the library panes: one loop of frames per effect,
//! one frame per look.
//!
//! An effect is a template with no picture of its own, and a still of a
//! chase and a still of a static wash are the same picture. So an effect
//! gets a *loop* — enough frames across one full cycle that the pane can
//! flip through them and show what the effect actually does.
//!
//! Everything renders in **one process**. A cold visualizer costs a
//! couple of seconds to reach its first frame, and the library has over
//! a hundred effects; a process each would be minutes of startup to
//! produce a few seconds of pictures. The rig is built once and every
//! subject is staged onto it in turn.
//!
//! What a preview shows is decided in [`Playback::preview_effect`]: the
//! rig flat white, the effect over it, so the *pattern* is what reads.
//! An effect that sets colour still shows its colour, because the white
//! is underneath it in the cascade.

// r[impl studio.views.whole-profile] - the library shows what each entry does

use crate::VizConfig;
use crate::app::Headless;
use crate::dmx::DmxUniverses;
use crate::gdtf_geometry::GdtfLibrary;
use crate::playback::Playback;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use bevy::time::TimeUpdateStrategy;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::time::Duration;

/// What to draw a preview of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// A library effect or bundle: a loop of frames.
    Effect(String),
    /// A profile look: one frame, since a look holds still.
    Look(String),
    /// A profile macro: a loop, because a macro is a little programme
    /// that plays out over time rather than a state to hold.
    Macro(String),
}

impl Subject {
    pub fn name(&self) -> &str {
        match self {
            Subject::Effect(n) | Subject::Look(n) | Subject::Macro(n) => n,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub out_dir: PathBuf,
    /// Frames across one loop of an effect. A look ignores this.
    pub frames: u32,
    pub subjects: Vec<Subject>,
}

/// How long one cycle of `name` lasts, in seconds, on this rig's tempo.
///
/// An effect's `measure` is in beats, which is the whole point — the
/// same effect is the same gesture at any tempo — so the preview has to
/// ask the tempo to know how long to sample for. A one-shot has no
/// cycle: it runs once, and the preview gives it its own length plus a
/// beat of stillness afterwards so the loop reads as a *hit* rather than
/// a stutter.
fn loop_secs(playback: &Playback, name: &str, bpm: f32) -> f32 {
    let beats_per_sec = bpm.max(1.0) / 60.0;
    let recipe = playback
        .bundles
        .get(name)
        .and_then(|b| b.recipes.first())
        .and_then(|n| playback.library.get(n))
        .or_else(|| playback.library.get(name));
    let Some(recipe) = recipe else {
        return 2.0;
    };
    let beats = recipe.timing.measure.max(0.05);
    let secs = beats / beats_per_sec;
    if recipe.timing.once {
        // The shot, then a rest, so a bump does not read as a strobe.
        (secs + 60.0 / bpm.max(1.0)).clamp(0.4, 4.0)
    } else {
        secs.clamp(0.4, 12.0)
    }
}

/// Renders every subject and writes its frames under `out_dir`.
///
/// A look lands at `<out_dir>/<name>.png`, matching what the Looks pane
/// already reads. An effect lands at `<out_dir>/<name>/00.png`, one file
/// per frame, because the renderer decodes an image to a single buffer
/// and has no notion of an animated one — the pane flips the frames
/// itself.
pub fn run_previews(
    config: VizConfig,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    request: &PreviewRequest,
) -> anyhow::Result<()> {
    let bpm = playback.speeds.get("Song").copied().unwrap_or(120.0);
    // A macro is defined in the shipped profile, which `Playback` does
    // not carry — it holds the *venue* profile. Rather than thread a
    // second one through every constructor, the generator loads it
    // itself, and simply has no macros if it is not there.
    let shipped = request
        .subjects
        .iter()
        .any(|s| matches!(s, Subject::Macro(_)))
        .then(|| ignition_core::Profile::load_with_authored(&profile_path()).ok())
        .flatten();
    let mut runner: Option<ignition_core::macros::MacroRunner> = None;
    let settle = config.settle_frames.max(1);
    let dmx = DmxUniverses::new();
    let mut headless = Headless::build(config, dmx, playback, gdtf);
    std::fs::create_dir_all(&request.out_dir)?;

    let total = request.subjects.len();
    let failed = Arc::new(AtomicBool::new(false));
    // Encoding runs on threads of its own rather than on a shared work
    // pool. It is tempting to reach for rayon here, and it deadlocks:
    // the render thread blocks inside wgpu's `poll(Wait)` waiting for a
    // capture, and if the encode tasks are queued on a pool the renderer
    // also draws from, the workers that would finish the frame are busy
    // encoding the last one. Dedicated writers cannot starve anything.
    //
    // The channel is bounded, so a slow disk applies backpressure to the
    // render loop instead of growing an unbounded queue of 225KB frames.
    let (tx, writers) = writer_pool(Arc::clone(&failed));
    for (i, subject) in request.subjects.iter().enumerate() {
        let name = subject.name();
        // Stage it. Every subject starts from a clean rig, or the last
        // effect goes on running under this one.
        let staged = {
            let world = headless.subapps.main.world_mut();
            let Some(mut playback) = world.get_resource_mut::<Playback>() else {
                anyhow::bail!("the visualizer has no playback");
            };
            playback.programmer.release_effects();
            playback.programmer.clear_values();
            playback.programmer.deselect();
            match subject {
                Subject::Effect(n) => playback.preview_effect(n),
                Subject::Look(n) => playback.hold_look(n),
                Subject::Macro(n) => match shipped.as_ref() {
                    Some(p) => {
                        runner = ignition_core::macros::MacroRunner::from_profile(p, n);
                        runner.is_some()
                    }
                    None => false,
                },
            }
        };
        if !staged {
            tracing::warn!(name, "preview: no such effect or look; skipped");
            continue;
        }

        let (frames, dt) = match subject {
            Subject::Look(_) => (1, Duration::ZERO),
            // A macro's length is its own programme's, which only the
            // runner knows as it goes. Four seconds of it is enough to
            // read, and the loop simply restarts wherever it got to.
            Subject::Macro(_) => (
                request.frames.max(1),
                Duration::from_secs_f32(4.0 / request.frames.max(1) as f32),
            ),
            Subject::Effect(n) => {
                let world = headless.subapps.main.world_mut();
                let secs = world
                    .get_resource::<Playback>()
                    .map(|p| loop_secs(p, n, bpm))
                    .unwrap_or(2.0);
                let frames = request.frames.max(1);
                (frames, Duration::from_secs_f32(secs / frames as f32))
            }
        };

        // Settle: the exposure and the temporal passes need frames to
        // converge, and a preview taken before they have is a preview of
        // the previous subject fading out.
        for _ in 0..settle {
            let world = headless.subapps.main.world_mut();
            world.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
            headless.step();
        }

        let dir = match subject {
            Subject::Look(_) => request.out_dir.clone(),
            Subject::Effect(_) | Subject::Macro(_) => request.out_dir.join(slug(name)),
        };
        std::fs::create_dir_all(&dir)?;
        for frame in 0..frames {
            // Frame 0 is the staged moment; every later frame advances
            // the clock by one step of the loop first.
            let advance = if frame == 0 { Duration::ZERO } else { dt };
            let path = match subject {
                // Slugged, like an effect's directory, and for a
                // reason that is not tidiness: a look called "chorus
                // full" wrote "chorus full.png", the pane referred to
                // it as a `file:` URL, and a URL has to percent-encode
                // that space — which Blitz then read back *without*
                // decoding, so the picture silently did not load. The
                // file-name contract is the fix, and it is the one the
                // effects already had.
                // r[impl studio.views.whole-profile] - a preview's name is a slug
                Subject::Look(_) => dir.join(format!("{}.png", slug(name))),
                Subject::Effect(_) | Subject::Macro(_) => dir.join(format!("{frame:02}.png")),
            };
            if let (Subject::Macro(_), Some(p)) = (subject, shipped.as_ref())
                && let Some(r) = runner.as_mut()
            {
                let world = headless.subapps.main.world_mut();
                if let Some(mut playback) = world.get_resource_mut::<Playback>() {
                    tick_macro(r, &mut playback, p);
                }
            }
            let rgba = capture(&mut headless, advance, &path)?;
            // Blocks only when every writer is busy, which a thumbnail
            // encode never manages against a frame render.
            tx.send((path, rgba))
                .map_err(|_| anyhow::anyhow!("the preview writers stopped"))?;
            // Encode and write off the render thread. This is the only
            // part of the job that parallelises: the GPU renders frames
            // one at a time whatever we do, but PNG encoding is pure CPU
            // and used to stall the loop between every frame. Encoding a
            // thumbnail is a millisecond or two against tens for a
            // frame, so the pool always drains faster than it fills.
        }
        println!(
            "  [{}/{}] {name}: {frames} frame{}",
            i + 1,
            total,
            if frames == 1 { "" } else { "s" }
        );
    }
    // Dropping the last sender ends the writers; joining them is what
    // makes "written" true rather than "queued".
    drop(tx);
    for writer in writers {
        let _ = writer.join();
    }
    anyhow::ensure!(
        !failed.load(Ordering::Relaxed),
        "some preview frames could not be written; see the log"
    );
    println!("previews written to {}", request.out_dir.display());
    Ok(())
}

/// Every macro the shipped profile carries, for `--preview-macros all`.
/// Empty when there is no profile to read, which is the same answer as
/// "no macros" and needs no special case.
pub fn macro_names() -> Vec<String> {
    ignition_core::Profile::load_with_authored(&profile_path())
        .map(|p| p.macros.keys().cloned().collect())
        .unwrap_or_default()
}

/// Where the shipped profile lives, the same way every other surface
/// finds it.
fn profile_path() -> PathBuf {
    PathBuf::from(
        std::env::var("IGNITION_PROFILE")
            .unwrap_or_else(|_| "data/profiles/ignition.ig-profile".to_string()),
    )
}

/// One step of a macro, against the playback the visualizer is drawing.
fn tick_macro(
    runner: &mut ignition_core::macros::MacroRunner,
    playback: &mut Playback,
    profile: &ignition_core::Profile,
) {
    use ignition_core::Show;
    let show = Show {
        groups: &playback.groups,
        palettes: &playback.palettes,
        rig: &playback.rig,
        speeds: &playback.speeds,
        roles: &playback.profile,
        library: &playback.library,
        bundles: &playback.bundles,
        looks: &playback.looks,
        named_tricks: &playback.named_tricks,
        ..Show::new(&playback.groups, &playback.rig)
    };
    runner.tick(
        &mut playback.programmer,
        &mut playback.playbacks,
        profile,
        &show,
    );
}

/// A few threads encoding PNGs, fed by a bounded queue.
type Frame = (PathBuf, image::RgbaImage);
fn writer_pool(failed: Arc<AtomicBool>) -> (SyncSender<Frame>, Vec<std::thread::JoinHandle<()>>) {
    let (tx, rx) = sync_channel::<Frame>(8);
    let rx = Arc::new(std::sync::Mutex::new(rx));
    let writers = (0..4)
        .map(|_| {
            let rx = Arc::clone(&rx);
            let failed = Arc::clone(&failed);
            std::thread::spawn(move || {
                loop {
                    let Ok(guard) = rx.lock() else { return };
                    let Ok((path, rgba)) = guard.recv() else {
                        return;
                    };
                    drop(guard);
                    if let Err(e) = rgba.save(&path) {
                        tracing::error!(path = %path.display(), "preview: {e}");
                        failed.store(true, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect();
    (tx, writers)
}

/// Steps the clock by `advance` and captures the frame. Writing it is
/// the caller's job, off this thread.
fn capture(
    headless: &mut Headless,
    advance: Duration,
    path: &Path,
) -> anyhow::Result<image::RgbaImage> {
    let (tx, rx) = std::sync::mpsc::channel::<Image>();
    let image = headless.target_image();
    let world = headless.subapps.main.world_mut();
    world.insert_resource(TimeUpdateStrategy::ManualDuration(advance));
    world
        .spawn(Screenshot::image(image))
        .observe(move |captured: On<ScreenshotCaptured>| {
            let _ = tx.send(captured.image.clone());
        });
    // The capture lands on the render world a frame or two later; keep
    // stepping with the clock held so no extra show time passes while
    // waiting for it.
    let mut captured = None;
    for _ in 0..32 {
        headless.step();
        headless
            .subapps
            .main
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        if let Ok(image) = rx.try_recv() {
            captured = Some(image);
            break;
        }
    }
    let Some(image) = captured else {
        anyhow::bail!("{}: the frame was never captured", path.display());
    };
    Ok(image
        .try_into_dynamic()
        .map_err(|e| anyhow::anyhow!("{}: {e:?}", path.display()))?
        .to_rgba8())
}

/// A name as a directory: effect names carry spaces, and one nested
/// directory per effect keeps a hundred-odd loops legible on disk.
pub fn slug(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' => c,
            _ => '_',
        })
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name has to survive the round trip to a directory and back to
    /// something the pane can look up.
    #[test]
    fn a_name_becomes_a_directory() {
        assert_eq!(slug("chase"), "chase");
        assert_eq!(slug("bar sparkle"), "bar_sparkle");
        assert_eq!(slug("Audience Sweep"), "audience_sweep");
        assert_eq!(slug("circle/tight"), "circle_tight");
    }
}
