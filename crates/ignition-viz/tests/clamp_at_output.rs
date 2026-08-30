//! A cut deeper than the level it lands on reaches zero, and the
//! arithmetic that got it there stays unclamped until the wire.
//!
//! The rule reads like an implementation detail and is not one. A
//! relative layer clamped where it is folded would make the result
//! depend on the order the layers happened to be folded in: a −0.95 cut
//! clamped on its own is a 0.0 that a later +0.5 lifts to 0.5, where
//! the same two layers folded the other way round give 0.1. Both are
//! defensible, neither is reproducible, and the difference only shows
//! up on the one cue that stacks a deep cut under a lift.

use ignition_core::{
    Attribute, Cue, CuePlayer, Recipe, RecipeApply, Selection, Show, Step,
};
use ignition_viz::DmxUniverses;
use ignition_viz::show::apply_cue_output;
use ignition_viz::venue::Venue;

/// r[verify playback.clamp-at-output]
#[test]
fn a_cut_deeper_than_the_level_reaches_zero_and_not_before_the_wire() {
    let Ok(venue) = Venue::load("../../data/venues/norco") else {
        return; // repo data absent — a runner outside the checkout
    };
    let patch = venue.patch();
    let Some((chan, fixture)) = patch
        .iter()
        .find(|(_, f)| f.map.offset_of(&Attribute::Dimmer).is_some())
    else {
        return;
    };

    let groups = venue.groups();
    let rig = venue.rig();
    let show = Show::new(&groups, &rig);

    let level = Cue {
        name: "Look".to_string(),
        recipes: vec![
            Recipe::new(Selection::Chans(vec![chan]), RecipeApply::Dimmer(0.55)).into(),
        ],
        block: true,
        ..Default::default()
    };
    let mut cut = Recipe::new(Selection::Chans(vec![chan]), RecipeApply::Dimmer(0.0));
    cut.steps = vec![Step::new(vec![RecipeApply::Delta(vec![(
        Attribute::Dimmer,
        -0.95,
    )])])];
    let cut = Cue {
        name: "· cut".to_string(),
        recipes: vec![cut.into()],
        ..Default::default()
    };

    let mut player = CuePlayer::new(vec![level, cut]);
    player.go(&show);

    // The level alone, on the wire — so a zero below is a fixture that
    // was cut out rather than one that was never lit.
    let dmx = DmxUniverses::new();
    apply_cue_output(&dmx, &venue, &player.output(&show));
    let lit = dmx.resolve(&fixture.address, &fixture.map).dimmer;
    assert!((lit - 0.55).abs() < 0.02, "the look never reached the wire: {lit}");

    player.go(&show);
    let values = player.output(&show);
    let folded = values[&(chan, Attribute::Dimmer)];

    // 0.55 − 0.95. The engine hands on the arithmetic it did, not a
    // clamped stand-in for it: this is what keeps a further layer's
    // effect independent of fold order.
    assert!(
        folded < 0.0,
        "the stack clamped before the wire, so what a further layer adds now depends \
         on the order the layers were folded: {folded}"
    );
    assert!((folded + 0.4).abs() < 1e-3, "{folded}");

    // And on the wire it is zero — a dark fixture, not a wrapped byte.
    apply_cue_output(&dmx, &venue, &values);
    let out = dmx.resolve(&fixture.address, &fixture.map).dimmer;
    assert!(
        out.abs() < 1e-3,
        "the cut left the fixture at {out} rather than out"
    );
}
