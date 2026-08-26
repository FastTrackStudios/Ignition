//! GDTF import — Slice 4 of the DMX work
//! (`docs/research/lighting-console-landscape.md`), the biggest-value,
//! least-mature-tooling piece. This pulls a `ChannelMap` (the same type
//! `channel_map.rs` hand-authors from address-spacing + guessed layouts)
//! straight out of a real manufacturer `.gdtf` file's DMX-mode channel
//! list, so a fixture with a real GDTF profile no longer needs its channel
//! *function order* estimated at all — only the footprint was ever
//! guaranteed correct before this (see `docs/domain/dmx-channel-maps.md`).
//!
//! What this does NOT do yet: import the fixture's real 3D geometry
//! (GDTF's Geometry tree — yoke/head as separate nodes — and the glTF/3DS
//! models each node references). That's a materially bigger job (a glTF
//! parser, a yoke/head-aware render path replacing `fixture_profile.rs`'s
//! single-mesh-plus-anchor model) and is the next slice, not this one.
//! `fixture_profile.rs`'s existing QLC+-mesh shapes are untouched — a
//! fixture imported via this module still renders with its existing
//! generic mesh, it just gets its live DMX behaviour from real data
//! instead of a guess.

use crate::fixture_profile::{FixtureEmitters, typical_emitter};
use gdtf::GdtfFile;
use ignition_core::color::{Emitter, emitter};
use ignition_proto::{Attribute, ChannelMap, ColorChannel};
use std::fs::File;
use std::path::Path;

/// One fixture type's real channel layout, pulled from a `.gdtf` file.
pub struct GdtfChannelMap {
    /// The fixture type's own name, as declared in the file — for
    /// matching against `fixtures.json`'s `manufacturer`/`model` strings
    /// at the call site (this module doesn't assume anything about how
    /// the venue data names its fixtures).
    pub fixture_type_name: String,
    pub dmx_mode_name: String,
    pub channel_map: ChannelMap,
    /// The mode's additive colour emitters, in `channel_map` order, with
    /// the file's own measured chromaticity where
    /// `PhysicalDescriptions/Emitters` carries one (linked from the
    /// channel's `ChannelFunction Emitter=`), and the class-typical value
    /// from `fixture_profile::typical_emitter` where it does not. `None`
    /// for a fixture with no `ColorAdd_*` channels at all.
    // r[impl color.emitter-solve] - real emitter data from GDTF when present
    pub emitters: Option<FixtureEmitters>,
}

/// Reads a `.gdtf` file and extracts the `ChannelMap` for one DMX mode.
/// `mode_name`, when `None`, picks the file's first DMX mode — most real
/// fixture GDTF files define exactly one, or a "Default"/simplest mode
/// first.
pub fn import_channel_map(path: &Path, mode_name: Option<&str>) -> anyhow::Result<GdtfChannelMap> {
    let file = File::open(path)?;
    let gdtf =
        GdtfFile::new(file).map_err(|e| anyhow::anyhow!("failed to parse GDTF file: {e}"))?;
    channel_map_from_description(&gdtf.description, mode_name)
}

