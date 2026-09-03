//! Colour and gobo wheels, read out of a channel's value ranges.
//!
//! A wheel mover's colour channel is not a mixing channel — it is a
//! list of discrete slots, and a cue asking for "deep blue" has to land
//! on the byte that puts the blue gel in front of the lamp rather than
//! on the split between blue and yellow. The slot table is what makes
//! that possible, and it is already written down: the document's `Color`
//! channel has a range per slot with the manual's own name in it.
//!
//! ```text
//! {"from": 16, "to": 31, "meaning": "red"}
//! {"from": 32, "to": 47, "meaning": "pale blue / cyan"}
//! ```
//!
//! So the table is derived, not authored a second time. The byte is the
//! **centre** of the range, because the edges of a wheel slot are where
//! the split colours live.
//!
//! Ported from `tools/make_gdtf.py`'s `NAMED_COLORS` / `named_color`,
//! and it must agree with it for the same reason [`crate::attribute`]
//! must.

use crate::attribute::{Function, Known, resolve};
use crate::document::{FixtureType, Range};
use crate::expand;

/// One position of a colour wheel.
#[derive(Debug, Clone, PartialEq)]
pub struct Slot {
    /// The manual's name for it.
    pub name: String,
    /// The byte that lands in the middle of this slot.
    pub byte: u8,
    /// Approximate CIE xy. Nobody has measured these wheels; these are
    /// the textbook chromaticities of the named colours, which is good
    /// enough to pick the right slot for a preset and nothing more.
    pub xy: (f32, f32),
}

/// Textbook CIE xy for the colours a wheel is described with. Longest
/// name first when matching, so `light blue` beats `blue` and
/// `warm white` beats `white`.
const NAMED: &[(&str, (f32, f32))] = &[
    ("open", (0.3127, 0.3290)),
    ("white", (0.3127, 0.3290)),
    ("warm white", (0.4500, 0.4100)),
    ("cool white", (0.2900, 0.3000)),
    ("red", (0.6394, 0.3302)),
    ("yellow", (0.4500, 0.5108)),
    ("green", (0.3000, 0.6000)),
    ("magenta", (0.3900, 0.1899)),
    ("blue", (0.1481, 0.0603)),
    ("light blue", (0.2000, 0.2500)),
    ("pale blue", (0.2000, 0.2500)),
    ("dark blue", (0.1481, 0.0603)),
    ("orange", (0.6025, 0.3832)),
    ("cyan", (0.2419, 0.3705)),
    ("pink", (0.4100, 0.2600)),
    ("purple", (0.2700, 0.1200)),
    ("violet", (0.2200, 0.0900)),
    ("uv", (0.1800, 0.0500)),
    ("amber", (0.5752, 0.4242)),
    ("lime", (0.4400, 0.5500)),
    ("turquoise", (0.2000, 0.4000)),
    ("teal", (0.2000, 0.4000)),
    ("rose", (0.4500, 0.2800)),
    ("lavender", (0.3000, 0.2000)),
    ("salmon", (0.5000, 0.3400)),
    ("gold", (0.5000, 0.4500)),
    ("peach", (0.4500, 0.3800)),
    ("sky", (0.2300, 0.2700)),
];

/// Words that mean a range is *not* a slot.
///
/// Above the slot ranges a wheel channel usually carries split colours
/// and then continuous rotation. Those are real behaviours and they are
/// not positions: a cue that landed on one would put the wheel between
/// two gels, or spinning.
const NOT_A_SLOT: &[&str] = &[
    "rotat",
    "scroll",
    "spin",
    "split",
    "rainbow",
    "flow",
    "auto",
    "sound",
    "reset",
    "macro",
    "index",
    "continuous",
    "半",
    "cw",
    "ccw",
];

