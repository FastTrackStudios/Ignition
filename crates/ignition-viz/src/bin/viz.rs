//! The visualizer. Argument parsing, then `ignition_viz::run`.
//!
//! ```text
//! viz --venue data/venues/norco
//! viz --venue data/venues/norco --snapshot out.png --view house
//! viz --venue data/venues/norco --snapshot out.png --camera "Drums"
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
use ignition_viz::{Grade, RenderQuality, Venue, ViewPreset, VizConfig, run, run_export};
use std::path::{Path, PathBuf};

/// Everything parsed off `argv`. One struct rather than fifty locals so
/// `main` can be broken into functions each small enough for
/// `too_many_lines` — see `docs/ops/clippy.md`.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a CLI flag set, one bool per independent on/off switch a user can pass; \
              a bitflags enum would not make `--body-glow --labels --loopback` any clearer"
)]
struct Args {
    venue_dir: PathBuf,
    view: String,
    width: u32,
    height: u32,
    // Defaults per the operator's own call: a real dark venue has no
    // ambient fill at all — everything you see is a fixture's beam or its
    // spill — and enough haze in the air that beams read as visible
    // shafts rather than invisible cones that only show up where they
    // land.
    // A multiplier on the hazers' output, roughly 0..2, where 1.0 is a
    // normally hazed room with the hazers up — see `VizSettings::haze`.
    haze: f32,
    // Stops on the stage camera's EV100 (`app::STAGE_EV100`). Zero is
    // the calibrated camera; the lights themselves are real photometry
    // and never scaled. `+1` doubles the picture.
    exposure: f32,
    // The eye that follows the frame. `--auto-exposure off` for a
    // picture at the fixed stage exposure.
    auto_exposure: bool,
    grade: Grade,
    // Not zero. A real dark venue genuinely has no ambient fill, but a
    // visualizer the operator is *working* in is not a photograph: with
    // nothing lit you need to still see the stage, the truss and where
    // the fixtures are pointing. Low enough that a lit beam still reads
    // as by far the brightest thing in frame.
    ambient: f32,
    max_universe: u16,
    snapshot: Option<PathBuf>,
    // `--export out.mp4 --from-bar A --to-bar B`: the show rendered to a
    // video offline, frame by frame against the song's clock.
    export: Option<PathBuf>,
    // `--bench N`: N frames through the studio's embedded route on a
    // headless device, timed. See `ignition_viz::bench`.
    bench: Option<u32>,
    bench_warmup: u32,
    bench_snapshot: Option<PathBuf>,
    from_bar: Option<u32>,
    to_bar: Option<u32>,
    export_fps: u32,
    settle_frames: u32,
    // On by default now that the people and mic stands have real shapes
    // rather than being placeholder boxes; `--hide-props` for a clean
    // plot of just the rig.
    show_props: bool,
    cuelist: Option<PathBuf>,
    recipes: Option<PathBuf>,
    cue: Option<usize>,
    // A profile look held for the still — `--look` is already the
    // camera's target, so this one is `--look-name`.
    look_name: Option<String>,
    // A library effect staged alone over a flat white rig, for the
    // effects library's previews.
    effect_name: Option<String>,
    // `--previews DIR`: every named effect (and look) rendered to
    // frames, in one process. `--preview-effects all` for the library.
    previews: Option<PathBuf>,
    preview_frames: u32,
    preview_effects: Option<String>,
    preview_looks: Option<String>,
    preview_macros: Option<String>,
    bar: Option<u32>,
    song_bpm: Option<f32>,
    effect_time: Option<f32>,
    gdtf_dir: Option<PathBuf>,
    exclude: Vec<String>,
    // `--body-glow`: lit housings glow their own colour. Off by default,
    // the real fixtures being black.
    body_glow: bool,
    // `--labels`: the DMX address over every fixture, from frame one.
    labels: bool,
    // `--select 1-8`: start with these channels in the programmer's
    // selection, so a still can show what the beams and tints look like.
    select: Option<String>,
    // Ships with the crate, so the visualizer runs from any directory.
    // Packaging this properly is a later problem.
    assets_dir: String,
    screen_content: Option<String>,
    canvas_content: std::collections::HashMap<String, String>,
    // On in a window, off in a snapshot unless asked for.
    overlay: Option<bool>,
    // Off by default: a snapshot with a frame counter baked into it is
    // not a snapshot of the rig, and the operator overlay already
    // carries the number for a live window.
    fps: bool,
    eye: Option<Vec3>,
    look: Option<Vec3>,
    // `--camera <preset>`: start on one of the venue's named cameras
    // (`cameras.json`) — the way a snapshot of the drum cam is taken.
    camera_preset: Option<String>,
    // `--output` sends DMX from the venue's config; off unless asked,
    // since a laptop opening a window should not take over a rig.
    output: bool,
    // `--loopback` feeds the sent frame back into the universes — a
    // verification path, see `ignition_viz::output::LoopbackSink`.
    loopback: bool,
}

