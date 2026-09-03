//! A figure's cutout against the real show and the real room.
//!
//! The unit tests in `ignition_core::cue` and `ignition_core::trigger`
//! prove the mechanisms on three pars. This proves them on *Bye Bye
//! Bye* at Norco, where the first third of figure 0 lands on a PRE
//! section carrying a looping wash chase — the case that was reported as
//! "the hit gets overwritten by the effect from the PRE cue".

use ignition_core::{Attribute, Bars, CueList, CuePlayer, Show, SpeedMasters, TriggerBus};
use ignition_viz::venue::Venue;
use std::collections::HashMap;

/// A channel count as `f32`, for averaging a handful of dimmer levels.
/// `ignition_viz`'s own audited helper (`num::f32_of_usize`) is
/// `pub(crate)` and unreachable from an integration test, so this is the
/// same one-line audit repeated here: every count this test ever builds
/// is a handful of channels on one venue, nowhere near where an `f32`
/// stops counting integers exactly.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "channel counts here are tiny; see the doc comment"
)]
const fn count_as_f32(n: usize) -> f32 {
    n as f32
}

/// r[verify playback.transient-over-sustained]
/// r[verify playback.triggers-sum]
/// r[verify triggers.transient-class]
/// r[verify triggers.crossing-fires]
/// r[verify song.chart.figure.cutout-or-bump]
#[test]
fn a_cutout_reveals_its_zone_through_a_running_chase() {
    let Ok(venue) = Venue::load("../../data/venues/norco") else {
        return; // repo data absent — a runner outside the checkout
    };
    let Some(list) = std::fs::read_to_string("../../data/songs/bye-bye-bye.json")
        .ok()
        .and_then(|t| serde_json::from_str::<CueList>(&t).ok())
    else {
        return;
    };
    let groups = venue.groups();
    let rig = venue.rig();
    let speeds = SpeedMasters::from([("Song".to_string(), 86.0f32), ("Tap".to_string(), 120.0)]);
    let show = Show {
        groups: &groups,
        palettes: &venue.palettes,
        rig: &rig,
        speeds: &speeds,
        roles: &venue.profile,
        ..Show::new(&groups, &rig)
    };

    // Figure 0's first moment: a cut over everything and a lift on the
    // left third, at one position.
    let reveal_trigger = list
        .triggers
        .iter()
        .find(|t| t.name == "fig 0 · 1/3")
        .expect("the show has figure 0");
    let cut = list
        .triggers
        .iter()
        .find(|t| t.name == "fig 0 · 1/3 cut")
        .expect("figure 0 cuts");
    let at = reveal_trigger
        .bars()
        .expect("the shipped show carries resolved bars");
    let zone = ignition_core::selection::resolve_with(
        &reveal_trigger.recipe.target,
        &groups,
        &rig,
        &venue.profile,
    );
    let all =
        ignition_core::selection::resolve_with(&cut.recipe.target, &groups, &rig, &venue.profile);
    assert!(!zone.is_empty(), "figure 0's zone selects nothing");

    // The section look underneath, at that bar.
    let t0 = 42.0;
    let mut player = CuePlayer::new(list.cues.clone());
    player.set_clock(t0);
    player.seek(at, &show);

    // The transport crosses the figure.
    let mut bus = TriggerBus::new(list.triggers.clone());
    bus.locate(Bars::new(at.bar, at.beat - 0.25));
    bus.advance(at, t0);

    let frame = |player: &mut CuePlayer, bus: &TriggerBus, t: f32| {
        player.set_clock(t);
        let ringing = bus.output(&show, t);
        let mut out = player.output_under(&show, &ringing.keys().cloned().collect());
        for (key, delta) in ringing {
            *out.entry(key).or_insert(0.0) += delta;
        }
        out
    };
    let level = |out: &HashMap<(u32, Attribute), f32>, chan: u32| {
        out.get(&(chan, Attribute::Dimmer))
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0)
    };
    let mean = |out: &HashMap<(u32, Attribute), f32>, chans: &[u32]| {
        chans.iter().map(|c| level(out, *c)).sum::<f32>() / count_as_f32(chans.len().max(1))
    };

    let during = frame(&mut player, &bus, t0 + 0.05);
    let rest: Vec<u32> = all.iter().copied().filter(|c| !zone.contains(c)).collect();
    let zone_mean = mean(&during, &zone);
    let rest_mean = mean(&during, &rest);
    assert!(
        zone_mean > 0.8,
        "the reveal did not land: zone mean {zone_mean:.2}"
    );
    assert!(
        rest_mean < 0.15,
        "the cut did not land: rest mean {rest_mean:.2}"
    );

    // It holds: two seconds on the carve is still there...
    let held = frame(&mut player, &bus, t0 + 2.0);
    assert!(
        mean(&held, &zone) > 0.8,
        "the hold fell: {:.2}",
        mean(&held, &zone)
    );
    // ...and a cue releases it, so the section look is back.
    bus.release();
    let after = frame(&mut player, &bus, t0 + 2.0);
    let zone_after = mean(&after, &zone);
    assert!(
        zone_after < zone_mean - 0.3,
        "the reveal never withdrew: {zone_after:.2} vs {zone_mean:.2}"
    );
}
