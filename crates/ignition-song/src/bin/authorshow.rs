//! The *Bye Bye Bye* light show, authored against the song's own map.
//!
//! ```bash
//! cargo run -p ignition-song --bin authorshow -- \
//!     "Bye Bye Bye.RPP" > data/songs/bye-bye-bye.json
//! ```
//!
//! ```text
//! authorshow <project.RPP> [--merge <existing.json>] [--edits <edits.json>]
//!            [--sidecar-dir <dir>] [--no-sidecars]
//! ```
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
//! `ignition_song::reposition` and the show follows the arrangement it
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
//! everything the song fires on its own: every charted band hit and
//! every figure. The first version of this file folded the hits into the
//! cue list and the list became a hundred and five entries nobody could
//! read; the hits are events, not looks, and they live apart.
//!
//! The design leans on what the engine can now say: colours **spread**
//! across an ordered selection, movers **fanned** between two focus
//! roles, the shipped effect library, and a cutout that carves the stage
//! into thirds. Every selection is a role, so this plays in any room
//! that implements the default profile.

use ignition_core::music::Position;
use ignition_core::preset::Ref;
use ignition_core::recipe::Distribute;
use ignition_core::selection::{Axis, Dir, Order, Where};
use ignition_core::{
    Attribute, Bars, Cue, CueList, Play, Recipe, RecipeApply, RecipeRef, Selection, SongMap, Speed,
    Step, Timing, Trick, Trigger, Waveform,
};
use ignition_song::chart::{HitChart, HitClass};

