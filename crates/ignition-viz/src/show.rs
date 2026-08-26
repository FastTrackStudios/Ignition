//! Bridges `ignition_core::cue`'s fixture-agnostic playback engine into
//! real DMX bytes — the counterpart to `dmx.rs::resolve()` (bytes ->
//! visualizer units) running in the opposite direction (cue-engine units ->
//! bytes), written into the same `DmxUniverses` shared state real sACN/
//! Art-Net packets land in. This is what makes a programmed cue list show
//! up in `live`'s 3D view exactly like a real console's output would —
//! `scene.rs`/`build_scene` never needs to know whether a given frame came
//! from a network packet or from this module.
//!
//! Byte encoding is the literal inverse of `dmx.rs::resolve()`'s
//! byte-to-value formulas for `Pan`/`Tilt` (the only two attributes with a
//! non-linear/offset range); everything else is a plain linear 0.0-1.0
//! fraction of the byte range, including attributes `dmx.rs::resolve()`
//! doesn't model on the read side yet (`ColorWheel`/`GoboWheel` slots,
//! `Custom`) — their real per-fixture semantics aren't modelled anywhere in
//! this project yet, so this is a working default, not verified against a
//! real fixture profile.

use crate::dmx::DmxUniverses;
use crate::fixture_profile::FixtureEmitters;
use crate::venue::Venue;
use ignition_core::color::{DEFAULT_QUALITY, Intent, Rgb};
use ignition_core::{Attribute, ChanId, CuePlayer, Show};
use ignition_proto::{ChannelMap, ColorChannel};
use std::collections::HashMap;

/// `value`'s unit depends on `attr` — see the module doc and
/// `ignition_core::cue`'s own doc comment (the shared convention both
/// directions of this bridge agree on).
// r[impl playback.no-merge-at-dmx] - a pure function of one resolved value per attribute
// r[impl playback.clamp-at-output] - clamped here, after the stack is folded
fn encode_attribute_byte(attr: &Attribute, value: f32) -> u8 {
    match attr {
        Attribute::Pan => (((value / 540.0) + 0.5) * 255.0).clamp(0.0, 255.0).round() as u8,
        Attribute::Tilt => (((value / 270.0) + 0.5) * 255.0).clamp(0.0, 255.0).round() as u8,
        _ => (value.clamp(0.0, 1.0) * 255.0).round() as u8,
    }
}

/// Pan or tilt at 16 bits — `[coarse, fine]`, the coarse byte being the
/// high one, which is how every fixture with a fine channel reads it.
/// Same formula as the 8-bit case over 65535 steps, so a fixture without
/// a fine channel and one with agree on where 0° is.
fn encode_wide(range_deg: f32, value: f32) -> [u8; 2] {
    let steps = (((value / range_deg) + 0.5) * 65535.0)
        .clamp(0.0, 65535.0)
        .round() as u16;
    steps.to_be_bytes()
}

/// The bytes one resolved attribute writes at its fixture's offsets:
/// two for a pan or tilt whose personality has a fine channel, one
/// otherwise.
fn encode_attribute(map: &ChannelMap, attr: &Attribute, value: f32) -> Vec<(u16, u8)> {
    let Some(offset) = map.offset_of(attr) else {
        return Vec::new();
    };
    let wide = match attr {
        Attribute::Pan => map.offset_of(&Attribute::PanFine).map(|f| (540.0, f)),
        Attribute::Tilt => map.offset_of(&Attribute::TiltFine).map(|f| (270.0, f)),
        _ => None,
    };
    match wide {
        Some((range, fine)) => {
            let [coarse, low] = encode_wide(range, value);
            vec![(offset, coarse), (fine, low)]
        }
        None => vec![(offset, encode_attribute_byte(attr, value))],
    }
}

