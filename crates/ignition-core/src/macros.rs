//! Macros — the two-key move, written down and run with timing.
//!
//! A busking operator has a handful of moves they make every night:
//! the drop (strobe, blinders, movers up, colour hot, four beats, let
//! go), the build into a last chorus, the breakdown, the end. Each is
//! three or four gestures in a particular order at a particular time,
//! and doing them by hand means doing them slightly differently every
//! night. A macro is that list as data in the profile, and this module
//! is what runs it against the programmer on the show clock.
//!
//! The runner is deliberately small: it holds a step index and a time
//! to resume at, and everything a step *does* is a call the programmer
//! already answers — hold a look, move a fader, fire an effect, flash,
//! blackout, release. Nothing here is a new layer; a macro is a hand
//! that does not get tired.
//!
//! The second half of the file is the busking programming the default
//! profile ships — looks, macros, pages, protected roles and speed
//! routing — authored here for the same reason the effects library is
//! authored in Rust: it is worth testing, and it *ships* as data, baked
//! into the profile file by `examples/bake-profile.rs`.

use crate::playbacks::{Class, Playbacks};
use crate::profile::{
    FaderSource, FaderSpec, Look, LookKind, Macro, MacroStep, Page, Param, Profile,
};
use crate::programmer::{AttrFilter, FADERS, Programmer};
use crate::recipe::{Band, Random, Recipe, RecipeApply, RecipeRef, Show};
use crate::selection::Selection;
use crate::step::Speed;
use ignition_proto::Attribute;
use std::collections::BTreeMap;

/// Something a macro asked for that the programmer cannot do itself,
/// handed back to the host — the transmitter switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRequest {
    /// DMX output on or off.
    Output(bool),
}

/// One macro, running.
///
/// Steps up to the first wait land in the frame the macro starts; each
/// wait resumes on the frame its beats have elapsed. One runs at a
/// time — a host that starts another replaces this one, and the new
/// macro's own release is what lets go of anything the old one took.
// r[impl playback.macro-runner]
#[derive(Debug, Clone, PartialEq)]
pub struct MacroRunner {
    pub name: String,
    steps: Vec<MacroStep>,
    next: usize,
    /// The show time the current wait ends, if one is in progress.
    resume_at: Option<f32>,
}

impl MacroRunner {
    pub fn new(name: &str, macro_: &Macro) -> Self {
        Self {
            name: name.to_string(),
            steps: macro_.steps.clone(),
            next: 0,
            resume_at: None,
        }
    }

    /// A profile macro by name, if the profile has it.
    pub fn from_profile(profile: &Profile, name: &str) -> Option<Self> {
        profile.macros.get(name).map(|m| Self::new(name, m))
    }

    /// Whether every step has run.
    pub fn finished(&self) -> bool {
        self.next >= self.steps.len() && self.resume_at.is_none()
    }

    /// The step about to run, for a surface that shows progress.
    pub fn position(&self) -> usize {
        self.next
    }

    /// Runs every step due by now. The clock is the Song playback's
    /// where there is one — a macro fired against a running song is
    /// timed by the song — and the programmer's own otherwise.
    ///
    /// Returns what the host has to carry out.
    // r[impl playback.macro-runner]
    // r[impl profile.macros.beats] - waits resolve against the Song master as they start
    pub fn tick(
        &mut self,
        programmer: &mut Programmer,
        playbacks: &mut Playbacks,
        profile: &Profile,
        show: &Show<'_>,
    ) -> Vec<HostRequest> {
        let now = playbacks
            .of_class(Class::Song)
            .map(|p| p.clock())
            .unwrap_or_else(|| programmer.now());
        let mut requests = Vec::new();
        loop {
            if let Some(at) = self.resume_at {
                if now < at {
                    return requests;
                }
                self.resume_at = None;
            }
            let Some(step) = self.steps.get(self.next).cloned() else {
                return requests;
            };
            self.next += 1;
            match step {
                MacroStep::Look(name) => {
                    let safe = profile
                        .looks
                        .get(&name)
                        .is_some_and(|l| l.kind == LookKind::Safe);
                    programmer.hold_look(profile.look_recipes(&name, show), safe);
                }
                MacroStep::Fader { index, level } => programmer.set_level(index, level),
                MacroStep::Effect { name, level } => {
                    let spec = FaderSpec::new(&name, FaderSource::Effect(name.clone()));
                    let speed = profile.speed_for(&spec, Some(&name));
                    for mut recipe in RecipeRef::named(&name).resolve(show) {
                        recipe.timing.speed = speed.clone();
                        programmer.take_effect(&name, recipe, level);
                    }
                }
                MacroStep::Flash { role, kind } => {
                    if let Some(kind) = crate::profile::bump_kind(&kind) {
                        programmer.flash(Selection::Role(role), kind, now);
                    }
                }
                MacroStep::Wait { beats } => {
                    let bps = Speed::Master("Song".into()).beats_per_second(show.speeds);
                    let bps = if bps > 0.0 {
                        bps
                    } else {
                        Speed::FALLBACK_BPM / 60.0
                    };
                    self.resume_at = Some(now + beats.max(0.0) / bps);
                }
                MacroStep::Release => programmer.release_macro(),
                MacroStep::Blackout => programmer.set_blackout(true),
                MacroStep::Output(on) => requests.push(HostRequest::Output(on)),
            }
        }
    }
}

