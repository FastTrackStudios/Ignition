//! `/demo` — the visualizer, running in the browser.
//!
//! # Why this can exist at all
//!
//! The visualizer is Bevy, and Bevy runs on wasm. What stood in the way
//! was never the renderer: `ignition-viz` pulled `sacn` for live DMX in,
//! `sacn` reaches `socket2`, and `socket2` does not compile for wasm32 —
//! so the build died long before anything about rendering was reached.
//! With the DMX listeners gated to the host, the whole stack builds.
//!
//! # Why it is a route and not the front page
//!
//! It is megabytes of WebAssembly. `dioxus-router`'s `wasm-split`
//! feature puts each route in its own lazily-fetched chunk, so a reader
//! who came for the guide never downloads any of this — the cost is paid
//! by whoever asks for it, on the click below.
//!
//! # What is not here yet
//!
//! Gobos. They are clustered decals, clustered decals need bindless
//! textures, and — from the doc comment in our own vendored `bevy_pbr` —
//! bindless "presently can't be used on WebGL 2 or WebGPU". The renderer
//! skips them rather than failing, so beams, haze, the PBR room and the
//! screens are all intact; the projector simply does not project.

use dioxus::prelude::*;

use crate::routes::Shell;

/// The Norco venue's documents, served beside the site.
///
/// Fetched rather than baked in: they are the same files the desk reads
/// out of `data/venues/norco`, and a demo carrying its own private copy
/// compiled into the binary would be a demo of something else.
///
/// Through `asset!` rather than a literal path, because dx copies the
/// files it is *told* about and content-hashes their names — a
/// hand-written `/assets/venue/room.json` would be both uncopied and
/// uncacheable.
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_FIXTURES: Asset = asset!("/assets/venue/fixtures.json");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_ROOM: Asset = asset!("/assets/venue/room.json");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_SCREENS: Asset = asset!("/assets/venue/screens.json");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_PROPS: Asset = asset!("/assets/venue/props.json");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_GROUPS: Asset = asset!("/assets/venue/groups.json");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_PALETTES: Asset = asset!("/assets/venue/palettes.json");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_BINDING: Asset = asset!("/assets/venue/profile.json");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_AREAS: Asset = asset!("/assets/venue/areas.json");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const PROFILE_DOCUMENT: Asset = asset!("/assets/venue/profile.ig-profile");

/// The visualizer's own asset root — the people, the drum kit, the
/// speakers, the screen content. A FOLDER asset: Bevy's asset server
/// resolves paths under it at run time (`people/man-casual.glb`), so
/// there is nothing here to name file by file, and dx would not copy the
/// directory at all if it were not asked for as a whole.
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VIZ_ASSETS: Asset = asset!("/assets/viz");

