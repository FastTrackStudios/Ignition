//! The patch, flattened for a surface to draw.
//!
//! This crate cannot see a `Venue` — it does not depend on
//! `ignition-viz`, deliberately, because the same components run in a
//! browser on an iPad where there is no Bevy world to hold one. So the
//! patch arrives the way [`crate::Surface`] does: resolved once by the
//! host into plain serialisable rows, and **republished after every
//! edit** rather than re-read from disk by the pane. Keeping a second
//! copy of venue data next to the one the engine is using is the failure
//! mode `cameras.rs` already calls out by name.
//!
//! One thing here is not flattening: [`Occupancy`]. Where a fixture
//! fits, and what it would collide with, is arithmetic over intervals —
//! not a fact the engine knows better than the surface does — so it is
//! computed here, from the rows, and unit-tested without a window.

use serde::{Deserialize, Serialize};

/// A DMX universe number as the venue writes it, 1-based.
pub type Universe = u16;

/// How wide a universe is. 512 slots, 1-based, as everything in lighting
/// counts them.
pub const SLOTS: u16 = 512;

/// One fixture on the patch sheet.
///
/// The columns are the ones consoles settled on decades ago — channel,
/// name, type, address — plus **label** and **gel**, which the venue
/// files have carried since they were written and which nothing has ever
/// displayed (`r[patch.sheet]`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PatchRow {
    /// The fixture's channel number: its identity everywhere above the
    /// wire, 1-based, matching the Eos and QLC+ convention.
    pub chan: u32,
    pub name: String,
    /// The operator's own word for this fixture — "SL truss 3", "the
    /// one behind the drum riser". Distinct from `name`, which is the
    /// venue's.
    pub label: String,
    /// The gel in front of it, if it is a conventional.
    pub gel: String,
    pub manufacturer: String,
    pub model: String,
    /// Which of the fixture type's modes this fixture is in.
    pub mode: String,
    /// The type this model resolved to, or empty if nothing matched —
    /// which is the single most useful thing a patch sheet can say,
    /// because a fixture with no type never lights.
    pub fixture_type: String,
    /// How the type's facts were come by — `manual`, `listing`, `guess`
    /// (`r[patch.type-confidence]`).
    pub confidence: String,
    pub universe: Universe,
    /// 1-based start channel within the universe.
    pub address: u16,
    /// How many consecutive channels this fixture occupies.
    pub footprint: u16,
    /// False for a prop, a spare, or a fixture whose node has not
    /// arrived: in the room, in groups, with no address
    /// (`r[patch.unpatched]`).
    pub patched: bool,
    /// Further addresses this one fixture also drives
    /// (`r[patch.multipatch]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mirrors: Vec<(Universe, u16)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Metres in the venue's stage space, for the sheet to say where a
    /// row is without asking the engine.
    pub position: [f32; 3],
    /// True while a venue-local layer is overriding this fixture
    /// (`r[patch.venue-layer.visible]`).
    #[serde(default)]
    pub overridden: bool,
}

impl PatchRow {
    /// The half-open channel span this fixture occupies, as 1-based
    /// slots. `None` for an unpatched fixture or one with no footprint —
    /// neither occupies anything, and neither can collide.
    #[must_use]
    pub const fn span(&self) -> Option<(u16, u16)> {
        if !self.patched || self.footprint == 0 || self.address == 0 {
            return None;
        }
        Some((self.address, self.address.saturating_add(self.footprint)))
    }

    /// `1.001` — the way an address is written on a sheet and spoken at
    /// a rig.
    #[must_use]
    pub fn address_label(&self) -> String {
        if !self.patched || self.address == 0 {
            return "—".to_owned();
        }
        format!("{}.{:03}", self.universe, self.address)
    }
}

/// Why two rows cannot both be right.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflict {
    /// The other fixture's channel.
    pub with: u32,
    /// The first slot the two share, 1-based, for the message.
    pub at: u16,
    pub universe: Universe,
}

/// What occupies a universe, and what does not.
///
/// The occupancy view answers "where does this fit", which is a spatial
/// question that a list of addresses answers badly
/// (`r[patch.occupancy]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Occupancy {
    pub universe: Universe,
    /// One entry per slot, 1-based: the channel number occupying it, or
    /// 0 for free. A `Vec` rather than a map because the pane draws all
    /// 512 in order and a map would be walked to do it.
    pub slots: Vec<u32>,
    /// How many of the 512 are taken.
    pub used: u16,
}

impl Occupancy {
    /// The longest run of free slots, and where it starts — what "next
    /// free address" is chosen from, and what the pane labels the gap
    /// with.
    #[must_use]
    pub fn largest_gap(&self) -> Option<(u16, u16)> {
        let mut best: Option<(u16, u16)> = None;
        let mut run_start = 0_u16;
        let mut run = 0_u16;
        for (index, occupant) in self.slots.iter().enumerate() {
            let slot = u16::try_from(index).unwrap_or(u16::MAX).saturating_add(1);
            if *occupant == 0 {
                if run == 0 {
                    run_start = slot;
                }
                run = run.saturating_add(1);
                if best.is_none_or(|(_, length)| run > length) {
                    best = Some((run_start, run));
                }
            } else {
                run = 0;
            }
        }
        best
    }
}

