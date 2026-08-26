//! The *Bye Bye Bye* light show, authored against the song's own map.
//!
//! ```bash
//! cargo run -p ignition-song --bin authorshow -- \
//!     "Bye Bye Bye.RPP" > data/songs/bye-bye-bye.json
//! ```
//!
//! A program rather than hand-edited JSON, for the same reason the cue
//! generator is: every position here is written as *"two bars into CH
//! 1"*, not as bar 25. Move the chorus in the DAW, re-run this, and the
//! show moves with it. Nothing is addressed in seconds and nothing is
//! addressed by absolute bar.
//!
//! It leans on the effect attributes ported from Eos (`Play`): the
//! pre-chorus **builds**, the choruses run a **negative** chase — a dark
//! gap travelling across the ceiling, the thing their docs point out
//! that forward chasing cannot do — and the bridge **bounces**.

use ignition_song::chart::{HitChart, HitClass};
use ignition_core::preset::Ref;
use ignition_core::selection::{Axis, Dir, Order, Where};
use ignition_core::{
    Attribute, Bars, Cue, CueList, Ease, Play, Recipe, RecipeApply, Selection, SongMap, Speed,
    Step, Timing, Waveform,
};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: authorshow <project file>"))?;
    let song = ignition_song::load(&path)?;
    let mut list = author(&song);

    // Hits are optional, and the show is complete without them. The
    // section looks and the chases are the show; accents punctuate it.
    // Keeping them separable means a re-analysis cannot break a working
    // show, and the same authored show runs for a song nobody has
    // analysed yet.
    // The chart is read from the project's own HITS track, so there is
    // nothing to pass and nothing to keep in sync: edit the MIDI in
    // REAPER, re-run this, and the show follows.
    let chart = ignition_song::chart::read(&path, &song)?;
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
            cue.recipes.extend(recipes);
        }
        eprintln!("{pulsed} pulse effects across the section looks");

        let accents = accents(&chart);
        eprintln!(
            "{} accent cues from {} charted hits in {} figures",
            accents.len() / 2,
            chart.hits.len(),
            chart.groups.len()
        );
        list.cues.extend(accents);
        // Position first, then blocking cues ahead of accents at the
        // same position. A section usually starts on a crash, so its
        // cue and that crash's accent land on one downbeat — and a
        // blocking cue is a complete statement that would wipe an
        // accent placed before it. Ordering them explicitly is the
        // difference between the crash reading on top of the new look
        // and disappearing into it, which otherwise comes down to
        // whether the sort happened to be stable.
        list.cues.sort_by(|x, y| {
            x.at
                .partial_cmp(&y.at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(y.block.cmp(&x.block))
        });
    }

    println!("{}", serde_json::to_string_pretty(&list)?);
    Ok(())
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
fn wash() -> Selection {
    Selection::Role("Wash".into())
}

/// The wash, ordered left to right by where the fixtures actually hang.
///
/// The "build the order, not the effect" seam. Nothing downstream says
/// "left"; it says "spread across the selection", and the selection is
/// what knows which end is which.
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
/// The drum special. Optional at most venues, and the show runs without
/// it — a recipe covering nothing is not an error.
fn drums() -> Selection {
    Selection::Role("Drums".into())
}

// ── recipe shorthands ────────────────────────────────────────────────

fn look(target: Selection, level: f32, colour: &str) -> Recipe {
    let mut r = Recipe::new(target, RecipeApply::Dimmer(level));
    r.steps[0]
        .apply
        .push(RecipeApply::Color(Ref::Named(colour.into())));
    r
}

fn level(target: Selection, level: f32) -> Recipe {
    Recipe::new(target, RecipeApply::Dimmer(level))
}

fn dark(target: Selection) -> Recipe {
    level(target, 0.0)
}

fn aim(target: Selection, focus: &str) -> Recipe {
    Recipe::new(target, RecipeApply::FocusPoint(Ref::Named(focus.into())))
}

/// A relative intensity phaser on a selection, slaved to the song.
///
/// Relative on purpose — Eos's own model is that step effects modulate
/// *intensity* and leave colour alone, "which is the property that makes
/// them layerable". A `Delta` says how much to take away; whatever
/// colour the cue set is none of its business.
fn chase(target: Selection, depth: f32, bars: f32, spread: f32, play: Play) -> Recipe {
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
    }
}

/// A hard chase with a narrow lit step — the shape `Play::Negative`
/// turns into a dark gap.
fn gap_chase(target: Selection, bars: f32, play: Play) -> Recipe {
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
    }
}

