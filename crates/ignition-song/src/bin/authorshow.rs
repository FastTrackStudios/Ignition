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

use ignition_core::preset::Ref;
use ignition_core::selection::{Axis, Cmp, Dir, Order, Where};
use ignition_core::{
    Attribute, Bars, Cue, CueList, Play, Recipe, RecipeApply, Selection, SongMap, Speed, Step,
    Timing, Waveform,
};

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: authorshow <project file>"))?;
    let song = ignition_song::load(&path)?;
    let list = author(&song);
    println!("{}", serde_json::to_string_pretty(&list)?);
    Ok(())
}

// ── the rig, by role ─────────────────────────────────────────────────

/// The ceiling wash — everything tagged as a wash above head height,
/// which is the truss and excludes the floor package.
fn ceiling() -> Selection {
    Selection::Where {
        of: Box::new(Selection::Tag("Luminaire_LED_Wash".into())),
        filter: Where::Half {
            axis: Axis::Z,
            cmp: Cmp::Gt,
            at: 2.0,
        },
    }
}

/// The ceiling, ordered left to right by where the fixtures actually
/// hang. This is the "build the order, not the effect" seam: the same
/// chase reads as a direction because the *selection* knows the room.
fn ceiling_lr() -> Selection {
    Selection::Order {
        of: Box::new(ceiling()),
        by: Order::Axis(Axis::X, Dir::Asc),
    }
}