// ── the shipped busking programming ─────────────────────────────────

/// What the default profile ships beyond its roles and effects.
#[derive(Debug, Clone, Default)]
pub struct Busking {
    pub looks: BTreeMap<String, Look>,
    pub macros: BTreeMap<String, Macro>,
    pub pages: Vec<Page>,
    pub protected: Vec<String>,
    pub speed_routing: BTreeMap<String, Speed>,
}

/// The default profile's busking programming.
// r[impl profile.looks]
// r[impl profile.macros]
// r[impl profile.pages]
// r[impl profile.protected-roles]
// r[impl profile.speed-routing]
pub fn shipped() -> Busking {
    Busking {
        looks: looks(),
        macros: macros(),
        pages: pages(),
        protected: vec!["House Lights".into()],
        speed_routing: speed_routing(),
    }
}

/// A profile carrying the shipped library *and* the shipped busking
/// programming — what the studio resolves its bank and its macros
/// through, and what the bake writes out.
pub fn shipped_profile() -> Profile {
    let busking = shipped();
    Profile {
        name: "ignition".into(),
        effects: crate::effects::library(),
        effect_notes: crate::effects::notes(),
        bundles: crate::effects::bundles(),
        looks: busking.looks,
        macros: busking.macros,
        pages: busking.pages,
        protected: busking.protected,
        speed_routing: busking.speed_routing,
        ..Default::default()
    }
}

/// Movement halves the tap, beams double it, and everything that is a
/// level or a colour follows the song.
// r[impl profile.speed-routing]
pub fn speed_routing() -> BTreeMap<String, Speed> {
    let tap = |scale: f32| Speed::Scaled {
        master: "Tap".into(),
        scale,
    };
    let song = || Speed::Master("Song".into());
    BTreeMap::from([
        ("movement".to_string(), tap(0.5)),
        ("beam".to_string(), tap(2.0)),
        ("intensity".to_string(), song()),
        ("colour".to_string(), song()),
        ("strip".to_string(), song()),
        ("one-shot".to_string(), song()),
    ])
}

fn role(name: &str) -> Selection {
    Selection::Role(name.to_string())
}

/// The whole stage, as the roles that light it — not `Audience`, which
/// a drop should leave alone, and never the house.
fn stage() -> Selection {
    Selection::Union(vec![
        role("Key"),
        role("Wash"),
        role("Back"),
        role("Movers"),
        role("Bars"),
        role("Beams"),
    ])
}

/// A static look: a level and a colour on a role.
fn level_and_colour(target: Selection, colour: &str, level: f32) -> Recipe {
    let mut recipe = Recipe::new(target, RecipeApply::Dimmer(level));
    recipe.steps[0]
        .apply
        .push(RecipeApply::Color(crate::preset::Ref::Named(
            colour.to_string(),
        )));
    recipe
}

fn inline(recipe: Recipe) -> RecipeRef {
    RecipeRef::Inline(recipe)
}

fn look(kind: LookKind, about: &str, recipes: Vec<RecipeRef>) -> Look {
    Look {
        kind,
        about: about.into(),
        recipes,
    }
}

// r[impl profile.looks]
// r[impl profile.looks.static]
pub fn looks() -> BTreeMap<String, Look> {
    BTreeMap::from([
        (
            "verse bed".to_string(),
            look(
                LookKind::Bed,
                "faces warm at a working level, the wash breathing under the vocal",
                vec![
                    inline(level_and_colour(role("Key"), "Warm White", 0.7)),
                    inline(level_and_colour(role("Back"), "Deep", 0.4)),
                    RecipeRef::Bundle {
                        bundle: "verse bed".into(),
                        target: None,
                    },
                ],
            ),
        ),
        (
            "chorus full".to_string(),
            look(
                LookKind::Full,
                "everything up: faces full, the wash hot, the back deep, the rig driving",
                vec![
                    inline(level_and_colour(role("Key"), "Warm White", 1.0)),
                    inline(level_and_colour(role("Wash"), "Hot", 1.0)),
                    inline(level_and_colour(role("Back"), "Deep", 1.0)),
                    inline(level_and_colour(role("Bars"), "Hot", 1.0)),
                    RecipeRef::Bundle {
                        bundle: "chorus drive".into(),
                        target: None,
                    },
                ],
            ),
        ),
        (
            "punt".to_string(),
            look(
                LookKind::Punt,
                "faces lit, warm, nothing moving — the state a stage can always be dropped into",
                vec![inline(level_and_colour(
                    Selection::Union(vec![role("Key"), role("Wash"), role("Back")]),
                    "Warm White",
                    0.6,
                ))],
            ),
        ),
        (
            "blackout".to_string(),
            look(
                LookKind::Safe,
                "the stage to zero; the house stays up",
                vec![inline(Recipe::new(stage(), RecipeApply::Dimmer(0.0)))],
            ),
        ),
    ])
}