/// A slow pan swing on the movers, a whole number of bars per sweep.
fn swing(bars: f32, degrees: f32) -> Recipe {
    Recipe {
        target: movers(),
        steps: Waveform::Sine.steps(Attribute::Pan, 0.0, degrees, true),
        timing: Timing {
            speed: Speed::Master("Song".into()),
            measure: bars * 4.0,
            phase_spread_deg: 180.0,
            ..Default::default()
        },
            tricks: Vec::new(),
    }
}

// ── positions ────────────────────────────────────────────────────────

/// `bars` into the named section. Every cue in this file is placed this
/// way, which is what makes the show survive an arrangement edit.
fn at(song: &SongMap, section: &str, bars: u32) -> Option<Bars> {
    song.section(section).map(|s| Bars::bar(s.start.bar + bars))
}

/// The last bar of a named section — where a build lands and a stab
/// goes.
fn last_bar(song: &SongMap, section: &str) -> Option<Bars> {
    song.section(section)
        .map(|s| Bars::bar(s.start.bar + s.bars as u32 - 1))
}

struct Author<'a> {
    song: &'a SongMap,
    cues: Vec<Cue>,
}

impl Author<'_> {
    /// A cue at a musical position. Fades are in beats and converted
    /// through the tempo map, so the show holds at any tempo.
    fn cue(&mut self, at: Option<Bars>, name: &str, fade_beats: f64, recipes: Vec<Recipe>) {
        let Some(at) = at else {
            // A section this arrangement does not have is skipped rather
            // than placed at bar 1, which is where an `unwrap_or_default`
            // would silently put it.
            tracing_missing(name);
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
            at: Some(at),
        });
    }
}

fn tracing_missing(name: &str) {
    eprintln!("warning: skipping {name:?} — the arrangement has no such section");
}

