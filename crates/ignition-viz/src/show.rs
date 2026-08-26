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
//! non-linear/offset range); everything else is the attribute's 0.0-1.0
//! value through its channel map's `Curve` (linear unless the venue's
//! fixture type says otherwise), including attributes `dmx.rs::resolve()`
//! doesn't model on the read side yet (`GoboWheel` slots, `Custom`).
//!
//! Colour is the one place a value is not written byte-for-byte: a
//! fixture whose frame carries a colour *intent* has that intent solved
//! against its own emitters, or snapped to its nearest wheel slot, here —
//! see `apply_output`.

use crate::dmx::DmxUniverses;
use crate::fixture_profile::{FixtureEmitters, FixtureProfile};
use crate::venue::{Patch, Venue};
use ignition_core::color::{ColorPreference, DEFAULT_QUALITY, Intent, Rgb, nearest_wheel_slot};
use ignition_core::{Attribute, ChanId, CuePlayer, Show};
use ignition_proto::{ChannelMap, ColorChannel};
use std::collections::{BTreeMap, HashMap};

/// `value`'s unit depends on `attr` — see the module doc and
/// `ignition_core::cue`'s own doc comment (the shared convention both
/// directions of this bridge agree on).
// r[impl playback.no-merge-at-dmx] - a pure function of one resolved value per attribute
// r[impl playback.clamp-at-output] - clamped here, after the stack is folded
// r[impl files.venue.dmx-curves] - the attribute's curve decides the wire byte
fn encode_attribute_byte(map: &ChannelMap, attr: &Attribute, value: f32) -> u8 {
    match attr {
        Attribute::Pan => (((value / 540.0) + 0.5) * 255.0).clamp(0.0, 255.0).round() as u8,
        Attribute::Tilt => (((value / 270.0) + 0.5) * 255.0).clamp(0.0, 255.0).round() as u8,
        _ => map.curve_of(attr).apply(value),
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
        None => vec![(offset, encode_attribute_byte(map, attr, value))],
    }
}

/// One resolved frame from the playback stack, everything the output
/// stage needs to put it on the wire.
pub struct OutputFrame<'a> {
    /// One value per `(chan, attr)` — the folded stack.
    pub values: &'a HashMap<(ChanId, Attribute), f32>,
    /// The colour each fixture is meant to be, where a cue said so in
    /// something richer than a triple. Solved here against the fixture.
    // r[impl color.intent-to-output]
    pub intents: &'a HashMap<ChanId, Intent>,
    /// Raw parked DMX, `(universe, 1-based channel) -> byte`, written
    /// last over everything the frame encoded.
    // r[impl playback.park] - a parked DMX channel is the value on the wire
    pub parked_dmx: &'a BTreeMap<(u16, u16), u8>,
}

impl Default for OutputFrame<'_> {
    /// An empty frame: nothing to write.
    fn default() -> Self {
        static VALUES: std::sync::LazyLock<HashMap<(ChanId, Attribute), f32>> =
            std::sync::LazyLock::new(HashMap::new);
        static INTENTS: std::sync::LazyLock<HashMap<ChanId, Intent>> =
            std::sync::LazyLock::new(HashMap::new);
        static PARKED: BTreeMap<(u16, u16), u8> = BTreeMap::new();
        Self {
            values: &VALUES,
            intents: &INTENTS,
            parked_dmx: &PARKED,
        }
    }
}

/// Writes one resolved cue-engine output frame into `dmx` for every
/// `(chan, attr)` pair whose fixture is actually patched (has a `chan` in
/// `venue.fixtures`, a live `dmx_address()`, and a known `ChannelMap` for
/// its manufacturer/model) — a cue targeting an unpatched or
/// unrecognized-fixture channel is silently skipped, the same "falls back
/// to static default" tolerance `scene.rs` already has for a fixture with
/// no channel map. Attribute values only; see `apply_output` for the
/// full frame.
pub fn apply_cue_output(
    dmx: &DmxUniverses,
    venue: &Venue,
    output: &HashMap<(ChanId, Attribute), f32>,
) {
    apply_output(
        dmx,
        venue,
        &OutputFrame {
            values: output,
            ..Default::default()
        },
    );
}

