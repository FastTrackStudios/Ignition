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

// ── The panes ────────────────────────────────────────────────────────

use crate::command::{Command, PatchEdit};
use crate::send;
use dioxus::prelude::*;

const CSS: &str = include_str!("patch.css");

/// How the host hands the sheet to the panes.
///
/// The same shape as [`crate::Surface`] and for the same reason: this
/// crate cannot see a `Venue`, and the browser on an iPad has no Bevy
/// world to hold one. The host resolves it once and republishes after
/// every edit — a pane that re-read the venue off disk would be a second
/// copy of the truth, which is the failure `cameras.rs` names.
#[derive(Clone, Copy)]
pub struct SheetFeed(pub Signal<PatchSheet>);

/// The patch, as the host last published it.
#[must_use]
pub fn use_sheet() -> Signal<PatchSheet> {
    use_context::<SheetFeed>().0
}

/// Which rows the sheet is showing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    /// No address yet — the filter an operator lives in while bringing
    /// a room up.
    Unpatched,
    /// Overlapping another fixture.
    Conflicts,
    /// No fixture type resolved. Every one of these is a fixture that
    /// will not light.
    Untyped,
    Universe(Universe),
    Model(String),
}

impl Filter {
    fn label(&self) -> String {
        match self {
            Self::All => "All".to_owned(),
            Self::Unpatched => "Unpatched".to_owned(),
            Self::Conflicts => "Conflicts".to_owned(),
            Self::Untyped => "No type".to_owned(),
            Self::Universe(universe) => format!("Universe {universe}"),
            Self::Model(model) => model.clone(),
        }
    }
}

