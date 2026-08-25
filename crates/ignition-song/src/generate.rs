//! Turning a song's shape into a starting cue list.
//!
//! A section named `CH 1` that runs eight bars is already most of a cue:
//! it says what part of the song it is and exactly where it sits. This
//! turns that into cues at bar positions — a first draft to busk over
//! and edit, not a finished show.
//!
//! Two things make the draft worth having rather than a gimmick:
//!
//! - It is built from **recipes**, so it survives a rig change. The
//!   chorus targets "the ceiling wash, left to right", not a channel
//!   list, and adding a fixture to the truss puts it in the chorus.
//! - Fade times are given in **bars** and converted through the tempo
//!   map, so a four-beat fade is four beats at any tempo.
//!
//! What it cannot do is know the song. Hits, stabs, the blackout before
//! the last chorus — those are the parts a person adds, and the parts a
//! chart will eventually carry. See `docs/domain/musical-time-cues.md`.

use ignition_core::preset::Ref;
use ignition_core::selection::{Axis, Cmp, Dir, Order, Where};
use ignition_core::{
    Attribute, Cue, CueList, Recipe, RecipeApply, Section, Selection, SongMap, Speed, Timing,
    Waveform,
};

/// What part of a song a section is.
///
/// Inferred from the name, because that is what an arranger already
/// wrote. `VS 1`, `Verse 2` and `V3` are all verses; nobody should have
/// to tag them again for the lighting to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    CountIn,
    Intro,
    Verse,
    PreChorus,
    Chorus,
    Break,
    Bridge,
    Breakdown,
    Outro,
    Other,
}

/// Reads a section name as a part of a song.
///
/// Order matters here: `Breakdown` has to be tested before `Break`, and
/// `Bridge`/`BR` before anything starting with `B`. Getting that wrong
/// is silent — the section still generates a cue, just the wrong one.
pub fn kind_of(name: &str) -> Kind {
    // The first run of letters, not every letter: section names carry a
    // number or a letter to tell repeats apart — `VS 1`, `CH 3`, `IN A`
    // — and stripping the separator glues those on. `IN A` became `INA`
    // and read as nothing at all.
    let key: String = name
        .trim_start_matches(|c: char| !c.is_ascii_alphabetic())
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_uppercase();
    match key.as_str() {
        "COUNT" | "COUNTIN" | "CI" => Kind::CountIn,
        "BREAKDOWN" => Kind::Breakdown,
        "BREAK" | "BRK" => Kind::Break,
        "BRIDGE" | "BR" => Kind::Bridge,
        "PRE" | "PRECHORUS" => Kind::PreChorus,
        "CHORUS" | "CH" | "CHO" => Kind::Chorus,
        "VERSE" | "VS" | "V" => Kind::Verse,
        "INTRO" | "IN" => Kind::Intro,
        "OUTRO" | "END" | "ENDING" => Kind::Outro,
        _ => Kind::Other,
    }
}

/// Which fixtures play which part, as selections rather than channels.
///
/// Defaults lean on tags and models where a rig can be described
/// generically, and named groups only where the role is genuinely
/// venue-specific. Override any of them for a rig that disagrees.
#[derive(Debug, Clone)]
pub struct Roles {
    /// The main overhead wash.
    pub wash: Selection,
    /// The wash over the middle of the stage — a talking head, a solo.
    pub key: Selection,
    /// Anything on the back wall: colour behind the band.
    pub back: Selection,
    pub strips: Selection,
    pub movers: Selection,
}

impl Default for Roles {
    fn default() -> Self {
        Self {
            wash: ceiling(),
            key: Selection::Group("Center Washers".into()),
            back: Selection::Group("Back Wall Pars".into()),
            strips: Selection::Group("Strips All".into()),
            movers: Selection::Model("Moving Head".into()),
        }
    }
}

/// Wash fixtures hung above head height — the truss, not the floor.
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

fn ceiling_left_to_right() -> Selection {
    Selection::Order {
        of: Box::new(ceiling()),
        by: Order::Axis(Axis::X, Dir::Asc),
    }
}

/// A look: level and colour on one selection.
fn look(target: Selection, level: f32, colour: &str) -> Recipe {
    let mut recipe = Recipe::new(target, RecipeApply::Dimmer(level));
    recipe.steps[0]
        .apply
        .push(RecipeApply::Color(Ref::Named(colour.to_string())));
    recipe
}