/// Writes a whole frame — values, colour intents and parked DMX — into
/// `dmx` against `venue`'s patch, every mirror of a multipatched fixture
/// included.
///
/// Colour is the one place a value is not written byte-for-byte. Where
/// the frame carries an intent for a fixture, the intent (read in the
/// fixture type's declared colour space) is solved against that
/// fixture's emitters, or snapped to its nearest wheel slot, per the
/// type's preference — so "3200 K" lands on the white LED of a four-
/// emitter wash and on the amber gel of a wheel spot. Without an intent
/// the `ColorAdd{Red,Green,Blue}` triple every older cue carries is read
/// back as an RGB intent and solved the same way when the fixture has
/// emitters beyond those three; fixtures with only RGB, or with no
/// colour at all, take the plain path.
// r[impl playback.no-merge-at-dmx] - one resolved value per (chan, attr) in; no HTP/LTP on bytes
// r[impl color.space-independent] - emitter levels are computed here, at output, per fixture
// r[impl color.emitter-solve] - the colour reaching output is solved against the fixture's emitters
// r[impl color.intent-to-output] - the intent, not a re-derived triple, is what is solved
// r[impl color.mix-or-wheel] - mixing or the nearest slot, per the fixture type's preference
// r[impl files.venue.multipatch] - every mirror receives the same bytes
// r[impl playback.park] - parked DMX overlays the encoded frame
pub fn apply_output(dmx: &DmxUniverses, venue: &Venue, frame: &OutputFrame<'_>) {
    // The patch is resolved once per venue and the bytes land under
    // one lock: this is per frame, and it used to rebuild the channel
    // index, re-match every fixture's channel map and take a write
    // lock per byte, every time.
    let patch = venue.patch();
    let mut bytes = Vec::with_capacity(frame.values.len());
    let solved = solve_colors(patch, frame.values, frame.intents);
    let mut push = |fixture: &Patch, offset: u16, byte: u8| {
        for addr in fixture.addresses() {
            let channel0 = addr.start_channel.saturating_sub(1) + offset;
            bytes.push((addr.universe, channel0, byte));
        }
    };
    for ((chan, attr), &value) in frame.values {
        let Some(fixture) = patch.by_chan(*chan) else {
            continue;
        };
        if let Some(color) = solved.get(chan) {
            let taken = match attr {
                Attribute::ColorAdd { .. } => color.emitters.is_some(),
                Attribute::ColorWheel { .. } => color.wheel.is_some(),
                _ => false,
            };
            if taken {
                continue; // written below, from the solve
            }
        }
        for (offset, byte) in encode_attribute(&fixture.map, attr, value) {
            push(fixture, offset, byte);
        }
    }
    for (chan, color) in &solved {
        let Some(fixture) = patch.by_chan(*chan) else {
            continue;
        };
        for (channel, level) in color.emitters.iter().flatten() {
            let attr = Attribute::ColorAdd { channel: *channel };
            for (offset, byte) in encode_attribute(&fixture.map, &attr, *level) {
                push(fixture, offset, byte);
            }
        }
        if let Some(slot) = color.wheel
            && let Some(offset) = wheel_offset(&fixture.map)
        {
            push(fixture, offset, slot);
        }
    }
    for ((universe, channel1), byte) in frame.parked_dmx {
        if let Some(channel0) = channel1.checked_sub(1) {
            bytes.push((*universe, channel0, *byte));
        }
    }
    dmx.set_channels(bytes);
}

/// The offset of a map's colour wheel channel, whatever slot index the
/// map spelled it with.
fn wheel_offset(map: &ChannelMap) -> Option<u16> {
    map.channels
        .iter()
        .find(|(_, a)| matches!(a, Attribute::ColorWheel { .. }))
        .map(|(o, _)| *o)
}

/// What one fixture's colour resolved to: emitter levels to write over
/// its `ColorAdd` channels, and/or a wheel byte. `None` in a field means
/// that channel group is not the solve's to write.
#[derive(Debug, Default, PartialEq)]
struct SolvedColor {
    emitters: Option<Vec<(ColorChannel, f32)>>,
    wheel: Option<u8>,
}

