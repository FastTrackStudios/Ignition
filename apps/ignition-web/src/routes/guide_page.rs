//! The guide — one vault note, with its table of contents and its local
//! graph.
//!
//! Three columns: the contents on the left, the note in the middle, and
//! on the right the **local graph** — this concept and everything one hop
//! from it, drawn and clickable. That is the view that makes a vault feel
//! like a vault rather than a chapter list: you can see what a page
//! touches, and go there, without reading it first.

use dioxus::prelude::*;
use view_knowledge_graph::model::{ColorMode, WikiGraph};

use crate::Route;
use crate::guide;
use crate::routes::Shell;

/// `/guide` — opens at the front door.
#[component]
pub fn GuideIndex() -> Element {
    let slug = guide::first_page().map_or_else(String::new, |p| p.slug.to_string());
    rsx! {
        GuidePage { slug }
    }
}

/// `/guide/:slug` — one note.
#[component]
pub fn GuidePage(slug: String) -> Element {
    let nav = navigator();

    let Some(page) = guide::page(&slug) else {
        return rsx! {
            Shell {
                section { class: "ig-prose",
                    h1 { "No such guide page" }
                    Link { to: Route::GuideIndex {}, class: "ig-button", "Back to the guide" }
                }
            }
        };
    };

    let graph = use_memo(guide::graph);
    let here = page.slug;
    let backlinks = use_memo(move || guide::backlinks(&graph.read(), here));
    let local = use_memo(move || guide::local_graph(&graph.read(), here));

    rsx! {
        Shell {
            div { class: "ig-guide",
                GuideToc { current: page.slug }

                article { class: "ig-guide-page",
                    ChapterNav { slug: page.slug, compact: true }

                    // The note. This HTML is what this crate's own build
                    // script produced from markdown in this repo — there
                    // is no untrusted author anywhere in the path, which
                    // is the only reason this is not the hole it looks
                    // like.
                    //
                    // The click handler is what keeps a wikilink inside
                    // the app: `build.rs` rewrote `[[slug]]` to an
                    // ordinary `/guide/slug` anchor, and left to itself
                    // the browser would reload the whole site to follow
                    // one. Delegated from the container rather than bound
                    // per link, because the links are inside opaque HTML
                    // and there is no element here to attach to.
                    div {
                        class: "ig-md",
                        dangerous_inner_html: "{page.html}",
                        onclick: move |evt| {
                            if let Some(route) = clicked_route(&evt) {
                                evt.prevent_default();
                                nav.push(route);
                            }
                        },
                    }

                    ChapterNav { slug: page.slug, compact: false }

                    Backlinks { pages: backlinks() }
                }

                LocalGraph { graph: local(), current: page.slug }
            }
        }
    }
}

/// The in-app route a click inside the rendered note asked for, if any.
///
/// Walks up from whatever was clicked to the nearest anchor and reads its
/// `href`. Only `/guide/…` is claimed: an external link, and a link to a
/// page that does not exist, are left to the browser.
#[cfg(target_arch = "wasm32")]
fn clicked_route(evt: &Event<MouseData>) -> Option<Route> {
    use wasm_bindgen::JsCast as _;

    let mouse = evt.data().downcast::<web_sys::MouseEvent>()?.clone();
    let target = mouse.target()?.dyn_into::<web_sys::Element>().ok()?;
    let href = target.closest("a").ok()??.get_attribute("href")?;
    let rest = href.strip_prefix("/guide")?;
    match rest.trim_start_matches('/') {
        "graph" => Some(Route::GuideGraph {}),
        "" => Some(Route::GuideIndex {}),
        slug if guide::page(slug).is_some() => Some(Route::GuidePage {
            slug: slug.to_owned(),
        }),
        _ => None,
    }
}

/// Off the web there is no DOM to read the click out of, and this crate
/// still builds for the host so `cargo check --workspace` and the tests
/// mean something.
#[cfg(not(target_arch = "wasm32"))]
const fn clicked_route(_evt: &Event<MouseData>) -> Option<Route> {
    None
}

