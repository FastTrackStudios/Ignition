//! The *Bye Bye Bye* light show, authored against the song's own map.
//!
//! ```bash
//! cargo run -p ignition-daw --bin authorshow -- \
//!     "Bye Bye Bye.RPP" > data/songs/bye-bye-bye.json
//! cargo run -p ignition-daw --bin authorshow -- "Bye Bye Bye.RPP" --lint
//! ```
//!
//! ```text
//! authorshow <project.RPP> [--lint] [--profile <ignition.ig-profile>]
//!            [--merge <existing.json>] [--edits <edits.json>]
//!            [--cameras two|four|eight]
//!            [--sidecar-dir <dir>] [--no-sidecars]
//! ```
//!
//! `--lint` prints the design-guide findings (`ignition_daw::lint`)
//! to stderr instead of the show, and exits non-zero if there are any.
//!
//! ## Regenerating without losing edits
//!
//! The output is a draft (`r[song.generate.is-a-draft]`). To keep what
//! a person changed across a re-run:
//!
//! 1. Edit `data/songs/<song>.json` by hand — retime `CH 1`, add a
//!    `· blackout` cue.
//! 2. List those cues in `data/songs/<song>.edits.json`:
//!    `{ "keep": ["CH 1", "· blackout"] }`. A cue can also be written
//!    there directly under `"cues": { "<name>": { …cue… } }`.
//! 3. Re-run with `--merge data/songs/<song>.json`. Kept cues replace
//!    the regenerated ones of the same name verbatim; everything else
//!    is regenerated. The edits file is found beside the sidecars by
//!    default, or named with `--edits`.
//!
//! Every cue's and trigger's position is written *into* the show file
//! relative to its section (`"at": {"section": "CH 1", "bars": 4}`),
//! with the bar it resolved to beside it (`"resolved"`) for a player
//! that has no song map. A loader with one calls
//! `ignition_daw::reposition` and the show follows the arrangement it
//! has. Library effects are written by name (`{"effect": "circle"}`),
//! never copied. The edits file is found in `--sidecar-dir` (default
//! `data/songs`), is the person's, and is only read.
//!
//! A program rather than hand-edited JSON, for the same reason the cue
//! generator is: every position here is written as *"two bars into CH
//! 1"*, not as bar 25. Move the chorus in the DAW, re-run this, and the
//! show moves with it. Nothing is addressed in seconds and nothing is
//! addressed by absolute bar.
//!
//! Two outputs, per `docs/spec/triggers.md`: a **cue list** of section
//! looks and lifts — fourteen sections and a handful of `·` accents, the
//! things an operator would press GO for — and a **trigger list** of
//! everything the song fires on its own: the charted band hits, thinned
//! to the guide's density, and every figure.
//!
//! ## What each section demonstrates
//!
//! The show is written to `docs/domain/cue-design-guide.md` and each
//! section leans on a different part of the engine, so the file doubles
//! as a tour:
//!
//! | section     | the feature                                                         |
//! |-------------|---------------------------------------------------------------------|
//! | Count-In    | a canvas recipe (`proc` noise) driving the **Bars** dimmer as a bitmap channel |
//! | IN A        | a `Split` (`Ocean`) spread on the wash; `build` L→R; `room circle` in metres |
//! | IN B        | `tilt fan` with the profile's `mirror` trick                          |
//! | VS 1        | opens on the profile's `verse bed` **look** (`{"look": …}`, which carries the `verse bed` bundle); `candle` generator on the key; a **stacked** nod under `circle breathe`; swing position curve |
//! | VS 2        | `OnAxis(Z, Invert(Pan))` — the upper height counter-rotates; `breathe` at `depth` 0.5 — an **effect parameter** on a reference |
//! | PRE / PRE 2 | `paired odds` two-tone; `FocusKeyframes` across the movers; a **delay fan** wipe; a one-shot focus walk; `strobe riser` (PRE 2 only) |
//! | CH 1        | chorus owns Gold/Amber; `FocusFan` Vocal→Audience; `dark chase` Negative; colour snaps, dimmer over a beat |
//! | Break       | opens on the `blackout` look (`Safe`, so a bound `House Lights` survives it); `negative flash` trigger, `lightning` generator on the back |
//! | CH 2        | CH 1 + `windmill` + floor                                             |
//! | BR          | `Congo Split` block; `hue rock`; `figure eight` on Drums; `fire flicker` on the floor |
//! | Breakdown   | `tv flicker` on the key; `fly out`; `saturation breathe`; a `Follow` lift two beats before CH 3 |
//! | CH 3        | `"macro drop"` in `commands` runs the profile's drop macro on the downbeat; the run-out (`· CH 3 drive`) opens on the `chorus full` look; `colour wipe` one-shot; `rainbow` on the bars **filtered to colour** while `strip chase` (`duty` ¼) owns their intensity; `random strobe` and `blinder chase` for four; `chase eighths` on the run-out at `Speed::Scaled { Song, ×2 }` with `duty` ¼ — the show-side form of speed routing |
//! | Outro       | `drain and hold`, `lift off`, an `s_curve` fall to black over eight beats, `osc /show/end` and `"macro end"`; `House Lights` (protected, optional) held at Warm 0.3 through the black |
//! | safe/reset  | open on the `punt` look, then say what differs                       |

use ignition_colour::preset::{ColorSplit, Ref};
use ignition_daw::chart::{HitChart, HitClass};
use ignition_daw::generate::{Kind, kind_of};
use ignition_daw_proto::Position;
use ignition_daw_proto::{Bars, SongMap};
use ignition_proto::Attribute;
use ignition_rig::selection::{Axis, Dir, Order, Where};
use ignition_rig::tricks::{Fan, InvertStyle};
use ignition_rig::{Selection, Trick};
use ignition_show::AttrFilter;
use ignition_show::canvas::{BitmapChannel, CanvasRecipe, Procedural, Quantity};
use ignition_show::cue::{CueFan, CurveName, Trig};
use ignition_show::recipe::Distribute;
use ignition_show::{
    Cue, CueList, Ease, Play, Recipe, RecipeApply, RecipeRef, Speed, Step, Timing, Trigger,
    Waveform,
};

// r[impl song.chart] - the chart is re-read from the project on every run
// r[impl cues.sorted-by-position]
// r[impl song.hits.detection-is-a-draft] - only the chart is consulted; detection is never read here
// r[impl triggers.from-the-chart]
fn main() -> anyhow::Result<()> {
    let opts = Options::parse(std::env::args().skip(1))?;
    let song = ignition_daw::load(&opts.project)?;

    // The chart is read from the project's own HITS track, so there is
    // nothing to pass and nothing to keep in sync: edit the MIDI in
    // REAPER, re-run this, and the show follows. An absent chart is a
    // show with no hits, which is still a show.
    let chart = ignition_daw::chart::read(&opts.project, &song)?;
    let mut list = build(&song, (!chart.is_empty()).then_some(&chart));
    if let Some(setup) = &opts.cameras {
        let added = camera_cuts(&mut list, &song, setup)?;
        eprintln!("camera cuts for the {setup} setup: {added} cue(s) carry a cut");
    }
    if !chart.is_empty() {
        eprintln!(
            "{} triggers from {} charted hits in {} figures; {} cues",
            list.triggers.len(),
            chart.hits.len(),
            chart.groups.len(),
            list.cues.len()
        );
    }

    if opts.lint {
        let splits = splits(&opts.profile);
        for (name, energy, level, layers) in ignition_daw::lint::energy_curve(&list, &song, &splits)
        {
            eprintln!("{name:<18} energy {energy:.2}  level {level:.2}  layers {layers}");
        }
        let findings = ignition_daw::lint::lint(&list, &song, &splits);
        for f in &findings {
            eprintln!("{f}");
        }
        eprintln!("{} finding(s) against the design guide", findings.len());
        if !findings.is_empty() {
            std::process::exit(1);
        }
        return Ok(());
    }

    // A person's edits, laid over the draft.
    // r[impl song.generate.is-a-draft] - re-derivable without destroying edits
    let slug = slug(&song.name);
    let edits_path = opts
        .edits
        .clone()
        .unwrap_or_else(|| opts.sidecar_dir.join(format!("{slug}.edits.json")));
    if edits_path.exists() {
        let edits = ignition_daw::Edits::load(&edits_path)?;
        let existing = match &opts.merge {
            Some(path) => Some(serde_json::from_str::<CueList>(&std::fs::read_to_string(
                path,
            )?)?),
            None => None,
        };
        let merged = ignition_daw::merge(&mut list, existing.as_ref(), &edits);
        eprintln!(
            "kept {} edited cue(s) from {}: {:?}",
            merged.kept.len(),
            edits_path.display(),
            merged.kept
        );
        if !merged.missing.is_empty() {
            eprintln!(
                "warning: {:?} are listed in {} but not in {}",
                merged.missing,
                edits_path.display(),
                opts.merge
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "any --merge file".into())
            );
        }
        // A cue kept from an old file may still carry an absolute bar
        // (a file from before positions were written relative). Its
        // position comes from the arrangement the old file was written
        // for — which we do not have — so the best available is to read
        // it relative to this one, and from the next run on it moves
        // with its section.
        for cue in &mut list.cues {
            if let Some(Position::Absolute(at)) = cue.at {
                cue.at = Some(Position::relative_to(&song, at));
            }
        }
    } else if opts.merge.is_some() {
        eprintln!(
            "warning: --merge given but {} does not exist; nothing kept",
            edits_path.display()
        );
    }

    if opts.no_sidecars {
        eprintln!("note: --no-sidecars is a no-op; positions now live in the show file");
    }

    // Every position is written relative to its section; the bar it
    // resolves to against *this* arrangement rides along for a player
    // that has no song map to hand.
    // r[impl song.relative-position.resolved-on-load] - the file carries both forms
    let unresolved = list.resolve_positions(&song);
    if !unresolved.is_empty() {
        eprintln!("warning: {unresolved:?} name a section this arrangement does not have");
    }

    // The file describes itself: which profile's vocabulary it uses and
    // which project it was written against, so `igcheck` can hold it to
    // the profile without being told, and a second show can find its
    // song. r[impl files.show.song-binding] r[impl profile.ignition-is-per-song]
    let document = ignition_show::show_file::ShowDocument::new(
        list,
        "Ignition",
        ignition_show::show_file::SongBinding {
            project: opts.project.clone(),
            name: song.name.clone(),
        },
    );
    println!("{}", serde_json::to_string_pretty(&document)?);
    Ok(())
}

// ── the camera cut ───────────────────────────────────────────────────
//
// The cut is written into the cues' `commands` beside `macro …`, so it
// rides the same clock as the lighting and there is no second timeline
// (`r[viz.camera-cuts]`). The names are the standard presets — a venue's
// `cameras.json` says where its Drums are — never a venue's own, and
// never a slot number, since slots are per operator (`r[song.no-room]`).
//
// The plan, by section kind: verses on the singer; the pre across the
// side stage and then the guitar on its last-bar wipe; a chorus wide,
// then super wide four bars in; the break on the drums; the breakdown
// on keys, its lift on bass; the outro flat at the lip, and the last
// cue from the bird's eye. Figures and high hits are one-to-two-beat
// punch-ins to the drum cam that return to whatever was up.

/// The cameras each setup has. A cut naming one a setup lacks is not
/// written, so a two-camera show is a two-camera show.
fn setup_cameras(setup: &str) -> anyhow::Result<&'static [&'static str]> {
    Ok(match setup {
        "two" => &["Wide", "Singer"],
        "four" => &["Wide", "Singer", "Drums", "Side stage"],
        // The eight, plus the two specials on `9` and `0`: the flat
        // front and the plan only the outro reaches for.
        "eight" => &[
            "Wide",
            "Singer",
            "Drums",
            "Guitar",
            "Bass",
            "Keys",
            "Side stage",
            "Super wide",
            "Flat front",
            "Bird's eye",
        ],
        other => anyhow::bail!("--cameras {other}: two, four or eight"),
    })
}

/// One cue's cut: the camera, its dissolve in beats, and an optional
/// follow-up — a second camera some beats later in the same cue.
type Cut = (&'static str, f32, Option<(&'static str, f32)>);