/// The patch sheet.
///
/// Channel-ordered, because that is how a patch sheet is read
/// everywhere. Condensed by default and full on request
/// (`r[patch.sheet.columns]`): patching forty fixtures needs six
/// columns and auditing one needs twenty, and a sheet that always shows
/// twenty cannot be read at a rig.
// r[impl patch.sheet] - the pane
// r[impl patch.sheet.columns] - condensed and full
// r[impl patch.sheet.filter] - the rail, including unpatched and conflicting
#[component]
pub fn PatchPane() -> Element {
    let sheet = use_sheet();
    let mut full = use_signal(|| false);
    let mut adding = use_signal(|| false);
    let filter = use_signal(Filter::default);
    let mut selected = use_signal(|| None::<u32>);

    let sheet = sheet();
    let conflicts: std::collections::BTreeMap<u32, Conflict> =
        sheet.conflicts().into_iter().collect();
    let current = filter();

    // The rail's model list, in first-seen order so it reads like the
    // rig rather than like an alphabet.
    let mut models: Vec<String> = Vec::new();
    for row in &sheet.rows {
        if !row.model.is_empty() && !models.contains(&row.model) {
            models.push(row.model.clone());
        }
    }
    let untyped = sheet.untyped().count();
    let unpatched = sheet.unpatched().count();

    let rows: Vec<&PatchRow> = sheet
        .rows
        .iter()
        .filter(|row| match &current {
            Filter::All => true,
            Filter::Unpatched => !row.patched || row.address == 0,
            Filter::Conflicts => conflicts.contains_key(&row.chan),
            Filter::Untyped => row.patched && row.fixture_type.is_empty(),
            Filter::Universe(universe) => row.universe == *universe,
            Filter::Model(model) => row.model == *model,
        })
        .collect();

    rsx! {
        style { {CSS} }
        section { class: "patch",
            header { class: "patch-head",
                span { class: "patch-count", "{rows.len()} of {sheet.rows.len()}" }
                if !conflicts.is_empty() {
                    span {
                        class: "patch-warn",
                        title: "two fixtures share DMX channels",
                        "{conflicts.len() / 2} conflicts"
                    }
                }
                if untyped > 0 {
                    span {
                        class: "patch-warn",
                        title: "no fixture type resolved — these will not light",
                        "{untyped} untyped"
                    }
                }
                button {
                    class: if adding() { "patch-key on" } else { "patch-key" },
                    title: "add fixtures to this room",
                    onclick: move |_| adding.toggle(),
                    "ADD"
                }
                button {
                    class: if full() { "patch-key on" } else { "patch-key" },
                    title: "show every column",
                    onclick: move |_| full.toggle(),
                    "FULL"
                }
                // Nothing reaches the venue file until this is pressed
                // (`r[patch.explicit-save]`): patching is exploratory,
                // and a file that changed under every keystroke could
                // not be diffed or reverted.
                button {
                    class: if sheet.dirty { "patch-key save on" } else { "patch-key save" },
                    disabled: !sheet.dirty,
                    title: "write the patch back to the venue",
                    onclick: move |_| send(Command::Patch(PatchEdit::Save)),
                    if sheet.dirty { "SAVE ●" } else { "SAVED" }
                }
            }
            if adding() {
                InsertBar { universes: sheet.universes.clone(), done: move |()| adding.set(false) }
            }
            div { class: "patch-body",
                nav { class: "patch-rail",
                    FilterKey { filter, current: current.clone(), it: Filter::All, count: sheet.rows.len() }
                    FilterKey { filter, current: current.clone(), it: Filter::Unpatched, count: unpatched }
                    FilterKey {
                        filter,
                        current: current.clone(),
                        it: Filter::Conflicts,
                        count: conflicts.len() / 2,
                    }
                    FilterKey { filter, current: current.clone(), it: Filter::Untyped, count: untyped }
                    div { class: "patch-rail-head", "Universes" }
                    for universe in sheet.universes.clone() {
                        FilterKey {
                            filter,
                            current: current.clone(),
                            it: Filter::Universe(universe),
                            count: sheet.rows.iter().filter(|r| r.universe == universe && r.patched).count(),
                        }
                    }
                    div { class: "patch-rail-head", "Types" }
                    for model in models {
                        FilterKey {
                            filter,
                            current: current.clone(),
                            it: Filter::Model(model.clone()),
                            count: sheet.rows.iter().filter(|r| r.model == model).count(),
                        }
                    }
                }
                div { class: "patch-sheet",
                    table {
                        thead {
                            tr {
                                th { class: "num", "Chan" }
                                th { "Name" }
                                th { "Type" }
                                if full() {
                                    th { "Mode" }
                                    th { "Made of" }
                                }
                                th { class: "num", "U.Addr" }
                                th { class: "num", "Wide" }
                                th { "Label" }
                                if full() {
                                    th { "Gel" }
                                    th { "Tags" }
                                    th { "Position" }
                                }
                            }
                        }
                        tbody {
                            for row in rows {
                                PatchLine {
                                    key: "{row.chan}",
                                    row: row.clone(),
                                    full: full(),
                                    conflict: conflicts.get(&row.chan).cloned(),
                                    selected: selected() == Some(row.chan),
                                    on_pick: move |chan| {
                                        selected.set(Some(chan));
                                        send(Command::Select(ignition_core::Selection::Chans(vec![chan])));
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn FilterKey(filter: Signal<Filter>, current: Filter, it: Filter, count: usize) -> Element {
    let on = current == it;
    let label = it.label();
    // A filter that would show nothing is drawn dim rather than hidden:
    // "no conflicts" is the answer an operator is looking for, and a key
    // that vanishes when the answer is good cannot give it.
    rsx! {
        button {
            class: if on { "rail-key on" } else if count == 0 { "rail-key none" } else { "rail-key" },
            onclick: move |_| filter.set(it.clone()),
            span { class: "rail-label", "{label}" }
            span { class: "rail-count", "{count}" }
        }
    }
}

#[component]
fn PatchLine(
    row: PatchRow,
    full: bool,
    conflict: Option<Conflict>,
    selected: bool,
    on_pick: EventHandler<u32>,
) -> Element {
    let chan = row.chan;
    let class = match (selected, conflict.is_some(), row.patched) {
        (true, _, _) => "patch-row on",
        (_, true, _) => "patch-row clash",
        (_, _, false) => "patch-row dark",
        _ => "patch-row",
    };
    let kind = if row.fixture_type.is_empty() {
        row.model.clone()
    } else {
        row.fixture_type.clone()
    };
    let title = conflict.as_ref().map_or_else(
        || row.name.clone(),
        |c| {
            format!(
                "shares channel {}.{:03} with chan {}",
                c.universe, c.at, c.with
            )
        },
    );
    rsx! {
        tr { class, title, onclick: move |_| on_pick.call(chan),
            td { class: "num", "{row.chan}" }
            td { "{row.name}" }
            td {
                span { class: if row.fixture_type.is_empty() { "type missing" } else { "type" }, "{kind}" }
                if !row.confidence.is_empty() {
                    span { class: "conf {row.confidence}", title: "how the channel chart was come by", "{row.confidence}" }
                }
            }
            if full {
                td { "{row.mode}" }
                td { "{row.manufacturer}" }
            }
            td { class: "num addr",
                AddressCell { chan, universe: row.universe, address: row.address, patched: row.patched }
            }
            td { class: "num", if row.footprint > 0 { "{row.footprint}" } else { "—" } }
            td {
                TextCell {
                    value: row.label.clone(),
                    placeholder: "label",
                    on_commit: move |text| send(Command::Patch(PatchEdit::Label { chan, label: text })),
                }
            }
            if full {
                td {
                    TextCell {
                        value: row.gel.clone(),
                        placeholder: "gel",
                        on_commit: move |text| send(Command::Patch(PatchEdit::Gel { chan, gel: text })),
                    }
                }
                td { "{row.tags.join(\" \")}" }
                td { class: "num",
                    "{row.position[0]:.1} {row.position[1]:.1} {row.position[2]:.1}"
                }
            }
        }
    }
}

/// A universe as its 512 channels.
///
/// "Where does this fit" is a spatial question and a list of addresses
/// answers it badly (`r[patch.occupancy]`). Each cell is one slot,
/// coloured by the fixture holding it, so a free run reads as a gap
/// rather than as arithmetic.
// r[impl patch.occupancy] - the pane
#[component]
pub fn UniversesPane() -> Element {
    let sheet = use_sheet();
    let sheet = sheet();
    let occupancy = sheet.occupancy();
    rsx! {
        style { {CSS} }
        section { class: "universes",
            if occupancy.is_empty() {
                div { class: "patch-empty", "no universes patched" }
            }
            for universe in occupancy {
                div { class: "uni",
                    header { class: "uni-head",
                        span { class: "uni-name", "Universe {universe.universe}" }
                        span { class: "uni-used", "{universe.used} / 512" }
                        if let Some((at, run)) = universe.largest_gap() {
                            span { class: "uni-gap", title: "the longest free run", "gap {at}+{run}" }
                        }
                    }
                    div { class: "uni-grid",
                        for (index , occupant) in universe.slots.iter().enumerate() {
                            span {
                                key: "{index}",
                                class: if *occupant == 0 { "slot" } else { "slot on" },
                                title: if *occupant == 0 {
                                    format!("{}.{:03} free", universe.universe, index + 1)
                                } else {
                                    format!("{}.{:03} — chan {occupant}", universe.universe, index + 1)
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A cell you can type an address into.
///
/// Accepts what a console accepts: `2.40` and `2/40` for universe two
/// address forty, a bare `40` for the universe it is already in, and an
/// empty box to unpatch (`r[patch.address]`). It commits on blur and on
/// Enter, never per keystroke — half a typed address is a real address
/// somewhere else, and repatching to it on the way past would be a
/// fixture briefly stealing another's channels.
// r[impl patch.address] - the console idiom, in a cell
#[component]
fn AddressCell(chan: u32, universe: Universe, address: u16, patched: bool) -> Element {
    // A plain `fn` rather than a closure: two handlers need it, and a
    // closure capturing signals cannot be called from both.
    fn commit(
        text: &str,
        chan: u32,
        universe: Universe,
        shown: &str,
        mut draft: Signal<String>,
        mut editing: Signal<bool>,
    ) {
        editing.set(false);
        let text = text.trim();
        if text.is_empty() {
            send(Command::Patch(PatchEdit::Unpatch { chan }));
            return;
        }
        let Some((into_universe, into_address)) = parse_address(text, universe) else {
            // Unreadable: put the cell back rather than guessing. A
            // guessed address is a fixture somewhere nobody put it.
            draft.set(shown.to_owned());
            return;
        };
        send(Command::Patch(PatchEdit::Address {
            chan,
            universe: into_universe,
            address: into_address,
        }));
    }

    let shown = if patched && address > 0 {
        format!("{universe}.{address}")
    } else {
        String::new()
    };
    let mut draft = use_signal(|| shown.clone());
    let mut editing = use_signal(|| false);

    // While not being typed in, the cell follows the sheet: an edit from
    // another window, or a refused one snapping back, has to show.
    if !editing() && draft() != shown {
        draft.set(shown.clone());
    }

    let back = shown.clone();
    let on_enter = shown;
    rsx! {
        input {
            class: "cell addr-cell",
            r#type: "text",
            placeholder: "—",
            value: "{draft}",
            onfocusin: move |_| editing.set(true),
            oninput: move |e| draft.set(e.value()),
            onblur: move |_| commit(&draft(), chan, universe, &back, draft, editing),
            onkeydown: move |e| {
                if e.key() == Key::Enter {
                    commit(&draft(), chan, universe, &on_enter, draft, editing);
                }
            },
        }
    }
}

/// `2.40`, `2/40` or a bare `40` in the universe the fixture is already
/// in. Returns `None` for anything that is not an address, so the caller
/// can put the cell back rather than guess.
fn parse_address(text: &str, current: Universe) -> Option<(Universe, u16)> {
    let text = text.trim();
    let (universe, address) = match text.split_once(['.', '/', ':']) {
        Some((left, right)) => (left.trim().parse::<u16>().ok()?, right.trim()),
        // A bare number keeps the universe it is in. A fixture with no
        // universe yet lands in the first one.
        None => (if current == 0 { 1 } else { current }, text),
    };
    let address = address.parse::<u16>().ok()?;
    (universe > 0 && (1..=512).contains(&address)).then_some((universe, address))
}

/// A cell you can type text into, committed on blur or Enter.
#[component]
fn TextCell(value: String, placeholder: String, on_commit: EventHandler<String>) -> Element {
    let mut draft = use_signal(|| value.clone());
    let mut editing = use_signal(|| false);
    // While not being typed in, the cell follows the sheet: an edit from
    // another window, or a refused one snapping back, has to show.
    if !editing() && draft() != value {
        draft.set(value);
    }
    rsx! {
        input {
            class: "cell",
            r#type: "text",
            placeholder: "{placeholder}",
            value: "{draft}",
            onfocusin: move |_| editing.set(true),
            oninput: move |e| draft.set(e.value()),
            onblur: move |_| {
                editing.set(false);
                on_commit.call(draft().trim().to_owned());
            },
            onkeydown: move |e| {
                if e.key() == Key::Enter {
                    editing.set(false);
                    on_commit.call(draft().trim().to_owned());
                }
            },
        }
    }
}

/// Adding fixtures: a type, how many, and where they go.
///
/// The wizard every console has, because rigs come in bars of eight and
/// patching them one at a time is the thirty minutes
/// `r[profile.setup-cost-is-the-metric]` is measured in. Both address
/// fields default to "next free", which is what adding to a rig usually
/// wants, and the offset defaults to the type's own footprint, which is
/// what packing a bar wants.
// r[impl patch.insert] - type, quantity, and where
#[component]
fn InsertBar(universes: Vec<Universe>, done: EventHandler<()>) -> Element {
    let library = crate::fixtures::use_library();
    let mut fixture_type = use_signal(String::new);
    let mut count = use_signal(|| "1".to_owned());
    let mut universe = use_signal(|| {
        universes
            .first()
            .map_or_else(|| "1".to_owned(), ToString::to_string)
    });
    let mut address = use_signal(String::new);
    let types = library().types;

    // Nothing to patch to until a type is picked: a fixture with no type
    // never lights, and offering to make seventy of them is not a
    // kindness.
    let chosen = fixture_type();
    let ready = !chosen.is_empty();

    rsx! {
        div { class: "insert",
            select {
                class: "insert-type",
                value: "{fixture_type}",
                onchange: move |e| fixture_type.set(e.value()),
                option { value: "", "fixture type…" }
                for row in types {
                    option { key: "{row.console_name}", value: "{row.console_name}", "{row.console_name}" }
                }
            }
            label { class: "insert-field",
                span { "count" }
                input {
                    r#type: "text",
                    value: "{count}",
                    oninput: move |e| count.set(e.value()),
                }
            }
            label { class: "insert-field",
                span { "universe" }
                input {
                    r#type: "text",
                    value: "{universe}",
                    oninput: move |e| universe.set(e.value()),
                }
            }
            label { class: "insert-field",
                span { "address" }
                input {
                    r#type: "text",
                    placeholder: "next free",
                    value: "{address}",
                    oninput: move |e| address.set(e.value()),
                }
            }
            button {
                class: "patch-key on",
                disabled: !ready,
                onclick: move |_| {
                    send(Command::Patch(PatchEdit::Insert {
                        fixture_type: fixture_type(),
                        count: count().trim().parse().unwrap_or(1),
                        // Zero throughout means "you choose": the next
                        // free channel, the next free address, and the
                        // type's own footprint as the offset.
                        chan: 0,
                        universe: universe().trim().parse().unwrap_or(1),
                        address: address().trim().parse().unwrap_or(0),
                        offset: 0,
                    }));
                    done.call(());
                },
                "PATCH"
            }
            button { class: "patch-key", onclick: move |_| done.call(()), "CANCEL" }
        }
    }
}

#[cfg(test)]
mod address_tests {
    use super::parse_address;

    /// r[verify patch.address] - the console idiom
    #[test]
    fn an_address_reads_the_way_it_is_typed() {
        // Universe and address, in the three separators consoles use.
        assert_eq!(parse_address("2.40", 1), Some((2, 40)));
        assert_eq!(parse_address("2/40", 1), Some((2, 40)));
        assert_eq!(parse_address("2:40", 1), Some((2, 40)));
        // A bare number keeps the universe the fixture is already in,
        // which is what typing into a sheet of one universe wants.
        assert_eq!(parse_address("40", 3), Some((3, 40)));
        // A fixture with no universe yet lands in the first.
        assert_eq!(parse_address("40", 0), Some((1, 40)));
        assert_eq!(parse_address("  2 . 40 ", 1), Some((2, 40)));
    }

    #[test]
    fn nonsense_is_refused_rather_than_guessed() {
        // A guessed address is a fixture somewhere nobody put it.
        for text in ["", "chan 4", "2.", ".40", "2.0", "2.513", "0.40", "-1"] {
            assert_eq!(parse_address(text, 1), None, "{text:?} was accepted");
        }
    }
}