/// The ceiling, ordered outward from the front of the stage.
fn ceiling_out() -> Selection {
    Selection::Order {
        of: Box::new(ceiling()),
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

fn key() -> Selection {
    Selection::Group("Center Washers".into())
}
fn back() -> Selection {
    Selection::Group("Back Wall Pars".into())
}
fn strips() -> Selection {
    Selection::Group("Strips All".into())
}
fn movers() -> Selection {
    Selection::Model("Moving Head".into())
}
fn drum_highlight() -> Selection {
    Selection::Union(vec![
        Selection::Group("Drum Highlight L".into()),
        Selection::Group("Drum Highlight R".into()),
    ])
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
            dark(ceiling()),
            dark(strips()),
            dark(movers()),
            dark(drum_highlight()),
            look(back(), 0.15, "Deep Blue"),
        ],
    );

    // ── intro: cold, and filling up ──────────────────────────────────
    a.cue(
        at(song, "IN A", 0),
        "IN A",
        4.0,
        vec![
            look(ceiling(), 0.3, "Cool White"),
            look(back(), 0.55, "House Blue"),
            dark(strips()),
            dark(movers()),
            // Build: the ceiling fills across four bars and resets — the
            // arrival of the song, stated by the rig rather than by a fade.
            chase(ceiling_lr(), 0.3, 4.0, 360.0, Play::Build),
        ],
    );

    a.cue(
        at(song, "IN B", 0),
        "IN B",
        2.0,
        vec![
            look(ceiling(), 0.45, "Cool White"),
            look(back(), 0.7, "House Blue"),
            look(strips(), 0.4, "Deep Blue"),
            look(movers(), 0.5, "Cool White"),
            aim(movers(), "Back Wall"),
            chase(ceiling_out(), 0.35, 2.0, 360.0, Play::Bounce),
        ],
    );

    // ── verse one: dark, vocal-led ───────────────────────────────────
    let verse = |strip_level: f32| {
        vec![
            look(ceiling(), 0.35, "Lavender"),
            look(key(), 0.75, "Warm White"),
            look(back(), 0.4, "Deep Blue"),
            look(strips(), strip_level, "Purple"),
            dark(movers()),
            dark(drum_highlight()),
            chase(ceiling_out(), 0.15, 8.0, 360.0, Play::Bounce),
        ]
    };
    a.cue(at(song, "VS 1", 0), "VS 1", 4.0, verse(0.2));
    // An accent four bars in: the strips lift, nothing else moves.
    a.cue(
        at(song, "VS 1", 4),
        "· VS 1 lift",
        1.0,
        vec![look(strips(), 0.55, "Magenta")],
    );

    // ── pre-chorus: build ────────────────────────────────────────────
    let pre = || {
        vec![
            look(ceiling(), 0.55, "Cool White"),
            look(back(), 0.8, "Magenta"),
            look(strips(), 0.7, "Magenta"),
            look(movers(), 0.6, "Cyan"),
            aim(movers(), "Stage Wide"),
            dark(drum_highlight()),
            // One build per bar, filling the room four times over the
            // section — the lift you can hear.
            chase(ceiling_lr(), 0.45, 1.0, 360.0, Play::Build),
        ]
    };
    a.cue(at(song, "PRE", 0), "PRE", 2.0, pre());

    // ── choruses ─────────────────────────────────────────────────────
    let chorus = |focus: &'static str| {
        vec![
            look(ceiling(), 1.0, "Warm White"),
            look(key(), 1.0, "Open White"),
            look(back(), 1.0, "Gold"),
            look(strips(), 1.0, "Amber"),
            look(movers(), 1.0, "Open White"),
            aim(movers(), focus),
            look(drum_highlight(), 0.9, "Cool White"),
            // The dark gap: everything lit, one hole travelling across
            // the ceiling once a bar. Reads as movement without ever
            // taking the stage light away.
            gap_chase(ceiling_lr(), 1.0, Play::Negative),
            swing(4.0, 18.0),
        ]
    };
    a.cue(at(song, "CH 1", 0), "CH 1", 0.25, chorus("Audience Front"));

    // ── the one-bar break ────────────────────────────────────────────
    a.cue(
        at(song, "Break", 0),
        "Break",
        0.0,
        vec![
            dark(ceiling()),
            dark(strips()),
            dark(movers()),
            dark(key()),
            dark(drum_highlight()),
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
            look(strips(), 0.6, "Magenta"),
            look(drum_highlight(), 0.5, "Cool White"),
        ],
    );
    // The second PRE is the same look; `section` finds the first by
    // name, so this one is placed off VS 2 instead.
    a.cue(at(song, "VS 2", 8), "PRE 2", 2.0, pre());
    a.cue(at(song, "CH 2", 0), "CH 2", 0.25, chorus("Audience Front"));

    // ── bridge and breakdown ─────────────────────────────────────────
    a.cue(
        at(song, "BR", 0),
        "BR",
        2.0,
        vec![
            look(ceiling(), 0.4, "Purple"),
            look(back(), 0.85, "Congo"),
            look(strips(), 0.5, "Magenta"),
            look(movers(), 0.85, "Cyan"),
            aim(movers(), "Drums"),
            look(drum_highlight(), 0.8, "Cool White"),
            chase(ceiling_lr(), 0.4, 2.0, 360.0, Play::Bounce),
            swing(2.0, 26.0),
        ],
    );

    a.cue(
        at(song, "Breakdown", 0),
        "Breakdown",
        4.0,
        vec![
            dark(ceiling()),
            dark(strips()),
            dark(movers()),
            dark(drum_highlight()),
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
            look(ceiling(), 0.5, "Cool White"),
            look(back(), 0.9, "Magenta"),
            chase(ceiling_lr(), 0.5, 1.0, 360.0, Play::Build),
        ],
    );

    // ── last chorus: the biggest thing in the show ───────────────────
    a.cue(at(song, "CH 3", 0), "CH 3", 0.25, chorus("Audience Back"));
    // Four bars in, hand it to the floor movers as well.
    a.cue(
        at(song, "CH 3", 4),
        "· CH 3 wider",
        1.0,
        vec![
            look(Selection::Group("Floor Movers".into()), 1.0, "Open White"),
            aim(Selection::Group("Floor Movers".into()), "Audience Back"),
        ],
    );
    // ...and for the last four, double the chase rate.
    a.cue(
        at(song, "CH 3", 8),
        "· CH 3 drive",
        0.25,
        vec![
            gap_chase(ceiling_lr(), 0.5, Play::Negative),
            swing(2.0, 26.0),
        ],
    );

    // ── out ──────────────────────────────────────────────────────────
    a.cue(
        at(song, "Outro", 0),
        "Outro",
        4.0,
        vec![
            look(ceiling(), 0.4, "Cool White"),
            look(key(), 0.5, "Cool White"),
            look(back(), 0.35, "House Blue"),
            look(strips(), 0.25, "Deep Blue"),
            dark(movers()),
            dark(drum_highlight()),
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
