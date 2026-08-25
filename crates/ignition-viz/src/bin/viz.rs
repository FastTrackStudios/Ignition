//! The visualizer. Argument parsing, then `ignition_viz::run`.
//!
//! ```text
//! viz --venue data/venues/norco
//! viz --venue data/venues/norco --snapshot out.png --view house
//! ```
//!
//! With no console on the network every fixture just renders dark — this
//! binary responds to sACN/Art-Net when it is there and never requires it.

use bevy::math::Vec3;
use ignition_viz::gdtf_geometry::GdtfLibrary;
use ignition_viz::playback::Playback;
use ignition_viz::spawn::BeamStyle;
use ignition_viz::{Venue, ViewPreset, VizConfig, run};
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
    // A hazer's fluid-output dial, roughly 0..2, where 1.0 is a normally
    // hazed room. Each beam style scales it to whatever its own renderer
    // wants — see `VizSettings::haze`.
    let mut haze = 1.6f32;
    // Multiplier on each fixture's real luminous output. Large because
    // it is doing the job a camera's aperture and shutter would: the
    // room is otherwise unlit, the surfaces in it are mostly black
    // (black plywood deck, black ceiling), and the fog absorbs some of
    // what does reach them. The *relative* brightness between fixtures
    // comes from their own wattage and beam angle now, not from this.
    let mut exposure = 3000.0f32;
    // Not zero. A real dark venue genuinely has no ambient fill, but a
    // visualizer the operator is *working* in is not a photograph: with
    // nothing lit you need to still see the stage, the truss and where
    // the fixtures are pointing. Low enough that a lit beam still reads
    // as by far the brightest thing in frame.
    let mut ambient = 0.15f32;
    let mut max_universe = 4u16;
    let mut snapshot: Option<PathBuf> = None;
    let mut settle_frames = 20u32;
    // On by default now that the people and mic stands have real shapes
    // rather than being placeholder boxes; `--hide-props` for a clean
    // plot of just the rig.
    let mut show_props = true;
    let mut cuelist: Option<PathBuf> = None;
    let mut recipes: Option<PathBuf> = None;
    let mut cue: Option<usize> = None;
    let mut effect_time: Option<f32> = None;
    let mut gdtf_dir: Option<PathBuf> = None;
    let mut exclude: Vec<String> = Vec::new();
    let mut beam_style = BeamStyle::Volumetric;
    // Ships with the crate, so the visualizer runs from any directory.
    // Packaging this properly is a later problem.
    let mut assets_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/assets").to_string();
    let mut screen_content: Option<String> = Some("screens/rockstars-logo.webp".to_string());
    // On in a window, off in a snapshot unless asked for.
    let mut overlay: Option<bool> = None;
    let mut eye: Option<Vec3> = None;
    let mut look: Option<Vec3> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut next = |what: &str| args.next().unwrap_or_else(|| panic!("{arg} needs {what}"));
        match arg.as_str() {
            "--venue" => venue_dir = PathBuf::from(next("a path")),
            "--view" => view = next("house|stage|top"),
            "--width" => width = next("a number").parse()?,
            "--height" => height = next("a number").parse()?,
            "--haze" => haze = next("a number, 0..2, 1.0 = normally hazed").parse()?,
            "--exposure" => {
                exposure = next("a multiplier on real fixture output, e.g. 60").parse()?
            }
            "--ambient" => ambient = next("a number 0..1").parse()?,
            "--max-universe" => max_universe = next("a number").parse()?,
            "--snapshot" => snapshot = Some(PathBuf::from(next("a path"))),
            "--settle-frames" => settle_frames = next("a number").parse()?,
            "--show-props" => show_props = true,
            "--hide-props" => show_props = false,
            // The cue list with cooked status, drawn over the render.
            // Always on in a window; this is for putting it in a
            // snapshot too.
            "--overlay" => overlay = Some(true),
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
            "--screens-off" => screen_content = None,
            "--assets" => assets_dir = next("a directory"),
            // How beams in the air are produced: Bevy's own volumetric
            // fog (haze is a property of the room, shafts fall out of
            // the lighting), or the hand-drawn additive cone ported from
            // ASLS. See `BeamStyle`.
            "--beams" => {
                beam_style = match next("volumetric|shader").as_str() {
                    "volumetric" => BeamStyle::Volumetric,
                    "shader" => BeamStyle::Shader,
                    other => anyhow::bail!("unknown --beams {other}; use volumetric or shader"),
                }
            }
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
        None => None,
    };

    let draw_overlay = overlay.unwrap_or(snapshot.is_none());
    let playback = Playback::load(
        &venue,
        cuelist.as_deref(),
        recipes.as_deref(),
        cue,
        effect_time,
    )?;

    run(
        VizConfig {
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
            exclude,
            beam_style,
            exposure,
            screen_content,
            assets_dir,
        },
        playback,
        gdtf,
    );
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
