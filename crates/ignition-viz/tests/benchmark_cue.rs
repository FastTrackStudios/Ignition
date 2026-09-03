//! The benchmark cue is a real show file: it loads, every recipe on it
//! cooks against Norco, and it drives what `r[viz.performance-budget]`
//! says it drives — every mover, every par, every bar, the beams, the
//! hazers. A benchmark that quietly cooked to a dark rig would measure
//! an empty room.

use ignition_core::{Attribute, CueList, Show, SpeedMasters};
use ignition_viz::venue::Venue;

/// r[verify viz.performance-budget]
#[test]
fn the_benchmark_cue_cooks_the_whole_rig_at_norco() {
    let Ok(venue) = Venue::load("../../data/venues/norco") else {
        return; // repo data absent — a runner outside the checkout
    };
    let text = std::fs::read_to_string("../../data/songs/benchmark.json")
        .expect("data/songs/benchmark.json is in the repo");
    let list: CueList = serde_json::from_str(&text).expect("the benchmark cue list parses");
    let cue = list.cues.first().expect("one cue");
    assert_eq!(cue.name, "bench");

    let groups = venue.groups();
    let rig = venue.rig();
    let speeds = SpeedMasters::from([("Song".to_string(), 140.0f32), ("Tap".to_string(), 120.0)]);
    let library = ignition_core::effects::library();
    let show = Show {
        groups: &groups,
        palettes: &venue.palettes,
        rig: &rig,
        speeds: &speeds,
        roles: &venue.profile,
        library: &library,
        ..Show::new(&groups, &rig)
    };

    // Every recipe on the cue resolves to something — a named effect
    // that misses its group, or a group this venue lacks, would cook
    // to nothing without an error.
    let mut dimmed = std::collections::BTreeSet::new();
    let mut moved = std::collections::BTreeSet::new();
    for recipe_ref in &cue.recipes {
        let recipes = recipe_ref.resolve(&show);
        assert!(
            !recipes.is_empty(),
            "a recipe on the benchmark cue does not resolve: {recipe_ref:?}"
        );
        for recipe in &recipes {
            let full = ignition_core::recipe::expand_recipe_full(recipe, &show, 0.37);
            assert!(
                !full.emits.is_empty() || !full.focus_deltas.is_empty(),
                "a recipe on the benchmark cue cooks to nothing: {recipe_ref:?}"
            );
            for emit in &full.emits {
                if emit.value.attr == Attribute::Dimmer {
                    dimmed.insert(emit.value.chan);
                }
                if matches!(emit.value.attr, Attribute::Pan | Attribute::Tilt) {
                    moved.insert(emit.value.chan);
                }
            }
            for delta in &full.focus_deltas {
                moved.insert(delta.chan);
            }
        }
    }
    // Norco: 48 pars, 4 SlimPARs, 8 movers, 8 strips, 2 hazers.
    assert!(
        dimmed.len() >= 60,
        "only {} channels get a dimmer: {dimmed:?}",
        dimmed.len()
    );
    assert!(
        moved.len() >= 8,
        "only {} movers move: {moved:?}",
        moved.len()
    );
}

/// Played, the cue keeps every dimmer at least half up at every sample
/// time: the chases and the strip chase ride a bed and never take a
/// fixture to black. A benchmark frame is measured with all the lights
/// on, or it is measuring the wrong thing.
/// r[verify viz.performance-budget]
#[test]
fn the_benchmark_cue_never_takes_a_light_to_black() {
    let Ok(venue) = Venue::load("../../data/venues/norco") else {
        return;
    };
    let text = std::fs::read_to_string("../../data/songs/benchmark.json").unwrap();
    let list: CueList = serde_json::from_str(&text).unwrap();
    let groups = venue.groups();
    let rig = venue.rig();
    let speeds = SpeedMasters::from([("Song".to_string(), 140.0f32), ("Tap".to_string(), 120.0)]);
    let library = ignition_core::effects::library();
    let show = Show {
        groups: &groups,
        palettes: &venue.palettes,
        rig: &rig,
        speeds: &speeds,
        roles: &venue.profile,
        library: &library,
        ..Show::new(&groups, &rig)
    };
    let mut player = ignition_core::CuePlayer::from_list(&list);
    player.go(&show);
    let mut lit_at_all = std::collections::BTreeSet::new();
    // Thirty-two samples across two bars at 140 bpm — every phase of
    // the half-bar chases and the one-bar rainbow.
    for sample in 0..32 {
        player.tick_with(0.107, &show);
        let out = player.output(&show);
        let mut dimmers = 0;
        for ((chan, attr), value) in &out {
            if *attr == Attribute::Dimmer {
                dimmers += 1;
                lit_at_all.insert(*chan);
                assert!(
                    *value >= 0.5,
                    "sample {sample}: channel {chan:?} dimmer at {value}, below the bed"
                );
            }
        }
        assert!(
            dimmers >= 60,
            "sample {sample}: only {dimmers} dimmers in the output"
        );
    }
    assert!(lit_at_all.len() >= 60);
}