/// Writes one resolved cue-engine output frame into `dmx` for every
/// `(chan, attr)` pair whose fixture is actually patched (has a `chan` in
/// `venue.fixtures`, a live `dmx_address()`, and a known `ChannelMap` for
/// its manufacturer/model) — a cue targeting an unpatched or
/// unrecognized-fixture channel is silently skipped, the same "falls back
/// to static default" tolerance `scene.rs` already has for a fixture with
/// no channel map.
//
/// Colour is the one place a value is not written byte-for-byte. The cue
/// engine hands over `ColorAdd{Red,Green,Blue}` — the RGB triple every
/// preset carries — and a fixture whose map has emitters beyond those
/// three (White, Amber, UV, Lime) would otherwise leave them dark, so an
/// "Open White" on an RGBW par came out as RGB-mixed white with the
/// white LED off. Here the triple is read back as an intent (xyY) and
/// solved against the fixture's emitters, so the same preset lands on the
/// white on a four-emitter wash and on the primaries on a three-emitter
/// par. Fixtures with only RGB, or with no colour at all, take the plain
/// path.
// r[impl playback.no-merge-at-dmx] - one resolved value per (chan, attr) in; no HTP/LTP on bytes
// r[impl color.space-independent] - emitter levels are computed here, at output, per fixture
// r[impl color.emitter-solve] - the RGB reaching output is solved against the fixture's emitters
pub fn apply_cue_output(
    dmx: &DmxUniverses,
    venue: &Venue,
    output: &HashMap<(ChanId, Attribute), f32>,
) {
    // The patch is resolved once per venue and the bytes land under
    // one lock: this is per frame, and it used to rebuild the channel
    // index, re-match every fixture's channel map and take a write
    // lock per byte, every time.
    let patch = venue.patch();
    let mut bytes = Vec::with_capacity(output.len());
    let solved = solve_emitter_channels(patch, output);
    for ((chan, attr), &value) in output {
        let Some(fixture) = patch.by_chan(*chan) else {
            continue;
        };
        if matches!(attr, Attribute::ColorAdd { .. }) && solved.contains_key(chan) {
            continue; // written below, from the solve
        }
        let addr = &fixture.address;
        for (offset, byte) in encode_attribute(&fixture.map, attr, value) {
            let channel0 = addr.start_channel.saturating_sub(1) + offset;
            bytes.push((addr.universe, channel0, byte));
        }
    }
    for (chan, levels) in &solved {
        let Some(fixture) = patch.by_chan(*chan) else {
            continue;
        };
        let addr = &fixture.address;
        for (channel, level) in levels {
            let attr = Attribute::ColorAdd { channel: *channel };
            for (offset, byte) in encode_attribute(&fixture.map, &attr, *level) {
                let channel0 = addr.start_channel.saturating_sub(1) + offset;
                bytes.push((addr.universe, channel0, byte));
            }
        }
    }
    dmx.set_channels(bytes);
}

/// For every fixture in `output` that has a full RGB triple *and*
/// emitters beyond RGB, the per-emitter levels that reach the triple's
/// colour. A fixture with a partial triple (a cue touching only Red) is
/// left alone: there is no colour to solve, only a channel to write.
// r[impl color.emitter-solve]
// r[impl color.quality] - no preset reaches this point, so the fixture picks the default
fn solve_emitter_channels(
    patch: &crate::venue::PatchTable,
    output: &HashMap<(ChanId, Attribute), f32>,
) -> HashMap<ChanId, Vec<(ColorChannel, f32)>> {
    let mut triples: HashMap<ChanId, [Option<f32>; 3]> = HashMap::new();
    for ((chan, attr), &value) in output {
        let Attribute::ColorAdd { channel } = attr else {
            continue;
        };
        let slot = match channel {
            ColorChannel::Red => 0,
            ColorChannel::Green => 1,
            ColorChannel::Blue => 2,
            _ => continue,
        };
        triples.entry(*chan).or_default()[slot] = Some(value);
    }
    triples
        .into_iter()
        .filter_map(|(chan, rgb)| {
            let [Some(r), Some(g), Some(b)] = rgb else {
                return None;
            };
            let fixture = patch.by_chan(chan)?;
            let emitters = FixtureEmitters::from_channel_map(&fixture.map)?;
            if !emitters.beyond_rgb() {
                return None;
            }
            let intent = Intent::Rgb(Rgb::new(r, g, b));
            Some((chan, emitters.solve(&intent, DEFAULT_QUALITY)))
        })
        .collect()
}

