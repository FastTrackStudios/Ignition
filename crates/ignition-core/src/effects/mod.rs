//! The default effects library.
//!
//! Named recipes written against **roles**, so every one of them resolves
//! at any venue implementing the profile. This is what the whole
//! Profile/Venue/Trick apparatus was for: a programmer picks `chase` and
//! `Wash`, and it works in Norco, Riverside and a room nobody has built
//! yet.
//!
//! Three rules run through all of it, and they are what make these
//! *layerable* rather than a list of looks:
//!
//! **Relative wherever possible.** A `Delta` says how much to add or take
//! away and leaves everything it did not name alone — so a chase runs
//! over whatever colour the cue set, instead of replacing it. An
//! absolute effect is a look; a relative one is an effect.
//!
//! **Slaved to `Song`.** Every rate is in bars, against the song's own
//! tempo master, so "one cycle per bar" is one cycle per bar of *this*
//! song and a tempo change carries the whole library with it.
//!
//! **Direction comes from the selection.** Nothing here says "left to
//! right"; that is an X-ordered selection, and the same effect reads as a
//! direction because the *selection* knows the room.
//!
//! The library is **curated**, not exhaustive. Every entry is a look an
//! LD would recognise from a console catalogue — Eos's circle, figure 8,
//! spiral and ballyhoo; Avolites' dimmer, pan, tilt and colour shapes;
//! Hog's sine, step, saw, ramp and random tables; MagicQ's pandim,
//! tiltdim and 2col; WLED's comet, scanner, meteor and twinkle — or from
//! plain practice. Two entries that differ only in rate or size are one
//! entry, because rate and size are live controls on the programmer
//! (`r[effects.live-control-on-programmer]`), and one that would not read
//! on a stage is not here at all.
//!
//! Every entry carries a one-line note — its family and what it looks
//! like and where it belongs — which ships in the profile beside the
//! recipe so a picker can show it. See [`catalogue`].
//!
//! Built in Rust rather than hand-written as JSON because a hundred
//! recipes of nested step tables is a lot of punctuation to get right by
//! eye, and because these are worth testing. They ship *as* data — see
//! the `effects` and `effect_notes` maps on `Profile` — so a venue or a
//! person can add their own without touching this.

use crate::Attribute;
use crate::profile::{Bundle, EffectNote};
use crate::recipe::{Recipe, RecipeApply};
use crate::selection::Selection;
use crate::step::{Play, Speed, Step, Timing, Waveform};
use crate::tricks::Trick;
use std::collections::BTreeMap;

// The library, by family. Each module adds its effects through the same
// `add` so the whole thing is one list keyed by the name a programmer
// types; the split is so a family can be read — and grown — on its own.
mod beam;
mod colour;
mod intensity;
mod movement;
mod oneshot;
mod strip;

/// The six families, and the only six. A picker groups by these.
pub const FAMILIES: [&str; 6] = [
    "intensity",
    "movement",
    "colour",
    "beam",
    "strip",
    "one-shot",
];

/// The sink every family adds through: name, family, note, recipe.
pub(super) type Add<'a> = &'a mut dyn FnMut(&str, &str, &str, Recipe);

/// The role an effect is written against, as a selection.
// r[impl effects.library.roles-only]
pub(super) fn role(name: &str) -> Selection {
    Selection::Role(name.into())
}

/// A timing slaved to the song, `bars` per cycle.
// r[impl recipes.timing-in-musical-terms]
// r[impl effects.measure] - authored in bars
// r[impl effects.masters.song] - the library is written against Song
pub(super) fn beat(bars: f32, spread: f32, direction: Play) -> Timing {
    Timing {
        speed: Speed::Master("Song".into()),
        // `measure` is in beats, and everything here is authored in bars,
        // because a lighting idea is "once a bar" or "twice a bar" and
        // never "every 3.7 beats".
        measure: bars * 4.0,
        phase_spread_deg: spread,
        direction,
        ..Default::default()
    }
}

/// The same, but slaved to the **tap** master rather than the song.
///
/// Busking, as opposed to playback. The song master is right when a
/// track is running and useless when one is not — a support act, a
/// change-over, a worship set nobody sequenced — and an operator tapping
/// four times is the oldest tempo source there is.
///
/// Deliberately the same recipes with a different rate source rather
/// than a second library. An effect that behaved differently depending
/// on where its tempo came from would be two effects wearing one name.
// r[impl effects.masters.tap] - the same recipes slaved to Tap
pub(super) fn tapped(bars: f32, spread: f32, direction: Play) -> Timing {
    Timing {
        speed: Speed::Master("Tap".into()),
        ..beat(bars, spread, direction)
    }
}

