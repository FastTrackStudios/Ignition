//! The attribute vocabulary: what a channel's prose name means.
//!
//! A fixture-type document names its channels the way the manual does —
//! `Dimmer`, `Warm White 3`, `Colour Selection`, `Dimmer (per LED
//! section)` — because the document is written by a person reading a
//! manual, and forcing them to spell `ColorAdd_WW` at authoring time is
//! how a channel map ends up wrong. Resolution happens here instead,
//! once, on the way in.
//!
//! Ported from `tools/make_gdtf.py`'s `ATTRS` / `FUZZY` / `resolve_attr`,
//! which is the same vocabulary the generated GDTF profiles are built
//! with. The two must agree: a document that resolves one way here and
//! another way there would put the visualizer's picture and the wire out
//! of step, which is the one thing `r[dmx.one-frame]` forbids.

// r[impl patch.type-vocabulary] - the manual's own words, resolved on the way in
// r[impl patch.type-unknown-channel] - an unresolved name keeps its byte and its name

use ignition_proto::{Attribute, ColorChannel};

/// A resolved channel function — the vocabulary entry a prose name
/// landed on, or the prose itself when nothing matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Function {
    /// A name in the vocabulary below.
    Known(Known),
    /// Outside the vocabulary. Carried through rather than dropped: a
    /// channel nobody has classified still occupies a byte, and a
    /// fixture whose footprint is short by one is a fixture that bleeds
    /// into its neighbour.
    Unknown(String),
}

/// Every channel function this project has a name for.
/// The vocabulary is closed by design — an unrecognised name becomes
/// [`Function::Unknown`] rather than a new variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    Dimmer,
    Pan,
    Tilt,
    PanFine,
    TiltFine,
    Red,
    Green,
    Blue,
    White,
    Amber,
    Uv,
    Lime,
    Cyan,
    Magenta,
    Yellow,
    WarmWhite,
    CoolWhite,
    Strobe,
    Shutter,
    /// A colour *wheel*, not a mixing channel.
    ColorWheel,
    ColorMacro,
    Gobo,
    GoboRotate,
    Prism,
    Focus,
    Zoom,
    Iris,
    Speed,
    Macro,
    Program,
    Sound,
    Fog,
    Haze,
    Pump,
    Fan,
    Lamp,
    Rotation,
    Reset,
    Control,
}

