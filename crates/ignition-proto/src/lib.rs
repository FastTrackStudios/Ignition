//! Wire contract for Ignition. Plain data only — no RPC traits yet.
//!
//! Service traits will follow architect's `#[architect::rpc]` idiom once
//! Ignition takes architect as a pinned git dependency (see
//! `docs/domain/DOMAIN.md`), the same pattern FastTrackStudio's `signal` and
//! `session` facades use.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    /// The low byte of a 16-bit pan, on fixtures that have one. Never
    /// programmed directly: a cue writes `Pan` in degrees and the patch
    /// splits it across both bytes at output time — an 8-bit pan on a
    /// 540° yoke steps 2.1° at a time, which is a visible lurch.
    PanFine,
    /// The low byte of a 16-bit tilt — see `PanFine`.
    TiltFine,
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

/// How an attribute's normalised value (`0..=1`) becomes the wire byte —
/// a venue's fixture type may carry one per attribute, so a dimmer that
/// is not linear, or a channel whose physical range is a subset of the
/// byte range, is corrected at output and HTP between two different types
/// compares like with like. `Linear` is the default everywhere.
// r[impl files.venue.dmx-curves] - the lookup from attribute value to wire value
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Curve {
    /// `byte = round(value * 255)`.
    #[default]
    Linear,
    /// A lookup table sampled evenly over `0..=1`, interpolated between
    /// entries. An empty table is linear; a one-entry table is a constant.
    Lut(Vec<u8>),
    /// The value spans a sub-range of the byte: `lo` at 0, `hi` at 1.
    /// `lo > hi` inverts the channel.
    Range { lo: u8, hi: u8 },
}

impl Curve {
    /// The wire byte for a clamped normalised value.
    pub fn apply(&self, value: f32) -> u8 {
        let v = if value.is_nan() {
            0.0
        } else {
            value.clamp(0.0, 1.0)
        };
        match self {
            Curve::Linear => (v * 255.0).round() as u8,
            Curve::Lut(table) => match table.len() {
                0 => (v * 255.0).round() as u8,
                1 => table[0],
                n => {
                    let pos = v * (n - 1) as f32;
                    let i = (pos.floor() as usize).min(n - 2);
                    let t = pos - i as f32;
                    let a = table[i] as f32;
                    let b = table[i + 1] as f32;
                    (a + (b - a) * t).round().clamp(0.0, 255.0) as u8
                }
            },
            Curve::Range { lo, hi } => {
                let lo = *lo as f32;
                let hi = *hi as f32;
                (lo + (hi - lo) * v).round().clamp(0.0, 255.0) as u8
            }
        }
    }
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
    /// Per-attribute output curves; an attribute with no entry is
    /// `Curve::Linear`. Pan/tilt are encoded in degrees and take no curve.
    // r[impl files.venue.dmx-curves]
    #[serde(
        default,
        skip_serializing_if = "HashMap::is_empty",
        with = "curve_pairs"
    )]
    pub curves: HashMap<Attribute, Curve>,
}

/// `curves` on the wire as a list of `[attribute, curve]` pairs — JSON
/// map keys have to be strings, and a `ColorAdd { channel }` is not one.
mod curve_pairs {
    use super::{Attribute, Curve};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    pub fn serialize<S: Serializer>(
        curves: &HashMap<Attribute, Curve>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut pairs: Vec<(&Attribute, &Curve)> = curves.iter().collect();
        pairs.sort_by_key(|(a, _)| format!("{a:?}"));
        pairs.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<HashMap<Attribute, Curve>, D::Error> {
        let pairs: Vec<(Attribute, Curve)> = Vec::deserialize(deserializer)?;
        Ok(pairs.into_iter().collect())
    }
}

impl ChannelMap {
    /// A map with every curve linear.
    pub fn new(footprint: u16, channels: Vec<(u16, Attribute)>) -> Self {
        Self {
            footprint,
            channels,
            curves: HashMap::new(),
        }
    }

    /// The output curve for `attr` — linear unless the map says otherwise.
    pub fn curve_of(&self, attr: &Attribute) -> &Curve {
        static LINEAR: Curve = Curve::Linear;
        self.curves.get(attr).unwrap_or(&LINEAR)
    }