/// Advances `player` by `dt_secs` and writes its resulting output into
/// `dmx` against `venue`'s patch — the one call `live.rs`'s redraw loop
/// needs each frame once a cue list is loaded.
pub fn tick_and_apply(
    dmx: &DmxUniverses,
    venue: &Venue,
    player: &mut CuePlayer,
    dt_secs: f32,
    show: &Show<'_>,
) {
    player.tick(dt_secs);
    apply_cue_output(dmx, venue, &player.output(show));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::venue::FixtureRecord;
    use ignition_core::{Cue, CueValue};
    use ignition_proto::ColorChannel;

    fn venue_with_one_uking_par(chan: u32, universe: u16, start_channel: u16) -> Venue {
        venue_with_one(chan, universe, start_channel, "Uking", "Par")
    }

    fn venue_with_one(
        chan: u32,
        universe: u16,
        start_channel: u16,
        manufacturer: &str,
        model: &str,
    ) -> Venue {
        let record: FixtureRecord = serde_json::from_value(serde_json::json!({
            "chan": chan,
            "name": "Test Fixture",
            "tags": [],
            "manufacturer": manufacturer,
            "model": model,
            "position": {"x": 0.0, "y": 0.0, "z": 0.0},
            "eulers": {"x": 0.0, "y": 0.0, "z": 0.0},
            "quat": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0},
            "size": {"x": 0.2, "y": 0.2, "z": 0.2},
            "universe": universe,
            "address": start_channel,
        }))
        .expect("valid fixture record");
        Venue {
            fixtures: vec![record],
            room: vec![],
            screens: vec![],
            props: vec![],
            group_records: vec![],
            palettes: Default::default(),
            profile: Default::default(),
            patch: Default::default(),
        }
    }

    /// End-to-end round trip: a cue targeting a real patched fixture's
    /// Dimmer/Red should, after `apply_cue_output`, resolve back out
    /// through `dmx.rs::resolve()` (the same path `scene.rs` uses) to
    /// approximately the cue's own values — proves the byte encoding
    /// this module does is the real inverse of `resolve()`'s decoding,
    /// not just internally self-consistent.
    #[test]
    /// r[verify playback.no-merge-at-dmx] - the encoder is the pure inverse of the decoder
    fn a_cue_value_round_trips_through_real_dmx_bytes_back_to_the_same_value() {
        let venue = venue_with_one_uking_par(5, 1, 10);
        let dmx = DmxUniverses::new();
        let mut player = CuePlayer::new(vec![Cue {
            name: "Cue 1".into(),
            fade_secs: 0.0,
            values: vec![
                CueValue {
                    chan: 5,
                    attr: Attribute::Dimmer,
                    value: 1.0,
                },
                CueValue {
                    chan: 5,
                    attr: Attribute::ColorAdd {
                        channel: ColorChannel::Red,
                    },
                    value: 0.5,
                },
            ],
            ..Default::default()
        }]);
        let show = Show::new(&[], &ignition_core::selection::EMPTY_RIG);
        player.go(&show);
        apply_cue_output(&dmx, &venue, &player.output(&show));

        let fixture = venue.patch().by_chan(5).unwrap();
        let resolved = dmx.resolve(&fixture.address, &fixture.map);
        assert!((resolved.dimmer - 1.0).abs() < 0.01);
        assert!((resolved.color[0] - 0.5).abs() < 0.02);
    }

    /// A cue targeting a channel with no patch entry, or one whose
    /// manufacturer/model has no known `ChannelMap`, must not panic — the
    /// same tolerance the read-side (`scene.rs`) already has.
    #[test]
    fn a_cue_targeting_an_unpatched_channel_is_silently_skipped() {
        let venue = Venue {
            fixtures: vec![],
            room: vec![],
            screens: vec![],
            props: vec![],
            group_records: vec![],
            palettes: Default::default(),
            profile: Default::default(),
            patch: Default::default(),
        };
        let dmx = DmxUniverses::new();
        let mut output = HashMap::new();
        output.insert((99, Attribute::Dimmer), 1.0);
        apply_cue_output(&dmx, &venue, &output); // must not panic
    }

    /// A mover with fine channels: 123.456° has to survive the trip to
    /// two bytes and back to well inside a step, where one byte alone
    /// would have landed 2° off.
    #[test]
    fn pan_and_tilt_round_trip_at_sixteen_bits_on_a_fine_channel_mover() {
        use crate::channel_map::channel_map_for;
        let map = channel_map_for("Betopper", "150W Beam").unwrap();
        let addr = ignition_proto::DmxAddress {
            universe: 1,
            start_channel: 1,
        };
        let dmx = DmxUniverses::new();
        let mut bytes = Vec::new();
        for (attr, deg) in [(Attribute::Pan, 123.456), (Attribute::Tilt, -71.25)] {
            for (offset, byte) in encode_attribute(&map, &attr, deg) {
                bytes.push((1, offset, byte));
            }
        }
        assert_eq!(bytes.len(), 4, "two bytes each for pan and tilt");
        dmx.set_channels(bytes);
        let resolved = dmx.resolve(&addr, &map);
        assert!(
            (resolved.pan_deg - 123.456).abs() < 0.01,
            "{}",
            resolved.pan_deg
        );
        assert!(
            (resolved.tilt_deg + 71.25).abs() < 0.01,
            "{}",
            resolved.tilt_deg
        );
    }

    #[test]
    /// r[verify playback.no-merge-at-dmx]
    fn pan_zero_degrees_encodes_to_the_dmx_centre_byte() {
        assert_eq!(encode_attribute_byte(&Attribute::Pan, 0.0), 128);
        assert_eq!(encode_attribute_byte(&Attribute::Tilt, 0.0), 128);
    }

    fn rgb_output(chan: u32, r: f32, g: f32, b: f32) -> HashMap<(ChanId, Attribute), f32> {
        let mut output = HashMap::new();
        for (channel, v) in [
            (ColorChannel::Red, r),
            (ColorChannel::Green, g),
            (ColorChannel::Blue, b),
        ] {
            output.insert((chan, Attribute::ColorAdd { channel }), v);
        }
        output
    }

    /// Reads one raw byte back through `resolve()`'s Dimmer path (a
    /// one-channel map at that address) — `DmxUniverses` keeps its
    /// frames private.
    fn byte_at(dmx: &DmxUniverses, universe: u16, channel1: u16) -> u8 {
        let probe = ChannelMap {
            footprint: 1,
            channels: vec![(0, Attribute::Dimmer)],
        };
        let addr = ignition_proto::DmxAddress {
            universe,
            start_channel: channel1,
        };
        (dmx.resolve(&addr, &probe).dimmer * 255.0).round() as u8
    }

    #[test]
    /// r[verify color.emitter-solve] - white on an RGBW fixture lights its white emitter
    /// r[verify color.cct] - the white emitter is preferred for a white
    fn white_on_an_rgbw_fixture_ends_with_white_lit() {
        // "RGBW Spot Light 6ch": R,G,B,W at offsets 1..=4.
        let venue = venue_with_one(3, 1, 1, "Generic", "RGBW Spot Light 6ch");
        let dmx = DmxUniverses::new();
        apply_cue_output(&dmx, &venue, &rgb_output(3, 1.0, 1.0, 1.0));
        let white = byte_at(&dmx, 1, 5);
        assert!(white > 128, "white emitter carries a white: {white}");
        let blue = byte_at(&dmx, 1, 4);
        assert!(blue < white, "RGB no longer does all the work: blue {blue}");

        // A saturated red leaves the white off.
        let dmx = DmxUniverses::new();
        apply_cue_output(&dmx, &venue, &rgb_output(3, 1.0, 0.0, 0.0));
        // sRGB red is less saturated than a red LED, so a few percent
        // of white is the right answer, not a leak.
        assert!(byte_at(&dmx, 1, 5) < 20, "{}", byte_at(&dmx, 1, 5));
        assert!(byte_at(&dmx, 1, 2) > 200, "{}", byte_at(&dmx, 1, 2));
    }

    #[test]
    /// r[verify color.emitter-solve] - a three-emitter par takes the triple as written
    /// r[verify color.space-independent] - the same intent, rendered against three emitters instead of four
    fn white_on_a_three_emitter_par_writes_the_triple_verbatim() {
        let venue = venue_with_one_uking_par(5, 1, 10);
        let dmx = DmxUniverses::new();
        apply_cue_output(&dmx, &venue, &rgb_output(5, 1.0, 1.0, 1.0));
        // Uking Par: Dimmer, R, G, B, Strobe — no White at all.
        assert_eq!(byte_at(&dmx, 1, 11), 255);
        assert_eq!(byte_at(&dmx, 1, 12), 255);
        assert_eq!(byte_at(&dmx, 1, 13), 255);
        assert_eq!(byte_at(&dmx, 1, 14), 0, "strobe untouched");
    }

    #[test]
    /// r[verify color.emitter-solve] - a lone channel is a channel, not a colour
    fn a_partial_triple_is_written_as_plain_channels() {
        let venue = venue_with_one(3, 1, 1, "Generic", "RGBW Spot Light 6ch");
        let dmx = DmxUniverses::new();
        let mut output = HashMap::new();
        output.insert(
            (
                3,
                Attribute::ColorAdd {
                    channel: ColorChannel::Red,
                },
            ),
            1.0,
        );
        apply_cue_output(&dmx, &venue, &output);
        assert_eq!(byte_at(&dmx, 1, 2), 255);
        assert_eq!(byte_at(&dmx, 1, 5), 0);
    }
}
