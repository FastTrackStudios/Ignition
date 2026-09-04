//! The guide's knowledge graph.
//!
//! The guide is a vault, and a vault's shape is its link web. This is that
//! web, drawn: a force-directed map of how the concepts reference each
//! other, with communities detected from the link structure rather than
//! declared by hand.
//!
//! It is a genuinely useful way in. A linear table of contents says
//! "recipes is chapter four"; the graph says recipes is the hub that
//! selections, the song and the cue player all lean on, which is a
//! different and more honest description of what to read next.
//!
//! `view-knowledge-graph` does the work — the same crate that draws Task's
//! wiki. It is pure and FS-free, so it runs here in wasm unchanged.

use dioxus::prelude::*;
use view_knowledge_graph::{GraphLegend, KnowledgeGraphView, model::ColorMode};

use crate::Route;
use crate::guide;
use crate::routes::Shell;

/// `/guide/graph` — the whole guide as a link web.
#[component]
pub fn GuideGraph() -> Element {
    let nav = navigator();
    let graph = use_memo(guide::graph);
    let mut query = use_signal(String::new);

    // Search highlights rather than filters: dropping unmatched nodes
    // would also drop the edges that give a match its context, which is
    // the thing you came to the graph for.
    let highlighted = use_memo(move || {
        let q = query.read().clone();
        if q.trim().is_empty() {
            return Vec::new();
        }
        let g = graph.read();
        let mut ids: Vec<String> =
            view_knowledge_graph::search::apply_search(&g.nodes, &g.edges, &q)
                .matched_ids
                .into_iter()
                .collect();
        // A HashSet's order is not stable across runs, and this feeds a
        // prop that drives rendering — sort so the view does not churn.
        ids.sort_unstable();
        ids
    });

    rsx! {
        Shell {
            div { class: "ig-graph-screen",
                header { class: "ig-graph-bar",
                    h1 { "The guide, as a graph" }
                    input {
                        class: "ig-graph-search",
                        r#type: "search",
                        placeholder: "Highlight…",
                        value: "{query}",
                        oninput: move |e| query.set(e.value()),
                    }
                    Link { to: Route::GuideIndex {}, class: "ig-button", "Read it in order" }
                }

                div { class: "ig-graph-canvas",
                    KnowledgeGraphView {
                        graph: graph(),
                        color_mode: ColorMode::Community,
                        // A seven-page guide is a handful of nodes in a
                        // wide frame, and the view sizes labels from the
                        // node radius — at the default scale the titles
                        // came out the size of headlines. Smaller nodes,
                        // more spread between them.
                        node_scale: 0.5,
                        spacing: 1.6,
                        highlighted: highlighted(),
                        on_node_click: move |id: String| {
                            if guide::VAULT.page(&id).is_some() {
                                nav.push(Route::GuidePage { slug: id });
                            }
                        },
                    }
                }

                aside { class: "ig-graph-legend",
                    GraphLegend {
                        nodes: graph().nodes,
                        communities: graph().communities,
                        color_mode: ColorMode::Community,
                        on_toggle_kind: move |_| {},
                        on_show_all: move |()| {},
                    }
                }
            }
        }
    }
}