fn dark(target: Selection) -> Recipe {
    Recipe::new(target, RecipeApply::Dimmer(0.0))
}

fn aim(target: Selection, focus: &str) -> Recipe {
    Recipe::new(
        target,
        RecipeApply::FocusPoint(Ref::Named(focus.to_string())),
    )
}

/// A chase over the ceiling, one cycle per `measure` beats, slaved to
/// the song's tempo rather than a free-running rate.
fn chase(measure: f64) -> Recipe {
    Recipe {
        target: ceiling_left_to_right(),
        // Relative, so it modulates whatever wash the cue also set
        // instead of replacing it.
        steps: Waveform::Sine.steps(Attribute::Dimmer, -0.35, 0.35, true),
        timing: Timing {
            speed: Speed::Master("Song".into()),
            measure: measure as f32,
            phase_spread_deg: 360.0,
            ..Default::default()
        },
    }
}

/// The recipes a section of each kind starts with.
fn look_for(kind: Kind, roles: &Roles) -> Vec<Recipe> {
    let (wash, key, back, strips, movers) = (
        roles.wash.clone(),
        roles.key.clone(),
        roles.back.clone(),
        roles.strips.clone(),
        roles.movers.clone(),
    );
    match kind {
        Kind::CountIn => vec![
            dark(wash),
            dark(strips),
            dark(movers),
            look(back, 0.25, "Deep Blue"),
        ],
        Kind::Intro => vec![
            look(wash, 0.35, "Cool White"),
            look(back, 0.6, "House Blue"),
            look(strips, 0.3, "Deep Blue"),
            dark(movers),
        ],
        Kind::Verse => vec![
            look(wash, 0.4, "Lavender"),
            look(key, 0.7, "Warm White"),
            look(back, 0.45, "Deep Blue"),
            look(strips, 0.25, "Purple"),
            dark(movers),
        ],
        Kind::PreChorus => vec![
            look(wash, 0.6, "Cool White"),
            look(back, 0.7, "Magenta"),
            look(strips, 0.6, "Magenta"),
            look(movers.clone(), 0.5, "Cyan"),
            aim(movers, "Stage Wide"),
        ],
        Kind::Chorus => vec![
            look(wash, 1.0, "Warm White"),
            look(back, 1.0, "Gold"),
            look(strips, 1.0, "Amber"),
            look(movers.clone(), 1.0, "Open White"),
            aim(movers, "Audience Front"),
            // One cycle per bar — the chase reads as the pulse of the
            // section rather than a speed somebody dialled in.
            chase(4.0),
        ],
        Kind::Break => vec![
            dark(wash),
            dark(strips),
            look(back, 1.0, "Congo"),
            dark(movers),
        ],
        Kind::Bridge => vec![
            look(wash, 0.45, "Purple"),
            look(back, 0.8, "Congo"),
            look(strips, 0.5, "Magenta"),
            look(movers.clone(), 0.8, "Cyan"),
            aim(movers, "Drums"),
        ],
        Kind::Breakdown => vec![
            dark(wash),
            look(key, 0.6, "Cool White"),
            look(back, 0.35, "Deep Blue"),
            dark(strips),
            dark(movers),
        ],
        Kind::Outro => vec![
            look(wash, 0.5, "Cool White"),
            look(back, 0.4, "House Blue"),
            look(strips, 0.3, "Deep Blue"),
            dark(movers),
        ],
        Kind::Other => vec![look(wash, 0.6, "Open White"), look(back, 0.4, "House Blue")],
    }
}

/// How long the move into a section takes, in **beats**.
///
/// Musical rather than seconds so it holds at any tempo: a chorus that
/// snaps in one beat snaps at 200 BPM too. Converted through the tempo
/// map at generate time, because `Cue::fade_secs` is what the fade
/// engine understands.
fn fade_beats(kind: Kind) -> f64 {
    match kind {
        // Arrivals land. A chorus that fades in has already missed.
        Kind::Chorus | Kind::Break => 0.25,
        Kind::PreChorus | Kind::Bridge => 2.0,
        Kind::CountIn => 0.0,
        _ => 4.0,
    }
}