/// Timing that runs the step list once and holds its last step.
// r[impl effects.once]
// r[impl recipes.one-shot]
pub(super) fn once(bars: f32, spread: f32, direction: Play) -> Timing {
    Timing {
        once: true,
        ..beat(bars, spread, direction)
    }
}

/// A step setting one relative attribute.
// r[impl effects.modulates-with-delta]
pub(super) fn delta(attr: Attribute, v: f32) -> Step {
    Step::new(vec![RecipeApply::Delta(vec![(attr, v)])])
}

/// A parametric position table — the generator most mover effects are.
///
/// Both axes in **one** recipe. The first version of this library shipped
/// `circle pan` and `circle tilt` as a pair, which is how a console with
/// only per-attribute effects has to do it — and it is a bad deal here:
/// a programmer picks two things that are meaningless apart, and losing
/// one silently turns a circle into a diagonal sweep that looks
/// deliberate enough that nobody notices. A step table can hold Pan and
/// Tilt together, so the shape is one object.
///
/// `pan_cycles` and `tilt_cycles` are how many times each axis goes
/// round per cycle of the effect, which is the whole vocabulary:
///
/// - 1 and 1, a quarter out of phase → a **circle**
/// - 1 and 2 → a **figure of eight**
/// - 3 and 2 → a **ballyhoo**, the classic excitement move, because the
///   ratio does not resolve quickly and the beam never quite repeats
/// - 1 and 0 → a flat **sweep**
///
/// `resolution` is how many steps the curve is drawn with. Sixteen is
/// smooth enough that the interpolation between steps does the rest, and
/// small enough that the table stays readable in the profile.
pub(super) fn orbit(
    pan_amp: f32,
    tilt_amp: f32,
    pan_cycles: f32,
    tilt_cycles: f32,
    tilt_phase_deg: f32,
) -> Vec<Step> {
    const RESOLUTION: usize = 16;
    let tau = std::f32::consts::TAU;
    (0..RESOLUTION)
        .map(|i| {
            let t = i as f32 / RESOLUTION as f32;
            let pan = pan_amp * (tau * pan_cycles * t).sin();
            let tilt = tilt_amp * (tau * tilt_cycles * t + tilt_phase_deg.to_radians()).sin();
            Step {
                apply: vec![RecipeApply::Delta(vec![
                    (Attribute::Pan, pan),
                    (Attribute::Tilt, tilt),
                ])],
                width: 1.0,
                // Fully transitioning, because a position effect is a
                // *path*: snapping between sixteen points would be a
                // stutter rather than a curve.
                transition: 1.0,
                ..Step::new(Vec::new())
            }
        })
        .collect()
}

/// A mover recipe from an orbit.
pub(super) fn mover(steps: Vec<Step>, bars: f32, spread: f32) -> Recipe {
    Recipe {
        target: role("Movers"),
        steps,
        timing: beat(bars, spread, Play::Forward),
        tricks: Vec::new(),
        stack: false,
        ..Default::default()
    }
}

/// A step setting an absolute colour by name.
pub(super) fn hue(name: &str) -> Step {
    Step::new(vec![RecipeApply::Color(crate::preset::Ref::Named(
        name.into(),
    ))])
}

/// A relative push on one colour emitter.
pub(super) fn add_chan(channel: ignition_proto::ColorChannel) -> Attribute {
    Attribute::ColorAdd { channel }
}

/// Every emitter and the level together, so a lift reads as white
/// through whatever colour is under it.
pub(super) fn every_emitter(v: f32) -> Vec<(Attribute, f32)> {
    use ignition_proto::ColorChannel;
    vec![
        (Attribute::Dimmer, v),
        (add_chan(ColorChannel::Red), v),
        (add_chan(ColorChannel::Green), v),
        (add_chan(ColorChannel::Blue), v),
        (add_chan(ColorChannel::White), v),
    ]
}