/// This note and everything one hop from it.
///
/// Nodes are coloured by community, matching the full graph view, so the
/// two reads agree about which cluster a concept belongs to. Clicking a
/// node navigates.
#[component]
fn LocalGraph(graph: WikiGraph, current: &'static str) -> Element {
    let nav = navigator();

    // A page with no links has nothing to draw, and an empty box beside
    // the text reads as broken rather than as "no connections".
    if graph.nodes.len() < 2 {
        return rsx! {};
    }

    rsx! {
        aside { class: "ig-local-graph",
            h2 { "Connections" }
            div { class: "ig-local-graph-canvas",
                view_knowledge_graph::KnowledgeGraphView {
                    graph,
                    color_mode: ColorMode::Community,
                    // Marks the page you are on without dimming the rest —
                    // `highlighted` would, and here everything IS relevant.
                    active: Some(current.to_string()),
                    // Tighter than the full view: this is a handful of
                    // nodes in a narrow rail, not a whole map.
                    spacing: 0.45,
                    node_scale: 0.75,
                    on_node_click: move |id: String| {
                        if guide::page(&id).is_some() {
                            nav.push(Route::GuidePage { slug: id });
                        }
                    },
                }
            }
            Link { to: Route::GuideGraph {}, class: "ig-note", "See the whole graph" }
        }
    }
}

/// The table of contents, in reading order, under its stages.
///
/// The notes are one path from "what is this" to "how do I run it", and a
/// flat list of seven links hides that — it reads as reference material
/// you dip into. The stage headings say where the path is going and,
/// more usefully, where someone can stop.
///
/// The stage comes from each note's frontmatter and a heading is emitted
/// whenever it changes, so the order of the headings is the reading order
/// by construction and a chapter cannot appear under a stage it does not
/// belong to.
#[component]
fn GuideToc(current: &'static str) -> Element {
    let mut stage = "";
    rsx! {
        nav { class: "ig-guide-toc",
            for entry in guide::GUIDE_PAGES {
                if entry.stage != stage {
                    {
                        stage = entry.stage;
                        rsx! { span { class: "ig-toc-stage", "{entry.stage}" } }
                    }
                }
                Link {
                    to: Route::GuidePage { slug: entry.slug.to_string() },
                    class: if entry.slug == current { "ig-toc-current" } else { "" },
                    // The note's `blurb:`. A title is a name; the blurb
                    // is what the chapter is about, and a sidebar with
                    // both in it stops being a sidebar.
                    title: entry.blurb,
                    "{entry.title}"
                }
            }
            Link { to: Route::GuideGraph {}, class: "ig-toc-graph", "Graph" }
        }
    }
}

/// Previous and next, above and below the note.
///
/// Both copies read from [`guide::neighbours`], which reads
/// `GUIDE_PAGES`, which is what the table of contents renders — so the
/// buttons, the sidebar and the reading order are one fact with one
/// source. The prose footer these replace still lives in each note's
/// `source`, where it is most of the graph's edges; `build.rs` strips it
/// from the rendered body.
///
/// `compact` is the top copy: the same two links with the labels and the
/// boxes dropped, because a full pair of cards above the title competes
/// with the title.
#[component]
fn ChapterNav(slug: &'static str, compact: bool) -> Element {
    let (previous, next) = guide::neighbours(slug);
    if previous.is_none() && next.is_none() {
        return rsx! {};
    }
    rsx! {
        nav {
            class: if compact { "ig-chapter-nav ig-chapter-nav-top" } else { "ig-chapter-nav" },
            "aria-label": "Chapters",
            if let Some(p) = previous {
                Link {
                    to: Route::GuidePage { slug: p.slug.to_string() },
                    class: "ig-chapter-link ig-chapter-prev",
                    span { class: "ig-chapter-dir", "Previous" }
                    span { class: "ig-chapter-title", "{p.title}" }
                }
            }
            if let Some(n) = next {
                Link {
                    to: Route::GuidePage { slug: n.slug.to_string() },
                    class: "ig-chapter-link ig-chapter-next",
                    span { class: "ig-chapter-dir", "Next" }
                    span { class: "ig-chapter-title", "{n.title}" }
                }
            }
        }
    }
}

/// What leads here.
///
/// Rendered only when there is something to show — an empty "Referenced
/// by" heading is worse than none.
#[component]
fn Backlinks(pages: Vec<&'static guide::GuidePage>) -> Element {
    if pages.is_empty() {
        return rsx! {};
    }
    rsx! {
        footer { class: "ig-backlinks",
            h2 { "Referenced by" }
            ul {
                for p in pages {
                    li { key: "{p.slug}",
                        Link { to: Route::GuidePage { slug: p.slug.to_string() }, "{p.title}" }
                    }
                }
            }
        }
    }
}