/// The colour named in a range's prose, if any. The longest known name
/// found wins, so `pale blue / cyan` is pale blue rather than blue.
fn named(meaning: &str) -> Option<(&'static str, (f32, f32))> {
    let text = meaning.to_ascii_lowercase();
    let mut best: Option<(&'static str, (f32, f32))> = None;
    for (name, xy) in NAMED {
        if !text.contains(name) {
            continue;
        }
        if best.is_none_or(|(existing, _)| name.len() > existing.len()) {
            best = Some((*name, *xy));
        }
    }
    best
}

/// Whether a range describes a position rather than a movement.
fn is_slot(range: &Range) -> bool {
    let text = range.meaning.to_ascii_lowercase();
    !NOT_A_SLOT.iter().any(|word| text.contains(word))
}

/// The slot table of a wheel channel's ranges.
fn slots_of(ranges: &[Range]) -> Vec<Slot> {
    ranges
        .iter()
        .filter(|range| is_slot(range))
        .filter_map(|range| {
            let (name, xy) = named(&range.meaning)?;
            Some(Slot {
                // The manual's own words, trimmed of the alias half:
                // the sheet should say "pale blue", not "pale blue /
                // cyan", and never the textbook name if the manual gave
                // a better one.
                name: range
                    .meaning
                    .split('/')
                    .next()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(name)
                    .to_owned(),
                byte: range.centre(),
                xy,
            })
        })
        .collect()
}

impl FixtureType {
    /// This type's colour wheel in one of its modes, if it has one.
    ///
    /// Empty for a mixing fixture, which has no wheel, and for a wheel
    /// whose ranges nobody has written down yet — the second is a gap in
    /// the research and shows up as a mover that cannot be given a
    /// colour, which is exactly what it is.
    #[must_use]
    pub fn color_wheel(&self, mode: &str) -> Vec<Slot> {
        self.wheel_of(mode, Known::ColorWheel)
    }

    /// This type's gobo wheel in one of its modes. Slot names only —
    /// the `xy` on each is meaningless for a gobo and is the open-white
    /// point.
    #[must_use]
    pub fn gobo_wheel(&self, mode: &str) -> Vec<Slot> {
        self.wheel_of(mode, Known::Gobo)
    }

    fn wheel_of(&self, mode: &str, wanted: Known) -> Vec<Slot> {
        let (resolved, _) = expand::mode(mode, &self.modes);
        resolved
            .iter()
            .find(|channel| channel.function == Function::Known(wanted))
            .map(|channel| slots_of(&channel.ranges))
            .unwrap_or_default()
    }

    /// The emitter letters this type mixes with — `["R", "G", "B",
    /// "W"]` — for the colour solve.
    #[must_use]
    pub fn emitters(&self) -> Vec<Known> {
        self.optics
            .as_ref()
            .map(|optics| {
                optics
                    .emitters
                    .iter()
                    .filter_map(|letter| {
                        crate::attribute::letter(letter).or_else(|| match resolve(letter) {
                            Function::Known(known) => Some(known),
                            Function::Unknown(_) => None,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::named;
    use crate::document::FixtureType;

    fn ty30() -> FixtureType {
        let text = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/fixtures/mini-gobo-moving-head-ty30.json"),
        )
        .expect("the TY-30 document ships with the repo");
        serde_json::from_str(&text).expect("it parses")
    }

    /// r[verify color.mix-or-wheel] - the wheel table comes from the chart
    #[test]
    fn a_wheels_slots_are_read_out_of_its_ranges() {
        // The TY-30's colour wheel steps eight colours in ranges of 16,
        // and `data/fixtures/README.md` records that the old
        // hand-written table had both the step size and the order
        // wrong. This is the fix, and the document is the authority.
        let slots = ty30().color_wheel("9ch");
        let names: Vec<&str> = slots.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "white",
                "red",
                "pale blue",
                "orange",
                "blue",
                "yellow",
                "green",
                "pink"
            ],
            "the manual's order, not a guess"
        );
        // Ranges of sixteen, so the centres are 7, 23, 39…
        assert_eq!(slots.first().map(|s| s.byte), Some(7));
        assert_eq!(slots.get(1).map(|s| s.byte), Some(23));
    }

    #[test]
    fn rotation_is_not_a_slot() {
        // Above 128 the same channel carries split colours and then
        // continuous rotation. A cue landing there puts the wheel
        // between two gels or spinning, which is not a colour.
        let slots = ty30().color_wheel("9ch");
        assert_eq!(slots.len(), 8, "eight positions, not the whole channel");
        assert!(slots.iter().all(|s| s.byte < 128), "{slots:?}");
    }

    #[test]
    fn a_mixing_fixture_has_no_wheel() {
        let doc: FixtureType = serde_json::from_str(
            r#"{"console_name": "Par", "modes": {"4ch": [
                 {"channel": 1, "attribute": "Dimmer"},
                 {"channel": 2, "attribute": "Red"},
                 {"channel": 3, "attribute": "Green"},
                 {"channel": 4, "attribute": "Blue"}]}}"#,
        )
        .unwrap();
        assert!(doc.color_wheel("4ch").is_empty());
    }

    #[test]
    fn the_longest_colour_name_in_the_prose_wins() {
        // `pale blue / cyan` is pale blue, not blue; `warm white` is not
        // white. Getting this backwards puts a cue on the wrong gel.
        assert_eq!(named("pale blue / cyan").map(|(n, _)| n), Some("pale blue"));
        assert_eq!(named("Warm White").map(|(n, _)| n), Some("warm white"));
        assert_eq!(named("Deep Red").map(|(n, _)| n), Some("red"));
        assert_eq!(named("gobo 3"), None);
    }
}