/// `import_channel_map` on an already-parsed description — the file's
/// `description.xml`, which is also what a test can hand over as a
/// string without building a zip.
pub fn channel_map_from_description(
    description: &gdtf::Description,
    mode_name: Option<&str>,
) -> anyhow::Result<GdtfChannelMap> {
    let fixture_type = description
        .fixture_types
        .first()
        .ok_or_else(|| anyhow::anyhow!("GDTF file defines no fixture types"))?;

    let mode = match mode_name {
        Some(name) => fixture_type
            .dmx_modes
            .iter()
            .find(|m| m.name.as_deref().map(|n| n.to_string()).as_deref() == Some(name))
            .ok_or_else(|| anyhow::anyhow!("GDTF file has no DMX mode named {name:?}"))?,
        None => fixture_type
            .dmx_modes
            .first()
            .ok_or_else(|| anyhow::anyhow!("GDTF file's fixture type defines no DMX modes"))?,
    };

    let mut channels = Vec::new();
    let mut emitters = Vec::new();
    let mut footprint = 0u16;
    for ch in &mode.dmx_channels {
        // A GDTF channel with no `Offset` is a "virtual" channel (no real
        // DMX byte — e.g. a purely logical dimmer that another channel's
        // function actually implements). Nothing to map to a byte offset,
        // skip it.
        let Some(offsets) = &ch.offset else { continue };
        let Some(&coarse) = offsets.first() else {
            continue;
        };
        if coarse < 1 {
            continue; // malformed/non-standard — 1-based per the GDTF spec.
        }
        let offset0 = (coarse - 1) as u16;
        footprint = footprint.max(offset0 + 1);

        let Some(logical) = ch.logical_channels.first() else {
            continue;
        };
        let attr_name = logical.attribute.to_string();
        if let Some(attr) = map_attribute_name(&attr_name) {
            if let Attribute::ColorAdd { channel } = &attr {
                let measured = logical
                    .channel_functions
                    .iter()
                    .find_map(|f| f.emitter(fixture_type))
                    .and_then(gdtf_emitter);
                emitters.push((
                    *channel,
                    measured.unwrap_or_else(|| typical_emitter(*channel)),
                ));
            }
            channels.push((offset0, attr));
        }
    }
    let emitters = (!emitters.is_empty()).then_some(FixtureEmitters { channels: emitters });

    let fixture_type_name = fixture_type
        .name
        .as_deref()
        .unwrap_or(&fixture_type.short_name)
        .to_string();
    let dmx_mode_name = mode.name.as_deref().unwrap_or("").to_string();

    Ok(GdtfChannelMap {
        fixture_type_name,
        dmx_mode_name,
        channel_map: ChannelMap {
            footprint,
            channels,
        },
        emitters,
    })
}

/// Every emitter the file declares, whether or not a channel links to
/// it — for a caller that wants the fixture's palette of sources
/// without a DMX mode. Wavelength-only emitters (no `Color=`) have no
/// chromaticity to give and are skipped.
// r[impl color.emitter-solve] - GDTF PhysicalDescriptions/Emitters
pub fn import_emitters(path: &Path) -> anyhow::Result<Vec<Emitter>> {
    let file = File::open(path)?;
    let gdtf =
        GdtfFile::new(file).map_err(|e| anyhow::anyhow!("failed to parse GDTF file: {e}"))?;
    let fixture_type = gdtf
        .description
        .fixture_types
        .first()
        .ok_or_else(|| anyhow::anyhow!("GDTF file defines no fixture types"))?;
    Ok(fixture_type
        .physical_descriptions
        .emitters
        .iter()
        .filter_map(gdtf_emitter)
        .collect())
}

/// A GDTF `<Emitter Name= Color="x,y,Y">` as this project's emitter.
/// GDTF's `Y` is relative to the fixture's luminous flux, which is the
/// same "relative to full white" the solve wants.
fn gdtf_emitter(e: &gdtf::physical_descriptions::Emitter) -> Option<Emitter> {
    let gdtf::physical_descriptions::EmitterOptic::Color { color, .. } = &e.optic else {
        return None;
    };
    let name = e.name.as_ref().map(|n| n.to_string()).unwrap_or_default();
    Some(emitter(
        &name,
        color.x as f32,
        color.y as f32,
        color.z as f32,
    ))
}

