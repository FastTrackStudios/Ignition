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
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_FIXTURES: Asset = asset!("/assets/venue/fixtures.json");
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_ROOM: Asset = asset!("/assets/venue/room.json");
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_SCREENS: Asset = asset!("/assets/venue/screens.json");
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_PROPS: Asset = asset!("/assets/venue/props.json");
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_GROUPS: Asset = asset!("/assets/venue/groups.json");
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_PALETTES: Asset = asset!("/assets/venue/palettes.json");
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_BINDING: Asset = asset!("/assets/venue/profile.json");
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VENUE_AREAS: Asset = asset!("/assets/venue/areas.json");
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const PROFILE_DOCUMENT: Asset = asset!("/assets/venue/profile.ig-profile");
/// The show the demo plays — `data/songs/bye-bye-bye.json`, a `CueList`.
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const SHOW: Asset = asset!("/assets/venue/show.json");

/// The visualizer's own asset root — the people, the drum kit, the
/// speakers, the screen content. A FOLDER asset: Bevy's asset server
/// resolves paths under it at run time (`people/man-casual.glb`), so
/// there is nothing here to name file by file, and dx would not copy the
/// directory at all if it were not asked for as a whole.
// Read only on the browser path; the SSG pre-render never fetches.
#[cfg(target_arch = "wasm32")]
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const VIZ_ASSETS: Asset = asset!("/assets/viz");

