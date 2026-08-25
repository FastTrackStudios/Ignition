//! Wire contract for Ignition. Plain data only — no RPC traits yet.
//!
//! Service traits will follow architect's `#[architect::rpc]` idiom once
//! Ignition takes architect as a pinned git dependency (see
//! `docs/domain/DOMAIN.md`), the same pattern FastTrackStudio's `signal` and
//! `session` facades use.

use serde::{Deserialize, Serialize};

/// A patched fixture's channel number, 1-based, matching Eos/QLC+ convention.
pub type ChanId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Quat {
    pub w: f64,
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

/// A fixture's placement in the venue. `position` and `orientation` are the
/// *hang* (how it is rigged), never the aim — see
/// `docs/domain/norco-venue-reference.md` for why conflating the two is a
/// bug: it draws the fixture bolted on at a false angle and offsets every
/// live pan/tilt reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub position: Vec3,
    pub orientation: Quat,
}

/// GDTF-style attribute identity, not a raw DMX channel offset. The patch
/// resolves an attribute to bytes at output time; everything upstream
/// (presets, effects, the visualizer) programs against this instead.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Attribute {
    Dimmer,
    Pan,
    Tilt,
    ColorAdd {
        channel: ColorChannel,
    },
    ColorWheel {
        slot: u16,
    },
    GoboWheel {
        slot: u16,
    },
    Zoom,
    Focus,
    Iris,
    Strobe,
    /// Escape hatch for a fixture-specific attribute not yet modelled —
    /// GDTF profiles are large and this crate does not aim to model all of
    /// them up front.
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ColorChannel {
    Red,
    Green,
    Blue,
    White,
    Amber,
    Uv,
    Lime,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchEntry {
    pub chan: ChanId,
    pub fixture_type: String,
    pub placement: Placement,
    /// Where this fixture actually lives on the wire — absent for a fixture
    /// that isn't DMX-controlled at all (a static prop/architectural
    /// object never has this). `None` means "render at its fixed
    /// placement/colour, there is no live signal to read."
    #[serde(default)]
    pub dmx: Option<DmxAddress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmxAddress {
    pub universe: u16,
    /// 1-based start channel within the universe (1..=512), matching Eos/QLC+
    /// convention — the fixture's footprint runs from here through
    /// `start + footprint_len - 1` per its `ChannelMap`.
    pub start_channel: u16,
}

/// One fixture *personality*'s channel layout: which byte offset (0-based,
/// relative to `DmxAddress::start_channel`) resolves to which `Attribute`.
/// This is deliberately the same shape QLC+'s `.qxf` Channel list and GDTF's
/// DMXChannel list both use — a fixture-type's real channel count and
/// function order, not hardcoded per-instance. See
/// `docs/domain/dmx-channel-maps.md` for where each map in this project came
/// from (confirmed via DMX-address spacing in the live patch vs. estimated
/// from typical fixtures of that class) and how confident it is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChannelMap {
    /// Total DMX footprint (how many consecutive channels this fixture's
    /// mode occupies) — must match the real spacing between this fixture
    /// and the next one patched after it, or two fixtures' live values will
    /// bleed into each other.
    pub footprint: u16,
    pub channels: Vec<(u16, Attribute)>,
}

impl ChannelMap {
    /// The 0-based offset of `attr` within this fixture's footprint, if this
    /// personality has that attribute at all.
    pub fn offset_of(&self, attr: &Attribute) -> Option<u16> {
        self.channels
            .iter()
            .find(|(_, a)| a == attr)
            .map(|(o, _)| *o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_entry_round_trips_through_json() {
        let entry = PatchEntry {
            chan: 4,
            fixture_type: "generic/led-wash".into(),
            placement: Placement {
                position: Vec3 {
                    x: -4.48,
                    y: -7.78,
                    z: 3.25,
                },
                orientation: Quat {
                    w: 0.842,
                    x: 0.537,
                    y: -0.028,
                    z: -0.044,
                },
            },
            dmx: Some(DmxAddress {
                universe: 1,
                start_channel: 1,
            }),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: PatchEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }
}