/// The whole patch, as the surface sees it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PatchSheet {
    /// Rows in channel order, which is how a patch sheet is always read.
    pub rows: Vec<PatchRow>,
    /// Which universes the venue configures for output, in order.
    pub universes: Vec<Universe>,
    /// The venue directory this came from, for the title bar.
    pub venue: String,
    /// True when there are edits the venue file has not been told about
    /// (`r[patch.explicit-save]`).
    pub dirty: bool,
}

impl PatchSheet {
    /// Every conflict, by channel.
    ///
    /// Two fixtures whose spans overlap within one universe are both
    /// reported, because from the sheet's point of view neither is the
    /// guilty one — the operator decides which moves
    /// (`r[patch.conflict]`).
    #[must_use]
    pub fn conflicts(&self) -> Vec<(u32, Conflict)> {
        let mut out = Vec::new();
        for (index, row) in self.rows.iter().enumerate() {
            let Some((start, end)) = row.span() else {
                continue;
            };
            for other in self.rows.iter().skip(index.saturating_add(1)) {
                if other.universe != row.universe {
                    continue;
                }
                let Some((other_start, other_end)) = other.span() else {
                    continue;
                };
                // Multipatch — two fixtures deliberately on one address
                // — is not a conflict (`r[patch.multipatch]`); an
                // overlap that is not an exact shared start is.
                if other_start == start {
                    continue;
                }
                if start < other_end && other_start < end {
                    let at = start.max(other_start);
                    out.push((
                        row.chan,
                        Conflict {
                            with: other.chan,
                            at,
                            universe: row.universe,
                        },
                    ));
                    out.push((
                        other.chan,
                        Conflict {
                            with: row.chan,
                            at,
                            universe: row.universe,
                        },
                    ));
                }
            }
        }
        out
    }

    /// What occupies each universe. Universes the venue configures but
    /// nothing is patched into still appear, empty, because an empty
    /// universe is where the next fixture goes.
    #[must_use]
    pub fn occupancy(&self) -> Vec<Occupancy> {
        let mut universes: Vec<Universe> = self.universes.clone();
        for row in &self.rows {
            if row.span().is_some() && !universes.contains(&row.universe) {
                universes.push(row.universe);
            }
        }
        universes.sort_unstable();
        universes.dedup();
        universes
            .into_iter()
            .map(|universe| {
                let mut slots = vec![0_u32; usize::from(SLOTS)];
                let mut used = 0_u16;
                for row in self.rows.iter().filter(|r| r.universe == universe) {
                    let Some((start, end)) = row.span() else {
                        continue;
                    };
                    for slot in start..end.min(SLOTS.saturating_add(1)) {
                        let Some(cell) = slots.get_mut(usize::from(slot.saturating_sub(1))) else {
                            continue;
                        };
                        if *cell == 0 {
                            used = used.saturating_add(1);
                        }
                        *cell = row.chan;
                    }
                }
                Occupancy {
                    universe,
                    slots,
                    used,
                }
            })
            .collect()
    }

    /// The lowest address in `universe` with `footprint` free slots
    /// after it — the console's "patch to next free address"
    /// (`r[patch.address]`).
    ///
    /// `None` when the universe cannot hold another one of these, which
    /// is a real answer and the sheet should say so rather than
    /// offering an address that would collide.
    #[must_use]
    pub fn next_free(&self, universe: Universe, footprint: u16) -> Option<u16> {
        if footprint == 0 || footprint > SLOTS {
            return None;
        }
        let occupancy = self
            .occupancy()
            .into_iter()
            .find(|o| o.universe == universe)
            .unwrap_or_else(|| Occupancy {
                universe,
                slots: vec![0_u32; usize::from(SLOTS)],
                used: 0,
            });
        let mut run = 0_u16;
        for (index, occupant) in occupancy.slots.iter().enumerate() {
            if *occupant == 0 {
                run = run.saturating_add(1);
                if run >= footprint {
                    let slot = u16::try_from(index).unwrap_or(u16::MAX).saturating_add(1);
                    return Some(slot.saturating_sub(footprint).saturating_add(1));
                }
            } else {
                run = 0;
            }
        }
        None
    }

    /// Rows with no address — the filter an operator lives in while
    /// bringing a room up (`r[patch.sheet.filter]`).
    pub fn unpatched(&self) -> impl Iterator<Item = &PatchRow> {
        self.rows.iter().filter(|r| !r.patched || r.address == 0)
    }

