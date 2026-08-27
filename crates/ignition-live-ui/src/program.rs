//! The Program view: building a show.
//!
//! The programmer in the middle — what is selected, what the hand
//! holds, dimmer, colour and focus through the same palettes Live uses
//! — with the cue list on one side and the library on the other. A
//! store writes the programmer's captured values into a cue of the show
//! file and into the running player in the same stroke, so the stage
//! shows the stored cue without a GO. A store to a look writes the
//! hand's recipes into the profile's authored overlay
//! (`r[profile.looks.authored]`) — never the baked file — and the looks
//! bank shows it on the next render.

// Nothing is dead here; it is mounted when `main.rs` hosts `live::Views`
// (and its stylesheet, `live::LIVE_CSS`). Until the integrator wires
// that, the crate root does not reach these items. Remove once mounted.

// r[impl studio.program.cue-editing] - select, set, store, see the list
// r[impl studio.views] - the Program view

use crate::command::{Command, OverlayKind};
use crate::library::{Library, Tab, css_of, use_operator};
use crate::operators::Kind;
use crate::{CueList, HSlider, Surface, send, use_playhead};
use dioxus::prelude::*;
use ignition_core::Selection;
use ignition_core::cue::StoreMode;
use ignition_core::profile::LookKind;

/// The programmer panel.
#[component]
pub fn Programmer(surface: Surface) -> Element {
    let playhead = use_playhead();
    let operator = use_operator();
    let mut mode = use_signal(|| StoreMode::Track);
    let mut look_name = use_signal(String::new);
    let mut look_kind = use_signal(|| LookKind::Bed);
    let mut overlays = use_signal(Overlays::default);
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
                            // The disc, not the bar: a palette's colours belong
                            // round the wheel as wedges, or two palettes that
                            // differ in one colour look identical at this size.
                            span { class: "pdisc", style: "background: {chip.disc()}" }
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
            // r[impl studio.program.pick-and-gizmos] - the overlay keys
            div { class: "prog-row overlays",
                span { class: "prog-label", "3D" }
                for kind in OverlayKind::ALL {
                    button {
                        key: "{kind.label()}",
                        class: if overlays().is_on(kind) { "ptile small on" } else { "ptile small" },
                        title: "draw this overlay on the visualizer while in Program",
                        onpointerdown: move |_| {
                            let on = !overlays().is_on(kind);
                            overlays.write().set(kind, on);
                            send(Command::Overlay { kind, on });
                        },
                        span { class: "pname", "{kind.label()}" }
                    }
                }
                button {
                    class: if overlays().labels { "ptile small on" } else { "ptile small" },
                    title: "the DMX address over every fixture",
                    onpointerdown: move |_| {
                        let on = !overlays().labels;
                        overlays.write().labels = on;
                        send(Command::Labels(on));
                    },
                    span { class: "pname", "LABELS" }
                }
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
                    title: "writes the captured values into the current cue of the show file and the running list",
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
                    title: "appends a new cue to the show file and the running list with the captured values",
                    onpointerdown: move |_| send(Command::StoreCue { index: cue_count, mode: StoreMode::Track }),
                    span { class: "pname", "STORE → NEW" }
                }
            }
            // r[impl profile.looks.authored] - a name, a kind, and the hand becomes a look
            div { class: "prog-row store",
                span { class: "prog-label", "Look" }
                input {
                    class: "look-name-input",
                    r#type: "text",
                    placeholder: "look name",
                    value: "{look_name}",
                    oninput: move |e| look_name.set(e.value()),
                }
                for (k, label) in [
                    (LookKind::Bed, "BED"),
                    (LookKind::Full, "FULL"),
                    (LookKind::Punt, "PUNT"),
                    (LookKind::Safe, "SAFE"),
                ] {
                    button {
                        key: "{label}",
                        class: if look_kind() == k { "ptile small on" } else { "ptile small" },
                        style: "border-color: {crate::library::look_css(k)}",
                        onpointerdown: move |_| look_kind.set(k),
                        span { class: "pname", "{label}" }
                    }
                }
                {
                    let has_hand = p.captured > 0 || p.held_look.is_some();
                    let ready = has_hand && !look_name().trim().is_empty();
                    rsx! {
                        button {
                            class: if ready { "ptile store" } else { "ptile store off" },
                            disabled: !ready,
                            title: if has_hand {
                                "stores the hand — the held look and every apply since CLEAR — as a look of this name in the profile's authored looks; the bank shows it at once"
                            } else {
                                "select, set a colour, a level or a focus (or hold a look), then name it"
                            },
                            onpointerdown: move |_| {
                                let name = look_name().trim().to_string();
                                if !name.is_empty() {
                                    send(Command::StoreLook { name, kind: look_kind() });
                                }
                            },
                            span { class: "pname", "STORE → LOOK" }
                        }
                    }
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
            // Program opens on every column; the operator can drop
            // to Live from the header without leaving the view.
            // r[impl studio.cuelist.one-panel]
            CueList { cues: surface.cues.clone(), preset: crate::Preset::Program }
            Programmer { surface: surface.clone() }
            Library { surface: surface.clone(), open: Tab::Kind(Kind::Effect) }
        }
    }
}

/// The overlay keys' own state — what the widget starts with, mirrored
/// here so the keys light without a round trip.
// r[impl studio.program.pick-and-gizmos] - FOCUS / BEAMS / GROUPS / LABELS
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overlays {
    pub focus: bool,
    pub beams: bool,
    pub groups: bool,
    pub labels: bool,
}

impl Default for Overlays {
    fn default() -> Self {
        Self {
            focus: true,
            beams: true,
            groups: true,
            labels: false,
        }
    }
}

impl Overlays {
    pub fn is_on(&self, kind: OverlayKind) -> bool {
        match kind {
            OverlayKind::Focus => self.focus,
            OverlayKind::Beams => self.beams,
            OverlayKind::Groups => self.groups,
        }
    }

    pub fn set(&mut self, kind: OverlayKind, on: bool) {
        match kind {
            OverlayKind::Focus => self.focus = on,
            OverlayKind::Beams => self.beams = on,
            OverlayKind::Groups => self.groups = on,
        }
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::*;

    /// r[verify studio.program.pick-and-gizmos] - the keys start where the widget does
    #[test]
    fn overlay_keys_default_to_the_widgets_defaults_and_toggle_one_at_a_time() {
        let mut o = Overlays::default();
        assert!(o.focus && o.beams && o.groups && !o.labels);
        o.set(OverlayKind::Beams, false);
        assert!(!o.is_on(OverlayKind::Beams));
        assert!(o.is_on(OverlayKind::Focus) && o.is_on(OverlayKind::Groups));
    }
}
