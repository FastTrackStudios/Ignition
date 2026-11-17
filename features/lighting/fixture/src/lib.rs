//! Fixture types as data.
//!
//! What a fixture *is* — its channels, their order, what each byte
//! means, what it weighs, how wide its beam is — lives in a JSON
//! document under `data/fixtures/`, one per model, and this crate is
//! how that document is read, resolved and turned into the channel map
//! the engine addresses bytes through.
//!
//! ## Why this is a crate and not a table
//!
//! It used to be a table: `crates/ignition-viz/src/channel_map.rs`, a
//! `match` on manufacturer and model returning a hand-written channel
//! list. That has two failure modes and this project has hit both.
//! Adding a fixture type meant editing Rust and recompiling, so a room
//! could not be patched by the person standing in it. And a model with
//! no arm in the match had no channel map at all, so it never lit —
//! twenty-four of Room 138's forty fixtures were dark for exactly that
//! reason. Data has neither problem: an unknown model is a file someone
//! can write at the desk, and a wrong channel order is a line someone
//! can correct between songs.
//!
//! ## The shape
//!
//! - [`document`] — the document itself, fitted to the files that
//!   already exist. Unknown keys survive a round trip; nothing panics on
//!   bad data.
//! - [`attribute`] — a channel's prose name (`Warm White 3`, `Dimmer
//!   (per LED section)`) resolved to what the engine addresses.
//! - [`expand`] — the compressed chart forms a manual uses, flattened to
//!   one entry per DMX channel.
//! - [`library`] — the whole `data/fixtures/` directory, indexed, and
//!   the one place a venue's model string is matched to a type.
//!
//! ## What it is not
//!
//! It is not geometry. What a fixture *looks like* stays with GDTF and
//! `crates/ignition-viz/src/gdtf_geometry.rs` — a fixture type here with
//! no GDTF profile still patches and still outputs; it draws as a box.
//! The two are matched on the same name (`r[viz.gdtf-generated]`) and
//! that is the whole of their coupling.

// r[impl patch.type-not-geometry] - what a fixture looks like stays with GDTF

pub mod attribute;
pub mod document;
pub mod expand;
pub mod library;
pub mod wheel;

pub use attribute::{Function, Known, resolve};
pub use document::{
    Channel, ChannelRef, Confidence, FixtureType, Mode, Movement, Optics, Physical, Range, Wheels,
};
pub use expand::{Complaint, Resolved};
pub use library::Library;
pub use wheel::Slot;

use ignition_proto::ChannelMap;

impl FixtureType {
    /// The channel map for one of this type's modes — the thing the
    /// encoder addresses bytes through.
    ///
    /// Offsets are 0-based from the fixture's start address, which is
    /// what [`ignition_proto::ChannelMap`] means by an offset; the
    /// document's channel numbers are 1-based, which is what a manual
    /// and a console both mean by a channel. The conversion happens
    /// here and nowhere else.
    ///
    /// The footprint is the highest channel the chart mentions, not the
    /// number of lines in it: a chart that describes channels 1, 2 and 8
    /// is a fixture that occupies eight, and calling it three would let
    /// the next fixture patch on top of it.
    ///
    /// Returns the map with whatever [`expand`] could not read, so a
    /// caller can show the gaps without having to re-walk the document.
    #[must_use]
    pub fn channel_map(&self, mode: &str) -> (ChannelMap, Vec<Complaint>) {
        let (resolved, complaints) = expand::mode(mode, &self.modes);
        let footprint = resolved.iter().map(|r| r.number).max().unwrap_or(0);
        let channels: Vec<(u16, ignition_proto::Attribute)> = resolved
            .iter()
            .filter_map(|r| {
                // A 1-based channel of 0 is a document error, not an
                // offset of -1.
                let offset = r.number.checked_sub(1)?;
                Some((offset, r.function.attribute()))
            })
            .collect();
        // Per-attribute output curves (`r[files.venue.dmx-curves]`).
        // Keyed by attribute rather than by offset because that is what
        // the encoder looks a curve up by, and because two channels
        // driving one attribute — a coarse and its fine — share a law.
        // r[impl patch.curves] - authored on the fixture type, applied at output
        let curves = resolved
            .iter()
            .filter_map(|r| Some((r.function.attribute(), r.curve.clone()?)))
            .collect();
        let mut map = ChannelMap::new(footprint, channels);
        map.curves = curves;
        (map, complaints)
    }

