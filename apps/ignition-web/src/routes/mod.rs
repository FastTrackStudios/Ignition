//! The site's screens, and the chrome they share.

mod demo;
mod graph;
mod guide_page;
mod home;

pub use demo::Demo;
pub use graph::GuideGraph;
pub use guide_page::{GuideIndex, GuidePage};
pub use home::Home;

use dioxus::prelude::*;

use crate::{REPO, Route};

/// Shared chrome: the header every screen sits under.
#[component]
pub fn Shell(children: Element) -> Element {
    rsx! {
        div { class: "ig-shell",
            header { class: "ig-header",
                Link { to: Route::Home {}, class: "ig-wordmark",
                    span { class: "ig-spark" }
                    "Ignition"
                }
                nav { class: "ig-nav",
                    Link { to: Route::GuideIndex {}, "Guide" }
                    Link { to: Route::GuideGraph {}, "Graph" }
                    a { href: REPO, rel: "noreferrer", "GitHub" }
                }
            }
            main { class: "ig-main", {children} }
        }
    }
}

/// Anything the router could not match.
#[component]
pub fn NotFound(segments: Vec<String>) -> Element {
    rsx! {
        Shell {
            section { class: "ig-prose",
                h1 { "Not found" }
                p { "There is no page at /{segments.join(\"/\")}." }
                Link { to: Route::GuideIndex {}, class: "ig-button", "Read the guide" }
            }
        }
    }
}