impl Args {
    fn defaults() -> Self {
        Self {
            venue_dir: PathBuf::from("data/venues/norco"),
            view: "house".to_string(),
            width: 1600,
            height: 1000,
            haze: 0.6,
            exposure: 0.0,
            auto_exposure: true,
            grade: Grade::Neutral,
            ambient: 0.15,
            max_universe: 4,
            snapshot: None,
            export: None,
            bench: None,
            bench_warmup: 60,
            bench_snapshot: None,
            from_bar: None,
            to_bar: None,
            export_fps: 30,
            settle_frames: 20,
            show_props: true,
            cuelist: None,
            recipes: None,
            cue: None,
            look_name: None,
            effect_name: None,
            previews: None,
            preview_frames: 16,
            preview_effects: None,
            preview_looks: None,
            preview_macros: None,
            bar: None,
            song_bpm: None,
            effect_time: None,
            gdtf_dir: None,
            exclude: Vec::new(),
            body_glow: false,
            labels: false,
            select: None,
            assets_dir: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").to_string(),
            screen_content: Some("screens/rockstars-logo.webp".to_string()),
            canvas_content: std::collections::HashMap::default(),
            overlay: None,
            fps: false,
            eye: None,
            look: None,
            camera_preset: None,
            output: false,
            loopback: false,
        }
    }

    /// Walks `argv`, filling in every field that a flag names. A missing
    /// value after a flag that needs one is reported through `anyhow`
    /// rather than `panic!` — `main` returns a `Result`, and
    /// `panic_in_result_fn` is right that the two should not mix.
    fn parse() -> anyhow::Result<Self> {
        let mut cfg = Self::defaults();
        let mut args = std::env::args().skip(1).peekable();
        while let Some(arg) = args.next() {
            // `--fps` is two flags: the frame-rate readout in a window,
            // and the export rate when followed by a number.
            let fps_number =
                arg == "--fps" && args.peek().is_some_and(|n| n.parse::<u32>().is_ok());
            let mut next = |what: &str| -> anyhow::Result<String> {
                args.next()
                    .ok_or_else(|| anyhow::anyhow!("{arg} needs {what}"))
            };
            if ignition_viz::output::parse_output_flag(&arg, &mut cfg.output) {
                continue;
            }
            if cfg.apply_basic_flag(&arg, fps_number, &mut next)? {
                continue;
            }
            if cfg.apply_media_flag(&arg, &mut next)? {
                continue;
            }
            if cfg.apply_preview_flag(&arg, &mut next)? {
                continue;
            }
            eprintln!("viz: ignoring unknown argument {arg}");
        }
        Ok(cfg)
    }