/// The section-cue plan, by cue name.
fn cut_plan(name: &str) -> Option<Cut> {
    let n = name.trim_start_matches("· ").trim();
    if n.starts_with("VS") && !n.contains("lift") {
        return Some(("Singer", 2.0, None));
    }
    if n.contains("lift") && n.starts_with("VS") {
        return Some(("Guitar", 0.0, Some(("Singer", 4.0))));
    }
    if n.starts_with("PRE") && n.contains("wipe") {
        return Some(("Guitar", 0.0, None));
    }
    if n.starts_with("PRE") {
        return Some(("Side stage", 1.0, None));
    }
    if n.starts_with("CH") && !n.contains("strobe") && !n.contains("drive") {
        return Some(("Wide", 0.0, Some(("Super wide", 16.0))));
    }
    if n.starts_with("CH") && n.contains("strobe") {
        return Some(("Super wide", 0.0, None));
    }
    if n.starts_with("CH") && n.contains("drive") {
        return Some(("Wide", 0.0, None));
    }
    if n == "Break" {
        return Some(("Drums", 0.0, None));
    }
    if n == "BR" {
        return Some(("Side stage", 2.0, None));
    }
    if n.starts_with("Breakdown") && n.contains("lift") {
        return Some(("Bass", 0.0, None));
    }
    if n.starts_with("Breakdown") {
        return Some(("Keys", 4.0, None));
    }
    if n.starts_with("Outro") && n.contains("end") {
        return None;
    }
    if n.starts_with("Outro") {
        return Some(("Flat front", 2.0, None));
    }
    if n == "reset" {
        return Some(("Bird's eye", 0.0, None));
    }
    if n.starts_with("IN ") {
        return Some(("Wide", 0.0, None));
    }
    if n == "Count-In" {
        return Some(("Super wide", 0.0, None));
    }
    None
}

/// Writes the cut for `setup` into `list`. Returns how many cues carry
/// one.
// r[impl song.camera-cuts] - the cut is cues' commands, by section kind
// r[impl viz.camera-cuts] - `camera <preset> [in n] [after n] [for n]`
fn camera_cuts(list: &mut CueList, song: &SongMap, setup: &str) -> anyhow::Result<usize> {
    let has = setup_cameras(setup)?;
    let allowed = |camera: &str| has.contains(&camera);
    let mut count = 0;
    for cue in &mut list.cues {
        cue.commands.retain(|c| !c.starts_with("camera "));
        let Some((camera, dissolve, follow)) = cut_plan(&cue.name) else {
            continue;
        };
        if allowed(camera) {
            cue.commands.push(if dissolve > 0.0 {
                format!("camera {camera} in {dissolve}")
            } else {
                format!("camera {camera}")
            });
            count += 1;
        }
        if let Some((camera, after)) = follow
            && allowed(camera)
        {
            cue.commands.push(format!("camera {camera} after {after}"));
        }
    }
    // Figures and high hits: a punch-in to the drum cam. One cue per
    // figure at its first moment, two beats; one per high hit, a beat.
    // An accent cue with no recipes, so it never blocks and lights
    // nothing (`r[cues.accents-do-not-block]`).
    if allowed("Drums") {
        // Not on a section's own downbeat: that moment already has a
        // camera, and a punch there would put every verse on the drums
        // for its first beat.
        let downbeats: Vec<Bars> = list
            .cues
            .iter()
            .filter(|c| c.block)
            .filter_map(|c| c.resolved)
            .collect();
        let mut punches: Vec<(Position, Option<Bars>, String, f32)> = Vec::new();
        for t in &list.triggers {
            if t.resolved.is_some_and(|at| downbeats.contains(&at)) {
                continue;
            }
            let punch = if t.name.starts_with("fig ")
                && t.name.contains("· 1/")
                && !t.name.ends_with("cut")
            {
                let figure = t.name.split(" · ").next().unwrap_or("fig").to_string();
                Some((format!("· {figure} drum cam"), 2.0))
            } else if t.name.starts_with("High ") {
                Some((
                    format!("· hit {} drum cam", t.name.trim_start_matches("High Hit ")),
                    1.0,
                ))
            } else {
                None
            };
            if let Some((name, beats)) = punch
                && !punches.iter().any(|(at, _, _, _)| *at == t.at)
            {
                punches.push((t.at.clone(), t.resolved, name, beats));
            }
        }
        for (at, resolved, name, beats) in punches {
            // A moment that already has a cue takes the punch as one more
            // command: the player lands on one cue per position, so a
            // second cue there would be replayed rather than taken and
            // its command never fire.
            if let Some(cue) = list
                .cues
                .iter_mut()
                .find(|c| c.resolved.is_some() && c.resolved == resolved)
            {
                cue.commands.push(format!("camera Drums for {beats}"));
                count += 1;
                continue;
            }
            list.cues.push(Cue {
                name,
                fade_secs: 0.0,
                block: false,
                at: Some(at),
                resolved,
                commands: vec![format!("camera Drums for {beats}")],
                ..Default::default()
            });
            count += 1;
        }
    }
    list.resolve_positions(song);
    Ok(count)
}

/// The whole show: looks, pulses and triggers, positioned and sorted.
fn build(song: &SongMap, chart: Option<&HitChart>) -> CueList {
    let mut list = author(song);
    if let Some(chart) = chart {
        // Pulse first: it belongs *in* the section look, not beside it.
        // A blocking section cue is a complete statement, so a running
        // flash has to be one of its recipes or the next section would
        // cancel it — which is also what makes the pattern change at a
        // section boundary rather than run through the whole song.
        for cue in list.cues.iter_mut().filter(|c| c.block) {
            let recipes = pulses(chart, song, &cue.name);
            cue.recipes.extend(recipes.into_iter().map(RecipeRef::from));
        }
        list.triggers = triggers(chart, song);
    }
    list.triggers.extend(section_triggers(song));
    list.resolve_positions(song);
    list
}

/// The shipped profile, for the two pools `Profile` does not carry:
/// the named colour splits and the shared Tricks chains. Baked in at
/// build time from the same file the venues load, so a name used here
/// is a name the room will know.
const PROFILE: &str = include_str!("../../../../../data/profiles/ignition.ig-profile");

#[derive(Default, serde::Deserialize)]
struct ProfilePools {
    #[serde(default)]
    splits: Vec<ColorSplit>,
    #[serde(default)]
    tricks: std::collections::BTreeMap<String, Vec<Trick>>,
}

fn pools(profile: &std::path::Path) -> ProfilePools {
    std::fs::read_to_string(profile)
        .ok()
        .or_else(|| Some(PROFILE.to_string()))
        .and_then(|raw| serde_json::from_str::<ProfilePools>(&raw).ok())
        .unwrap_or_default()
}

/// The profile's named colour splits, for the lint's hue reading.
fn splits(profile: &std::path::Path) -> Vec<ColorSplit> {
    pools(profile).splits
}

/// The profile's shared Tricks chains, by name.
fn profile_tricks() -> std::collections::BTreeMap<String, Vec<Trick>> {
    serde_json::from_str::<ProfilePools>(PROFILE)
        .expect("the shipped profile parses")
        .tricks
}

/// The command line.
struct Options {
    project: String,
    merge: Option<std::path::PathBuf>,
    edits: Option<std::path::PathBuf>,
    sidecar_dir: std::path::PathBuf,
    no_sidecars: bool,
    lint: bool,
    profile: std::path::PathBuf,
    /// `--cameras <setup>`: write the camera cut into the cues'
    /// commands for this setup — `two`, `four` or `eight`.
    cameras: Option<String>,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let usage = "usage: authorshow <project file> [--lint] [--profile <ig-profile>] \
                     [--merge <existing.json>] [--edits <edits.json>] \
                     [--sidecar-dir <dir>] [--no-sidecars] [--cameras two|four|eight]";
        let mut project = None;
        let mut merge = None;
        let mut edits = None;
        let mut sidecar_dir = std::path::PathBuf::from("data/songs");
        let mut no_sidecars = false;
        let mut lint = false;
        let mut cameras = None;
        let mut profile = std::path::PathBuf::from("data/profiles/ignition.ig-profile");
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            let mut value = |flag: &str| {
                args.next()
                    .ok_or_else(|| anyhow::anyhow!("{flag} needs a value\n{usage}"))
            };
            match arg.as_str() {
                "--merge" => merge = Some(value("--merge")?.into()),
                "--edits" => edits = Some(value("--edits")?.into()),
                "--sidecar-dir" => sidecar_dir = value("--sidecar-dir")?.into(),
                "--profile" => profile = value("--profile")?.into(),
                "--no-sidecars" => no_sidecars = true,
                "--lint" => lint = true,
                "--cameras" => cameras = Some(value("--cameras")?),
                other if other.starts_with("--") => {
                    anyhow::bail!("unknown flag {other}\n{usage}")
                }
                _ if project.is_none() => project = Some(arg),
                _ => anyhow::bail!("{usage}"),
            }
        }
        Ok(Self {
            project: project.ok_or_else(|| anyhow::anyhow!("{usage}"))?,
            merge,
            edits,
            sidecar_dir,
            no_sidecars,
            lint,
            profile,
            cameras,
        })
    }
}

/// `Bye Bye Bye` -> `bye-bye-bye`: how the data files are named.
fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

// ── the rig, by role ─────────────────────────────────────────────────
//
// Every selection here names a **role**, never a group this venue
// happens to have. That is the whole difference between a show and a
// Norco show: `Role("Wash")` is a job every room fills, and the room
// says which of its fixtures fills it.
//
// What survives from the venue-specific version is the *ordering*.
// `Order` wraps a selection, so "the wash, left to right" composes on
// top of a role exactly as it did on top of a tag — and a chase reads as
// a direction because the selection knows the room, at whatever room it
// is resolved against.

/// The main colour surface.
// r[impl song.no-role-binding] - the show, not the song, names the roles
// r[impl song.no-room] - roles, never a fixture, channel or address
fn wash() -> Selection {
    Selection::Role("Wash".into())
}

/// The wash, ordered left to right by where the fixtures actually hang.
///
/// The "build the order, not the effect" seam. Nothing downstream says
/// "left"; it says "spread across the selection", and the selection is
/// what knows which end is which.
// r[impl recipes.selection-owns-order]
fn wash_lr() -> Selection {
    Selection::Order {
        of: Box::new(wash()),
        by: Order::Axis(Axis::X, Dir::Asc),
    }
}

/// Front light on faces.
fn key() -> Selection {
    Selection::Role("Key".into())
}
/// Anything behind the band.
fn back() -> Selection {
    Selection::Role("Back".into())
}
/// Pixel bars and battens — the accent layer.
fn bars() -> Selection {
    Selection::Role("Bars".into())
}
/// Anything that pans and tilts.
fn movers() -> Selection {
    Selection::Role("Movers".into())
}
/// The movers, left to right, so a fan opens across the stage.
fn movers_lr() -> Selection {
    Selection::Order {
        of: Box::new(movers()),
        by: Order::Axis(Axis::X, Dir::Asc),
    }
}
/// The narrow beams, where a room has them — the layer held back until
/// the last chorus.
fn beams() -> Selection {
    Selection::Role("Beams".into())
}
/// Audience blinders. Optional; the peak reads without them.
fn audience() -> Selection {
    Selection::Role("Audience".into())
}
/// The drum special. Optional at most venues, and the show runs without
/// it — a recipe covering nothing is not an error.
fn drums() -> Selection {
    Selection::Role("Drums".into())
}
/// The floor package, where a room has one.
fn floor() -> Selection {
    Selection::Role("Floor".into())
}
/// The room's own lights, where a venue puts them on the desk. A
/// **protected** role: a blackout, the black key, a rig drop, a `Safe`
/// look and the grand master never touch it, so the show can hold it
/// through its own black and the room is never dark under fire
/// regulations. Optional — most rooms keep the house on a wall panel.
// r[impl profile.protected-roles] - authored use
fn house() -> Selection {
    Selection::Role("House Lights".into())
}

/// Every stage layer at once — what a cutout takes away.
///
/// The key is left out on purpose: the guide's cutout is "everything
/// but key to zero", so a face is never cut on a figure. The beams and
/// blinders are left out too — they are the peak's own layer and keep
/// their own clock through a hit.
fn everything() -> Selection {
    Selection::Union(vec![wash(), back(), bars(), movers(), drums(), floor()])
}

// ── recipe shorthands ────────────────────────────────────────────────

/// The brightest a section look may sit outside the peak.
///
/// Headroom, and it is the difference between a chorus that punches and
/// one that sits there. Hits are **additive**: `+0.85` on a fixture
/// already at 1.0 has nowhere to go, so it clamps and nothing visibly
/// happens — which is exactly what a chorus authored at full does to
/// every hit in it. The look has to leave room for the thing that lands
/// on top of it. The last chorus is the one exception, and its hits are
/// cutouts, which is what the guide says to do on a full stage.
const LOOK_CEILING: f32 = 0.72;

fn look(target: Selection, level: f32, colour: &str) -> RecipeRef {
    let mut r = Recipe::new(target, RecipeApply::Dimmer(level.min(LOOK_CEILING)));
    r.steps[0]
        .apply
        .push(RecipeApply::Color(Ref::Named(colour.into())));
    r.into()
}

/// A look with no ceiling — the peak only.
fn full(target: Selection, level: f32, colour: &str) -> RecipeRef {
    let mut r = Recipe::new(target, RecipeApply::Dimmer(level));
    r.steps[0]
        .apply
        .push(RecipeApply::Color(Ref::Named(colour.into())));
    r.into()
}

