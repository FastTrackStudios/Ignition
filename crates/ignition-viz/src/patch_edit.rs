//! Applying a Setup-view edit to a venue.
//!
//! Kept here rather than in the studio's command drain for one reason:
//! this is the code that can lose somebody's rig, and it should be
//! testable without a window, a GPU or a running Bevy app. The studio's
//! job is to call [`apply`] and then decide what to do about the result;
//! everything about *what an edit means* is here.
//!
//! Two rules run through all of it:
//!
//! - **A fixture is named by its channel, never by its index.** An index
//!   is a position in a file that inserting a fixture changes, so a
//!   command that arrived one frame late would edit the wrong light.
//! - **Nothing is saved until it is asked for** (`r[patch.explicit-save]`).
//!   Patching is exploratory — an address is tried and abandoned — and a
//!   file that changes under every keystroke cannot be diffed or
//!   reverted.

use crate::venue::Venue;
use ignition_proto::DmxAddress;

/// What an edit did, for the surface to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The venue changed and has not been written yet.
    Changed,
    /// The edit asked for something the room does not have — a channel
    /// nobody is on, a type with no document, a universe with no room
    /// left. Not an error: the operator gets told and the venue is
    /// untouched.
    Refused(String),
    /// Nothing about the venue is different. A label set to what it
    /// already was.
    Unchanged,
    /// Some of the edit happened. Adding twelve pars to a universe with
    /// room for eleven leaves eleven patched, and saying "nothing
    /// happened" would be a lie the operator then has to discover.
    Partial(String),
}

impl Outcome {
    /// Whether this edit left the venue needing a save.
    #[must_use]
    pub const fn dirties(&self) -> bool {
        matches!(self, Self::Changed | Self::Partial(_))
    }

    /// What to tell the operator, if anything.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Refused(why) | Self::Partial(why) => Some(why),
            Self::Changed | Self::Unchanged => None,
        }
    }
}

/// One edit, applied.
///
/// Anything that changes an address, a model or the patched flag calls
/// [`Venue::repatch`] on the way out, because the resolved patch table
/// is cached and would otherwise go on addressing the room the way it
/// was before the edit.
// r[impl patch.writes-the-venue] - what an edit does to a room
pub fn apply(venue: &mut Venue, edit: &Edit) -> Outcome {
    match edit {
        Edit::Address {
            chan,
            universe,
            address,
        } => set_address(venue, *chan, *universe, *address),
        Edit::Unpatch { chan } => {
            let Some(fixture) = venue.by_chan_mut(*chan) else {
                return missing(*chan);
            };
            if fixture.dmx_address().is_none() {
                return Outcome::Unchanged;
            }
            fixture.set_address(None);
            venue.repatch();
            Outcome::Changed
        }
        Edit::Label { chan, label } => {
            let Some(fixture) = venue.by_chan_mut(*chan) else {
                return missing(*chan);
            };
            if fixture.label == *label {
                return Outcome::Unchanged;
            }
            fixture.label.clone_from(label);
            // A label is not on the wire, so the patch stands.
            Outcome::Changed
        }
        Edit::Gel { chan, gel } => {
            let Some(fixture) = venue.by_chan_mut(*chan) else {
                return missing(*chan);
            };
            if fixture.gel == *gel {
                return Outcome::Unchanged;
            }
            fixture.gel.clone_from(gel);
            Outcome::Changed
        }
        Edit::Remove { chan } => {
            let before = venue.fixtures.len();
            venue.fixtures.retain(|f| f.chan != Some(*chan));
            if venue.fixtures.len() == before {
                return missing(*chan);
            }
            venue.repatch();
            Outcome::Changed
        }
        Edit::Insert(insert) => self::insert(venue, insert),
    }
}

/// The edits, as this crate models them.
///
/// A near-copy of `ignition_live_ui::PatchEdit` on purpose: that one is
/// a wire type carrying a `Save` variant, which is a thing the *host*
/// does, not a thing that happens to a venue. Translating at the seam
/// keeps "what an edit means" free of "how an edit arrived".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    Address {
        chan: u32,
        universe: u16,
        address: u16,
    },
    Unpatch {
        chan: u32,
    },
    Label {
        chan: u32,
        label: String,
    },
    Gel {
        chan: u32,
        gel: String,
    },
    Remove {
        chan: u32,
    },
    Insert(Insert),
}