fn author(song: &SongMap) -> CueList {
    let mut a = Author {
        song,
        cues: Vec::new(),
    };

    // ── top ──────────────────────────────────────────────────────────
    a.cue(
        at(song, "Count-In", 0),
        "Count-In",
        0.0,
        vec![
            dark(wash()),
            dark(bars()),
            dark(movers()),
            dark(drums()),
            look(back(), 0.15, "Deep Blue"),
        ],
    );

    // ── intro: cold, and filling up ──────────────────────────────────
    a.cue(
        at(song, "IN A", 0),
        "IN A",
        4.0,
        vec![
            look(wash(), 0.3, "Cool White"),
            look(back(), 0.55, "House Blue"),
            dark(bars()),
            dark(movers()),
            // Build: the ceiling fills across four bars and resets — the
            // arrival of the song, stated by the rig rather than by a fade.
            chase(wash_lr(), 0.3, 4.0, 360.0, Play::Build),
        ],
    );

    a.cue(
        at(song, "IN B", 0),
        "IN B",
        2.0,
        vec![
            look(wash(), 0.45, "Cool White"),
            look(back(), 0.7, "House Blue"),
            look(bars(), 0.4, "Deep Blue"),
            look(movers(), 0.5, "Cool White"),
            aim(movers(), "Back Wall"),
            chase(wash_out(), 0.35, 2.0, 360.0, Play::Bounce),
        ],
    );

    // ── verse one: dark, vocal-led ───────────────────────────────────
    let verse = |strip_level: f32| {
        vec![
            look(wash(), 0.35, "Lavender"),
            look(key(), 0.75, "Warm White"),
            look(back(), 0.4, "Deep Blue"),
            look(bars(), strip_level, "Purple"),
            dark(movers()),
            dark(drums()),
            chase(wash_out(), 0.15, 8.0, 360.0, Play::Bounce),
        ]
    };
    a.cue(at(song, "VS 1", 0), "VS 1", 4.0, verse(0.2));
    // An accent four bars in: the strips lift, nothing else moves.
    a.cue(
        at(song, "VS 1", 4),
        "· VS 1 lift",
        1.0,
        vec![look(bars(), 0.55, "Magenta")],
    );

    // ── pre-chorus: build ────────────────────────────────────────────
    let pre = || {
        vec![
            look(wash(), 0.55, "Cool White"),
            look(back(), 0.8, "Magenta"),
            look(bars(), 0.7, "Magenta"),
            look(movers(), 0.6, "Cyan"),
            aim(movers(), "Stage"),
            dark(drums()),
            // One build per bar, filling the room four times over the
            // section — the lift you can hear.
            chase(wash_lr(), 0.45, 1.0, 360.0, Play::Build),
        ]
    };
    a.cue(at(song, "PRE", 0), "PRE", 2.0, pre());

    // ── choruses ─────────────────────────────────────────────────────
    let chorus = |focus: &'static str| {
        vec![
            look(wash(), 1.0, "Warm White"),
            look(key(), 1.0, "Open White"),
            look(back(), 1.0, "Gold"),
            look(bars(), 1.0, "Amber"),
            look(movers(), 1.0, "Open White"),
            aim(movers(), focus),
            look(drums(), 0.9, "Cool White"),
            // The dark gap: everything lit, one hole travelling across
            // the ceiling once a bar. Reads as movement without ever
            // taking the stage light away.
            gap_chase(wash_lr(), 1.0, Play::Negative),
            swing(4.0, 18.0),
        ]
    };
    a.cue(at(song, "CH 1", 0), "CH 1", 0.25, chorus("Audience"));

    // ── the one-bar break ────────────────────────────────────────────
    a.cue(
        at(song, "Break", 0),
        "Break",
        0.0,
        vec![
            dark(wash()),
            dark(bars()),
            dark(movers()),
            dark(key()),
            dark(drums()),
            look(back(), 1.0, "Congo"),
        ],
    );

    // ── second time round, a little more ─────────────────────────────
    a.cue(at(song, "VS 2", 0), "VS 2", 2.0, verse(0.35));
    a.cue(
        at(song, "VS 2", 4),
        "· VS 2 lift",
        1.0,
        vec![
            look(bars(), 0.6, "Magenta"),
            look(drums(), 0.5, "Cool White"),
        ],
    );
    // The second PRE is the same look; `section` finds the first by
    // name, so this one is placed off VS 2 instead.
    a.cue(at(song, "VS 2", 8), "PRE 2", 2.0, pre());
    a.cue(at(song, "CH 2", 0), "CH 2", 0.25, chorus("Audience"));

    // ── bridge and breakdown ─────────────────────────────────────────
    a.cue(
        at(song, "BR", 0),
        "BR",
        2.0,
        vec![
            look(wash(), 0.4, "Purple"),
            look(back(), 0.85, "Congo"),
            look(bars(), 0.5, "Magenta"),
            look(movers(), 0.85, "Cyan"),
            aim(movers(), "Drums"),
            look(drums(), 0.8, "Cool White"),
            chase(wash_lr(), 0.4, 2.0, 360.0, Play::Bounce),
            swing(2.0, 26.0),
        ],
    );

    a.cue(
        at(song, "Breakdown", 0),
        "Breakdown",
        4.0,
        vec![
            dark(wash()),
            dark(bars()),
            dark(movers()),
            dark(drums()),
            look(key(), 0.55, "Cool White"),
            look(back(), 0.3, "Deep Blue"),
        ],
    );
    // The last bar of the breakdown is the run-up.
    a.cue(
        last_bar(song, "Breakdown"),
        "· Breakdown build",
        0.5,
        vec![
            look(wash(), 0.5, "Cool White"),
            look(back(), 0.9, "Magenta"),
            chase(wash_lr(), 0.5, 1.0, 360.0, Play::Build),
        ],
    );

    // ── last chorus: the biggest thing in the show ───────────────────
    a.cue(at(song, "CH 3", 0), "CH 3", 0.25, chorus("House"));
    // Four bars in, hand it to the floor movers as well.
    a.cue(
        at(song, "CH 3", 4),
        "· CH 3 wider",
        1.0,
        vec![
            look(Selection::Role("Floor".into()), 1.0, "Open White"),
            aim(Selection::Role("Floor".into()), "House"),
        ],
    );
    // ...and for the last four, double the chase rate.
    a.cue(
        at(song, "CH 3", 8),
        "· CH 3 drive",
        0.25,
        vec![
            gap_chase(wash_lr(), 0.5, Play::Negative),
            swing(2.0, 26.0),
        ],
    );

    // ── out ──────────────────────────────────────────────────────────
    a.cue(
        at(song, "Outro", 0),
        "Outro",
        4.0,
        vec![
            look(wash(), 0.4, "Cool White"),
            look(key(), 0.5, "Cool White"),
            look(back(), 0.35, "House Blue"),
            look(bars(), 0.25, "Deep Blue"),
            dark(movers()),
            dark(drums()),
        ],
    );

    // Positions are authored per section, not in order, so sort before
    // handing over: `seek` walks the list backwards looking for the last
    // cue at or before a position and needs it ordered.
    a.cues
        .sort_by(|x, y| x.at.partial_cmp(&y.at).unwrap_or(std::cmp::Ordering::Equal));

    CueList {
        name: song.name.clone(),
        cues: a.cues,
    }
}

