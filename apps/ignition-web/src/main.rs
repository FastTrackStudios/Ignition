//! ignition.fasttrackstudio.app
//!
//! Two things: a landing page that shows what Ignition is, and the
//! guide. There is no backend and no account — the site is a document
//! and a video, and the software it is about lives on the desk, not
//! here.
//!
//! The guide is a **vault**, the same way Task's wiki and Keyflow's guide
//! are: `docs/guides/*.md` are notes with frontmatter that cross-link as
//! `[[slug]]`, `build.rs` compiles them in, and `view-knowledge-graph`
//! turns that web into a real knowledge graph at `/guide/graph`. See
//! [`guide`].
//!
//! The hero is a **real render**, not a mockup: `assets/hero.mp4` comes
//! out of `ignition-viz`'s own offline exporter (`just site-video`),
//! frame by frame against the song's clock, from the same show file the
//! desk plays. A landing page for a visualizer that showed anything else
//! would be making a claim it had not met.

mod guide;
mod routes;

use dioxus::prelude::*;

/// Site routes.
///
/// Deliberately small: a landing page, the guide, and the guide's graph.
#[derive(Routable, Clone, PartialEq, Eq)]
#[rustfmt::skip]
pub enum Route {
    #[route("/")]
    Home {},
    #[route("/guide")]
    GuideIndex {},
    // Before `/guide/:slug`, or "graph" matches as a page slug.
    #[route("/guide/graph")]
    GuideGraph {},
    #[route("/guide/:slug")]
    GuidePage { slug: String },
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

/// Where "Source" and every "on GitHub" link points.
pub const REPO: &str = "https://github.com/FastTrackStudios/Ignition";

use routes::{GuideGraph, GuideIndex, GuidePage, Home, NotFound};

#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const SITE_CSS: Asset = asset!("/assets/site.css");
/// Compiled from `tailwind.css` at the crate root by `just
/// site-tailwind`. Not the site's styling — that is `site.css` — but the
/// guide's graph is `view-knowledge-graph`'s component and it is styled
/// in Tailwind utilities, so its classes have to exist somewhere. See the
/// long comment in `tailwind.css`.
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

/// The Ignition mark — a fixture and its beam. A COPY of
/// `apps/ignition-mobile/ios/icon.svg`, which is where the mark is
/// authored; `asset!` takes a path inside the crate, and reaching across
/// a crate boundary with `include_str!` is what this repo forbids and a
/// build script is the sanctioned answer to. A build script cannot help
/// here, because `asset!` needs a literal path at compile time and not an
/// `OUT_DIR` one. `just site-icons` re-copies it and re-renders the raster
/// fallbacks, so the sync is a command rather than a memory.
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const ICON_SVG: Asset = asset!("/assets/icon.svg");
/// The same mark rasterised. Safari has never taken an SVG favicon, and
/// `apple-touch-icon` must be a PNG by specification.
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const ICON_PNG: Asset = asset!("/assets/icon-32.png");
#[expect(
    clippy::volatile_composites,
    reason = "the asset! macro generates a const with volatile inner types; this is a limitation of the dioxus macro system"
)]
const ICON_APPLE: Asset = asset!("/assets/icon-180.png");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        // The tab icon. SVG first — it is the mark as authored, and it
        // stays sharp at whatever size a browser asks for; the PNG is
        // the fallback for the ones that will not take an SVG.
        document::Link { rel: "icon", r#type: "image/svg+xml", href: ICON_SVG }
        document::Link { rel: "icon", r#type: "image/png", sizes: "32x32", href: ICON_PNG }
        document::Link { rel: "apple-touch-icon", sizes: "180x180", href: ICON_APPLE }

        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link {
            rel: "preconnect",
            href: "https://fonts.gstatic.com",
            crossorigin: "anonymous",
        }
        document::Link {
            rel: "stylesheet",
            href: "https://fonts.googleapis.com/css2?family=Geist:wght@300..800&family=Geist+Mono:wght@400..600&display=swap",
        }
        // Order matters: Tailwind first (it carries the architect-ui
        // design tokens the graph resolves against), then the site's own
        // sheet, which must win where the two overlap.
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        document::Link { rel: "stylesheet", href: SITE_CSS }
        Router::<Route> {}
    }
}
