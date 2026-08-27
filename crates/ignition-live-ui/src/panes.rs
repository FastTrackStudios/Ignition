//! Pane contents for the studio's dock: one pane per kind of thing the
//! profile offers, each a search field over what it lists, favourites
//! first — plus the fader row sized to the console's bottom band, and
//! the venue's desk banks.
//!
//! Groups, colours, splits and focus points are square grids of tiles
//! (a colour tile is the colour); looks are a list with a rendered
//! preview of each; effects are a table with a notes column; tricks,
//! bundles and macros are tiles. Every tile is selectable or fires: a
//! group tile selects (shift adds to the selection), colours and focus
//! land on the selection, looks, effects, bundles and macros fire. What
//! is lit comes from the playhead — the selection in words, the held
//! look, the effects playing — never from a value remembered here.
//! There is no favourite toggle on a pane: favourites order the list,
//! and editing them is a mode for later.
//!
//! Nothing scrolls: a pane clips to its leaf and its grid wraps, so the
//! window never grows a scrollbar.

// r[impl studio.dock.no-scroll] - panes clip, grids wrap
// r[impl studio.operators.favourites] - favourites first, a search field per pane
// r[impl studio.views.whole-profile] - every kind reaches a pane

use crate::command::Command;
use crate::library::{Entry, Tab, catalogue, matches, ordered, use_operator};
use crate::live::{FaderBank, Masters};
use crate::operators::Kind;
use crate::{Surface, send, use_playhead};
use dioxus::prelude::*;
use ignition_core::Selection;
use ignition_core::profile::LookKind;

/// The fader track height inside the Faders pane, in CSS pixels — the
/// console's bottom band is a quarter of a 1440-high window, and the
/// Live view's 220px track does not fit it with the key and the page
/// tabs. Kept in step with `console_fits_a_1440p_monitor`.
pub const PANE_TRACK: f32 = 120.0;

/// Where `just look-previews` writes a look's thumbnail.
pub const PREVIEW_DIR: &str = "data/looks/previews";

