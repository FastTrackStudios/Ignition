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

//! What a show file says about its hits.
//!
//! `bye-bye-bye.json` is a real charted song — 43 cues and 113 triggers
//! — which makes it the honest place to check the claims the spec makes
//! about how the two are stored and shaped. A unit test can assert that
//! a `Trigger` *can* hold a name; only the shipped chart can show that
//! every hit in a real song actually does.

use ignition_core::{Bars, CueList, TriggerBus};

/// The repo's charted song, or `None` on a runner outside the checkout.
fn charted_song() -> Option<CueList> {
    let text = std::fs::read_to_string("../../data/songs/bye-bye-bye.json").ok()?;
    Some(serde_json::from_str(&text).expect("the charted song parses"))
}

/// Hits are kept apart from the cue list and never appear in the GO
/// order.
///
/// An operator running the list by hand presses GO for sections and
/// lifts. If the 113 hits of this song had been folded into its 43 cues,
/// GO would walk the snare pattern one press at a time.
///
/// r[verify triggers.are-not-cues]
/// r[verify files.show.triggers]
#[test]
fn a_songs_hits_are_not_in_its_go_order() {
    let Some(list) = charted_song() else {
        return;
    };

    assert!(!list.triggers.is_empty(), "the charted song has hits");
    assert!(!list.cues.is_empty(), "and sections to take by hand");

    // The GO order is the cues, and only the cues. Every hit carries a
    // name; none of those names is a cue an operator could land on.
    let cues: std::collections::HashSet<&str> = list.cues.iter().map(|c| c.name.as_str()).collect();
    for trigger in &list.triggers {
        assert!(
            !cues.contains(trigger.name.as_str()),
            "hit `{}` is also a cue, so GO would step through it",
            trigger.name
        );
    }

    // And they survive a round trip in their own field rather than
    // being flattened into the list.
    let text = serde_json::to_string(&list).expect("serialises");
    let again: CueList = serde_json::from_str(&text).expect("parses back");
    assert_eq!(again.triggers.len(), list.triggers.len());
    assert_eq!(again.cues.len(), list.cues.len());
}

/// Every hit carries a position, a recipe and a name, and every recipe
/// is one-shot.
///
/// The one-shot part is the one that bites: a looping recipe on a
/// trigger would ring until something else stopped it, which is not
/// what a hit is. `TriggerBus::retire` drops a hit when its envelope
/// finishes, and a recipe that never finishes never retires.
///
/// r[verify triggers.shape]
#[test]
fn every_charted_hit_is_named_positioned_and_one_shot() {
    let Some(list) = charted_song() else {
        return;
    };

    for trigger in &list.triggers {
        assert!(!trigger.name.trim().is_empty(), "a hit with no name");
        assert!(
            trigger.bars().is_some(),
            "hit `{}` has no bar to fire at",
            trigger.name
        );
        assert!(
            !trigger.recipe.steps.is_empty(),
            "hit `{}` fires an empty recipe",
            trigger.name
        );
        assert!(
            trigger.recipe.timing.once,
            "hit `{}` loops, so it would ring until something stopped it",
            trigger.name
        );
    }
}

/// A section cue and the hit on its downbeat land together.
///
/// The bus is driven by the same transport that seeks the cue player,
/// in the same frame — `app.rs` advances it on the line above the
/// player's `seek`. What that buys is this: on a bar carrying both, the
/// look changes and the hit fires on the same playhead move, rather
/// than the hit arriving a frame late on top of the wrong section.
///
/// The charted song has 96 such bars, so this is the ordinary case
/// rather than a contrivance.
///
/// r[verify triggers.wired]
#[test]
fn a_section_cue_and_the_hit_on_its_downbeat_land_together() {
    let Some(list) = charted_song() else {
        return;
    };

    // A bar the chart puts a hit on and the list puts a cue on.
    let cue_bars: std::collections::HashMap<u32, &str> = list
        .cues
        .iter()
        .filter_map(|c| c.position().map(|b| (b.bar, c.name.as_str())))
        .collect();
    let (bar, cue_name, hit_name) = list
        .triggers
        .iter()
        .find_map(|t| {
            let at = t.bars()?;
            cue_bars
                .get(&at.bar)
                .map(|cue| (at.bar, *cue, t.name.as_str()))
        })
        .expect("the charted song puts a hit on a section downbeat");

    // The transport arrives at that bar: one move of the playhead.
    let mut bus = TriggerBus::new(list.triggers.clone());
    bus.locate(Bars::new(bar - 1, 1.0));
    bus.advance(Bars::bar(bar), 0.0);

    assert!(
        bus.live_count() > 0,
        "bar {bar} carries cue `{cue_name}` and hit `{hit_name}`, but the move that \
         takes the cue fired nothing"
    );
}
