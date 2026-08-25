//! The visualizer. Argument parsing, then `ignition_viz::run`.
//!
//! ```text
//! viz --venue data/venues/norco
//! viz --venue data/venues/norco --snapshot out.png --view house
//! ```
//!
//! With no console on the network every fixture just renders dark — this
//! binary responds to sACN/Art-Net when it is there and never requires it.

use ignition_viz::gdtf_geometry::GdtfLibrary;
use ignition_viz::playback::Playback;
use ignition_viz::{run, Venue, ViewPreset, VizConfig};
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
    // Higher than the pre-Bevy default (0.35) because the exposure it
    // was tuned against is gone: beams now render into an HDR target and
    // go through Bevy's tonemapper and bloom rather than being added
    // straight onto an already-tonemapped image.
    let mut haze = 10.0f32;
    // Not zero. A real dark venue genuinely has no ambient fill, but a
    // visualizer the operator is *working* in is not a photograph: with
    // nothing lit you need to still see the stage, the truss and where
    // the fixtures are pointing. Low enough that a lit beam still reads
    // as by far the brightest thing in frame.
    let mut ambient = 0.15f32;
    let mut max_universe = 4u16;
    let mut snapshot: Option<PathBuf> = None;
    let mut settle_frames = 20u32;
    let mut show_props = false;
    let mut cuelist: Option<PathBuf> = None;
    let mut recipes: Option<PathBuf> = None;
    let mut effects: Option<PathBuf> = None;
    let mut cue: Option<usize> = None;
    let mut effect_time: Option<f32> = None;
    let mut gdtf_dir: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut next = |what: &str| args.next().unwrap_or_else(|| panic!("{arg} needs {what}"));
        match arg.as_str() {
            "--venue" => venue_dir = PathBuf::from(next("a path")),
            "--view" => view = next("house|stage|top"),
            "--width" => width = next("a number").parse()?,
            "--height" => height = next("a number").parse()?,
            "--haze" => haze = next("a number, e.g. 0.35").parse()?,
            "--ambient" => ambient = next("a number 0..1").parse()?,
            "--max-universe" => max_universe = next("a number").parse()?,
            "--snapshot" => snapshot = Some(PathBuf::from(next("a path"))),
            "--settle-frames" => settle_frames = next("a number").parse()?,
            "--show-props" => show_props = true,
            // A programmed show. `--cuelist` is the flat compiled form,
            // `--recipes` the authoring form compiled against this
            // venue's own groups; press Space to GO through either. With
            // `--snapshot`, `--cue N` jumps straight to the end of cue
            // N's fade so a look can be captured without a keyboard.
            "--cuelist" => cuelist = Some(PathBuf::from(next("a path"))),
            "--recipes" => recipes = Some(PathBuf::from(next("a path"))),
            "--cue" => cue = Some(next("a 0-based cue index").parse()?),
            // Effects run continuously from load, layered on top of any
            // cue; `--effect-time` freezes one at a chosen moment for a
            // snapshot.
            "--effects" => effects = Some(PathBuf::from(next("a path"))),
            "--effect-time" => effect_time = Some(next("seconds, e.g. 2.5").parse()?),
            // A directory of real `.gdtf` fixture profiles. A patched
            // fixture whose manufacturer/model matches one is drawn from
            // the manufacturer's own geometry tree — real nested
            // yoke/head/beam nodes with real dimensions, and pan/tilt on
            // the joints the file itself names — instead of the generic
            // QLC+ category mesh it otherwise falls back to.
            "--gdtf-dir" => gdtf_dir = Some(PathBuf::from(next("a path"))),
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

    let playback = Playback::load(
        &venue,
        cuelist.as_deref(),
        recipes.as_deref(),
        effects.as_deref(),
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
        },
        playback,
        gdtf,
    );
    Ok(())
}
