//! The fixture-type library and its editor, as a surface sees them.
//!
//! Shaped on QLC+'s Fixture Definition Editor, which is the one everyone
//! who has ever written a fixture profile already knows: General,
//! Physical, Channels, Modes. What differs is what this project has that
//! QLC+ does not — a **confidence** per type and per mode, and the
//! **sources** each fact came from, because half these fixtures are
//! two-dollar pars whose channel order was read off a listing and
//! somebody will doubt it later.
//!
//! Like [`crate::patch`], the data arrives flattened from the host: this
//! crate cannot open `data/fixtures/`, and the same panes run in a
//! browser that has no filesystem at all.

use serde::{Deserialize, Serialize};

/// One value range on a channel — what the fixture does between two
/// bytes, in the manual's words.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeRow {
    pub from: u8,
    pub to: u8,
    pub meaning: String,
    /// The wheel slot this range selects, when the channel is a wheel.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub slot: String,
}

impl RangeRow {
    /// `16–31`, the way a manual prints it.
    #[must_use]
    pub fn span(&self) -> String {
        if self.from == self.to {
            self.from.to_string()
        } else {
            format!("{}–{}", self.from, self.to)
        }
    }
}

/// One DMX channel of a mode, flattened.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelRow {
    /// 1-based within the mode, as a manual and a console both count.
    pub number: u16,
    /// The manual's own name for it — `Warm White 3`, `Colour
    /// Selection`.
    pub name: String,
    /// What it resolved to in the engine's vocabulary. Empty when
    /// nothing matched, which is a channel that occupies its byte and
    /// nothing can drive.
    pub function: String,
    /// Where it rests when nothing drives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<RangeRow>,
}

/// One named mode: an ordered chart, and how much of it is known.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeRow {
    pub name: String,
    /// How many consecutive channels it occupies — the number that
    /// decides whether the next fixture's address is free.
    pub footprint: u16,
    /// Overrides the type's confidence for this mode alone. An 8ch
    /// chart from the manual beside a 4ch chart that is a guess is the
    /// normal case, not an exotic one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub confidence: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub channels: Vec<ChannelRow>,
    /// Lines of the chart that could not be read, with why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub complaints: Vec<String>,
}

/// A wheel position.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotRow {
    pub name: String,
    /// The byte that lands in the middle of the slot — the edges are
    /// where the split colours live.
    pub byte: u8,
    /// CSS for the swatch, for a colour wheel.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub css: String,
}

/// One fixture type.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeRow {
    /// The document's file stem — how a save names it.
    pub key: String,
    /// The identity: what a venue writes in `model`, and what a
    /// generated GDTF profile carries as its name.
    pub console_name: String,
    pub manufacturer: String,
    pub model: String,
    /// Other model strings that resolve here. One OEM head is sold under
    /// four brand names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// `manual`, `listing` or `guess`.
    pub confidence: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Where the facts came from. The next person to doubt a channel
    /// order needs the manual, not a commit message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// Width, length, height in mm; weight in kg; power in W. `None`
    /// where nobody published it.
    #[serde(default)]
    pub physical: Vec<(String, String)>,
    #[serde(default)]
    pub optics: Vec<(String, String)>,
    pub modes: Vec<ModeRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub color_wheel: Vec<SlotRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gobo_wheel: Vec<SlotRow>,
    /// How many fixtures in the loaded venue are patched to this type.
    /// A type nothing uses is not wrong, but it is worth seeing.
    #[serde(default)]
    pub patched: usize,
}