    /// The 0-based offset of `attr` within this fixture's footprint, if this
    /// personality has that attribute at all.
    pub fn offset_of(&self, attr: &Attribute) -> Option<u16> {
        self.channels
            .iter()
            .find(|(_, a)| a == attr)
            .map(|(o, _)| *o)
    }
}

/// The DMX transmitter's state, flattened for display.
///
/// Lives here rather than in the visualizer so the studio's `Playhead`
/// — which carries it back to every surface, including a browser on an
/// iPad — is a plain serialisable record with no Bevy behind it. The
/// visualizer fills it from `ignition_io::Status`; the overlay and the
/// studio's OUTPUT key read it without a socket.
// r[impl dmx.output-toggle] - the transmit state as a wire record
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputSummary {
    pub enabled: bool,
    /// Protocol names in use — `sACN`, `Art-Net` — in that order.
    pub protocols: Vec<String>,
    /// How many universes are configured.
    pub universes: usize,
    /// Frames per second per universe, as the sender measures it.
    pub hz: f32,
    /// The first error the sender or the bind reported.
    pub error: Option<String>,
    /// One line per universe, for the surface's detail.
    pub lines: Vec<String>,
}

impl OutputSummary {
    /// The overlay's line: `OUT sACN ×4 44Hz`, `OUT off`, or the error.
    // r[impl dmx.output-toggle] - the state, on the picture
    pub fn line(&self) -> String {
        if let Some(e) = &self.error {
            return format!("OUT ERROR {e}");
        }
        if !self.enabled {
            return "OUT off".to_string();
        }
        if self.universes == 0 {
            return "OUT on (no universes)".to_string();
        }
        let protocols = if self.protocols.is_empty() {
            "none".to_string()
        } else {
            self.protocols.join("+")
        };
        format!("OUT {protocols} ×{} {:.0}Hz", self.universes, self.hz)
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

    /// r[verify files.venue.dmx-curves] - each curve shape maps 0..=1 to the wire as documented
    #[test]
    fn curves_map_a_normalised_value_to_the_wire() {
        assert_eq!(Curve::Linear.apply(0.5), 128);
        assert_eq!(Curve::Linear.apply(2.0), 255);
        assert_eq!(Curve::Linear.apply(f32::NAN), 0);
        let lut = Curve::Lut(vec![0, 10, 255]);
        assert_eq!(lut.apply(0.0), 0);
        assert_eq!(lut.apply(0.5), 10);
        assert_eq!(lut.apply(0.75), 133);
        assert_eq!(lut.apply(1.0), 255);
        assert_eq!(Curve::Lut(vec![]).apply(1.0), 255);
        assert_eq!(Curve::Lut(vec![7]).apply(0.3), 7);
        let range = Curve::Range { lo: 10, hi: 200 };
        assert_eq!(range.apply(0.0), 10);
        assert_eq!(range.apply(1.0), 200);
        assert_eq!(range.apply(0.5), 105);
        assert_eq!(Curve::Range { lo: 255, hi: 0 }.apply(0.25), 191);
    }

    /// r[verify files.venue.dmx-curves] - a map without curves reads as before
    #[test]
    fn a_channel_map_without_curves_is_linear_and_still_parses() {
        let map: ChannelMap =
            serde_json::from_str(r#"{"footprint":1,"channels":[[0,"Dimmer"]]}"#).unwrap();
        assert_eq!(map.curve_of(&Attribute::Dimmer), &Curve::Linear);
        let map: ChannelMap = serde_json::from_str(
            r#"{"footprint":1,"channels":[[0,"Dimmer"]],"curves":[["Dimmer",{"range":{"lo":5,"hi":250}}]]}"#,
        )
        .unwrap();
        assert_eq!(map.curve_of(&Attribute::Dimmer).apply(0.0), 5);
        let json = serde_json::to_string(&map).unwrap();
        let back: ChannelMap = serde_json::from_str(&json).unwrap();
        assert_eq!(back, map);
    }
}
