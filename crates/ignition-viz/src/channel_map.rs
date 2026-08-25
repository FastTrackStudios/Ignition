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
//! `global_address` data). *Per-channel function order* is a mix: two
//! entries (Uking Par, Chauvet Hurricane Haze) are now CONFIRMED against
//! the fixture's own real published profile in the Open Fixture Library
//! (openlighting.org/ofl — plain JSON, no zip/XML pipeline needed, unlike
//! GDTF; see `docs/research/lighting-console-landscape.md`'s Slice 8 for
//! why OFL turned out to be the more directly usable channel-map source
//! for this project's remaining unconfirmed fixtures than GDTF). The rest
//! are still estimated from the general class of fixture (RGB(W) par,
//! cheap moving head, LED batten) since neither OFL nor GDTF has a profile
//! for those specific budget/no-name models (checked — see below). Treat
//! unconfirmed entries as a working default, not ground truth.

use ignition_proto::{Attribute, ChannelMap, ColorChannel};

fn rgb_par(footprint: u16, dimmer_channel: Option<u16>, rgb_start: u16) -> ChannelMap {
    let mut channels = Vec::new();
    if let Some(d) = dimmer_channel {
        channels.push((d, Attribute::Dimmer));
    }
    channels.push((
        rgb_start,
        Attribute::ColorAdd {
            channel: ColorChannel::Red,
        },
    ));
    channels.push((
        rgb_start + 1,
        Attribute::ColorAdd {
            channel: ColorChannel::Green,
        },
    ));
    channels.push((
        rgb_start + 2,
        Attribute::ColorAdd {
            channel: ColorChannel::Blue,
        },
    ));
    ChannelMap {
        footprint,
        channels,
    }
}

/// `manufacturer`/`model` match the same way `fixture_profile::shape_for`
/// does — verbatim strings from the live Eos patch, matched case-insensitive
/// substring.
pub fn channel_map_for(manufacturer: &str, model: &str) -> Option<ChannelMap> {
    let m = manufacturer.to_ascii_lowercase();
    let mo = model.to_ascii_lowercase();

    if m == "uking" && mo.contains("par") {
        // CONFIRMED against the real fixture's own published profile:
        // github.com/OpenLightingProject/open-fixture-library
        // fixtures/uking/par-light-b262.json (180x180x100mm — the exact
        // dimensions fixture_profile.rs's shape_for() already used for
        // this fixture, independently confirming this is the right model
        // match). Its real "7-channel" mode: Master Dimmer, Red, Green,
        // Blue, Strobe, Mode, Hue Selection/Speed — no White channel at
        // all (the earlier estimated layout guessed one at offset 4 and
        // put Strobe at 5; the real fixture has Strobe at 4). Mode/Hue
        // Selection have no modelled Attribute yet, so this fixture's
        // real footprint is 7 but only 4 of its channels resolve to
        // anything the visualizer reads.
        let mut cm = rgb_par(7, Some(0), 1);
        cm.channels.push((4, Attribute::Strobe));
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
            // Strobe, Program — same "generic budget-par 7ch" convention
            // originally used for the Uking par below, which turned out to
            // be wrong when checked against that fixture's real published
            // profile (no White channel at all — see the Uking Par entry
            // above). Rockville has no fixture in the Open Fixture Library
            // to check this one against (only "rockpar50" exists there,
            // not this "Rockstrip 252" model), so this guess is now flagged
            // as suspect rather than treated as a safe default — the same
            // wrong assumption may well be baked in here too.
            let mut cm = rgb_par(7, Some(0), 1);
            cm.channels.push((
                4,
                Attribute::ColorAdd {
                    channel: ColorChannel::White,
                },
            ));
            cm.channels.push((5, Attribute::Strobe));
            return Some(cm);
        }
        if mo.contains("3ch") {
            // Confirmed by name: bare RGB, no separate dimmer channel.
            return Some(rgb_par(3, None, 0));
        }
    }
    if m == "chauvet" && mo.contains("hurricane") {
        // CONFIRMED against the real fixture's own published profile:
        // open-fixture-library fixtures/chauvet-dj/hurricane-haze-1dx.json
        // — the real fixture has exactly one mode, "1 Channel": a single
        // Haze (Fog) channel, no separate fan-speed channel at all. The
        // earlier estimate guessed a 2ch dimmer+fan mode that doesn't
        // exist for this fixture. `Dimmer` stands in for the haze-output
        // channel — there's no dedicated haze `Attribute` yet.
        return Some(ChannelMap {
            footprint: 1,
            channels: vec![(0, Attribute::Dimmer)],
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locks in the correction from the Open Fixture Library's real
    /// uking/par-light-b262.json profile: no White channel, Strobe at
    /// offset 4 (not the originally-guessed offset 5).
    #[test]
    fn uking_par_matches_the_real_ofl_profile() {
        let cm = channel_map_for("Uking", "Par").expect("Uking Par has a channel map");
        assert_eq!(cm.footprint, 7);
        assert_eq!(cm.offset_of(&Attribute::Dimmer), Some(0));
        assert_eq!(
            cm.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::Red
            }),
            Some(1)
        );
        assert_eq!(
            cm.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::Green
            }),
            Some(2)
        );
        assert_eq!(
            cm.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::Blue
            }),
            Some(3)
        );
        assert_eq!(cm.offset_of(&Attribute::Strobe), Some(4));
        assert_eq!(
            cm.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::White
            }),
            None
        );
    }

    /// Locks in the correction from the real chauvet-dj/hurricane-haze-1dx.json
    /// profile: the fixture's only mode is 1 channel, not the originally-
    /// guessed 2ch (dimmer + fan).
    #[test]
    fn hurricane_haze_matches_the_real_ofl_profile() {
        let cm = channel_map_for("Chauvet", "Hurricane Haze 1DX")
            .expect("Hurricane Haze has a channel map");
        assert_eq!(cm.footprint, 1);
        assert_eq!(cm.offset_of(&Attribute::Dimmer), Some(0));
    }
}