// ── the charted hits ─────────────────────────────────────────────────
//
// Two kinds of thing, treated as two kinds of thing.
//
// The two soft tiers are *pulse*. The snare runs two to the bar for
// fifty-six bars, and writing a cue for each would mean three hundred
// cues to say "flash on two and four" — a cue list nobody can read,
// describing a groove that never changes. On a console that is an
// effect: one running flash locked to the song, living in the section
// look. So the chart is read for the *pattern* each section plays, and
// that becomes a repeating one-bar phaser.
//
// The soft tier is sparser — placed where one is wanted rather than
// played through — which the per-section derivation already handles:
// a section with none simply gets no pulse, and the long stretches of
// this song that carry none stay clean.
//
// The band hits are *events*. They happen a few dozen times, each one
// means something, and each gets a cue.

/// How wide the wash rig is, in metres either side of centre.
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
/// This is the payoff of grouping. Three hits that are one idea become
/// left, centre, right — a figure travelling across the stage — where
/// three ungrouped hits could only ever be the same fixtures flashing
/// three times. The information that makes the difference is not in the
/// audio; it is in somebody having drawn one long note over the three.
fn zone(n: usize, count: usize) -> Selection {
    if count <= 1 {
        return wash();
    }
    let width = 2.0 * RIG_HALF_WIDTH / count as f64;
    let min_x = -RIG_HALF_WIDTH + width * n as f64;
    Selection::Where {
        // The wash role, not the tag it happens to carry at Norco. A
        // spatial filter over a role is still portable: every venue's
        // coordinates mean metres from its own stage, so "the left third
        // of the wash" is the left third wherever it is resolved.
        of: Box::new(wash()),
        filter: Where::Within {
            min: ignition_core::Vec3 { x: min_x, y: -30.0, z: 2.0 },
            max: ignition_core::Vec3 { x: min_x + width, y: 30.0, z: 30.0 },
        },
    }
}

/// The one-bar pattern a class plays inside a section, as eighth slots.
///
/// Folded across the section's bars and kept only where the hit lands in
/// most of them. That threshold is what separates the groove from the
/// fills: a snare on two and four every bar is the pattern, and the
/// extra snare in the turnaround of the last bar is not — repeating the
/// turnaround through the whole section would be wrong every bar but
/// one.
fn pattern(chart: &HitChart, song: &SongMap, section: &str, class: HitClass) -> Option<[bool; SLOTS]> {
    let section = song.section(section)?;
    let first = section.start.bar;
    let last = first + section.bars as u32;
    let bars = (last - first).max(1);

    let mut counts = [0usize; SLOTS];
    for hit in chart.hits.iter().filter(|h| h.class == class) {
        if hit.at.bar < first || hit.at.bar >= last {
            continue;
        }
        // beat 1.0 is slot 0, beat 1.5 slot 1, ... beat 4.5 slot 7.
        let slot = ((hit.at.beat - 1.0) * 2.0).round() as usize;
        if let Some(count) = counts.get_mut(slot) {
            *count += 1;
        }
    }

    let mut slots = [false; SLOTS];
    let mut any = false;
    for (slot, count) in counts.iter().enumerate() {
        // "In most bars" — half rounded up, so a four-bar section needs
        // the hit in two of them.
        if *count * 2 >= bars as usize {
            slots[slot] = true;
            any = true;
        }
    }
    any.then_some(slots)
}

/// A repeating one-bar flash on the charted pattern.
///
/// One step per eighth, `measure` a whole bar, locked to the `Song`
/// speed master — so it is the song's own tempo driving it and a tempo
/// change carries it along. `Delta` rather than an absolute level, for
/// the same reason the chases use one: it adds to whatever the section
/// look set and leaves the colour alone.
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
            // No spread: a backbeat is the whole rig ticking together,
            // not something travelling across it.
            phase_spread_deg: 0.0,
            ..Default::default()
        },
            tricks: Vec::new(),
    }
}

