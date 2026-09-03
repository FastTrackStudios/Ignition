//! The Live view: running a night.
//!
//! Big touch targets, no destructive editing, everything reachable in
//! one tap. Left is the scene side — the profile's looks by kind, the
//! macros with their beat length, and the venue's desk banks where it
//! came with a console show. Centre is the fader bank on its pages,
//! every fader wearing its clock and filter and carrying its parameter
//! sliders inline, with the tap masters under it. Right is the palettes
//! (favourites first), the protected-role toggles and the grand master.
//!
//! Nothing here remembers a lighting value: a control draws what the
//! playhead says and sends what the hand did (`r[studio.one-truth]`).
//! The seven busking features of the profile are controls, not
//! settings (`r[studio.views.seven-busking-features]`).

// r[impl studio.views] - the Live view, and the Live/Program switch
// r[impl studio.views.seven-busking-features] - looks, macros, pages, filters, protection, speed, params
// r[impl studio.live.desk-scenes] - the desk banks beside the looks
// r[impl studio.touch] - 44px targets, press not release, wide grabs
// r[impl studio.labels] - every fader and key label is eight characters or fewer

use crate::command::{Command, PageMove, Playhead, SpeedKey};
use crate::library::{self, Library, css_of, look_css, macro_shape, use_operator};
use crate::operators::Kind;
use crate::{Bootstrap, HSlider, Surface, send, use_desk, use_playhead};
use dioxus::prelude::*;
use ignition_core::profile::LookKind;
use ignition_core::{AttrFilter, Selection, Speed};

/// The styles for every panel in this family. The studio includes it
/// beside `studio.css`; the web app includes it beside its own base.
/// The palette every sheet draws from — see `tokens.css`.
///
/// Injected before any other stylesheet by both the studio and the Live
/// page. Order is not strictly required (custom properties resolve at
/// computed-value time, not parse time) but putting the definitions
/// first is how the cascade reads.
pub const TOKENS_CSS: &str = include_str!("tokens.css");

pub const LIVE_CSS: &str = include_str!("live.css");

/// The two views of a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Program,
    Live,
}

/// A label that fits a strip, a fader and a touch key alike: eight
/// characters, upper-case, the rest cut.
// r[impl studio.labels] - eight characters or fewer
// r[impl profile.pages.label-fits]
#[must_use]
pub fn label8(name: &str) -> String {
    name.chars().take(8).collect::<String>().to_uppercase()
}

/// Which clock a fader follows, as the badge says it: Song, Tap, Tap ½,
/// Tap ×2 — or its own rate where the recipe is not slaved.
// r[impl profile.speed-routing] - every fader shows its clock
#[must_use]
pub fn clock_badge(speed: Option<&Speed>) -> String {
    match speed {
        Some(Speed::Master(m)) => m.clone(),
        Some(Speed::Scaled { master, scale }) => {
            if (*scale - 0.5).abs() < 1e-3 {
                format!("{master} ½")
            } else if (*scale - 2.0).abs() < 1e-3 {
                format!("{master} ×2")
            } else {
                format!("{master} ×{scale}")
            }
        }
        Some(_) => "own".into(),
        None => "—".into(),
    }
}

/// A filter as a badge — empty when the fader passes everything.
// r[impl profile.attribute-filter] - the filter shows on the fader
#[must_use]
pub fn filter_badge(filter: &AttrFilter) -> String {
    if *filter == AttrFilter::ALL {
        return String::new();
    }
    let mut parts = Vec::new();
    if filter.intensity {
        parts.push("INT");
    }
    if filter.colour {
        parts.push("COL");
    }
    if filter.position {
        parts.push("POS");
    }
    if filter.beam {
        parts.push("BEAM");
    }
    parts.join("+")
}