/// The colour of every fixture the frame says something about, solved
/// against that fixture's profile: an intent where the frame carries
/// one, else the fixture's full RGB triple where it has emitters beyond
/// RGB. A fixture with a partial triple and no intent (a cue touching
/// only Red) is left alone: there is no colour to solve, only a channel
/// to write.
// r[impl color.emitter-solve]
// r[impl color.intent-to-output]
// r[impl color.mix-or-wheel]
// r[impl color.spaces] - an RGB intent is read in the fixture's declared space first
// r[impl color.quality] - no preset reaches this point, so the fixture picks the default
fn solve_colors(
    patch: &crate::venue::PatchTable,
    values: &HashMap<(ChanId, Attribute), f32>,
    intents: &HashMap<ChanId, Intent>,
) -> HashMap<ChanId, SolvedColor> {
    let mut out = HashMap::new();
    for (chan, intent) in intents {
        let Some(fixture) = patch.by_chan(*chan) else {
            continue;
        };
        if let Some(color) = solve_intent(&fixture.profile, intent) {
            out.insert(*chan, color);
        }
    }
    let mut triples: HashMap<ChanId, [Option<f32>; 3]> = HashMap::new();
    for ((chan, attr), &value) in values {
        if out.contains_key(chan) {
            continue;
        }
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
    for (chan, rgb) in triples {
        let [Some(r), Some(g), Some(b)] = rgb else {
            continue;
        };
        let Some(fixture) = patch.by_chan(chan) else {
            continue;
        };
        let Some(emitters) = FixtureEmitters::from_channel_map(&fixture.map) else {
            continue;
        };
        if !emitters.beyond_rgb() {
            continue;
        }
        let intent = Intent::Rgb(Rgb::new(r, g, b));
        out.insert(
            chan,
            SolvedColor {
                emitters: Some(emitters.solve(&intent, DEFAULT_QUALITY)),
                wheel: None,
            },
        );
    }
    out
}

/// One intent against one fixture type. Mixing where the type prefers
/// it and can; otherwise the nearest wheel slot, with any mixing held
/// at white so the gel is what colours the beam. `None` for a type with
/// no colour system at all.
// r[impl color.intent-to-output]
// r[impl color.mix-or-wheel]
fn solve_intent(profile: &FixtureProfile, intent: &Intent) -> Option<SolvedColor> {
    let intent = profile.color_space.interpret(intent);
    let mix = profile.emitters.as_ref();
    let wheel = (!profile.wheel.is_empty()).then(|| profile.wheel_slots());
    match (profile.color_preference, mix, wheel) {
        (ColorPreference::Mix, Some(emitters), _)
        | (ColorPreference::Wheel, Some(emitters), None) => Some(SolvedColor {
            emitters: Some(emitters.solve(&intent, DEFAULT_QUALITY)),
            wheel: None,
        }),
        (_, mix, Some(slots)) => {
            let slot = nearest_wheel_slot(&intent, &slots)?;
            let white = Intent::Rgb(Rgb::WHITE);
            Some(SolvedColor {
                emitters: mix.map(|e| e.solve(&white, DEFAULT_QUALITY)),
                wheel: Some(slot),
            })
        }
        (_, None, None) => None,
    }
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
    apply_output(
        dmx,
        venue,
        &OutputFrame {
            values: &player.output(show),
            intents: &player.output_intents(show),
            ..Default::default()
        },
    );
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
        venue_of(record)
    }

    fn venue_of(record: FixtureRecord) -> Venue {
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
        let map = ChannelMap::new(2, vec![(0, Attribute::Pan), (1, Attribute::Tilt)]);
        assert_eq!(encode_attribute_byte(&map, &Attribute::Pan, 0.0), 128);
        assert_eq!(encode_attribute_byte(&map, &Attribute::Tilt, 0.0), 128);
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
            curves: Default::default(),
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

    fn intents_of(chan: u32, intent: Intent) -> HashMap<ChanId, Intent> {
        HashMap::from([(chan, intent)])
    }

    #[test]
    /// r[verify color.intent-to-output] - a CCT reaches the emitters as a CCT, not as a triple
    fn a_cct_intent_on_an_rgbw_par_lands_on_the_white() {
        let venue = venue_with_one(3, 1, 1, "Generic", "RGBW Spot Light 6ch");
        let dmx = DmxUniverses::new();
        // The frame also carries the stale RGB triple an older cue would
        // have written; the intent wins over it.
        let values = rgb_output(3, 1.0, 0.2, 0.0);
        let intents = intents_of(
            3,
            Intent::Cct {
                kelvin: 3200.0,
                tint: 0.0,
            },
        );
        apply_output(
            &dmx,
            &venue,
            &OutputFrame {
                values: &values,
                intents: &intents,
                ..Default::default()
            },
        );
        let (r, g, b, w) = (
            byte_at(&dmx, 1, 2),
            byte_at(&dmx, 1, 3),
            byte_at(&dmx, 1, 4),
            byte_at(&dmx, 1, 5),
        );
        // 3200 K is a long way warmer than the class-typical white LED
        // (~5600 K), so the red is pulled up to full alongside the white;
        // in light the white still carries the most, and the blue is
        // off. The stale triple would have left the white at zero.
        assert!(
            w > 100,
            "the white emitter carries the warm white: r{r} g{g} b{b} w{w}"
        );
        let flux = |byte: u8, ch: ColorChannel| {
            byte as f32 / 255.0 * crate::fixture_profile::typical_emitter(ch).max_lumens
        };
        assert!(
            flux(w, ColorChannel::White) > flux(r, ColorChannel::Red)
                && flux(w, ColorChannel::White) > flux(g, ColorChannel::Green),
            "white dominates in light: r{r} g{g} b{b} w{w}"
        );
        assert_eq!(b, 0, "3200 K has no blue in it");
        assert!(
            g > 60,
            "the intent, not the stale triple, set the green: g{g}"
        );

        // Without the intent the same values are the plain triple, solved
        // as an RGB — a saturated orange, with far less white in it.
        let dmx = DmxUniverses::new();
        apply_cue_output(&dmx, &venue, &values);
        let fallback_w = byte_at(&dmx, 1, 5);
        assert!(fallback_w < w / 2, "triple w{fallback_w} vs intent w{w}");
    }

    #[test]
    /// r[verify color.mix-or-wheel] - a wheel-only spot takes its nearest gel
    fn a_colour_intent_on_a_wheel_mover_selects_the_nearest_slot() {
        // Riukoe 11ch: colour wheel at offset 4.
        let venue = venue_with_one(80, 1, 1, "Riukoe", "Gobo 11ch");
        let dmx = DmxUniverses::new();
        let values = HashMap::from([((80, Attribute::Dimmer), 1.0)]);
        let intents = intents_of(80, Intent::Rgb(Rgb::new(0.0, 0.0, 1.0)));
        apply_output(
            &dmx,
            &venue,
            &OutputFrame {
                values: &values,
                intents: &intents,
                ..Default::default()
            },
        );
        assert_eq!(byte_at(&dmx, 1, 5), 27, "blue is the fourth slot");
        assert_eq!(byte_at(&dmx, 1, 8), 255, "dimmer untouched");

        // A gel: Lee 201 is a CT blue, still nearest the blue-ish slots.
        let dmx = DmxUniverses::new();
        let intents = intents_of(
            80,
            Intent::Cct {
                kelvin: 2500.0,
                tint: 0.0,
            },
        );
        apply_output(
            &dmx,
            &venue,
            &OutputFrame {
                values: &values,
                intents: &intents,
                ..Default::default()
            },
        );
        let slot = byte_at(&dmx, 1, 5);
        assert!(
            slot == 59 || slot == 35,
            "a warm white lands on orange or yellow: {slot}"
        );
    }

    #[test]
    /// r[verify color.mix-or-wheel] - the preference is per type, and Wheel holds the mix at white
    fn a_type_that_prefers_its_wheel_holds_the_mixing_white() {
        let map = ChannelMap::new(
            5,
            vec![
                (0, Attribute::Dimmer),
                (
                    1,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Red,
                    },
                ),
                (
                    2,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Green,
                    },
                ),
                (
                    3,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Blue,
                    },
                ),
                (4, Attribute::ColorWheel { slot: 0 }),
            ],
        );
        let mut profile = FixtureProfile::from_channel_map(map).with_wheel(vec![
            crate::fixture_profile::ColorWheelSlot::xy("Open", 0, 0.313, 0.329),
            crate::fixture_profile::ColorWheelSlot::xy("Red", 40, 0.68, 0.31),
        ]);
        assert_eq!(profile.color_preference, ColorPreference::Mix);
        let red = Intent::Rgb(Rgb::new(1.0, 0.0, 0.0));
        let mixed = solve_intent(&profile, &red).unwrap();
        assert_eq!(mixed.wheel, None);
        assert!(mixed.emitters.unwrap()[0].1 > 0.9);

        profile.color_preference = ColorPreference::Wheel;
        let snapped = solve_intent(&profile, &red).unwrap();
        assert_eq!(snapped.wheel, Some(40));
        let levels = snapped.emitters.unwrap();
        // The fixture's own white: D65 on these three LEDs is not 1/1/1.
        assert!(
            levels.iter().all(|(_, v)| *v > 0.5),
            "mix at white: {levels:?}"
        );
    }

    #[test]
    /// r[verify color.spaces] - the fixture's declared space changes what a triple means
    fn an_rgb_intent_is_read_in_the_fixtures_declared_space() {
        let map = crate::channel_map::channel_map_for("x", "RGBW Spot Light 6ch").unwrap();
        let mut profile = FixtureProfile::from_channel_map(map);
        let intent = Intent::Rgb(Rgb::new(0.0, 0.0, 1.0));
        let srgb = solve_intent(&profile, &intent).unwrap();
        profile.color_space = crate::fixture_profile::DeclaredColorSpace::Known(
            ignition_core::color::ColorSpace::Rec2020,
        );
        let wide = solve_intent(&profile, &intent).unwrap();
        let white = |c: &SolvedColor| {
            c.emitters
                .as_ref()
                .unwrap()
                .iter()
                .find(|(ch, _)| *ch == ColorChannel::White)
                .map(|(_, v)| *v)
                .unwrap()
        };
        assert!(
            white(&wide) < white(&srgb) || white(&srgb) < 0.02,
            "a Rec.2020 blue is more saturated: {:?} vs {:?}",
            wide,
            srgb
        );
        assert_ne!(wide, srgb);
    }

    #[test]
    /// r[verify files.venue.multipatch] - every mirror gets the bytes the primary gets
    fn a_multipatched_fixture_writes_every_address() {
        let record: FixtureRecord = serde_json::from_value(serde_json::json!({
            "chan": 5,
            "name": "House pars",
            "tags": [],
            "manufacturer": "Uking",
            "model": "Par",
            "position": {"x": 0.0, "y": 0.0, "z": 0.0},
            "eulers": {"x": 0.0, "y": 0.0, "z": 0.0},
            "quat": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0},
            "size": {"x": 0.2, "y": 0.2, "z": 0.2},
            "universe": 1,
            "address": 10,
            "mirrors": [{"universe": 1, "start_channel": 200}, {"universe": 3, "start_channel": 1}],
        }))
        .unwrap();
        let venue = venue_of(record);
        let dmx = DmxUniverses::new();
        let mut values = rgb_output(5, 1.0, 0.5, 0.0);
        values.insert((5, Attribute::Dimmer), 0.8);
        apply_cue_output(&dmx, &venue, &values);
        for (universe, start) in [(1u16, 10u16), (1, 200), (3, 1)] {
            assert_eq!(
                byte_at(&dmx, universe, start),
                204,
                "dimmer at {universe}/{start}"
            );
            assert_eq!(byte_at(&dmx, universe, start + 1), 255);
            assert_eq!(byte_at(&dmx, universe, start + 2), 128);
            assert_eq!(byte_at(&dmx, universe, start + 3), 0);
        }
    }

    #[test]
    /// r[verify files.venue.dmx-curves] - a curve on the map corrects the byte at output
    fn a_dimmer_curve_is_applied_at_encode() {
        let mut map = ChannelMap::new(1, vec![(0, Attribute::Dimmer)]);
        assert_eq!(
            encode_attribute(&map, &Attribute::Dimmer, 0.5),
            vec![(0, 128)]
        );
        map.curves.insert(
            Attribute::Dimmer,
            ignition_proto::Curve::Range { lo: 16, hi: 240 },
        );
        assert_eq!(
            encode_attribute(&map, &Attribute::Dimmer, 0.0),
            vec![(0, 16)]
        );
        assert_eq!(
            encode_attribute(&map, &Attribute::Dimmer, 1.0),
            vec![(0, 240)]
        );
        map.curves.insert(
            Attribute::Dimmer,
            ignition_proto::Curve::Lut(vec![0, 64, 255]),
        );
        assert_eq!(
            encode_attribute(&map, &Attribute::Dimmer, 0.5),
            vec![(0, 64)]
        );
        // Pan is in degrees and never goes through a curve.
        let mut pan = ChannelMap::new(1, vec![(0, Attribute::Pan)]);
        pan.curves.insert(
            Attribute::Pan,
            ignition_proto::Curve::Range { lo: 0, hi: 1 },
        );
        assert_eq!(encode_attribute(&pan, &Attribute::Pan, 0.0), vec![(0, 128)]);
    }

    #[test]
    /// r[verify playback.park] - a parked DMX channel is the value on the wire, after encode
    fn parked_dmx_overlays_the_encoded_frame() {
        let venue = venue_with_one_uking_par(5, 1, 10);
        let dmx = DmxUniverses::new();
        let values = HashMap::from([((5, Attribute::Dimmer), 1.0)]);
        let parked = BTreeMap::from([((1u16, 10u16), 7u8), ((2, 1), 99)]);
        apply_output(
            &dmx,
            &venue,
            &OutputFrame {
                values: &values,
                intents: &HashMap::new(),
                parked_dmx: &parked,
            },
        );
        assert_eq!(
            byte_at(&dmx, 1, 10),
            7,
            "the park beats the cue's full dimmer"
        );
        assert_eq!(byte_at(&dmx, 2, 1), 99, "a park needs no fixture at all");
    }
}