/// The names the playhead's selection description is made of. The
/// engine describes a union as `a + b`, a role as `role X`.
pub fn selected_names(description: &str) -> Vec<String> {
    description
        .split(" + ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `name` is in the selection the playhead describes.
pub fn is_selected(description: &str, name: &str) -> bool {
    selected_names(description)
        .iter()
        .any(|n| n.eq_ignore_ascii_case(name))
}

/// The selection a group tile sends: the group alone, or — with shift
/// — the current selection plus it (or minus it, when it is already
/// there). A described entry that reads `role X` goes back as a role.
// r[impl studio.program.pick-and-gizmos] - shift adds; the one Select command
pub fn pick(description: &str, name: &str, shift: bool) -> Selection {
    if !shift {
        return Selection::Group(name.to_string());
    }
    let mut names = selected_names(description);
    if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(name)) {
        names.remove(i);
    } else {
        names.push(name.to_string());
    }
    let mut parts: Vec<Selection> = names
        .into_iter()
        .map(|n| match n.strip_prefix("role ") {
            Some(role) => Selection::Role(role.to_string()),
            None => Selection::Group(n),
        })
        .collect();
    match parts.len() {
        0 => Selection::Group(name.to_string()),
        1 => parts.pop().expect("one"),
        _ => Selection::Union(parts),
    }
}

/// The tiles of a tab, favourites first, filtered by the search.
fn tiles_of(tab: Tab, surface: &Surface, query: &str) -> Vec<Entry> {
    let operator = use_operator();
    let all = catalogue(tab, surface);
    let favourites = tab
        .kind()
        .map(|k| operator().favourites.of(k).clone())
        .unwrap_or_default();
    ordered(&all, &favourites)
        .into_iter()
        .filter(|e| matches(e, query))
        .collect()
}

/// A pane of one kind: the shape the kind wants.
#[component]
pub fn KindPane(tab: Tab, surface: Surface) -> Element {
    match tab {
        Tab::Kind(Kind::Group) | Tab::Kind(Kind::Colour) | Tab::Kind(Kind::Focus) | Tab::Splits => {
            rsx! { SquaresPane { tab, surface } }
        }
        Tab::Kind(Kind::Look) => rsx! { LooksPane { surface } },
        Tab::Kind(Kind::Effect) => rsx! { EffectsPane { surface } },
        _ => rsx! { TilesPane { tab, surface } },
    }
}

/// The header every pane shares: the title, a note, the search box.
#[component]
fn PaneHead(
    title: String,
    #[props(default)] note: String,
    query: Signal<String>,
    #[props(default)] clear: bool,
) -> Element {
    let mut query = query;
    rsx! {
        header { class: "pane-head",
            span { class: "pane-title", "{title}" }
            if !note.is_empty() {
                span { class: "pane-note", "{note}" }
            }
            if clear {
                button {
                    class: "pane-key",
                    onpointerdown: move |_| send(Command::Deselect),
                    "CLEAR"
                }
            }
            input {
                class: "pane-search",
                r#type: "text",
                placeholder: "search",
                value: "{query}",
                oninput: move |e| query.set(e.value()),
            }
        }
    }
}

/// Groups, colours, splits, focus points: a grid of squares. A colour
/// square is the colour with its name small; a group or focus square
/// is its name. The selected groups are lit from the playhead.
// r[impl studio.touch] - squares no smaller than 56px
#[component]
pub fn SquaresPane(tab: Tab, surface: Surface) -> Element {
    let query = use_signal(String::new);
    let playhead = use_playhead();
    let tiles = tiles_of(tab, &surface, &query());
    let is_groups = tab == Tab::Kind(Kind::Group);
    let selection = playhead().selection.clone().unwrap_or_default();
    let note = if is_groups {
        if selection.is_empty() {
            "nothing selected".to_string()
        } else {
            selection.clone()
        }
    } else {
        String::new()
    };
    let class = format!("pane pane-{}", tab.label().to_lowercase());
    rsx! {
        section { class: "{class}",
            PaneHead { title: tab.label().to_string(), note, query, clear: is_groups }
            div { class: "pane-squares",
                for entry in tiles.iter().cloned() {
                    Square { key: "{entry.name}", entry, selection: selection.clone() }
                }
                if tiles.is_empty() {
                    span { class: "lib-empty", "nothing matches" }
                }
            }
        }
    }
}

#[component]
fn Square(entry: Entry, selection: String) -> Element {
    let is_group = entry.tab == Tab::Kind(Kind::Group);
    let on = is_group && is_selected(&selection, &entry.name);
    let playhead = use_playhead();
    let class = match (entry.missing, on, entry.favourite, entry.css.is_some()) {
        (true, _, _, _) => "sq missing",
        (_, true, _, _) => "sq on",
        (_, _, true, true) => "sq colour fav",
        (_, _, true, false) => "sq fav",
        (_, _, false, true) => "sq colour",
        _ => "sq",
    };
    let style = entry
        .css
        .as_ref()
        .map(|css| format!("background: {css};"))
        .unwrap_or_default();
    let tap_entry = entry.clone();
    let hover_name = entry.name.clone();
    let group_name = entry.name.clone();
    rsx! {
        button {
            class: "{class}",
            style: "{style}",
            title: "{entry.about}",
            onmouseenter: move |_| if is_group { send(Command::HighlightGroup(Some(hover_name.clone()))) },
            onmouseleave: move |_| if is_group { send(Command::HighlightGroup(None)) },
            onpointerdown: move |e| {
                if is_group && !tap_entry.missing {
                    let shift = e.data.modifiers().contains(Modifiers::SHIFT);
                    send(Command::Select(pick(&selection, &group_name, shift)));
                } else {
                    crate::library::tap(&tap_entry, &playhead());
                }
            },
            span { class: "sq-name", "{entry.name}" }
        }
    }
}

/// Tricks, bundles, macros: tiles with a second line.
#[component]
pub fn TilesPane(tab: Tab, surface: Surface) -> Element {
    let query = use_signal(String::new);
    let tiles = tiles_of(tab, &surface, &query());
    let class = format!("pane pane-{}", tab.label().to_lowercase());
    rsx! {
        section { class: "{class}",
            PaneHead { title: tab.label().to_string(), query }
            div { class: "pane-grid",
                for entry in tiles.iter().cloned() {
                    PaneTile { key: "{entry.name}", entry }
                }
                if tiles.is_empty() {
                    span { class: "lib-empty", "nothing matches" }
                }
            }
        }
    }
}

/// One tile: the name, a second line, a swatch where there is one.
/// Lit from the playhead. No star: favourites are data here.
#[component]
pub fn PaneTile(entry: Entry) -> Element {
    let playhead = use_playhead();
    let on = crate::library::is_on(&entry, &playhead());
    let class = match (entry.missing, on, entry.favourite) {
        (true, _, _) => "lib-tile missing",
        (_, true, _) => "lib-tile on",
        (_, _, true) => "lib-tile fav",
        _ => "lib-tile",
    };
    let swatch = entry.css.clone();
    let tap_entry = entry.clone();
    rsx! {
        div { class: "{class}", title: "{entry.about}",
            button {
                class: "lib-body",
                onpointerdown: move |_| crate::library::tap(&tap_entry, &playhead()),
                if let Some(css) = swatch {
                    span { class: "lib-swatch", style: "background: {css}" }
                }
                span { class: "lib-name", "{entry.name}" }
                if !entry.family.is_empty() {
                    span { class: "lib-family", "{entry.family}" }
                }
            }
        }
    }
}

/// Effects: a table — name, family, the note — favourites first, the
/// playing ones lit; a row fires or lets go.
#[component]
pub fn EffectsPane(surface: Surface) -> Element {
    let query = use_signal(String::new);
    let playhead = use_playhead();
    let rows = tiles_of(Tab::Kind(Kind::Effect), &surface, &query());
    rsx! {
        section { class: "pane pane-effects",
            PaneHead { title: "Effects".to_string(), query }
            div { class: "pane-table",
                for entry in rows.iter().cloned() {
                    {
                        let on = crate::library::is_on(&entry, &playhead());
                        let class = match (on, entry.favourite) {
                            (true, _) => "row on",
                            (_, true) => "row fav",
                            _ => "row",
                        };
                        let tap_entry = entry.clone();
                        rsx! {
                            button {
                                key: "{entry.name}",
                                class: "{class}",
                                onpointerdown: move |_| crate::library::tap(&tap_entry, &playhead()),
                                span { class: "row-name", "{entry.name}" }
                                span { class: "row-family", "{entry.family}" }
                                span { class: "row-note", "{entry.about}" }
                            }
                        }
                    }
                }
                if rows.is_empty() {
                    span { class: "lib-empty", "nothing matches" }
                }
            }
        }
    }
}

/// Looks: a list, each row a rendered preview of the look, its name
/// and its kind; the held one lit.
// r[impl playback.look-hold] - tap latches, tap again lets go
#[component]
pub fn LooksPane(surface: Surface) -> Element {
    let query = use_signal(String::new);
    let rows = tiles_of(Tab::Kind(Kind::Look), &surface, &query());
    rsx! {
        section { class: "pane pane-looks",
            PaneHead { title: "Looks".to_string(), query }
            div { class: "pane-list",
                for entry in rows.iter().cloned() {
                    LookRow { key: "{entry.name}", entry }
                }
                if rows.is_empty() {
                    span { class: "lib-empty", "nothing matches" }
                }
            }
        }
    }
}

#[component]
fn LookRow(entry: Entry) -> Element {
    let playhead = use_playhead();
    // The preview is read once per mount, not per frame.
    let preview = use_hook(|| preview_data_uri(&entry.name));
    let on = crate::library::is_on(&entry, &playhead());
    let class = match (on, entry.favourite) {
        (true, _) => "look-row on",
        (_, true) => "look-row fav",
        _ => "look-row",
    };
    let kind = entry.family.clone();
    let kind_css = look_kind_css(&kind);
    let tap_entry = entry.clone();
    rsx! {
        button {
            class: "{class}",
            title: "{entry.about}",
            onpointerdown: move |_| crate::library::tap(&tap_entry, &playhead()),
            match &preview {
                Some(uri) => rsx! { img { class: "look-thumb", src: "{uri}" } },
                None => rsx! { span { class: "look-thumb empty", "no preview" } },
            }
            span { class: "look-row-name", "{entry.name}" }
            span { class: "look-badge", style: "border-color: {kind_css}; color: {kind_css}", "{kind}" }
        }
    }
}

fn look_kind_css(kind: &str) -> &'static str {
    match kind {
        "bed" => crate::library::look_css(LookKind::Bed),
        "full" => crate::library::look_css(LookKind::Full),
        "punt" => crate::library::look_css(LookKind::Punt),
        "safe" => crate::library::look_css(LookKind::Safe),
        _ => "#6a6a78",
    }
}