/// A look whose colour runs from one hue to another across the
/// selection's order — the thing a single colour preset could not say.
// r[impl color.multi] - authored use
fn gradient(target: Selection, level: f32, from: &str, to: &str) -> RecipeRef {
    let mut r = Recipe::new(target, RecipeApply::Dimmer(level.min(LOOK_CEILING)));
    r.steps[0].apply.push(RecipeApply::Colors {
        colors: vec![Ref::Named(from.into()), Ref::Named(to.into())],
        distribute: Distribute::Spread,
    });
    r.into()
}

/// Two colours alternating fixture by fixture.
fn two_tone(target: Selection, level: f32, a: &str, b: &str) -> RecipeRef {
    let mut r = Recipe::new(target, RecipeApply::Dimmer(level.min(LOOK_CEILING)));
    r.steps[0].apply.push(RecipeApply::Colors {
        colors: vec![Ref::Named(a.into()), Ref::Named(b.into())],
        distribute: Distribute::Cycle,
    });
    r.into()
}

/// A named palette split from the profile — `Ocean`, `Congo Split` —
/// laid across the selection the way the split says.
// r[impl color.recall-by-reference] - the cue stores the split's name
fn split(target: Selection, level: f32, name: &str) -> RecipeRef {
    let mut r = Recipe::new(target, RecipeApply::Dimmer(level.min(LOOK_CEILING)));
    r.steps[0]
        .apply
        .push(RecipeApply::Split(Ref::Named(name.into())));
    r.into()
}

fn level(target: Selection, level: f32) -> RecipeRef {
    Recipe::new(target, RecipeApply::Dimmer(level)).into()
}

fn dark(target: Selection) -> RecipeRef {
    level(target, 0.0)
}

fn aim(target: Selection, focus: &str) -> RecipeRef {
    Recipe::new(target, RecipeApply::FocusPoint(Ref::Named(focus.into()))).into()
}

/// The movers fanned between two focus roles, first to last across the
/// stage. A fan is what makes eight beams read as a rig rather than
/// eight lights pointing at one spot.
// r[impl focus.pattern.fan] - authored use
fn fan(from: &str, to: &str) -> RecipeRef {
    Recipe::new(
        movers_lr(),
        RecipeApply::FocusFan {
            from: Ref::Named(from.into()),
            to: Ref::Named(to.into()),
        },
    )
    .into()
}

/// The movers aimed along several focus roles at once — the first head
/// at the first, the last at the last, every head between interpolated
/// through its own placement. MA3's MAgic presets.
// r[impl focus.magic] - authored use
fn keyframes(points: &[&str]) -> RecipeRef {
    Recipe::new(
        movers_lr(),
        RecipeApply::FocusKeyframes(points.iter().map(|p| Ref::Named((*p).into())).collect()),
    )
    .into()
}

/// A one-shot walk of the whole mover rig through several aims over
/// `bars`, easing between them — the pickup into a chorus.
fn focus_walk(points: &[&str], bars: f32) -> RecipeRef {
    let steps = points
        .iter()
        .map(|p| {
            let mut s = Step::new(vec![RecipeApply::FocusPoint(Ref::Named((*p).into()))]);
            s.transition = 1.0;
            s.ease = Ease::Sine;
            s
        })
        .collect();
    Recipe {
        target: movers(),
        steps,
        timing: Timing {
            speed: Speed::Master("Song".into()),
            measure: bars * 4.0,
            once: true,
            ..Default::default()
        },
        ..Default::default()
    }
    .into()
}

/// A relative intensity phaser on a selection, slaved to the song.
///
/// Relative on purpose — Eos's own model is that step effects modulate
/// *intensity* and leave colour alone, "which is the property that makes
/// them layerable". A `Delta` says how much to take away; whatever
/// colour the cue set is none of its business.
// r[impl effects.masters.song] - slaved to the `Song` master
fn chase(target: Selection, depth: f32, bars: f32, spread: f32, play: Play) -> RecipeRef {
    Recipe {
        target,
        steps: Waveform::Sine.steps(Attribute::Dimmer, -depth, depth, true),
        timing: Timing {
            speed: Speed::Master("Song".into()),
            measure: bars * 4.0,
            phase_spread_deg: spread,
            direction: play,
            ..Default::default()
        },
        ..Default::default()
    }
    .into()
}

/// A small tilt shiver that **stacks** under whatever position effect
/// is already running — the library's `nod` shape, written inline so it
/// can carry `stack: true`, which a name cannot. Two relatives on one
/// attribute summing is the whole point (`r[effects.relative-stack]`).
fn stacked_nod(bars: f32) -> RecipeRef {
    Recipe {
        target: movers(),
        steps: Waveform::Sine.steps(Attribute::Tilt, 0.0, 4.0, true),
        timing: Timing {
            speed: Speed::Master("Song".into()),
            measure: bars * 4.0,
            ..Default::default()
        },
        stack: true,
        ..Default::default()
    }
    .into()
}

/// A canvas picture driving the bars' dimmer: the `Main` canvas's noise
/// field, sampled where each bar sits in the rig grid, so the TVs and
/// the battens carry one slow picture between them.
// r[impl canvas.bitmap-channels] - authored use
// r[impl canvas.procedural]
fn canvas_noise_on_bars(colour: &str) -> RecipeRef {
    let mut r = Recipe::new(
        bars(),
        RecipeApply::Canvas {
            recipe: CanvasRecipe {
                source: Procedural::Noise {
                    scale: 2.0,
                    seed: 1741,
                    colors: vec![[0.0, 0.0, 0.08], [0.1, 0.25, 0.6], [0.5, 0.75, 1.0]],
                },
                timing: Timing {
                    speed: Speed::Master("Song".into()),
                    measure: 16.0,
                    ..Default::default()
                },
            },
            channel: BitmapChannel {
                canvas: "Main".into(),
                quantity: Quantity::Brightness,
                attr: Attribute::Dimmer,
                low: 0.03,
                high: 0.3,
                relative: false,
            },
        },
    );
    r.steps[0]
        .apply
        .push(RecipeApply::Color(Ref::Named(colour.into())));
    r.into()
}

/// A library effect by name, retargeted and retimed for this show.
///
/// Written into the file as a *reference* — the player looks the name
/// up in the library when the cue is taken — so the show follows the
/// library rather than freezing a copy of it. The library is written
/// against roles, so most of the time it is used as shipped; `bars`
/// overrides the loop length so a section can run the same shape at
/// half or double rate without a second recipe. The name is checked
/// here so a typo fails the run, not the show.
// r[impl effects.library.by-name] - the file names the effect; nothing is copied
// r[impl profile.effect-parameters] - `bars` is written as the `bars` parameter
fn effect(name: &str, target: Option<Selection>, bars: Option<f32>) -> RecipeRef {
    assert!(
        ignition_effects::effects::library().contains_key(name),
        "no library effect named {name:?}"
    );
    let mut r = RecipeRef::named(name);
    if let Some(t) = target {
        r = r.on(t);
    }
    if let Some(b) = bars {
        r = r.with_param("bars", b);
    }
    r
}

/// A library effect with one effect parameter set — `depth`, `bars`,
/// `duty` — the same three a page fader exposes, meaning the same
/// thing: the engine applies them to a copy of the library recipe at
/// the moment the cue resolves, so the library is never rewritten and
/// a strobe at a quarter duty on a fader and in a cue are one thing.
// r[impl profile.effect-parameters] - authored use on a cue's reference
fn param(r: RecipeRef, name: &str, value: f32) -> RecipeRef {
    assert!(
        matches!(r, RecipeRef::Named { .. }),
        "{name} is an effect parameter; {r:?} is not a library effect"
    );
    r.with_param(name, value)
}

/// A library effect narrowed to attribute families — a `rainbow` that
/// may only colour, so the chase beside it keeps the intensity. The
/// engine drops every emit outside the filter when the cue resolves.
// r[impl profile.attribute-filter] - authored use on a cue's reference
fn filtered(r: RecipeRef, filter: AttrFilter) -> RecipeRef {
    assert!(
        matches!(r, RecipeRef::Named { .. }),
        "a filter goes on a library effect; {r:?} is not one"
    );
    r.filtered(filter)
}

/// A library effect at an explicit speed. The **show-side form of speed
/// routing**: a page fader routes by the effect's family through the
/// profile's table (`r[profile.speed-routing]`) because a fader has no
/// song to follow; a cue is synced to the song, so everything here stays
/// on the `Song` master and a per-recipe `Scaled` is how one effect runs
/// double or half against it.
// r[impl profile.speed-routing] - the show's spelling: an explicit scale on the Song master
fn at_speed(r: RecipeRef, speed: Speed) -> RecipeRef {
    assert!(
        matches!(r, RecipeRef::Named { .. }),
        "a speed goes on a library effect; {r:?} is not one"
    );
    r.at(speed)
}

/// A profile look by name — `verse bed`, `chorus full`, `punt`,
/// `blackout` — every recipe it carries, as the busk keys hold it. A
/// section opens on the look that fits and states what differs on top,
/// so the same scene under a hand and under the clock is the same
/// scene. The name is checked here so a typo fails the run.
// r[impl profile.looks] - authored use in a cue
fn look_ref(name: &str) -> RecipeRef {
    assert!(
        ignition_playback::macros::looks().contains_key(name),
        "no profile look named {name:?}"
    );
    RecipeRef::look(name)
}

/// A library bundle by name — several effects taken as one. This show
/// takes its bundle (`verse bed`) through the look of the same name
/// now, so the helper is kept for the next show rather than used here.
// r[impl effects.bundle] - authored use
#[allow(dead_code)]
fn bundle(name: &str) -> RecipeRef {
    assert!(
        ignition_effects::effects::bundles().contains_key(name),
        "no library bundle named {name:?}"
    );
    RecipeRef::Bundle {
        bundle: name.to_string(),
        target: None,
    }
}

/// A recipe with Tricks on it.
// r[impl tricks.on-the-recipe] - authored use
fn tricked(r: RecipeRef, tricks: Vec<Trick>) -> RecipeRef {
    match r {
        RecipeRef::Inline(mut r) => {
            r.tricks = tricks;
            RecipeRef::Inline(r)
        }
        named @ RecipeRef::Named { .. } => named.tricked(tricks),
        other => other,
    }
}

/// A recipe using one of the profile's shared, named Tricks chains —
/// `paired odds`, `mirror` — by reference, so one edit to the pool is a
/// rig-wide layout change.
// r[impl tricks.shared-or-inline] - authored use of the reference half
fn with_trick(r: RecipeRef, name: &str) -> RecipeRef {
    let pool = profile_tricks();
    assert!(pool.contains_key(name), "no profile trick named {name:?}");
    match r {
        RecipeRef::Inline(mut r) => {
            r.tricks_ref = Some(name.to_string());
            RecipeRef::Inline(r)
        }
        // A named effect carries its tricks inline; the pool's chain is
        // copied in so the file still says which one it meant.
        named @ RecipeRef::Named { .. } => named.tricked(profile_tricks()[name].clone()),
        other => other,
    }
}

// ── positions ────────────────────────────────────────────────────────

/// `bars` into the `nth` section with this name. Every cue in this file
/// is placed this way, which is what makes the show survive an
/// arrangement edit — and naming the occurrence is what lets the second
/// PRE be found without a workaround. The position is *written* in this
/// form, with the bar it resolved to beside it.
// r[impl song.relative-position]
// r[impl song.relative-position.duplicate-names] - the nth occurrence by name
fn at_nth(section: &str, nth: usize, bars: u32) -> Position {
    Position::nth(section, nth, bars)
}

fn at(section: &str, bars: u32) -> Position {
    at_nth(section, 0, bars)
}

/// A beat inside a bar of a section — "two beats before the chorus".
fn at_beat(section: &str, nth: usize, bars: u32, beat: f64) -> Position {
    Position::Relative {
        section: section.to_string(),
        ordinal: nth,
        bars,
        beat,
    }
}

/// The last bar of a named section — where a build lands and a stab
/// goes.
// r[impl song.relative-position] - "the last bar of"
fn last_bar(section: &str) -> Position {
    Position::last_bar(section)
}

/// The last bar of the `nth` section with this name.
fn last_bar_nth(section: &str, nth: usize) -> Position {
    Position::LastBar {
        section: section.to_string(),
        ordinal: nth,
        last_bar: true,
    }
}

struct Author<'a> {
    song: &'a SongMap,
    cues: Vec<Cue>,
}