/// Generates a starting cue list for a song.
///
/// One cue per section, positioned at its first bar. Every cue blocks —
/// a section is a complete statement of what the rig is doing, not a
/// delta from whatever came before, because sections get reordered,
/// repeated and skipped and a tracked leftover from a bridge showing up
/// in a chorus is exactly the bug that makes people distrust the desk.
pub fn generate(song: &SongMap, roles: &Roles) -> CueList {
    let cues = song
        .sections
        .iter()
        .map(|section| cue_for(song, section, roles))
        .collect();
    CueList {
        name: song.name.clone(),
        cues,
    }
}

fn cue_for(song: &SongMap, section: &Section, roles: &Roles) -> Cue {
    let kind = kind_of(&section.name);
    let point = song.tempo.at(section.start);
    let fade_secs = (fade_beats(kind) * 60.0 / point.bpm) as f32;
    Cue {
        name: section.name.clone(),
        fade_secs,
        values: Vec::new(),
        recipes: look_for(kind, roles),
        block: true,
        at: Some(section.start),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::{Bars, TempoMap, TimeSignature};

    fn song() -> SongMap {
        SongMap {
            name: "Test".into(),
            tempo: TempoMap::constant(86.28, TimeSignature::default()),
            sections: vec![
                Section {
                    name: "Count-In".into(),
                    start: Bars::bar(1),
                    bars: 2.0,
                },
                Section {
                    name: "VS 1".into(),
                    start: Bars::bar(3),
                    bars: 8.0,
                },
                Section {
                    name: "PRE".into(),
                    start: Bars::bar(11),
                    bars: 4.0,
                },
                Section {
                    name: "CH 1".into(),
                    start: Bars::bar(15),
                    bars: 8.0,
                },
            ],
        }
    }

    /// The names an arranger actually writes, including the ones whose
    /// prefixes collide.
    #[test]
    fn section_names_read_as_parts_of_a_song() {
        for (name, kind) in [
            ("Count-In", Kind::CountIn),
            ("IN A", Kind::Intro),
            ("Intro", Kind::Intro),
            ("VS 1", Kind::Verse),
            ("Verse 2", Kind::Verse),
            ("PRE", Kind::PreChorus),
            ("CH 3", Kind::Chorus),
            ("Chorus", Kind::Chorus),
            ("Break", Kind::Break),
            // The collisions this ordering exists to get right.
            ("Breakdown", Kind::Breakdown),
            ("BR", Kind::Bridge),
            ("Bridge", Kind::Bridge),
            ("Outro", Kind::Outro),
            ("Guitar Solo", Kind::Other),
        ] {
            assert_eq!(kind_of(name), kind, "{name}");
        }
    }

    #[test]
    fn every_section_becomes_a_positioned_cue() {
        let song = song();
        let list = generate(&song, &Roles::default());
        assert_eq!(list.cues.len(), song.sections.len());
        for (cue, section) in list.cues.iter().zip(&song.sections) {
            assert_eq!(cue.name, section.name);
            assert_eq!(cue.at, Some(section.start));
            assert!(cue.block, "a section is a whole statement, not a delta");
            assert!(!cue.recipes.is_empty(), "{} has no look", cue.name);
        }
    }

    /// Fades are authored in beats and converted, so the same show holds
    /// at another tempo.
    #[test]
    fn fade_times_are_musical() {
        let list = generate(&song(), &Roles::default());
        let beat = 60.0 / 86.28;
        let chorus = list.cues.iter().find(|c| c.name == "CH 1").unwrap();
        assert!((chorus.fade_secs as f64 - beat * 0.25).abs() < 1e-4);
        let verse = list.cues.iter().find(|c| c.name == "VS 1").unwrap();
        assert!((verse.fade_secs as f64 - beat * 4.0).abs() < 1e-4);
    }

    /// The generated show is built from recipes, which is what makes it
    /// survive a rig change — no cue should carry a bare channel list.
    #[test]
    fn the_draft_is_recipes_not_channels() {
        let list = generate(&song(), &Roles::default());
        for cue in &list.cues {
            assert!(cue.values.is_empty(), "{} has direct values", cue.name);
            for recipe in &cue.recipes {
                assert!(
                    !matches!(recipe.target, Selection::Chans(_)),
                    "{} targets channels directly",
                    cue.name
                );
            }
        }
    }
}
