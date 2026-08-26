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

use crate::fixture_profile::{
    ColorWheelSlot, DeclaredColorSpace, FixtureEmitters, FixtureProfile, rest_defaults,
    typical_emitter,
};
use gdtf::GdtfFile;
use ignition_core::color::{ColorSpace, Emitter, Intent, emitter};
use ignition_proto::{Attribute, ChannelMap, ColorChannel};
use std::collections::HashMap;
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
    /// The colour wheel's slots, from the first `Color1` channel function
    /// that links a wheel; empty when the mode has no wheel.
    pub wheel: Vec<ColorWheelSlot>,
    /// The fixture type's declared colour space (`PhysicalDescriptions/
    /// ColorSpace`); sRGB when it declares none.
    pub color_space: DeclaredColorSpace,
    /// Each channel's `Default` (from its initial channel function),
    /// in cue-engine units — degrees for pan/tilt, `0..=1` otherwise.
    /// Channels whose function carries no usable default fall back to
    /// `rest_defaults`.
    pub defaults: HashMap<Attribute, f32>,
}

impl GdtfChannelMap {
    /// The output-side profile this import describes.
    // r[impl color.spaces] - the GDTF colour space reaches the solve
    // r[impl playback.defaults] - GDTF defaults reach the floor
    pub fn profile(&self) -> FixtureProfile {
        let mut profile = FixtureProfile::from_channel_map(self.channel_map.clone())
            .with_wheel(self.wheel.clone());
        if self.emitters.is_some() {
            profile.emitters = self.emitters.clone();
        }
        profile.color_space = self.color_space.clone();
        profile.defaults = self.defaults.clone();
        profile
    }
}

/// A GDTF `DmxValue` as a fraction of its own byte width.
fn dmx_fraction(value: gdtf::values::DmxValue) -> f32 {
    let bits = 8 * value.bytes().get() as u32;
    let max = ((1u64 << bits) - 1) as f64;
    (value.value() as f64 / max) as f32
}

/// A GDTF `DmxValue` at 8 bits — its coarse byte.
fn dmx_byte(value: gdtf::values::DmxValue) -> u8 {
    (dmx_fraction(value) * 255.0).round().clamp(0.0, 255.0) as u8
}

/// A channel function's `Default` in cue-engine units for `attr`.
fn default_for(attr: &Attribute, function: &gdtf::dmx_mode::ChannelFunction) -> f32 {
    let f = dmx_fraction(function.default);
    match attr {
        Attribute::Pan => (f - 0.5) * 540.0,
        Attribute::Tilt => (f - 0.5) * 270.0,
        _ => f,
    }
}

fn gdtf_color_space(fixture_type: &gdtf::fixture_type::FixtureType) -> DeclaredColorSpace {
    use gdtf::physical_descriptions::ColorSpaceMode;
    let Some(space) = &fixture_type.physical_descriptions.color_space else {
        return DeclaredColorSpace::Known(ColorSpace::Srgb);
    };
    let xy = |c: &gdtf::values::ColorCie| (c.x as f32, c.y as f32);
    match &space.mode {
        ColorSpaceMode::Srgb => DeclaredColorSpace::Known(ColorSpace::Srgb),
        ColorSpaceMode::ProPhoto => DeclaredColorSpace::Primaries {
            red: (0.7347, 0.2653),
            green: (0.1596, 0.8404),
            blue: (0.0366, 0.0001),
            white: (0.3457, 0.3585),
        },
        ColorSpaceMode::Ansi => DeclaredColorSpace::Primaries {
            red: (0.7347, 0.2653),
            green: (0.1596, 0.8404),
            blue: (0.0366, 0.001),
            white: (0.4254, 0.4044),
        },
        ColorSpaceMode::Custom {
            red,
            green,
            blue,
            white_point,
        } => DeclaredColorSpace::Primaries {
            red: xy(red),
            green: xy(green),
            blue: xy(blue),
            white: xy(white_point),
        },
    }
}

