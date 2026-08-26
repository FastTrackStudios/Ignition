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

use crate::fixture_profile::{ColorWheelSlot, FixtureEmitters, FixtureProfile};
use ignition_proto::{Attribute, ChannelMap, ColorChannel};

/// The whole output-side profile of a hand-authored fixture type: its
/// channel map, emitters, colour wheel and rest defaults. `None` for a
/// type with no channel map at all.
// r[impl color.mix-or-wheel] - wheel tables for the wheel movers
// r[impl playback.defaults] - per-model rest values for the hand-authored maps
pub fn profile_for(manufacturer: &str, model: &str) -> Option<FixtureProfile> {
    let map = channel_map_for(manufacturer, model)?;
    Some(FixtureProfile::from_channel_map(map).with_wheel(wheel_slots_for(manufacturer, model)))
}

/// Approximate CIE xy of the gels a budget mover's colour wheel carries.
/// Nobody has measured these wheels; the values are the textbook
/// chromaticities of the named colours, good enough to pick the right
/// slot for a preset and nothing more.
mod wheel_xy {
    pub const WHITE: (f32, f32) = (0.313, 0.329);
    pub const RED: (f32, f32) = (0.680, 0.310);
    pub const GREEN: (f32, f32) = (0.210, 0.710);
    pub const BLUE: (f32, f32) = (0.150, 0.060);
    pub const YELLOW: (f32, f32) = (0.450, 0.480);
    pub const MAGENTA: (f32, f32) = (0.350, 0.170);
    pub const CYAN: (f32, f32) = (0.200, 0.320);
    pub const ORANGE: (f32, f32) = (0.580, 0.400);
    pub const PINK: (f32, f32) = (0.400, 0.250);
}

fn wheel(slots: &[(&str, u8, (f32, f32))]) -> Vec<ColorWheelSlot> {
    slots
        .iter()
        .map(|(name, byte, (x, y))| ColorWheelSlot::xy(name, *byte, *x, *y))
        .collect()
}

/// The colour-wheel slot table of a model that has a wheel; empty for
/// every mixing fixture. Bytes are the centre of each slot's range on
/// the wheel channel, per the fixture class's usual manual: the Riukoe
/// / Lixada mini gobo heads step eight colours in ranges of 8, the
/// Betopper beam nine colours in ranges of 10 (above those ranges both
/// wheels carry split colours and then continuous rotation, which are
/// not slots and are left out).
// r[impl color.mix-or-wheel] - the Riukoe and Betopper wheel colours
pub fn wheel_slots_for(manufacturer: &str, model: &str) -> Vec<ColorWheelSlot> {
    use wheel_xy::*;
    let m = manufacturer.to_ascii_lowercase();
    let mo = model.to_ascii_lowercase();
    if ((m == "riukoe" || m == "lixada") && mo.contains("gobo"))
        || mo.contains("mini gobo moving head light")
    {
        return wheel(&[
            ("White", 3, WHITE),
            ("Red", 11, RED),
            ("Green", 19, GREEN),
            ("Blue", 27, BLUE),
            ("Yellow", 35, YELLOW),
            ("Magenta", 43, MAGENTA),
            ("Cyan", 51, CYAN),
            ("Orange", 59, ORANGE),
        ]);
    }
    if mo.contains("150w moving head beam") {
        // U'King ZQ02341 manual: 0-9 white, then seven colours in
        // 10-value steps (10-79). The manual does not name the colours;
        // the order is the family's usual one and is a guess until a
        // unit is on the bench.
        return wheel(&[
            ("White", 5, WHITE),
            ("Red", 15, RED),
            ("Green", 25, GREEN),
            ("Blue", 35, BLUE),
            ("Yellow", 45, YELLOW),
            ("Magenta", 55, MAGENTA),
            ("Cyan", 65, CYAN),
            ("Orange", 75, ORANGE),
        ]);
    }
    if m == "betopper" {
        return wheel(&[
            ("White", 4, WHITE),
            ("Red", 14, RED),
            ("Green", 24, GREEN),
            ("Blue", 34, BLUE),
            ("Yellow", 44, YELLOW),
            ("Orange", 54, ORANGE),
            ("Magenta", 64, MAGENTA),
            ("Pink", 74, PINK),
            ("Cyan", 84, CYAN),
        ]);
    }
    Vec::new()
}