    /// Rendering, camera and windowing flags. Returns whether `arg` was
    /// one of them.
    fn apply_basic_flag(
        &mut self,
        arg: &str,
        fps_number: bool,
        next: &mut impl FnMut(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<bool> {
        match arg {
            "--loopback" => self.loopback = true,
            "--body-glow" => self.body_glow = true,
            "--labels" => self.labels = true,
            "--select" => self.select = Some(next("channels, e.g. 1-8,12")?),
            "--venue" => self.venue_dir = PathBuf::from(next("a path")?),
            "--view" => self.view = next("house|stage|top")?,
            "--width" => self.width = next("a number")?.parse()?,
            "--height" => self.height = next("a number")?.parse()?,
            "--haze" => {
                self.haze = next("a number, 0..2, 1.0 = normally hazed")?.parse()?;
            }
            "--exposure" => {
                self.exposure = next("stops on the stage exposure, e.g. +1 or -0.5")?.parse()?;
            }
            "--auto-exposure" => {
                self.auto_exposure = match next("on|off")?.as_str() {
                    "on" => true,
                    "off" => false,
                    other => anyhow::bail!("--auto-exposure {other}: on or off"),
                }
            }
            // A look after tonemapping: neutral (the default), warm,
            // cool or punchy.
            "--grade" => {
                let name = next("neutral|warm|cool|punchy")?;
                self.grade = Grade::parse(&name).ok_or_else(|| {
                    anyhow::anyhow!("--grade {name}: neutral, warm, cool or punchy")
                })?;
            }
            "--ambient" => self.ambient = next("a number 0..1")?.parse()?,
            "--max-universe" => self.max_universe = next("a number")?.parse()?,
            "--show-props" => self.show_props = true,
            "--hide-props" => self.show_props = false,
            "--settle-frames" => self.settle_frames = next("a number")?.parse()?,
            // The cue list with cooked status, drawn over the render.
            // Always on in a window; this is for putting it in a
            // snapshot too.
            "--overlay" => self.overlay = Some(true),
            "--fps" if fps_number => self.export_fps = next("frames per second")?.parse()?,
            "--fps" => self.fps = true,
            "--no-overlay" => self.overlay = Some(false),
            // An arbitrary camera, for looking at one thing rather than
            // at the room. Both are needed; either alone is ignored.
            "--eye" => self.eye = Some(parse_point(&next("x,y,z in metres")?)?),
            "--look" => self.look = Some(parse_point(&next("x,y,z in metres")?)?),
            "--camera" => self.camera_preset = Some(next("a preset name from cameras.json")?),
            "--assets" => self.assets_dir = next("a directory")?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Show, export and screen-content flags. Returns whether `arg` was
    /// one of them.
    fn apply_media_flag(
        &mut self,
        arg: &str,
        next: &mut impl FnMut(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<bool> {
        match arg {
            "--snapshot" => self.snapshot = Some(PathBuf::from(next("a path")?)),
            // A video of the show from one bar to another, at `--fps`
            // and `--width`/`--height`, with the `--cuelist` and `--bpm`
            // it should play. An H.264 `.mp4` when built with the
            // `ffmpeg` feature; otherwise a PNG sequence in a directory.
            "--export" => self.export = Some(PathBuf::from(next("a path")?)),
            "--bench" => self.bench = Some(next("a frame count")?.parse()?),
            "--bench-warmup" => self.bench_warmup = next("a frame count")?.parse()?,
            // The bench's last frame as a PNG, at the studio's quality.
            "--bench-snapshot" => self.bench_snapshot = Some(PathBuf::from(next("a path")?)),
            "--from-bar" => self.from_bar = Some(next("a bar number, 1-based")?.parse()?),
            "--to-bar" => self.to_bar = Some(next("a bar number, exclusive")?.parse()?),
            // A programmed show. The two spellings are the same format
            // now — a cue carries direct values and recipes as the two
            // layers of one cascade — and both are kept because both
            // appear in scripts. Press Space to GO. With
            // `--snapshot`, `--cue N` jumps straight to the end of cue
            // N's fade so a look can be captured without a keyboard.
            "--cuelist" => self.cuelist = Some(PathBuf::from(next("a path")?)),
            "--recipes" => self.recipes = Some(PathBuf::from(next("a path")?)),
            "--cue" => self.cue = Some(next("a 0-based cue index")?.parse()?),
            // A profile look (baked or authored) latched on the
            // programmer's held layer, for a preview of the look.
            "--look-name" => self.look_name = Some(next("a look name")?),
            "--effect-name" => self.effect_name = Some(next("a library effect or bundle name")?),
            // Address the show musically rather than by list index.
            "--bar" => self.bar = Some(next("a bar number, 1-based")?.parse()?),
            // Seeds the `Song` speed master, so effects slaved to the
            // song move in a still frame.
            "--bpm" => self.song_bpm = Some(next("beats per minute")?.parse()?),
            // Advances the show clock without advancing the current
            // fade — freezes a running phaser at a chosen moment for a
            // snapshot.
            "--time" | "--effect-time" => {
                self.effect_time = Some(next("seconds, e.g. 2.5")?.parse()?);
            }
            // A directory of real `.gdtf` fixture profiles. A patched
            // fixture whose manufacturer/model matches one is drawn from
            // the manufacturer's own geometry tree — real nested
            // yoke/head/beam nodes with real dimensions, and pan/tilt on
            // the joints the file itself names — instead of the generic
            // QLC+ category mesh it otherwise falls back to.
            "--gdtf-dir" => self.gdtf_dir = Some(PathBuf::from(next("a path")?)),
            // Leave a room object out by name substring. `--exclude
            // Ceiling` is the common one: a plan view otherwise renders
            // only the roof, and at Norco the ceiling plane sits below
            // the truss the pars hang from, so it hides the whole rig.
            "--exclude" => self.exclude.push(next("a name substring")?),
            // What the venue's screens display, relative to --assets.
            // `--screens-off` blanks them.
            "--screen-content" => self.screen_content = Some(next("an asset path")?),
            "--screens-off" => self.screen_content = None,
            // One canvas's source. Panels sharing a canvas each show
            // the slice of it matching where they physically are.
            "--canvas" => {
                let spec = next("name=asset/path")?;
                match spec.split_once('=') {
                    Some((name, path)) => {
                        self.canvas_content
                            .insert(name.to_string(), path.to_string());
                    }
                    None => anyhow::bail!("--canvas wants name=path, got {spec}"),
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// `--previews` and the subject lists it takes. Returns whether
    /// `arg` was one of them.
    fn apply_preview_flag(
        &mut self,
        arg: &str,
        next: &mut impl FnMut(&str) -> anyhow::Result<String>,
    ) -> anyhow::Result<bool> {
        match arg {
            "--previews" => self.previews = Some(PathBuf::from(next("a directory")?)),
            "--preview-frames" => self.preview_frames = next("frames per loop")?.parse()?,
            "--preview-effects" => {
                self.preview_effects = Some(next("names, comma separated, or all")?);
            }
            "--preview-looks" => {
                self.preview_looks = Some(next("names, comma separated, or all")?);
            }
            "--preview-macros" => {
                self.preview_macros = Some(next("names, comma separated, or all")?);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse()?;

    let view = ViewPreset::parse(&args.view)
        .ok_or_else(|| anyhow::anyhow!("unknown --view {}; use house, stage, or top", args.view))?;
    let venue = Venue::load(&args.venue_dir)?;
    let cameras = resolve_cameras(&args.venue_dir, &venue, args.camera_preset.as_deref())?;
    println!(
        "loaded venue {}: {} fixtures, {} room objects",
        args.venue_dir.display(),
        venue.fixtures.len(),
        venue.room.len()
    );

    let gdtf = load_gdtf(args.gdtf_dir.as_deref())?;
    let export = build_export(&args)?;
    let headless = args.snapshot.is_some() || export.is_some();
    // A benchmark runs headless but at the studio's quality: the
    // question it answers is what the studio's viewport costs.
    let overlay = if args.bench.is_some() {
        Some(false)
    } else {
        args.overlay
    };
    let draw_overlay = overlay.unwrap_or(!headless);

    let mut playback = load_playback(&venue, &args, export.as_ref())?;
    stage_look_and_selection(&mut playback, &args)?;

    let quality = pick_quality(headless, args.bench.is_some());
    let config = build_config(&args, venue, view, cameras, quality, draw_overlay);

    run_pipeline(config, playback, gdtf, &args, export)
}

/// Loads the venue's named camera presets and checks `--camera` (if
/// given) actually names one, before anything else is staged.
fn resolve_cameras(
    venue_dir: &Path,
    venue: &Venue,
    camera_preset: Option<&str>,
) -> anyhow::Result<ignition_viz::Cameras> {
    let (min, max) = venue.bounds();
    let cameras = ignition_viz::Cameras::load_or_builtin(venue_dir, min, max);
    if let Some(name) = camera_preset
        && cameras.preset(name).is_none()
    {
        anyhow::bail!(
            "--camera {name}: no such preset; {} has {}",
            venue_dir.join(ignition_viz::camera::FILE).display(),
            cameras
                .presets
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(cameras)
}

/// `--gdtf-dir`, or the crate's own bundled profiles when it is absent.
fn load_gdtf(gdtf_dir: Option<&Path>) -> anyhow::Result<Option<GdtfLibrary>> {
    Ok(if let Some(dir) = gdtf_dir {
        let library = GdtfLibrary::load_dir(dir)?;
        if library.is_empty() {
            eprintln!("viz: no usable .gdtf files in {}", dir.display());
        }
        Some(library)
    } else {
        let library = GdtfLibrary::load_default();
        println!("loaded {} GDTF profiles from data/gdtf", library.len());
        Some(library)
    })
}

/// Validates `--export`'s companion flags and turns them into the
/// request `run_export` wants, or `None` when `--export` was not given.
fn build_export(args: &Args) -> anyhow::Result<Option<ExportRequest>> {
    let Some(path) = args.export.clone() else {
        return Ok(None);
    };
    let (Some(from_bar), Some(to_bar)) = (args.from_bar, args.to_bar) else {
        anyhow::bail!("--export needs --from-bar and --to-bar");
    };
    anyhow::ensure!(to_bar > from_bar, "--to-bar must be after --from-bar");
    anyhow::ensure!(
        args.cuelist.is_some() || args.recipes.is_some(),
        "--export needs a --cuelist"
    );
    Ok(Some(ExportRequest {
        path,
        from_bar,
        to_bar,
        fps: args.export_fps,
    }))
}

/// Loads the cue list or recipe file `--cuelist`/`--recipes` names,
/// starting at the export's first bar when there is one, `--bar`
/// otherwise.
fn load_playback(
    venue: &Venue,
    args: &Args,
    export: Option<&ExportRequest>,
) -> anyhow::Result<Playback> {
    Playback::load(
        venue,
        ignition_viz::playback::LoadOptions {
            cuelist: args.cuelist.as_deref(),
            recipes: args.recipes.as_deref(),
            jump_to_cue: args.cue,
            effect_time: args.effect_time,
            // An export starts at its first bar.
            bar: export.map(|e| e.from_bar).or(args.bar),
            song_bpm: args.song_bpm,
            song: None,
        },
    )
}

/// `--look-name`, `--effect-name` and `--select`, applied to the
/// programmer after the show is loaded.
fn stage_look_and_selection(playback: &mut Playback, args: &Args) -> anyhow::Result<()> {
    if let Some(name) = &args.look_name {
        anyhow::ensure!(
            playback.hold_look(name),
            "no look named {name:?} in the profile"
        );
    }
    if let Some(name) = &args.effect_name {
        anyhow::ensure!(
            playback.preview_effect(name),
            "no library effect or bundle named {name:?}"
        );
    }
    if let Some(select) = &args.select {
        let chans = ignition_viz::picking::parse_chan_ranges(select)?;
        playback
            .programmer
            .select(ignition_core::Selection::Chans(chans));
    }
    Ok(())
}

/// A still keeps the quality every existing snapshot was made at; a
/// window gets the same dials as the studio. `IGNITION_QUALITY`
/// overrides both — which is the only way to take a *repeatable*
/// picture of what a live tier looks like. Judging a beam by
/// screenshotting the studio means fighting a window manager for every
/// comparison; a headless snapshot at `IGNITION_QUALITY=low` is the
/// same frame every time.
// r[impl viz.quality-presets]
fn pick_quality(headless: bool, bench: bool) -> RenderQuality {
    if std::env::var("IGNITION_QUALITY").is_ok() {
        RenderQuality::live()
    } else if headless && !bench {
        RenderQuality::STILL
    } else {
        RenderQuality::live()
    }
}

/// Assembles the `VizConfig` every run path (window, snapshot, export,
/// bench, previews) shares. The few fields cloned out of `args` here
/// (paths, the small canvas map) are cheap one-shot CLI allocations, not
/// a hot path.
fn build_config(
    args: &Args,
    venue: Venue,
    view: ViewPreset,
    cameras: ignition_viz::Cameras,
    quality: RenderQuality,
    draw_overlay: bool,
) -> VizConfig {
    VizConfig {
        quality,
        venue,
        view,
        width: args.width,
        height: args.height,
        haze: args.haze,
        ambient: args.ambient,
        max_universe: args.max_universe,
        snapshot: args.snapshot.clone(),
        settle_frames: args.settle_frames,
        show_props: args.show_props,
        camera: args.eye.zip(args.look),
        cameras,
        camera_preset: args.camera_preset.clone(),
        overlay: draw_overlay,
        // The studio draws the fps readout; so does the bench.
        fps: args.fps || args.bench.is_some(),
        exclude: args.exclude.clone(),
        exposure: args.exposure,
        auto_exposure: args.auto_exposure,
        grade: args.grade,
        screen_content: args.screen_content.clone(),
        canvas_content: args.canvas_content.clone(),
        canvas_focus: std::collections::HashMap::default(),
        assets_dir: args.assets_dir.clone(),
        output: args.output,
        loopback: args.loopback,
        body_glow: args.body_glow,
        labels: args.labels,
    }
}

/// The four ways a run ends: a previews sweep, a benchmark, an offline
/// export, or the interactive window. Previews own the process — they
/// stage each subject themselves, so nothing else may have been staged
/// first — which is why they are checked ahead of the `(bench, export)`
/// dispatch rather than folded into it.
fn run_pipeline(
    config: VizConfig,
    playback: Playback,
    gdtf: Option<GdtfLibrary>,
    args: &Args,
    export: Option<ExportRequest>,
) -> anyhow::Result<()> {
    if let Some(out_dir) = args.previews.clone() {
        use ignition_viz::preview::{PreviewRequest, Subject};
        let pick = |arg: Option<&String>, all: Vec<String>| -> Vec<String> {
            match arg.map(String::as_str) {
                None => Vec::new(),
                Some("all") => all,
                Some(list) => list
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            }
        };
        let mut subjects: Vec<Subject> = pick(
            args.preview_effects.as_ref(),
            playback.library.keys().cloned().collect(),
        )
        .into_iter()
        .map(Subject::Effect)
        .collect();
        subjects.extend(
            pick(
                args.preview_looks.as_ref(),
                playback.looks.keys().cloned().collect(),
            )
            .into_iter()
            .map(Subject::Look),
        );
        subjects.extend(
            // The macros are in the shipped profile, not the venue's.
            // `all` reads it here so the flag can name them; the
            // generator loads it again for the runner.
            pick(
                args.preview_macros.as_ref(),
                ignition_viz::preview::macro_names(),
            )
            .into_iter()
            .map(Subject::Macro),
        );
        anyhow::ensure!(
            !subjects.is_empty(),
            "--previews needs --preview-effects, --preview-looks or --preview-macros"
        );
        println!(
            "rendering {} previews at {}x{}",
            subjects.len(),
            args.width,
            args.height
        );
        return ignition_viz::preview::run_previews(
            config,
            playback,
            gdtf,
            &PreviewRequest {
                out_dir,
                frames: args.preview_frames,
                subjects,
            },
        );
    }
    match (args.bench, export) {
        (Some(frames), _) => {
            let report = ignition_viz::bench::run_bench(
                config,
                playback,
                gdtf,
                frames,
                args.bench_warmup,
                args.bench_snapshot.as_deref(),
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