    /// Where every channel of a mode rests when nothing is driving it.
    ///
    /// A shutter that rests closed is a fixture that never lights however
    /// much dimmer is asked of it, so a document's `default` is not a
    /// nicety — it is the difference between a working fixture and one
    /// that looks broken. Keyed by 0-based offset, like the channel map.
    #[must_use]
    pub fn rest_defaults(&self, mode: &str) -> Vec<(u16, u8)> {
        let (resolved, _) = expand::mode(mode, &self.modes);
        resolved
            .iter()
            .filter_map(|r| Some((r.number.checked_sub(1)?, r.default?)))
            .collect()
    }
}

impl FixtureType {
    /// Which mode a fixture is in, when nobody wrote it down.
    ///
    /// A venue records an address, not a mode — but the room recorded
    /// the mode anyway, in the gap it left before the next fixture. A
    /// rig patched at 1, 8, 15 is saying those fixtures are seven
    /// channels wide, and that is a measured fact about the rig rather
    /// than a guess about the model.
    ///
    /// So: the **widest** mode that still fits `gap`. Widest, because a
    /// fixture patched with room to spare is far more common than one
    /// deliberately run in a reduced mode, and because guessing narrow
    /// silently drops the fixture's last channels — on a mover, its
    /// dimmer. With no gap to go on (the last fixture in a universe),
    /// the widest mode outright.
    ///
    /// `None` only when the type has no modes at all.
    ///
    /// Prefer [`Self::mode_for`], which reads the venue's model string
    /// first: a room that wrote `Rockstrip 252 7ch` has *said* which
    /// mode it is in, and no amount of arithmetic over addresses beats
    /// being told.
    #[must_use]
    pub fn mode_for_gap(&self, gap: Option<u16>) -> Option<&str> {
        let width = |mode: &str| self.channel_map(mode).0.footprint;
        let fits = |mode: &str| {
            let footprint = width(mode);
            footprint > 0 && gap.is_none_or(|gap| footprint <= gap)
        };
        self.modes
            .keys()
            .filter(|mode| fits(mode))
            .max_by_key(|mode| width(mode))
            .or_else(|| self.modes.keys().max_by_key(|mode| width(mode)))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod mode_tests {
    use super::FixtureType;

    fn doc() -> FixtureType {
        // A Rockstrip: the same bar in three widths, which is exactly
        // the case where guessing wrong overruns the next fixture.
        serde_json::from_str(
            r#"{"console_name": "Bar", "modes": {
                 "3ch":  [{"channel": "1-3", "attribute": "Red/Green/Blue per section"}],
                 "7ch":  [{"channel": "1-7", "attribute": "Dimmer (per section)"}],
                 "24ch": [{"channel": "1-24", "attribute": "Red/Green/Blue per section"}]
               }}"#,
        )
        .unwrap()
    }

    /// r[verify patch.type-modes] - the room's own spacing picks the mode
    #[test]
    fn the_gap_the_room_left_decides_the_mode() {
        let doc = doc();
        // Seven channels before the next fixture: the 7ch mode, not the
        // 24ch one, which would run over its neighbour.
        assert_eq!(doc.mode_for_gap(Some(7)), Some("7ch"));
        // Room to spare: the widest that fits.
        assert_eq!(doc.mode_for_gap(Some(30)), Some("24ch"));
        // Tight: only the narrowest.
        assert_eq!(doc.mode_for_gap(Some(3)), Some("3ch"));
    }

    #[test]
    fn the_last_fixture_in_a_universe_gets_the_widest() {
        // Nothing after it to overrun, and a fixture run narrow loses
        // its last channels silently — on a mover, its dimmer.
        assert_eq!(doc().mode_for_gap(None), Some("24ch"));
    }

    #[test]
    fn a_gap_too_small_for_any_mode_still_yields_one() {
        // Refusing to pick would leave the fixture dark, which is worse
        // than a fixture that overlaps and is visibly wrong. The
        // conflict view is what reports it.
        assert_eq!(doc().mode_for_gap(Some(1)), Some("24ch"));
    }
}

/// The channel count a venue's model string names, if it names one.
///
/// Rooms write the mode into the model: `Rockstrip 252 7ch`, `Mini Gobo
/// Moving Head Light 11ch`, `RGBW Spot Light 6CH`. It is not a
/// convention anybody agreed on, it is just what people type, and it is
/// the most reliable statement of a fixture's mode there is — better
/// than any inference, because it is a person saying so.
#[must_use]
pub fn mode_hint(model: &str) -> Option<u16> {
    let lowered = model.to_ascii_lowercase();
    // Walk every `…ch` and take the digits immediately before it. The
    // last one wins: `Rockstrip 252 7ch` must be 7, not 252.
    let mut found = None;
    let bytes = lowered.as_bytes();
    for (index, window) in bytes.windows(2).enumerate() {
        if window != b"ch" {
            continue;
        }
        // `ch` must not be the start of a longer word — `chase`,
        // `channel` — so what follows is a boundary or nothing.
        let after = bytes.get(index.saturating_add(2));
        if after.is_some_and(u8::is_ascii_alphanumeric) {
            continue;
        }
        let digits: String = lowered
            .get(..index)
            .unwrap_or_default()
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if let Ok(count) = digits.parse::<u16>()
            && count > 0
        {
            found = Some(count);
        }
    }
    found
}

