//! The visualizer. Argument parsing, then `ignition_viz::run`.
//!
//! ```text
//! viz --venue data/venues/norco
//! viz --venue data/venues/norco --snapshot out.png --view house
//! viz --venue data/venues/norco --cuelist data/songs/x.json --bpm 128 \
//!     --export out.mp4 --from-bar 9 --to-bar 17 --fps 30 --width 1280 --height 720
//! ```
//!
//! With no console on the network every fixture just renders dark — this
//! binary responds to sACN/Art-Net when it is there and never requires it.

use bevy::math::Vec3;
use ignition_viz::gdtf_geometry::GdtfLibrary;
use ignition_viz::playback::Playback;
use ignition_viz::video::export::ExportRequest;
use ignition_viz::{RenderQuality, Venue, ViewPreset, VizConfig, run, run_export};
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut venue_dir = PathBuf::from("data/venues/norco");
    let mut view = "house".to_string();
    let mut width = 1600u32;
    let mut height = 1000u32;
    // Defaults per the operator's own call: a real dark venue has no
    // ambient fill at all — everything you see is a fixture's beam or its
    // spill — and enough haze in the air that beams read as visible
    // shafts rather than invisible cones that only show up where they
    // land.
    // A multiplier on the hazers' output, roughly 0..2, where 1.0 is a
    // normally hazed room with the hazers up — see `VizSettings::haze`.
    let mut haze = 1.6f32;
    // Multiplier on each fixture's real luminous output. Large because
    // it is doing the job a camera's aperture and shutter would: the
    // room is otherwise unlit, the surfaces in it are mostly black
    // (black plywood deck, black ceiling), and the fog absorbs some of
    // what does reach them. The *relative* brightness between fixtures
    // comes from their own wattage and beam angle now, not from this.
    let mut exposure = 12.0f32;
    // Not zero. A real dark venue genuinely has no ambient fill, but a
    // visualizer the operator is *working* in is not a photograph: with
    // nothing lit you need to still see the stage, the truss and where
    // the fixtures are pointing. Low enough that a lit beam still reads
    // as by far the brightest thing in frame.
    let mut ambient = 0.15f32;
    let mut max_universe = 4u16;
    let mut snapshot: Option<PathBuf> = None;
    // `--export out.mp4 --from-bar A --to-bar B`: the show rendered to a
    // video offline, frame by frame against the song's clock.
    let mut export: Option<PathBuf> = None;
    // `--bench N`: N frames through the studio's embedded route on a
    // headless device, timed. See `ignition_viz::bench`.
    let mut bench: Option<u32> = None;
    let mut bench_warmup = 60u32;
    let mut bench_snapshot: Option<PathBuf> = None;
    let mut from_bar: Option<u32> = None;
    let mut to_bar: Option<u32> = None;
    let mut export_fps = 30u32;
    let mut settle_frames = 20u32;
    // On by default now that the people and mic stands have real shapes
    // rather than being placeholder boxes; `--hide-props` for a clean
    // plot of just the rig.
    let mut show_props = true;
    let mut cuelist: Option<PathBuf> = None;
    let mut recipes: Option<PathBuf> = None;
    let mut cue: Option<usize> = None;
    let mut bar: Option<u32> = None;
    let mut song_bpm: Option<f32> = None;
    let mut effect_time: Option<f32> = None;
    let mut gdtf_dir: Option<PathBuf> = None;
    let mut exclude: Vec<String> = Vec::new();
    // `--body-glow`: lit housings glow their own colour. Off by default,
    // the real fixtures being black.
    let mut body_glow = false;
    // Ships with the crate, so the visualizer runs from any directory.
    // Packaging this properly is a later problem.
    let mut assets_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets").to_string();
    let mut screen_content: Option<String> = Some("screens/rockstars-logo.webp".to_string());
    let mut canvas_content: std::collections::HashMap<String, String> = Default::default();
    // On in a window, off in a snapshot unless asked for.
    let mut overlay: Option<bool> = None;
    // Off by default: a snapshot with a frame counter baked into it is
    // not a snapshot of the rig, and the operator overlay already
    // carries the number for a live window.
    let mut fps = false;
    let mut eye: Option<Vec3> = None;
    let mut look: Option<Vec3> = None;
    // `--output` sends DMX from the venue's config; off unless asked,
    // since a laptop opening a window should not take over a rig.
    let mut output = false;
    // `--loopback` feeds the sent frame back into the universes — a
    // verification path, see `ignition_viz::output::LoopbackSink`.
    let mut loopback = false;

    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        // `--fps` is two flags: the frame-rate readout in a window, and
        // the export rate when followed by a number.
        let fps_number = arg == "--fps" && args.peek().is_some_and(|n| n.parse::<u32>().is_ok());
        let mut next = |what: &str| args.next().unwrap_or_else(|| panic!("{arg} needs {what}"));
        if ignition_viz::output::parse_output_flag(&arg, &mut output) {
            continue;
        }
        match arg.as_str() {
            "--loopback" => loopback = true,
            "--body-glow" => body_glow = true,
            "--venue" => venue_dir = PathBuf::from(next("a path")),
            "--view" => view = next("house|stage|top"),
            "--width" => width = next("a number").parse()?,
            "--height" => height = next("a number").parse()?,
            "--haze" => haze = next("a number, 0..2, 1.0 = normally hazed").parse()?,
            "--exposure" => {
                exposure = next("a multiplier on real fixture candela, e.g. 12").parse()?
            }
            "--ambient" => ambient = next("a number 0..1").parse()?,
            "--max-universe" => max_universe = next("a number").parse()?,
            "--snapshot" => snapshot = Some(PathBuf::from(next("a path"))),
            // A video of the show from one bar to another, at `--fps`
            // and `--width`/`--height`, with the `--cuelist` and `--bpm`
            // it should play. An H.264 `.mp4` when built with the
            // `ffmpeg` feature; otherwise a PNG sequence in a directory.
            "--export" => export = Some(PathBuf::from(next("a path"))),
            "--bench" => bench = Some(next("a frame count").parse()?),
            "--bench-warmup" => bench_warmup = next("a frame count").parse()?,
            // The bench's last frame as a PNG, at the studio's quality.
            "--bench-snapshot" => bench_snapshot = Some(PathBuf::from(next("a path"))),
            "--from-bar" => from_bar = Some(next("a bar number, 1-based").parse()?),
            "--to-bar" => to_bar = Some(next("a bar number, exclusive").parse()?),
            "--fps" if fps_number => export_fps = next("frames per second").parse()?,
            "--settle-frames" => settle_frames = next("a number").parse()?,
            "--show-props" => show_props = true,
            "--hide-props" => show_props = false,
            // The cue list with cooked status, drawn over the render.
            // Always on in a window; this is for putting it in a
            // snapshot too.
            "--overlay" => overlay = Some(true),
            "--fps" => fps = true,
            "--no-overlay" => overlay = Some(false),
            // An arbitrary camera, for looking at one thing rather than
            // at the room. Both are needed; either alone is ignored.
            "--eye" => eye = Some(parse_point(&next("x,y,z in metres"))?),
            "--look" => look = Some(parse_point(&next("x,y,z in metres"))?),
            // A programmed show. The two spellings are the same format
            // now — a cue carries direct values and recipes as the two
            // layers of one cascade — and both are kept because both
            // appear in scripts. Press Space to GO. With
            // `--snapshot`, `--cue N` jumps straight to the end of cue
            // N's fade so a look can be captured without a keyboard.
            "--cuelist" => cuelist = Some(PathBuf::from(next("a path"))),
            "--recipes" => recipes = Some(PathBuf::from(next("a path"))),
            "--cue" => cue = Some(next("a 0-based cue index").parse()?),
            // Address the show musically rather than by list index.
            "--bar" => bar = Some(next("a bar number, 1-based").parse()?),
            // Seeds the `Song` speed master, so effects slaved to the
            // song move in a still frame.
            "--bpm" => song_bpm = Some(next("beats per minute").parse()?),
            // Advances the show clock without advancing the current
            // fade — freezes a running phaser at a chosen moment for a
            // snapshot.
            "--time" | "--effect-time" => effect_time = Some(next("seconds, e.g. 2.5").parse()?),
            // A directory of real `.gdtf` fixture profiles. A patched
            // fixture whose manufacturer/model matches one is drawn from
            // the manufacturer's own geometry tree — real nested
            // yoke/head/beam nodes with real dimensions, and pan/tilt on
            // the joints the file itself names — instead of the generic
            // QLC+ category mesh it otherwise falls back to.
            "--gdtf-dir" => gdtf_dir = Some(PathBuf::from(next("a path"))),
            // Leave a room object out by name substring. `--exclude
            // Ceiling` is the common one: a plan view otherwise renders
            // only the roof, and at Norco the ceiling plane sits below
            // the truss the pars hang from, so it hides the whole rig.
            "--exclude" => exclude.push(next("a name substring")),
            // What the venue's screens display, relative to --assets.
            // `--screens-off` blanks them.
            "--screen-content" => screen_content = Some(next("an asset path")),
            // One canvas's source. Panels sharing a canvas each show
            // the slice of it matching where they physically are.
            "--canvas" => {
                let spec = next("name=asset/path");
                match spec.split_once('=') {
                    Some((name, path)) => {
                        canvas_content.insert(name.to_string(), path.to_string());
                    }
                    None => anyhow::bail!("--canvas wants name=path, got {spec}"),
                }
            }
            "--screens-off" => screen_content = None,
            "--assets" => assets_dir = next("a directory"),
            other => eprintln!("viz: ignoring unknown argument {other}"),
        }
    }

    let view = ViewPreset::parse(&view)
        .ok_or_else(|| anyhow::anyhow!("unknown --view {view}; use house, stage, or top"))?;
    let venue = Venue::load(&venue_dir)?;
    println!(
        "loaded venue {:?}: {} fixtures, {} room objects",
        venue_dir,
        venue.fixtures.len(),
        venue.room.len()
    );

    let gdtf = match &gdtf_dir {
        Some(dir) => {
            let library = GdtfLibrary::load_dir(dir)?;
            if library.is_empty() {
                eprintln!("viz: no usable .gdtf files in {}", dir.display());
            }
            Some(library)
        }
        None => {
            let library = GdtfLibrary::load_default();
            println!("loaded {} GDTF profiles from data/gdtf", library.len());
            Some(library)
        }
    };

    let export = match export {
        Some(path) => {
            let (Some(from_bar), Some(to_bar)) = (from_bar, to_bar) else {
                anyhow::bail!("--export needs --from-bar and --to-bar");
            };
            anyhow::ensure!(to_bar > from_bar, "--to-bar must be after --from-bar");
            anyhow::ensure!(
                cuelist.is_some() || recipes.is_some(),
                "--export needs a --cuelist"
            );
            Some(ExportRequest {
                path,
                from_bar,
                to_bar,
                fps: export_fps,
            })
        }
        None => None,
    };
    let headless = snapshot.is_some() || export.is_some();
    // A benchmark runs headless but at the studio's quality: the
    // question it answers is what the studio's viewport costs.
    let overlay = if bench.is_some() {
        Some(false)
    } else {
        overlay
    };
    let draw_overlay = overlay.unwrap_or(!headless);
    let playback = Playback::load(
        &venue,
        cuelist.as_deref(),
        recipes.as_deref(),
        cue,
        effect_time,
        // An export starts at its first bar.
        export.as_ref().map(|e| e.from_bar).or(bar),
        song_bpm,
        None,
    )?;

    // A still keeps the quality every existing snapshot was made at; a
    // window gets the same dials as the studio.
    let quality = if headless && bench.is_none() {
        RenderQuality::STILL
    } else {
        RenderQuality::live()
    };
    let config = VizConfig {
        quality,
        venue,
        view,
        width,
        height,
        haze,
        ambient,
        max_universe,
        snapshot,
        settle_frames,
        show_props,
        camera: eye.zip(look),
        overlay: draw_overlay,
        // The studio draws the fps readout; so does the bench.
        fps: fps || bench.is_some(),
        exclude,
        exposure,
        screen_content,
        canvas_content,
        canvas_focus: Default::default(),
        assets_dir,
        output,
        loopback,
        body_glow,
    };
    match (bench, export) {
        (Some(frames), _) => {
            let report = ignition_viz::bench::run_bench(
                config,
                playback,
                gdtf,
                frames,
                bench_warmup,
                bench_snapshot.as_deref(),
            )?;
            print!("{}", report.render());
        }
        (None, Some(request)) => run_export(config, playback, gdtf, &request)?,
        (None, None) => run(config, playback, gdtf),
    }
    Ok(())
}

/// Parses an `x,y,z` point in metres, for `--eye` / `--look`.
fn parse_point(text: &str) -> anyhow::Result<Vec3> {
    let parts: Vec<&str> = text.split(',').collect();
    let [x, y, z] = parts[..] else {
        anyhow::bail!("expected x,y,z but got {text}");
    };
    Ok(Vec3::new(
        x.trim().parse()?,
        y.trim().parse()?,
        z.trim().parse()?,
    ))
}