impl Author<'_> {
    /// A cue at a musical position. Fades are in beats and converted
    /// through the tempo map, so the show holds at any tempo.
    // r[impl cues.fade-in-beats]
    // r[impl cues.accents-do-not-block] - `·`-named cues never block
    // r[impl cues.sections-block]
    // r[impl cues.recipes-not-values] - `values` is always empty
    // r[impl cues.position]
    // r[impl song.relative-position] - the cue carries the relative form
    fn cue(
        &mut self,
        position: Position,
        name: &str,
        fade_beats: f64,
        recipes: Vec<RecipeRef>,
    ) -> Option<&mut Cue> {
        let Some(at) = position.resolve(self.song) else {
            // A section this arrangement does not have is skipped rather
            // than placed at bar 1, which is where an `unwrap_or_default`
            // would silently put it.
            eprintln!("warning: skipping {name:?} — the arrangement has no such section");
            return None;
        };
        let bpm = self.song.tempo.at(at).bpm;
        self.cues.push(Cue {
            name: name.to_string(),
            fade_secs: (fade_beats * 60.0 / bpm) as f32,
            values: Vec::new(),
            recipes,
            // Sections are complete statements; accents inside a section
            // are deltas on top of it and must NOT block.
            block: !name.starts_with('·'),
            at: Some(position),
            resolved: Some(at),
            ..Default::default()
        });
        self.cues.last_mut()
    }

    /// Beats in a named section, for a follow that has to land a fixed
    /// distance before the next one.
    fn beats_in(&self, section: &str) -> f32 {
        self.song
            .section(section)
            .map(|s| {
                (s.bars * self.song.tempo.at(s.start).time_signature.numerator.max(1) as f64) as f32
            })
            .unwrap_or(16.0)
    }
}

// ── the show ─────────────────────────────────────────────────────────
//
// *Bye Bye Bye* at 86 BPM: a cold, mechanical intro riff; verses that
// sit low under the vocal; a pre-chorus that is one long build; a
// chorus everyone in the room knows the words to; a bridge that goes
// somewhere else; a breakdown that strips it back; and a last chorus
// that is the biggest thing in the show, ending on the eight-hit stab.
//
// The palette is a decision, not a default, and the guide's: **two hues
// and white**. The *home* hue is violet — Lavender, Purple, Magenta,
// Pink, Indigo, Congo — and it is the verse, the pre, the bridge and
// the break. The **chorus owns Gold and Amber** and they appear nowhere
// else but the last bar of a pre. The intro and outro are the frame:
// cold (Ocean, Deep Blue, Cyan) — the third hue, kept away from the
// vocal core. The key is Warm White at one colour temperature all song.
//
// Energy: CH 1 holds back (no beams, no floor, static movers), CH 2 is
// CH 1 plus the windmill and the floor, and CH 3 is the only cue at
// full — beams, blinders, a strobe for four bars and a rainbow on the
// bars for four. The breakdown before it is the darkest thing since the
// count-in.

// r[impl song.relative-position] - every cue placed relative to a section
// r[impl cues.sorted-by-position]
fn author(song: &SongMap) -> CueList {
    let mut a = Author {
        song,
        cues: Vec::new(),
    };

    // ── safe ─────────────────────────────────────────────────────────
    // The profile's punt look — stage lit, warm, nothing moving — then
    // this show's own colours on top. Positioned under the count-in so
    // the clock never lands on it and GO always can. The count-in
    // blocks over it a frame later.
    a.cue(
        at("Count-In", 0),
        "safe",
        1.0,
        vec![
            look_ref("punt"),
            look(key(), 0.6, "Warm White"),
            look(wash(), 0.5, "Lavender"),
            look(back(), 0.4, "Indigo"),
            dark(bars()),
            dark(movers()),
            dark(beams()),
            dark(floor()),
            dark(audience()),
            dark(drums()),
        ],
    );

    // ── count-in ─────────────────────────────────────────────────────
    // Nothing but the back wall and a slow noise field drifting over
    // the bars — the same picture the TVs carry, sampled onto the
    // battens' dimmer. The first note has somewhere to arrive from.
    // The house is set here, once, at a working level: it is a
    // protected role, so nothing between here and the end — the
    // break's blackout look, the drop macro's release, the end's
    // blackout, a rig drop from the desk — takes it away. A room that
    // binds it keeps its house through the show's black; a room that
    // does not has nothing to lose.
    a.cue(
        at("Count-In", 0),
        "Count-In",
        0.0,
        vec![
            look(house(), 0.3, "Warm White"),
            dark(wash()),
            dark(key()),
            dark(movers()),
            dark(beams()),
            dark(drums()),
            dark(floor()),
            dark(audience()),
            look(back(), 0.12, "Deep Blue"),
            canvas_noise_on_bars("Deep Blue"),
        ],
    );

    // ── intro A: the riff, cold and mechanical ───────────────────────
    // The Ocean split spread across the wash, filling left to right
    // every two bars — the riff stated by the rig. Movers up on the
    // back wall drawing a two-metre circle round it, the same size in
    // any room. No key light: nobody is singing.
    a.cue(
        at("IN A", 0),
        "IN A",
        1.0,
        vec![
            split(wash_lr(), 0.3, "Ocean"),
            dark(key()),
            look(back(), 0.45, "Deep Blue"),
            look(bars(), 0.3, "Deep Blue"),
            look(movers(), 0.5, "Cyan"),
            aim(movers(), "Back Wall"),
            dark(beams()),
            dark(floor()),
            dark(audience()),
            dark(drums()),
            effect("build", Some(wash_lr()), None),
            effect("room circle", None, None),
        ],
    );

    // ── intro B: the room opens ──────────────────────────────────────
    // One size bigger: the same cold split, and the movers lift from
    // the centre outward and settle, mirrored so the two halves of the
    // rig answer each other.
    a.cue(
        at("IN B", 0),
        "IN B",
        2.0,
        vec![
            split(wash_lr(), 0.4, "Ocean"),
            dark(key()),
            look(back(), 0.6, "House Blue"),
            look(bars(), 0.35, "Deep Blue"),
            look(movers(), 0.6, "Cyan"),
            aim(movers(), "Back Wall"),
            with_trick(effect("tilt fan", None, None), "mirror"),
            chase(wash_lr(), 0.15, 2.0, 360.0, Play::Bounce),
        ],
    );

    // ── verses: low, and the vocal is the picture ────────────────────
    // Home. Key warm and up with a candle in it — barely there; the
    // wash a lavender-to-purple spread, low; indigo behind. The movers
    // run the library's verse bed (the wash breathing under a circle
    // that swells and shrinks with it) with a stacked nod summed on the
    // tilt, so the beams drift and bob at once. The position class
    // arrives on a swing curve over a bar, per `set_class_timing`.
    // The second verse counter-rotates the upper row of movers about Z,
    // which a venue with one height simply does not have.
    let verse = |second: bool| {
        let mut v = vec![
            gradient(wash_lr(), 0.32, "Lavender", "Purple"),
            look(key(), 0.72, "Warm White"),
            effect("candle", None, None),
            look(back(), 0.35, "Indigo"),
            look(bars(), if second { 0.3 } else { 0.2 }, "Purple"),
            look(movers(), 0.3, "Indigo"),
            aim(movers(), "Band"),
            stacked_nod(2.0),
            dark(beams()),
            dark(floor()),
            dark(audience()),
            dark(drums()),
        ];
        if second {
            // The second verse restates the bed itself, because its
            // circle carries a trick the look's cannot: half as deep a
            // breathe as the library's — the second verse moves less,
            // not more, so the pre has somewhere to go — under the
            // counter-rotating circle.
            v.push(param(effect("breathe", None, None), "depth", 0.5));
            v.push(tricked(
                effect("circle breathe", None, None),
                vec![Trick::OnAxis(
                    Axis::Z,
                    Box::new(Trick::Invert(InvertStyle::Pan)),
                )],
            ));
        } else {
            // The profile's verse bed — key warm, back deep, the wash
            // breathing under a circle — is the scene the VERSE key
            // holds; the first verse opens on it and says what differs.
            v.insert(0, look_ref("verse bed"));
        }
        v
    };
    if let Some(c) = a.cue(at("VS 1", 0), "VS 1", 2.0, verse(false)) {
        c.timing.ease.position = CurveName::Swing.ease();
    }
    // Halfway through, the bars join in — a lift the ear hears at the
    // second half of a verse.
    a.cue(
        at("VS 1", 4),
        "· VS 1 lift",
        2.0,
        vec![look(bars(), 0.35, "Magenta")],
    );

    // ── pre-chorus: one long build ───────────────────────────────────
    // Magenta and pink in pairs across the wash — the `paired odds`
    // trick from the profile — and a build that fills the wash every
    // bar. The movers come to the band, the vocal and the audience in
    // one keyframed line across the truss. Beams stay dark: they are
    // for the end.
    let pre = || {
        vec![
            with_trick(two_tone(wash_lr(), 0.55, "Magenta", "Pink"), "paired odds"),
            look(key(), 0.7, "Warm White"),
            look(back(), 0.6, "Magenta"),
            look(bars(), 0.55, "Magenta"),
            look(movers(), 0.6, "Indigo"),
            keyframes(&["Band", "Vocal", "Audience"]),
            dark(beams()),
            dark(floor()),
            dark(audience()),
            dark(drums()),
            effect("build", Some(wash_lr()), Some(1.0)),
        ]
    };
    a.cue(at("PRE", 0), "PRE", 2.0, pre());
    // The last bar: the wash wipes up left to right on a **delay fan**
    // — every fixture arrives at the same pink, just later — while the
    // movers walk Band → Vocal → Audience over the bar and land on the
    // downbeat. Figure 0 (the three hits carving the stage) sits on top.
    let pre_lift = |riser: bool| {
        let mut v = vec![
            look(wash_lr(), 0.65, "Pink"),
            focus_walk(&["Band", "Vocal", "Audience"], 1.0),
        ];
        if riser {
            // The shutter climbs from nothing over the bar and the
            // chorus ends it — the one thing PRE 2 has that PRE 1 did
            // not, and the first the beams are seen.
            v.push(look(beams(), 0.4, "Gold"));
            v.push(aim(beams(), "Audience"));
            v.push(effect("strobe riser", None, Some(1.0)));
        }
        v
    };
    if let Some(c) = a.cue(last_bar("PRE"), "· PRE wipe", 1.0, pre_lift(false)) {
        c.fan = Some(CueFan {
            delay: Fan {
                from: 0.0,
                to: 2.0,
                ..Default::default()
            },
            fade: Fan::default(),
        });
    }

    // ── choruses ─────────────────────────────────────────────────────
    // Everything the chorus owns, and only the chorus: gold on the
    // wash, amber behind and on the bars, open white movers fanned from
    // the singer out over the crowd. A dark gap chases through the wash
    // once a bar — motion without more light. Colour snaps; the dimmer
    // arrives over a beat (`set_class_timing` and the timing below).
    let chorus = |fan_to: &'static str| {
        vec![
            look(wash(), 0.72, "Gold"),
            look(key(), 0.72, "Warm White"),
            look(back(), 0.72, "Amber"),
            look(bars(), 0.65, "Amber"),
            look(movers(), 0.7, "Open White"),
            fan("Vocal", fan_to),
            look(drums(), 0.5, "Warm White"),
            dark(beams()),
            dark(audience()),
            effect("dark chase", Some(wash_lr()), None),
        ]
    };
    let chorus_timing = |c: &mut Cue| {
        c.timing.dimmer_in = Some(1.0);
        c.timing.color = Some(0.0);
    };
    let mut ch1 = chorus("Audience");
    ch1.push(dark(floor()));
    if let Some(c) = a.cue(at("CH 1", 0), "CH 1", 0.25, ch1) {
        chorus_timing(c);
    }

    // ── the one-bar break ────────────────────────────────────────────
    // A breath: the profile's blackout look — the whole stage to zero,
    // the house untouched — then the back wall in congo with lightning
    // in it, and the key just enough to see a face. A negative flash
    // trigger cuts the downbeat. Blackout-safe: the key never goes.
    a.cue(
        at("Break", 0),
        "Break",
        0.0,
        vec![
            look_ref("blackout"),
            dark(drums()),
            dark(floor()),
            dark(audience()),
            look(key(), 0.25, "Warm White"),
            look(back(), 0.72, "Congo"),
            effect("lightning", None, None),
        ],
    );

    // ── second time round, a little more ─────────────────────────────
    if let Some(c) = a.cue(at("VS 2", 0), "VS 2", 4.0, verse(true)) {
        c.timing.ease.position = CurveName::Swing.ease();
    }
    a.cue(
        at("VS 2", 4),
        "· VS 2 lift",
        1.0,
        vec![look(bars(), 0.45, "Magenta")],
    );
    a.cue(at_nth("PRE", 1, 0), "PRE 2", 2.0, pre());
    if let Some(c) = a.cue(last_bar_nth("PRE", 1), "· PRE 2 wipe", 1.0, pre_lift(true)) {
        c.fan = Some(CueFan {
            delay: Fan {
                from: 0.0,
                to: 2.0,
                ..Default::default()
            },
            fade: Fan::default(),
        });
    }
    // CH 2 = CH 1 + one addition: the windmill, and the floor.
    let mut ch2 = chorus("Audience");
    ch2.push(look(floor(), 0.6, "Amber"));
    ch2.push(effect("windmill", None, None));
    if let Some(c) = a.cue(at("CH 2", 0), "CH 2", 0.25, ch2) {
        chorus_timing(c);
    }

    // ── bridge: somewhere else ───────────────────────────────────────
    // The home hue at its deepest: the Congo Split in blocks across the
    // wash, rocking warmer and cooler about itself; the movers on the
    // drummer tracing a figure of eight; firelight on the floor. Slower,
    // wider, and deliberately not warm.
    a.cue(
        at("BR", 0),
        "BR",
        4.0,
        vec![
            split(wash_lr(), 0.45, "Congo Split"),
            effect("hue rock", None, None),
            look(key(), 0.6, "Warm White"),
            look(back(), 0.5, "Congo"),
            look(bars(), 0.4, "Magenta"),
            look(movers(), 0.6, "Congo"),
            aim(movers(), "Drums"),
            effect("figure eight", None, None),
            look(floor(), 0.4, "Magenta"),
            effect("fire flicker", Some(floor()), Some(0.5)),
            dark(beams()),
            dark(audience()),
            dark(drums()),
        ],
    );

    // ── breakdown: strip it back ─────────────────────────────────────
    // The darkest cue since the count-in. A television flickering on
    // the singer's face, a whisper of blue behind washing in and out of
    // saturation, the movers flying out over the house. The lift into
    // the last chorus is a **follow**: it fires two beats before CH 3
    // however long the breakdown is, and carries a position too so the
    // clock lands it in the same place.
    a.cue(
        at("Breakdown", 0),
        "Breakdown",
        4.0,
        vec![
            dark(wash()),
            dark(bars()),
            dark(drums()),
            dark(floor()),
            dark(beams()),
            dark(audience()),
            look(key(), 0.5, "Warm White"),
            effect("tv flicker", None, None),
            look(back(), 0.2, "Deep Blue"),
            effect("saturation breathe", Some(back()), None),
            look(movers(), 0.25, "Indigo"),
            fan("Band", "House"),
            effect("fly out", None, None),
        ],
    );
    let breakdown_beats = a.beats_in("Breakdown");
    let lift_beats = (breakdown_beats - 2.0).max(1.0);
    let lift_bars = (lift_beats / 4.0) as u32;
    let lift_beat = (lift_beats - lift_bars as f32 * 4.0) as f64 + 1.0;
    if let Some(c) = a.cue(
        at_beat("Breakdown", 0, lift_bars, lift_beat),
        "· Breakdown lift",
        0.5,
        vec![
            look(wash(), 0.5, "Pink"),
            look(bars(), 0.4, "Magenta"),
            // Every layer but the key: faces are never chased.
            effect(
                "rig build",
                Some(Selection::Union(vec![wash(), back(), bars()])),
                Some(0.5),
            ),
            look(beams(), 0.4, "Gold"),
            aim(beams(), "Audience"),
            effect("strobe riser", None, Some(0.5)),
        ],
    ) {
        c.trig = Trig::Follow { beats: lift_beats };
    }

    // ── last chorus: the biggest thing in the show ───────────────────
    // Everything, at full — the only cue that is. The chorus look with
    // the beams over the audience, the blinders up, the floor in, a
    // colour wipe sweeping the hot colour across the wash as it lands
    // and a rainbow rolling along the bars for four bars. Then, four
    // bars in, the strobe: random pops on the beams and the blinders
    // chasing for four bars, no more. Then the run-out: eighth-note
    // chases and the windmill at its fastest. The hits here are cutouts,
    // as they should be on a stage this bright.
    let mut ch3 = vec![
        full(wash(), 1.0, "Gold"),
        look(key(), 0.8, "Warm White"),
        full(back(), 1.0, "Amber"),
        full(bars(), 0.9, "Amber"),
        full(movers(), 1.0, "Open White"),
        fan("Vocal", "House"),
        full(beams(), 0.9, "Gold"),
        aim(beams(), "Audience"),
        full(floor(), 0.8, "Amber"),
        full(audience(), 0.6, "Warm White"),
        look(drums(), 0.7, "Warm White"),
        effect("dark chase", Some(wash_lr()), None),
        effect("colour wipe", Some(wash_lr()), None),
        // Two effects share the bars: the strip chase owns intensity —
        // one pixel in four lit for a quarter of the cycle — and the
        // rainbow, filtered to colour, owns the hue. Without the filter
        // the rainbow's dimmer would fight the chase for the same
        // channel.
        param(effect("strip chase", Some(bars()), None), "duty", 0.25),
        filtered(effect("rainbow", Some(bars()), None), AttrFilter::COLOUR),
        effect("audience sweep", None, None),
    ];
    ch3.push(effect("white pop", None, None));
    if let Some(c) = a.cue(at("CH 3", 0), "CH 3", 0.25, ch3) {
        chorus_timing(c);
        // The profile's drop macro on the downbeat — strobe burst,
        // blinders, the movers flying out over the chorus look for four
        // beats, then let go — the same move the DROP key runs, fired
        // by the show. The host reads it from the cue's commands.
        // r[impl profile.macros] - a cue starts one
        c.commands = vec!["macro drop".into()];
    }
    a.cue(
        at("CH 3", 4),
        "· CH 3 strobe",
        0.25,
        vec![
            // Restating the bars ends the rainbow: an absolute colour
            // arriving over a colour effect takes the layer.
            full(bars(), 0.9, "Amber"),
            effect("random strobe", Some(beams()), Some(0.5)),
            effect("blinder chase", None, Some(0.5)),
        ],
    );
    a.cue(
        at("CH 3", 8),
        "· CH 3 drive",
        0.25,
        vec![
            // The CHORUS key's scene — everything up, the `chorus drive`
            // bundle running — under the run-out, where the guide's
            // movement budget has room for its windmill (opening the
            // whole chorus on it would run the movers past 60 % of the
            // song), then this song's gold and amber over its hot.
            look_ref("chorus full"),
            full(wash(), 1.0, "Gold"),
            look(key(), 0.8, "Warm White"),
            full(back(), 1.0, "Amber"),
            full(bars(), 0.9, "Amber"),
            // Restating the beams and blinders ends both strobes.
            full(beams(), 0.9, "Gold"),
            full(audience(), 0.6, "Warm White"),
            // The eighths chase written the routed way: a one-bar loop
            // on the Song master at double — the same half-bar the
            // library authors, said as a scale so the desk's speed
            // routing and the show's spelling agree. A quarter duty
            // makes the point of light a point.
            at_speed(
                param(
                    effect("chase eighths", Some(wash_lr()), Some(1.0)),
                    "duty",
                    0.25,
                ),
                Speed::Scaled {
                    master: "Song".into(),
                    scale: 2.0,
                },
            ),
        ],
    );

    // ── out ──────────────────────────────────────────────────────────
    // Back to the intro's cold. The wash drains light by light and
    // stays dark; the movers lift off the stage and go. Then the end:
    // everything to black over eight beats on an S-curve, and the host
    // is told the show is over.
    a.cue(
        at("Outro", 0),
        "Outro",
        2.0,
        vec![
            split(wash_lr(), 0.4, "Ocean"),
            look(key(), 0.5, "Warm White"),
            look(back(), 0.35, "House Blue"),
            look(bars(), 0.25, "Deep Blue"),
            look(movers(), 0.4, "Cyan"),
            aim(movers(), "Back Wall"),
            dark(beams()),
            dark(audience()),
            dark(drums()),
            dark(floor()),
            effect("drain and hold", Some(wash_lr()), None),
            effect("lift off", None, None),
        ],
    );
    // The hand-authored fade stays beside the profile's `end` macro:
    // the macro is "two beats, then blackout" at the programmer's
    // program time, which is the busk key's end and not this one's —
    // the S-curve over eight beats is the song's. Both run; the macro's
    // blackout lands under a stage already on its way to black, and
    // neither touches the house, restated here so a room that binds it
    // is lit for the walk-off while the stage is not.
    if let Some(c) = a.cue(
        at("Outro", 1),
        "· Outro end",
        8.0,
        vec![
            dark(wash()),
            dark(key()),
            dark(bars()),
            dark(movers()),
            dark(back()),
            look(house(), 0.3, "Warm White"),
        ],
    ) {
        c.timing.ease.intensity = CurveName::SCurve.ease();
        // r[impl profile.macros] - the end macro, from the last cue
        c.commands = vec!["osc /show/end".into(), "macro end".into()];
    }
    // The reset, a bar after black: effects off, stage lit for the
    // changeover. Positioned past the end so the clock never reaches it
    // during the song and GO does.
    a.cue(
        at("Outro", 3),
        "reset",
        2.0,
        vec![
            look_ref("punt"),
            look(key(), 0.6, "Warm White"),
            look(wash(), 0.5, "Open White"),
            look(back(), 0.3, "House Blue"),
            dark(bars()),
            dark(movers()),
            dark(beams()),
            dark(floor()),
            dark(audience()),
            dark(drums()),
        ],
    );

    // Positions are authored per section, not in order, so sort before
    // handing over: `seek` walks the list backwards looking for the last
    // cue at or before a position and needs it ordered.
    let mut list = CueList {
        name: song.name.clone(),
        cues: a.cues,
        triggers: Vec::new(),
        ..Default::default()
    };
    list.sort_by_position();
    // Movers that re-aim are flagged from the recipes once the order is
    // known, so a mover parked dark in the bridge swings before the chorus.
    // r[impl cues.generator-emits-mib]
    ignition_daw::set_mib(&mut list);
    ignition_daw::set_class_timing(&mut list);
    list
}

