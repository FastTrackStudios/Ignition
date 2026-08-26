//! The Program view: building a show.
//!
//! The programmer in the middle — what is selected, what the hand
//! holds, dimmer, colour and focus through the same palettes Live uses
//! — with the cue list on one side and the library on the other. A
//! store writes the programmer's captured values into a cue of the show
//! file; the running player reads the file at load, so the stored cue
//! plays on the next launch rather than the next GO, and the button
//! says so. Storing to a look is not wired: the profile is baked from
//! code (`bake-profile`), and a look written into the file alone would
//! drift from the one the bank resolves through — so the key is here,
//! disabled, with the reason on it.

// Nothing is dead here; it is mounted when `main.rs` hosts `live::Views`
// (and its stylesheet, `live::LIVE_CSS`). Until the integrator wires
// that, the crate root does not reach these items. Remove once mounted.

// r[impl studio.program.cue-editing] - select, set, store, see the list
// r[impl studio.views] - the Program view

use crate::command::Command;
use crate::library::{Library, Tab, css_of, use_operator};
use crate::operators::Kind;
use crate::{CueList, HSlider, Surface, send, use_playhead};
use dioxus::prelude::*;
use ignition_core::Selection;
use ignition_core::cue::StoreMode;

/// The programmer panel.
#[component]
pub fn Programmer(surface: Surface) -> Element {
    let playhead = use_playhead();
    let operator = use_operator();
    let mut mode = use_signal(|| StoreMode::Track);
    let profile = crate::library::profile();
    let p = playhead();
    let favs = operator().favourites.clone();
    let colours: Vec<(String, String)> = {
        let mut all: Vec<(String, String)> = profile
            .colors
            .iter()
            .map(|c| (c.name.clone(), css_of(c)))
            .collect();
        let mut out: Vec<(String, String)> = favs
            .colours
            .iter()
            .filter_map(|f| all.iter().find(|(n, _)| n == f).cloned())
            .collect();
        all.retain(|(n, _)| !favs.colours.contains(n));
        out.append(&mut all);
        out
    };
    let cue_count = surface
        .cues
        .iter()
        .filter(|r| matches!(r, crate::Row::Cue { .. }))
        .count();
    let current = p.cue;
    rsx! {
        section { class: "programmer",
            header { class: "prog-head",
                span { class: "prog-title", "Programmer" }
                span { class: "prog-sel",
                    match &p.selection {
                        Some(s) => rsx! { "{s}" },
                        None => rsx! { "nothing selected" },
                    }
                }
                span { class: "prog-captured", "{p.captured} values" }
                if p.blind { span { class: "blind-flag", "BLIND" } }
            }
            div { class: "prog-row",
                span { class: "prog-label", "Roles" }
                div { class: "tile-grid",
                    for role in profile.roles.iter().filter(|r| r.kind == ignition_core::RoleKind::Group) {
                        button {
                            key: "{role.name}",
                            class: if p.selection.as_deref() == Some(&format!("role {}", role.name)) { "ptile on" } else { "ptile" },
                            title: "{role.about}",
                            onpointerdown: { let n = role.name.clone(); move |_| send(Command::Select(Selection::Role(n.clone()))) },
                            span { class: "pname", "{role.name}" }
                        }
                    }
                    for group in surface.groups.iter().cloned() {
                        button {
                            key: "g-{group}",
                            class: if p.selection.as_deref() == Some(group.as_str()) { "ptile on" } else { "ptile" },
                            onpointerdown: { let n = group.clone(); move |_| send(Command::Select(Selection::Group(n.clone()))) },
                            span { class: "pname", "{group}" }
                        }
                    }
                    button { class: "ptile warn", onpointerdown: move |_| send(Command::Deselect), span { class: "pname", "CLEAR SEL" } }
                }
            }
            div { class: "prog-row",
                span { class: "prog-label", "Dimmer" }
                div { class: "tparam wide",
                    HSlider { initial: 0.0, on_change: move |v: f32| send(Command::Dimmer(v)) }
                }
                for pct in [100u32, 75, 50, 25, 0] {
                    button { key: "{pct}", class: "ptile small", onpointerdown: move |_| send(Command::Dimmer(pct as f32 / 100.0)), span { class: "pname", "{pct}" } }
                }
            }
            div { class: "prog-row",
                span { class: "prog-label", "Colour" }
                div { class: "tile-grid",
                    for (name, css) in colours {
                        button {
                            key: "{name}",
                            class: if favs.colours.contains(&name) { "ptile fav" } else { "ptile" },
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
            }
            div { class: "prog-row",
                span { class: "prog-label", "Focus" }
                div { class: "tile-grid",
                    for name in surface.focus.iter().cloned() {
                        button {
                            key: "{name}",
                            class: if favs.focus.contains(&name) { "ptile fav" } else { "ptile" },
                            onpointerdown: { let n = name.clone(); move |_| send(Command::Focus(n.clone())) },
                            span { class: "pname", "{name}" }
                        }
                    }
                }
            }
            div { class: "prog-row",
                span { class: "prog-label", "Hand" }
                button { class: "ptile warn", onpointerdown: move |_| send(Command::Release), span { class: "pname", "RELEASE" } }
                button { class: "ptile warn", onpointerdown: move |_| send(Command::ClearValues), span { class: "pname", "CLEAR" } }
            }
            // r[impl cues.shield] - the store modes are the engine's own
            div { class: "prog-row store",
                span { class: "prog-label", "Store" }
                for (m, label) in [
                    (StoreMode::Track, "TRACK"),
                    (StoreMode::CueOnly, "CUE ONLY"),
                    (StoreMode::ShieldFromZero, "SHIELD 0"),
                    (StoreMode::ShieldAboveZero, "SHIELD >0"),
                ] {
                    button {
                        key: "{label}",
                        class: if mode() == m { "ptile small on" } else { "ptile small" },
                        onpointerdown: move |_| mode.set(m),
                        span { class: "pname", "{label}" }
                    }
                }
                button {
                    class: if current.is_some() && p.captured > 0 { "ptile store" } else { "ptile store off" },
                    disabled: current.is_none() || p.captured == 0,
                    title: "writes the captured values into the current cue of the show file; plays on the next launch",
                    onpointerdown: move |_| {
                        if let Some(index) = current {
                            send(Command::StoreCue { index, mode: mode() });
                        }
                    },
                    span { class: "pname",
                        match current {
                            Some(i) => rsx! { "STORE → CUE {i}" },
                            None => rsx! { "STORE → CUE" },
                        }
                    }
                }
                button {
                    class: if p.captured > 0 { "ptile store" } else { "ptile store off" },
                    disabled: p.captured == 0,
                    title: "appends a new cue to the show file with the captured values; plays on the next launch",
                    onpointerdown: move |_| send(Command::StoreCue { index: cue_count, mode: StoreMode::Track }),
                    span { class: "pname", "STORE → NEW" }
                }
                button {
                    class: "ptile store off",
                    disabled: true,
                    title: "not wired: looks are baked into the profile from code, and a look written to the file alone would drift from the bank",
                    span { class: "pname", "STORE → LOOK" }
                }
            }
        }
    }
}

/// The Program view: cue list, programmer, library.
// r[impl studio.program.cue-editing] - one view, no leaving it
#[component]
pub fn Program(surface: Surface) -> Element {
    rsx! {
        section { class: "program",
            CueList { cues: surface.cues.clone() }
            Programmer { surface: surface.clone() }
            Library { surface: surface.clone(), open: Tab::Kind(Kind::Effect) }
        }
    }
}
