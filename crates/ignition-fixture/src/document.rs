//! The fixture-type document: what a fixture *is*, as data on disk.
//!
//! This is the shape `data/fixtures/*.json` already has — those files
//! were written by hand while reading manufacturer manuals, and the type
//! here is deliberately fitted to them rather than the other way round.
//! Everything is optional except a name and at least one mode, because
//! the research behind a two-dollar par is often a listing and a
//! photograph, and a fixture whose weight nobody published still has to
//! patch.
//!
//! Two rules shape every field:
//!
//! - **Unknown keys survive** (`r[files.additive-evolution]`). A
//!   document edited by an older build must not lose what that build
//!   knew nothing about, so every level carries an `extra` map and
//!   writes it back out.
//! - **Nothing here panics on bad data.** A fixture document is data,
//!   and a malformed one is a research problem, not a crash: it loads as
//!   far as it can and says what it could not read.

// r[impl patch.type-is-data] - a fixture type is a JSON document, not a match arm
// r[impl patch.type-modes] - named channel charts in the manual's own order
// r[impl patch.type-confidence] - per document and, where they differ, per mode
// r[impl patch.type-sources] - the manual, kept with the facts it produced
// r[impl patch.type-identity] - `console_name` is the identity
// r[impl patch.type-aliases] - one OEM head, four brand names, one document

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Anything the build does not know about, kept so it can be written
/// back unchanged.
pub type Extra = BTreeMap<String, Value>;

/// How much of this document is measured and how much is inferred.
///
/// A patch sheet shows this, because "the channel order on this fixture
/// is a guess" is something an operator wants to know *before* the
/// fixture does something unexpected, not after.
/// Reading is lenient about spelling and writing is not: the research
/// files were written by hand and say `manual found` where a schema
/// would say `manual-found`, and rejecting eighteen documents over a
/// hyphen would be a schema serving itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    /// The DMX chart came from a manufacturer manual or a published
    /// profile.
    #[serde(alias = "manual found", alias = "manual")]
    ManualFound,
    /// A reseller listing and the console's own channel names; ranges
    /// inferred.
    #[serde(alias = "listing only", alias = "listing", alias = "photograph")]
    ListingOnly,
    /// The product could not be identified. Everything beyond the
    /// console's channel names is inference.
    #[default]
    #[serde(alias = "guess", alias = "unknown")]
    Guessed,
}

impl Confidence {
    /// A short badge for a sheet.
    #[must_use]
    pub const fn badge(self) -> &'static str {
        match self {
            Self::ManualFound => "manual",
            Self::ListingOnly => "listing",
            Self::Guessed => "guess",
        }
    }
}

/// One value range on a channel: what the fixture does between two
/// bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub from: u8,
    pub to: u8,
    /// What the manual says happens here, in the manual's words.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub meaning: String,
    /// The physical span this range maps onto — degrees, hertz, per
    /// cent — when the manual gives one. Feeds GDTF's
    /// `PhysicalFrom`/`PhysicalTo` on export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub physical: Option<(f32, f32)>,
    /// The wheel slot this range selects, by name, for a colour or gobo
    /// wheel channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Range {
    /// The byte at the middle of the range — what a console sends to
    /// land *in* it rather than on its edge, which on a wheel is the
    /// difference between a colour and a split.
    #[must_use]
    pub const fn centre(&self) -> u8 {
        let (lo, hi) = if self.from <= self.to {
            (self.from, self.to)
        } else {
            (self.to, self.from)
        };
        lo.saturating_add(hi.saturating_sub(lo) / 2)
    }
}

/// Which channel or channels an entry describes.
///
/// A document may write a single number, or a span — `"1-24"` — for the
/// compressed forms a research note uses when a fixture has twenty-four
/// identical sections. [`crate::expand`] is what turns a span into
/// individual channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum ChannelRef {
    /// One channel, 1-based within the mode.
    One(u16),
    /// An inclusive span of channels, 1-based.
    Span(u16, u16),
}

