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

use crate::channel_map::channel_map_for;
use crate::dmx::DmxUniverses;
use crate::venue::{FixtureRecord, Venue};
use ignition_core::{Attribute, ChanId, CuePlayer, Show};
use std::collections::HashMap;

/// `value`'s unit depends on `attr` — see the module doc and
/// `ignition_core::cue`'s own doc comment (the shared convention both
/// directions of this bridge agree on).
fn encode_attribute_byte(attr: &Attribute, value: f32) -> u8 {
    match attr {
        Attribute::Pan => (((value / 540.0) + 0.5) * 255.0).clamp(0.0, 255.0).round() as u8,
        Attribute::Tilt => (((value / 270.0) + 0.5) * 255.0).clamp(0.0, 255.0).round() as u8,
        _ => (value.clamp(0.0, 1.0) * 255.0).round() as u8,
    }
}

/// Writes one resolved cue-engine output frame into `dmx` for every
/// `(chan, attr)` pair whose fixture is actually patched (has a `chan` in
/// `venue.fixtures`, a live `dmx_address()`, and a known `ChannelMap` for
/// its manufacturer/model) — a cue targeting an unpatched or
/// unrecognized-fixture channel is silently skipped, the same "falls back
/// to static default" tolerance `scene.rs` already has for a fixture with
/// no channel map.
pub fn apply_cue_output(
    dmx: &DmxUniverses,
    venue: &Venue,
    output: &HashMap<(ChanId, Attribute), f32>,
) {
    let by_chan: HashMap<u32, &FixtureRecord> = venue
        .fixtures
        .iter()
        .filter_map(|f| f.chan.map(|c| (c, f)))
        .collect();
    for ((chan, attr), &value) in output {
        let Some(fixture) = by_chan.get(chan) else {
            continue;
        };
        let Some(addr) = fixture.dmx_address() else {
            continue;
        };
        let manufacturer = fixture.manufacturer.as_deref().unwrap_or("");
        let model = fixture.model.as_deref().unwrap_or("");
        let Some(map) = channel_map_for(manufacturer, model) else {
            continue;
        };
        let Some(offset) = map.offset_of(attr) else {
            continue;
        };
        let channel0 = addr.start_channel.saturating_sub(1) + offset;
        dmx.set_channel(addr.universe, channel0, encode_attribute_byte(attr, value));
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
    apply_cue_output(dmx, venue, &player.output(show));
}

/// The `EffectPlayer` counterpart to `tick_and_apply` — advances `player`
/// and writes its continuous output into `dmx` against `venue`'s real
/// groups. `live.rs` ticks/applies a loaded `CuePlayer` first and this
/// second each redraw, so a running effect layers on top of (and can
/// override) whatever a cue set — the same HTP-ish "effect rides on top
/// of the cue" behaviour a real console has, though without true relative/
/// HTP blending (this is a flat last-write-wins per `(chan, attr)` byte,
/// same as everywhere else in this project's DMX write path).
pub fn tick_and_apply_effects(
    dmx: &DmxUniverses,
    venue: &Venue,
    player: &mut ignition_core::EffectPlayer,
    dt_secs: f32,
) {
    player.tick(dt_secs);
    let groups = venue.groups();
    apply_cue_output(dmx, venue, &player.output(&groups));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignition_core::{Cue, CueValue};
    use ignition_proto::ColorChannel;

    fn venue_with_one_uking_par(chan: u32, universe: u16, start_channel: u16) -> Venue {
        let record: FixtureRecord = serde_json::from_value(serde_json::json!({
            "chan": chan,
            "name": "Test Par",
            "tags": [],
            "manufacturer": "Uking",
            "model": "Par",
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
        }
    }

    /// End-to-end round trip: a cue targeting a real patched fixture's
    /// Dimmer/Red should, after `apply_cue_output`, resolve back out
    /// through `dmx.rs::resolve()` (the same path `scene.rs` uses) to
    /// approximately the cue's own values — proves the byte encoding
    /// this module does is the real inverse of `resolve()`'s decoding,
    /// not just internally self-consistent.
    #[test]
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
        let show = Show::new(&[], &|_| None);
        player.go(&show);
        apply_cue_output(&dmx, &venue, &player.output(&show));

        let fixture = &venue.fixtures[0];
        let map = channel_map_for("Uking", "Par").unwrap();
        let resolved = dmx.resolve(&fixture.dmx_address().unwrap(), &map);
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
        };
        let dmx = DmxUniverses::new();
        let mut output = HashMap::new();
        output.insert((99, Attribute::Dimmer), 1.0);
        apply_cue_output(&dmx, &venue, &output); // must not panic
    }

    #[test]
    fn pan_zero_degrees_encodes_to_the_dmx_centre_byte() {
        assert_eq!(encode_attribute_byte(&Attribute::Pan, 0.0), 128);
        assert_eq!(encode_attribute_byte(&Attribute::Tilt, 0.0), 128);
    }
}
