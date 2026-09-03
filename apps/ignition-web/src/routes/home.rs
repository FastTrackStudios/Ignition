//! The landing page.
//!
//! One screen, no scrolling: copy on the left, the room on the right.
//! The right-hand pane is a **render of the actual visualizer** playing an
//! actual show file — the claim this page makes is that cues come out of
//! the song, and a still photograph of a stage cannot make it. A video
//! can, because the thing being claimed is motion.

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
                        "Lighting that "
                        // The gradient sweeps across this phrase alone,
                        // so it stays one span — a break inside it would
                        // restart the gradient on the second line box.
                        span { class: "ig-accel", "knows the song" }
                    }
                    p { class: "ig-lede",
                        "A DMX console, a GPU visualizer and projection mapping in one \
                         application. Patch a room, program in roles and looks rather than \
                         channel numbers, and let the cues fall on the bar line — because the \
                         desk is reading the same session the band is playing to."
                    }
                    ul { class: "ig-points",
                        li { "sACN and Art-Net out, GDTF fixture profiles in" }
                        li { "Real-time 3D visualizer — beams in haze, gobos, video screens" }
                        li { "Tracking cues, referenced palettes, recipes and phasers" }
                        li { "Desktop and web from one Dioxus codebase" }
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

                figure { class: "ig-window",
                    div { class: "ig-window-bar",
                        span { class: "ig-window-name", "norco · bye-bye-bye · CH 3" }
                        span { class: "ig-window-tag", "rendered live" }
                    }
                    // Autoplay needs `muted` — every browser blocks a clip
                    // with a soundtrack from starting itself, and this one
                    // has none to block. `playsinline` keeps iOS from
                    // taking it fullscreen on play.
                    video {
                        class: "ig-window-video",
                        src: HERO_VIDEO,
                        poster: HERO_POSTER,
                        autoplay: true,
                        r#loop: true,
                        muted: true,
                        playsinline: true,
                        preload: "auto",
                    }
                    figcaption { class: "ig-window-caption",
                        "The visualizer, rendered offline from "
                        code { "data/songs/bye-bye-bye.json" }
                        " against the Norco rig — the same show file the desk plays."
                    }
                }
            }
        }
    }
}