impl ChannelRef {
    /// The first channel this covers.
    #[must_use]
    pub const fn first(self) -> u16 {
        match self {
            Self::One(n) | Self::Span(n, _) => n,
        }
    }

    /// How many channels this covers. Never zero: a reversed or
    /// degenerate span is one channel.
    #[must_use]
    pub const fn count(self) -> u16 {
        match self {
            Self::One(_) => 1,
            Self::Span(lo, hi) => hi.saturating_sub(lo).saturating_add(1),
        }
    }
}

impl<'de> Deserialize<'de> for ChannelRef {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        // A number, or a string that is either a number or `lo-hi`.
        // Written by hand, so an en dash turns up as often as a hyphen.
        match Value::deserialize(deserializer)? {
            Value::Number(n) => n
                .as_u64()
                .and_then(|n| u16::try_from(n).ok())
                .map(Self::One)
                .ok_or_else(|| D::Error::custom(format!("channel {n} is not a DMX channel"))),
            Value::String(s) => parse_channel_ref(&s)
                .ok_or_else(|| D::Error::custom(format!("cannot read {s:?} as a channel"))),
            other => Err(D::Error::custom(format!(
                "a channel is a number or a span like \"1-24\", not {other}"
            ))),
        }
    }
}

/// `"7"`, `"1-24"` or `"1–24"` (en dash) as a channel reference.
fn parse_channel_ref(text: &str) -> Option<ChannelRef> {
    let text = text.trim();
    let split = text
        .split_once('-')
        .or_else(|| text.split_once('\u{2013}'))
        .or_else(|| text.split_once('\u{2014}'));
    match split {
        None => text.parse::<u16>().ok().map(ChannelRef::One),
        Some((lo, hi)) => {
            let lo = lo.trim().parse::<u16>().ok()?;
            let hi = hi.trim().parse::<u16>().ok()?;
            Some(if lo == hi {
                ChannelRef::One(lo)
            } else {
                ChannelRef::Span(lo.min(hi), lo.max(hi))
            })
        }
    }
}

/// A string field that a research file may have left as `null`.
///
/// Two documents in the library say `"manufacturer": null` — they are
/// the fixtures whose maker nobody could identify, which is a real
/// answer and the honest one. `null` and `""` mean the same thing here,
/// and neither is a reason to refuse a document that otherwise carries
/// a full channel chart.
fn nullable_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

/// One line of a mode's channel chart, as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub channel: ChannelRef,
    /// The channel's name in the manual's own words — resolved to the
    /// vocabulary by [`crate::attribute::resolve`], never here.
    pub attribute: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<Range>,
    /// The 16-bit partner of this channel, when the mode has one: the
    /// 1-based channel carrying the low byte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fine: Option<u16>,
    /// Where this channel rests when nothing is driving it — the byte a
    /// shutter wants in order to be open, or a colour wheel to be at
    /// white. `None` rests at zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<u8>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One DMX mode: an ordered chart of channels, and how much of it is
/// known.
///
/// Deserialises from either the object form written here or a bare
/// array, which is what every document written before this type existed
/// contains. Writing always uses the object form, so a document is
/// migrated the first time it is saved.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Mode {
    pub channels: Vec<Channel>,
    /// Overrides the document's confidence for this mode alone — an
    /// 8ch chart from the manual beside a 4ch chart that is a guess is
    /// the normal case, not an exotic one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<Confidence>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

impl<'de> Deserialize<'de> for Mode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            /// The legacy form: the mode *is* its channel list.
            Bare(Vec<Channel>),
            Full {
                channels: Vec<Channel>,
                #[serde(default)]
                confidence: Option<Confidence>,
                #[serde(default)]
                note: String,
                #[serde(flatten)]
                extra: Extra,
            },
        }
        Ok(match Either::deserialize(deserializer)? {
            Either::Bare(channels) => Self {
                channels,
                ..Self::default()
            },
            Either::Full {
                channels,
                confidence,
                note,
                extra,
            } => Self {
                channels,
                confidence,
                note,
                extra,
            },
        })
    }
}

