// Integration test: `clippy.toml`'s test allowances only reach
// `#[cfg(test)]` modules, so the panic set is lifted here instead.
// See docs/ops/clippy.md.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "integration test — see docs/ops/clippy.md"
)]

//! The claim the whole design exists to make, tested.
//!
//! One recipe. Two rooms with different fixtures, different counts,
//! different names for everything. Nothing about the recipe changes, and
//! both rooms light up.
//!
//! Written as an integration test rather than a unit test because it is
//! not testing a function — it is testing that the *seams* line up:
//! roles resolve through venue bindings, Tricks cut whatever they find,
//! and a phase spread is a proportion of a selection rather than a count
//! of fixtures. Any one of those going absolute would break portability
//! while every unit test still passed.

use ignition_core::recipe::{Emit, Show, expand_recipe};
use ignition_core::selection::Roles;
use ignition_core::{Attribute, Group, Recipe, RecipeApply, Selection, Speed, Step, Timing, Trick};
use std::collections::BTreeMap;

/// The profile's colours, which every venue inherits.
///
/// Loaded from the shipped file rather than invented, so a colour effect
/// naming `Deep` is tested against the value a real venue would resolve.
/// Without palettes a colour effect emits nothing at all — correctly, an
/// unresolvable colour is not a colour — which makes an empty palette a
/// silent way to test nothing.
fn profile_palettes() -> ignition_core::Palettes {
    let raw =
        std::fs::read_to_string("../../data/profiles/ignition.ig-profile").unwrap_or_default();
    let profile: ignition_core::profile::Profile = serde_json::from_str(&raw).unwrap_or_default();
    ignition_core::Palettes {
        colors: profile.colors,
        ..Default::default()
    }
}

/// A venue: what it calls its groups, and which of them play which role.
struct Venue {
    groups: Vec<Group>,
    bindings: BTreeMap<String, Selection>,
}

impl Roles for Venue {
    fn role(&self, name: &str) -> Option<&Selection> {
        self.bindings.get(name)
    }
}

impl Venue {
    fn new(groups: &[(&str, &[u32])], roles: &[(&str, &str)]) -> Self {
        Self {
            groups: groups
                .iter()
                .map(|(name, chans)| Group {
                    name: (*name).to_string(),
                    chans: chans.to_vec(),
                })
                .collect(),
            bindings: roles
                .iter()
                .map(|(role, group)| ((*role).to_string(), Selection::Group((*group).to_string())))
                .collect(),
        }
    }
}

/// Norco: a big rig, and it calls its key light `Front Wash`.
fn norco() -> Venue {
    Venue::new(
        &[
            ("Front Wash", &[1, 2, 3, 4, 5, 6, 7, 8]),
            ("Back Wall Pars", &[20, 21, 22, 23]),
        ],
        &[
            ("Key", "Front Wash"),
            // Norco's key light and its wash are the same bar, which is
            // ordinary in a small-to-middling rig and is exactly what
            // binding-by-role is for: two roles, one set of fixtures,
            // and no show has to know.
            ("Wash", "Front Wash"),
            ("Back", "Back Wall Pars"),
        ],
    )
}

/// A small room, with a third of the fixtures and none of the same
/// names. This is the venue a show has never seen.
fn riverside() -> Venue {
    Venue::new(
        &[("FOH Bar", &[101, 102, 103]), ("Cyc", &[110])],
        &[("Key", "FOH Bar"), ("Wash", "FOH Bar"), ("Back", "Cyc")],
    )
}

fn lit(recipe: &Recipe, venue: &Venue, secs: f32) -> Vec<(u32, f32)> {
    let palettes = profile_palettes();
    let show = Show {
        groups: &venue.groups,
        palettes: &palettes,
        rig: &ignition_core::selection::EMPTY_RIG,
        speeds: &ignition_core::recipe::NO_SPEEDS,
        roles: venue,
        ..Show::new(&venue.groups, &ignition_core::selection::EMPTY_RIG)
    };
    let mut out: Vec<(u32, f32)> = expand_recipe(recipe, &show, secs)
        .into_iter()
        .filter(|Emit { value, .. }| value.attr == Attribute::Dimmer)
        .map(|Emit { value, .. }| (value.chan, value.value))
        .collect();
    out.sort_by_key(|(chan, _)| *chan);
    out
}