impl TypeRow {
    /// `Uking Par · 7ch, 8ch`.
    #[must_use]
    pub fn widths(&self) -> String {
        self.modes
            .iter()
            .map(|mode| format!("{}", mode.footprint))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The library as the panes see it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeLibrary {
    pub types: Vec<TypeRow>,
    /// Documents that would not load, with why. A library with one
    /// broken file should still patch the rest, and the desk should be
    /// able to say which file is broken.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected: Vec<(String, String)>,
}

impl TypeLibrary {
    /// The type a console name resolves to, for the editor to open.
    #[must_use]
    pub fn get(&self, console_name: &str) -> Option<&TypeRow> {
        self.types
            .iter()
            .find(|row| row.console_name == console_name)
    }
}

#[cfg(test)]
mod tests {
    use super::{ModeRow, RangeRow, TypeRow};

    #[test]
    fn a_range_reads_the_way_a_manual_prints_it() {
        let range = RangeRow {
            from: 16,
            to: 31,
            ..RangeRow::default()
        };
        assert_eq!(range.span(), "16–31");
        // A one-byte range is a single value, not a span of one.
        let single = RangeRow {
            from: 5,
            to: 5,
            ..RangeRow::default()
        };
        assert_eq!(single.span(), "5");
    }

    #[test]
    fn a_types_widths_are_its_modes() {
        let row = TypeRow {
            modes: vec![
                ModeRow {
                    footprint: 7,
                    ..ModeRow::default()
                },
                ModeRow {
                    footprint: 24,
                    ..ModeRow::default()
                },
            ],
            ..TypeRow::default()
        };
        assert_eq!(row.widths(), "7, 24");
    }
}

// ── The panes ────────────────────────────────────────────────────────

use dioxus::prelude::*;

const CSS: &str = include_str!("fixtures.css");

/// How the host hands the library to the panes.
#[derive(Clone, Copy)]
pub struct LibraryFeed(pub Signal<TypeLibrary>);

/// Which type the editor is showing. Shared between the two panes: the
/// library is a list you pick from and the editor is what you picked.
#[derive(Clone, Copy)]
pub struct Opened(pub Signal<Option<String>>);

#[must_use]
pub fn use_library() -> Signal<TypeLibrary> {
    use_context::<LibraryFeed>().0
}

#[must_use]
pub fn use_opened() -> Signal<Option<String>> {
    use_context::<Opened>().0
}

/// The fixture-type library: what Ignition knows how to address.
///
/// A list rather than a grid, because what distinguishes two entries is
/// their names and their widths, not a picture. Each row carries its
/// **confidence** badge, which is the fact this library has and a
/// console's does not: `guess` means nobody has seen a manual, and an
/// operator should know that before the fixture does something
/// unexpected rather than after.
// r[impl patch.type-confidence] - the badge, on the list
#[component]
pub fn FixtureTypesPane() -> Element {
    let library = use_library();
    let mut opened = use_opened();
    let mut search = use_signal(String::new);
    let library = library();
    let needle = search().to_lowercase();
    let rows: Vec<&TypeRow> = library
        .types
        .iter()
        .filter(|row| {
            needle.is_empty()
                || row.console_name.to_lowercase().contains(&needle)
                || row.manufacturer.to_lowercase().contains(&needle)
                || row.model.to_lowercase().contains(&needle)
        })
        .collect();
    let current = opened();

    rsx! {
        style { {CSS} }
        section { class: "types",
            header { class: "types-head",
                input {
                    class: "types-search",
                    r#type: "text",
                    placeholder: "search {library.types.len()} types",
                    value: "{search}",
                    oninput: move |e| search.set(e.value()),
                }
            }
            if !library.rejected.is_empty() {
                div { class: "types-broken",
                    for (path , why) in library.rejected.clone() {
                        div { title: "{why}", "{path} would not load" }
                    }
                }
            }
            div { class: "types-list",
                if rows.is_empty() {
                    div { class: "types-empty", "nothing matches" }
                }
                for row in rows {
                    button {
                        key: "{row.console_name}",
                        class: if current.as_deref() == Some(row.console_name.as_str()) {
                            "type-row on"
                        } else {
                            "type-row"
                        },
                        onclick: {
                            let name = row.console_name.clone();
                            move |_| opened.set(Some(name.clone()))
                        },
                        div { class: "type-line",
                            span { class: "type-name", "{row.console_name}" }
                            span { class: "conf {row.confidence}", "{row.confidence}" }
                        }
                        div { class: "type-sub",
                            span { "{row.manufacturer}" }
                            span { class: "type-widths", "{row.widths()} ch" }
                            if row.patched > 0 {
                                span { class: "type-used", "×{row.patched}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Which tab of the editor is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Channels,
    Modes,
    Wheels,
    General,
}

impl Tab {
    const ALL: [Self; 4] = [Self::Channels, Self::Modes, Self::Wheels, Self::General];
    const fn label(self) -> &'static str {
        match self {
            Self::Channels => "Channels",
            Self::Modes => "Modes",
            Self::Wheels => "Wheels",
            Self::General => "General",
        }
    }
}

/// One fixture type, in full.
///
/// QLC+'s tabs, minus the ones that do not apply: there is no Heads tab
/// because a multi-section bar is described here by a compressed channel
/// span (`Red/Green/Blue per section`) rather than by a separate head
/// list, and no Physical tab of its own because six numbers do not need
/// one — they sit under General with the sources.
// r[impl patch.type-modes] - modes side by side, with their charts
// r[impl patch.type-sources] - the manual, with the facts it produced
#[component]
pub fn FixtureEditorPane() -> Element {
    let library = use_library();
    let opened = use_opened();
    let mut tab = use_signal(Tab::default);
    let mode_name = use_signal(String::new);

    let library = library();
    let Some(name) = opened() else {
        return rsx! {
            style { {CSS} }
            section { class: "editor",
                div { class: "editor-empty", "pick a fixture type" }
            }
        };
    };
    let Some(row) = library.get(&name).cloned() else {
        return rsx! {
            style { {CSS} }
            section { class: "editor",
                div { class: "editor-empty", "{name} is not in the library" }
            }
        };
    };

    // The mode being looked at: whatever was picked, else the widest,
    // which is the one a fixture is most often in.
    let current_mode = row
        .modes
        .iter()
        .find(|m| m.name == mode_name())
        .or_else(|| row.modes.iter().max_by_key(|m| m.footprint));
    let showing = tab();

    rsx! {
        style { {CSS} }
        section { class: "editor",
            header { class: "editor-head",
                span { class: "editor-name", "{row.console_name}" }
                span { class: "conf {row.confidence}", "{row.confidence}" }
                nav { class: "editor-tabs",
                    for it in Tab::ALL {
                        button {
                            key: "{it.label()}",
                            class: if showing == it { "editor-tab on" } else { "editor-tab" },
                            onclick: move |_| tab.set(it),
                            "{it.label()}"
                        }
                    }
                }
            }
            div { class: "editor-body",
                match showing {
                    Tab::Channels => rsx! {
                        div { class: "mode-strip",
                            for mode in row.modes.clone() {
                                ModeKey {
                                    key: "{mode.name}",
                                    name: mode.name.clone(),
                                    on: current_mode.map(|m| m.name.as_str()) == Some(mode.name.as_str()),
                                    picked: mode_name,
                                }
                            }
                        }
                        if let Some(mode) = current_mode {
                            if !mode.note.is_empty() {
                                div { class: "mode-note", "{mode.note}" }
                            }
                            for complaint in mode.complaints.clone() {
                                div { class: "mode-complaint", "{complaint}" }
                            }
                            table { class: "chan-table",
                                thead {
                                    tr {
                                        th { class: "num", "#" }
                                        th { "Channel" }
                                        th { "Drives" }
                                        th { class: "num", "Rest" }
                                        th { "Ranges" }
                                    }
                                }
                                tbody {
                                    for channel in mode.channels.clone() {
                                        tr { key: "{channel.number}", class: "chan-row",
                                            td { class: "num", "{channel.number}" }
                                            td { "{channel.name}" }
                                            td {
                                                span {
                                                    class: if channel.function.is_empty() { "fn none" } else { "fn" },
                                                    if channel.function.is_empty() {
                                                        "nothing"
                                                    } else {
                                                        "{channel.function}"
                                                    }
                                                }
                                            }
                                            td { class: "num",
                                                if let Some(rest) = channel.default { "{rest}" } else { "—" }
                                            }
                                            td { class: "ranges",
                                                if channel.ranges.is_empty() {
                                                    span { class: "none", "not charted" }
                                                }
                                                for range in channel.ranges {
                                                    span { class: "range",
                                                        span { class: "range-span", "{range.span()}" }
                                                        span { class: "range-what", "{range.meaning}" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    },
                    Tab::Modes => rsx! {
                        table { class: "chan-table",
                            thead {
                                tr {
                                    th { "Mode" }
                                    th { class: "num", "Channels" }
                                    th { "Confidence" }
                                    th { "Note" }
                                }
                            }
                            tbody {
                                for mode in row.modes.clone() {
                                    ModeLine {
                                        key: "{mode.name}",
                                        mode,
                                        fallback: row.confidence.clone(),
                                    }
                                }
                            }
                        }
                    },
                    Tab::Wheels => rsx! {
                        if row.color_wheel.is_empty() && row.gobo_wheel.is_empty() {
                            div { class: "editor-empty",
                                "this fixture mixes its colour — it has no wheel"
                            }
                        }
                        if !row.color_wheel.is_empty() {
                            div { class: "wheel-head", "Colour" }
                            div { class: "wheel",
                                for slot in row.color_wheel.clone() {
                                    div { key: "{slot.byte}", class: "slot-chip",
                                        span { class: "swatch", style: "background: {slot.css}" }
                                        span { class: "slot-name", "{slot.name}" }
                                        span { class: "slot-byte", "{slot.byte}" }
                                    }
                                }
                            }
                        }
                        if !row.gobo_wheel.is_empty() {
                            div { class: "wheel-head", "Gobo" }
                            div { class: "wheel",
                                for slot in row.gobo_wheel.clone() {
                                    div { key: "{slot.byte}", class: "slot-chip",
                                        span { class: "slot-name", "{slot.name}" }
                                        span { class: "slot-byte", "{slot.byte}" }
                                    }
                                }
                            }
                        }
                    },
                    Tab::General => rsx! {
                        dl { class: "facts",
                            dt { "Manufacturer" }
                            dd { "{row.manufacturer}" }
                            dt { "Model" }
                            dd { "{row.model}" }
                            if !row.aliases.is_empty() {
                                dt { "Also patched as" }
                                dd { "{row.aliases.join(\", \")}" }
                            }
                            for (name , value) in row.physical.clone() {
                                dt { "{name}" }
                                dd { "{value}" }
                            }
                            for (name , value) in row.optics.clone() {
                                dt { "{name}" }
                                dd { "{value}" }
                            }
                        }
                        if !row.sources.is_empty() {
                            div { class: "wheel-head", "Sources" }
                            ul { class: "sources",
                                for source in row.sources.clone() {
                                    li { key: "{source}", "{source}" }
                                }
                            }
                        }
                        if !row.notes.is_empty() {
                            div { class: "wheel-head", "Notes" }
                            p { class: "notes", "{row.notes}" }
                        }
                    },
                }
            }
        }
    }
}

/// One row of the Modes tab.
///
/// A mode with no confidence of its own inherits the document's, which
/// is the common case: the fixture was researched once and every chart
/// in it is as good as the others.
#[component]
fn ModeLine(mode: ModeRow, fallback: String) -> Element {
    let confidence = if mode.confidence.is_empty() {
        fallback
    } else {
        mode.confidence.clone()
    };
    rsx! {
        tr { class: "chan-row",
            td { "{mode.name}" }
            td { class: "num", "{mode.footprint}" }
            td {
                span { class: "conf {confidence}", "{confidence}" }
            }
            td { "{mode.note}" }
        }
    }
}

/// One key of the Channels tab's mode strip.
#[component]
fn ModeKey(name: String, on: bool, picked: Signal<String>) -> Element {
    let mut picked = picked;
    let chosen = name.clone();
    rsx! {
        button {
            class: if on { "mode-key on" } else { "mode-key" },
            onclick: move |_| picked.set(chosen.clone()),
            "{name}"
        }
    }
}