/// Adding fixtures to a room (`r[patch.insert]`).
///
/// Rigs come in bars of eight and patching them one at a time is the
/// thirty minutes `r[profile.setup-cost-is-the-metric]` is measured in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Insert {
    /// The model string the new fixtures carry — what resolves them to
    /// a fixture type.
    pub fixture_type: String,
    pub count: u16,
    /// The channel the first one takes; the rest follow. Zero means "the
    /// next free channel", which is what adding to a rig usually wants.
    pub chan: u32,
    pub universe: u16,
    /// Zero means "the next free address in that universe".
    pub address: u16,
    /// Channels between one fixture's address and the next. Zero means
    /// the type's own footprint, which is what packing a bar wants.
    pub offset: u16,
}

fn missing(chan: u32) -> Outcome {
    Outcome::Refused(format!("nothing is on channel {chan}"))
}

fn set_address(venue: &mut Venue, chan: u32, universe: u16, address: u16) -> Outcome {
    if universe == 0 {
        return Outcome::Refused("universes are numbered from 1".to_owned());
    }
    if address == 0 || address > 512 {
        return Outcome::Refused(format!("{address} is not a DMX address"));
    }
    let wanted = DmxAddress {
        universe,
        start_channel: address,
    };
    let Some(fixture) = venue.by_chan_mut(chan) else {
        return missing(chan);
    };
    if fixture.dmx_address() == Some(wanted) {
        return Outcome::Unchanged;
    }
    fixture.set_address(Some(wanted));
    venue.repatch();
    // Deliberately not refused when it overlaps something. A patch is
    // often briefly wrong on the way to being right — moving three
    // fixtures up by seven means passing through a clash — and the
    // conflict view is what reports it (`r[patch.conflict]`).
    Outcome::Changed
}