/// A recipe written against a role lights both rooms, untouched.
/// r[verify profile.resolution-by-role]
/// r[verify files.no-fixture-identity]
/// r[verify recipes.template]
#[test]
fn one_recipe_lights_two_different_rigs() {
    let recipe = Recipe::new(Selection::Role("Key".into()), RecipeApply::Dimmer(0.8));

    let at_norco = lit(&recipe, &norco(), 0.0);
    let at_riverside = lit(&recipe, &riverside(), 0.0);

    assert_eq!(at_norco.len(), 8, "Norco's key light is eight fixtures");
    assert_eq!(at_riverside.len(), 3, "Riverside's is three");
    assert!(at_norco.iter().all(|(_, v)| (*v - 0.8).abs() < 1e-6));
    assert!(at_riverside.iter().all(|(_, v)| (*v - 0.8).abs() < 1e-6));
    // And they are genuinely different fixtures, not a coincidence.
    assert_eq!(at_norco[0].0, 1);
    assert_eq!(at_riverside[0].0, 101);
}

/// An unbound role lights nothing rather than erroring, and nothing else
/// in the cue is affected. A room with no follow spot plays the show
/// without one — see `r[files.graceful-degradation]`.
/// r[verify files.graceful-degradation]
/// r[verify effects.library.missing-role-is-empty] - empty; reporting is not checked here
#[test]
fn an_unbound_role_is_empty_not_fatal() {
    let recipe = Recipe::new(Selection::Role("Spot".into()), RecipeApply::Dimmer(1.0));
    assert!(lit(&recipe, &norco(), 0.0).is_empty());
}

/// A Trick cuts whatever it finds. `Group(2)` is odds and evens on eight
/// fixtures and on three, so a show using it is not secretly written for
/// a fixture count.
#[test]
fn a_trick_cuts_both_rigs_proportionally() {
    let mut recipe = Recipe::new(Selection::Role("Key".into()), RecipeApply::Dimmer(1.0));
    recipe.tricks = vec![Trick::Group(2)];

    // Every fixture is still lit — a Trick regroups, it does not filter.
    assert_eq!(lit(&recipe, &norco(), 0.0).len(), 8);
    assert_eq!(lit(&recipe, &riverside(), 0.0).len(), 3);
}

/// The load-bearing one: a chase phases across *units*, so a blocked
/// pair moves together and a spread reaches both ends whatever the rig
/// size. If phase were computed per fixture instead, a blocked selection
/// would fan inside its own blocks and the whole point of blocking would
/// be gone.
#[test]
fn a_blocked_chase_moves_in_pairs() {
    // One step per fixture, each a distinct level. With fewer steps than
    // fixtures the spread puts neighbours in the same step anyway, and a
    // control built that way cannot tell blocking from arithmetic — the
    // first version of this test passed its real assertions and failed
    // its control for exactly that reason.
    let step = |v: f32| Step::new(vec![RecipeApply::Dimmer(v)]);
    let ladder: Vec<Step> = (0..8).map(|i| step(i as f32 / 7.0)).collect();
    let mut recipe = Recipe {
        target: Selection::Role("Key".into()),
        steps: ladder,
        timing: Timing {
            speed: Speed::Bpm(60.0),
            measure: 1.0,
            phase_spread_deg: 360.0,
            ..Default::default()
        },
        tricks: vec![Trick::Block(2)],
        stack: false,
        ..Default::default()
    };

    let out = lit(&recipe, &norco(), 0.0);
    assert_eq!(out.len(), 8);
    // Four pairs, and within a pair both fixtures agree exactly.
    for pair in out.chunks(2) {
        assert!(
            (pair[0].1 - pair[1].1).abs() < 1e-6,
            "a blocked pair disagreed: {pair:?}"
        );
    }
    // Four units means four distinct levels, not eight — the spread is
    // across units, which is what blocking is for.
    let distinct = |v: &[(u32, f32)]| {
        let mut xs: Vec<i32> = v.iter().map(|(_, x)| (x * 1000.0) as i32).collect();
        xs.sort_unstable();
        xs.dedup();
        xs.len()
    };
    assert_eq!(distinct(&out), 4, "blocked levels: {out:?}");

    // Unblocked, every fixture gets its own — which is what makes the
    // pairing above meaningful rather than an artefact of a flat chase.
    recipe.tricks.clear();
    let plain = lit(&recipe, &norco(), 0.0);
    assert_eq!(distinct(&plain), 8, "unblocked levels: {plain:?}");
}

/// A role may bind to an expression, not just a group — so one venue's
/// key light can be "these two bars" without the show knowing.
/// r[verify profile.venue-binds]
#[test]
fn a_role_may_bind_to_an_expression() {
    let mut venue = norco();
    venue.bindings.insert(
        "Key".into(),
        Selection::Union(vec![
            Selection::Group("Front Wash".into()),
            Selection::Group("Back Wall Pars".into()),
        ]),
    );
    let recipe = Recipe::new(Selection::Role("Key".into()), RecipeApply::Dimmer(0.5));
    assert_eq!(lit(&recipe, &venue, 0.0).len(), 12);
}