/// The whole library, with its notes: every family, in order.
///
/// Each entry is the name a programmer types, the family and one-line
/// note a picker shows beside it, and the recipe. [`library`] and
/// [`notes`] are the two halves of this, split for the two maps the
/// profile carries.
// r[impl effects.library.profile-ships-it] - the default library; baking into the profile is elsewhere
// r[impl effects.library.categories]
// r[impl effects.library.roles-only]
// r[impl effects.library.by-name] - keyed by the name a programmer types
pub fn catalogue() -> Vec<(String, EffectNote, Recipe)> {
    let mut out = Vec::new();
    let mut add = |name: &str, family: &str, about: &str, recipe: Recipe| {
        out.push((
            name.to_string(),
            EffectNote {
                family: family.to_string(),
                about: about.to_string(),
            },
            recipe,
        ));
    };
    intensity::add(&mut add);
    // The generator flickers replace the table flickers of the same
    // name — see `intensity::add_random`.
    intensity::add_random(&mut add);
    movement::add(&mut add);
    // Patterns drawn in metres, the same shape at every venue.
    movement::add_room(&mut add);
    colour::add(&mut add);
    beam::add(&mut add);
    strip::add(&mut add);
    oneshot::add(&mut add);
    out
}

/// The song moments a bundle is filed under. A picker groups by these,
/// the way it groups effects by [`FAMILIES`].
pub const BUNDLE_FAMILIES: [&str; 8] = [
    "intro",
    "verse",
    "build",
    "chorus",
    "drop",
    "bridge",
    "breakdown",
    "outro",
];

/// The default bundles: several library effects under one name.
///
/// Each is a look an LD would build by hand in the first rehearsal —
/// a chase under a windmill under a two-colour step — and would then
/// want to fire as one thing. Members are library names, never copies,
/// so the bundle follows the library (`r[effects.library.by-name]`) and
/// each member keeps its own clock (`r[effects.timing.uniform]`).
// r[impl effects.bundle]
// r[impl effects.library.by-name] - bundles reference effects by name
pub fn bundles() -> BTreeMap<String, Bundle> {
    let mut out = BTreeMap::new();
    let mut put = |name: &str, family: &str, about: &str, recipes: &[&str]| {
        out.insert(
            name.to_string(),
            Bundle {
                name: name.to_string(),
                family: family.to_string(),
                about: about.to_string(),
                recipes: recipes.iter().map(|r| r.to_string()).collect(),
            },
        );
    };
    put(
        "intro reveal",
        "intro",
        "the stage filling to its look while the movers fly in over a slow rainbow on the bars — the first bar of the show",
        &["fill and hold", "fly in", "rainbow scroll"],
    );
    put(
        "verse bed",
        "verse",
        "the wash breathing slowly under a circle that swells and shrinks with it — every verse, the thing under the vocal",
        &["breathe", "circle breathe"],
    );
    put(
        "verse drift",
        "verse",
        "fixtures breathing out of step while the movers sway and the colour drifts in and out of saturation — a second verse that wants to move without lifting",
        &["offset breathe", "sway", "saturation breathe"],
    );
    put(
        "pre chorus rise",
        "build",
        "the wash filling up fixture by fixture as the movers fan open and an iris pinch runs the rig — the eight bars before a chorus",
        &["build", "pan fan", "iris chase"],
    );
    put(
        "build tension",
        "build",
        "a sawtooth pulse under a rolling tilt wave with the strobes ticking underneath — the riser before a drop",
        &["ramp pulse", "tilt wave", "strobe bed"],
    );
    put(
        "chorus drive",
        "chorus",
        "a chase across the wash, a windmill on the movers and two colours stepping through — the default chorus, the one to reach for first",
        &["chase", "windmill", "two colour chase"],
    );
    put(
        "chorus lift",
        "chorus",
        "everything pulsing together under a figure of eight while the beams breathe with the beat — a chorus that wants to be big rather than busy",
        &["pulse", "figure eight", "zoom pulse"],
    );
    put(
        "drop",
        "drop",
        "a strobe burst, the blinders hitting the room and the movers flying out — the downbeat of the drop, fired once",
        &["strobe burst", "blinder hit", "fly out"],
    );
    put(
        "drop and hold",
        "drop",
        "a beat pulse under a ballyhoo in both position and colour with the bars strobing — the bars after the drop, as loud as the library goes",
        &["beat pulse", "ballyhoo", "colour ballyhoo", "strip strobe"],
    );
    put(
        "bridge hush",
        "bridge",
        "dark sparkle over a split warm and cool wash while the movers nod — a bridge that pulls the room in rather than pushing at it",
        &["dark sparkle", "nod", "warm cool split"],
    );
    put(
        "breakdown embers",
        "breakdown",
        "fire flicker on the wash, candles on the floor and the colour flushing between red and amber — a breakdown down to embers",
        &["fire flicker", "candle", "colour fire"],
    );
    put(
        "outro fade",
        "outro",
        "the stage draining to a glow while the movers lift off and the colour fades between two hues — the last bars, held until the cue drops",
        &["drain and hold", "lift off", "two colour fade"],
    );
    out
}