/// Maps a GDTF standard-attribute name (the `Attribute` XML value on a
/// `<LogicalChannel>`, e.g. `"Dimmer"`, `"Pan"`, `"ColorAdd_R"`) onto this
/// project's own `Attribute` enum. GDTF's attribute set is much larger than
/// what `ignition-proto::Attribute` models (see that type's own doc comment
/// — it's deliberately a subset, not an attempt to cover the whole GDTF
/// attribute library up front) — an unrecognised name still round-trips via
/// `Attribute::Custom` rather than being silently dropped, so nothing about
/// a real fixture's channel list disappears just because this project
/// doesn't act on it yet.
fn map_attribute_name(name: &str) -> Option<Attribute> {
    match name {
        "Dimmer" => Some(Attribute::Dimmer),
        "Pan" => Some(Attribute::Pan),
        "Tilt" => Some(Attribute::Tilt),
        "ColorAdd_R" => Some(Attribute::ColorAdd {
            channel: ColorChannel::Red,
        }),
        "ColorAdd_G" => Some(Attribute::ColorAdd {
            channel: ColorChannel::Green,
        }),
        "ColorAdd_B" => Some(Attribute::ColorAdd {
            channel: ColorChannel::Blue,
        }),
        "ColorAdd_W" => Some(Attribute::ColorAdd {
            channel: ColorChannel::White,
        }),
        "ColorAdd_A" | "ColorAdd_Am" => Some(Attribute::ColorAdd {
            channel: ColorChannel::Amber,
        }),
        "ColorAdd_UV" => Some(Attribute::ColorAdd {
            channel: ColorChannel::Uv,
        }),
        "ColorAdd_L" | "ColorAdd_Lime" => Some(Attribute::ColorAdd {
            channel: ColorChannel::Lime,
        }),
        "Zoom" => Some(Attribute::Zoom),
        "Focus" | "Focus1" => Some(Attribute::Focus),
        "Iris" => Some(Attribute::Iris),
        s if s.starts_with("Gobo") && !s.contains("Pos") && !s.contains("Rotate") => {
            Some(Attribute::GoboWheel { slot: 0 })
        }
        s if s.starts_with("Color") && s.starts_with("Color1") => {
            Some(Attribute::ColorWheel { slot: 0 })
        }
        s if s.starts_with("Shutter") || s.starts_with("Strobe") => Some(Attribute::Strobe),
        "" => None, // Virtual/placeholder logical channel, not a real attribute.
        other => Some(Attribute::Custom(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real GDTF file, not hand-authored test data — see
    /// `assets/gdtf-samples/LICENSE-NOTICE.txt`.
    const SAMPLE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/gdtf-samples/Generic@RGBW8@test.gdtf"
    );

    #[test]
    fn imports_real_channel_layout_from_a_gdtf_file() {
        let result =
            import_channel_map(Path::new(SAMPLE), None).expect("parses the sample GDTF file");
        assert_eq!(result.dmx_mode_name, "Default");
        assert_eq!(result.channel_map.footprint, 4);
        assert_eq!(
            result.channel_map.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::Red
            }),
            Some(0)
        );
        assert_eq!(
            result.channel_map.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::Green
            }),
            Some(1)
        );
        assert_eq!(
            result.channel_map.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::Blue
            }),
            Some(2)
        );
        assert_eq!(
            result.channel_map.offset_of(&Attribute::ColorAdd {
                channel: ColorChannel::White
            }),
            Some(3)
        );
        // The file's virtual Dimmer channel has no DMX offset at all (it's
        // implemented by the RGBW mix, not a real byte) — correctly absent
        // from the resolved channel map, not silently mapped to offset 0.
        assert_eq!(result.channel_map.offset_of(&Attribute::Dimmer), None);
    }

    /// The sample file declares an sRGB `ColorSpace` but no `Emitters`
    /// node, so its four `ColorAdd_*` channels come back with the
    /// class-typical chromaticities — enough for the solve to put a white
    /// on its white.
    #[test]
    /// r[verify color.emitter-solve] - a GDTF import yields per-channel emitters
    fn imports_emitters_for_each_color_channel() {
        let result = import_channel_map(Path::new(SAMPLE), None).unwrap();
        let emitters = result.emitters.expect("an RGBW fixture has emitters");
        assert_eq!(
            emitters
                .channels
                .iter()
                .map(|(c, _)| *c)
                .collect::<Vec<_>>(),
            vec![
                ColorChannel::Red,
                ColorChannel::Green,
                ColorChannel::Blue,
                ColorChannel::White
            ]
        );
        assert!(emitters.beyond_rgb());
        assert!(import_emitters(Path::new(SAMPLE)).unwrap().is_empty());
    }

    /// A file that does carry `PhysicalDescriptions/Emitters` linked from
    /// its channel functions: the measured chromaticity wins over the
    /// typical one.
    #[test]
    /// r[verify color.emitter-solve] - measured GDTF emitter data reaches the profile
    fn measured_emitters_override_the_typical_ones() {
        let description: gdtf::Description = EMITTER_XML.parse().unwrap();
        let result = channel_map_from_description(&description, None).unwrap();
        let emitters = result.emitters.unwrap();
        let (_, red) = &emitters.channels[0];
        assert!(
            (red.x - 0.700).abs() < 1e-4 && (red.y - 0.299).abs() < 1e-4,
            "{red:?}"
        );
        assert_eq!(red.name, "LED Red");
        assert!((red.max_lumens - 0.25).abs() < 1e-4);
        let (_, green) = &emitters.channels[1];
        assert_eq!(green.name, "LED Green");
    }

    /// A hand-written `description.xml` with two linked emitters.
    const EMITTER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<GDTF DataVersion="1.2">
  <FixtureType Name="Emitter Test" ShortName="ET" LongName="Emitter Test" Manufacturer="Test" Description="" FixtureTypeID="00000000-0000-0000-0000-000000000001" RefFT="">
    <AttributeDefinitions>
      <ActivationGroups/>
      <FeatureGroups>
        <FeatureGroup Name="Color" Pretty="Color"><Feature Name="RGB"/></FeatureGroup>
      </FeatureGroups>
      <Attributes>
        <Attribute Name="ColorAdd_R" Pretty="R" Feature="Color.RGB" PhysicalUnit="ColorComponent"/>
        <Attribute Name="ColorAdd_G" Pretty="G" Feature="Color.RGB" PhysicalUnit="ColorComponent"/>
      </Attributes>
    </AttributeDefinitions>
    <Wheels/>
    <PhysicalDescriptions>
      <Emitters>
        <Emitter Name="LED Red" Color="0.700000,0.299000,0.250000" DominantWaveLength="625.000000"/>
        <Emitter Name="LED Green" Color="0.170000,0.720000,0.600000" DominantWaveLength="525.000000"/>
      </Emitters>
      <ColorSpace Mode="sRGB"/>
    </PhysicalDescriptions>
    <Models/>
    <Geometries>
      <Geometry Name="Body" Position="{1,0,0,0}{0,1,0,0}{0,0,1,0}{0,0,0,1}"/>
    </Geometries>
    <DMXModes>
      <DMXMode Name="Default" Geometry="Body">
        <DMXChannels>
          <DMXChannel DMXBreak="1" Offset="1" Highlight="255/1" Geometry="Body">
            <LogicalChannel Attribute="ColorAdd_R">
              <ChannelFunction Name="Red" Attribute="ColorAdd_R" DMXFrom="0/1" Emitter="LED Red"/>
            </LogicalChannel>
          </DMXChannel>
          <DMXChannel DMXBreak="1" Offset="2" Highlight="255/1" Geometry="Body">
            <LogicalChannel Attribute="ColorAdd_G">
              <ChannelFunction Name="Green" Attribute="ColorAdd_G" DMXFrom="0/1" Emitter="LED Green"/>
            </LogicalChannel>
          </DMXChannel>
        </DMXChannels>
        <Relations/>
        <FTMacros/>
      </DMXMode>
    </DMXModes>
  </FixtureType>
</GDTF>
"#;

    #[test]
    fn unknown_mode_name_is_a_real_error_not_a_panic() {
        let result = import_channel_map(Path::new(SAMPLE), Some("Nonexistent Mode"));
        assert!(result.is_err());
    }
}
