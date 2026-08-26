//! Open Fixture Library (OFL, openlighting.org/ofl) channel-map import —
//! Slice 8 of the DMX work (`docs/research/lighting-console-landscape.md`).
//! Sibling to `gdtf_import.rs`, same purpose (pull a real `ChannelMap` out
//! of a fixture's own published profile instead of guessing the channel
//! function order), different source format. Found by actually cloning
//! ASLS Studio (`github.com/ASLS-org/studio`) and reading its fixture
//! library rather than inventing an "ASLS format": ASLS doesn't have its
//! own fixture format at all — it fetches raw OFL JSON verbatim
//! (`fixtureData.OFLData` in its `show.model.js`), unmodified. OFL turned
//! out to be the more directly usable channel-map source for this
//! project's remaining unconfirmed fixtures than GDTF: plain JSON via
//! `serde_json`, no zip/XML pipeline, and a much larger real-world
//! fixture database (github.com/OpenLightingProject/open-fixture-library)
//! — it's what actually resolved the Uking Par and Chauvet Hurricane Haze
//! corrections in `channel_map.rs`.
//!
//! OFL's schema is deliberately loose (a mode's channel list can hold a
//! channel name, `null` for an unused slot, or an object for templated/
//! matrix channels this project doesn't model) — parsed via `serde_json::
//! Value` rather than a strict typed schema to tolerate that, the same
//! spirit `gdtf_import.rs` uses for GDTF's `Attribute::Custom` fallback.

use ignition_proto::{Attribute, ChannelMap, ColorChannel};
use serde_json::Value;
use std::path::Path;

pub struct OflChannelMap {
    pub fixture_name: String,
    pub mode_name: String,
    pub channel_map: ChannelMap,
}

/// Reads an OFL fixture JSON file and extracts the `ChannelMap` for one
/// mode. `mode_name`, when `None`, picks the file's first mode.
pub fn import_channel_map(path: &Path, mode_name: Option<&str>) -> anyhow::Result<OflChannelMap> {
    let text = std::fs::read_to_string(path)?;
    let doc: Value = serde_json::from_str(&text)?;

    let fixture_name = doc["name"].as_str().unwrap_or("").to_string();
    let available_channels = doc["availableChannels"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("no availableChannels in fixture"))?;
    let modes = doc["modes"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("no modes in fixture"))?;

    let mode = match mode_name {
        Some(name) => modes
            .iter()
            .find(|m| m["name"].as_str() == Some(name))
            .ok_or_else(|| anyhow::anyhow!("fixture has no mode named {name:?}"))?,
        None => modes
            .first()
            .ok_or_else(|| anyhow::anyhow!("fixture defines no modes"))?,
    };
    let mode_channels = mode["channels"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("mode has no channel list"))?;
    let resolved_mode_name = mode["name"].as_str().unwrap_or("").to_string();

    let mut channels = Vec::new();
    for (offset, entry) in mode_channels.iter().enumerate() {
        // `null` = an unused DMX slot (still counts toward footprint); an
        // object = a templated/matrix channel reference (e.g. per-pixel
        // channels) — not modelled here, skipped like a GDTF virtual
        // channel with no offset.
        let Some(channel_name) = entry.as_str() else {
            continue;
        };
        let Some(channel_def) = available_channels.get(channel_name) else {
            continue;
        };
        if let Some(attr) = map_capability(channel_def) {
            channels.push((offset as u16, attr));
        }
    }

    let footprint = mode_channels.len() as u16;
    Ok(OflChannelMap {
        fixture_name,
        mode_name: resolved_mode_name,
        channel_map: ChannelMap {
            curves: Default::default(),
            footprint,
            channels,
        },
    })
}