/// Physical facts about the box: what it weighs, what it draws, how big
/// it is. Only used for display and for GDTF export — nothing here
/// reaches the wire.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Physical {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_mm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_mm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_mm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight_kg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_w: Option<f32>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// The optics: how wide the beam is and what makes it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Optics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub beam_angle_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_angle_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub led_count: Option<u32>,
    /// Emitter letters — `["R", "G", "B", "W"]` — naming what colours
    /// the fixture actually makes light with. The colour solve needs
    /// this; a document without it can only be driven by raw channel.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emitters: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// How far the head moves. Absent on anything that does not move.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Movement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pan_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilt_deg: Option<f32>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Colour and gobo wheel slot names, in wheel order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wheels {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub color: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gobo: Vec<String>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One fixture type: everything Ignition knows about a model.
///
/// [`Self::console_name`] is the identity. It is what a venue's
/// `fixtures.json` writes in its `model` field and what a generated
/// GDTF profile carries as its `FixtureType` `Name`
/// (`r[viz.gdtf-generated]`), which is why it must be unique across the
/// library and must not be renamed casually — renaming it unpatches
/// every fixture that used it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FixtureType {
    pub console_name: String,
    #[serde(
        default,
        deserialize_with = "nullable_string",
        skip_serializing_if = "String::is_empty"
    )]
    pub manufacturer: String,
    #[serde(
        default,
        deserialize_with = "nullable_string",
        skip_serializing_if = "String::is_empty"
    )]
    pub model: String,
    /// Other model strings a venue might spell this fixture with. The
    /// same OEM head is sold under four brand names, and a venue that
    /// bought it as a Lixada should not need a second document.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub console_aliases: Vec<String>,
    /// Where the facts came from. Kept in the document because the next
    /// person to doubt a channel order needs the manual, not a commit
    /// message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(default)]
    pub physical: Physical,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optics: Option<Optics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub movement: Option<Movement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wheels: Option<Wheels>,
    /// Modes by name — `"8ch"`, `"24ch"` — each an ordered channel
    /// chart. A document with no modes describes a fixture nothing can
    /// address.
    #[serde(default, deserialize_with = "modes")]
    pub modes: BTreeMap<String, Mode>,
    #[serde(default)]
    pub confidence: Confidence,
    #[serde(
        default,
        deserialize_with = "nullable_string",
        skip_serializing_if = "String::is_empty"
    )]
    pub notes: String,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Read the `modes` map, tolerating the flags older documents keep
/// beside the modes themselves.
///
/// The research files write `"8ch_ranges_guess": true` as a *sibling* of
/// `"8ch"`, so the map is not uniformly mode-shaped. Rather than reject
/// those files or model a boolean as a mode, a flag is folded onto the
/// mode it names — `<mode>_guess` and `<mode>_ranges_guess` both mean
/// "this chart is inference" — and anything else that is not a mode is
/// kept in that mode's `extra` if it names one, or dropped with the
/// document's own `extra` catching the rest.
fn modes<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, Mode>, D::Error> {
    let raw: BTreeMap<String, Value> = BTreeMap::deserialize(deserializer)?;
    let mut out: BTreeMap<String, Mode> = BTreeMap::new();
    let mut flags: Vec<String> = Vec::new();
    for (name, value) in raw {
        if value.is_boolean() {
            flags.push(name);
            continue;
        }
        let mode = Mode::deserialize(value).map_err(serde::de::Error::custom)?;
        out.insert(name, mode);
    }
    for flag in flags {
        for suffix in ["_ranges_guess", "_guess"] {
            let Some(base) = flag.strip_suffix(suffix) else {
                continue;
            };
            if let Some(mode) = out.get_mut(base) {
                mode.confidence = Some(Confidence::ListingOnly);
                if mode.note.is_empty() {
                    mode.note = if suffix == "_ranges_guess" {
                        "value ranges inferred, not from a manual".to_owned()
                    } else {
                        "channel chart inferred, not from a manual".to_owned()
                    };
                }
            }
            break;
        }
    }
    Ok(out)
}