/// The pulse recipes for one section — kick on the floor, snare overhead.
///
/// Depths are per section, so a chorus can lean on the backbeat harder
/// than a verse without either of them being a different kind of thing.
fn pulses(chart: &HitChart, song: &SongMap, section: &str) -> Vec<Recipe> {
    // A chorus can carry a much harder backbeat; a verse cannot, and an
    // intro should barely tick.
    let chorus = section.starts_with("CH");
    let snare_depth = if chorus { 0.30 } else { 0.16 };
    // The soft tier stays soft everywhere. A chorus is allowed to lean
    // on the backbeat; the point of the gentlest class is that it does
    // not shout, and a chorus-loud version of it would just be a snare.
    let kick_depth = 0.10;

    let mut out = Vec::new();
    if let Some(slots) = pattern(chart, song, section, HitClass::Kick) {
        // The soft tier lives on the bar lights — Cody's call, and a
        // good one: the strips are a layer of their own, so a verse can
        // carry a gentle tick along the bars without touching the wash
        // the vocal is lit by or the ceiling the backbeat uses. Three
        // soft layers in three places read as texture; three in one
        // place read as flicker.
        out.push(pulse(bars(), slots, kick_depth));
    }
    if let Some(slots) = pattern(chart, song, section, HitClass::Snare) {
        out.push(pulse(wash(), slots, snare_depth));
    }
    out
}

/// Cues for the band hits — the events, as opposed to the pulse.
///
/// One cue per hit, and no releases. A bump is one event with a shape,
/// so the shape lives in the recipe as a one-shot envelope rather than
/// in a second cue a beat later. Before that, every hit cost two cues
/// and half the names in the list were "… out" — a cue list that could
/// not be read at a glance and told an operator nothing, since nobody
/// wants to press GO to end a flash.
///
/// A group is played *across* the rig; a lone hit is played on all of
/// it. All are `·`-named and non-blocking, so they add to whatever
/// section look is running rather than replacing it.
fn accents(chart: &HitChart) -> Vec<Cue> {
    let mut cues = Vec::new();

    for (index, group) in chart.groups.iter().enumerate() {
        // Hits landing together are one moment: a snare and a crash on
        // the same eighth should light one zone, not two, and the figure
        // should travel by musical position rather than by note count.
        let mut moments: Vec<(Bars, f32)> = Vec::new();
        for hit in chart.members(group) {
            match moments.last_mut() {
                Some((at, level)) if *at == hit.at => *level = level.max(hit.intensity()),
                _ => moments.push((hit.at, hit.intensity())),
            }
        }
        let count = moments.len();
        for (n, (at, level)) in moments.into_iter().enumerate() {
            cues.push(bump_cue(
                at,
                &format!("· fig {index} · {}/{count}", n + 1),
                zone(n, count),
                level,
            ));
        }
    }

    // The soft tiers are the pulse and already have their effect; a cue
    // as well would flash twice for one hit.
    for hit in chart
        .ungrouped()
        .filter(|h| !matches!(h.class, HitClass::Kick | HitClass::Snare))
    {
        cues.push(bump_cue(
            hit.at,
            &format!("· {} {}.{:.2}", hit.class.label(), hit.at.bar, hit.at.beat),
            wash(),
            hit.intensity(),
        ));
    }

    cues.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal));
    cues
}

/// How long a bump takes to fall back, in beats.
///
/// Just under an eighth, so a figure written in eighths has each flash
/// clear before the next arrives. Longer and the hits smear into one
/// another; much shorter and the rig ticks rather than swells.
const BUMP_BEATS: f32 = 0.45;

/// One hit, as a cue that puts itself out.
fn bump_cue(at: Bars, name: &str, target: Selection, level: f32) -> Cue {
    // Snap up, fall back. `transition: 0.0` on the lift is what makes it
    // a hit rather than a swell — a hit that eases in has already missed
    // the moment it was for.
    let up = Step {
        apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, level)])],
        width: 1.0,
        transition: 0.0,
        ease: Ease::default(),
    };
    let down = Step {
        apply: vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])],
        // Three times the lift's width and transitioning the whole way,
        // so the fall is a fall and not a second snap.
        width: 3.0,
        transition: 1.0,
        ease: Ease::default(),
    };
    Cue {
        name: name.to_string(),
        fade_secs: 0.0,
        values: Vec::new(),
        recipes: vec![Recipe {
            target,
            steps: vec![up, down],
            timing: Timing {
                speed: Speed::Master("Song".into()),
                measure: BUMP_BEATS,
                phase_spread_deg: 0.0,
                // The envelope runs once and holds at zero, which for a
                // Delta is the same as not being there.
                once: true,
                ..Default::default()
            },
            tricks: Vec::new(),
        }],
        block: false,
        at: Some(at),
    }
}

