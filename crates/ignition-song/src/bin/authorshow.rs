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

use ignition_song::hits::{Hit, HitBand, Hits};
use ignition_core::preset::Ref;
use ignition_core::selection::{Axis, Cmp, Dir, Order, Where};
use ignition_core::{
    Attribute, Bars, Cue, CueList, Play, Recipe, RecipeApply, Selection, SongMap, Speed, Step,
    Timing, Waveform,
};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: authorshow <project file> [hits.json]"))?;
    let song = ignition_song::load(&path)?;
    let mut list = author(&song);

    // Hits are optional, and the show is complete without them. The
    // section looks and the chases are the show; accents punctuate it.
    // Keeping them separable means a re-analysis cannot break a working
    // show, and the same authored show runs for a song nobody has
    // analysed yet.
    if let Some(hits_path) = args.next() {
        let hits: Hits = serde_json::from_str(&std::fs::read_to_string(&hits_path)?)?;
        let accents = accents(&song, &hits);
        eprintln!(
            "{} accent cues from {} hits",
            accents.len() / 2,
            hits.hits.len()
        );
        list.cues.extend(accents);
        list.cues
            .sort_by(|x, y| x.at.partial_cmp(&y.at).unwrap_or(std::cmp::Ordering::Equal));
    }

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

// ── accents, from the detected hits ───────────────────────────────────

/// How hard a hit must be before it earns its own cue.
///
/// High deliberately. The detector finds about a thousand hits in this
/// song and nearly all of them are real — hats, ghost notes, the
/// eighth-note pulse. None of those want a light cue. A chase already
/// carries the pulse, running off the `Song` speed master, and bumping
/// on every eighth on top of that reads as flicker rather than accent.
/// What a bump is *for* is the handful of moments the whole band hits
/// together, and there are a dozen or so of those in any pop song.
const ACCENT: f32 = 0.72;

/// Beats of quiet either side for a hit to count as isolated.
///
/// A stab in a breakdown is worth the whole rig; the same strength
/// inside a chorus is just the chorus being loud. Isolation is what
/// separates them, and the detector cannot — it has no idea what a
/// breakdown is — so it is decided here, from the neighbours.
const ISOLATION_BEATS: f64 = 1.5;

/// How strong a neighbour has to be to spoil a hit's isolation.
///
/// Calibrated against this song rather than guessed, because the first
/// guess was wrong in a way that hid itself: at 0.35 *nothing* in the
/// track qualified as isolated, so the whole-rig stab branch was dead
/// code that still looked reasonable. At 0.5 three moments qualify, at
/// 0.6 eight. Three full-rig hits in a three-minute pop song is about
/// right — a stab that happens eight times is not a stab any more.
const NEIGHBOUR: f32 = 0.5;

/// Bump cues on the song's biggest hits.
///
/// Each accent is a pair: a hard snap up, and a recovery a beat later.
/// The cue system tracks and has no one-shot envelope — a two-step
/// recipe would loop, which is a strobe rather than a bump — so a bump
/// is spelled the way an operator busks it, as GO and GO again.
///
/// Both are `·`-named and therefore non-blocking, so an accent adds to
/// whatever section look is running rather than replacing it. That is
/// what lets this be generated separately from the show: it never has to
/// know what it is landing on top of.
fn accents(song: &SongMap, hits: &Hits) -> Vec<Cue> {
    let mut chosen: Vec<&Hit> = Vec::new();
    for hit in &hits.hits {
        if hit.strength < ACCENT {
            continue;
        }
        // One bump per moment. A band stab fires the detector more than
        // once, and two cues at the same position would fight over the
        // same fixtures at the same instant.
        if chosen
            .last()
            .is_some_and(|last| beats_between(song, last.at, hit.at) < 1.0)
        {
            continue;
        }
        chosen.push(hit);
    }

    let mut cues = Vec::new();
    for (i, hit) in chosen.iter().enumerate() {
        let isolated = is_isolated(song, hits, hit);
        // What the hit *is* decides what it lights, which is the point
        // of classifying the band at all. A kick is felt low and wide,
        // so it takes the floor strips and the back wall; a cymbal is
        // bright and overhead, so it takes the ceiling. Lighting both
        // the same way throws away the one piece of information the
        // detector worked hardest for.
        let mut targets = match hit.band {
            HitBand::Low => vec![strips(), back()],
            HitBand::Mid => vec![ceiling(), back()],
            HitBand::High => vec![ceiling()],
        };
        // An isolated stab gets the whole rig — that is the band
        // stopping together, and it should read as the room stopping
        // with them. A hit inside a busy chorus gets only its own
        // fixtures, or it obliterates the look it is punctuating.
        let depth = if isolated {
            targets = vec![ceiling(), strips(), back()];
            0.55
        } else {
            0.30
        };
        // Two decimals: at eighth resolution "6.4" and "6.3.50" are
        // different places, and a name that rounds one onto the other
        // makes the cue list lie about where its own cue is.
        let name = format!(
            "· hit {}.{:.2} {}",
            hit.at.bar,
            hit.at.beat,
            if isolated { "stab" } else { "accent" }
        );
        let bpm = song.tempo.at(hit.at).bpm;
        cues.push(Cue {
            name: name.clone(),
            fade_secs: 0.0,
            values: Vec::new(),
            recipes: targets
                .iter()
                .map(|t| bump(t.clone(), depth * hit.strength))
                .collect(),
            block: false,
            at: Some(hit.at),
        });
        // Recover a beat later — unless the next accent is already
        // there. Emitting both would put a zero and a lift at the same
        // position, and which survived would come down to sort order:
        // the new bump silently cancelled by the previous hit's
        // recovery, on the beat where it mattered most.
        let out_at = after(song, hit.at, 1.0);
        if chosen
            .get(i + 1)
            .is_some_and(|next| beats_between(song, out_at, next.at).abs() < 0.25)
        {
            continue;
        }
        cues.push(Cue {
            name: format!("{name} out"),
            // Recovery is a fade, not a snap: an instant drop reads as
            // the light having failed rather than as the hit ending.
            fade_secs: (0.5 * 60.0 / bpm) as f32,
            values: Vec::new(),
            recipes: targets.iter().map(|t| bump(t.clone(), 0.0)).collect(),
            block: false,
            at: Some(out_at),
        });
    }
    cues
}

/// The lift itself — a positive `Delta`, so it adds to the running look
/// and leaves its colour alone: the same layering rule the chases use.
fn bump(target: Selection, depth: f32) -> Recipe {
    Recipe::new(target, RecipeApply::Delta(vec![(Attribute::Dimmer, depth)]))
}

/// Is this hit standing on its own?
fn is_isolated(song: &SongMap, hits: &Hits, hit: &Hit) -> bool {
    !hits.hits.iter().any(|other| {
        other.at != hit.at
            && other.strength > NEIGHBOUR
            && beats_between(song, other.at, hit.at).abs() < ISOLATION_BEATS
    })
}

/// Signed distance between two positions, in beats.
fn beats_between(song: &SongMap, from: Bars, to: Bars) -> f64 {
    let bpm = song.tempo.at(from).bpm;
    (song.tempo.seconds_at(to) - song.tempo.seconds_at(from)) * bpm / 60.0
}

/// `beats` past a position.
fn after(song: &SongMap, at: Bars, beats: f64) -> Bars {
    let bpm = song.tempo.at(at).bpm;
    song.tempo
        .position_at(song.tempo.seconds_at(at) + beats * 60.0 / bpm)
}
