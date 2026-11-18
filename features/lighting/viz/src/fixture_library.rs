//! The bridge from a fixture *document* to the output stage's profile.
//!
//! `ignition-fixture` reads `data/fixtures/*.json` and knows what a
//! fixture's channels are; `fixture_profile.rs` knows what the encoder
//! needs. This is the fifty lines between them, and the place the
//! project's DMX truth moved to.
//!
//! ## Why there is still a fallback
//!
//! `channel_map.rs` was the authority, and it says so in its own
//! comments: a model with no arm in that `match` had no channel map,
//! and a fixture with no channel map never lights — twenty-four of Room
//! 138's forty went dark for exactly that reason. Swapping an authority
//! for a directory of files is the kind of change that repeats that
//! failure quietly, so it is not a swap: the library is asked first and
//! the table answers for anything the library has no document for.
//!
//! `crates/ignition-fixture/tests/library_covers_the_venues.rs` is what
//! decides when the fallback can go. It asserts that every patched
//! fixture in every shipped venue resolves to a document with a mode
//! narrow enough to fit the address spacing the room itself left. While
//! that is green the fallback is dead code for the shipped venues, and
//! it stays anyway for the venue nobody has written a document for yet.

use crate::fixture_profile::{ColorWheelSlot, FixtureProfile};
use ignition_fixture::Library;

/// What resolving a patched fixture's model string produced.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// How the encoder addresses this fixture's bytes.
    pub profile: FixtureProfile,
    /// The fixture type's console name — what a patch sheet should show
    /// in its type column. Empty when the hand-written table answered,
    /// which has no notion of a named type.
    pub fixture_type: String,
    /// The mode chosen, per `r[patch.type-modes]`. `legacy` when the
    /// table answered.
    pub mode: String,
    /// `manual`, `listing` or `guess` — how the chart was come by
    /// (`r[patch.type-confidence]`). Empty for the table, whose
    /// confidence nobody recorded per model.
    pub confidence: String,
}

/// The library, read once. A missing `data/fixtures` is an empty
/// library and the fallback carries everything — the desk still opens.
fn library() -> &'static Library {
    static LIBRARY: std::sync::OnceLock<Library> = std::sync::OnceLock::new();
    LIBRARY.get_or_init(|| {
        let library = Library::load_default();
        for rejected in library.rejected() {
            tracing::warn!(
                path = %rejected.path.display(),
                message = %rejected.message,
                "a fixture document would not load; fixtures of that type will fall back"
            );
        }
        tracing::info!(types = library.types().len(), "fixture library loaded");
        library
    })
}