fn macro_(about: &str, steps: Vec<MacroStep>) -> Macro {
    Macro {
        about: about.into(),
        steps,
    }
}

fn effect(name: &str, level: f32) -> MacroStep {
    MacroStep::Effect {
        name: name.into(),
        level,
    }
}

// r[impl profile.macros]
pub fn macros() -> BTreeMap<String, Macro> {
    BTreeMap::from([
        (
            "drop".to_string(),
            macro_(
                "the drop: strobe burst, blinders, movers fly out, colour hot; four beats, then let go",
                vec![
                    effect("strobe burst", 1.0),
                    MacroStep::Flash {
                        role: "Audience".into(),
                        kind: "white".into(),
                    },
                    effect("fly out", 1.0),
                    MacroStep::Look("chorus full".into()),
                    MacroStep::Wait { beats: 4.0 },
                    MacroStep::Release,
                ],
            ),
        ),
        (
            "build 8".to_string(),
            macro_(
                "the riser into a last chorus: the rig builds light by light under a strobe climbing over eight bars",
                vec![
                    effect("rig build", 1.0),
                    effect("strobe riser", 1.0),
                    MacroStep::Wait { beats: 32.0 },
                    MacroStep::Release,
                ],
            ),
        ),
        (
            "breakdown".to_string(),
            macro_(
                "down to the verse bed, the movers fly out, a little sparkle",
                vec![
                    MacroStep::Look("verse bed".into()),
                    effect("fly out", 1.0),
                    effect("sparkle", 0.3),
                ],
            ),
        ),
        (
            "end".to_string(),
            macro_(
                "two beats, then black",
                vec![MacroStep::Wait { beats: 2.0 }, MacroStep::Blackout],
            ),
        ),
    ])
}

fn fx(label: &str, name: &str) -> FaderSpec {
    FaderSpec::new(label, FaderSource::Effect(name.into()))
}

fn static_(label: &str, target: Selection, colour: &str, level: f32) -> FaderSpec {
    FaderSpec::new(
        label,
        FaderSource::Inline(level_and_colour(target, colour, level)),
    )
}

/// A role's attribute driven by a sound band — see the sound page.
// r[impl playback.sound-as-value] - a band level as a fader's recipe
fn sound(
    label: &str,
    target: Selection,
    band: Band,
    low: f32,
    high: f32,
    relative: bool,
) -> FaderSpec {
    FaderSpec::new(
        label,
        FaderSource::Inline(Recipe::new(
            target,
            RecipeApply::Sound {
                band,
                attr: Attribute::Dimmer,
                low,
                high,
                relative,
            },
        )),
    )
}

/// A random twinkle on the wash whose *range* is a band's level.
// r[impl playback.sound-as-value] - a generator's range
fn sparkle_on(label: &str, band: Band) -> FaderSpec {
    let mut recipe = Recipe::new(
        role("Wash"),
        RecipeApply::Random(Random {
            attr: Attribute::Dimmer,
            low: 0.0,
            high: 1.0,
            high_from_band: Some(band),
            ..Random::default()
        }),
    );
    recipe.timing.speed = Speed::Master("Song".into());
    FaderSpec::new(label, FaderSource::Inline(recipe))
}

fn page(name: &str, faders: Vec<FaderSpec>) -> Page {
    let faders: [FaderSpec; FADERS] = faders
        .try_into()
        .unwrap_or_else(|v: Vec<FaderSpec>| panic!("page {name:?} has {} faders", v.len()));
    Page {
        name: name.into(),
        faders,
    }
}