/// Every effect in the default library, keyed by name.
///
/// Names are lower case and plain English: this is a list somebody
/// scrolls at a desk, and `chase` beats `IntensityChaseForward`.
pub fn library() -> BTreeMap<String, Recipe> {
    catalogue()
        .into_iter()
        .map(|(name, _, recipe)| (name, recipe))
        .collect()
}

/// The family and note for every effect, keyed by name.
pub fn notes() -> BTreeMap<String, EffectNote> {
    catalogue()
        .into_iter()
        .map(|(name, note, _)| (name, note))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::RoleKind;

    /// Every family adds through one list, so a name used twice would
    /// silently replace the first in the map. The families are written
    /// by different hands; this is the line between them.
    // r[verify effects.library.by-name]
    #[test]
    fn every_name_is_unique_and_plain() {
        let all = catalogue();
        let mut seen = std::collections::HashSet::new();
        let mut dupes = Vec::new();
        for (name, _, _) in &all {
            assert_eq!(name, &name.to_lowercase(), "{name}: names are lower case");
            assert!(!name.trim().is_empty());
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == ' '),
                "{name:?}: plain words only"
            );
            if !seen.insert(name.clone()) {
                dupes.push(name.clone());
            }
        }
        assert!(dupes.is_empty(), "duplicate effect names: {dupes:?}");
        assert_eq!(library().len(), all.len());
    }

    /// Every effect carries a note, and every note names one of the six
    /// families and says something an operator can read on a button.
    #[test]
    fn every_effect_has_a_note_in_a_known_family() {
        let all = catalogue();
        for (name, note, _) in &all {
            assert!(
                FAMILIES.contains(&note.family.as_str()),
                "{name:?} is in unknown family {:?}",
                note.family
            );
            assert!(
                note.about.split_whitespace().count() >= 6,
                "{name:?}: the note is not a sentence: {:?}",
                note.about
            );
            assert!(
                note.about.contains(" — "),
                "{name:?}: the note should say what it looks like — and where it belongs"
            );
        }
        let used: std::collections::BTreeSet<&str> =
            all.iter().map(|(_, n, _)| n.family.as_str()).collect();
        assert_eq!(used.len(), FAMILIES.len(), "a family is empty: {used:?}");
        assert_eq!(notes().len(), all.len());
    }

    /// The library is curated: about a hundred general effects and a
    /// short, complete list of mover patterns. Not padded to a number,
    /// but a family that doubled overnight would be worth a look.
    #[test]
    fn the_library_is_the_size_it_means_to_be() {
        let all = catalogue();
        let movers = all
            .iter()
            .filter(|(_, n, _)| n.family == "movement")
            .count();
        assert!(
            (25..=34).contains(&movers),
            "{movers} mover patterns; the list is meant to be 25–34"
        );
        let rest = all.len() - movers;
        assert!(
            (85..=115).contains(&rest),
            "{rest} general effects; the library is meant to be about a hundred"
        );
    }

    /// Every family's entries are of that family's kind.
    #[test]
    fn families_are_what_they_say() {
        for (name, note, recipe) in catalogue() {
            let targets_movers = recipe.target == role("Movers") || recipe.target == role("Beams");
            match note.family.as_str() {
                "movement" => {
                    assert!(targets_movers, "{name:?} is movement but not on the movers");
                    assert!(
                        !recipe.timing.once,
                        "{name:?} is a one-shot in the movement family"
                    );
                }
                "one-shot" => assert!(recipe.timing.once, "{name:?} is in one-shot but loops"),
                "strip" => assert_eq!(recipe.target, role("Bars"), "{name:?} left the strip"),
                _ => assert!(
                    !recipe.timing.once,
                    "{name:?} runs once but is not in one-shot"
                ),
            }
        }
    }

    /// Every rate is a musical subdivision the design guide allows —
    /// four, two, one or half a bar — unless the thing is a flicker or
    /// a one-shot, which are the two cases where a period is not the
    /// look. A rate of 8 bars is a rate somebody should set live.
    #[test]
    fn every_period_is_a_bar_subdivision_or_a_flicker() {
        const FLICKERS: [&str; 12] = [
            "strobe",
            "random strobe",
            "shimmer",
            "ramp pulse",
            "beat pulse",
            "blinder chase",
            "gobo shake",
            "strip twinkle",
            "white pop",
            "tilt bounce",
            "strip strobe",
            "random strobe pops",
        ];
        for (name, note, recipe) in catalogue() {
            if note.family == "one-shot" || FLICKERS.contains(&name.as_str()) {
                continue;
            }
            let bars = recipe.timing.measure / 4.0;
            assert!(
                [4.0, 2.0, 1.0, 0.5].contains(&bars),
                "{name:?} runs at {bars} bars; the guide allows 4, 2, 1 or ½"
            );
        }
    }

    /// Every name an effect reaches for is a role.
    ///
    /// The rule the library exists to keep, and it has to walk the whole
    /// selection tree rather than check the outermost node: a rig effect
    /// targets a `Union` of roles, and an earlier version of this test
    /// asked only whether the top-level selection *was* a role — which
    /// the union is not, and which would have passed for a union of
    /// venue group names just the same.
    ///
    /// One `Selection::Group` in here and that effect works at exactly
    /// one address, which is worse than it not existing: it would be
    /// picked from the same list as the portable ones and fail somewhere
    /// else.
    // r[verify effects.library.roles-only]
    #[test]
    fn every_effect_names_only_roles() {
        fn venue_names(sel: &Selection, out: &mut Vec<String>) {
            match sel {
                Selection::Role(_) | Selection::Chans(_) => {}
                Selection::Group(n) => out.push(format!("group {n:?}")),
                Selection::Tag(t) => out.push(format!("tag {t:?}")),
                Selection::Model(m) => out.push(format!("model {m:?}")),
                Selection::Union(parts) | Selection::Intersect(parts) => {
                    parts.iter().for_each(|p| venue_names(p, out))
                }
                Selection::Except { of, minus } => {
                    venue_names(of, out);
                    venue_names(minus, out);
                }
                Selection::Where { of, .. }
                | Selection::Order { of, .. }
                | Selection::Layout { of, .. } => venue_names(of, out),
            }
        }
        for (name, recipe) in library() {
            let mut found = Vec::new();
            venue_names(&recipe.target, &mut found);
            assert!(
                found.is_empty(),
                "effect {name:?} reaches for venue-specific names: {found:?}"
            );
        }
    }

    /// Every role an effect names is one the default profile declares.
    ///
    /// Without this the library could quietly depend on a role nobody
    /// implements, and the failure would appear at a venue rather than
    /// here.
    // r[verify effects.library.roles-only]
    #[test]
    fn every_targeted_role_is_declared() {
        let profile: crate::profile::Profile = serde_json::from_str(
            &std::fs::read_to_string("../../data/profiles/ignition.ig-profile").unwrap_or_default(),
        )
        .unwrap_or_default();
        if profile.roles.is_empty() {
            return;
        }
        let declared = profile.vocabulary(RoleKind::Group);
        fn roles(sel: &Selection, out: &mut Vec<String>) {
            match sel {
                Selection::Role(r) => out.push(r.clone()),
                Selection::Union(parts) | Selection::Intersect(parts) => {
                    parts.iter().for_each(|p| roles(p, out))
                }
                _ => {}
            }
        }
        for (name, recipe) in library() {
            let mut used = Vec::new();
            roles(&recipe.target, &mut used);
            for role in used {
                assert!(
                    declared.contains(&role.as_str()),
                    "effect {name:?} targets role {role:?}, which the profile does not declare"
                );
            }
        }
    }

    /// Every rate is against a named master, never a hard-coded tempo.
    ///
    /// `Song` for playback and `Tap` for busking — the same shapes with
    /// a different rate source, which is why they are one library rather
    /// than two. What must never appear is a raw Hz or BPM: that is a
    /// tempo somebody dialled in once, and it will not follow the music.
    // r[verify effects.library.categories]
    // r[verify recipes.timing-in-musical-terms]
    // r[verify effects.masters.registry]
    #[test]
    fn every_effect_is_slaved_to_a_named_master() {
        for (name, recipe) in library() {
            match &recipe.timing.speed {
                Speed::Master(m) => assert!(
                    m == "Song" || m == "Tap",
                    "effect {name:?} runs on unknown master {m:?}"
                ),
                other => panic!("effect {name:?} has a hard-coded rate: {other:?}"),
            }
        }
    }

    /// The tapped spellings are the song spellings with a different
    /// master and nothing else — one library, not two.
    // r[verify effects.masters.tap]
    #[test]
    fn the_tapped_spellings_share_their_shape() {
        let lib = library();
        for (tap, song) in [
            ("tap chase", "chase"),
            ("tap pulse", "pulse"),
            ("tap circle", "circle"),
            ("tap ballyhoo", "ballyhoo"),
            ("tap two colour", "two colour chase"),
        ] {
            assert_eq!(
                lib[tap].steps, lib[song].steps,
                "{tap:?} drifted from {song:?}"
            );
            assert_eq!(lib[tap].tricks, lib[song].tricks);
            assert_eq!(lib[tap].timing.speed, Speed::Master("Tap".into()));
            assert_eq!(lib[song].timing.speed, Speed::Master("Song".into()));
        }
    }

    /// Effects layer, so an intensity effect is relative unless there is
    /// a reason it cannot be.
    ///
    /// Two kinds of exception, and keeping them apart is the point.
    /// **Colour effects are absolute by nature**: setting a colour has
    /// nothing to be relative to, so they replace whatever the look
    /// established — which is what they are for, and also why stacking
    /// two is last-wins rather than a blend. The **named** exceptions are
    /// intensity effects that are deliberately absolute: a strobe that
    /// merely modulated would still show what was under it.
    ///
    /// Listing them exactly is what stops the next accidental absolute —
    /// an effect that quietly stops layering is very hard to notice from
    /// the stage, because it looks like it is working.
    // r[verify effects.library.categories]
    // r[verify effects.modulates-with-delta]
    #[test]
    fn intensity_effects_are_relative_except_where_named() {
        // `drum ring` names its focus in every step (a point plus a delta in
        // metres), so it is absolute on purpose: the ring is around the kit.
        const DELIBERATELY_ABSOLUTE: [&str; 3] = ["strobe", "random strobe", "drum ring"];

        let mut unexpected: Vec<String> = Vec::new();
        for (name, recipe) in library() {
            let sets_colour = recipe.steps.iter().any(|s| {
                s.apply
                    .iter()
                    .any(|a| matches!(a, RecipeApply::Color(_) | RecipeApply::Colors { .. }))
            });
            let absolute = recipe.steps.iter().any(|s| {
                s.apply.iter().any(|a| {
                    !matches!(
                        a,
                        RecipeApply::Delta(_)
                            | RecipeApply::FocusDelta(_)
                            | RecipeApply::Random(crate::recipe::Random {
                                absolute: false,
                                ..
                            })
                    )
                })
            });
            if absolute && !sets_colour && !DELIBERATELY_ABSOLUTE.contains(&name.as_str()) {
                unexpected.push(name);
            }
        }
        assert!(
            unexpected.is_empty(),
            "these stopped layering without being declared: {unexpected:?}"
        );
    }

    /// A circle is one recipe whose steps carry both axes, a quarter
    /// cycle apart. Shipped as a pan/tilt *pair* — which is how a console
    /// with per-attribute effects has to do it — losing one silently
    /// turns the circle into a diagonal sweep that looks deliberate
    /// enough that nobody notices.
    // r[verify effects.library.categories]
    #[test]
    fn a_circle_is_one_recipe_carrying_both_axes() {
        let circle = &library()["circle"];
        assert!(circle.steps.len() > 8, "too coarse to read as a curve");
        for step in &circle.steps {
            let attrs: Vec<&Attribute> = step
                .apply
                .iter()
                .flat_map(|a| match a {
                    RecipeApply::Delta(pairs) => pairs.iter().map(|(a, _)| a).collect(),
                    _ => Vec::new(),
                })
                .collect();
            assert!(attrs.contains(&&Attribute::Pan), "a step with no pan");
            assert!(attrs.contains(&&Attribute::Tilt), "a step with no tilt");
        }
    }

    /// The quarter-cycle offset is what makes it a circle rather than a
    /// line, and it lives in the table now. Pan peaks a quarter of the
    /// way round; tilt peaks at the start.
    // r[verify effects.library.categories]
    #[test]
    fn the_circles_axes_are_a_quarter_cycle_apart() {
        let circle = &library()["circle"];
        let axis = |step: &Step, want: &Attribute| -> f32 {
            step.apply
                .iter()
                .filter_map(|a| match a {
                    RecipeApply::Delta(pairs) => {
                        pairs.iter().find(|(at, _)| at == want).map(|(_, v)| *v)
                    }
                    _ => None,
                })
                .next()
                .unwrap_or_default()
        };
        let quarter = circle.steps.len() / 4;
        // Pan starts at zero and is at its widest a quarter round.
        assert!(axis(&circle.steps[0], &Attribute::Pan).abs() < 1e-4);
        assert!(axis(&circle.steps[quarter], &Attribute::Pan).abs() > 10.0);
        // Tilt does the opposite, which is exactly the offset.
        assert!(axis(&circle.steps[0], &Attribute::Tilt).abs() > 10.0);
        assert!(axis(&circle.steps[quarter], &Attribute::Tilt).abs() < 1e-4);
    }

    /// A ballyhoo's axes must not share a simple ratio, or the beam
    /// resolves into a repeating shape and stops reading as excitement.
    #[test]
    fn a_ballyhoo_does_not_resolve_quickly() {
        let ballyhoo = &library()["ballyhoo"];
        let circle = &library()["circle"];
        assert_ne!(
            ballyhoo.steps, circle.steps,
            "the ballyhoo is drawing a circle"
        );
    }

    /// A bump runs once and holds. Looping, it is a strobe.
    // r[verify effects.once]
    // r[verify recipes.one-shot]
    #[test]
    fn the_bump_is_a_one_shot() {
        assert!(library()["bump"].timing.once);
        assert!(!library()["chase"].timing.once);
    }

    /// Effects with two or more steps are phasers; the static ones are
    /// looks. Both are recipes, which is the point — but an effect that
    /// meant to move and has one step would silently sit still.
    // r[verify recipes.steps-are-the-switch]
    #[test]
    fn every_effect_has_more_than_one_step() {
        for (name, recipe) in library() {
            assert!(
                recipe.steps.len() > 1,
                "{name:?} has one step and cannot move"
            );
            assert!(recipe.timing.measure > 0.0, "{name:?} has no measure");
        }
    }

    /// Every bundle is made of effects that exist, filed under a known
    /// moment, with a note a picker can show — and no bundle is one
    /// effect wearing a second name.
    // r[verify effects.bundle]
    // r[verify effects.library.by-name]
    #[test]
    fn every_bundle_member_exists() {
        let lib = library();
        let all = bundles();
        assert!((8..=12).contains(&all.len()), "{} bundles", all.len());
        for (key, bundle) in &all {
            assert_eq!(key, &bundle.name);
            assert!(
                key.chars().all(|c| c.is_ascii_lowercase() || c == ' '),
                "{key:?}: plain words only"
            );
            assert!(
                BUNDLE_FAMILIES.contains(&bundle.family.as_str()),
                "{key:?} is filed under unknown moment {:?}",
                bundle.family
            );
            assert!(
                bundle.about.contains(" — "),
                "{key:?}: note needs a — where"
            );
            assert!(bundle.recipes.len() >= 2, "{key:?} is not a bundle");
            let mut seen = std::collections::HashSet::new();
            for member in &bundle.recipes {
                assert!(
                    lib.contains_key(member),
                    "bundle {key:?} names {member:?}, which the library does not have"
                );
                assert!(seen.insert(member), "bundle {key:?} lists {member:?} twice");
            }
            assert!(
                !lib.contains_key(key),
                "bundle {key:?} shadows an effect of the same name"
            );
        }
    }

    /// The names other crates reach for by hand are still here. The
    /// show author in `ignition-song` picks these by name, and a rename
    /// here is a panic there.
    #[test]
    fn the_names_the_show_author_uses_survive() {
        let lib = library();
        for name in [
            "ballyhoo",
            "bar sparkle",
            "bar tick",
            "chase eighths",
            "circle",
            "circle tight",
            "fly out",
            "nod",
            "rig build",
            "sway",
            "windmill",
        ] {
            assert!(lib.contains_key(name), "{name:?} is gone");
        }
    }
}