/// A fader drawn from the playhead's level rather than its own memory.
///
/// The same div construction as `main.rs`'s `Fader` — a range input is
/// a bare bar in Blitz and too small for a thumb — but the level is a
/// prop: what the engine has is what is drawn, so a fader moved from a
/// remote or another window moves here too.
// r[impl studio.one-truth] - level from the playhead, not remembered
// r[impl studio.touch] - a wide grab and a tall track
#[component]
#[expect(
    clippy::float_cmp,
    reason = "`v` is `local`'s own value re-derived from the same latch; \
              exact equality is the right test for \"nothing moved\""
)]
pub fn TouchFader(
    label: String,
    css: String,
    level: f32,
    #[props(default)] latched: bool,
    #[props(default)] toggled: bool,
    /// The track's height in CSS pixels. The Live view's default is
    /// `.tfader .ttrack` in live.css; a pane in a shorter band passes
    /// its own, and the track is sized inline to match.
    #[props(default = 220.0)]
    track: f32,
    on_change: EventHandler<f32>,
) -> Element {
    // Pressed on the track, the fader follows the window's pointer
    // until the release — wherever the pointer went — moving by how far
    // the hand moved, and shows its own value rather than the
    // playhead's until then, so the engine echoing an older level back
    // cannot fight the hand.
    let mut latch = use_signal(|| Option::<crate::pointer::Latch>::None);
    let mut local = use_signal(|| level);
    let feed = crate::pointer::use_pointer_feed();
    use_effect(move || {
        let Some(l) = latch() else {
            return;
        };
        let p = feed();
        if crate::pointer::released(&l, &p) {
            latch.set(None);
            return;
        }
        let v = crate::pointer::drag_up(&l, p.y, track);
        if v != *local.peek() {
            local.set(v);
            on_change.call(v);
        }
    });
    let shown = if latch().is_some() { local() } else { level };
    let class = match (latched, toggled) {
        (true, _) => "tfader latched",
        (_, true) => "tfader toggled",
        _ => "tfader",
    };
    rsx! {
        div { class: "{class}",
            div {
                class: "ttrack",
                style: "height: {track}px",
                onpointerdown: move |e| {
                    let p = e.data.client_coordinates();
                    local.set(level);
                    latch.set(Some(crate::pointer::Latch {
                        at: (crate::pointer::coord(p.x), crate::pointer::coord(p.y)),
                        level,
                        ups: feed.peek().ups,
                    }));
                },
                div { class: "tfill", style: "height: {shown * 100.0}%; background: {css}" }
                div { class: "thandle", style: "bottom: {shown * 100.0}%; border-color: {css}" }
            }
            span { class: "tlabel", "{label8(&label)}" }
            span { class: "tvalue", "{(shown * 100.0) as u32}" }
        }
    }
}