impl FixtureType {
    /// The mode with the most channels — what a patch defaults to when
    /// the venue does not say. Modes are named `"8ch"`, `"24ch"` and so
    /// on, but the name is prose and the count is the fact, so the
    /// count is what is compared.
    #[must_use]
    pub fn widest_mode(&self) -> Option<(&str, &Mode)> {
        self.modes
            .iter()
            .max_by_key(|(_, mode)| {
                mode.channels
                    .iter()
                    .map(|c| {
                        u32::from(c.channel.first()).saturating_add(u32::from(c.channel.count()))
                    })
                    .max()
                    .unwrap_or(0)
            })
            .map(|(name, mode)| (name.as_str(), mode))
    }

    /// Every model string that should resolve to this document.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.console_name.as_str())
            .chain(self.console_aliases.iter().map(String::as_str))
    }
}

#[cfg(test)]
mod tests {
    use super::{ChannelRef, Confidence, FixtureType, Mode, parse_channel_ref};

    #[test]
    fn a_span_reads_from_a_hyphen_or_an_en_dash() {
        // Both turn up in the research files, because both turn up in
        // the manuals they were copied from.
        assert_eq!(parse_channel_ref("1-24"), Some(ChannelRef::Span(1, 24)));
        assert_eq!(
            parse_channel_ref("1\u{2013}24"),
            Some(ChannelRef::Span(1, 24))
        );
        assert_eq!(parse_channel_ref(" 7 "), Some(ChannelRef::One(7)));
        assert_eq!(parse_channel_ref("3-3"), Some(ChannelRef::One(3)));
        assert_eq!(parse_channel_ref("nonsense"), None);
    }

    /// r[verify patch.type-modes] - a chart, however it was written
    #[test]
    fn a_mode_reads_from_a_bare_array_or_an_object() {
        // The bare array is every document written before `Mode`
        // existed; the object is what saving produces.
        let bare: Mode =
            serde_json::from_str(r#"[{"channel": 1, "attribute": "Dimmer", "ranges": []}]"#)
                .unwrap();
        let full: Mode = serde_json::from_str(
            r#"{"channels": [{"channel": 1, "attribute": "Dimmer"}], "note": "hi"}"#,
        )
        .unwrap();
        assert_eq!(bare.channels.len(), 1);
        assert_eq!(full.channels.len(), 1);
        assert_eq!(full.note, "hi");
    }

    /// r[verify patch.type-confidence] - per mode where they differ
    #[test]
    fn a_guess_flag_beside_a_mode_lands_on_that_mode() {
        // `"8ch_ranges_guess": true` sits as a sibling of `"8ch"` in
        // the research files. It is a fact about the 8ch chart, so that
        // is where it has to end up — not as a mode of its own.
        let doc: FixtureType = serde_json::from_str(
            r#"{"console_name": "X", "modes": {
                 "8ch": [{"channel": 1, "attribute": "Dimmer"}],
                 "8ch_ranges_guess": true
               }}"#,
        )
        .unwrap();
        assert_eq!(doc.modes.len(), 1, "the flag is not a mode");
        let mode = doc.modes.get("8ch").unwrap();
        assert_eq!(mode.confidence, Some(Confidence::ListingOnly));
        assert!(mode.note.contains("ranges inferred"));
    }

    #[test]
    fn an_unknown_key_survives_a_round_trip() {
        // r[verify files.additive-evolution] - a document edited by an
        // older build must not lose what that build did not know about.
        let json = r#"{"console_name": "X", "modes": {}, "invented_later": {"a": 1}}"#;
        let doc: FixtureType = serde_json::from_str(json).unwrap();
        let back = serde_json::to_string(&doc).unwrap();
        assert!(back.contains("invented_later"), "{back}");
    }
}
