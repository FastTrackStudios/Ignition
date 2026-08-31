//! The Ignition iPhone app.
//!
//! A prototype: it reads a demo show (`show.rs`) rather than talking to a
//! running console, but every screen is rendered from the real
//! `ignition-core` types and GO drives the real `CuePlayer`, tracking and
//! fades included. When the vox link lands, `show::` is the only module
//! that has to change.
use dioxus::prelude::*;
use ignition_core::{Attribute, ChanId, CuePlayer, Show};
use ignition_core::selection::EMPTY_RIG;
use std::collections::HashMap;

pub mod show;

const CSS: Asset = asset!("/assets/mobile.css");

#[derive(Routable, Clone, PartialEq)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
    #[route("/")]
    Cues {},
    #[route("/patch")]
    Patch {},
    #[route("/groups")]
    Groups {},
}

// PascalCase is the Dioxus component convention; the launch entrypoint is
// a plain fn rather than a `#[component]`, so the lint needs silencing here.
#[allow(non_snake_case)]
pub fn App() -> Element {
    rsx! {
        document::Link { rel: "stylesheet", href: CSS }
        Router::<Route> {}
    }
}

#[component]
fn Shell() -> Element {
    rsx! {
        div { class: "app",
            main { class: "body", Outlet::<Route> {} }
            nav { class: "tabs",
                Link { to: Route::Cues {}, class: "tab", "Cues" }
                Link { to: Route::Patch {}, class: "tab", "Patch" }
                Link { to: Route::Groups {}, class: "tab", "Groups" }
            }
        }
    }
}

#[component]
fn Cues() -> Element {
    let list = use_signal(show::cues);
    // The player is real state: GO advances it and the levels below are its
    // actual interpolated output, not a lookup of the cue's target values.
    let mut player = use_signal(|| CuePlayer::new(show::cues().cues));
    // Fades are wall-clock in the console; stepping to the end of the move
    // on GO keeps this honest without an animation loop the prototype has
    // no use for yet.
    // `go`/`output` resolve against a Show -- the groups, rig, palettes and
    // role bindings a recipe needs to know what "the key light" means here.
    // This prototype drives explicit per-channel values, so the minimal
    // Show (venue groups + an empty rig) is enough and nothing is faked.
    let groups = use_signal(show::groups);
    let mut go = move || {
        let g = groups.read();
        let s = Show::new(&g, &EMPTY_RIG);
        player.write().go(&s);
        let fade = player.read().current_index().map(|i| list.read().cues[i].fade_secs);
        if let Some(f) = fade {
            player.write().tick(f);
        }
    };

    let current = player.read().current_index();
    let out: HashMap<(ChanId, Attribute), f32> = {
        let g = groups.read();
        player.read().output(&Show::new(&g, &EMPTY_RIG))
    };

    rsx! {
        header { class: "head",
            h1 { "{list.read().name}" }
            span { class: "sub",
                {current.map_or("— standby —".to_string(), |i| format!("cue {} of {}", i + 1, list.read().cues.len()))}
            }
        }
        ol { class: "cuelist",
            for (i, cue) in list.read().cues.iter().enumerate() {
                li { key: "{i}", class: if Some(i) == current { "cue active" } else { "cue" },
                    div { class: "cue-name", "{cue.name}" }
                    div { class: "cue-fade",
                        {if cue.fade_secs == 0.0 { "snap".to_string() } else { format!("{:.1}s", cue.fade_secs) }}
                    }
                }
            }
        }
        div { class: "levels",
            for chan in 1u32..=6 {
                {
                    let level = out.get(&(chan, Attribute::Dimmer)).copied().unwrap_or(0.0);
                    rsx! {
                        div { key: "{chan}", class: "meter",
                            div { class: "meter-fill", style: "height: {level * 100.0}%" }
                            span { class: "meter-label", "{chan}" }
                        }
                    }
                }
            }
        }
        button { class: "go", onclick: move |_| go(), "GO" }
    }
}

#[component]
fn Patch() -> Element {
    let patch = use_signal(show::patch);
    rsx! {
        header { class: "head", h1 { "Patch" } span { class: "sub", "{patch.read().len()} fixtures" } }
        ul { class: "rows",
            for f in patch.read().iter() {
                li { key: "{f.chan}", class: "row",
                    span { class: "chan", "{f.chan}" }
                    span { class: "name", "{f.fixture_type}" }
                    span { class: "addr",
                        {f.dmx.map_or("—".to_string(), |d| format!("{}/{}", d.universe, d.start_channel))}
                    }
                }
            }
        }
    }
}

#[component]
fn Groups() -> Element {
    let groups = use_signal(show::groups);
    rsx! {
        header { class: "head", h1 { "Groups" } span { class: "sub", "{groups.read().len()} groups" } }
        ul { class: "rows",
            for g in groups.read().iter() {
                li { key: "{g.name}", class: "row",
                    span { class: "name", "{g.name}" }
                    span { class: "addr", "{g.chans.len()} ch" }
                }
            }
        }
    }
}