impl FixtureType {
    /// Which mode a patched fixture is in.
    ///
    /// In order of how much the answer can be trusted:
    ///
    /// 1. the **model string**, when it names a channel count — the room
    ///    said so, and a person saying so beats any inference;
    /// 2. the **gap** the room left before the next fixture
    ///    ([`Self::mode_for_gap`]);
    /// 3. the widest mode.
    ///
    /// Rule 1 is not a nicety. Norco patches a `Rockstrip 252 7ch` at
    /// universe 1 address 276 with the next fixture at 303 — twenty-seven
    /// channels of room — so the gap alone says 24ch, and a bar addressed
    /// as 24 channels wide when it is listening on 7 puts its dimmer
    /// somewhere nothing writes. That is a dark fixture, and it is what
    /// `tests/dmx_loopback.rs` caught.
    #[must_use]
    pub fn mode_for(&self, model: &str, gap: Option<u16>) -> Option<&str> {
        if let Some(count) = mode_hint(model) {
            // By name first — `7ch` — then by what the chart actually
            // occupies, since a mode may be named anything.
            let named = format!("{count}ch");
            if let Some(mode) = self
                .modes
                .keys()
                .find(|mode| mode.eq_ignore_ascii_case(&named))
            {
                return Some(mode);
            }
            if let Some(mode) = self
                .modes
                .keys()
                .find(|mode| self.channel_map(mode).0.footprint == count)
            {
                return Some(mode);
            }
        }
        self.mode_for_gap(gap)
    }
}

#[cfg(test)]
mod hint_tests {
    use super::{FixtureType, mode_hint};

    #[test]
    fn a_model_string_that_names_its_mode_is_read() {
        assert_eq!(mode_hint("Rockstrip 252 7ch"), Some(7));
        assert_eq!(mode_hint("Mini Gobo Moving Head Light 11ch"), Some(11));
        assert_eq!(mode_hint("RGBW Spot Light 6CH"), Some(6));
        assert_eq!(mode_hint("Uking Par"), None);
        // `ch` inside a word is not a mode.
        assert_eq!(mode_hint("2ch Chase Bar"), Some(2));
        assert_eq!(mode_hint("Chase Bar"), None);
    }

    /// r[verify patch.type-modes] - what the room wrote beats what it left
    #[test]
    fn the_model_string_beats_the_gap() {
        // The exact shape that broke the DMX loopback: a bar the room
        // named `7ch`, patched with twenty-seven channels of room after
        // it. The gap says 24ch; the room says 7ch; the room wins.
        let doc: FixtureType = serde_json::from_str(
            r#"{"console_name": "Bar", "modes": {
                 "7ch":  [{"channel": "1-7", "attribute": "Dimmer (per section)"}],
                 "24ch": [{"channel": "1-24", "attribute": "Red/Green/Blue per section"}]
               }}"#,
        )
        .unwrap();
        assert_eq!(doc.mode_for("Rockstrip 252 7ch", Some(27)), Some("7ch"));
        // With nothing in the name, the gap is all there is.
        assert_eq!(doc.mode_for("Rockstrip 252", Some(27)), Some("24ch"));
    }

    /// r[verify patch.curves] - a curve on the chart reaches the map
    #[test]
    fn a_channels_curve_travels_with_its_attribute() {
        // A shutter that is only open above 32 is corrected here rather
        // than by every cue that touches it.
        let doc: FixtureType = serde_json::from_str(
            r#"{"console_name": "T", "modes": {"2ch": [
                 {"channel": 1, "attribute": "Dimmer",
                  "curve": {"range": {"lo": 0, "hi": 200}}},
                 {"channel": 2, "attribute": "Strobe"}]}}"#,
        )
        .unwrap();
        let (map, complaints) = doc.channel_map("2ch");
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(
            map.curve_of(&ignition_proto::Attribute::Dimmer),
            &ignition_proto::Curve::Range { lo: 0, hi: 200 }
        );
        // Full sends 200, not 255.
        assert_eq!(
            map.curve_of(&ignition_proto::Attribute::Dimmer).apply(1.0),
            200
        );
        // And a channel with no curve is linear.
        assert_eq!(
            map.curve_of(&ignition_proto::Attribute::Strobe),
            &ignition_proto::Curve::Linear
        );
    }
}
