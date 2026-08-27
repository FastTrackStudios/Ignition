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
//! The window never grows a scrollbar — a leaf clips and a grid wraps —
//! but a pane's own content scrolls inside its leaf, because a library
//! of a hundred and thirty-one effects does not fit by wrapping.

// r[impl studio.dock.no-scroll] - the leaf clips; the content inside it scrolls
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

/// The effects grid's geometry, so the pane can work out which cards
/// are on screen without measuring anything. Kept in step with
/// `live.css`: `.pane-effects .card` is a quarter of the row wide and
/// `.card` is 116px tall with a 6px gap under it.
const CARD_COLUMNS: usize = 4;
const CARD_ROW: f64 = 122.0;
/// Taller than the pane really is on a 1440 monitor, deliberately: the
/// cost of animating a card that turns out to be just off screen is one
/// image swap, and the cost of stopping one that is on screen is a
/// thumbnail that visibly freezes.
const CARDS_VIEWPORT: f64 = 1200.0;
/// How fast a turning thumbnail steps, and how lazily a still one
/// checks whether it has scrolled into view. Twelve-and-a-half frames a
/// second is enough to read a chase; the idle poll only has to beat a
/// human scrolling.
const FRAME_MS: u64 = 80;
const IDLE_MS: u64 = 400;

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
    #[props(default)] note: String,
    query: Signal<String>,
    #[props(default)] clear: bool,
    #[props(default)] children: Element,
) -> Element {
    let mut query = query;
    rsx! {
        header { class: "pane-head",
            // No title here: the tab above the pane already names it,
            // and repeating it spends a line of a pane that does not
            // scroll on a word the eye has just read.
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
            {children}
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
            PaneHead { note, query, clear: is_groups }
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
    // Two shapes, not one. A swatch tile is a disc with its name on it;
    // a name-only tile — a group, a focus point — is all text, and its
    // text wants every pixel of the tile rather than a line along the
    // bottom of an empty square.
    let mut class = String::from("sq");
    if entry.css.is_some() {
        class.push_str(" colour");
        if entry.light {
            class.push_str(" light");
        }
    } else {
        class.push_str(" label");
    }
    if entry.missing {
        class.push_str(" missing");
    } else if on {
        class.push_str(" on");
    } else if entry.favourite {
        class.push_str(" fav");
    }
    // The colour is a disc on the tile, not the tile itself. A round
    // swatch reads as a *colour* rather than as a coloured button, it
    // is the shape a palette's wedges want to be drawn in, and it
    // leaves the name somewhere to sit that is not on top of the thing
    // it is naming.
    let disc = entry.css.clone();
    let tap_entry = entry.clone();
    let hover_name = entry.name.clone();
    let group_name = entry.name.clone();
    rsx! {
        button {
            class: "{class}",
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
            if let Some(css) = disc {
                span { class: "sq-disc", style: "background: {css}",
                    span { class: "sq-name", "{entry.name}" }
                }
            } else {
                span { class: "sq-name", "{entry.name}" }
            }
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
            PaneHead { query }
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
pub fn EffectsPane(surface: Surface, #[props(default)] only: EffectKinds) -> Element {
    let query = use_signal(String::new);
    // Every card turning at once, or only the one under the pointer.
    // On by default: the library is a wall of moving light and you find
    // what you want by looking, rather than by hovering a hundred and
    // thirty-one names in turn. It stays affordable because only the
    // cards actually on screen turn — see the band below — so the
    // switch is there for a slow machine, not for the common case.
    let mut play_all = use_signal(|| true);
    // Where the grid is scrolled to. Play-all means *every card on
    // screen*, not every card: a hundred and thirty-one loops turning
    // at once is the whole library decoded and held at once, and the
    // hundred of them scrolled out of sight cost exactly as much as the
    // thirty you can see while showing you nothing.
    let mut scrolled = use_signal(|| 0.0f64);
    let rows: Vec<Entry> = tiles_of(only.tab(), &surface, &query())
        .into_iter()
        .filter(|e| only.wants(&e.family))
        .collect();
    // The band of indices on screen, from the grid's own geometry: four
    // to a row (`live.css` pins it), a card and its gap 122px tall. A
    // row of slack each way so a card is already running by the time it
    // is scrolled into view rather than starting when it arrives.
    rsx! {
        section { class: "pane {only.css()}",
            PaneHead { query,
                button {
                    class: if play_all() { "pane-key on" } else { "pane-key" },
                    title: "play every thumbnail at once",
                    onclick: move |_| play_all.set(!play_all()),
                    "PLAY ALL"
                }
            }
            div { class: "cards",
                onscroll: move |e| scrolled.set(e.data.scroll_top()),
                for (i, entry) in rows.iter().cloned().enumerate() {
                    EffectRow {
                        key: "{entry.name}",
                        entry,
                        play_all,
                        index: i,
                        scrolled,
                        dir: only.preview_dir(),
                    }
                }
                if rows.is_empty() {
                    span { class: "lib-empty", "nothing matches" }
                }
            }
        }
    }
}

/// Which effects a pane shows.
///
/// The library is a hundred and thirty-one entries and they are not one
/// kind of thing: what a movement effect does is where the beams go,
/// what the rest do is what the rig does. Split across panes they are
/// two short lists you can scan; mixed, they are one long one you
/// cannot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EffectKinds {
    /// Everything that is not movement — intensity, colour, beam, strip.
    #[default]
    Rig,
    /// Movement only.
    Movement,
    /// Not effects at all: the profile's macros, which are little
    /// programmes rather than templates but read as the same kind of
    /// card and fire the same way.
    Macros,
}

impl EffectKinds {
    /// `family` is the note's family, as `catalogue` puts it on the
    /// entry: "movement", "intensity", "colour", "beam", "strip",
    /// "one-shot". An effect with no note at all lands in `Rig` rather
    /// than nowhere.
    /// The pane's class, which is what pins its column count.
    pub fn css(self) -> &'static str {
        match self {
            EffectKinds::Rig => "pane-effects",
            EffectKinds::Movement => "pane-movers",
            EffectKinds::Macros => "pane-macros",
        }
    }

    /// Which catalogue the pane lists.
    fn tab(self) -> Tab {
        match self {
            EffectKinds::Macros => Tab::Kind(Kind::Macro),
            _ => Tab::Kind(Kind::Effect),
        }
    }

    /// Where its loops were rendered.
    fn preview_dir(self) -> &'static str {
        match self {
            EffectKinds::Macros => crate::library::MACRO_PREVIEW_DIR,
            _ => crate::library::EFFECT_PREVIEW_DIR,
        }
    }

    fn wants(self, family: &str) -> bool {
        let moves = family == "movement";
        match self {
            EffectKinds::Rig => !moves,
            EffectKinds::Movement => moves,
            // A macro pane lists macros; there is nothing to filter.
            EffectKinds::Macros => true,
        }
    }
}

/// One effect: a thumbnail that comes alive under the pointer, the
/// name, and — while it is hovered — what the effect is for.
///
/// The frames only turn while the pointer is on the row. Every row
/// animating at once would be the whole library decoded and held in
/// memory (a 320×180 frame is 225KB *decoded*, so a hundred effects at
/// sixteen frames is most of a gigabyte); one row at a time is a few
/// megabytes, and reads as though the whole library were alive.
// r[impl studio.views.whole-profile] - the library shows what each entry does
#[component]
fn EffectRow(
    entry: Entry,
    play_all: Signal<bool>,
    index: usize,
    scrolled: Signal<f64>,
    dir: &'static str,
) -> Element {
    let playhead = use_playhead();
    let mut hovered = use_signal(|| false);
    let mut frame = use_signal(|| 0usize);
    // Whether this row is in the scrolled viewport, derived from the
    // shared offset rather than handed down, so the loop below can watch
    // it change.
    let visible = use_memo(move || {
        let top = scrolled();
        let row = (index / CARD_COLUMNS) as f64 * CARD_ROW;
        row + CARD_ROW > top - CARD_ROW && row < top + CARDS_VIEWPORT
    });
    // The frames are loaded when the row comes into view and dropped
    // when it leaves.
    //
    // Loading them all at mount is what made opening this pane cost
    // about as much as the visualizer: a hundred and thirty-one rows,
    // sixteen frames each, base64 into a string — a hundred and seventy
    // megabytes of them built before a single card had animated, and up
    // to half a gigabyte of texture if the renderer touched them all.
    // Held to what is on screen it is a few megabytes, and scrolling
    // pays for a screenful at a time.
    let mut frames = use_signal(Vec::<String>::new);
    let name = entry.name.clone();
    let count = frames.read().len();

    // While hovered, step the loop. `use_future` is cancelled and
    // restarted by the signal, so nothing ticks over a cold row.
    use_future(move || {
        let name = name.clone();
        async move {
            loop {
                // A row that is not animating still has to notice when
                // it scrolls into view, but it can do that lazily. The
                // fast tick is only for rows actually turning: at 80ms
                // across a hundred and thirty-one rows this loop alone
                // was sixteen hundred wakeups a second, most of them
                // for cards nobody could see.
                let turning = visible() && (hovered() || play_all());
                let wait = if turning { FRAME_MS } else { IDLE_MS };
                futures_timer::Delay::new(std::time::Duration::from_millis(wait)).await;
                let here = visible();
                let loaded = !frames.read().is_empty();
                if here && !loaded {
                    frames.set(crate::library::frames_in(dir, &name));
                } else if !here && loaded {
                    frames.set(Vec::new());
                    frame.set(0);
                }
                let count = frames.read().len();
                // Hovering always plays: the pointer is on it, so it
                // is on screen by definition, and the arithmetic above
                // is an estimate rather than a fact.
                if turning && count > 1 {
                    frame.set((frame() + 1) % count);
                } else if frame() != 0 {
                    frame.set(0);
                }
            }
        }
    });

    let on = crate::library::is_on(&entry, &playhead());
    let class = match (on, entry.favourite) {
        (true, _) => "card on",
        (_, true) => "card fav",
        _ => "card",
    };
    let shown = frames
        .read()
        .get(frame().min(count.saturating_sub(1)))
        .cloned();
    let tap_entry = entry.clone();
    let family = entry.family.clone();
    rsx! {
        // The same card a look gets — the picture is the tile, the name
        // on it, the badge on the right. The one difference is that this
        // picture moves.
        button {
            class: "{class}",
            onmouseenter: move |_| hovered.set(true),
            onmouseleave: move |_| hovered.set(false),
            onpointerdown: move |_| crate::library::tap(&tap_entry, &playhead()),
            match shown {
                Some(uri) => rsx! { img { class: "card-thumb", src: "{uri}" } },
                None => rsx! { span { class: "card-thumb empty", "no preview" } },
            }
            div { class: "card-marks",
                if !family.is_empty() {
                    span { class: "card-badge fx", "{family}" }
                }
            }
            span { class: "card-name", "{entry.name}" }
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
            PaneHead { query }
            div { class: "cards",
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
        (true, _) => "card on",
        (_, true) => "card fav",
        _ => "card",
    };
    let kind = entry.family.clone();
    let kind_css = look_kind_css(&kind);
    let tap_entry = entry.clone();
    rsx! {
        // The picture *is* the card. A look is chosen by recognising it,
        // so the thumbnail gets the whole tile and everything else sits
        // on top of it: the name along the bottom, the kind and what the
        // look does down the right-hand edge.
        button {
            class: "{class}",
            title: "{entry.about}",
            onpointerdown: move |_| crate::library::tap(&tap_entry, &playhead()),
            match &preview {
                Some(uri) => rsx! { img { class: "card-thumb", src: "{uri}" } },
                None => rsx! { span { class: "card-thumb empty", "no preview" } },
            }
            div { class: "card-marks",
                span { class: "card-badge", style: "border-color: {kind_css}; color: {kind_css}", "{kind}" }
                for (glyph, means) in entry.marks.iter() {
                    span { key: "{glyph}", class: "card-mark", title: "{means}", "{glyph}" }
                }
            }
            span { class: "card-name", "{entry.name}" }
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

/// The look's preview as a `file:` URL, if it has been rendered.
/// Checked at run time so a re-render shows without a rebuild; a
/// browser has no disk and gets `None`.
pub fn preview_data_uri(name: &str) -> Option<String> {
    file_uri(&preview_path(name))
}

/// A local path as a `file:` URL — the *short* way to put a picture in
/// the DOM.
///
/// The obvious way is a `data:` URI, and it was what this did. It cost
/// twenty-seven per cent of the studio's CPU. An `img src` is parsed as
/// a URL every time it is set, by the `url` crate, character by
/// character — and a base64 PNG is two hundred kilobytes of characters.
/// The image cache is keyed on the *parsed* string, so even a cache hit
/// paid the parse in full, and the thumbnail loops re-set `src` twelve
/// times a second per animating row. Sixty bytes of `file:` URL parse
/// in no time at all, hash in no time at all, and Blitz then caches the
/// decoded image against that short key — so a frame is read and
/// decoded once for the life of the process instead of on every turn of
/// the loop.
///
/// `None` when the file is not there, which is the same answer as "no
/// preview" and needs no special case — including on wasm, where there
/// is no disk to look at.
pub fn file_uri(path: &std::path::Path) -> Option<String> {
    let absolute = std::fs::canonicalize(path).ok()?;
    let mut out = String::from("file://");
    // Percent-encode what a path may hold and a URL may not. The
    // preview directories are slugs, but the *repository* can live
    // anywhere — a checkout under "My Documents" would otherwise
    // produce a URL that parses to the wrong path, silently.
    for byte in absolute.to_str()?.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    Some(out)
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
    /// Play-all plays what is on screen, not the whole library. The
    /// band is computed from the grid's own geometry, so this pins the
    /// arithmetic against the constants `live.css` also encodes.
    #[test]
    fn play_all_covers_the_visible_band_and_not_the_rest() {
        let band = |top: f64| {
            let first_row = ((top / CARD_ROW) as usize).saturating_sub(1);
            let last_row = ((top + CARDS_VIEWPORT) / CARD_ROW) as usize + 1;
            (first_row * CARD_COLUMNS, (last_row + 1) * CARD_COLUMNS)
        };
        // At the top: from the first card, and far short of 131.
        let (first, last) = band(0.0);
        assert_eq!(first, 0);
        assert!(last < 131, "the whole library would be playing: {last}");
        assert!(last >= 40, "less than a screenful would be playing: {last}");

        // Scrolled down: the band moves with it and stays a bandful.
        // Not the *same* size — at the top it is clipped against row
        // zero — but the same order of size.
        let (first_down, last_down) = band(2000.0);
        assert!(first_down > 0, "the band never left the top");
        let span = last_down - first_down;
        assert!(
            (40..=60).contains(&span),
            "the band is {span} cards, which is not a screenful"
        );
        // And a card well above the fold is not in it.
        assert!(
            first_down > CARD_COLUMNS,
            "a card scrolled away is still playing"
        );
    }

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

    /// The four shipped looks have previews on disk, and they reach the
    /// DOM as short `file:` URLs.
    ///
    /// The length is the assertion, not a detail of it. These used to be
    /// base64 `data:` URIs and the studio spent twenty-seven per cent of
    /// its CPU in the URL parser as a result — see
    /// [`super::file_uri`]. A preview that goes back to being inlined
    /// would look completely correct and cost a quarter of the frame.
    #[test]
    fn the_shipped_looks_have_previews() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        for name in ["blackout", "chorus full", "punt", "verse bed"] {
            let path = std::path::Path::new(root).join(preview_path(name));
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("{}: {e} — run `just look-previews`", path.display()));
            assert_eq!(&bytes[..4], &[0x89, b'P', b'N', b'G'], "{name}");

            let uri = file_uri(&path).expect("the file is right there");
            assert!(uri.starts_with("file:///"), "{name}: {uri}");
            assert!(uri.ends_with(".png"), "{name}: {uri}");
            assert!(
                uri.len() < 1024,
                "{name}: a {} byte src is an inlined image, not a reference",
                uri.len()
            );
            // The space in "chorus full" has to survive as a URL.
            assert!(!uri.contains(' '), "{name}: {uri}");
        }
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