/// The four pages: busking, movement, sound, colour. The slot families
/// stay put across pages — fader four is the movers on every page that
/// has movers — so a hand that has learned the shape is still right
/// after a page turn.
// r[impl profile.pages]
// r[impl playback.pages]
pub fn pages() -> Vec<Page> {
    let tap2 = Speed::Scaled {
        master: "Tap".into(),
        scale: 2.0,
    };
    vec![
        page(
            "busking",
            vec![
                static_("KEY", role("Key"), "Warm White", 1.0),
                fx("CHASE", "chase").with(Param::new("depth", 0.0, 1.0, 1.0)),
                fx("SPARKLE", "sparkle").with(Param::new("depth", 0.0, 1.0, 1.0)),
                fx("BALLY", "ballyhoo"),
                fx("BARS", "strip chase"),
                fx("BACK", "back breathe").with(Param::new("bars", 1.0, 8.0, 2.0)),
                fx("STROBE", "strobe")
                    .at(tap2)
                    .with(Param::new("duty", 0.05, 0.95, 0.5)),
                static_("BLIND", role("Audience"), "Open White", 1.0),
            ],
        ),
        page(
            "movement",
            vec![
                fx("WINDMILL", "windmill").filtered(AttrFilter::POSITION),
                fx("PAN WAVE", "pan wave").filtered(AttrFilter::POSITION),
                fx("2 COLOUR", "two colour chase"),
                fx("RAINBOW", "rainbow"),
                fx("ZOOM", "zoom pulse"),
                fx("IRIS", "iris chase"),
                fx("STRIPS", "strip chase"),
                fx("BLINDERS", "blinder chase"),
            ],
        ),
        page(
            "sound",
            vec![
                sound("KEY MID", role("Key"), Band::Mid, 0.0, 0.3, true),
                sound("WASH MID", role("Wash"), Band::Mid, 0.0, 0.5, true),
                sparkle_on("SPARK HI", Band::High),
                sound("MOVE MID", role("Movers"), Band::Mid, 0.0, 1.0, false),
                sound("BARS HI", role("Bars"), Band::High, 0.0, 1.0, false),
                sound("BACK LOW", role("Back"), Band::Low, 0.1, 1.0, false),
                sound("BEAM LOW", role("Beams"), Band::Low, 0.0, 1.0, false),
                sound("BLND LOW", role("Audience"), Band::Low, 0.0, 1.0, false),
            ],
        ),
        page(
            "colour",
            vec![
                fx("2 COLOUR", "two colour chase").filtered(AttrFilter::COLOUR),
                fx("RAINBOW", "rainbow").filtered(AttrFilter::COLOUR),
                fx("WIPE", "colour wipe").filtered(AttrFilter::COLOUR),
                fx("FIRE", "colour fire").filtered(AttrFilter::COLOUR),
                fx("WARM", "warm chase").filtered(AttrFilter::COLOUR),
                fx("SAT", "saturation breathe").filtered(AttrFilter::COLOUR),
                fx("WHITE", "white pop"),
                fx("CLR BUMP", "colour bump"),
            ],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cue::CuePlayer;
    use crate::group::Group;
    use crate::profile::Bundle;
    use crate::selection::EMPTY_RIG;
    use crate::step::{SpeedMasters, Step, Timing};
    use ignition_proto::ChanId;
    use std::collections::HashMap;

    /// One group, bound as every role a test names.
    struct Roles;
    impl crate::selection::Roles for Roles {
        fn role(&self, _: &str) -> Option<&Selection> {
            static PARS: std::sync::LazyLock<Selection> =
                std::sync::LazyLock::new(|| Selection::Group("Pars".into()));
            Some(&PARS)
        }
    }

    fn groups() -> Vec<Group> {
        vec![Group {
            name: "Pars".into(),
            chans: vec![1, 2],
        }]
    }

    fn dim(level: f32) -> Recipe {
        Recipe::new(role("Wash"), RecipeApply::Dimmer(level))
    }

    fn profile() -> Profile {
        let mut library = BTreeMap::new();
        library.insert("pulse".to_string(), {
            let mut r = Recipe {
                target: role("Wash"),
                steps: vec![
                    Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.5)])]),
                    Step::new(vec![RecipeApply::Delta(vec![(Attribute::Dimmer, 0.0)])]),
                ],
                timing: Timing::default(),
                ..Default::default()
            };
            r.timing.speed = Speed::Bpm(60.0);
            r
        });
        let mut notes = BTreeMap::new();
        notes.insert(
            "pulse".to_string(),
            crate::profile::EffectNote {
                family: "movement".into(),
                about: String::new(),
            },
        );
        let mut looks = BTreeMap::new();
        looks.insert(
            "bed".to_string(),
            look(LookKind::Bed, "", vec![inline(dim(0.4))]),
        );
        looks.insert(
            "black".to_string(),
            look(LookKind::Safe, "", vec![inline(dim(0.0))]),
        );
        let mut macros = BTreeMap::new();
        macros.insert(
            "hit".to_string(),
            macro_(
                "",
                vec![
                    MacroStep::Look("bed".into()),
                    effect("pulse", 1.0),
                    MacroStep::Wait { beats: 2.0 },
                    MacroStep::Output(false),
                    MacroStep::Release,
                ],
            ),
        );
        Profile {
            effects: library,
            effect_notes: notes,
            looks,
            macros,
            speed_routing: speed_routing(),
            ..Default::default()
        }
    }

    fn output(p: &Programmer, show: &Show<'_>, secs: f32) -> HashMap<(ChanId, Attribute), f32> {
        let mut out = HashMap::new();
        out.insert((1, Attribute::Dimmer), 1.0);
        out.insert((2, Attribute::Dimmer), 1.0);
        p.apply_to(&mut out, show, secs);
        out
    }

    /// r[verify playback.macro-runner]
    /// r[verify profile.macros.beats]
    /// r[verify profile.macros.release]
    /// r[verify playback.macro-effects]
    #[test]
    fn a_macro_runs_to_its_wait_then_resumes_on_the_beat_and_releases() {
        let groups = groups();
        let profile = profile();
        let mut speeds = SpeedMasters::new();
        speeds.insert("Song".into(), 120.0); // two beats per second
        let show = Show {
            speeds: &speeds,
            roles: &Roles,
            library: &profile.effects,
            ..Show::new(&groups, &EMPTY_RIG)
        };
        let mut programmer = Programmer::new();
        let mut playbacks = Playbacks::new();
        playbacks.push(Class::Song, CuePlayer::new(Vec::new()));
        playbacks.set_clock(10.0);
        programmer.set_now(10.0);

        let mut runner = MacroRunner::from_profile(&profile, "hit").unwrap();
        let requests = runner.tick(&mut programmer, &mut playbacks, &profile, &show);
        assert!(requests.is_empty());
        assert!(programmer.is_holding(), "the look is held");
        assert_eq!(programmer.effects_playing(), vec!["pulse"]);
        assert!(!runner.finished(), "waiting");
        // The held look lands over the macro layer, absolute: the bed
        // is 0.4 whatever the pulse beneath it is doing.
        let out = output(&programmer, &show, 10.0);
        assert!((out[&(1, Attribute::Dimmer)] - 0.4).abs() < 0.01, "{out:?}");

        // Half a second in: two beats at 120 is one second, not yet.
        playbacks.set_clock(10.5);
        assert!(
            runner
                .tick(&mut programmer, &mut playbacks, &profile, &show)
                .is_empty()
        );
        assert!(programmer.is_holding());

        // The beat lands: output request returned, everything released.
        playbacks.set_clock(11.0);
        let requests = runner.tick(&mut programmer, &mut playbacks, &profile, &show);
        assert_eq!(requests, vec![HostRequest::Output(false)]);
        assert!(runner.finished());
        assert!(!programmer.is_holding());
        assert!(programmer.effects_playing().is_empty());
        let out = output(&programmer, &show, 11.0);
        assert_eq!(
            out[&(1, Attribute::Dimmer)],
            1.0,
            "what was beneath shows through"
        );
    }

    /// An unset Song master waits at the fallback tempo rather than
    /// forever.
    /// r[verify profile.macros.beats]
    #[test]
    fn a_wait_with_no_song_master_uses_the_fallback_tempo() {
        let groups = groups();
        let mut profile = profile();
        profile.macros.insert(
            "end".into(),
            macro_(
                "",
                vec![MacroStep::Wait { beats: 2.0 }, MacroStep::Blackout],
            ),
        );
        let show = Show {
            roles: &Roles,
            ..Show::new(&groups, &EMPTY_RIG)
        };
        let mut programmer = Programmer::new();
        let mut playbacks = Playbacks::new();
        let mut runner = MacroRunner::from_profile(&profile, "end").unwrap();
        runner.tick(&mut programmer, &mut playbacks, &profile, &show);
        assert!(!programmer.blackout);
        // Two beats at 120 is one second.
        programmer.set_now(0.99);
        runner.tick(&mut programmer, &mut playbacks, &profile, &show);
        assert!(!programmer.blackout);
        programmer.set_now(1.0);
        runner.tick(&mut programmer, &mut playbacks, &profile, &show);
        assert!(programmer.blackout);
        assert!(runner.finished());
    }

    /// A macro's effect takes the family's routed speed, so a mover
    /// effect fired by a macro halves the tap like one on a fader.
    /// r[verify profile.speed-routing]
    #[test]
    fn a_macro_effect_is_routed_by_family() {
        let profile = profile();
        let spec = FaderSpec::new("x", FaderSource::Effect("pulse".into()));
        assert_eq!(
            profile.speed_for(&spec, Some("pulse")),
            Speed::Scaled {
                master: "Tap".into(),
                scale: 0.5
            }
        );
        let own = spec.at(Speed::Master("Rate".into()));
        assert_eq!(
            profile.speed_for(&own, Some("pulse")),
            Speed::Master("Rate".into()),
            "the fader's own speed wins"
        );
        let unknown = FaderSpec::new("y", FaderSource::Effect("nope".into()));
        assert_eq!(
            profile.speed_for(&unknown, Some("nope")),
            Speed::Master("Song".into())
        );
    }

    /// r[verify playback.look-hold]
    #[test]
    fn a_safe_look_is_held_as_safe_and_a_bed_is_not() {
        let groups = groups();
        let profile = profile();
        let show = Show {
            roles: &Roles,
            ..Show::new(&groups, &EMPTY_RIG)
        };
        let mut programmer = Programmer::new();
        programmer.protected = vec!["House".into()];
        let mut playbacks = Playbacks::new();
        let mut p = profile.clone();
        p.macros.insert(
            "b".into(),
            macro_("", vec![MacroStep::Look("black".into())]),
        );
        let mut runner = MacroRunner::from_profile(&p, "b").unwrap();
        runner.tick(&mut programmer, &mut playbacks, &p, &show);
        // Every role resolves to the pars here, so the protected role
        // covers them and the safe look leaves them alone.
        let out = output(&programmer, &show, 0.0);
        assert_eq!(out[&(1, Attribute::Dimmer)], 1.0);
        let mut p2 = profile.clone();
        p2.macros
            .insert("d".into(), macro_("", vec![MacroStep::Look("bed".into())]));
        let mut runner = MacroRunner::from_profile(&p2, "d").unwrap();
        runner.tick(&mut programmer, &mut playbacks, &p2, &show);
        let out = output(&programmer, &show, 0.0);
        assert!(
            (out[&(1, Attribute::Dimmer)] - 0.4).abs() < 0.01,
            "a bed is not safe"
        );
    }

    /// A step naming a bump kind the desk does not have flashes
    /// nothing rather than something.
    #[test]
    fn an_unknown_bump_label_is_skipped() {
        assert_eq!(
            crate::profile::bump_kind("white"),
            Some(crate::bump::Kind::White)
        );
        assert_eq!(
            crate::profile::bump_kind("BURST"),
            Some(crate::bump::Kind::Burst)
        );
        assert_eq!(crate::profile::bump_kind("sideways"), None);
    }

    /// A page fader on a bundle plays every member at one level.
    /// r[verify profile.pages]
    #[test]
    fn a_bundle_fader_plays_every_member() {
        let groups = groups();
        let mut profile = profile();
        profile.effects.insert("two".into(), dim(0.2));
        profile.bundles.insert(
            "both".into(),
            Bundle {
                name: "both".into(),
                family: "verse".into(),
                about: String::new(),
                recipes: vec!["pulse".into(), "two".into()],
            },
        );
        let show = Show {
            roles: &Roles,
            library: &profile.effects,
            bundles: &profile.bundles,
            ..Show::new(&groups, &EMPTY_RIG)
        };
        let spec = FaderSpec::new("BOTH", FaderSource::Bundle("both".into()));
        let fader = profile.resolve_fader(&spec, &show);
        assert_eq!(fader.recipes().count(), 2);
        assert_eq!(fader.name, "BOTH");
        // A look fader likewise; a master fader carries the role.
        let bed = profile.resolve_fader(
            &FaderSpec::new("BED", FaderSource::Look("bed".into())),
            &show,
        );
        assert_eq!(bed.recipes().count(), 1);
        let master = profile.resolve_fader(
            &FaderSpec::new("MOVERS", FaderSource::Master("Movers".into())),
            &show,
        );
        assert_eq!(master.master.as_deref(), Some("Movers"));
        assert!(master.recipe.is_none());
        // Declared params seed the fader.
        let with = profile.resolve_fader(
            &FaderSpec::new("P", FaderSource::Effect("pulse".into()))
                .with(Param::new("depth", 0.0, 1.0, 0.5)),
            &show,
        );
        assert_eq!(with.params.get("depth"), Some(&0.5));
    }

    // ----- the shipped programming -----

    fn shipped_show<'a>(profile: &'a Profile, groups: &'a [Group]) -> Show<'a> {
        Show {
            library: &profile.effects,
            bundles: &profile.bundles,
            ..Show::new(groups, &EMPTY_RIG)
        }
    }

    /// r[verify profile.looks]
    /// r[verify profile.looks.static]
    #[test]
    fn the_shipped_looks_resolve_and_carry_the_four_kinds() {
        let profile = shipped_profile();
        let groups = groups();
        let show = shipped_show(&profile, &groups);
        for (name, kind) in [
            ("verse bed", LookKind::Bed),
            ("chorus full", LookKind::Full),
            ("punt", LookKind::Punt),
            ("blackout", LookKind::Safe),
        ] {
            let look = profile.looks.get(name).expect(name);
            assert_eq!(look.kind, kind);
            assert!(!look.about.is_empty());
            assert!(
                !profile.look_recipes(name, &show).is_empty(),
                "{name} resolves"
            );
            for r in &look.recipes {
                assert!(
                    r.missing(&show).is_empty(),
                    "{name}: {:?}",
                    r.missing(&show)
                );
            }
        }
        // The punt and the blackout are static: one step, nothing moving.
        for name in ["punt", "blackout"] {
            for recipe in profile.look_recipes(name, &show) {
                assert_eq!(recipe.steps.len(), 1, "{name} is static");
            }
        }
    }

    /// r[verify profile.macros]
    #[test]
    fn the_shipped_macros_name_only_effects_and_looks_that_exist() {
        let profile = shipped_profile();
        for name in ["drop", "build 8", "breakdown", "end"] {
            let m = profile.macros.get(name).expect(name);
            assert!(!m.about.is_empty());
            for step in &m.steps {
                match step {
                    MacroStep::Look(l) => assert!(profile.looks.contains_key(l), "{name}: {l}"),
                    MacroStep::Effect { name: e, .. } => {
                        assert!(profile.effects.contains_key(e), "{name}: {e}")
                    }
                    MacroStep::Flash { kind, .. } => {
                        assert!(crate::profile::bump_kind(kind).is_some(), "{name}: {kind}")
                    }
                    _ => {}
                }
            }
        }
        // The drop lets go; the end goes to black.
        assert!(matches!(
            profile.macros["drop"].steps.last(),
            Some(MacroStep::Release)
        ));
        assert!(matches!(
            profile.macros["end"].steps.last(),
            Some(MacroStep::Blackout)
        ));
    }

    /// r[verify profile.pages]
    /// r[verify profile.pages.label-fits]
    /// r[verify profile.attribute-filter]
    #[test]
    fn the_shipped_pages_are_four_of_eight_and_every_fader_resolves() {
        let profile = shipped_profile();
        let groups = groups();
        let show = shipped_show(&profile, &groups);
        let names: Vec<&str> = profile.pages.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["busking", "movement", "sound", "colour"]);
        for page in &profile.pages {
            for spec in &page.faders {
                assert!(spec.label.len() <= 8, "{:?} will wrap", spec.label);
                let fader = profile.resolve_fader(spec, &show);
                assert!(
                    fader.recipe.is_some() || fader.master.is_some(),
                    "{}/{} resolves to nothing",
                    page.name,
                    spec.label
                );
            }
        }
        // The colour page is filtered to colour where it chases colour.
        let colour = &profile.pages[3];
        assert_eq!(colour.faders[0].filter, AttrFilter::COLOUR);
        // The strobe runs double against the tap.
        assert_eq!(
            profile.pages[0].faders[6].speed,
            Some(Speed::Scaled {
                master: "Tap".into(),
                scale: 2.0
            })
        );
    }

    /// r[verify profile.protected-roles]
    /// r[verify profile.speed-routing]
    #[test]
    fn the_shipped_protection_and_routing() {
        let b = shipped();
        assert_eq!(b.protected, vec!["House Lights".to_string()]);
        assert_eq!(
            b.speed_routing["movement"],
            Speed::Scaled {
                master: "Tap".into(),
                scale: 0.5
            }
        );
        assert_eq!(
            b.speed_routing["beam"],
            Speed::Scaled {
                master: "Tap".into(),
                scale: 2.0
            }
        );
        for family in ["intensity", "colour"] {
            assert_eq!(b.speed_routing[family], Speed::Master("Song".into()));
        }
        for family in crate::effects::FAMILIES {
            assert!(b.speed_routing.contains_key(family), "{family} is routed");
        }
    }

    /// The shipped programming survives a round trip through JSON, as
    /// the bake needs it to.
    #[test]
    fn the_shipped_programming_round_trips() {
        let profile = shipped_profile();
        let text = serde_json::to_string(&profile).unwrap();
        let back = Profile::parse(&text).unwrap();
        assert_eq!(back.looks, profile.looks);
        assert_eq!(back.macros, profile.macros);
        assert_eq!(back.pages, profile.pages);
        assert_eq!(back.protected, profile.protected);
        assert_eq!(back.speed_routing, profile.speed_routing);
    }
}

