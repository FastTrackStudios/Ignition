//! The engine's frame, out through the encoder and back through the
//! decoder the visualizer draws from, on the real Norco patch.
//!
//! One frame of *Bye Bye Bye* at a bar: every patched fixture's bytes
//! are resolved through `DmxUniverses::resolve` — the only path
//! `spawn.rs` reads — and compared with what the engine said, within
//! one byte's resolution per channel. A multipatch mirror and a parked
//! byte are checked on the same frame, because those are the two things
//! a visualizer that read attributes could never show.

// Integration test: `clippy.toml`'s test allowances only reach a
// function carrying `#[test]` directly, not a helper it calls — and
// splitting the round trip into named helpers (for `too_many_lines`) is
// exactly what moved its assertions out from under that exemption. The
// panic set is lifted here instead; see docs/ops/clippy.md.
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

use ignition_core::{Attribute, Bars, CueList, CuePlayer, Show, SpeedMasters};
use ignition_proto::{ColorChannel, DmxAddress};
use ignition_viz::DmxUniverses;
use ignition_viz::show::{OutputFrame, apply_output};
use ignition_viz::venue::Venue;
use std::collections::{BTreeMap, HashMap};

/// Patches a synthetic mirror onto the first patched fixture: an address
/// in universe 4 that nothing else uses. Must run before the patch
/// table (`venue.patch()`) is built, so the mirror is in it. Returns the
/// fixture's index and the mirror address, for the caller's own
/// byte-for-byte check against the primary.
fn add_synthetic_mirror(venue: &mut Venue) -> (usize, DmxAddress) {
    let first = venue
        .fixtures
        .iter()
        .position(|f| f.patched && f.dmx_address().is_some())
        .expect("norco has a patched fixture");
    let mirror = DmxAddress {
        universe: 4,
        start_channel: 400,
    };
    venue.fixtures[first].mirrors.push(mirror);
    (first, mirror)
}

/// Every attribute the frame carries, checked against what `dmx`
/// decodes back off the wire for that fixture's patch — within one
/// byte's rounding for a curve-encoded value, exactly for a mirror.
/// Returns how many comparisons it made, so the caller can assert the
/// sweep actually covered the rig rather than silently checking
/// nothing.
fn check_patch_round_trips<I>(
    venue: &Venue,
    values: &HashMap<(u32, Attribute), f32>,
    intents: &HashMap<u32, I>,
    dmx: &DmxUniverses,
) -> usize {
    let patch = venue.patch();
    let mut checked = 0usize;
    for (chan, fixture) in patch.iter() {
        let decoded = dmx.resolve(&fixture.address, &fixture.map);
        let value = |attr: Attribute| values.get(&(chan, attr)).copied();
        let map = &fixture.map;

        if let Some(v) = value(Attribute::Dimmer)
            && map.offset_of(&Attribute::Dimmer).is_some()
        {
            // The encoder applies the attribute's curve; the decoder
            // hands back the byte as a fraction. Compare at the byte.
            let expect = f32::from(map.curve_of(&Attribute::Dimmer).apply(v));
            let got = (decoded.dimmer * 255.0).round();
            assert!(
                (expect - got).abs() <= 1.0,
                "ch {chan} dimmer: engine {v} -> byte {expect}, decoded byte {got}"
            );
            checked += 1;
        }
        let fine_pan = map.offset_of(&Attribute::PanFine).is_some();
        let fine_tilt = map.offset_of(&Attribute::TiltFine).is_some();
        if let Some(v) = value(Attribute::Pan)
            && map.offset_of(&Attribute::Pan).is_some()
        {
            let tol = if fine_pan {
                540.0 / 65535.0
            } else {
                540.0 / 255.0
            };
            assert!(
                (decoded.pan_deg - v).abs() <= tol + 1e-3,
                "ch {chan} pan: engine {v}°, decoded {}° (tol {tol})",
                decoded.pan_deg
            );
            checked += 1;
        }
        if let Some(v) = value(Attribute::Tilt)
            && map.offset_of(&Attribute::Tilt).is_some()
        {
            let tol = if fine_tilt {
                270.0 / 65535.0
            } else {
                270.0 / 255.0
            };
            assert!(
                (decoded.tilt_deg - v).abs() <= tol + 1e-3,
                "ch {chan} tilt: engine {v}°, decoded {}° (tol {tol})",
                decoded.tilt_deg
            );
            checked += 1;
        }
        // Colour is byte-for-byte only where the frame carries no
        // intent and the fixture has nothing but an RGB triple to
        // solve onto; anything richer is solved at output by design.
        let rgb_only = !map.channels.iter().any(|(_, a)| {
            matches!(a, Attribute::ColorAdd { channel } if !matches!(channel, ColorChannel::Red | ColorChannel::Green | ColorChannel::Blue))
                || matches!(a, Attribute::ColorWheel { .. })
        });
        if rgb_only && !intents.contains_key(&chan) {
            for (i, channel) in [ColorChannel::Red, ColorChannel::Green, ColorChannel::Blue]
                .into_iter()
                .enumerate()
            {
                let attr = Attribute::ColorAdd { channel };
                if let Some(v) = value(attr.clone())
                    && map.offset_of(&attr).is_some()
                {
                    let expect = f32::from(map.curve_of(&attr).apply(v));
                    let got = (decoded.color[i] * 255.0).round();
                    assert!(
                        (expect - got).abs() <= 1.0,
                        "ch {chan} {channel:?}: engine {v} -> byte {expect}, decoded {got}"
                    );
                    checked += 1;
                }
            }
        }
    }
    checked
}

