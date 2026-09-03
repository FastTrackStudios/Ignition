//! The landing page.
//!
//! One screen, no scrolling: what Ignition is on the left, the room on
//! the right. Three words carry the whole claim — lighting, projection
//! mapping, graphics — because the thing being argued is that they are
//! one toolkit and not three, and a page that needed a paragraph to say
//! so would be arguing the opposite.
//!
//! The right-hand pane is a **real render of the visualizer** playing a
//! real show file. A still photograph of a stage cannot make the claim
//! this page makes; the thing being claimed is motion.

use dioxus::prelude::*;

use crate::routes::Shell;
use crate::{REPO, Route};

#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const HERO_VIDEO: Asset = asset!("/assets/hero.mp4");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const HERO_POSTER: Asset = asset!("/assets/hero-poster.jpg");

#[component]
pub fn Home() -> Element {
    rsx! {
        Shell {
            section { class: "ig-hero",
                div { class: "ig-hero-copy",
                    p { class: "ig-eyebrow", "Pre-alpha · GPL-3.0 · Rust" }
                    h1 { class: "ig-display",
                        span { class: "ig-pillar", "Lighting." }
                        span { class: "ig-pillar", "Projection mapping." }
                        span { class: "ig-pillar", "Graphics." }
                    }
                    // The unifying claim, and the one thing on the page
                    // that moves. It gets the gradient because it is what
                    // the three lines above are FOR: they are a list until
                    // something says they are one product.
                    p { class: "ig-kicker",
                        span { class: "ig-accel", "One visual production toolkit." }
                    }
                    p { class: "ig-lede",
                        "A DMX console, a real-time 3D visualizer, and video mapped onto the \
                         actual surfaces in the room — one application, one show file. The cues \
                         fall on the bar line, because the desk is reading the same session the \
                         band is playing to."
                    }
                    div { class: "ig-cta",
                        Link { to: Route::GuideIndex {}, class: "ig-button ig-button-primary",
                            "Read the guide"
                        }
                        a { href: REPO, rel: "noreferrer", class: "ig-button", "View on GitHub" }
                    }
                    p { class: "ig-note",
                        "Open source, and pre-alpha in the honest sense: it runs, it is not done."
                    }
                }

                div { class: "ig-window",
                    video {
                        class: "ig-window-video",
                        src: HERO_VIDEO,
                        poster: HERO_POSTER,
                        r#loop: true,
                        playsinline: true,
                        preload: "auto",
                        // NOT `autoplay: true` / `muted: true`, and this
                        // is the whole reason there is a handler here.
                        //
                        // A browser blocks an unmuted clip from starting
                        // itself, and `muted` is only initialised from the
                        // attribute when the PARSER builds the element.
                        // Dioxus creates the element and then applies
                        // attributes, so `muted="true"` lands on a node
                        // whose `muted` PROPERTY stays false — the live
                        // site showed the poster frame forever, with the
                        // attribute plainly there in the DOM.
                        //
                        // Setting the property is the fix, and once it is
                        // set the play has to be asked for explicitly,
                        // because the autoplay attempt already happened
                        // and already failed.
                        onmounted: move |evt| autoplay_muted(&evt),
                    }
                }
            }
        }
    }
}

/// Mute the element and start it — see the comment at the call site.
///
/// A rejected `play()` is ignored on purpose: a browser that refuses
/// leaves the poster frame up, which is a still of the same render and
/// a perfectly good picture.
#[cfg(target_arch = "wasm32")]
fn autoplay_muted(evt: &Event<MountedData>) {
    use wasm_bindgen::JsCast as _;

    let data = evt.data();
    let Some(element) = data.downcast::<web_sys::Element>() else {
        return;
    };
    let Some(video) = element.dyn_ref::<web_sys::HtmlMediaElement>() else {
        return;
    };
    video.set_muted(true);
    // The PROPERTY, not the attribute, and both of them: `muted` is what
    // makes autoplay permissible, `autoplay` is what makes the browser
    // start it on its own once enough data has arrived. The explicit
    // `play()` below covers the case where that data is already there.
    //
    // Both are needed. With only the `play()` the clip started and then
    // stopped again at t=0.05 whenever the element's `src` was applied
    // after the call — a rejected-then-interrupted promise, and the
    // failure was intermittent, which is the worst way to find it.
    video.set_autoplay(true);
    let _ = video.play();
}

/// Off the web there is no element to mount, and this crate still builds
/// for the host so `cargo check --workspace` and the tests mean something.
#[cfg(not(target_arch = "wasm32"))]
const fn autoplay_muted(_evt: &Event<MountedData>) {}