impl Known {
    /// The canonical spelling, which is also the key
    /// `tools/make_gdtf.py` uses.
    #[must_use]
    pub const fn canonical(self) -> &'static str {
        match self {
            Self::Dimmer => "Dimmer",
            Self::Pan => "Pan",
            Self::Tilt => "Tilt",
            Self::PanFine => "PanFine",
            Self::TiltFine => "TiltFine",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::White => "White",
            Self::Amber => "Amber",
            Self::Uv => "UV",
            Self::Lime => "Lime",
            Self::Cyan => "Cyan",
            Self::Magenta => "Magenta",
            Self::Yellow => "Yellow",
            Self::WarmWhite => "WarmWhite",
            Self::CoolWhite => "CoolWhite",
            Self::Strobe => "Strobe",
            Self::Shutter => "Shutter",
            Self::ColorWheel => "Color",
            Self::ColorMacro => "ColorMacro",
            Self::Gobo => "Gobo",
            Self::GoboRotate => "GoboRotate",
            Self::Prism => "Prism",
            Self::Focus => "Focus",
            Self::Zoom => "Zoom",
            Self::Iris => "Iris",
            Self::Speed => "Speed",
            Self::Macro => "Macro",
            Self::Program => "Program",
            Self::Sound => "Sound",
            Self::Fog => "Fog",
            Self::Haze => "Haze",
            Self::Pump => "Pump",
            Self::Fan => "Fan",
            Self::Lamp => "Lamp",
            Self::Rotation => "Rotation",
            Self::Reset => "Reset",
            Self::Control => "Control",
        }
    }

    /// The GDTF attribute name, for export.
    #[must_use]
    pub const fn gdtf(self) -> &'static str {
        match self {
            Self::Dimmer => "Dimmer",
            // A fine channel carries the low byte of the attribute it
            // refines, so in GDTF it is the same attribute at a second
            // offset — not an attribute of its own.
            Self::Pan | Self::PanFine => "Pan",
            Self::Tilt | Self::TiltFine => "Tilt",
            Self::Red => "ColorAdd_R",
            Self::Green => "ColorAdd_G",
            Self::Blue => "ColorAdd_B",
            Self::White => "ColorAdd_W",
            Self::Amber => "ColorAdd_RY",
            Self::Uv => "ColorAdd_UV",
            Self::Lime => "ColorAdd_GY",
            Self::Cyan => "ColorAdd_C",
            Self::Magenta => "ColorAdd_M",
            Self::Yellow => "ColorAdd_Y",
            Self::WarmWhite => "ColorAdd_WW",
            Self::CoolWhite => "ColorAdd_CW",
            Self::Strobe | Self::Shutter => "Shutter1",
            Self::ColorWheel => "Color1",
            Self::ColorMacro => "ColorMacro1",
            Self::Gobo => "Gobo1",
            Self::GoboRotate => "Gobo1PosRotate",
            Self::Prism => "Prism1",
            Self::Focus => "Focus1",
            Self::Zoom => "Zoom",
            Self::Iris => "Iris",
            Self::Speed => "PositionMSpeed",
            Self::Macro => "Effects2",
            Self::Program => "Effects1",
            Self::Sound => "Sound",
            Self::Fog | Self::Pump => "Fog1",
            Self::Haze => "Haze1",
            Self::Fan => "Fan1",
            Self::Lamp => "LampControl",
            Self::Rotation => "Rotation",
            Self::Reset | Self::Control => "Control1",
        }
    }

    /// The engine-side attribute this drives, if the engine has one.
    ///
    /// `None` is not "ignore this channel" — it is "the engine has
    /// nothing to say about it", which is true of a macro channel, a
    /// reset channel or a fan. Those still occupy their byte and still
    /// take a rest default; they are simply never the target of a cue.
    #[must_use]
    pub fn attribute(self) -> Option<Attribute> {
        let colour = |channel| Some(Attribute::ColorAdd { channel });
        match self {
            Self::Dimmer => Some(Attribute::Dimmer),
            Self::Pan => Some(Attribute::Pan),
            Self::Tilt => Some(Attribute::Tilt),
            Self::PanFine => Some(Attribute::PanFine),
            Self::TiltFine => Some(Attribute::TiltFine),
            Self::Red => colour(ColorChannel::Red),
            Self::Green => colour(ColorChannel::Green),
            Self::Blue => colour(ColorChannel::Blue),
            // A fixture with warm and cool white emitters has two white
            // channels and the engine has one `White`. The warm one is
            // the one a colour solve can reach; the cool one is carried
            // as a custom attribute so the byte is still described.
            Self::White | Self::WarmWhite => colour(ColorChannel::White),
            Self::Amber => colour(ColorChannel::Amber),
            Self::Uv => colour(ColorChannel::Uv),
            Self::Lime => colour(ColorChannel::Lime),
            Self::Strobe | Self::Shutter => Some(Attribute::Strobe),
            // Slot 0 is the wheel's home. The real slot table lives on
            // the fixture type's `wheels`, and the patch resolves a
            // colour to a slot at output time.
            Self::ColorWheel => Some(Attribute::ColorWheel { slot: 0 }),
            Self::Gobo => Some(Attribute::GoboWheel { slot: 0 }),
            Self::Focus => Some(Attribute::Focus),
            Self::Zoom => Some(Attribute::Zoom),
            Self::Iris => Some(Attribute::Iris),
            Self::Cyan
            | Self::Magenta
            | Self::Yellow
            | Self::CoolWhite
            | Self::ColorMacro
            | Self::GoboRotate
            | Self::Prism
            | Self::Speed
            | Self::Macro
            | Self::Program
            | Self::Sound
            | Self::Fog
            | Self::Haze
            | Self::Pump
            | Self::Fan
            | Self::Lamp
            | Self::Rotation
            | Self::Reset
            | Self::Control => None,
        }
    }
}

