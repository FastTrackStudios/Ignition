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
use ignition_core::{
    Attribute, Group, Recipe, RecipeApply, Selection, Speed, Step, Timing, Trick,
};
use std::collections::BTreeMap;

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
                .map(|(role, group)| {
                    ((*role).to_string(), Selection::Group((*group).to_string()))
                })
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
        &[("Key", "Front Wash"), ("Back", "Back Wall Pars")],
    )
}

/// A small room, with a third of the fixtures and none of the same
/// names. This is the venue a show has never seen.
fn riverside() -> Venue {
    Venue::new(
        &[
            ("FOH Bar", &[101, 102, 103]),
            ("Cyc", &[110]),
        ],
        &[("Key", "FOH Bar"), ("Back", "Cyc")],
    )
}

fn lit(recipe: &Recipe, venue: &Venue, secs: f32) -> Vec<(u32, f32)> {
    let show = Show {
        groups: &venue.groups,
        palettes: ignition_core::Palettes::EMPTY,
        rig: &ignition_core::selection::EMPTY_RIG,
        speeds: &ignition_core::recipe::NO_SPEEDS,
        roles: venue,
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
#[test]
fn one_recipe_lights_two_different_rigs() {
    let recipe = Recipe::new(
        Selection::Role("Key".into()),
        RecipeApply::Dimmer(0.8),
    );

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
        xs.sort();
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