    /// Rows whose model resolved to no fixture type. Each one is a
    /// fixture that will not light, and the sheet's most important
    /// warning.
    pub fn untyped(&self) -> impl Iterator<Item = &PatchRow> {
        self.rows
            .iter()
            .filter(|r| r.patched && r.fixture_type.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::{PatchRow, PatchSheet};

    fn row(chan: u32, universe: u16, address: u16, footprint: u16) -> PatchRow {
        PatchRow {
            chan,
            universe,
            address,
            footprint,
            patched: true,
            ..PatchRow::default()
        }
    }

    fn sheet(rows: Vec<PatchRow>) -> PatchSheet {
        PatchSheet {
            rows,
            universes: vec![1],
            ..PatchSheet::default()
        }
    }

    /// r[verify patch.conflict] - an overlap is reported on both rows
    #[test]
    fn two_fixtures_sharing_a_slot_are_both_told() {
        // 1 occupies 1..=7, 2 occupies 5..=11. Neither is the guilty
        // one; the operator decides which moves.
        let found = sheet(vec![row(1, 1, 1, 7), row(2, 1, 5, 7)]).conflicts();
        let channels: Vec<u32> = found.iter().map(|(chan, _)| *chan).collect();
        assert_eq!(channels, vec![1, 2]);
        assert_eq!(found[0].1.with, 2);
        assert_eq!(found[0].1.at, 5, "the first shared slot");
    }

    #[test]
    fn adjacent_is_not_overlapping() {
        // 1 ends at 7, 2 starts at 8. This is a correctly packed patch
        // and flagging it would make the warning worthless.
        assert!(
            sheet(vec![row(1, 1, 1, 7), row(2, 1, 8, 7)])
                .conflicts()
                .is_empty()
        );
    }

    #[test]
    fn a_different_universe_is_not_a_conflict() {
        assert!(
            sheet(vec![row(1, 1, 1, 7), row(2, 2, 1, 7)])
                .conflicts()
                .is_empty()
        );
    }

    /// r[verify patch.multipatch] - one address, deliberately, twice
    #[test]
    fn an_exactly_shared_address_is_multipatch_not_a_clash() {
        // Four house pars on one address is normal in a small venue.
        assert!(
            sheet(vec![row(1, 1, 1, 7), row(2, 1, 1, 7)])
                .conflicts()
                .is_empty()
        );
    }

    #[test]
    fn an_unpatched_fixture_occupies_nothing() {
        let mut spare = row(2, 1, 1, 7);
        spare.patched = false;
        assert!(sheet(vec![row(1, 1, 1, 7), spare]).conflicts().is_empty());
    }

    /// r[verify patch.address] - next free, from the live occupancy
    #[test]
    fn next_free_finds_the_first_gap_that_actually_fits() {
        // 1..=7 taken, 8..=10 free, 11..=17 taken. A 7-channel fixture
        // does not fit in the gap and must go after 17; a 3-channel one
        // fits in it.
        let sheet = sheet(vec![row(1, 1, 1, 7), row(2, 1, 11, 7)]);
        assert_eq!(sheet.next_free(1, 3), Some(8));
        assert_eq!(sheet.next_free(1, 7), Some(18));
    }

    #[test]
    fn a_full_universe_says_so_rather_than_offering_a_collision() {
        let sheet = sheet(vec![row(1, 1, 1, 512)]);
        assert_eq!(sheet.next_free(1, 1), None);
    }

    #[test]
    fn an_empty_universe_starts_at_one() {
        assert_eq!(sheet(vec![]).next_free(1, 7), Some(1));
        // A universe the venue never configured is still somewhere a
        // fixture can go.
        assert_eq!(sheet(vec![]).next_free(9, 7), Some(1));
    }

    /// r[verify patch.occupancy] - the 512 slots, and the gap in them
    #[test]
    fn occupancy_names_who_holds_each_slot() {
        let occupancy = sheet(vec![row(1, 1, 1, 7), row(2, 1, 11, 7)]).occupancy();
        let first = &occupancy[0];
        assert_eq!(first.used, 14);
        assert_eq!(first.slots[0], 1, "slot 1 is channel 1");
        assert_eq!(first.slots[6], 1, "slot 7 is still channel 1");
        assert_eq!(first.slots[7], 0, "slot 8 is free");
        assert_eq!(first.slots[10], 2, "slot 11 is channel 2");
        // The rest of the universe past channel 2 is the biggest gap.
        assert_eq!(first.largest_gap(), Some((18, 495)));
    }

    #[test]
    fn a_fixture_running_past_512_is_clipped_not_wrapped() {
        // A 20-channel fixture at 500 overruns the universe. It must
        // not appear in slot 1 of the same universe, which is what a
        // wrap would do and what would send a mover's pan to a par.
        let occupancy = sheet(vec![row(1, 1, 500, 20)]).occupancy();
        assert_eq!(occupancy[0].slots[0], 0, "slot 1 is untouched");
        assert_eq!(occupancy[0].used, 13, "500..=512 only");
    }

    #[test]
    fn an_address_reads_the_way_it_is_spoken() {
        assert_eq!(row(1, 1, 1, 7).address_label(), "1.001");
        assert_eq!(row(1, 2, 40, 7).address_label(), "2.040");
        let mut spare = row(1, 1, 1, 7);
        spare.patched = false;
        assert_eq!(spare.address_label(), "—");
    }
}