/// The path a look's preview is rendered to.
pub fn preview_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(PREVIEW_DIR).join(format!("{name}.png"))
}

/// The look's preview as a `data:` URI, if it has been rendered. Read
/// at run time so a re-render shows without a rebuild; a browser has
/// no disk and gets `None`.
pub fn preview_data_uri(name: &str) -> Option<String> {
    let bytes = std::fs::read(preview_path(name)).ok()?;
    Some(format!("data:image/png;base64,{}", base64(&bytes)))
}

/// Standard base64, unpadded input, padded output. Tiny, so the UI
/// crate does not take a dependency for one thumbnail.
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The fader row for the console's bottom band: the pageable bank with
/// its page tabs, key modes and tap masters, the masters beside it.
// r[impl studio.views.seven-busking-features] - pages, filters, speed, params on the row
#[component]
pub fn FadersPane() -> Element {
    rsx! {
        section { class: "pane pane-faders",
            div { class: "faders-row",
                FaderBank { track: PANE_TRACK }
                Masters { track: PANE_TRACK }
            }
        }
    }
}

/// The venue's desk banks as a pane.
// r[impl studio.live.desk-scenes]
#[component]
pub fn DeskPane(banks: Vec<crate::desk::Bank>) -> Element {
    rsx! {
        section { class: "pane pane-desk",
            if banks.is_empty() {
                span { class: "lib-empty", "this venue came with no desk show" }
            } else {
                crate::live::DeskBanks { banks }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r[verify studio.program.pick-and-gizmos]
    #[test]
    fn a_plain_pick_replaces_and_shift_adds_or_removes() {
        assert_eq!(
            pick("Washers", "Spots", false),
            Selection::Group("Spots".into())
        );
        assert_eq!(
            pick("Washers", "Spots", true),
            Selection::Union(vec![
                Selection::Group("Washers".into()),
                Selection::Group("Spots".into())
            ])
        );
        // Shift on a selected group takes it back out.
        assert_eq!(
            pick("Washers + Spots", "spots", true),
            Selection::Group("Washers".into())
        );
        // Nothing selected: shift is a plain pick.
        assert_eq!(pick("", "Spots", true), Selection::Group("Spots".into()));
        // A role in the description goes back as a role.
        assert_eq!(
            pick("role Key", "Spots", true),
            Selection::Union(vec![
                Selection::Role("Key".into()),
                Selection::Group("Spots".into())
            ])
        );
        assert!(is_selected("Washers + Spots", "spots"));
        assert!(!is_selected("Washers + Spots", "Movers"));
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0x89, b'P', b'N', b'G']), "iVBORw==");
    }

    /// The four shipped looks have previews on disk, and they read as
    /// PNG data URIs.
    #[test]
    fn the_shipped_looks_have_previews() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        for name in ["blackout", "chorus full", "punt", "verse bed"] {
            let path = std::path::Path::new(root).join(preview_path(name));
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("{}: {e} — run `just look-previews`", path.display()));
            assert_eq!(&bytes[..4], &[0x89, b'P', b'N', b'G'], "{name}");
        }
        let uri = format!(
            "data:image/png;base64,{}",
            base64(&[0x89, b'P', b'N', b'G'])
        );
        assert!(uri.starts_with("data:image/png;base64,iVBOR"));
    }

    /// The pane's track and the console band agree: the tallest fader
    /// column fits the band a 1440-high window gives it.
    /// r[verify studio.dock.no-scroll]
    #[test]
    fn the_fader_pane_is_shorter_than_the_console_band() {
        // tab 18 + padding 4 + header 44 + 8 + badges 18 + 6 + track +
        // 4 + label 14 + value 12 + 6 + param 42 + 6 + key 44
        let tallest = 18.0
            + 4.0
            + 44.0
            + 8.0
            + 18.0
            + 6.0
            + PANE_TRACK
            + 4.0
            + 14.0
            + 12.0
            + 6.0
            + 42.0
            + 6.0
            + 44.0;
        let band = (1440.0 - 28.0 - 2.0) * 0.27;
        assert!(tallest <= band, "{tallest} > {band}");
    }
}