/// The output profile for a patched fixture.
///
/// `gap` is how many channels the room left before the next fixture in
/// the same universe — the venue's own statement of which mode this
/// fixture is in, since nothing records the mode directly. `None` for
/// the last fixture in a universe. See
/// [`ignition_fixture::FixtureType::mode_for_gap`].
///
/// Returns the profile with the type, mode and confidence it resolved
/// to, so a patch sheet can show what was decided rather than leaving
/// the operator to guess at it.
#[must_use]
pub fn profile_for(manufacturer: &str, model: &str, gap: Option<u16>) -> Option<Resolved> {
    if let Some(doc) = library().find(manufacturer, model)
        && let Some(mode) = doc.mode_for(model, gap)
    {
        {
            let (map, complaints) = doc.channel_map(mode);
            for complaint in &complaints {
                tracing::warn!(
                    fixture = %doc.console_name,
                    mode = %complaint.mode,
                    channel = %complaint.channel,
                    message = %complaint.message,
                    "a line of this fixture's chart could not be read"
                );
            }
            if map.footprint > 0 {
                let wheel: Vec<ColorWheelSlot> = doc
                    .color_wheel(mode)
                    .into_iter()
                    .map(|slot| ColorWheelSlot::xy(&slot.name, slot.byte, slot.xy.0, slot.xy.1))
                    .collect();
                return Some(Resolved {
                    profile: FixtureProfile::from_channel_map(map).with_wheel(wheel),
                    fixture_type: doc.console_name.clone(),
                    mode: mode.to_owned(),
                    confidence: doc.confidence.badge().to_owned(),
                });
            }
        }
    }
    // No document, or a document whose chart yields nothing addressable.
    // The hand-written table is what keeps the fixture lit.
    crate::channel_map::profile_for(manufacturer, model).map(|profile| Resolved {
        profile,
        fixture_type: String::new(),
        mode: "legacy".to_owned(),
        confidence: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::profile_for;
    use ignition_proto::{Attribute, ColorChannel};

    /// The document, not the table, is what answers now.
    ///
    /// `data/fixtures/README.md` recorded six places where the two
    /// disagreed and the table was the one that had drifted. This is
    /// one of them: the TY-30's colour wheel steps eight colours in
    /// ranges of *sixteen*, in the manual's order — the table had
    /// eight-value steps and a different order, so every colour a cue
    /// asked of a mini gobo head landed on the wrong gel.
    #[test]
    fn the_document_supplies_the_wheel_the_table_had_wrong() {
        let Some(found) = profile_for("Riukoe", "Mini Gobo Moving Head Light", Some(11)) else {
            panic!("the mini gobo head resolves to something");
        };
        assert_ne!(found.mode, "legacy", "the document answered, not the table");
        assert!(!found.fixture_type.is_empty(), "and it named its type");
        let slots = found.profile.wheel_slots();
        assert_eq!(slots.len(), 8, "eight positions on the wheel");
        // Ranges of sixteen put the first two centres at 7 and 23. The
        // old table said 3 and 11.
        assert_eq!(slots.first().map(|(byte, _)| *byte), Some(7));
        assert_eq!(slots.get(1).map(|(byte, _)| *byte), Some(23));
    }

    /// A model nobody has written a document for still lights.
    #[test]
    fn an_undocumented_model_falls_through_to_the_table() {
        // Whatever the table has and the library does not, the answer
        // must not be `None` — that is the shape of the bug that put
        // twenty-four of Room 138's forty fixtures in the dark.
        let documented = profile_for("Uking", "Par", Some(8));
        assert!(documented.is_some(), "a documented model resolves");
        assert!(
            profile_for("Nobody", "No Such Fixture At All", None).is_none(),
            "and a model in neither is honestly reported as unknown"
        );
    }

    /// The room's spacing picks the mode, and the mode picks the width.
    #[test]
    fn the_gap_the_room_left_decides_the_footprint() {
        let Some(wide) = profile_for("Rockville", "RockStrip 252", Some(28)) else {
            panic!("the Rockstrip resolves");
        };
        let Some(narrow) = profile_for("Rockville", "RockStrip 252", Some(7)) else {
            panic!("the Rockstrip resolves at seven too");
        };
        let (wide, narrow) = (wide.profile, narrow.profile);
        assert!(
            wide.map.footprint > narrow.map.footprint,
            "a bar patched with 28 channels of room is not the same fixture as one with 7 \
             (got {} and {})",
            wide.map.footprint,
            narrow.map.footprint
        );
        assert!(narrow.map.footprint <= 7, "and the narrow one fits its gap");
    }

    /// A mixing par comes back with emitters rather than a wheel.
    #[test]
    fn a_par_mixes_and_a_mover_wheels() {
        let Some(par) = profile_for("Uking", "Par", Some(8)) else {
            panic!("the par resolves");
        };
        let par = par.profile;
        assert!(par.wheel.is_empty(), "a mixing par has no wheel");
        assert!(
            par.map.channels.iter().any(|(_, attr)| matches!(
                attr,
                Attribute::ColorAdd {
                    channel: ColorChannel::Red
                }
            )),
            "and it has a red channel to mix with"
        );
    }
}
