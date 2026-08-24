//! Screenshot the venue as currently modelled — no window, no display
//! needed. Usage from the repo root:
//!
//! ```bash
//! cargo run -p ignition-viz --bin shot -- \
//!     --venue data/venues/norco --out /tmp/norco.png --view house
//! ```
//!
//! Props (drum kit, speakers, mics, ...) are hidden by default — pass
//! `--show-props` to bring them back.

use ignition_viz::{build_scene, Camera, HeadlessRenderer, Venue};
use std::path::PathBuf;

struct Args {
    venue: PathBuf,
    out: PathBuf,
    width: u32,
    height: u32,
    view: String,
    exclude: Vec<String>,
    focus_chans: Vec<u32>,
    focus_points: Vec<glam::Vec3>,
    show_props: bool,
}

fn parse_args() -> Args {
    let mut venue = PathBuf::from("data/venues/norco");
    let mut out = PathBuf::from("/tmp/ignition-shot.png");
    let mut width = 1600u32;
    let mut height = 1000u32;
    let mut view = "house".to_string();
    let mut exclude = Vec::new();
    let mut focus_chans = Vec::new();
    let mut focus_points = Vec::new();
    let mut show_props = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--venue" => venue = PathBuf::from(args.next().expect("--venue needs a path")),
            "--out" => out = PathBuf::from(args.next().expect("--out needs a path")),
            "--width" => width = args.next().expect("--width needs a number").parse().unwrap(),
            "--height" => height = args.next().expect("--height needs a number").parse().unwrap(),
            "--view" => view = args.next().expect("--view needs house|stage|top|screens|chans"),
            "--exclude" => exclude.push(args.next().expect("--exclude needs a name substring")),
            "--focus-chan" => focus_chans.push(
                args.next().expect("--focus-chan needs a channel number").parse().unwrap(),
            ),
            "--show-props" => show_props = true,
            "--focus-point" => {
                let raw = args.next().expect("--focus-point needs \"x,y,z\"");
                let parts: Vec<f32> = raw.split(',').map(|s| s.trim().parse().unwrap()).collect();
                assert_eq!(parts.len(), 3, "--focus-point needs exactly x,y,z");
                focus_points.push(glam::Vec3::new(parts[0], parts[1], parts[2]));
            }
            other => eprintln!("ignition-shot: ignoring unknown argument {other}"),
        }
    }
    Args { venue, out, width, height, view, exclude, focus_chans, focus_points, show_props }
}

fn main() -> anyhow::Result<()> {
    let args = parse_args();

    let venue = Venue::load(&args.venue)?;
    println!(
        "loaded venue {:?}: {} fixtures, {} room objects, {} screens, {} props",
        args.venue,
        venue.fixtures.len(),
        venue.room.len(),
        venue.screens.len(),
        venue.props.len()
    );

    let (min, max) = venue.bounds();
    let aspect = args.width as f32 / args.height as f32;
    let camera = match args.view.as_str() {
        "stage" => Camera::frame_stage_view(min, max, aspect),
        "top" => Camera::frame_top_view(min, max, aspect),
        "house" => Camera::frame_house_view(min, max, aspect),
        "screens" => {
            let points: Vec<_> = venue.screens.iter().map(|s| s.position.to_glam()).collect();
            anyhow::ensure!(!points.is_empty(), "venue has no screens to frame");
            Camera::frame_points(&points, true, 1.0, aspect)
        }
        "chans" => {
            anyhow::ensure!(!args.focus_chans.is_empty(), "--view chans needs at least one --focus-chan");
            let points: Vec<_> = venue
                .fixtures
                .iter()
                .filter(|f| f.chan.is_some_and(|c| args.focus_chans.contains(&c)))
                .map(|f| f.position.to_glam())
                .collect();
            anyhow::ensure!(
                points.len() == args.focus_chans.len(),
                "found {} of {} requested channels in {:?}",
                points.len(),
                args.focus_chans.len(),
                args.venue
            );
            Camera::frame_points(&points, true, 1.5, aspect)
        }
        "points" => {
            anyhow::ensure!(!args.focus_points.is_empty(), "--view points needs at least one --focus-point");
            Camera::frame_points(&args.focus_points, true, 0.5, aspect)
        }
        other => anyhow::bail!("unknown --view {other}; use house, stage, top, screens, chans, or points"),
    };

    let mesh = build_scene(&venue, &args.exclude, args.show_props, None);
    println!("scene: {} vertices, {} indices", mesh.vertices.len(), mesh.indices.len());

    let renderer = HeadlessRenderer::new()?;
    renderer.render_to_png(&mesh, &camera, args.width, args.height, &args.out)?;

    println!("wrote {}", args.out.display());
    Ok(())
}