// ── the charted hits ─────────────────────────────────────────────────
//
// Two things come out of the chart. The **pulse** — kick and snare —
// becomes a looping effect inside each section look, because a backbeat
// is part of what the section *is*. The **hits** — the band landing
// together — become triggers: one-shots the transport fires as it
// crosses them, above the cue player, summed when they coincide.

/// Half the rig's width, in metres, for cutting a figure into zones.
///
/// Zones are cut from this rather than from a fixture query, because a
/// zone has to mean the same thing for a two-hit figure and a six-hit
/// one — "stage left" is a place in the room, not "whichever third of
/// the fixtures matched".
const RIG_HALF_WIDTH: f64 = 5.0;

/// Eighths in a bar — the resolution the chart is written at.
const SLOTS: usize = 8;

/// The `n`th of `count` zones across the rig, as a selection.
///
/// Three hits that are one idea become left, centre, right — a figure
/// travelling across the stage — where three ungrouped hits could only
/// ever be the same fixtures flashing three times. The information that
/// makes the difference is not in the audio; it is in somebody having
/// drawn one long note over the three.
// r[impl song.chart.figure.zones] - nth of count slices, selected by where the light lands at face height
// r[impl profile.areas.not-a-focus-point] - a zone asks which fixtures *cover* a region, via `Where::Covers`
fn zone(n: usize, count: usize) -> Selection {
    if count <= 1 {
        return wash();
    }
    let width = 2.0 * RIG_HALF_WIDTH / count as f64;
    let min_x = -RIG_HALF_WIDTH + width * n as f64;
    Selection::Where {
        of: Box::new(wash()),
        // Where the light *lands*, not where the fixture hangs. A front
        // wash hung over the left of the stage aims at the centre, as
        // almost every front wash in every room does; "the left third of
        // the stage" is a question about coverage.
        filter: Where::Covers {
            min: ignition_proto::Vec3 {
                x: min_x,
                y: -30.0,
                z: 0.0,
            },
            max: ignition_proto::Vec3 {
                x: min_x + width,
                y: 30.0,
                z: 0.0,
            },
            // Face height, because that is the third of the stage an
            // audience sees lit — not the patch of floor in front of it.
            height: 1.7,
        },
    }
}

/// The one-bar pattern a class plays inside a section, as eighth slots.
///
/// Folded across the section's bars and kept only where the hit lands in
/// most of them. That threshold is what separates the groove from the
/// fills: a snare on two and four every bar is the pattern, and the
/// extra snare in the turnaround of the last bar is not.
// r[impl song.chart.pulse] - the one-bar pattern, folded across the section
fn pattern(
    chart: &HitChart,
    song: &SongMap,
    section: &str,
    class: HitClass,
) -> Option<[bool; SLOTS]> {
    let section = song.section(section)?;
    let first = section.start.bar;
    let last = first + section.bars as u32;
    let bars = (last - first).max(1);

    let mut counts = [0usize; SLOTS];
    for hit in chart.hits.iter().filter(|h| h.class == class) {
        if hit.at.bar < first || hit.at.bar >= last {
            continue;
        }
        let slot = ((hit.at.beat - 1.0) * 2.0).round() as usize;
        if let Some(count) = counts.get_mut(slot) {
            *count += 1;
        }
    }

    let mut slots = [false; SLOTS];
    let mut any = false;
    for (slot, count) in counts.iter().enumerate() {
        if *count * 2 >= bars as usize {
            slots[slot] = true;
            any = true;
        }
    }
    any.then_some(slots)
}