impl Function {
    /// The engine-side attribute, with an unresolved name carried as
    /// `Attribute::Custom` so nothing is silently dropped.
    #[must_use]
    pub fn attribute(&self) -> Attribute {
        match self {
            Self::Known(known) => known
                .attribute()
                .unwrap_or_else(|| Attribute::Custom(known.canonical().to_owned())),
            Self::Unknown(name) => Attribute::Custom(name.clone()),
        }
    }

    /// What to call this on a patch sheet.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::Known(known) => known.canonical(),
            Self::Unknown(name) => name,
        }
    }
}

/// Exact vocabulary entries, tried before any fuzzy match.
const EXACT: &[(&str, Known)] = &[
    ("dimmer", Known::Dimmer),
    ("pan", Known::Pan),
    ("tilt", Known::Tilt),
    ("panfine", Known::PanFine),
    ("pan fine", Known::PanFine),
    ("tiltfine", Known::TiltFine),
    ("tilt fine", Known::TiltFine),
    ("red", Known::Red),
    ("green", Known::Green),
    ("blue", Known::Blue),
    ("white", Known::White),
    ("amber", Known::Amber),
    ("uv", Known::Uv),
    ("lime", Known::Lime),
    ("cyan", Known::Cyan),
    ("magenta", Known::Magenta),
    ("yellow", Known::Yellow),
    ("warmwhite", Known::WarmWhite),
    ("warm white", Known::WarmWhite),
    ("coolwhite", Known::CoolWhite),
    ("cool white", Known::CoolWhite),
    ("strobe", Known::Strobe),
    ("shutter", Known::Shutter),
    ("color", Known::ColorWheel),
    ("colour", Known::ColorWheel),
    ("colormacro", Known::ColorMacro),
    ("gobo", Known::Gobo),
    ("goborotate", Known::GoboRotate),
    ("prism", Known::Prism),
    ("focus", Known::Focus),
    ("zoom", Known::Zoom),
    ("iris", Known::Iris),
    ("speed", Known::Speed),
    ("macro", Known::Macro),
    ("program", Known::Program),
    ("sound", Known::Sound),
    ("fog", Known::Fog),
    ("haze", Known::Haze),
    ("pump", Known::Pump),
    ("fan", Known::Fan),
    ("lamp", Known::Lamp),
    ("rotation", Known::Rotation),
    ("reset", Known::Reset),
    ("control", Known::Control),
];

/// Substring fallbacks, in priority order — the first needle found in
/// the name wins. Order matters and is not alphabetical: `warm white`
/// must be tried before `white`, `gobo rot` before `gobo`, and `colo`
/// sits last because it would otherwise swallow `color macro`.
const FUZZY: &[(&str, Known)] = &[
    ("warm white", Known::WarmWhite),
    ("cool white", Known::CoolWhite),
    ("color macro", Known::ColorMacro),
    ("colour macro", Known::ColorMacro),
    ("gobo rot", Known::GoboRotate),
    ("pan fine", Known::PanFine),
    ("tilt fine", Known::TiltFine),
    ("curve", Known::Control),
    ("reserved", Known::Control),
    ("mode", Known::Control),
    ("fade", Known::Control),
    ("motor", Known::Rotation),
    ("dimmer", Known::Dimmer),
    ("intensity", Known::Dimmer),
    ("macro", Known::Macro),
    ("program", Known::Program),
    ("speed", Known::Speed),
    ("strobe", Known::Strobe),
    ("shutter", Known::Shutter),
    ("control", Known::Control),
    ("function", Known::Control),
    ("reset", Known::Reset),
    ("sound", Known::Sound),
    ("haze", Known::Haze),
    ("fog", Known::Fog),
    ("pump", Known::Pump),
    ("fan", Known::Fan),
    ("lamp", Known::Lamp),
    ("rotat", Known::Rotation),
    ("spin", Known::Rotation),
    ("uv", Known::Uv),
    ("amber", Known::Amber),
    ("white", Known::White),
    ("red", Known::Red),
    ("green", Known::Green),
    ("blue", Known::Blue),
    ("lime", Known::Lime),
    ("pan", Known::Pan),
    ("tilt", Known::Tilt),
    ("zoom", Known::Zoom),
    ("focus", Known::Focus),
    ("iris", Known::Iris),
    ("prism", Known::Prism),
    ("gobo", Known::Gobo),
    ("colo", Known::ColorWheel),
];