/// The looks bank: the profile's looks, coloured by kind, the held one
/// drawn held. Favourites first, then the rest, so an operator who has
/// starred four looks sees those four at the top.
// r[impl playback.look-hold] - tap latches, tap again lets go
#[component]
pub fn LooksBank() -> Element {
    let playhead = use_playhead();
    let operator = use_operator();
    let profile = crate::library::profile();
    let all: Vec<(String, LookKind)> = profile
        .looks
        .iter()
        .map(|(n, l)| (n.clone(), l.kind))
        .collect();
    let favs = operator().favourites.looks;
    let looks = favs
        .iter()
        .filter_map(|f| {
            all.iter()
                .find(|(n, _)| n == f)
                .map(|(n, k)| (n.clone(), *k, true))
        })
        .chain(
            all.iter()
                .filter(|(n, _)| !favs.contains(n))
                .map(|(n, k)| (n.clone(), *k, false)),
        );
    let held = playhead().held_look;
    rsx! {
        div { class: "live-block looks",
            header { "Looks" }
            div { class: "look-grid",
                for (name, kind, fav) in looks {
                    {
                        let on = held.as_deref() == Some(&name);
                        let css = look_css(kind);
                        let name_for_tap = name.clone();
                        let bg = if on { css } else { "#1b1b22" };
                        let kind_word = format!("{kind:?}").to_lowercase();
                        let star = if fav { " ★" } else { "" };
                        rsx! {
                            button {
                                key: "{name}",
                                class: if on { "look-key held" } else { "look-key" },
                                style: "border-color: {css}; background: {bg}",
                                title: "{kind_word} look",
                                onpointerdown: move |_| {
                                    if on {
                                        send(Command::Look(None));
                                    } else {
                                        send(Command::Look(Some(name_for_tap.clone())));
                                    }
                                },
                                span { class: "look-name", "{label8(&name)}" }
                                span { class: "look-kind", "{kind_word}{star}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The macros row: one press runs the move; the key says how many
/// beats it takes and whether it lets go at the end.
// r[impl playback.macro-runner]
#[component]
pub fn MacrosRow() -> Element {
    let profile = crate::library::profile();
    rsx! {
        div { class: "live-block macros",
            header { "Macros" }
            div { class: "macro-row",
                for (name, m) in profile.macros.iter() {
                    {
                        let (beats, releases) = macro_shape(m);
                        let name = name.clone();
                        let fire = name.clone();
                        rsx! {
                            button {
                                key: "{name}",
                                class: "macro-key",
                                title: "{m.about}",
                                onpointerdown: move |_| send(Command::Macro(fire.clone())),
                                span { class: "macro-name", "{label8(&name)}" }
                                span { class: "macro-shape",
                                    if beats > 0.0 { "{beats as u32} beats" } else { "now" }
                                    if releases { " · releases" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The venue's desk banks, collapsed by bank. The scene the Show-class
/// playback stands on is lit; DESK OFF folds the playback out.
// r[impl studio.live.desk-scenes]
#[component]
pub fn DeskBanks(banks: Vec<crate::desk::Bank>) -> Element {
    let playhead = use_playhead();
    let mut open = use_signal(|| Option::<String>::None);
    if banks.is_empty() {
        return rsx! {};
    }
    let on = playhead().desk_scene;
    rsx! {
        div { class: "live-block desk",
            header {
                span { "Desk" }
                button {
                    class: if on.is_some() { "desk-off on" } else { "desk-off" },
                    onpointerdown: move |_| send(Command::DeskRelease),
                    "DESK OFF"
                }
            }
            for bank in banks.iter().cloned() {
                {
                    let is_open = open().as_deref() == Some(&bank.name);
                    let holds_current = on.is_some_and(|i| bank.scenes.iter().any(|s| s.index == i));
                    let bank_name = bank.name.clone();
                    rsx! {
                        div { key: "{bank.name}", class: "desk-bank",
                            button {
                                class: if holds_current { "desk-bank-head on" } else { "desk-bank-head" },
                                onpointerdown: move |_| {
                                    open.set(if is_open { None } else { Some(bank_name.clone()) });
                                },
                                span { "{bank.name}" }
                                span { class: "desk-count", "{bank.scenes.len()}" }
                            }
                            if is_open {
                                div { class: "desk-scenes",
                                    for scene in bank.scenes.iter().cloned() {
                                        button {
                                            key: "{scene.index}",
                                            class: if on == Some(scene.index) { "desk-scene on" } else { "desk-scene" },
                                            onpointerdown: move |_| send(Command::DeskScene(scene.index)),
                                            "{scene.name}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The fader bank on its pages: tabs from `Profile.pages`, pickup state
/// per fader, and every fader wearing its clock and filter with its
/// parameters inline.
// r[impl profile.pages] - the page selector is the profile's pages
// r[impl profile.effect-parameters] - secondary sliders inline, touch-sized
#[component]
pub fn FaderBank(#[props(default = 220.0)] track: f32) -> Element {
    let desk = use_desk();
    let playhead = use_playhead();
    let mut key_mode = use_signal(|| ignition_core::KeyAction::Flash);
    let pages = crate::faders::bank_pages();
    let names = crate::faders::page_names();
    let profile = crate::library::profile();
    let current = desk().page.min(pages.len().saturating_sub(1));
    rsx! {
        div { class: "live-block bank",
            header {
                span { "Faders" }
                div { class: "page-tabs",
                    for (i, name) in names.iter().enumerate() {
                        button {
                            key: "{i}",
                            class: if current == i { "page-tab on" } else { "page-tab" },
                            onpointerdown: move |_| send(Command::Page(PageMove::Set(i))),
                            "{label8(name)}"
                        }
                    }
                }
                div { class: "key-modes",
                    for (mode, label) in [
                        (ignition_core::KeyAction::Flash, "FLASH"),
                        (ignition_core::KeyAction::Toggle, "TOGGLE"),
                        (ignition_core::KeyAction::Swap, "SWAP"),
                        (ignition_core::KeyAction::Kill, "KILL"),
                        (ignition_core::KeyAction::Temp, "TEMP"),
                    ] {
                        button {
                            key: "{label}",
                            class: if key_mode() == mode { "key-mode on" } else { "key-mode" },
                            onpointerdown: move |_| key_mode.set(mode),
                            "{label}"
                        }
                    }
                }
            }
            div { class: "tfader-row",
                for i in 0..ignition_core::FADERS {
                    {
                        // The bank is exactly `FADERS` wide by construction
                        // (`faders::the_bank_fills_every_fader_and_no_more`
                        // proves it), but that is not something the
                        // compiler can see through a runtime index — a
                        // slot the bank does not have draws nothing rather
                        // than taking the whole surface down.
                        let slot = pages.get(current).and_then(|page| page.get(i));
                        slot.map_or_else(|| rsx! {}, |spec| {
                            let page_fader = profile.pages.get(current).and_then(|p| p.faders.get(i));
                            let effect = page_fader.and_then(|f| match &f.source {
                                ignition_core::profile::FaderSource::Effect(n) => Some(n.as_str()),
                                _ => None,
                            });
                            let speed = spec
                                .fader
                                .recipe
                                .as_ref()
                                .map(|r| r.timing.speed.clone())
                                .or_else(|| match (effect, page_fader) {
                                    (Some(e), Some(pf)) => Some(profile.speed_for(pf, Some(e))),
                                    _ => None,
                                });
                            let clock = clock_badge(speed.as_ref());
                            let filter = filter_badge(&spec.fader.filter);
                            let params = spec.params.clone();
                            let level = playhead().levels.get(i).copied().unwrap_or(0.0);
                            let latched = desk().latched.get(i).copied().unwrap_or(false);
                            let toggled = desk().toggled.get(i).copied().unwrap_or(false);
                            rsx! {
                                div { key: "{current}-{i}", class: "tslot",
                                    div { class: "badges",
                                        span { class: "badge clock", "{clock}" }
                                        if !filter.is_empty() {
                                            span { class: "badge filter", "{filter}" }
                                        }
                                    }
                                    TouchFader {
                                        label: spec.name.clone(),
                                        css: spec.css.to_string(),
                                        level,
                                        latched,
                                        toggled,
                                        track,
                                        on_change: move |v: f32| send(Command::Level(i, v)),
                                    }
                                    for param in params.iter() {
                                        {
                                            let name = param.name.clone();
                                            let label = name.clone();
                                            let (min, max) = (param.min, param.max);
                                            let span = (max - min).max(f32::EPSILON);
                                            let initial = ((param.default - min) / span).clamp(0.0, 1.0);
                                            rsx! {
                                                div { class: "tparam", key: "{label}",
                                                    span { class: "tparam-name", "{label8(&label)}" }
                                                    HSlider {
                                                        initial,
                                                        on_change: move |v: f32| send(Command::Param {
                                                            index: i,
                                                            name: name.clone(),
                                                            value: v.mul_add(span, min),
                                                        }),
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    button {
                                        class: if toggled { "tkey on" } else { "tkey" },
                                        onpointerdown: move |_| send(Command::Key { index: i, action: key_mode(), down: true }),
                                        onpointerup: move |_| send(Command::Key { index: i, action: key_mode(), down: false }),
                                        onpointerleave: move |_| send(Command::Key { index: i, action: key_mode(), down: false }),
                                        "●"
                                    }
                                }
                            }
                        })
                    }
                }
            }
            TapMasters {}
        }
    }
}

/// The tap masters: learn, half, double, reset, and the readout; SIZE
/// and SPEED beside them.
// r[impl playback.speed-keys] - on the Live surface
#[component]
pub fn TapMasters() -> Element {
    let desk = use_desk();
    rsx! {
        div { class: "tap-masters",
            button { class: "tap-key learn", onpointerdown: move |_| send(Command::Speed(SpeedKey::Tap)), "TAP" }
            button {
                class: if desk().tap_multiplier < 0.99 { "tap-key on" } else { "tap-key" },
                onpointerdown: move |_| send(Command::Speed(SpeedKey::Half)),
                "TAP ½"
            }
            button {
                class: if desk().tap_multiplier > 1.01 { "tap-key on" } else { "tap-key" },
                onpointerdown: move |_| send(Command::Speed(SpeedKey::Double)),
                "TAP ×2"
            }
            button { class: "tap-key", onpointerdown: move |_| send(Command::Speed(SpeedKey::Reset)), "RESET" }
            span { class: "tap-readout",
                if desk().tap_bpm > 0.0 { "{desk().tap_bpm as u32} bpm" } else { "— bpm" }
            }
            div { class: "tparam wide",
                span { class: "tparam-name", "SIZE" }
                HSlider { initial: 1.0, on_change: move |v: f32| send(Command::Size(v)) }
            }
            div { class: "tparam wide",
                span { class: "tparam-name", "SPEED" }
                HSlider { initial: 0.5, on_change: move |v: f32| send(Command::EffectRate(v.mul_add(1.5, 0.5))) }
            }
        }
    }
}

/// Palettes: colours, splits, focus, groups — favourites first — with
/// the swatch on the tile so the pool reads as colour, not words.
// r[impl studio.operators.favourites] - shown first in Live
#[component]
pub fn Palettes(surface: Surface) -> Element {
    let operator = use_operator();
    let profile = crate::library::profile();
    let favs = operator().favourites;
    // Profile colours first — the busking vocabulary — then the venue's
    // own where a name is not in the profile.
    let mut colours: Vec<(String, String)> = profile
        .colors
        .iter()
        .map(|c| (c.name.clone(), css_of(c)))
        .collect();
    for chip in &surface.colors {
        if !colours.iter().any(|(n, _)| n == &chip.name) {
            colours.push((chip.name.clone(), chip.css.clone()));
        }
    }
    let colours = first(colours, &favs.colours);
    let focus = first(
        surface
            .focus
            .iter()
            .map(|f| (f.clone(), String::new()))
            .collect(),
        &favs.focus,
    );
    let groups = first(
        surface
            .groups
            .iter()
            .map(|g| (g.clone(), String::new()))
            .collect(),
        &favs.groups,
    );
    rsx! {
        div { class: "live-block palettes",
            header { "Colour" }
            div { class: "tile-grid",
                for (name, css, fav) in colours {
                    button {
                        key: "c-{name}",
                        class: if fav { "ptile fav" } else { "ptile" },
                        onpointerdown: { let n = name; move |_| send(Command::Color(n.clone())) },
                        span { class: "pdisc", style: "background: {css}" }
                        span { class: "pname", "{name}" }
                    }
                }
                for chip in surface.splits.iter().cloned() {
                    button {
                        key: "s-{chip.name}",
                        class: "ptile split",
                        onpointerdown: { let n = chip.name.clone(); move |_| send(Command::Split(n.clone())) },
                        // The disc, not the bar: a palette's colours belong
                            // round the wheel as wedges, or two palettes that
                            // differ in one colour look identical at this size.
                            span { class: "pdisc", style: "background: {chip.disc()}" }
                        span { class: "pname", "{chip.name}" }
                    }
                }
            }
            header { "Focus" }
            div { class: "tile-grid",
                for (name, _, fav) in focus {
                    button {
                        key: "f-{name}",
                        class: if fav { "ptile fav" } else { "ptile" },
                        onpointerdown: { let n = name; move |_| send(Command::Focus(n.clone())) },
                        span { class: "pname", "{name}" }
                    }
                }
            }
            header { "Groups" }
            div { class: "tile-grid",
                for (name, _, fav) in groups {
                    button {
                        key: "g-{name}",
                        class: if fav { "ptile fav" } else { "ptile" },
                        onpointerdown: { let n = name; move |_| send(Command::Select(Selection::Group(n.clone()))) },
                        span { class: "pname", "{name}" }
                    }
                }
            }
        }
    }
}

/// Favourites first, flagged, then the rest.
fn first(all: Vec<(String, String)>, favs: &[String]) -> Vec<(String, String, bool)> {
    let mut out: Vec<(String, String, bool)> = favs
        .iter()
        .filter_map(|f| {
            all.iter()
                .find(|(n, _)| n == f)
                .map(|(n, c)| (n.clone(), c.clone(), true))
        })
        .collect();
    out.extend(
        all.into_iter()
            .filter(|(n, _)| !favs.contains(n))
            .map(|(n, c)| (n, c, false)),
    );
    out
}

/// Protected roles as a state on the roles, with the toggle here:
/// protection is an operator decision, not a preference.
// r[impl profile.protected-roles] - toggled on the Live surface
#[component]
pub fn ProtectedRoles() -> Element {
    let playhead = use_playhead();
    let profile = crate::library::profile();
    let roles = profile
        .roles
        .iter()
        .filter(|r| r.kind == ignition_core::RoleKind::Group)
        .map(|r| r.name.clone());
    let protected = playhead().protected;
    rsx! {
        div { class: "live-block protect",
            header { "Protected" }
            div { class: "tile-grid",
                for role in roles {
                    {
                        let on = protected.iter().any(|p| p.eq_ignore_ascii_case(&role));
                        let name = role.clone();
                        rsx! {
                            button {
                                key: "{role}",
                                class: if on { "ptile protected" } else { "ptile" },
                                onpointerdown: move |_| send(Command::Protect { role: name.clone(), on: !on }),
                                span { class: "pname", "{role}" }
                                if on { span { class: "plock", "🔒" } }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The grand master and the two playback masters, from the playhead.
// r[impl playback.grand-master]
#[component]
pub fn Masters(#[props(default = 220.0)] track: f32) -> Element {
    let playhead = use_playhead();
    let p: Playhead = playhead();
    rsx! {
        div { class: "live-block masters-block",
            header { "Masters" }
            div { class: "tfader-row",
                TouchFader { label: "GM".to_string(), css: "#e05050".to_string(), level: p.grand, track, on_change: move |v: f32| send(Command::Grand(v)) }
                TouchFader { label: "SONG".to_string(), css: "#c8a050".to_string(), level: p.song_master(), track, on_change: move |v: f32| send(Command::PlaybackMaster(ignition_core::Class::Song, v)) }
                TouchFader { label: "LOOK".to_string(), css: "#a0c850".to_string(), level: p.look_master(), track, on_change: move |v: f32| send(Command::PlaybackMaster(ignition_core::Class::Look, v)) }
            }
        }
    }
}

/// The Live view.
#[component]
pub fn Live(surface: Surface, #[props(default)] banks: Vec<crate::desk::Bank>) -> Element {
    let mut browse = use_signal(|| false);
    rsx! {
        section { class: "live",
            div { class: "live-col scenes",
                LooksBank {}
                MacrosRow {}
                DeskBanks { banks }
            }
            div { class: "live-col centre",
                FaderBank {}
                Masters {}
            }
            div { class: "live-col right",
                button {
                    class: if browse() { "browse on" } else { "browse" },
                    onpointerdown: move |_| browse.set(!browse()),
                    if browse() { "PALETTES" } else { "BROWSE / SEARCH" }
                }
                if browse() {
                    Library { surface, open: library::Tab::Kind(Kind::Effect) }
                } else {
                    Palettes { surface }
                    ProtectedRoles {}
                }
            }
        }
    }
}

/// The Program / Live switch. Both views stay mounted so switching
/// loses nothing, and neither view changes what the rig outputs — a
/// view is chrome over one engine.
// r[impl studio.views] - switchable at any time without losing state
#[component]
pub fn Views(boot: Bootstrap) -> Element {
    let operator = use_context_provider(|| Signal::new(boot.operator.clone()));
    let surface = boot.surface.clone();
    let mut view = use_signal(|| {
        if operator().default_view == "program" {
            View::Program
        } else {
            View::Live
        }
    });
    // The viewport draws the programmer's overlays only in Program.
    // r[impl studio.program.pick-and-gizmos] - Live has the overlays off
    use_effect(move || {
        crate::send(crate::command::Command::ProgramView(
            view() == View::Program,
        ));
    });
    let lan = boot.lan.join("  ");
    rsx! {
        div { class: "views",
            div { class: "view-strip",
                span { class: "operator", "{operator().name}" }
                // Where an iPad connects, when the studio is serving
                // (`IGNITION_LIVE=1`). Typed into Safari; nothing to tap.
                // r[impl studio.touch.ipad] - the address is on the strip
                if !lan.is_empty() {
                    span { class: "lan", "{lan}" }
                }
                button {
                    class: if view() == View::Program { "view-key on" } else { "view-key" },
                    onpointerdown: move |_| view.set(View::Program),
                    "PROGRAM"
                }
                button {
                    class: if view() == View::Live { "view-key on" } else { "view-key" },
                    onpointerdown: move |_| view.set(View::Live),
                    "LIVE"
                }
            }
            div { class: if view() == View::Live { "view" } else { "view hidden" },
                Live { surface: surface.clone(), banks: boot.banks }
            }
            div { class: if view() == View::Program { "view" } else { "view hidden" },
                crate::program::Program { surface }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r[verify studio.labels]
    #[test]
    fn labels_fit_eight_characters() {
        assert_eq!(label8("chorus full"), "CHORUS F");
        assert_eq!(label8("punt"), "PUNT");
        let profile = crate::library::profile();
        for name in profile.looks.keys().chain(profile.macros.keys()) {
            assert!(label8(name).chars().count() <= 8);
        }
        for page in &profile.pages {
            assert!(label8(&page.name).chars().count() <= 8);
        }
    }

    /// Every fader on every page wears a clock the operator can read.
    /// r[verify studio.views.seven-busking-features]
    #[test]
    fn every_fader_has_a_clock_and_filter_badge() {
        assert_eq!(clock_badge(Some(&Speed::Master("Song".into()))), "Song");
        assert_eq!(
            clock_badge(Some(&Speed::Scaled {
                master: "Tap".into(),
                scale: 0.5
            })),
            "Tap ½"
        );
        assert_eq!(
            clock_badge(Some(&Speed::Scaled {
                master: "Tap".into(),
                scale: 2.0
            })),
            "Tap ×2"
        );
        assert_eq!(filter_badge(&AttrFilter::ALL), "");
        assert_eq!(filter_badge(&AttrFilter::COLOUR), "COL");
        let pages = crate::faders::bank_pages();
        let colour = &pages[3];
        assert!(
            colour
                .iter()
                .take(6)
                .all(|s| filter_badge(&s.fader.filter) == "COL")
        );
        for spec in pages.iter().flatten() {
            let badge = clock_badge(spec.fader.recipe.as_ref().map(|r| &r.timing.speed));
            assert!(!badge.is_empty(), "{}", spec.name);
        }
    }

    /// Both views stay mounted, and switching sends nothing that could
    /// change what the rig is doing.
    ///
    /// A UI this shallow cannot be rendered headlessly, so what is
    /// checked is the two things the rule is actually about, in the
    /// source that decides them. First: the switch hides a view rather
    /// than unmounting it — an `if` here would drop every open pane,
    /// scroll position and half-typed field on the way past, which is
    /// exactly the "without losing state" the rule asks for. Second:
    /// the only command the switch sends is `ProgramView`, which turns
    /// the viewport's overlays on and off. Any other command from this
    /// function would be a view changing the show.
    ///
    /// r[verify studio.views]
    #[test]
    fn both_views_stay_mounted_and_switching_touches_nothing_but_the_overlays() {
        let source = include_str!("live.rs");
        let start = source
            .find("pub fn Views(")
            .expect("the Program / Live switch");
        let body = source.get(start..).expect("a valid byte offset");
        let body = body
            .get(..body.find("\n}\n").expect("the end of Views"))
            .expect("a valid byte offset");

        // Both views are constructed, unconditionally, and hidden by a
        // class rather than by an `if` that would unmount one.
        assert!(body.contains("Live {"), "the Live view is not mounted here");
        assert!(
            body.contains("program::Program {"),
            "the Program view is not mounted here"
        );
        let hidden = body.matches("\"view hidden\"").count();
        assert_eq!(
            hidden, 2,
            "a view is switched by mounting rather than by hiding, so switching loses \
             whatever it was holding"
        );
        assert!(
            LIVE_CSS.contains(".view.hidden { display: none; }"),
            "the hidden class does not actually hide"
        );

        // And the switch sends one command: the overlays.
        let sent: std::collections::BTreeSet<&str> = body
            .match_indices("Command::")
            .map(|(i, _)| {
                let rest = body
                    .get(i.saturating_add("Command::".len())..)
                    .expect("a valid byte offset");
                let end = rest
                    .find(|c: char| !c.is_alphanumeric() && c != '_')
                    .unwrap_or(rest.len());
                rest.get(..end).expect("a valid byte offset")
            })
            .collect();
        assert_eq!(
            sent,
            std::iter::once("ProgramView").collect(),
            "the view switch sends something beyond the viewport's overlays, so changing \
             view changes the show: {sent:?}"
        );
    }

    /// r[verify studio.touch]
    /// Under a finger the targets are larger still, and a track drag
    /// does not scroll the page.
    // r[impl studio.touch] - the coarse-pointer block
    #[test]
    fn coarse_pointer_targets_are_larger_and_tracks_do_not_scroll() {
        let start = LIVE_CSS
            .find("@media (pointer: coarse)")
            .expect("a coarse-pointer block");
        let block = LIVE_CSS.get(start..).expect("a valid byte offset");
        let block = block
            .get(..block.find("\n}\n").expect("block end"))
            .expect("a valid byte offset");
        let min = block
            .split("min-height:")
            .nth(1)
            .and_then(|s| s.trim().split("px").next())
            .and_then(|n| n.trim().parse::<u32>().ok())
            .expect("a min-height");
        assert!(min >= 44, "coarse targets are {min}px");
        for class in [".tkey", ".ptile", ".look-key", ".tap-key", ".view-key"] {
            assert!(block.contains(class), "{class} is not enlarged for touch");
        }
        assert!(block.contains("touch-action: none"));
        assert!(block.contains("user-select: none"));
    }

    #[test]
    fn touch_targets_are_at_least_44px() {
        // The stylesheet is the contract: every Live control class
        // declares a min-height of 44px or more.
        for class in [
            ".look-key",
            ".macro-key",
            ".desk-scene",
            ".ptile",
            ".tap-key",
            ".tkey",
            ".page-tab",
            ".lib-body",
            ".view-key",
        ] {
            let rule = LIVE_CSS
                .split('}')
                .find(|r| {
                    let selector = r.rsplit('{').next_back().unwrap_or("").trim();
                    selector.split(',').any(|s| s.trim() == class)
                })
                .unwrap_or_else(|| panic!("{class} has no rule"));
            let min = rule
                .split("min-height:")
                .nth(1)
                .and_then(|s| s.trim().split("px").next())
                .and_then(|n| n.trim().parse::<u32>().ok())
                .unwrap_or_else(|| panic!("{class} has no min-height"));
            assert!(min >= 44, "{class} is {min}px");
        }
    }
}

#[cfg(test)]
mod token_sheet {
    //! The palette is one file, and stays one file.
    //!
    //! These are the rules that make `tokens.css` a layer rather than a
    //! one-off tidy-up. Both failures they catch had already happened
    //! before the sheet existed: a colour written out in two stylesheets
    //! that then drifted apart, and four different spellings of the same
    //! grey (`#1a1a20`, `#1b1b21`, `#1b1b22`, `#1c1c22`) that no eye
    //! could tell apart and no search could find together.

    const TOKENS: &str = include_str!("tokens.css");
    const LIVE: &str = include_str!("live.css");
    const STUDIO: &str = include_str!("../../../apps/ignition-studio/src/studio.css");
    const DOCK: &str = include_str!("../../../apps/ignition-studio/src/dock.css");
    const BASE: &str = include_str!("../../../apps/ignition-live-web/src/base.css");
    const PANEL: &str = include_str!("../../../apps/ignition-studio/src/panel.css");
    const THEME: &str = include_str!("theme.css");

    fn sheets() -> [(&'static str, &'static str); 5] {
        [
            ("live.css", LIVE),
            ("studio.css", STUDIO),
            ("dock.css", DOCK),
            ("base.css", BASE),
            ("panel.css", PANEL),
        ]
    }

    /// Every `#rrggbb` (and `#rgb`) literal in a sheet, normalised to six
    /// digits. Deliberately naive: it also sees hexes inside comments,
    /// which is the conservative direction — a colour written in a
    /// comment is still a colour someone will copy.
    fn hexes(css: &str) -> Vec<String> {
        let bytes: Vec<char> = css.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == '#' {
                let digits: String = bytes[i.saturating_add(1)..]
                    .iter()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .collect();
                let n = digits.len();
                // Only 3- and 6-digit runs are colours; 4 and 8 carry
                // alpha and are left alone, and anything longer is not a
                // colour at all.
                if n == 3 {
                    out.push(
                        digits
                            .chars()
                            .flat_map(|c| [c, c])
                            .collect::<String>()
                            .to_lowercase(),
                    );
                } else if n == 6 {
                    out.push(digits.to_lowercase());
                }
                i = i.saturating_add(n.max(1)).saturating_add(1);
            } else {
                i = i.saturating_add(1);
            }
        }
        out
    }

    fn token_values() -> Vec<(String, String)> {
        TOKENS
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let (name, rest) = l.strip_prefix("--")?.split_once(':')?;
                // First whitespace-delimited word, *then* drop the
                // semicolon — a trailing `/* comment */` means the line
                // itself does not end in one.
                let value = rest.split_whitespace().next()?.trim_end_matches(';');
                let hex = value.strip_prefix('#')?;
                (hex.len() == 6).then(|| (name.trim().to_string(), hex.to_lowercase()))
            })
            .collect()
    }

    fn rgb(hex: &str) -> [i32; 3] {
        let p = |i: usize| {
            hex.get(i..i.saturating_add(2))
                .and_then(|s| i32::from_str_radix(s, 16).ok())
                .unwrap_or(0)
        };
        [p(0), p(2), p(4)]
    }

    /// No colour is written out in more than one stylesheet.
    ///
    /// This is the drift that motivated the file: thirty-one colours
    /// were duplicated across two or three sheets, so "the selected
    /// blue" was several strings that agreed only by luck.
    #[test]
    fn no_colour_is_spelled_out_in_two_sheets() {
        let mut seen: std::collections::BTreeMap<String, Vec<&str>> =
            std::collections::BTreeMap::default();
        for (name, css) in sheets() {
            for h in hexes(css) {
                let e = seen.entry(h).or_default();
                if !e.contains(&name) {
                    e.push(name);
                }
            }
        }
        let shared: Vec<_> = seen.iter().filter(|(_, v)| v.len() > 1).collect();
        assert!(
            shared.is_empty(),
            "these colours are written out in more than one sheet; give each a \
             token in tokens.css and use var(): {shared:?}"
        );
    }

    /// No raw colour is close enough to a token to be mistaken for it.
    ///
    /// Within two per channel is below anything an eye resolves, so such
    /// a literal is not a deliberate shade — it is the token, typed
    /// again and slightly wrong. Eighteen of these existed when the
    /// sheet was written.
    #[test]
    fn no_raw_colour_is_a_near_miss_of_a_token() {
        let tokens = token_values();
        let mut near = Vec::new();
        for (sheet, css) in sheets() {
            for h in hexes(css) {
                for (name, value) in &tokens {
                    if &h != value
                        && rgb(&h)
                            .iter()
                            .zip(rgb(value))
                            .all(|(a, b)| (a - b).abs() <= 2)
                    {
                        near.push(format!("{sheet}: #{h} is --{name} (#{value}) mistyped"));
                    }
                }
            }
        }
        assert!(near.is_empty(), "{near:#?}");
    }

    /// Tailwind's `@theme` and the token sheet agree, colour for colour.
    ///
    /// Two files for one palette, unavoidably: `theme.css` is what
    /// generates `bg-panel` and friends, `tokens.css` is what the
    /// hand-written sheets read through `var()` with no build step —
    /// and a plain `cargo run`, which compiles no Tailwind at all, must
    /// still come up in the right colours. This is what stops them
    /// drifting, and it demands the *whole* palette in both, so a
    /// colour can never be usable from a stylesheet but not from a
    /// class.
    #[test]
    fn the_theme_agrees_with_the_tokens() {
        let tokens: std::collections::BTreeMap<_, _> = token_values().into_iter().collect();
        // `--color-panel` in the theme is `--panel` here.
        let mut checked = 0;
        for line in THEME.lines() {
            let line = line.trim();
            let Some((name, rest)) = line
                .strip_prefix("--color-")
                .and_then(|l| l.split_once(':'))
            else {
                continue;
            };
            let Some(theme_hex) = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.trim_end_matches(';').strip_prefix('#'))
            else {
                continue;
            };
            let ours = name.trim();
            let token = tokens.get(ours).unwrap_or_else(|| {
                panic!("theme.css has --color-{ours} but tokens.css has no --{ours}")
            });
            assert_eq!(
                &theme_hex.to_lowercase(),
                token,
                "theme.css --color-{ours} and tokens.css --{ours} disagree"
            );
            checked += 1;
        }
        // Every token gets a utility: a colour the hand-written sheets
        // can use but a class cannot is a hole the migration would fall
        // into, one rule at a time.
        assert_eq!(
            checked,
            tokens.len(),
            "theme.css names {checked} colours, tokens.css names {}",
            tokens.len()
        );
    }
}