/// The colour emitters of a fixture type, derived from its channel map
/// with `fixture_profile::typical_emitter`'s class-of-LED chromaticities:
/// the RGBW pars and the Endyshow bar mix with a white, the RGB pars and
/// strips with their three primaries, and a colour-wheel mover (Riukoe,
/// Betopper) has none — its wheel mapping is untouched.
// r[impl color.emitter-solve] - per-model emitter data for the hand-authored maps
pub fn emitters_for(manufacturer: &str, model: &str) -> Option<FixtureEmitters> {
    FixtureEmitters::from_channel_map(&channel_map_for(manufacturer, model)?)
}

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
        curves: Default::default(),
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

    // Confirmed layouts first. Riverside's are read from its own desk, so
    // they outrank every "typical for this class of fixture" guess below.
    if let Some(cm) = riverside_channel_map(model) {
        return Some(cm);
    }

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
        // Dimmer, Speed, Special, Reset. The fine bytes are mapped so
        // pan and tilt travel at 16 bits — see `Attribute::PanFine`.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 11,
            channels: vec![
                (0, Attribute::Pan),
                (1, Attribute::PanFine),
                (2, Attribute::Tilt),
                (3, Attribute::TiltFine),
                (4, Attribute::ColorWheel { slot: 0 }),
                (5, Attribute::GoboWheel { slot: 0 }),
                (6, Attribute::Strobe),
                (7, Attribute::Dimmer),
            ],
        });
    }
    if mo.contains("150w moving head beam") {
        // U'King ZQ02341 (Riverside's pole-top beams), per its manual:
        // Pan, Pan fine, Tilt, Tilt fine, Speed, Dimmer, Strobe, Colour,
        // Gobo, Prism, Macro, Reset. Same layout as the Betopper below
        // (same OEM chassis); ch11 is an auto/sound macro, not focus.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 12,
            channels: vec![
                (0, Attribute::Pan),
                (1, Attribute::PanFine),
                (2, Attribute::Tilt),
                (3, Attribute::TiltFine),
                (5, Attribute::Dimmer),
                (6, Attribute::Strobe),
                (7, Attribute::ColorWheel { slot: 0 }),
                (8, Attribute::GoboWheel { slot: 0 }),
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
            curves: Default::default(),
            footprint: 12,
            channels: vec![
                (0, Attribute::Pan),
                (1, Attribute::PanFine),
                (2, Attribute::Tilt),
                (3, Attribute::TiltFine),
                (5, Attribute::Dimmer),
                (6, Attribute::Strobe),
                (7, Attribute::ColorWheel { slot: 0 }),
                (8, Attribute::GoboWheel { slot: 0 }),
            ],
        });
    }
    if m == "rockville" && mo.contains("rockstrip") {
        if mo.contains("7ch") {
            // CONFIRMED, and the suspicion recorded here was justified: the
            // estimate really was wrong. This entry used to read Dimmer, R,
            // G, B, White, Strobe, Program — the "generic budget-par 7ch"
            // convention — and flagged itself as suspect because that same
            // assumption had already proved wrong for the Uking par.
            //
            // Riverside's console carries this exact fixture in its own
            // library, and the desk's profile says the real 7ch layout is
            // R, G, B, Dimmer, Shutter, Chase Mode, Control: colour comes
            // FIRST, the dimmer is fourth, and there is no White channel at
            // all. See `data/venues/riverside/console-show.json`, and
            // `riverside_channel_map` below, which serves the same fixture
            // when it is patched under its bare model name.
            //
            // The old layout put colour one address high and drove Strobe
            // with the blue value — every Rockstrip in the rig lit wrong.
            let mut cm = rgb_par(7, None, 0);
            cm.channels.push((3, Attribute::Dimmer));
            cm.channels.push((4, Attribute::Strobe));
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
            curves: Default::default(),
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

    /// r[verify color.emitter-solve] - RGBW pars carry a white emitter; wheel movers none
    #[test]
    fn emitters_match_each_model_class() {
        let bar = emitters_for("Endyshow", "Endyshow LED Stage Light Bar PL-32M").unwrap();
        assert_eq!(bar.channels.len(), 4);
        assert!(bar.channels.iter().any(|(c, _)| *c == ColorChannel::White));
        let par = emitters_for("Chauvet", "SlimPAR 7ch").unwrap();
        assert_eq!(par.channels.len(), 3);
        assert!(emitters_for("Riukoe", "Gobo 11ch").is_none());
    }

    /// r[verify color.mix-or-wheel] - the wheel movers carry a slot table, the pars none
    #[test]
    fn wheel_movers_carry_a_slot_table_and_prefer_it() {
        use ignition_core::color::{ColorPreference, Intent, nearest_wheel_slot};
        let riukoe = profile_for("Riukoe", "Gobo 11ch").unwrap();
        assert_eq!(riukoe.wheel.len(), 8);
        assert_eq!(riukoe.color_preference, ColorPreference::Wheel);
        assert!(riukoe.emitters.is_none());
        let betopper = profile_for("Betopper", "150W Beam").unwrap();
        assert_eq!(betopper.wheel.len(), 9);
        let blue = Intent::Rgb(ignition_core::color::Rgb::new(0.0, 0.0, 1.0));
        assert_eq!(nearest_wheel_slot(&blue, &betopper.wheel_slots()), Some(34));
        let par = profile_for("Uking", "Par").unwrap();
        assert!(par.wheel.is_empty());
        assert_eq!(par.color_preference, ColorPreference::Mix);
        assert_eq!(par.defaults[&Attribute::Dimmer], 0.0);
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

/// Riverside's fixture types — every one CONFIRMED, and by the strongest
/// source there is: the venue's own console.
///
/// These are not estimates from "the general class of fixture". They were
/// read straight out of the desk's showfile, where each type carries its
/// own `SSLLIBRARY`/`SSLMODE`/`SSLCHANNEL` profile inline — the exact
/// layout the desk puts on the wire. The extract is committed at
/// `data/venues/riverside/console-show.json`, generated read-only by
/// `tools/dvc_parse.py`, so every line here is checkable against a file
/// rather than against a memory of one.
///
/// Channel *order* is the showfile's document order. `SSLCHANNELTYPEINDEX`
/// looks like an offset and is not — it disambiguates several channels of
/// the same type, and reading it as one silently scrambles the map. The
/// fine half of a 16-bit pair (`SSLCHANNELLSB`) is skipped, matching how
/// `dmx.rs` reads 8-bit coarse values today; it still occupies its address
/// so the next fixture stays correctly patched.
///
/// Matched on model alone: the showfile records a library path
/// (`_varied/Solena Professional Max Par 54 RGB.ssl2`), not a separate
/// manufacturer field, so that is what the venue's patch carries.
fn riverside_channel_map(model: &str) -> Option<ChannelMap> {
    let mo = model.to_ascii_lowercase();
    if mo.contains("36 led par can") {
        // 7ch: Dimmer, Red, Green, Blue, Shutter, Color Macros, Color Macors Speed
        // 2 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 7,
            channels: vec![
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
                (4, Attribute::Strobe),
            ],
        });
    }
    if mo.contains("base hazer 1500w") {
        // 2ch: Pump, Fan
        // 1 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 2,
            channels: vec![(0, Attribute::Dimmer)],
        });
    }
    if mo.contains("endyshow led stage light bar pl-32m") {
        // 8ch: Total Dimmer, Red, Green, Blue, White, Total Strobe, Function Choice, Function Speed
        // 2 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 8,
            channels: vec![
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
                (
                    4,
                    Attribute::ColorAdd {
                        channel: ColorChannel::White,
                    },
                ),
                (5, Attribute::Strobe),
            ],
        });
    }
    if mo.contains("led mini beam wh") {
        // 8ch: X, ÂµX, Y, ÂµY, Speed, Shutter, Dimmer, Special
        // 4 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 8,
            channels: vec![
                (0, Attribute::Pan),
                (2, Attribute::Tilt),
                (5, Attribute::Strobe),
                (6, Attribute::Dimmer),
            ],
        });
    }
    if mo.contains("mbdmx-plus") {
        // 2ch: Lamp, Rotation
        // 1 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 2,
            channels: vec![(0, Attribute::Dimmer)],
        });
    }
    if mo.contains("mini derby") {
        // 9ch: MODE, RED, GREEN, BLUE, LED COLOR CONTROL, LED STROBE, LED FADE, MOTOR SPEED, MOTOR SPIN SPEED
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 9,
            channels: vec![
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
                (4, Attribute::Dimmer),
                (5, Attribute::Strobe),
                (6, Attribute::Dimmer),
                (7, Attribute::Dimmer),
                (8, Attribute::Dimmer),
            ],
        });
    }
    if mo.contains("mini gobo moving head light") {
        // 11ch: X, ÂµX, Y, ÂµY, Color Wheel, Gobo, Shutter, Dimmer, Speed, Function, Dimmer Modes
        // 5 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
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
    if mo.contains("rgbw spot light 6ch") {
        // 6ch: Dimmer/Strobe/Effect, Red, Green, Blue, White, Macro
        // 2 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 6,
            channels: vec![
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
                (
                    4,
                    Attribute::ColorAdd {
                        channel: ColorChannel::White,
                    },
                ),
            ],
        });
    }
    // The bare model name and its 7ch mode; the 3ch mode is a
    // different personality (bare RGB, `channel_map_for`), and Norco
    // patches two of those three addresses apart. Serving the 7ch map
    // there put ch 97's red byte on ch 96's "dimmer" — caught by the
    // DMX loopback test, which is what a byte-driven viz is for.
    // r[impl viz.driven-by-dmx] - a patch mistake shows on the bytes
    if mo.contains("rockstrip 252") && !mo.contains("3ch") {
        // 7ch: Red, Green, Blue, Dimmer, Shutter, Chase Mode, Control
        // 2 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 7,
            channels: vec![
                (
                    0,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Red,
                    },
                ),
                (
                    1,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Green,
                    },
                ),
                (
                    2,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Blue,
                    },
                ),
                (3, Attribute::Dimmer),
                (4, Attribute::Strobe),
            ],
        });
    }
    if mo.contains("solena max bar 28 rgb") {
        // 6ch: Red, Green, Blue, Macros, Shutter, Selection Control
        // 2 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 6,
            channels: vec![
                (
                    0,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Red,
                    },
                ),
                (
                    1,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Green,
                    },
                ),
                (
                    2,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Blue,
                    },
                ),
                (4, Attribute::Strobe),
            ],
        });
    }
    if mo.contains("solena professional max par 54 rgb") {
        // 7ch: Dimmer, Red, Green, Blue, Shutter, Color Macros, Color Selection/Shade
        // 2 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 7,
            channels: vec![
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
                (4, Attribute::Strobe),
            ],
        });
    }
    if mo.contains("zq01334") {
        // 15ch: Dimmer, Strobe, 1. Warm White, 2. Warm White, 3. Warm White, 4. Warm White, 5. Warm White, 6. Warm White, Red, Green, Blue, Warm White Macro, Warm White Macro Speed, Auxiliary Light Effect, Auxiliary Light Effect Speed
        // 10 channel(s) have no modelled Attribute yet.
        return Some(ChannelMap {
            curves: Default::default(),
            footprint: 15,
            channels: vec![
                (0, Attribute::Dimmer),
                (1, Attribute::Strobe),
                (
                    8,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Red,
                    },
                ),
                (
                    9,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Green,
                    },
                ),
                (
                    10,
                    Attribute::ColorAdd {
                        channel: ColorChannel::Blue,
                    },
                ),
            ],
        });
    }
    None
}

#[cfg(test)]
mod uking_beam_tests {
    use super::*;

    /// /// Riverside's U'King beams have a map
    #[test]
    fn riverside_uking_beams_have_a_channel_map_and_wheel() {
        let map = channel_map_for("U'King", "150W Moving Head Beam").expect("map");
        assert_eq!(map.footprint, 12);
        assert!(
            map.channels
                .iter()
                .any(|(i, a)| *i == 7 && matches!(a, Attribute::ColorWheel { .. }))
        );
        assert_eq!(wheel_slots_for("U'King", "150W Moving Head Beam").len(), 8);
    }
}
