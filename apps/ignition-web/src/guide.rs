//! The guide, as a vault.
//!
//! `docs/guides/*.md` are wiki notes: frontmatter, `[[wikilink]]`
//! cross-references, and a `Previous: … · Next: … · Up: …` footer.
//! `build.rs` hands them to `ssg-build`, which renders them and codegens
//! the page table this module includes; everything that makes them a
//! *guide* rather than a pile of files happens here and in
//! [`crate::routes`].
//!
//! Two consumers of the same text:
//!
//! - the **page**, which renders `html` — the note without its
//!   frontmatter or footer, with wikilinks rewritten to routes.
//! - the **graph**, which reads `source` — the note verbatim — and turns
//!   the whole set into a force-directed map of how the concepts
//!   connect. `view-knowledge-graph` is the crate that draws Task's own
//!   wiki, and it is pure and FS-free by design, so the same builder
//!   runs here in the browser.
//!
//! The guide is also **pre-rendered**: `dx build --ssg` writes each of
//! its routes out as a finished `index.html`, so the notes arrive as
//! text rather than as a program that produces text. The bundle then
//! hydrates the page into the ordinary app — which is what keeps the
//! graph interactive and wikilinks navigating in place.

use view_knowledge_graph::model::WikiGraph;
use view_knowledge_graph::parse::WikiFile;

// `pub static VAULT: ssg::StaticVault`, from `build.rs`.
ssg::include_vault!();

/// Where the guide is published, as a URL prefix.
///
/// `build.rs` resolves `[[wikilinks]]` against this, and
/// `crate::static_routes` enumerates the pages under it for the
/// pre-render — the two have to agree.
pub const BASE: &str = "/guide";

/// The guide as the graph builder wants it.
///
/// Built on demand rather than compiled in: a serialised graph would be
/// a second copy of every note in the binary, and the builder is fast
/// enough to run when asked. Callers hold it in a `use_memo`.
fn wiki_files() -> Vec<WikiFile> {
    VAULT
        .pages
        .iter()
        .map(|page| WikiFile {
            name: format!("{}.md", page.slug),
            path: format!("{}.md", page.slug),
            content: page.source.to_owned(),
        })
        .collect()
}

/// The whole guide as a link graph.
#[must_use]
pub fn graph() -> WikiGraph {
    view_knowledge_graph::build_wiki_graph(&wiki_files())
}

/// The neighbourhood around one note — the local graph.
///
/// The whole-guide graph answers "how is this body of writing shaped".
/// Beside a page you want the other question: what does *this* concept
/// touch. So this is the node, everything one hop away, and the edges
/// among that set.
///
/// One hop, not two: at two hops a guide this size is almost fully
/// connected, and a map of everything is a map of nothing.
#[must_use]
pub fn local_graph(graph: &WikiGraph, slug: &str) -> WikiGraph {
    use std::collections::HashSet;

    let mut keep: HashSet<&str> = HashSet::new();
    keep.insert(slug);
    for e in &graph.edges {
        if e.source == slug {
            keep.insert(e.target.as_str());
        } else if e.target == slug {
            keep.insert(e.source.as_str());
        }
    }

    WikiGraph {
        nodes: graph
            .nodes
            .iter()
            .filter(|n| keep.contains(n.id.as_str()))
            .cloned()
            .collect(),
        edges: graph
            .edges
            .iter()
            .filter(|e| keep.contains(e.source.as_str()) && keep.contains(e.target.as_str()))
            .cloned()
            .collect(),
        // Community colouring is a property of the WHOLE graph; carried
        // through so a node keeps the colour it has in the full view and
        // the two reads agree.
        communities: graph.communities.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_is_not_empty() {
        assert!(
            VAULT.pages.len() >= 5,
            "the whole guide should compile in"
        );
    }

    #[test]
    fn pages_are_ordered_by_frontmatter() {
        let orders: Vec<u32> = VAULT.pages.iter().map(|p| p.order).collect();
        let mut sorted = orders.clone();
        sorted.sort_unstable();
        assert_eq!(orders, sorted, "guide pages must arrive in reading order");
    }

    #[test]
    fn slugs_are_unique() {
        let mut slugs: Vec<&str> = VAULT.pages.iter().map(|p| p.slug).collect();
        slugs.sort_unstable();
        let before = slugs.len();
        slugs.dedup();
        assert_eq!(before, slugs.len(), "two guide pages share a slug");
    }

    #[test]
    fn every_page_has_a_title_a_blurb_and_a_body() {
        for p in VAULT.pages {
            assert!(!p.title.is_empty(), "guide page `{}` has no title", p.slug);
            assert!(
                !p.summary.is_empty(),
                "guide page `{}` has no blurb for the contents list",
                p.slug
            );
            assert!(!p.html.is_empty(), "guide page `{}` rendered empty", p.slug);
        }
    }

    #[test]
    fn the_rendered_page_drops_the_frontmatter_and_the_footer() {
        for p in VAULT.pages {
            assert!(
                !p.html.contains("stage:"),
                "guide page `{}` still carries its frontmatter",
                p.slug
            );
            assert!(
                !p.html.contains("Up: "),
                "guide page `{}` still carries its nav footer",
                p.slug
            );
        }
    }

    /// Every `[[wikilink]]` resolves.
    ///
    /// `ssg-build` fails the build on an unresolved one, so this cannot
    /// fire — which is the point. It is here to say so, and to catch the
    /// day someone reaches for `allow_broken_links`.
    #[test]
    fn every_cross_reference_resolves() {
        for page in VAULT.pages {
            for target in page.links {
                assert!(
                    VAULT.page(target).is_some(),
                    "guide page `{}` links to `{target}`, which does not exist",
                    page.slug
                );
            }
        }
    }

    #[test]
    fn the_graph_has_a_node_per_page() {
        assert_eq!(graph().nodes.len(), VAULT.pages.len());
    }

    #[test]
    fn a_local_graph_is_the_page_and_its_neighbours() {
        let whole = graph();
        for page in VAULT.pages {
            let local = local_graph(&whole, page.slug);
            assert!(
                local.nodes.iter().any(|n| n.id == page.slug),
                "the local graph of `{}` does not contain it",
                page.slug
            );
            assert!(local.nodes.len() <= whole.nodes.len());
        }
    }
}
