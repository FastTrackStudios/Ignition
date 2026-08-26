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

// Nothing is dead here; it is mounted when `main.rs` hosts `live::Views`
// (and its stylesheet, `live::LIVE_CSS`). Until the integrator wires
// that, the crate root does not reach these items. Remove once mounted.

// r[impl studio.views] - the Live view, and the Live/Program switch
// r[impl studio.views.seven-busking-features] - looks, macros, pages, filters, protection, speed, params
// r[impl studio.live.desk-scenes] - the desk banks beside the looks
// r[impl studio.touch] - 44px targets, press not release, wide grabs
// r[impl studio.labels] - every fader and key label is eight characters or fewer

use crate::command::{Command, PageMove, Playhead, SpeedKey};
use crate::library::{self, Library, css_of, look_css, macro_shape, use_operator};
use crate::operators::Kind;
use crate::{HSlider, Surface, send, use_desk, use_playhead};
use dioxus::prelude::*;
use ignition_core::profile::LookKind;
use ignition_core::{AttrFilter, Selection, Speed};

/// The styles for every panel in this family. `main.rs` includes it
/// beside `studio.css`.
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
pub fn label8(name: &str) -> String {
    name.chars().take(8).collect::<String>().to_uppercase()
}

/// Which clock a fader follows, as the badge says it: Song, Tap, Tap ½,
/// Tap ×2 — or its own rate where the recipe is not slaved.
// r[impl profile.speed-routing] - every fader shows its clock
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
pub fn TouchFader(
    label: String,
    css: String,
    level: f32,
    #[props(default)] latched: bool,
    #[props(default)] toggled: bool,
    on_change: EventHandler<f32>,
) -> Element {
    let mut held = use_signal(|| false);
    // Keep in step with `.tfader .ttrack` in live.css.
    const TRACK: f32 = 220.0;
    let set_from = move |y: f64| {
        let v = (1.0 - (y as f32 / TRACK)).clamp(0.0, 1.0);
        on_change.call(v);
    };
    let class = match (latched, toggled) {
        (true, _) => "tfader latched",
        (_, true) => "tfader toggled",
        _ => "tfader",
    };
    rsx! {
        div { class: "{class}",
            div {
                class: "ttrack",
                onpointerdown: move |e| {
                    held.set(true);
                    set_from(e.data.element_coordinates().y);
                },
                onpointermove: move |e| {
                    if held() {
                        set_from(e.data.element_coordinates().y);
                    }
                },
                onpointerup: move |_| held.set(false),
                onpointerleave: move |_| held.set(false),
                div { class: "tfill", style: "height: {level * 100.0}%; background: {css}" }
                div { class: "thandle", style: "bottom: {level * 100.0}%; border-color: {css}" }
            }
            span { class: "tlabel", "{label8(&label)}" }
            span { class: "tvalue", "{(level * 100.0) as u32}" }
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
    let favs = operator().favourites.looks.clone();
    let mut looks: Vec<(String, LookKind, bool)> = favs
        .iter()
        .filter_map(|f| {
            all.iter()
                .find(|(n, _)| n == f)
                .map(|(n, k)| (n.clone(), *k, true))
        })
        .collect();
    looks.extend(
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
pub fn FaderBank() -> Element {
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
                        let spec = &pages[current][i];
                        let effect = match &profile.pages[current].faders[i].source {
                            ignition_core::profile::FaderSource::Effect(n) => Some(n.as_str()),
                            _ => None,
                        };
                        let speed = spec
                            .fader
                            .recipe
                            .as_ref()
                            .map(|r| r.timing.speed.clone())
                            .or_else(|| effect.map(|e| profile.speed_for(&profile.pages[current].faders[i], Some(e))));
                        let clock = clock_badge(speed.as_ref());
                        let filter = filter_badge(&spec.fader.filter);
                        let params = spec.params.clone();
                        let level = playhead().levels[i];
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
                                    latched: desk().latched[i],
                                    toggled: desk().toggled[i],
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
                                                        value: min + v * span,
                                                    }),
                                                }
                                            }
                                        }
                                    }
                                }
                                button {
                                    class: if desk().toggled[i] { "tkey on" } else { "tkey" },
                                    onpointerdown: move |_| send(Command::Key { index: i, action: key_mode(), down: true }),
                                    onpointerup: move |_| send(Command::Key { index: i, action: key_mode(), down: false }),
                                    onpointerleave: move |_| send(Command::Key { index: i, action: key_mode(), down: false }),
                                    "●"
                                }
                            }
                        }
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
                HSlider { initial: 0.5, on_change: move |v: f32| send(Command::EffectRate(0.5 + v * 1.5)) }
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
    let favs = operator().favourites.clone();
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
                        onpointerdown: { let n = name.clone(); move |_| send(Command::Color(n.clone())) },
                        span { class: "pdisc", style: "background: {css}" }
                        span { class: "pname", "{name}" }
                    }
                }
                for chip in surface.splits.iter().cloned() {
                    button {
                        key: "s-{chip.name}",
                        class: "ptile split",
                        onpointerdown: { let n = chip.name.clone(); move |_| send(Command::Split(n.clone())) },
                        span { class: "pdisc", style: "background: {chip.css}" }
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
                        onpointerdown: { let n = name.clone(); move |_| send(Command::Focus(n.clone())) },
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
                        onpointerdown: { let n = name.clone(); move |_| send(Command::Select(Selection::Group(n.clone()))) },
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
    let roles: Vec<String> = profile
        .roles
        .iter()
        .filter(|r| r.kind == ignition_core::RoleKind::Group)
        .map(|r| r.name.clone())
        .collect();
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
pub fn Masters() -> Element {
    let playhead = use_playhead();
    let p: Playhead = playhead();
    rsx! {
        div { class: "live-block masters-block",
            header { "Masters" }
            div { class: "tfader-row",
                TouchFader { label: "GM".to_string(), css: "#e05050".to_string(), level: p.grand, on_change: move |v: f32| send(Command::Grand(v)) }
                TouchFader { label: "SONG".to_string(), css: "#c8a050".to_string(), level: p.song_master(), on_change: move |v: f32| send(Command::PlaybackMaster(ignition_core::Class::Song, v)) }
                TouchFader { label: "LOOK".to_string(), css: "#a0c850".to_string(), level: p.look_master(), on_change: move |v: f32| send(Command::PlaybackMaster(ignition_core::Class::Look, v)) }
            }
        }
    }
}

/// The Live view.
#[component]
pub fn Live(surface: Surface) -> Element {
    let banks = use_hook(|| crate::desk::load(&crate::venue_dir()));
    let mut browse = use_signal(|| false);
    rsx! {
        section { class: "live",
            div { class: "live-col scenes",
                LooksBank {}
                MacrosRow {}
                DeskBanks { banks: banks.clone() }
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
                    Library { surface: surface.clone(), open: library::Tab::Kind(Kind::Effect) }
                } else {
                    Palettes { surface: surface.clone() }
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
pub fn Views(surface: Surface) -> Element {
    let operator = use_context_provider(|| Signal::new(crate::operators::Operator::current()));
    let mut view = use_signal(|| {
        if operator().default_view == "program" {
            View::Program
        } else {
            View::Live
        }
    });
    rsx! {
        div { class: "views",
            div { class: "view-strip",
                span { class: "operator", "{operator().name}" }
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
                Live { surface: surface.clone() }
            }
            div { class: if view() == View::Program { "view" } else { "view hidden" },
                crate::program::Program { surface: surface.clone() }
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

    /// r[verify studio.touch]
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
                    let selector = r.rsplit('{').last().unwrap_or("").trim();
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