/// A single emitter letter, as it appears in a compressed channel name
/// like `R/G/B per section`.
#[must_use]
pub fn letter(part: &str) -> Option<Known> {
    match part.trim().to_ascii_uppercase().as_str() {
        "R" => Some(Known::Red),
        "G" => Some(Known::Green),
        "B" => Some(Known::Blue),
        "W" => Some(Known::White),
        "A" => Some(Known::Amber),
        "UV" => Some(Known::Uv),
        "WW" => Some(Known::WarmWhite),
        "CW" => Some(Known::CoolWhite),
        "L" => Some(Known::Lime),
        _ => None,
    }
}

/// Strip a parenthesised aside and a trailing index: `Warm White 3` and
/// `Dimmer (per LED section)` both reduce to something the vocabulary
/// knows.
fn bare(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut depth = 0_u32;
    for ch in name.chars() {
        match ch {
            '(' => depth = depth.saturating_add(1),
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let trimmed = out.trim_end();
    let without_index = trimmed
        .rsplit_once(' ')
        .filter(|(_, last)| !last.is_empty() && last.chars().all(|c| c.is_ascii_digit()))
        .map_or(trimmed, |(head, _)| head);
    without_index.trim().to_owned()
}

/// A channel's prose name, resolved to the vocabulary.
///
/// Exact match first (on the name and on the name with its asides and
/// trailing index stripped), then the substring table. Nothing matching
/// is [`Function::Unknown`], never an error: an undescribed channel is a
/// gap in the research, and a gap in the research must not stop a
/// fixture patching.
#[must_use]
pub fn resolve(name: &str) -> Function {
    let name = name.trim();
    let lowered = name.to_ascii_lowercase();
    let stripped = bare(name);
    let stripped_lower = stripped.to_ascii_lowercase();
    for (needle, known) in EXACT {
        if lowered == *needle || stripped_lower == *needle {
            return Function::Known(*known);
        }
    }
    for (needle, known) in FUZZY {
        if stripped_lower.contains(needle) {
            return Function::Known(*known);
        }
    }
    Function::Unknown(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Function, Known, resolve};

    /// r[verify patch.type-vocabulary] - transcribing, not translating
    #[test]
    fn the_manuals_own_spellings_resolve() {
        // Every one of these is a real `attribute` string out of
        // `data/fixtures/*.json`, which is why they are the test.
        let cases = [
            ("Dimmer", Known::Dimmer),
            ("Warm White 3", Known::WarmWhite),
            ("Dimmer (per LED section)", Known::Dimmer),
            ("Color Selection", Known::ColorWheel),
            ("Section Control", Known::Control),
            ("Aux Speed", Known::Speed),
            ("Aux Macro", Known::Macro),
            ("Motor", Known::Rotation),
            ("DimmerCurve", Known::Control),
            ("Reserved", Known::Control),
            ("PanFine", Known::PanFine),
            ("ColorMacro", Known::ColorMacro),
        ];
        for (name, want) in cases {
            assert_eq!(resolve(name), Function::Known(want), "resolving {name:?}");
        }
    }

    /// r[verify patch.type-unknown-channel] - the byte survives the name
    #[test]
    fn an_unclassified_name_is_carried_not_dropped() {
        // The byte still exists. A fixture whose footprint is short by
        // one bleeds into whatever is patched after it.
        let Function::Unknown(name) = resolve("Vendor Widget") else {
            panic!("an unknown name should not resolve into the vocabulary");
        };
        assert_eq!(name, "Vendor Widget");
    }

    #[test]
    fn warm_white_beats_white() {
        // The fuzzy table is ordered, not alphabetical: `white` would
        // otherwise swallow every warm-white channel on the ZQ01334.
        assert_eq!(resolve("Warm White"), Function::Known(Known::WarmWhite));
        assert_eq!(resolve("Cool White"), Function::Known(Known::CoolWhite));
    }
}