/// The protected role, from the show's side: a cue that takes the
/// `Safe` look and the `end` macro leave a bound house light where the
/// show put it.
#[cfg(test)]
mod protected_from_the_show {
    use super::*;
    use crate::cue::{Cue, CuePlayer};
    use crate::group::Group;
    use crate::selection::EMPTY_RIG;
    use ignition_proto::ChanId;
    use std::collections::HashMap;

    /// The house on its own group; every other role on the pars.
    struct Bound;
    impl crate::selection::Roles for Bound {
        fn role(&self, name: &str) -> Option<&Selection> {
            static PARS: std::sync::LazyLock<Selection> =
                std::sync::LazyLock::new(|| Selection::Group("Pars".into()));
            static HOUSE: std::sync::LazyLock<Selection> =
                std::sync::LazyLock::new(|| Selection::Group("House".into()));
            Some(if name.eq_ignore_ascii_case("House Lights") {
                &HOUSE
            } else {
                &PARS
            })
        }
    }

    fn groups() -> Vec<Group> {
        vec![
            Group {
                name: "Pars".into(),
                chans: vec![1, 2],
            },
            Group {
                name: "House".into(),
                chans: vec![9],
            },
        ]
    }

    fn lit(target: &str, level: f32) -> RecipeRef {
        RecipeRef::Inline(Recipe::new(role(target), RecipeApply::Dimmer(level)))
    }

