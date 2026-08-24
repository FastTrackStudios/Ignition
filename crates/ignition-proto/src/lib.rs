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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Attribute {
    Dimmer,
    Pan,
    Tilt,
    ColorAdd { channel: ColorChannel },
    ColorWheel { slot: u16 },
    GoboWheel { slot: u16 },
    Zoom,
    Focus,
    Iris,
    Strobe,
    /// Escape hatch for a fixture-specific attribute not yet modelled —
    /// GDTF profiles are large and this crate does not aim to model all of
    /// them up front.
    Custom(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
                position: Vec3 { x: -4.48, y: -7.78, z: 3.25 },
                orientation: Quat { w: 0.842, x: 0.537, y: -0.028, z: -0.044 },
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        let back: PatchEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(entry, back);
    }
}