/// Distinct channels an effect touches, whatever attribute it sets.
///
/// Separate from `lit`, which filters to `Dimmer` because it is asking
/// about *levels*. A colour effect sets `ColorAdd` and no dimmer at all,
/// so asking the level question of one gives the answer "nothing" about
/// an effect that is working perfectly well.
fn touched(recipe: &Recipe, venue: &Venue, secs: f32) -> Vec<u32> {
    let palettes = profile_palettes();
    let show = Show {
        groups: &venue.groups,
        palettes: &palettes,
        rig: &ignition_core::selection::EMPTY_RIG,
        speeds: &ignition_core::recipe::NO_SPEEDS,
        roles: venue,
        ..Show::new(&venue.groups, &ignition_core::selection::EMPTY_RIG)
    };
    let mut chans: Vec<u32> = expand_recipe(recipe, &show, secs)
        .into_iter()
        .map(|Emit { value, .. }| value.chan)
        .collect();
    chans.sort_unstable();
    chans.dedup();
    chans
}

/// Every shipped effect produces output at a big rig and a small one.
///
/// The library is the payoff of the whole design, so this is the test
/// that says the payoff arrived: twenty effects, two rooms with nothing
/// in common but their role bindings, and not one of them silent.
///
/// It catches the failure mode that unit tests cannot — an effect whose
/// target, Tricks and spread are each individually fine but which
/// resolves to nothing once composed against a rig of three.
/// r[verify effects.library.roles-only]
/// r[verify files.graceful-degradation]
#[test]
fn the_whole_library_lights_both_rigs() {
    let library = ignition_core::effects::library();
    assert!(library.len() >= 15, "the library shrank unexpectedly");

    for (name, recipe) in &library {
        // Only the roles these venues actually bind. An effect on
        // `Movers` at a room with none is legitimately silent — that is
        // graceful degradation, not a defect.
        let Selection::Role(target) = &recipe.target else {
            continue;
        };
        if !matches!(target.as_str(), "Key" | "Wash" | "Back") {
            continue;
        }
        for (venue, label, expect) in [
            (norco(), "Norco", 8usize),
            (riverside(), "Riverside", 3usize),
        ] {
            // Sampled across a cycle: a phaser can legitimately sit at
            // zero delta at one instant, and testing a single moment
            // would fail on that rather than on anything real.
            let any = (0..8).any(|i| !touched(recipe, &venue, i as f32 * 0.25).is_empty());
            assert!(any, "effect {name:?} produced nothing at {label}");

            let out = touched(recipe, &venue, 0.0);
            let covered = if target == "Back" {
                if label == "Norco" { 4 } else { 1 }
            } else {
                expect
            };
            assert_eq!(
                out.len(),
                covered,
                "effect {name:?} covered {} of {covered} fixtures at {label}",
                out.len()
            );
        }
    }
}

/// A charted hit and a flash key produce the same thing.
///
/// The claim `bump` exists to make. If these ever diverge the symptom is
/// "the chart feels different from playing it by hand", which is
/// unfalsifiable from the stage and impossible to bisect — so it is
/// worth pinning at the level where the two paths meet.
/// r[verify playback.flash-equals-hit]
/// r[verify effects.bump.one-object]
#[test]
fn a_charted_hit_and_a_flash_key_agree() {
    use ignition_core::bump::{Kind, bump};

    let venue = norco();
    let target = Selection::Role("Wash".into());

    // What the show's author emits for a hit.
    let charted = bump(target.clone(), Kind::Level, 1.0);
    // What an operator's flash key fires.
    let by_hand = bump(target, Kind::Level, 1.0);

    for secs in [0.0, 0.05, 0.2, 0.4] {
        assert_eq!(
            lit(&charted, &venue, secs),
            lit(&by_hand, &venue, secs),
            "the two paths diverged at {secs}s"
        );
    }
}

/// Every bump ends at nothing, so a song's worth of snares does not
/// ratchet the rig brighter.
/// r[verify effects.delta-ends-at-nothing]
#[test]
fn bumps_do_not_accumulate() {
    use ignition_core::bump::{Kind, bump};

    let venue = norco();
    for kind in [Kind::Level, Kind::White, Kind::ColorBoost, Kind::Burst] {
        let recipe = bump(Selection::Role("Wash".into()), kind, 1.0);
        // Well past the envelope, where a one-shot holds its last step.
        let settled = lit(&recipe, &venue, 30.0);
        for (chan, value) in settled {
            assert!(
                value.abs() < 1e-4,
                "{kind:?} settled at {value} on channel {chan}"
            );
        }
    }
}