    fn roles_in(selection: &Selection, out: &mut Vec<String>) {
        match selection {
            Selection::Role(name) => out.push(name.clone()),
            Selection::Union(items) | Selection::Intersect(items) => {
                items.iter().for_each(|s| roles_in(s, out))
            }
            Selection::Except { of, minus } => {
                roles_in(of, out);
                roles_in(minus, out);
            }
            Selection::Where { of, .. } | Selection::Order { of, .. } => roles_in(of, out),
            _ => {}
        }
    }

    /// r[verify profile.protected-roles]
    /// r[verify playback.protected-untouched]
    /// r[verify profile.looks]
    #[test]
    fn a_safe_look_taken_by_a_cue_and_the_end_macro_leave_the_house_alone() {
        let profile = shipped_profile();
        assert!(profile.is_protected("House Lights"));
        // No shipped look names a protected role: that is what makes a
        // `Safe` look safe from the show's side, where no programmer
        // stands between the cue and the output.
        for (name, look) in &profile.looks {
            let mut used = Vec::new();
            for r in &look.recipes {
                if let RecipeRef::Inline(recipe) = r {
                    roles_in(&recipe.target, &mut used);
                }
            }
            for used in used {
                assert!(
                    !profile.is_protected(&used),
                    "look {name:?} touches {used:?}"
                );
            }
        }

        let groups = groups();
        let show = Show {
            roles: &Bound,
            library: &profile.effects,
            bundles: &profile.bundles,
            looks: &profile.looks,
            ..Show::new(&groups, &EMPTY_RIG)
        };
        // The count-in sets the house; the break takes the blackout look.
        let mut player = CuePlayer::new(vec![
            Cue {
                name: "Count-In".into(),
                recipes: vec![lit("House Lights", 0.3), lit("Wash", 1.0)],
                ..Default::default()
            },
            Cue {
                name: "Break".into(),
                recipes: vec![RecipeRef::look("blackout")],
                ..Default::default()
            },
        ]);
        player.go(&show);
        player.go(&show);
        let out = player.output(&show);
        assert_eq!(out[&(1, Attribute::Dimmer)], 0.0, "the stage went to black");
        assert!((out[&(9, Attribute::Dimmer)] - 0.3).abs() < 1e-6, "{out:?}");

        // The end macro's blackout, folded on top, leaves it too.
        let mut programmer = Programmer::new();
        programmer.protected = profile.protected.clone();
        let mut playbacks = Playbacks::new();
        let mut runner = MacroRunner::from_profile(&profile, "end").expect("the end macro");
        runner.tick(&mut programmer, &mut playbacks, &profile, &show);
        assert!(!runner.finished(), "two beats first");
        programmer.set_now(10.0);
        runner.tick(&mut programmer, &mut playbacks, &profile, &show);
        assert!(runner.finished());
        let mut folded: HashMap<(ChanId, Attribute), f32> = out.clone();
        programmer.apply_to(&mut folded, &show, 30.0);
        assert!(
            (folded[&(9, Attribute::Dimmer)] - 0.3).abs() < 1e-6,
            "{folded:?}"
        );
        // …and a lit stage under the same blackout does go.
        let mut stage: HashMap<(ChanId, Attribute), f32> = HashMap::new();
        stage.insert((1, Attribute::Dimmer), 1.0);
        stage.insert((9, Attribute::Dimmer), 0.3);
        programmer.apply_to(&mut stage, &show, 30.0);
        assert_eq!(stage[&(1, Attribute::Dimmer)], 0.0);
        assert!((stage[&(9, Attribute::Dimmer)] - 0.3).abs() < 1e-6);
    }
}