// r[impl song.chart] - the chart is re-read from the project on every run
// r[impl cues.sorted-by-position]
// r[impl song.hits.detection-is-a-draft] - only the chart is consulted; detection is never read here
// r[impl triggers.from-the-chart]
fn main() -> anyhow::Result<()> {
    let opts = Options::parse(std::env::args().skip(1))?;
    let song = ignition_song::load(&opts.project)?;
    let mut list = author(&song);

    // The chart is read from the project's own HITS track, so there is
    // nothing to pass and nothing to keep in sync: edit the MIDI in
    // REAPER, re-run this, and the show follows. An absent chart is a
    // show with no hits, which is still a show.
    let chart = ignition_song::chart::read(&opts.project, &song)?;
    if !chart.is_empty() {
        // Pulse first: it belongs *in* the section look, not beside it.
        // A blocking section cue is a complete statement, so a running
        // flash has to be one of its recipes or the next section would
        // cancel it — which is also what makes the pattern change at a
        // section boundary rather than run through the whole song.
        let mut pulsed = 0usize;
        for cue in list.cues.iter_mut().filter(|c| c.block) {
            let recipes = pulses(&chart, &song, &cue.name);
            pulsed += recipes.len();
            cue.recipes.extend(recipes.into_iter().map(RecipeRef::from));
        }
        eprintln!("{pulsed} pulse effects across the section looks");

        list.triggers = triggers(&chart, &song);
        eprintln!(
            "{} triggers from {} charted hits in {} figures; {} cues",
            list.triggers.len(),
            chart.hits.len(),
            chart.groups.len(),
            list.cues.len()
        );
    }
    // A person's edits, laid over the draft.
    // r[impl song.generate.is-a-draft] - re-derivable without destroying edits
    let slug = slug(&song.name);
    let edits_path = opts
        .edits
        .clone()
        .unwrap_or_else(|| opts.sidecar_dir.join(format!("{slug}.edits.json")));
    if edits_path.exists() {
        let edits = ignition_song::Edits::load(&edits_path)?;
        let existing = match &opts.merge {
            Some(path) => Some(serde_json::from_str::<CueList>(&std::fs::read_to_string(
                path,
            )?)?),
            None => None,
        };
        let merged = ignition_song::merge(&mut list, existing.as_ref(), &edits);
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
    let document = ignition_core::show_file::ShowDocument::new(
        list,
        "Ignition",
        ignition_core::show_file::SongBinding {
            project: opts.project.clone(),
            name: song.name.clone(),
            ..Default::default()
        },
    );
    println!("{}", serde_json::to_string_pretty(&document)?);
    Ok(())
}

/// The command line.
struct Options {
    project: String,
    merge: Option<std::path::PathBuf>,
    edits: Option<std::path::PathBuf>,
    sidecar_dir: std::path::PathBuf,
    no_sidecars: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> anyhow::Result<Self> {
        let usage = "usage: authorshow <project file> [--merge <existing.json>] \
                     [--edits <edits.json>] [--sidecar-dir <dir>] [--no-sidecars]";
        let mut project = None;
        let mut merge = None;
        let mut edits = None;
        let mut sidecar_dir = std::path::PathBuf::from("data/songs");
        let mut no_sidecars = false;
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
                "--no-sidecars" => no_sidecars = true,
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

/// The wash, ordered outward from the front of the stage.
///
/// A distance ordering from a point in the room, which stays meaningful
/// at any venue because every venue's coordinates mean metres from its
/// own stage — see `r[focus.stage-space]`.
fn wash_out() -> Selection {
    Selection::Order {
        of: Box::new(wash()),
        by: Order::Distance {
            from: ignition_core::Vec3 {
                x: 0.0,
                y: -3.0,
                z: 2.7,
            },
            dir: Dir::Asc,
        },
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
/// The drum special. Optional at most venues, and the show runs without
/// it — a recipe covering nothing is not an error.
fn drums() -> Selection {
    Selection::Role("Drums".into())
}
/// The floor package, where a room has one.
fn floor() -> Selection {
    Selection::Role("Floor".into())
}

/// Every lighting layer at once — what a cutout takes away.
///
/// The cut has to take the *whole stage*, not the wash. Cutting only the
/// wash left the key light and the movers untouched, so the singer
/// stayed lit through every hit and the reveal read as nothing changing.
fn everything() -> Selection {
    Selection::Union(vec![
        wash(),
        key(),
        back(),
        bars(),
        movers(),
        drums(),
        floor(),
    ])
}

// ── recipe shorthands ────────────────────────────────────────────────

/// The brightest a section look may sit.
///
/// Headroom, and it is the difference between a chorus that punches and
/// one that sits there. Hits are **additive**: `+0.85` on a fixture
/// already at 1.0 has nowhere to go, so it clamps and nothing visibly
/// happens — which is exactly what a chorus authored at full does to
/// every hit in it. The look has to leave room for the thing that lands
/// on top of it.
const LOOK_CEILING: f32 = 0.72;

fn look(target: Selection, level: f32, colour: &str) -> RecipeRef {
    let mut r = Recipe::new(target, RecipeApply::Dimmer(level.min(LOOK_CEILING)));
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
        tricks: Vec::new(),
        stack: false,
        ..Default::default()
    }
    .into()
}

/// A hard chase with a narrow lit step — the shape `Play::Negative`
/// turns into a dark gap travelling through a lit rig.
fn gap_chase(target: Selection, bars: f32, play: Play) -> RecipeRef {
    let at = |v: f32| Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, v)])]);
    Recipe {
        target,
        steps: vec![at(0.0), at(-0.75), at(-0.75), at(-0.75)],
        timing: Timing {
            speed: Speed::Master("Song".into()),
            measure: bars * 4.0,
            phase_spread_deg: 360.0,
            direction: play,
            ..Default::default()
        },
        tricks: Vec::new(),
        stack: false,
        ..Default::default()
    }
    .into()
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
fn effect(name: &str, target: Option<Selection>, bars: Option<f32>) -> RecipeRef {
    assert!(
        ignition_core::effects::library().contains_key(name),
        "no library effect named {name:?}"
    );
    RecipeRef::Named {
        effect: name.to_string(),
        target,
        bars,
        tricks: None,
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
        RecipeRef::Named {
            effect,
            target,
            bars,
            ..
        } => RecipeRef::Named {
            effect,
            target,
            bars,
            tricks: Some(tricks),
        },
        bundle @ RecipeRef::Bundle { .. } => bundle,
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

/// The last bar of a named section — where a build lands and a stab
/// goes.
// r[impl song.relative-position] - "the last bar of"
fn last_bar(section: &str) -> Position {
    Position::last_bar(section)
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
    fn cue(&mut self, position: Position, name: &str, fade_beats: f64, recipes: Vec<RecipeRef>) {
        let Some(at) = position.resolve(self.song) else {
            // A section this arrangement does not have is skipped rather
            // than placed at bar 1, which is where an `unwrap_or_default`
            // would silently put it.
            eprintln!("warning: skipping {name:?} — the arrangement has no such section");
            return;
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
// The palette is a decision, not a default: **cold** (Cool White, Sky,
// Cyan, Deep Blue) for the intro and verses, **hot** (Warm White, Gold,
// Amber) for the choruses, and one colour the song has not used yet —
// Congo and Purple — for the bridge. Every section changes at least two
// of level, colour, layers and movement from the one before.

// r[impl song.relative-position] - every cue placed relative to a section
// r[impl cues.sorted-by-position]
fn author(song: &SongMap) -> CueList {
    let mut a = Author {
        song,
        cues: Vec::new(),
    };

    // ── top ──────────────────────────────────────────────────────────
    // Nothing but the back wall, so the first note has somewhere to
    // arrive from.
    a.cue(
        at("Count-In", 0),
        "Count-In",
        0.0,
        vec![
            dark(wash()),
            dark(key()),
            dark(bars()),
            dark(movers()),
            dark(drums()),
            dark(floor()),
            look(back(), 0.12, "Deep Blue"),
        ],
    );

    // ── intro A: the riff, cold and mechanical ───────────────────────
    // The ceiling fills left to right once a bar — the riff stated by
    // the rig. Movers up on the back wall in cyan, a fan so the beams
    // read as a set rather than a spot. No key light: nobody is singing.
    a.cue(
        at("IN A", 0),
        "IN A",
        1.0,
        vec![
            look(wash(), 0.28, "Cool White"),
            dark(key()),
            look(back(), 0.5, "House Blue"),
            look(bars(), 0.3, "Deep Blue"),
            look(movers(), 0.55, "Cyan"),
            fan("Back Wall", "Band"),
            dark(drums()),
            chase(wash_lr(), 0.28, 1.0, 360.0, Play::Build),
            effect("bar tick", None, None),
        ],
    );

    // ── intro B: the room opens ──────────────────────────────────────
    // The same idea, one size bigger: a cold gradient across the wash,
    // the movers start to sway, the bars pick up purple.
    a.cue(
        at("IN B", 0),
        "IN B",
        2.0,
        vec![
            gradient(wash_lr(), 0.45, "Sky", "Cyan"),
            look(key(), 0.3, "Cool White"),
            look(back(), 0.65, "House Blue"),
            look(bars(), 0.4, "Purple"),
            look(movers(), 0.6, "Cyan"),
            aim(movers(), "Back Wall"),
            effect("sway", None, Some(2.0)),
            chase(wash_out(), 0.3, 2.0, 360.0, Play::Bounce),
        ],
    );

    // ── verses: low, and the vocal is the picture ────────────────────
    // Key light warm and up; the wash sits low in a lavender-to-purple
    // spread; the back wall deep blue. Movers dark in the first verse,
    // indigo and barely moving in the second. A slow bounce breathes
    // across the ceiling so the rig is alive without being busy.
    let verse = |second: bool| {
        let mut v = vec![
            gradient(wash_lr(), 0.32, "Lavender", "Purple"),
            look(key(), 0.72, "Warm White"),
            look(back(), 0.4, "Deep Blue"),
            look(bars(), if second { 0.35 } else { 0.2 }, "Purple"),
            dark(drums()),
            dark(floor()),
            chase(wash_out(), 0.12, 8.0, 360.0, Play::Bounce),
        ];
        if second {
            v.push(look(movers(), 0.3, "Indigo"));
            v.push(aim(movers(), "Band"));
            v.push(effect("circle tight", None, Some(8.0)));
        } else {
            v.push(dark(movers()));
        }
        v
    };
    a.cue(at("VS 1", 0), "VS 1", 2.0, verse(false));
    // Halfway through, the bars join in — a lift the ear hears at the
    // second half of a verse.
    a.cue(
        at("VS 1", 4),
        "· VS 1 lift",
        2.0,
        vec![
            look(bars(), 0.4, "Magenta"),
            look(drums(), 0.3, "Cool White"),
        ],
    );

    // ── pre-chorus: one long build ───────────────────────────────────
    // Magenta and pink alternating across the wash, a build that fills
    // the room once a bar, movers nodding on the stage in cyan. The
    // last bar carries figure 0 — three hits carving the stage into
    // thirds — which the triggers supply.
    let pre = || {
        vec![
            two_tone(wash_lr(), 0.55, "Magenta", "Pink"),
            look(key(), 0.7, "Warm White"),
            look(back(), 0.72, "Magenta"),
            look(bars(), 0.6, "Magenta"),
            look(movers(), 0.6, "Cyan"),
            aim(movers(), "Stage"),
            effect("nod", None, Some(1.0)),
            dark(drums()),
            chase(wash_lr(), 0.45, 1.0, 360.0, Play::Build),
        ]
    };
    a.cue(at("PRE", 0), "PRE", 2.0, pre());

    // ── choruses ─────────────────────────────────────────────────────
    // Everything, warm. Gold on the back wall, amber bars, open white
    // movers fanned out over the audience. The dark gap chase travels
    // through the ceiling once a bar so the rig moves without ever
    // taking the stage light away; the movers windmill. Headroom is
    // left for the hits, which land white.
    let chorus = |focus_to: &'static str, movement: &'static str| {
        vec![
            look(wash(), 0.72, "Warm White"),
            look(key(), 0.72, "Open White"),
            look(back(), 0.72, "Gold"),
            look(bars(), 0.72, "Amber"),
            look(movers(), 0.72, "Open White"),
            fan("Vocal", focus_to),
            look(drums(), 0.6, "Cool White"),
            look(floor(), 0.6, "Amber"),
            gap_chase(wash_lr(), 1.0, Play::Negative),
            effect(movement, None, Some(4.0)),
        ]
    };
    a.cue(at("CH 1", 0), "CH 1", 0.25, chorus("Audience", "windmill"));

    // ── the one-bar break ────────────────────────────────────────────
    // A breath: everything out but the back wall in congo, and the key
    // just enough to see a face. The contrast is the point.
    a.cue(
        at("Break", 0),
        "Break",
        0.0,
        vec![
            dark(wash()),
            dark(bars()),
            dark(movers()),
            dark(drums()),
            dark(floor()),
            look(key(), 0.25, "Warm White"),
            look(back(), 0.72, "Congo"),
        ],
    );

    // ── second time round, a little more ─────────────────────────────
    a.cue(at("VS 2", 0), "VS 2", 2.0, verse(true));
    a.cue(
        at("VS 2", 4),
        "· VS 2 lift",
        1.0,
        vec![
            look(bars(), 0.55, "Magenta"),
            look(drums(), 0.45, "Cool White"),
            effect("bar sparkle", None, None),
        ],
    );
    a.cue(at_nth("PRE", 1, 0), "PRE 2", 2.0, pre());
    // The second chorus swaps the movement so it is not a replay.
    a.cue(at("CH 2", 0), "CH 2", 0.25, chorus("Audience", "ballyhoo"));

    // ── bridge: somewhere else ───────────────────────────────────────
    // The colour the song has not used: purple into congo across the
    // wash, cyan movers on the drums, a bounce and a circle. Slower,
    // wider, and deliberately not warm.
    a.cue(
        at("BR", 0),
        "BR",
        2.0,
        vec![
            gradient(wash_lr(), 0.45, "Purple", "Congo"),
            look(key(), 0.6, "Cool White"),
            look(back(), 0.72, "Congo"),
            look(bars(), 0.5, "Magenta"),
            look(movers(), 0.72, "Cyan"),
            aim(movers(), "Drums"),
            look(drums(), 0.72, "Cool White"),
            dark(floor()),
            chase(wash_lr(), 0.35, 2.0, 360.0, Play::Bounce),
            effect("circle", None, Some(4.0)),
        ],
    );

    // ── breakdown: strip it back ─────────────────────────────────────
    // Cool key alone, a whisper of blue behind, the movers slowly
    // flying out over the house. The last bar is the run-up.
    a.cue(
        at("Breakdown", 0),
        "Breakdown",
        4.0,
        vec![
            dark(wash()),
            dark(bars()),
            dark(drums()),
            dark(floor()),
            look(key(), 0.5, "Cool White"),
            look(back(), 0.25, "Deep Blue"),
            look(movers(), 0.35, "Indigo"),
            fan("Band", "House"),
            effect("fly out", None, Some(4.0)),
        ],
    );
    a.cue(
        last_bar("Breakdown"),
        "· Breakdown build",
        0.5,
        vec![
            look(wash(), 0.5, "Cool White"),
            look(back(), 0.72, "Magenta"),
            look(bars(), 0.5, "Magenta"),
            chase(wash_lr(), 0.5, 1.0, 360.0, Play::Build),
            effect("rig build", None, Some(1.0)),
        ],
    );

    // ── last chorus: the biggest thing in the show ───────────────────
    // The chorus, then more every four bars: the floor and a gold-to-red
    // gradient, then eighth-note chases and a sparkle on the bars for
    // the run-out. The eight-hit stab at the end is a trigger run.
    a.cue(at("CH 3", 0), "CH 3", 0.25, chorus("House", "windmill"));
    a.cue(
        at("CH 3", 4),
        "· CH 3 wider",
        1.0,
        vec![
            gradient(wash_lr(), 0.72, "Gold", "Red"),
            look(floor(), 0.72, "Open White"),
            aim(floor(), "House"),
            effect("ballyhoo", None, Some(2.0)),
        ],
    );
    a.cue(
        at("CH 3", 8),
        "· CH 3 drive",
        0.25,
        vec![
            effect("chase eighths", Some(wash_lr()), None),
            tricked(effect("bar sparkle", None, None), vec![Trick::Group(2)]),
            effect("windmill", None, Some(1.0)),
        ],
    );

    // ── out ──────────────────────────────────────────────────────────
    // Back to the intro's cold, then away.
    a.cue(
        at("Outro", 0),
        "Outro",
        2.0,
        vec![
            gradient(wash_lr(), 0.4, "Sky", "Cyan"),
            look(key(), 0.5, "Cool White"),
            look(back(), 0.35, "House Blue"),
            look(bars(), 0.25, "Deep Blue"),
            look(movers(), 0.4, "Cyan"),
            fan("Back Wall", "Band"),
            dark(drums()),
            dark(floor()),
        ],
    );
    a.cue(
        at("Outro", 1),
        "· Outro out",
        4.0,
        vec![
            dark(wash()),
            dark(key()),
            dark(bars()),
            dark(movers()),
            look(back(), 0.12, "Deep Blue"),
        ],
    );

    // Positions are authored per section, not in order, so sort before
    // handing over: `seek` walks the list backwards looking for the last
    // cue at or before a position and needs it ordered.
    let mut list = CueList {
        name: song.name.clone(),
        cues: a.cues,
        triggers: Vec::new(),
    };
    list.sort_by_position();
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
            min: ignition_core::Vec3 {
                x: min_x,
                y: -30.0,
                z: 0.0,
            },
            max: ignition_core::Vec3 {
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
        tricks: Vec::new(),
        stack: false,
        ..Default::default()
    }
}

/// The pulse recipes for one section — kick on the bars, snare overhead.
// r[impl song.chart.pulse] - lives in the section look; a section with none gets none
fn pulses(chart: &HitChart, song: &SongMap, section: &str) -> Vec<Recipe> {
    let chorus = section.starts_with("CH");
    let snare_depth = if chorus { 0.28 } else { 0.14 };
    let kick_depth = 0.10;

    let mut out = Vec::new();
    if let Some(slots) = pattern(chart, song, section, HitClass::Kick) {
        out.push(pulse(bars(), slots, kick_depth));
    }
    if let Some(slots) = pattern(chart, song, section, HitClass::Snare) {
        out.push(pulse(wash(), slots, snare_depth));
    }
    out
}

/// A bump on a selection, sized by the hit's class.
///
/// White for the band hits, so they read through a coloured look; a
/// level lift for the soft ones. One object shared with the operator's
/// flash key, so a charted hit and a hand on the same snare are
/// indistinguishable.
// r[impl effects.bump.one-object]
// r[impl playback.flash-equals-hit]
fn bump(target: Selection, depth: f32) -> Recipe {
    let kind = if depth >= 0.6 {
        ignition_core::BumpKind::White
    } else {
        ignition_core::BumpKind::Level
    };
    ignition_core::bump::bump(target, kind, depth)
}

/// A relative envelope with the bump's shape, scaled by `depth` —
/// negative for a cut, and past 1.0 for a lift that has to climb out of
/// a cut on the same fixtures.
///
/// Built from a unit bump and scaled here rather than passed through
/// `bump`'s own depth, which clamps to one: a reveal summed with a cut
/// needs the lift to be cut-plus-level, which is more than one.
fn envelope(target: Selection, depth: f32) -> Recipe {
    let mut r = ignition_core::bump::bump(target, ignition_core::BumpKind::Level, 1.0);
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

/// The triggers for every band hit and every figure.
///
/// A figure of two or three moments is a **cutout**: the whole rig is
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
// r[impl song.chart.hit] - one trigger per band hit; pulse classes get none
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
                    recipe: bump(zone(n, count), level),
                    name,
                    hold,
                });
            }
        }
    }

    // The soft tiers are the pulse and already have their effect; a
    // trigger as well would flash twice for one hit.
    for hit in chart
        .ungrouped()
        .filter(|h| !matches!(h.class, HitClass::Kick | HitClass::Snare))
    {
        // High hits take the bars with them; the rest stay on the wash.
        let target = if hit.class == HitClass::High {
            Selection::Union(vec![wash(), bars()])
        } else {
            wash()
        };
        let (position, resolved) = place(hit.at);
        out.push(Trigger {
            at: position,
            resolved,
            recipe: bump(target, hit.intensity()),
            name: format!("{} {}.{:.2}", hit.class.label(), hit.at.bar, hit.at.beat),
            // A lone hit is a moment, not a state: it snaps and falls.
            // Holding is for figures, where the next moment is what
            // releases the last.
            hold: false,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::{Section, TempoMap};

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
            ("BR", 8.0),
            ("Breakdown", 8.0),
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
        // Lengthen the verses: the second PRE follows its own section,
        // which "8 bars into VS 2" would not have.
        let song = arrangement(12.0);
        let list = author(&song);
        assert_eq!(cue_at(&list, "PRE 2"), Bars::bar(48));
    }

    /// r[verify song.relative-position]
    /// r[verify song.relative-position.resolved-on-load]
    #[test]
    fn the_file_repositions_itself_against_a_new_arrangement() {
        let mut list = author(&arrangement(8.0));
        assert_eq!(cue_at(&list, "· Breakdown build"), Bars::bar(67));
        // What a loader does: the file round-trips through JSON with its
        // relative positions, and resolves against this week's map.
        let json = serde_json::to_string(&list).unwrap();
        let mut list: CueList = serde_json::from_str(&json).unwrap();
        let unresolved = list.resolve_positions(&arrangement(6.0));
        assert!(unresolved.is_empty(), "{unresolved:?}");
        // Every cue after the first verse moved up two bars per verse.
        assert_eq!(cue_at(&list, "CH 1"), Bars::bar(21));
        assert_eq!(cue_at(&list, "PRE 2"), Bars::bar(36));
        assert_eq!(cue_at(&list, "· Breakdown build"), Bars::bar(63));
        let build = list
            .cues
            .iter()
            .find(|c| c.name == "· Breakdown build")
            .unwrap();
        assert_eq!(build.at, Some(Position::last_bar("Breakdown")));
        list.sort_by_position();
    }

    /// The library effects go into the file by name, never as copies.
    /// r[verify effects.library.by-name]
    #[test]
    fn library_effects_are_written_as_references() {
        let list = author(&arrangement(8.0));
        let named: Vec<&str> = list
            .cues
            .iter()
            .flat_map(|c| &c.recipes)
            .filter_map(|r| match r {
                RecipeRef::Named { effect, .. } => Some(effect.as_str()),
                _ => None,
            })
            .collect();
        assert!(named.contains(&"circle tight"), "{named:?}");
        let json = serde_json::to_string(&list).unwrap();
        assert!(json.contains(r#""effect":"circle tight""#));
    }

    #[test]
    fn options_parse_and_slug_names_the_sidecars() {
        let o = Options::parse(
            ["song.RPP", "--merge", "old.json", "--no-sidecars"]
                .map(String::from)
                .into_iter(),
        )
        .unwrap();
        assert_eq!(o.project, "song.RPP");
        assert_eq!(o.merge.as_deref(), Some(std::path::Path::new("old.json")));
        assert!(o.no_sidecars);
        assert!(Options::parse(["--merge"].map(String::from).into_iter()).is_err());
        assert_eq!(slug("Bye Bye Bye"), "bye-bye-bye");
        assert_eq!(slug("I Want It That Way (Live)"), "i-want-it-that-way-live");
    }
}