/// What the demo is doing, for the reader.
///
/// The pre-render half only ever sees `Idle` — it emits the page's shell
/// and nothing launches — so its other variants look dead from the host.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(dead_code, reason = "only the browser ever leaves `Idle`")
)]
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
    // The desk appears when the show is loaded, and is fed from then on.
    let mut boot = use_signal(|| Option::<ignition_live_ui::Bootstrap>::None);
    // The signal every `use_playhead` in the tree reads. Polled off the
    // engine rather than pushed, for the reason the studio gives: the
    // page already repaints to keep the canvas animating, so a second
    // timer costs nothing and there is no borrow to hold across an await.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(unused_mut, reason = "only the browser's poll loop writes it")
    )]
    let mut playhead = use_signal(ignition_live_ui::command::Playhead::default);
    use_context_provider(|| ignition_live_ui::PlayheadFeed(playhead));
    use_hook(|| {
        // `send` anywhere in the desk lands in the queue the engine
        // drains each frame. Installed once; a second call is ignored.
        let desk = crate::routes::desk::desk();
        ignition_live_ui::install(move |command| desk.push(command));
    });
    // Wasm only: the pre-render half of `dx build --ssg` runs this
    // component on the host to emit HTML, where there is no engine to
    // poll and no browser timer to poll it with.
    #[cfg(target_arch = "wasm32")]
    use_future(move || async move {
        let desk = crate::routes::desk::desk();
        loop {
            gloo_timers::future::TimeoutFuture::new(33).await;
            let next = desk.playhead();
            // Only when it actually moved: a signal write is a re-render,
            // and the desk is not cheap to draw.
            if next != playhead() {
                playhead.set(next);
            }
        }
    });

    rsx! {
        Shell {
            // The desk's own stylesheets, the same three the studio and
            // the iPad mount. Not restyled here: a demo of the desk that
            // looked different from the desk would be a demo of nothing.
            style { {ignition_live_ui::live::TOKENS_CSS} }
            style { {ignition_live_ui::live::LIVE_CSS} }

            section { class: "ig-demo",
                header { class: "ig-demo-bar",
                    h1 { "The visualizer, in your browser" }
                    match &*stage.read() {
                        Stage::Idle => rsx! {
                            button {
                                class: "ig-button ig-button-primary",
                                onclick: move |_| {
                                    spawn(async move {
                                        match launch(&mut stage, move |b| boot.set(Some(b))).await {
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
                            span { class: "ig-demo-status", "Norco · Bye Bye Bye" }
                        },
                        Stage::Failed(e) => rsx! {
                            span { class: "ig-demo-status ig-demo-failed", "{e}" }
                        },
                    }
                }

                // The room and the desk, side by side. Bevy is handed
                // the canvas by CSS selector and owns it from then on —
                // which is what lets the visualizer sit inside a page
                // Dioxus is laying out rather than a window of its own.
                div { class: if boot.read().is_some() { "ig-demo-body has-desk" } else { "ig-demo-body" },
                    div { class: "ig-demo-stage",
                        canvas { id: "ig-viz", class: "ig-demo-canvas" }
                        if *stage.read() == Stage::Idle {
                            p { class: "ig-demo-hint",
                                "The Norco rig, its room and its show — the same files the desk "
                                "reads. Needs WebGPU."
                            }
                        }
                    }
                    if let Some(boot) = boot() {
                        aside { class: "ig-demo-desk",
                            ignition_live_ui::pointer::PointerRoot {
                                ignition_live_ui::live::Views { boot }
                            }
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
async fn launch(
    stage: &mut Signal<Stage>,
    on_boot: impl FnOnce(ignition_live_ui::Bootstrap),
) -> Result<(), String> {
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

    stage.set(Stage::Loading("the show"));
    let cuelist = fetch_text(&SHOW.to_string()).await.ok();
    let profile = files.profile_document.clone();

    // The desk's own vocabulary for this room, built by the SAME
    // function the studio uses — palettes, the busking groups, and the
    // cue rows. `r[studio.one-truth]`
    let profile_parsed = profile
        .as_deref()
        .and_then(|raw| serde_json::from_str::<ignition_core::Profile>(raw).ok());
    if let Some(p) = profile_parsed.clone() {
        ignition_live_ui::library::install_profile(p);
    }
    let boot = ignition_live_ui::Bootstrap {
        surface: ignition_live_ui::Surface::from_room(
            &venue.palettes,
            venue.groups().into_iter().map(|g| g.name).collect(),
            cue_rows(cuelist.as_deref()),
        ),
        // No fader banks and no operator file: both are desk-local state
        // a page has nowhere to keep. The banks come back empty, which
        // the fader strip draws as an empty bank rather than as an error.
        banks: Vec::new(),
        operator: // `starter` rather than a stored operator: a page has no
        // operator file, and the starter is what a desk shows the first
        // time it is opened too.
        ignition_live_ui::operators::Operator::starter("Demo"),
        profile: profile_parsed,
        lan: Vec::new(),
    };
    on_boot(boot);

    stage.set(Stage::Loading("the visualizer"));
    ignition_viz::run_web(
        venue,
        &ignition_viz::WebShow {
            cuelist: cuelist.as_deref(),
            profile: profile.as_deref(),
            // No song map: the show's positions are already absolute
            // bars. A map would only matter if the arrangement had been
            // re-cut since it was written.
            song: None,
        },
        "#ig-viz",
        &VIZ_ASSETS.to_string(),
        // The page's end of the desk — the same object `live-ui`'s
        // `send` was installed against, so the surface and the engine
        // are talking about the same show.
        Some(crate::routes::desk::desk()),
    );
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[expect(
    clippy::unused_async,
    clippy::future_not_send,
    reason = "the wasm twin of this is an async fn holding `!Send` futures, and the two \
              signatures have to match at the call site"
)]
async fn launch(
    _stage: &mut Signal<Stage>,
    _on_boot: impl FnOnce(ignition_live_ui::Bootstrap),
) -> Result<(), String> {
    Err("the demo only runs in a browser".to_string())
}

/// The cue list as rows for the surface.
///
/// `cuelist::rows` is the studio's builder too — it reads the file and
/// calls this; the page fetched the document and calls it directly.
#[cfg(target_arch = "wasm32")]
fn cue_rows(cuelist: Option<&str>) -> Vec<ignition_live_ui::Row> {
    cuelist
        .and_then(|raw| serde_json::from_str::<ignition_core::CueList>(raw).ok())
        .map(|list| ignition_live_ui::cuelist::rows(&list, None))
        .unwrap_or_default()
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