/// The slots of the wheel `function` links, with the byte each slot's
/// channel set starts at; evenly spaced when the function lists no sets.
fn gdtf_wheel_slots(
    fixture_type: &gdtf::fixture_type::FixtureType,
    function: &gdtf::dmx_mode::ChannelFunction,
) -> Vec<ColorWheelSlot> {
    let Some(wheel) = function.wheel(fixture_type) else {
        return Vec::new();
    };
    let slot_color = |slot: &gdtf::wheel::WheelSlot| -> Option<Intent> {
        let gdtf::wheel::WheelSlotOptic::Color(c) = &slot.optic else {
            return None;
        };
        Some(Intent::Xy {
            x: c.x as f32,
            y: c.y as f32,
            luminance: 1.0,
        })
    };
    let name = |slot: &gdtf::wheel::WheelSlot| {
        slot.name
            .as_ref()
            .map(|n| n.to_string())
            .unwrap_or_default()
    };
    let by_sets: Vec<ColorWheelSlot> = function
        .channel_sets
        .iter()
        .filter_map(|set| {
            // The parser has already made GDTF's 1-based WheelSlotIndex
            // 0-based (see `ChannelSet::wheel_slot`).
            let index = usize::try_from(set.wheel_slot_index?).ok()?;
            let slot = wheel.slots.get(index)?;
            Some(ColorWheelSlot {
                name: name(slot),
                byte: dmx_byte(set.dmx_from),
                color: slot_color(slot)?,
            })
        })
        .collect();
    if !by_sets.is_empty() {
        return by_sets;
    }
    let n = wheel.slots.len().max(1) as f32;
    wheel
        .slots
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| {
            Some(ColorWheelSlot {
                name: name(slot),
                byte: ((i as f32 + 0.5) / n * 255.0).round() as u8,
                color: slot_color(slot)?,
            })
        })
        .collect()
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
    let mut defaults = HashMap::new();
    let mut wheel = Vec::new();
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
            let initial = ch
                .initial_function()
                .map(|(_, f)| f)
                .or_else(|| logical.channel_functions.first());
            if let Some(function) = initial
                && !matches!(attr, Attribute::PanFine | Attribute::TiltFine)
            {
                defaults.insert(attr.clone(), default_for(&attr, function));
            }
            if matches!(attr, Attribute::ColorWheel { .. }) && wheel.is_empty() {
                wheel = logical
                    .channel_functions
                    .iter()
                    .map(|f| gdtf_wheel_slots(fixture_type, f))
                    .find(|w| !w.is_empty())
                    .unwrap_or_default();
            }
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

    let channel_map = ChannelMap {
        curves: Default::default(),
        footprint,
        channels,
    };
    for (attr, rest) in rest_defaults(&channel_map) {
        defaults.entry(attr).or_insert(rest);
    }
    Ok(GdtfChannelMap {
        fixture_type_name,
        dmx_mode_name,
        channel_map,
        emitters,
        wheel,
        color_space: gdtf_color_space(fixture_type),
        defaults,
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
        "ColorAdd_A" | "ColorAdd_Am" | "ColorAdd_RY" => Some(Attribute::ColorAdd {
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

    /// r[verify color.spaces] - a GDTF custom colour space reaches the profile
    /// r[verify playback.defaults] - a channel function's Default reaches the profile
    /// r[verify color.mix-or-wheel] - a GDTF wheel's slots reach the profile with their bytes
    #[test]
    fn colour_space_defaults_and_wheel_come_from_the_description() {
        let description: gdtf::Description = SPACE_XML.parse().unwrap();
        let result = channel_map_from_description(&description, None).unwrap();
        let DeclaredColorSpace::Primaries { red, white, .. } = &result.color_space else {
            panic!("custom space expected, got {:?}", result.color_space);
        };
        assert!((red.0 - 0.7).abs() < 1e-5 && (white.1 - 0.33).abs() < 1e-5);
        assert!((result.defaults[&Attribute::Dimmer] - 0.0).abs() < 1e-6);
        assert!((result.defaults[&Attribute::Zoom] - 0.5).abs() < 0.01);
        assert!(
            (result.defaults[&Attribute::Pan] - 0.0).abs() < 1.5,
            "{:?}",
            result.defaults
        );
        assert_eq!(result.wheel.len(), 2);
        assert_eq!(result.wheel[0].name, "Open");
        assert_eq!(result.wheel[1].byte, 20);
        assert!(matches!(result.wheel[1].color, Intent::Xy { x, .. } if (x - 0.68).abs() < 1e-5));
        let profile = result.profile();
        assert_eq!(
            profile.color_preference,
            ignition_core::color::ColorPreference::Mix,
            "mixing exists, so mixing is preferred"
        );
        // sRGB is what a description without a ColorSpace node means.
        let description: gdtf::Description = EMITTER_XML.parse().unwrap();
        let result = channel_map_from_description(&description, None).unwrap();
        assert_eq!(
            result.color_space,
            DeclaredColorSpace::Known(ColorSpace::Srgb)
        );
    }

    const SPACE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<GDTF DataVersion="1.2">
  <FixtureType Name="Space Test" ShortName="ST" LongName="Space Test" Manufacturer="Test" Description="" FixtureTypeID="00000000-0000-0000-0000-000000000002" RefFT="">
    <AttributeDefinitions>
      <ActivationGroups/>
      <FeatureGroups>
        <FeatureGroup Name="Color" Pretty="Color"><Feature Name="RGB"/><Feature Name="Color"/></FeatureGroup>
        <FeatureGroup Name="Dimmer" Pretty="Dimmer"><Feature Name="Dimmer"/></FeatureGroup>
        <FeatureGroup Name="Beam" Pretty="Beam"><Feature Name="Beam"/></FeatureGroup>
        <FeatureGroup Name="Position" Pretty="Position"><Feature Name="PanTilt"/></FeatureGroup>
      </FeatureGroups>
      <Attributes>
        <Attribute Name="Dimmer" Pretty="Dim" Feature="Dimmer.Dimmer" PhysicalUnit="LuminousIntensity"/>
        <Attribute Name="Zoom" Pretty="Zoom" Feature="Beam.Beam" PhysicalUnit="Angle"/>
        <Attribute Name="Pan" Pretty="P" Feature="Position.PanTilt" PhysicalUnit="Angle"/>
        <Attribute Name="ColorAdd_R" Pretty="R" Feature="Color.RGB" PhysicalUnit="ColorComponent"/>
        <Attribute Name="Color1" Pretty="C1" Feature="Color.Color" PhysicalUnit="None"/>
      </Attributes>
    </AttributeDefinitions>
    <Wheels>
      <Wheel Name="ColorWheel">
        <Slot Name="Open" Color="0.3127,0.3290,1.000000"/>
        <Slot Name="Red" Color="0.680000,0.310000,0.250000"/>
      </Wheel>
    </Wheels>
    <PhysicalDescriptions>
      <Emitters/>
      <ColorSpace Mode="Custom" Red="0.700000,0.300000,1.000000" Green="0.200000,0.700000,1.000000" Blue="0.140000,0.060000,1.000000" WhitePoint="0.320000,0.330000,1.000000"/>
    </PhysicalDescriptions>
    <Models/>
    <Geometries>
      <Geometry Name="Body" Position="{1,0,0,0}{0,1,0,0}{0,0,1,0}{0,0,0,1}"/>
    </Geometries>
    <DMXModes>
      <DMXMode Name="Default" Geometry="Body">
        <DMXChannels>
          <DMXChannel DMXBreak="1" Offset="1" Highlight="255/1" Geometry="Body" InitialFunction="Dimmer.Dimmer.Dimmer 1">
            <LogicalChannel Attribute="Dimmer">
              <ChannelFunction Name="Dimmer 1" Attribute="Dimmer" DMXFrom="0/1" Default="0/1"/>
            </LogicalChannel>
          </DMXChannel>
          <DMXChannel DMXBreak="1" Offset="2" Highlight="255/1" Geometry="Body">
            <LogicalChannel Attribute="Zoom">
              <ChannelFunction Name="Zoom 1" Attribute="Zoom" DMXFrom="0/1" Default="128/1"/>
            </LogicalChannel>
          </DMXChannel>
          <DMXChannel DMXBreak="1" Offset="3,4" Highlight="255/1" Geometry="Body">
            <LogicalChannel Attribute="Pan">
              <ChannelFunction Name="Pan 1" Attribute="Pan" DMXFrom="0/2" Default="32768/2"/>
            </LogicalChannel>
          </DMXChannel>
          <DMXChannel DMXBreak="1" Offset="5" Highlight="255/1" Geometry="Body">
            <LogicalChannel Attribute="ColorAdd_R">
              <ChannelFunction Name="Red" Attribute="ColorAdd_R" DMXFrom="0/1" Default="0/1"/>
            </LogicalChannel>
          </DMXChannel>
          <DMXChannel DMXBreak="1" Offset="6" Highlight="0/1" Geometry="Body">
            <LogicalChannel Attribute="Color1">
              <ChannelFunction Name="Wheel" Attribute="Color1" DMXFrom="0/1" Default="0/1" Wheel="ColorWheel">
                <ChannelSet Name="Open" DMXFrom="0/1" WheelSlotIndex="1"/>
                <ChannelSet Name="Red" DMXFrom="20/1" WheelSlotIndex="2"/>
              </ChannelFunction>
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