/// A repeating one-bar flash on the charted pattern, locked to the song.
// r[impl song.chart.pulse] - one looping effect locked to the `Song` master
// r[impl effects.masters.song]
fn pulse(target: Selection, slots: [bool; SLOTS], depth: f32) -> Recipe {
    let step = |on: bool| {
        Step::new(vec![RecipeApply::Delta(vec![(
            Attribute::Dimmer,
            if on { depth } else { 0.0 },
        )])])
    };
    Recipe {
        target,
        steps: slots.iter().map(|on| step(*on)).collect(),
        timing: Timing {
            speed: Speed::Master("Song".into()),
            measure: 4.0,
            // No spread: a backbeat is the whole rig ticking together.
            phase_spread_deg: 0.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// The pulse recipes for one section — kick on the floor and back
/// (low, warm), snare on the wash. Never both on one role.
// r[impl song.chart.pulse] - lives in the section look; a section with none gets none
fn pulses(chart: &HitChart, song: &SongMap, section: &str) -> Vec<Recipe> {
    let kind = kind_of(section);
    if !matches!(
        kind,
        Kind::Verse | Kind::PreChorus | Kind::Chorus | Kind::Bridge | Kind::Intro
    ) {
        return Vec::new();
    }
    let chorus = kind == Kind::Chorus;
    let snare_depth = if chorus { 0.25 } else { 0.12 };
    let kick_depth = 0.10;

    let mut out = Vec::new();
    if let Some(slots) = pattern(chart, song, section, HitClass::Kick) {
        out.push(pulse(
            Selection::Union(vec![floor(), back()]),
            slots,
            kick_depth,
        ));
    }
    if let Some(slots) = pattern(chart, song, section, HitClass::Snare) {
        out.push(pulse(wash(), slots, snare_depth));
    }
    out
}

/// A bump on a selection, sized by the hit's class.
///
/// White for the band's high hits — the `white pop` shape, a flash
/// through whatever colour the wash is in — so they read through a
/// coloured look; a level lift for the medium ones. One object shared
/// with the operator's flash key, so a charted hit and a hand on the
/// same snare are indistinguishable.
// r[impl effects.bump.one-object]
// r[impl playback.flash-equals-hit]
fn bump(target: Selection, class: HitClass, depth: f32) -> Recipe {
    let kind = if class == HitClass::High || depth >= 0.6 {
        ignition_show::BumpKind::White
    } else {
        ignition_show::BumpKind::Level
    };
    ignition_show::bump::bump(target, kind, depth)
}

/// A relative envelope with the bump's shape, scaled by `depth` —
/// negative for a cut, and past 1.0 for a lift that has to climb out of
/// a cut on the same fixtures.
///
/// Built from a unit bump and scaled here rather than passed through
/// `bump`'s own depth, which clamps to one: a reveal summed with a cut
/// needs the lift to be cut-plus-level, which is more than one.
fn envelope(target: Selection, depth: f32) -> Recipe {
    let mut r = ignition_show::bump::bump(target, ignition_show::BumpKind::Level, 1.0);
    for step in r.steps.iter_mut() {
        for apply in step.apply.iter_mut() {
            if let RecipeApply::Delta(pairs) = apply {
                for (_, v) in pairs.iter_mut() {
                    *v *= depth;
                }
            }
        }
    }
    r
}

/// The triggers for every figure and the hits the guide keeps.
///
/// A figure of two or three moments is a **cutout**: the whole stage is
/// taken away and one zone left standing, one zone per moment, so the
/// stage is carved into halves or thirds. Cutting reads far harder than
/// adding, because the eye reads contrast rather than level. Longer
/// runs are additive bumps travelling across the zones: six cuts in six
/// eighths is a strobe with extra steps.
///
/// A cutout is two triggers at one position — the cut over everything
/// and the lift on the zone — which the bus **sums**: the zone's
/// fixtures take both, and the lift is sized so the sum lands the zone
/// at the hit's level while everything else goes to black.
///
/// Lone hits are thinned to the guide's density — "hit the downbeat,
/// not every hit": one per bar in a chorus, pre or intro, one per two
/// bars in a verse, none in a breakdown until its last bar, and never in
/// a bar a figure already owns. The strongest hit in the window wins,
/// a downbeat over an off-beat.
// r[impl song.chart.hit] - one trigger per kept band hit; pulse classes get none
// r[impl song.chart.figure] - members on one grid position collapse into one moment
// r[impl song.chart.figure.cutout-or-bump]
// r[impl song.chart.accents-are-additive]
// r[impl triggers.shape] - relative, one-shot, named
// r[impl triggers.simultaneous-sum] - authored against the summing rule
// r[impl triggers.hold] - figure moments hold until the next; the last, and lone hits, fall
fn triggers(chart: &HitChart, song: &SongMap) -> Vec<Trigger> {
    let mut out = Vec::new();
    let cut_depth = 0.95;
    // Hits arrive at absolute bars from the chart; written down relative
    // to their section so they move with it, the bar kept beside.
    // r[impl song.relative-position] - triggers too
    let place = |at: Bars| (Position::relative_to(song, at), Some(at));

    let mut figure_bars = std::collections::BTreeSet::new();
    for (index, group) in chart.groups.iter().enumerate() {
        // Hits landing together are one moment: a snare and a crash on
        // the same eighth should light one zone, not two.
        let mut moments: Vec<(Bars, f32)> = Vec::new();
        for hit in chart.members(group) {
            match moments.last_mut() {
                Some((at, level)) if *at == hit.at => *level = level.max(hit.intensity()),
                _ => moments.push((hit.at, hit.intensity())),
            }
        }
        let count = moments.len();
        for (n, (at, level)) in moments.into_iter().enumerate() {
            figure_bars.insert(at.bar);
            let name = format!("fig {index} · {}/{count}", n + 1);
            // Inside a figure each moment holds until the next one moves
            // the shape along; the last moment falls, so the figure
            // ends and the look comes back. A held last moment would
            // leave the stage carved until the next cue, which is a
            // different idea from the one that was drawn.
            let hold = n + 1 < count;
            if (2..=3).contains(&count) {
                let (position, resolved) = place(at);
                out.push(Trigger {
                    at: position.clone(),
                    resolved,
                    recipe: envelope(everything(), -cut_depth),
                    name: format!("{name} cut"),
                    hold,
                });
                out.push(Trigger {
                    at: position,
                    resolved,
                    recipe: envelope(zone(n, count), cut_depth + level.max(0.6)),
                    name,
                    hold,
                });
            } else {
                let (position, resolved) = place(at);
                out.push(Trigger {
                    at: position,
                    resolved,
                    recipe: bump(zone(n, count), HitClass::High, level),
                    name,
                    hold,
                });
            }
        }
    }

    // The soft tiers are the pulse and already have their effect; a
    // trigger as well would flash twice for one hit. The rest are
    // thinned per section to the guide's density.
    let mut candidates: Vec<&ignition_daw::chart::ChartHit> = chart
        .ungrouped()
        .filter(|h| matches!(h.class, HitClass::High | HitClass::Medium))
        .filter(|h| !figure_bars.contains(&h.at.bar))
        .collect();
    candidates.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    for section in &song.sections {
        let kind = kind_of(&section.name);
        let first = section.start.bar;
        let last = first + (section.bars as u32).max(1) - 1;
        // (allowed, per how many bars)
        let per = match kind {
            Kind::Verse | Kind::Outro => 2,
            _ => 1,
        };
        let mut bar = first;
        while bar <= last {
            let span_end = (bar + per - 1).min(last);
            let allowed = match kind {
                Kind::Breakdown if span_end != last => 0,
                // The break's accent is the negative flash on its
                // downbeat (`section_triggers`); the count-in has none.
                Kind::CountIn | Kind::Break => 0,
                _ => 1,
            };
            if allowed > 0 && !(bar..=span_end).any(|b| figure_bars.contains(&b)) {
                let best = candidates
                    .iter()
                    .filter(|h| h.at.bar >= bar && h.at.bar <= span_end)
                    .max_by(|a, b| {
                        let score = |h: &ignition_daw::chart::ChartHit| {
                            (
                                h.class == HitClass::High,
                                h.at.beat == 1.0,
                                (h.intensity() * 100.0) as i32,
                                -(h.at.bar as i32),
                            )
                        };
                        score(a).cmp(&score(b))
                    });
                if let Some(hit) = best {
                    // High hits take the bars with them; the rest stay
                    // on the wash.
                    let target = if hit.class == HitClass::High {
                        Selection::Union(vec![wash(), bars()])
                    } else {
                        wash()
                    };
                    let (position, resolved) = place(hit.at);
                    out.push(Trigger {
                        at: position,
                        resolved,
                        recipe: bump(target, hit.class, hit.intensity()),
                        name: format!("{} {}.{:.2}", hit.class.label(), hit.at.bar, hit.at.beat),
                        // A lone hit is a moment, not a state: it snaps
                        // and falls. Holding is for figures, where the
                        // next moment is what releases the last.
                        hold: false,
                    });
                }
            }
            bar += per;
        }
    }
    out.sort_by(|a, b| {
        a.resolved
            .partial_cmp(&b.resolved)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// The accents the *arrangement* fires, chart or no chart: the negative
/// flash on the break's downbeat — the whole stage cut for an instant
/// and straight back, the accent for a stage a bump has nowhere to go
/// on.
fn section_triggers(song: &SongMap) -> Vec<Trigger> {
    let mut out = Vec::new();
    if song.section("Break").is_some() {
        let mut flash = ignition_effects::effects::library()["negative flash"].clone();
        flash.target = everything();
        out.push(Trigger {
            at: at("Break", 0),
            resolved: at("Break", 0).resolve(song),
            recipe: flash,
            name: "Break · negative flash".into(),
            hold: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_daw_proto::{Section, TempoMap};

    /// An arrangement with the real song's shape, at a chosen verse length.
    fn arrangement(verse: f64) -> SongMap {
        let mut sections = Vec::new();
        let mut bar = 1u32;
        for (name, bars) in [
            ("Count-In", 2.0),
            ("IN A", 4.0),
            ("IN B", 4.0),
            ("VS 1", verse),
            ("PRE", 4.0),
            ("CH 1", 8.0),
            ("Break", 1.0),
            ("VS 2", verse),
            ("PRE", 4.0),
            ("CH 2", 8.0),
            ("BR", 5.0),
            ("Breakdown", 4.0),
            ("CH 3", 12.0),
            ("Outro", 2.0),
        ] {
            sections.push(Section {
                name: name.into(),
                start: Bars::bar(bar),
                bars,
            });
            bar += bars as u32;
        }
        SongMap {
            name: "Bye Bye Bye".into(),
            tempo: TempoMap::constant(86.28, Default::default()),
            sections,
        }
    }

    /// A figure's moments each become one trigger against one zone, and
    /// a hit's class chooses how deep the bump goes.
    ///
    /// The chart is the authority and a trigger list is its rendering,
    /// so this is the join that has to hold: charted moments in, named
    /// triggers out, one per moment, positioned where the chart put
    /// them. Two hits on the same eighth are one moment — a snare and a
    /// crash together should light one zone, not two.
    ///
    /// r[verify triggers.from-the-chart]
    #[test]
    fn a_figure_renders_one_trigger_per_moment() {
        use ignition_daw::chart::{ChartHit, Group as ChartGroup, HitChart};

        let song = arrangement(8.0);
        // Four moments, one of them struck twice at the same instant.
        let at = |bar: u32, beat: f64| Bars::new(bar, beat);
        let hits = vec![
            ChartHit {
                at: at(10, 1.0),
                class: HitClass::High,
                velocity: 100,
                group: Some(0),
            },
            ChartHit {
                at: at(10, 1.0),
                class: HitClass::Low,
                velocity: 90,
                group: Some(0),
            },
            ChartHit {
                at: at(10, 2.0),
                class: HitClass::High,
                velocity: 100,
                group: Some(0),
            },
            ChartHit {
                at: at(10, 3.0),
                class: HitClass::High,
                velocity: 100,
                group: Some(0),
            },
            ChartHit {
                at: at(10, 4.0),
                class: HitClass::High,
                velocity: 100,
                group: Some(0),
            },
        ];
        let chart = HitChart {
            groups: vec![ChartGroup {
                start: at(10, 1.0),
                end: at(10, 4.0),
                members: (0..hits.len()).collect(),
            }],
            hits,
        };

        let fired = triggers(&chart, &song);
        assert!(!fired.is_empty(), "a charted figure rendered nothing");

        // Five hits, four moments — the doubled instant is one.
        let mut moments: Vec<Bars> = fired.iter().filter_map(|t| t.bars()).collect();
        moments.sort_by(|a, b| a.partial_cmp(b).expect("charted bars compare"));
        moments.dedup();
        assert_eq!(moments.len(), 4, "moments: {moments:?}");

        // Every trigger is named after its figure and placed where the
        // chart put it, and none is left ringing past the figure: the
        // last moment falls so the look comes back.
        for trigger in &fired {
            assert!(
                trigger.name.starts_with("fig "),
                "a figure's trigger is not named for it: {}",
                trigger.name
            );
            assert!(trigger.recipe.timing.once, "a hit that would never stop");
        }
        let last = fired
            .iter()
            .filter(|t| t.bars() == Some(at(10, 4.0)))
            .collect::<Vec<_>>();
        assert!(!last.is_empty(), "the closing moment did not render");
        assert!(
            last.iter().all(|t| !t.hold),
            "the figure's last moment holds, so the stage stays carved"
        );
    }

    /// Where a chart exists it is the only thing consulted, and where
    /// none does the generator does not go looking for onsets.
    ///
    /// The detector finds a thousand onsets in a three-minute song and
    /// is right about nearly all of them, which is exactly the problem:
    /// a hi-hat is a real onset and wants no cue. Which handful of hits
    /// carries the song is a musical judgement made in a MIDI editor
    /// with the track playing. So detection is a draft a person reads,
    /// never an input to this — and the check is both halves of that:
    /// an empty chart yields a show with no charted hits in it, rather
    /// than one quietly filled in from the audio.
    ///
    /// r[verify song.hits.detection-is-a-draft]
    #[test]
    fn the_chart_is_the_only_source_of_hits_and_nothing_fills_in_for_it() {
        use ignition_daw::chart::{ChartHit, Group as ChartGroup, HitChart};

        let song = arrangement(8.0);

        // No chart: an ordinary show, and not a hit in it beyond the
        // section markers the arrangement itself provides.
        let bare = build(&song, None);
        assert!(
            !bare.cues.is_empty(),
            "a show with no chart is still a show"
        );
        let section_triggers = bare.triggers.len();
        assert!(
            !bare.triggers.iter().any(|t| t.name.starts_with("fig ")),
            "a figure appeared with no chart to have charted it: {:?}",
            bare.triggers.iter().map(|t| &t.name).collect::<Vec<_>>()
        );

        // The same song with one charted figure: the hits that appear
        // are that figure's, and the cue list is otherwise the same.
        let at = |bar: u32, beat: f64| Bars::new(bar, beat);
        let hits: Vec<ChartHit> = [1.0, 2.0, 3.0]
            .into_iter()
            .map(|beat| ChartHit {
                at: at(10, beat),
                class: HitClass::High,
                velocity: 100,
                group: Some(0),
            })
            .collect();
        let chart = HitChart {
            groups: vec![ChartGroup {
                start: at(10, 1.0),
                end: at(10, 3.0),
                members: (0..hits.len()).collect(),
            }],
            hits,
        };
        let charted = build(&song, Some(&chart));

        assert_eq!(
            charted.cues.len(),
            bare.cues.len(),
            "the chart changed the section list, which is the arrangement's business"
        );
        let figures: Vec<&str> = charted
            .triggers
            .iter()
            .filter(|t| t.name.starts_with("fig "))
            .map(|t| t.name.as_str())
            .collect();
        assert!(!figures.is_empty(), "the charted figure rendered nothing");
        let mut moments: Vec<Bars> = charted
            .triggers
            .iter()
            .filter(|t| t.name.starts_with("fig "))
            .filter_map(|t| t.bars())
            .collect();
        moments.sort_by(|a, b| a.partial_cmp(b).expect("charted bars compare"));
        moments.dedup();
        assert_eq!(moments.len(), 3, "{moments:?}");
        assert_eq!(
            charted.triggers.len(),
            section_triggers + figures.len(),
            "the chart added something that is not one of its own hits"
        );

        // And every one of them sits where the chart put it — not
        // where an onset happened to be.
        for trigger in charted
            .triggers
            .iter()
            .filter(|t| t.name.starts_with("fig "))
        {
            let at = trigger.bars().expect("a charted hit has a position");
            assert_eq!(at.bar, 10, "{}: {at:?}", trigger.name);
            assert!(
                (1.0..=3.0).contains(&at.beat),
                "{}: {at:?} is outside the charted figure",
                trigger.name
            );
        }
    }

    /// A louder class digs a deeper bump.
    ///
    /// Class is what says how big a hit is — velocity is charted but
    /// deliberately not applied (`r[song.chart.class-is-intensity]`), so
    /// the depth has to move with the class and nothing else.
    ///
    /// r[verify triggers.from-the-chart]
    #[test]
    fn a_bigger_class_digs_a_deeper_bump() {
        assert!(
            HitClass::High.weight() > HitClass::Kick.weight(),
            "class weights do not order the tiers"
        );

        let depth = |class: HitClass| {
            let recipe = bump(wash(), class, class.weight());
            recipe
                .steps
                .iter()
                .flat_map(|s| &s.apply)
                .filter_map(|a| match a {
                    RecipeApply::Delta(pairs) => pairs
                        .iter()
                        .find(|(attr, _)| *attr == Attribute::Dimmer)
                        .map(|(_, v)| v.abs()),
                    _ => None,
                })
                .fold(0.0f32, f32::max)
        };

        let soft = depth(HitClass::Kick);
        let hard = depth(HitClass::High);
        assert!(
            hard > soft,
            "a band hit is no deeper than a kick: {hard} vs {soft}"
        );
    }

    /// Band hits become one-shot triggers; the pulse classes get none.
    ///
    /// A note must not flash twice. `Kick` and `Snare` are the pattern
    /// each section plays and are rendered as a running effect in the
    /// look; if they also produced triggers, every backbeat would fire
    /// a bump on top of the pulse already playing it. That half of
    /// `r[song.chart.hit]` is what this pins.
    ///
    /// The other half does not hold as written, and the test says so
    /// rather than asserting the code back at itself. The rule reads
    /// "`Low`, `Medium` and `High` hits MUST become hits ... one per
    /// charted note"; the renderer keeps only `High` and `Medium`, and
    /// thins even those to the guide's density — at most one a bar, one
    /// per two bars in a verse. Both are deliberate (see the comment
    /// above `candidates`), so it is the spec that is behind the code,
    /// not the code that is broken. Worth settling before either is
    /// leaned on.
    ///
    /// r[verify song.chart.hit] - the pulse classes never double-fire
    #[test]
    fn band_hits_become_triggers_and_pulse_classes_do_not() {
        use ignition_daw::chart::{ChartHit, HitChart};

        let song = arrangement(8.0);
        let hit = |bar: u32, class: HitClass| ChartHit {
            at: Bars::new(bar, 1.0),
            class,
            velocity: 100,
            group: None,
        };
        let render = |hits: Vec<ChartHit>| -> Vec<u32> {
            let chart = HitChart {
                hits,
                groups: Vec::new(),
            };
            triggers(&chart, &song)
                .iter()
                .filter_map(|t| t.bars())
                .map(|b| b.bar)
                .collect()
        };

        // One tier at a time, and far apart: the renderer thins hits to
        // the guide's density, so two in one span would prove only that
        // the louder of them won.
        for (bar, class) in [(13, HitClass::Medium), (25, HitClass::High)] {
            let bars = render(vec![hit(bar, class)]);
            assert!(
                bars.contains(&bar),
                "the {class:?} hit in bar {bar} produced no trigger: {bars:?}"
            );
        }

        // The pulse classes, on their own, produce nothing at all.
        let bars = render(vec![hit(10, HitClass::Kick), hit(11, HitClass::Snare)]);
        for pulsed in [10, 11] {
            assert!(
                !bars.contains(&pulsed),
                "a pulse class also fired a trigger in bar {pulsed}, so the note flashes twice"
            );
        }
    }

    /// The pulse classes become a looping effect in the section's look,
    /// and a section whose chart carries none gets none.
    ///
    /// A snare two to the bar for fifty-six bars is three hundred cues
    /// saying "flash on two and four"; on a console it is one running
    /// effect.
    ///
    /// r[verify song.chart.pulse]
    #[test]
    fn the_pulse_classes_become_one_looping_effect_per_section() {
        use ignition_daw::chart::{ChartHit, HitChart};

        let song = arrangement(8.0);
        // A backbeat through the first chorus: bars 23..31, on two and four.
        let mut hits = Vec::new();
        for bar in 23..31 {
            for beat in [2.0, 4.0] {
                hits.push(ChartHit {
                    at: Bars::new(bar, beat),
                    class: HitClass::Snare,
                    velocity: 100,
                    group: None,
                });
            }
        }
        let chart = HitChart {
            hits,
            groups: Vec::new(),
        };

        let chorus = pulses(&chart, &song, "CH 1");
        assert!(!chorus.is_empty(), "a charted backbeat produced no pulse");
        for recipe in &chorus {
            assert!(
                !recipe.timing.once,
                "a pulse that does not loop is a one-shot, which is the other thing"
            );
        }

        // The verse before it has no charted hits of its own.
        let verse = pulses(&chart, &song, "VS 1");
        assert!(
            verse.is_empty(),
            "a section with nothing charted was given a pulse anyway"
        );
    }

    /// Zones are equal slices of the *room's* width, addressed by where
    /// the light lands.
    ///
    /// Cut from the room rather than from whichever fixtures matched, so
    /// "stage left" is the same place for a two-moment figure and a
    /// six-moment one — and selected by coverage, because a front wash
    /// hung over the left of the stage aims at the centre.
    ///
    /// r[verify song.chart.figure.zones]
    #[test]
    fn zones_are_equal_slices_of_the_room_selected_by_coverage() {
        let bounds = |selection: &Selection| match selection {
            Selection::Where {
                filter: Where::Covers { min, max, .. },
                ..
            } => (min.x, max.x),
            other => panic!("a zone is not a coverage filter: {other:?}"),
        };

        // Three zones tile the width, left to right, without gaps.
        let (a_min, a_max) = bounds(&zone(0, 3));
        let (b_min, b_max) = bounds(&zone(1, 3));
        let (c_min, c_max) = bounds(&zone(2, 3));
        assert!((a_max - b_min).abs() < 1e-9, "a gap between zones 0 and 1");
        assert!((b_max - c_min).abs() < 1e-9, "a gap between zones 1 and 2");
        assert!(
            a_min < b_min && b_min < c_min,
            "zones are not left to right"
        );
        assert!(
            (c_max - -a_min).abs() < 1e-9,
            "the zones are not centred on the room"
        );

        // The leftmost zone starts at the same edge however many there
        // are: the slices come from the room, not from the figure.
        let (two_min, _) = bounds(&zone(0, 2));
        let (six_min, _) = bounds(&zone(0, 6));
        assert!(
            (two_min - six_min).abs() < 1e-9,
            "the left edge moved with the moment count: {two_min} vs {six_min}"
        );

        // One moment addresses the whole wash rather than a third of it.
        assert!(
            matches!(
                zone(0, 1),
                Selection::Union(_) | Selection::Role(_) | Selection::Group(_)
            ),
            "a single-moment figure was given a slice"
        );
    }

    /// The real project, when it is on this machine.
    fn real() -> Option<(String, SongMap)> {
        let path = concat!(env!("HOME"), "/Downloads/Bye Bye Bye/Bye Bye Bye.RPP");
        std::path::Path::new(path).exists().then(|| {
            (
                path.to_string(),
                ignition_daw::load(path).expect("the project parses"),
            )
        })
    }

    fn profile_splits() -> Vec<ColorSplit> {
        splits(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../data/profiles/ignition.ig-profile"
        )))
    }

    fn cue_at(list: &CueList, name: &str) -> Bars {
        list.cues
            .iter()
            .find(|c| c.name == name)
            .unwrap_or_else(|| panic!("no cue {name:?}"))
            .position()
            .unwrap()
    }

    /// r[verify song.relative-position.duplicate-names]
    #[test]
    fn the_second_pre_is_placed_by_ordinal_not_by_counting_into_the_verse() {
        let song = arrangement(8.0);
        let list = author(&song);
        assert_eq!(cue_at(&list, "PRE"), Bars::bar(19));
        assert_eq!(cue_at(&list, "PRE 2"), Bars::bar(40));
        let pre2 = list.cues.iter().find(|c| c.name == "PRE 2").unwrap();
        assert_eq!(pre2.at, Some(Position::nth("PRE", 1, 0)));
        assert_eq!(cue_at(&list, "· PRE 2 wipe"), Bars::bar(43));
        // Lengthen the verses: the second PRE follows its own section,
        // which "8 bars into VS 2" would not have.
        let song = arrangement(12.0);
        let list = author(&song);
        assert_eq!(cue_at(&list, "PRE 2"), Bars::bar(48));
    }

    /// r[verify cues.generator-emits-mib]
    /// r[verify cues.mib.preference]
    /// r[verify cues.timing.per-attribute]
    #[test]
    fn every_cue_that_reaims_the_movers_asks_for_mib_and_no_other_does() {
        use ignition_show::cue::{Mib, MibMode};
        let list = author(&arrangement(8.0));
        let mut flagged = 0;
        for (i, cue) in list.cues.iter().enumerate() {
            if ignition_daw::mib::reaims(&list, i) {
                flagged += 1;
                assert_ne!(cue.mib.mode, MibMode::None, "{}", cue.name);
                assert_eq!(cue.mib.fade_beats, 2.0, "{}", cue.name);
                assert_eq!(
                    cue.mib.preference,
                    if cue.block { 80 } else { 30 },
                    "{}",
                    cue.name
                );
            } else {
                assert_eq!(cue.mib, Mib::default(), "{} moves no mover", cue.name);
            }
        }
        assert!(flagged > 0, "the show re-aims its movers somewhere");
        let ch = list.cues.iter().find(|c| c.name == "CH 1").unwrap();
        assert_eq!(ch.timing.color, Some(0.0));
        assert_eq!(ch.timing.dimmer_in, Some(1.0));
        let vs = list.cues.iter().find(|c| c.name == "VS 1").unwrap();
        assert_eq!(vs.timing.position, Some(4.0));
        assert_eq!(vs.timing.ease.position, CurveName::Swing.ease());
        let pre = list.cues.iter().find(|c| c.name == "PRE").unwrap();
        assert_eq!(pre.mib.mode, MibMode::Early);
        assert_eq!(pre.mib.preference, 80);
    }

    /// r[verify song.camera-cuts] - the cut is in the cues' commands, by section, and only for the setup's cameras
    /// r[verify viz.camera-cuts]
    #[test]
    fn the_camera_cut_is_written_into_the_cues_for_the_setup() {
        let song = arrangement(8.0);
        let mut eight = author(&song);
        eight.resolve_positions(&song);
        let before = eight.cues.len();
        camera_cuts(&mut eight, &song, "eight").unwrap();
        let commands = |list: &CueList, name: &str| -> Vec<String> {
            list.cues
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.commands.clone())
                .unwrap_or_default()
        };
        assert_eq!(commands(&eight, "VS 1"), ["camera Singer in 2"]);
        assert_eq!(commands(&eight, "PRE"), ["camera Side stage in 1"]);
        assert_eq!(commands(&eight, "· PRE wipe"), ["camera Guitar"]);
        assert_eq!(
            commands(&eight, "CH 1"),
            ["camera Wide", "camera Super wide after 16"]
        );
        assert_eq!(commands(&eight, "Break"), ["camera Drums"]);
        assert_eq!(commands(&eight, "Breakdown"), ["camera Keys in 4"]);
        assert_eq!(commands(&eight, "· Breakdown lift"), ["camera Bass"]);
        assert_eq!(commands(&eight, "Outro"), ["camera Flat front in 2"]);
        assert_eq!(commands(&eight, "reset"), ["camera Bird's eye"]);
        // The drop macro is kept beside the cut.
        assert_eq!(
            commands(&eight, "CH 3"),
            ["macro drop", "camera Wide", "camera Super wide after 16"]
        );
        // No chart, so no punch-in cues were added.
        assert_eq!(eight.cues.len(), before);
        // Re-running replaces rather than doubles.
        camera_cuts(&mut eight, &song, "eight").unwrap();
        assert_eq!(commands(&eight, "VS 1"), ["camera Singer in 2"]);

        // Two cameras: only Wide and Singer are ever named.
        let mut two = author(&song);
        two.resolve_positions(&song);
        camera_cuts(&mut two, &song, "two").unwrap();
        assert_eq!(commands(&two, "VS 1"), ["camera Singer in 2"]);
        assert_eq!(commands(&two, "CH 1"), ["camera Wide"]);
        assert!(commands(&two, "Break").is_empty());
        for cue in &two.cues {
            for c in cue.commands.iter().filter(|c| c.starts_with("camera ")) {
                assert!(
                    c.contains("Wide") || c.contains("Singer"),
                    "{}: {c}",
                    cue.name
                );
            }
        }
        assert!(camera_cuts(&mut two, &song, "twelve").is_err());
    }

    /// r[verify song.camera-cuts] - figures and high hits punch in to the drums, never on a section downbeat
    #[test]
    fn figures_and_high_hits_punch_in_to_the_drum_cam() {
        let Some((path, song)) = real() else { return };
        let chart = ignition_daw::chart::read(&path, &song).unwrap();
        if chart.is_empty() {
            return;
        }
        let mut list = build(&song, Some(&chart));
        camera_cuts(&mut list, &song, "eight").unwrap();
        let punches: Vec<&Cue> = list
            .cues
            .iter()
            .filter(|c| c.name.ends_with("drum cam"))
            .collect();
        assert!(punches.iter().any(|c| c.name.starts_with("· fig ")));
        assert!(punches.iter().any(|c| c.name.starts_with("· hit ")));
        let downbeats: Vec<Bars> = list
            .cues
            .iter()
            .filter(|c| c.block)
            .filter_map(|c| c.resolved)
            .collect();
        for p in &punches {
            assert!(
                !p.block && p.recipes.is_empty() && p.values.is_empty(),
                "{}",
                p.name
            );
            assert_eq!(p.commands.len(), 1);
            assert!(
                p.commands[0].starts_with("camera Drums for "),
                "{}",
                p.commands[0]
            );
            assert!(
                !downbeats.contains(&p.resolved.unwrap()),
                "{} sits on a downbeat",
                p.name
            );
        }
        // One cue per moment: a punch on a moment that has a cue rides
        // that cue's commands, after its own cut, so no punch cue shares
        // a position with another cue.
        for p in &punches {
            let at_same = list
                .cues
                .iter()
                .filter(|c| c.resolved == p.resolved)
                .count();
            assert_eq!(at_same, 1, "{} shares its moment with another cue", p.name);
        }
        let wipe = list.cues.iter().find(|c| c.name == "· PRE wipe").unwrap();
        assert_eq!(wipe.commands, ["camera Guitar", "camera Drums for 2"]);
    }

    /// r[verify song.relative-position]
    /// r[verify song.relative-position.resolved-on-load]
    #[test]
    fn the_file_repositions_itself_against_a_new_arrangement() {
        let list = author(&arrangement(8.0));
        assert_eq!(cue_at(&list, "· Breakdown lift"), Bars::new(60, 3.0));
        // What a loader does: the file round-trips through JSON with its
        // relative positions, and resolves against this week's map.
        let json = serde_json::to_string(&list).unwrap();
        let mut list: CueList = serde_json::from_str(&json).unwrap();
        let unresolved = list.resolve_positions(&arrangement(6.0));
        assert!(unresolved.is_empty(), "{unresolved:?}");
        // Every cue after the first verse moved up two bars per verse.
        assert_eq!(cue_at(&list, "CH 1"), Bars::bar(21));
        assert_eq!(cue_at(&list, "PRE 2"), Bars::bar(36));
        assert_eq!(cue_at(&list, "· Breakdown lift"), Bars::new(56, 3.0));
        let lift = list
            .cues
            .iter()
            .find(|c| c.name == "· Breakdown lift")
            .unwrap();
        assert_eq!(lift.trig, Trig::Follow { beats: 14.0 });
        assert!(!lift.block);
    }

    /// The library effects and bundles go into the file by name, never
    /// as copies; the profile's tricks are named too.
    /// r[verify effects.library.by-name]
    /// r[verify effects.bundle]
    #[test]
    fn library_effects_are_written_as_references() {
        let list = author(&arrangement(8.0));
        let named: Vec<&str> = list
            .cues
            .iter()
            .flat_map(|c| &c.recipes)
            .filter_map(|r| match r {
                RecipeRef::Named { effect, .. } => Some(effect.as_str()),
                RecipeRef::Bundle { bundle, .. } => Some(bundle.as_str()),
                // The `verse bed` bundle arrives through the look of
                // the same name now.
                RecipeRef::Look { look } => Some(look.as_str()),
                _ => None,
            })
            .collect();
        for name in [
            "circle breathe",
            "verse bed",
            "room circle",
            "tilt fan",
            "candle",
            "dark chase",
            "windmill",
            "figure eight",
            "fire flicker",
            "hue rock",
            "tv flicker",
            "fly out",
            "saturation breathe",
            "rig build",
            "strobe riser",
            "random strobe",
            "blinder chase",
            "rainbow",
            "colour wipe",
            "drain and hold",
            "lift off",
            "lightning",
            "white pop",
            "build",
        ] {
            assert!(named.contains(&name), "{name} missing from {named:?}");
        }
        let library = ignition_effects::effects::library();
        let bundles = ignition_effects::effects::bundles();
        let looks = ignition_playback::macros::looks();
        for name in &named {
            assert!(
                library.contains_key(*name)
                    || bundles.contains_key(*name)
                    || looks.contains_key(*name),
                "{name} is not in the library"
            );
        }
        let json = serde_json::to_string(&list).unwrap();
        assert!(json.contains(r#""effect":"circle breathe""#));
        assert!(json.contains(r#""tricks_ref":"paired odds""#));
        assert!(json.contains(r#""Split":"Ocean""#));
        assert!(json.contains(r#""OnAxis":["Z",{"Invert":"Pan"}]"#));
        assert!(json.contains(r#""stack":true"#));
        assert!(json.contains(r#""Canvas""#));
        assert!(json.contains(r#""FocusKeyframes""#));
        assert!(json.contains(r#""commands":["osc /show/end","macro end"]"#));
        assert!(json.contains(r#""commands":["macro drop"]"#));
        // The seven busking features, as the file spells them.
        assert!(json.contains(r#""look":"verse bed""#));
        assert!(json.contains(r#""look":"chorus full""#));
        assert!(json.contains(r#""look":"blackout""#));
        assert!(json.contains(r#""look":"punt""#));
        assert!(json.contains(r#""params":{"depth":0.5}"#));
        assert!(json.contains(r#""params":{"duty":0.25}"#));
        assert!(json.contains(r#""params":{"bars":1.0,"duty":0.25}"#));
        assert!(json.contains(
            r#""filter":{"intensity":false,"colour":true,"position":false,"beam":false}"#
        ));
        assert!(json.contains(r#""speed":{"Scaled":{"master":"Song","scale":2.0}}"#));
        assert!(json.contains(r#""Role":"House Lights""#));
        let wipe = list.cues.iter().find(|c| c.name == "· PRE wipe").unwrap();
        assert_eq!(wipe.fan.map(|f| f.delay.to), Some(2.0));
    }

    /// The show passes every rule of the design guide the lint checks,
    /// on the synthetic arrangement and — when the project is on this
    /// machine — on the real one with its chart and triggers.
    #[test]
    fn the_show_passes_the_design_lint() {
        let song = arrangement(8.0);
        let list = build(&song, None);
        let findings = ignition_daw::lint::lint(&list, &song, &profile_splits());
        assert!(
            findings.is_empty(),
            "{}",
            findings
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
        if let Some((path, song)) = real() {
            let chart = ignition_daw::chart::read(&path, &song).unwrap();
            let list = build(&song, (!chart.is_empty()).then_some(&chart));
            let findings = ignition_daw::lint::lint(&list, &song, &profile_splits());
            assert!(
                findings.is_empty(),
                "{}",
                findings
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            assert!(list.triggers.iter().any(|t| t.name == "fig 0 · 1/3 cut"));
            assert!(
                list.triggers
                    .iter()
                    .any(|t| t.name == "Break · negative flash")
            );
        }
    }

    #[test]
    fn options_parse_and_slug_names_the_sidecars() {
        let o = Options::parse(
            [
                "song.RPP",
                "--merge",
                "old.json",
                "--no-sidecars",
                "--lint",
                "--cameras",
                "eight",
            ]
            .map(String::from)
            .into_iter(),
        )
        .unwrap();
        assert_eq!(o.project, "song.RPP");
        assert_eq!(o.merge.as_deref(), Some(std::path::Path::new("old.json")));
        assert_eq!(o.cameras.as_deref(), Some("eight"));
        assert!(o.no_sidecars);
        assert!(o.lint);
        assert!(Options::parse(["--merge"].map(String::from).into_iter()).is_err());
        assert_eq!(slug("Bye Bye Bye"), "bye-bye-bye");
        assert_eq!(slug("I Want It That Way (Live)"), "i-want-it-that-way-live");
    }
}
