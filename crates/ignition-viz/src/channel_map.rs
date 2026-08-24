//! Per-fixture-type DMX channel layouts — which byte offset resolves to
//! which `Attribute`, for each manufacturer/model this venue actually
//! patches. Mirrors `fixture_profile.rs`'s manufacturer/model matching
//! (substring, case-insensitive) deliberately: shape and channel layout are
//! two independent facts about a fixture type, looked up the same way.
//!
//! Confidence varies per entry — see `docs/domain/dmx-channel-maps.md`.
//! `footprint` (the real channel count) is confirmed for every fixture here
//! by checking the live patch's own DMX-address spacing between consecutive
//! fixtures of that type (`docs/domain/norco-patch-and-groups.md`'s
//! `global_address` data); the *per-channel function order* is the
//! estimated part, taken from the class of fixture (RGB(W) par, cheap
//! moving head, LED batten, hazer) since Ignition has no real `.qxf`/GDTF
//! file for any of these specific models yet. Treat as a working default,
//! not ground truth — swap for a real fixture profile import
//! (`docs/research/lighting-console-landscape.md`'s GDTF/MVR slice) once
//! one exists for these models.

use ignition_proto::{Attribute, ChannelMap, ColorChannel};

fn rgb_par(footprint: u16, dimmer_channel: Option<u16>, rgb_start: u16) -> ChannelMap {
    let mut channels = Vec::new();
    if let Some(d) = dimmer_channel {
        channels.push((d, Attribute::Dimmer));
    }
    channels.push((rgb_start, Attribute::ColorAdd { channel: ColorChannel::Red }));
    channels.push((rgb_start + 1, Attribute::ColorAdd { channel: ColorChannel::Green }));
    channels.push((rgb_start + 2, Attribute::ColorAdd { channel: ColorChannel::Blue }));
    ChannelMap { footprint, channels }
}

/// `manufacturer`/`model` match the same way `fixture_profile::shape_for`
/// does — verbatim strings from the live Eos patch, matched case-insensitive
/// substring.
pub fn channel_map_for(manufacturer: &str, model: &str) -> Option<ChannelMap> {
    let m = manufacturer.to_ascii_lowercase();
    let mo = model.to_ascii_lowercase();

    if m == "uking" && mo.contains("par") {
        // Footprint confirmed: chan 1 -> chan 3 is 7 DMX addresses apart in
        // the live patch (both 2 channel-numbers apart, so 1 fixture's worth
        // of addresses = 7). Layout estimated as the common budget-par 7ch
        // convention: Master Dimmer, R, G, B, White, Strobe, Program.
        let mut cm = rgb_par(7, Some(0), 1);
        cm.channels.push((4, Attribute::ColorAdd { channel: ColorChannel::White }));
        cm.channels.push((5, Attribute::Strobe));
        return Some(cm);
    }
    if m == "chauvet" && mo.contains("slimpar") {
        // Footprint confirmed: chan 50 -> chan 51 is 7 addresses apart,
        // matching "7ch" in the model name exactly. Layout estimated per
        // Chauvet's typical SlimPAR 7ch personality: Dimmer, R, G, B,
        // Strobe, Color macro, macro speed — the last two aren't modelled
        // here (no macro/speed attribute exists yet), so this fixture's
        // real footprint is 7 but only 4 of its channels resolve to
        // anything the visualizer reads.
        return Some(rgb_par(7, Some(0), 1));
    }
    if (m == "riukoe" || m == "lixada") && mo.contains("gobo") {
        // Footprint confirmed: chan 80 -> chan 81 is 11 addresses apart,
        // matching "11ch" in the model name exactly. Layout estimated per
        // the common cheap mini-moving-head convention: Pan, Pan fine,
        // Tilt, Tilt fine, Colour wheel, Gobo wheel, Shutter/strobe,
        // Dimmer, Speed, Special, Reset. Only the coarse Pan/Tilt/Dimmer
        // bytes are read today (see `dmx.rs`'s 8-bit resolution) — the fine
        // bytes exist in the footprint so the *next* fixture's addressing
        // stays correct, they just aren't consumed for extra precision yet.
        return Some(ChannelMap {
            footprint: 11,
            channels: vec![
                (0, Attribute::Pan),
                (2, Attribute::Tilt),
                (4, Attribute::ColorWheel { slot: 0 }),
                (5, Attribute::GoboWheel { slot: 0 }),
                (6, Attribute::Strobe),
                (7, Attribute::Dimmer),
            ],
        });
    }
    if m == "betopper" {
        // Footprint confirmed: sorted by live address, consecutive
        // Betopper units are 12 addresses apart. Layout estimated per a
        // generic cheap 12ch beam-mover convention: Pan, Pan fine, Tilt,
        // Tilt fine, Speed, Dimmer, Strobe, Colour wheel, Gobo wheel,
        // Prism, Focus, Reset.
        return Some(ChannelMap {
            footprint: 12,
            channels: vec![
                (0, Attribute::Pan),
                (2, Attribute::Tilt),
                (5, Attribute::Dimmer),
                (6, Attribute::Strobe),
                (7, Attribute::ColorWheel { slot: 0 }),
                (8, Attribute::GoboWheel { slot: 0 }),
            ],
        });
    }
    if m == "rockville" && mo.contains("rockstrip") {
        if mo.contains("7ch") {
            // Confirmed by name. Layout estimated: Dimmer, R, G, B, White,
            // Strobe, Program — same convention as the Uking par.
            let mut cm = rgb_par(7, Some(0), 1);
            cm.channels.push((4, Attribute::ColorAdd { channel: ColorChannel::White }));
            cm.channels.push((5, Attribute::Strobe));
            return Some(cm);
        }
        if mo.contains("3ch") {
            // Confirmed by name: bare RGB, no separate dimmer channel.
            return Some(rgb_par(3, None, 0));
        }
    }
    if m == "chauvet" && mo.contains("hurricane") {
        // Not confirmed by address spacing (only 2 units, on different
        // universes) — estimated from Chauvet's documented Hurricane Haze
        // 1DX 2ch mode: Haze output, Fan speed. `Dimmer` stands in for the
        // haze-output channel — there's no dedicated haze `Attribute` yet.
        return Some(ChannelMap { footprint: 2, channels: vec![(0, Attribute::Dimmer)] });
    }
    None
}