/// OFL channels carry either a single `capability` object or a
/// `capabilities` array (multiple DMX sub-ranges within one byte, e.g. a
/// fog channel's off/weak/strong bands). This project's `Attribute` model
/// is single-valued per byte (no sub-range awareness yet), so only the
/// first capability's `type` (and `color`, for `ColorIntensity`) is used —
/// good enough to identify *which* attribute a channel is, which is all
/// `dmx.rs`'s 8-bit resolution needs.
fn map_capability(channel_def: &Value) -> Option<Attribute> {
    let cap = if let Some(c) = channel_def.get("capability") {
        c
    } else {
        channel_def.get("capabilities")?.as_array()?.first()?
    };
    let cap_type = cap["type"].as_str()?;
    match cap_type {
        "Intensity" => Some(Attribute::Dimmer),
        "Pan" => Some(Attribute::Pan),
        "Tilt" => Some(Attribute::Tilt),
        "ColorIntensity" => {
            let color = match cap["color"].as_str()? {
                "Red" => ColorChannel::Red,
                "Green" => ColorChannel::Green,
                "Blue" => ColorChannel::Blue,
                "White" | "Warm White" | "Cold White" => ColorChannel::White,
                "Amber" => ColorChannel::Amber,
                "UV" => ColorChannel::Uv,
                "Lime" => ColorChannel::Lime,
                other => return Some(Attribute::Custom(format!("ColorIntensity:{other}"))),
            };
            Some(Attribute::ColorAdd { channel: color })
        }
        "WheelSlot" | "ColorPreset" => Some(Attribute::ColorWheel { slot: 0 }),
        "Zoom" => Some(Attribute::Zoom),
        "Focus" => Some(Attribute::Focus),
        "Iris" | "IrisEffect" => Some(Attribute::Iris),
        "ShutterStrobe" => Some(Attribute::Strobe),
        "Fog" => Some(Attribute::Dimmer), // no dedicated haze Attribute yet — see channel_map.rs's hurricane entry.
        other => Some(Attribute::Custom(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real fixture files, not hand-authored test data — the exact ones
    /// that resolved the Uking Par / Chauvet Hurricane Haze corrections in
    /// channel_map.rs. Fetched once from
    /// github.com/OpenLightingProject/open-fixture-library (MIT-licensed,
    /// same as the rest of OFL) and vendored under
    /// `assets/ofl-samples/LICENSE-NOTICE.txt`.
    const UKING_PAR: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/ofl-samples/uking-par-light-b262.json"
    );
    const HURRICANE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/ofl-samples/chauvet-dj-hurricane-haze-1dx.json"
    );

    #[test]
    fn imports_the_real_uking_par_channel_layout() {
        let result = import_channel_map(Path::new(UKING_PAR), Some("7-channel")).unwrap();
        assert_eq!(result.channel_map.footprint, 7);
        assert_eq!(result.channel_map.offset_of(&Attribute::Dimmer), Some(0));
        assert_eq!(
            result.channel_map.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::Red
            }),
            Some(1)
        );
        assert_eq!(
            result.channel_map.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::Green
            }),
            Some(2)
        );
        assert_eq!(
            result.channel_map.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::Blue
            }),
            Some(3)
        );
        assert_eq!(result.channel_map.offset_of(&Attribute::Strobe), Some(4));
        // Confirms the bug this file's own corrections in channel_map.rs
        // fixed: no White channel exists on the real fixture.
        assert_eq!(
            result.channel_map.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::White
            }),
            None
        );
    }

    #[test]
    fn imports_the_real_hurricane_haze_channel_layout() {
        let result = import_channel_map(Path::new(HURRICANE), None).unwrap();
        assert_eq!(result.channel_map.footprint, 1);
        assert_eq!(result.channel_map.offset_of(&Attribute::Dimmer), Some(0));
    }

    #[test]
    fn unknown_mode_name_is_a_real_error_not_a_panic() {
        let result = import_channel_map(Path::new(UKING_PAR), Some("Nonexistent Mode"));
        assert!(result.is_err());
    }
}