/// The mirror decodes to exactly what the primary does — not within a
/// byte, exactly, because both addresses are the same DMX bytes read
/// twice. `assert_eq!` on the resolved floats is therefore the right
/// check, not `float_cmp`'s usual worry about accumulated rounding: `==`
/// is what "the same wire value" means here.
#[expect(
    clippy::float_cmp,
    reason = "the mirror and the primary decode the identical bytes off the wire; exact \
              equality is the property under test, not a coincidence to relax with a tolerance"
)]
fn assert_mirror_matches_primary(
    dmx: &DmxUniverses,
    primary: &ignition_viz::venue::Patch,
    mirror: DmxAddress,
) {
    let at_primary = dmx.resolve(&primary.address, &primary.map);
    let at_mirror = dmx.resolve(&mirror, &primary.map);
    assert_eq!(at_primary.dimmer, at_mirror.dimmer, "mirror dimmer");
    assert_eq!(at_primary.color, at_mirror.color, "mirror colour");
    assert_eq!(at_primary.pan_deg, at_mirror.pan_deg, "mirror pan");
}

/// r[verify dmx.loopback] - the engine's values survive the wire within a byte
/// r[verify viz.driven-by-dmx] - what the viz decodes is what the engine meant
/// r[verify dmx.one-frame] - mirrors and parks are in the same frame the viz reads
#[test]
fn a_frame_of_the_show_round_trips_through_the_bytes_on_norco() {
    let Ok(mut venue) = Venue::load("../../data/venues/norco") else {
        return; // repo data absent — a runner outside the checkout
    };
    let Some(list) = std::fs::read_to_string("../../data/songs/bye-bye-bye.json")
        .ok()
        .and_then(|t| serde_json::from_str::<CueList>(&t).ok())
    else {
        return;
    };

    // A synthetic multipatch, set before the patch table is built.
    let (first, mirror) = add_synthetic_mirror(&mut venue);

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

    // A bar in the first chorus, which lights most of the rig.
    let mut player = CuePlayer::new(list.cues);
    player.set_clock(60.0);
    player.seek(Bars::new(40, 1.0), &show);
    let values = player.output(&show);
    let intents = player.output_intents(&show);
    assert!(
        values.keys().any(|(_, a)| *a == Attribute::Dimmer),
        "the frame lights something"
    );

    // A parked byte on a channel no fixture owns.
    let parked = BTreeMap::from([((4u16, 500u16), 77u8)]);

    let dmx = DmxUniverses::new();
    apply_output(
        &dmx,
        &venue,
        &OutputFrame {
            values: &values,
            intents: &intents,
            parked_dmx: &parked,
        },
    );

    let checked = check_patch_round_trips(&venue, &values, &intents, &dmx);
    assert!(checked > 20, "only {checked} attributes were compared");

    let patch = venue.patch();
    let primary = patch.get(first).expect("the first fixture is patched");
    assert_mirror_matches_primary(&dmx, primary, mirror);

    // The parked byte is on the wire, in the frame the sender takes.
    let frame = dmx.snapshot();
    assert_eq!(frame.get(&4).map(|u| u[499]), Some(77), "the parked byte");
}