/// What the demo is doing, for the reader.
#[derive(Clone, PartialEq, Eq)]
enum Stage {
    /// Nothing fetched, nothing running. The click below starts it.
    Idle,
    Loading(&'static str),
    /// Bevy owns the canvas from here; this component stops re-rendering
    /// it and gets out of the way.
    Running,
    Failed(String),
}

#[component]
pub fn Demo() -> Element {
    let mut stage = use_signal(|| Stage::Idle);

    rsx! {
        Shell {
            section { class: "ig-demo",
                header { class: "ig-demo-bar",
                    h1 { "The visualizer, in your browser" }
                    match &*stage.read() {
                        Stage::Idle => rsx! {
                            button {
                                class: "ig-button ig-button-primary",
                                onclick: move |_| {
                                    spawn(async move {
                                        match launch(&mut stage).await {
                                            Ok(()) => stage.set(Stage::Running),
                                            Err(e) => stage.set(Stage::Failed(e)),
                                        }
                                    });
                                },
                                "Launch the demo"
                            }
                        },
                        Stage::Loading(what) => rsx! {
                            span { class: "ig-demo-status", "Loading {what}…" }
                        },
                        Stage::Running => rsx! {
                            span { class: "ig-demo-status", "Norco · running" }
                        },
                        Stage::Failed(e) => rsx! {
                            span { class: "ig-demo-status ig-demo-failed", "{e}" }
                        },
                    }
                }

                // Bevy is handed this canvas by CSS selector and owns it
                // from then on — which is the whole reason the demo can
                // sit inside a page Dioxus is laying out, rather than
                // opening a window of its own.
                div { class: "ig-demo-stage",
                    canvas { id: "ig-viz", class: "ig-demo-canvas" }
                    if *stage.read() == Stage::Idle {
                        p { class: "ig-demo-hint",
                            "The Norco rig, its room and its fixtures — the same files the desk "
                            "reads. Needs WebGPU."
                        }
                    }
                }
            }
        }
    }
}

/// Fetch the venue and hand it to Bevy.
///
/// Split out behind `wasm_split` so the router puts it — and everything
/// it reaches, which is the entire visualizer — in a chunk of its own.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::future_not_send,
    reason = "a browser is single-threaded and `JsFuture` is `!Send` by construction; \
              this runs on Dioxus's own local task set, which never requires it"
)]
async fn launch(stage: &mut Signal<Stage>) -> Result<(), String> {
    use ignition_viz::venue::VenueFiles;

    // Before anything is fetched, and before Bevy is asked for a
    // surface: without an adapter the visualizer boots, initialises, and
    // renders a black rectangle, which tells a visitor nothing at all.
    stage.set(Stage::Loading("the GPU"));
    webgpu_adapter().await?;

    stage.set(Stage::Loading("the venue"));
    let files = VenueFiles {
        fixtures: fetch_text(&VENUE_FIXTURES.to_string()).await?,
        room: fetch_text(&VENUE_ROOM.to_string()).await?,
        screens: fetch_text(&VENUE_SCREENS.to_string()).await?,
        props: fetch_text(&VENUE_PROPS.to_string()).await?,
        groups: fetch_text(&VENUE_GROUPS.to_string()).await.ok(),
        palettes: fetch_text(&VENUE_PALETTES.to_string()).await.ok(),
        profile: fetch_text(&VENUE_BINDING.to_string()).await.ok(),
        areas: fetch_text(&VENUE_AREAS.to_string()).await.ok(),
        profile_document: fetch_text(&PROFILE_DOCUMENT.to_string()).await.ok(),
        // No manifest and no local layer on the web: the demo is the
        // base room, which is the one that must work on its own.
        dmx: None,
        layer: None,
    };

    stage.set(Stage::Loading("the rig"));
    let venue = ignition_viz::Venue::from_files(&files).map_err(|e| format!("venue: {e}"))?;

    stage.set(Stage::Loading("the visualizer"));
    ignition_viz::run_web(venue, "#ig-viz", &VIZ_ASSETS.to_string());
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
async fn launch(_stage: &mut Signal<Stage>) -> Result<(), String> {
    Err("the demo only runs in a browser".to_string())
}

/// Fail early, and say why, if this browser will not give us a GPU.
///
/// `navigator.gpu` existing is not the question — Chromium exposes the
/// binding on every platform and then refuses an adapter on Linux, where
/// WebGPU is still behind a flag in stable. So the check that matters is
/// whether `requestAdapter()` returns anything, and the message has to
/// name the flag, because "no adapter" on a machine with a working
/// Vulkan stack and a current driver is otherwise baffling.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::future_not_send,
    reason = "a browser is single-threaded and `JsFuture` is `!Send` by construction; \
              this runs on Dioxus's own local task set, which never requires it"
)]
async fn webgpu_adapter() -> Result<(), String> {
    use wasm_bindgen::{JsCast as _, JsValue};

    const NO_WEBGPU: &str = "This browser has no WebGPU, which the visualizer needs.";
    const NO_ADAPTER: &str = "WebGPU is present but this browser would not provide an adapter. \
         On Linux that is the default in stable Chrome and Brave: enable \
         chrome://flags/#enable-unsafe-webgpu (brave://flags/… on Brave) and relaunch.";

    let window = web_sys::window().ok_or("no window")?;
    let gpu = js_sys::Reflect::get(&window.navigator(), &JsValue::from_str("gpu"))
        .map_err(|_| NO_WEBGPU.to_string())?;
    if gpu.is_undefined() || gpu.is_null() {
        return Err(NO_WEBGPU.to_string());
    }
    let request = js_sys::Reflect::get(&gpu, &JsValue::from_str("requestAdapter"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| NO_WEBGPU.to_string())?;
    let promise = request
        .call0(&gpu)
        .ok()
        .and_then(|p| p.dyn_into::<js_sys::Promise>().ok())
        .ok_or_else(|| NO_ADAPTER.to_string())?;
    let adapter = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|_| NO_ADAPTER.to_string())?;
    if adapter.is_null() || adapter.is_undefined() {
        return Err(NO_ADAPTER.to_string());
    }
    Ok(())
}

/// One document, over HTTP.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::future_not_send,
    reason = "a browser is single-threaded and `JsFuture` is `!Send` by construction; \
              this runs on Dioxus's own local task set, which never requires it"
)]
async fn fetch_text(url: &str) -> Result<String, String> {
    use wasm_bindgen::JsCast as _;

    let window = web_sys::window().ok_or("no window")?;
    let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|_| format!("fetching {url}"))?
        .dyn_into::<web_sys::Response>()
        .map_err(|_| format!("fetching {url}: not a response"))?;
    if !response.ok() {
        return Err(format!("fetching {url}: {}", response.status()));
    }
    let text = wasm_bindgen_futures::JsFuture::from(
        response.text().map_err(|_| format!("reading {url}"))?,
    )
    .await
    .map_err(|_| format!("reading {url}"))?;
    text.as_string().ok_or_else(|| format!("reading {url}"))
}
