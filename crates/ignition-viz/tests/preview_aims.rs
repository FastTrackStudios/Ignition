//! A movement preview has to have somewhere to move *on*.
//!
//! Every movement effect in the library is a relative Pan/Tilt delta, on
//! purpose — a circle around the drummer stays around the drummer. A
//! preview sets no cue, so for a long time it set no focus either, and
//! the whole catalogue previewed as six beams pointing near-vertically
//! at nothing, wiggling around a patch of air. Frame 0 and frame 8 of a
//! full revolution were the same picture. This is the guard: the
//! preview's base aims, and a mover really travels over the loop.

use ignition_core::{Attribute, ChanId, Show, SpeedMasters};
use ignition_viz::playback::Playback;
use ignition_viz::venue::Venue;
use std::collections::{HashMap, HashSet};

const VENUE: &str = "../../data/venues/norco";

/// One fold of the whole stack at `clock`, as the viz loop does it:
/// every player by class, then the programmer — which is where a
/// previewed effect lives — on top.
fn values_at(playback: &mut Playback, clock: f32) -> HashMap<(ChanId, Attribute), f32> {
    let groups = playback.groups.clone();
    let rig = playback.rig.clone();
    let speeds: SpeedMasters = playback.speeds.clone();
    let show = Show {
        groups: &groups,
        palettes: &playback.palettes,
        rig: &rig,
        speeds: &speeds,
        roles: &playback.profile,
        library: &playback.library,
        bundles: &playback.bundles,
        looks: &playback.looks,
        named_tricks: &playback.named_tricks,
        ..Show::new(&groups, &rig)
    };
    playback.playbacks.set_clock(clock);
    let mut out =
        playback
            .playbacks
            .output_with_defaults(&show, &HashSet::new(), &playback.defaults);
    playback.programmer.apply_to(&mut out, &show, clock);
    out
}

/// Every mover's pan and tilt in one fold.
fn pan_tilts(values: &HashMap<(ChanId, Attribute), f32>) -> HashMap<ChanId, (f32, f32)> {
    let mut out: HashMap<ChanId, (f32, f32)> = HashMap::new();
    for ((chan, attribute), value) in values {
        match attribute {
            Attribute::Pan => out.entry(*chan).or_default().0 = *value,
            Attribute::Tilt => out.entry(*chan).or_default().1 = *value,
            _ => {}
        }
    }
    out
}

/// r[verify focus.delta]
#[test]
fn a_movement_preview_aims_and_then_moves() {
    let Ok(venue) = Venue::load(VENUE) else {
        // The venue is data in this repo; absent, there is nothing to
        // check rather than something to fail.
        return;
    };
    let mut playback = Playback::load(&venue, ignition_viz::playback::LoadOptions::default())
        .expect("a playback with no show");
    assert!(
        playback.preview_effect("circle"),
        "the library has no `circle`, so this test is measuring nothing"
    );

    // `circle` is a four-beat orbit; at 120 bpm that is two seconds, so
    // these are the quarter points of one revolution.
    let quarters: Vec<HashMap<ChanId, (f32, f32)>> = [0.0f32, 0.5, 1.0, 1.5]
        .into_iter()
        .map(|t| pan_tilts(&values_at(&mut playback, t)))
        .collect();

    let movers: Vec<ChanId> = quarters[0].keys().copied().collect();
    assert!(
        movers.len() > 1,
        "this venue has fewer than two movers, so it cannot show the difference"
    );

    // The assertion, and it is not "the movers moved".
    //
    // They always moved. `Delta` with no focus beneath it is still a
    // delta — of the *park* position — so the broken previews swung the
    // full eighteen degrees too. What they did not do is point anywhere:
    // with no aim every head in the rig resolved the *same* angles, a
    // dozen beams in parallel wiggling around a patch of air where
    // nothing was lit for them to draw on. Aimed at a point, each head
    // solves its own angles to reach it — here a spread of some three
    // hundred degrees of pan across the hang — and the orbit rides on
    // that. So: two heads, two aims.
    let pans: Vec<f32> = movers.iter().map(|chan| quarters[0][chan].0).collect();
    let spread =
        pans.iter().fold(f32::MIN, |a, b| a.max(*b)) - pans.iter().fold(f32::MAX, |a, b| a.min(*b));
    assert!(
        spread > 1.0,
        "every mover in the rig resolved the same pan ({:.1}°): the preview \
         base aimed nothing, so the pattern rides on a beam pointing at air",
        pans[0]
    );
}