fn insert(venue: &mut Venue, insert: &Insert) -> Outcome {
    if insert.count == 0 {
        return Outcome::Unchanged;
    }
    if insert.fixture_type.trim().is_empty() {
        return Outcome::Refused("a fixture needs a type".to_owned());
    }
    let universe = if insert.universe == 0 {
        1
    } else {
        insert.universe
    };

    // How wide one of these is, so the default offset packs them and
    // "next free" leaves room. A type with no document is not refused —
    // somebody may be patching a fixture whose profile they are about to
    // write — but it takes one channel until it has one.
    let footprint = crate::fixture_library::profile_for("", &insert.fixture_type, None)
        .map_or(1, |found| found.profile.map.footprint.max(1));
    let offset = if insert.offset == 0 {
        footprint
    } else {
        insert.offset
    };

    let mut chan = if insert.chan == 0 {
        venue.next_free_chan()
    } else {
        insert.chan
    };
    let mut address = if insert.address == 0 {
        let Some(free) = venue.next_free_address(universe, footprint) else {
            return Outcome::Refused(format!(
                "universe {universe} has no run of {footprint} free channels"
            ));
        };
        free
    } else {
        insert.address
    };

    // Somewhere to put them: the room's own extent, so a new fixture is
    // visible rather than at the origin under the stage. Height comes
    // from the top of the room; they line up along X at the back.
    let (min, max) = venue.bounds();
    let mut placed = 0_u16;
    for step in 0..insert.count {
        while venue.fixtures.iter().any(|f| f.chan == Some(chan)) {
            chan = chan.saturating_add(1);
        }
        let across = f32::from(step).mul_add(0.5, min.x);
        venue.fixtures.push(crate::venue::FixtureRecord {
            chan: Some(chan),
            name: format!("Chan {chan}"),
            tags: Vec::new(),
            patched: true,
            manufacturer: None,
            model: Some(insert.fixture_type.clone()),
            position: crate::venue::Vec3 {
                x: across.min(max.x),
                y: max.y,
                z: max.z,
            },
            eulers: crate::venue::Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            quat: crate::venue::Quat {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            size: crate::venue::Vec3 {
                x: 0.3,
                y: 0.3,
                z: 0.3,
            },
            universe: None,
            address: None,
            beam_angle_deg: None,
            mirrors: Vec::new(),
            label: String::new(),
            gel: String::new(),
            global_address: None,
            extra: std::collections::BTreeMap::new(),
        });
        // Through `set_address`, so `global_address` and `patched`
        // follow — the three fields an address is made of.
        if let Some(fixture) = venue.by_chan_mut(chan) {
            fixture.set_address(Some(DmxAddress {
                universe,
                start_channel: address,
            }));
        }
        placed = placed.saturating_add(1);
        chan = chan.saturating_add(1);
        // Past the end of a universe, the rest go unpatched rather than
        // wrapping onto slot 1, which would silently put a mover's pan
        // on a par.
        let next = address.saturating_add(offset);
        if next > 512 {
            break;
        }
        address = next;
    }
    venue.repatch();
    if placed < insert.count {
        // The ones that fit are patched and are worth keeping — telling
        // an operator "nothing happened" after adding eleven of twelve
        // pars would be a lie they then have to discover.
        return Outcome::Partial(format!(
            "patched {placed} of {}; universe {universe} ran out of channels",
            insert.count
        ));
    }
    Outcome::Changed
}

#[cfg(test)]
mod tests {
    use super::{Edit, Insert, Outcome, apply};
    use crate::venue::Venue;

    fn norco() -> Option<Venue> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/venues/norco");
        Venue::load(dir).ok()
    }

    /// r[verify patch.writes-the-venue] - a repatch reaches the wire
    ///
    /// The cached patch table is the trap here: moving a fixture and
    /// forgetting to invalidate it leaves the room addressed the way it
    /// was, and everything would look right until the light came on.
    #[test]
    fn moving_a_fixture_changes_where_its_bytes_go() {
        let Some(mut venue) = norco() else {
            return;
        };
        let chan = venue.fixtures.first().and_then(|f| f.chan).unwrap_or(1);
        let before = venue.patch().by_chan(chan).map(|p| p.address);
        assert!(before.is_some(), "the fixture starts patched");

        let outcome = apply(
            &mut venue,
            &Edit::Address {
                chan,
                universe: 4,
                address: 300,
            },
        );
        assert_eq!(outcome, Outcome::Changed);

        let after = venue.patch().by_chan(chan).map(|p| p.address);
        assert_ne!(before, after, "the patch table was not re-resolved");
        assert_eq!(after.map(|a| (a.universe, a.start_channel)), Some((4, 300)));
    }

    #[test]
    fn an_edit_to_a_channel_nobody_is_on_is_refused_not_ignored() {
        let Some(mut venue) = norco() else {
            return;
        };
        let outcome = apply(
            &mut venue,
            &Edit::Label {
                chan: 99_999,
                label: "nowhere".to_owned(),
            },
        );
        assert!(matches!(outcome, Outcome::Refused(_)));
        assert!(outcome.message().is_some_and(|m| m.contains("99999")));
        assert!(!outcome.dirties(), "a refusal does not dirty the venue");
    }

    #[test]
    fn a_nonsense_address_is_refused_before_it_reaches_the_venue() {
        let Some(mut venue) = norco() else {
            return;
        };
        let chan = venue.fixtures.first().and_then(|f| f.chan).unwrap_or(1);
        let before = venue.fixtures.clone();
        for (universe, address) in [(0, 1), (1, 0), (1, 513)] {
            let outcome = apply(
                &mut venue,
                &Edit::Address {
                    chan,
                    universe,
                    address,
                },
            );
            assert!(
                matches!(outcome, Outcome::Refused(_)),
                "{universe}.{address} was accepted"
            );
        }
        assert_eq!(before, venue.fixtures, "a refused edit changed the venue");
    }

    /// r[verify patch.insert] - a bar of eight, in one action
    #[test]
    fn inserting_packs_fixtures_at_their_own_footprint() {
        let Some(mut venue) = norco() else {
            return;
        };
        let before = venue.fixtures.len();
        let outcome = apply(
            &mut venue,
            &Edit::Insert(Insert {
                // Eight of one type should land exactly that type's
                // footprint apart — packed, with no gaps and, more to
                // the point, no overlaps.
                fixture_type: "Uking Par".to_owned(),
                count: 8,
                chan: 0,
                universe: 4,
                address: 1,
                offset: 0,
            }),
        );
        assert_eq!(outcome, Outcome::Changed, "{:?}", outcome.message());
        assert_eq!(venue.fixtures.len(), before + 8);

        let added: Vec<u16> = venue
            .fixtures
            .iter()
            .filter(|f| f.universe == Some(4))
            .filter_map(|f| f.address)
            .collect();
        assert_eq!(added.len(), 8);
        // Exactly the footprint the type resolved to. Hard-coding the
        // number here would be asserting a fact about one document
        // rather than about the packing; what matters is that the gap
        // *is* the width, because anything less overlaps and anything
        // more wastes a universe.
        let width = venue
            .fixtures
            .iter()
            .position(|f| f.universe == Some(4))
            .and_then(|index| venue.patch().get(index).map(|p| p.map.footprint))
            .expect("the new fixtures resolved to a type");
        assert!(width > 1, "a par is not one channel wide");
        let mut sorted = added;
        sorted.sort_unstable();
        for pair in sorted.windows(2) {
            let [a, b] = pair else { continue };
            assert_eq!(
                b - a,
                width,
                "not packed at the type's own footprint of {width}: {sorted:?}"
            );
        }
        // And every one of them got a channel of its own.
        let chans: Vec<u32> = venue.fixtures.iter().filter_map(|f| f.chan).collect();
        let mut unique = chans.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(chans.len(), unique.len(), "two fixtures share a channel");
    }

    #[test]
    fn inserting_past_the_end_of_a_universe_keeps_what_fitted() {
        let Some(mut venue) = norco() else {
            return;
        };
        let before = venue.fixtures.len();
        // Two hundred seven-channel pars do not fit in 512 slots.
        let outcome = apply(
            &mut venue,
            &Edit::Insert(Insert {
                fixture_type: "Uking Par".to_owned(),
                count: 200,
                chan: 0,
                universe: 4,
                address: 1,
                offset: 0,
            }),
        );
        let Outcome::Partial(why) = &outcome else {
            panic!("expected a partial insert, got {outcome:?}");
        };
        assert!(why.contains("ran out"), "{why}");
        assert!(outcome.dirties(), "what fitted is worth keeping");
        assert!(venue.fixtures.len() > before, "nothing was added");
        // Nothing wrapped onto slot 1 of the same universe, which would
        // silently put one fixture's channels on another's.
        for fixture in venue.fixtures.iter().filter(|f| f.universe == Some(4)) {
            assert!(fixture.address.is_some_and(|a| (1..=512).contains(&a)));
        }
    }

    /// r[verify patch.unpatched] - removed from the wire, not the room
    #[test]
    fn unpatching_then_repatching_puts_a_fixture_back() {
        let Some(mut venue) = norco() else {
            return;
        };
        let chan = venue.fixtures.first().and_then(|f| f.chan).unwrap_or(1);
        assert_eq!(apply(&mut venue, &Edit::Unpatch { chan }), Outcome::Changed);
        assert!(venue.patch().by_chan(chan).is_none(), "still on the wire");
        // A second unpatch is not a change.
        assert_eq!(
            apply(&mut venue, &Edit::Unpatch { chan }),
            Outcome::Unchanged
        );
        assert_eq!(
            apply(
                &mut venue,
                &Edit::Address {
                    chan,
                    universe: 1,
                    address: 1,
                },
            ),
            Outcome::Changed
        );
        assert!(venue.patch().by_chan(chan).is_some(), "did not come back");
    }

    #[test]
    fn setting_a_label_to_what_it_already_is_is_not_a_change() {
        // Otherwise every keystroke in the sheet marks the venue dirty
        // and the SAVE key never goes out.
        let Some(mut venue) = norco() else {
            return;
        };
        let chan = venue.fixtures.first().and_then(|f| f.chan).unwrap_or(1);
        let edit = Edit::Label {
            chan,
            label: "SL truss 3".to_owned(),
        };
        assert_eq!(apply(&mut venue, &edit), Outcome::Changed);
        assert_eq!(apply(&mut venue, &edit), Outcome::Unchanged);
    }
}
