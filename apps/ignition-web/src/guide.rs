//! The guide, as a vault.
//!
//! `docs/guides/*.md` are wiki notes: frontmatter, `[[wikilink]]`
//! cross-references, and a `Previous: … · Next: … · Up: …` footer.
//! `build.rs` compiles them in; everything that makes them a *guide*
//! rather than a pile of files happens here and in [`crate::routes`].
//!
//! Two consumers of the same text:
//!
//! - the **page**, which renders `html` — the note without its
//!   frontmatter or footer, with wikilinks rewritten to routes.
//! - the **graph**, which reads `source` — the note verbatim — and turns
//!   the whole set into a force-directed map of how the concepts connect.
//!   `view-knowledge-graph` is the crate that draws Task's own wiki, and
//!   it is pure and FS-free by design, so the same builder runs here in
//!   the browser.

use view_knowledge_graph::model::WikiGraph;
use view_knowledge_graph::parse::WikiFile;

/// One note of the guide.
#[derive(PartialEq, Eq)]
pub struct GuidePage {
    /// URL segment, from the filename. Also the wikilink target.
    pub slug: &'static str,
    /// Display title, from the frontmatter.
    pub title: &'static str,
    /// One line for the table of contents, from `blurb:`.
    pub blurb: &'static str,
    /// Sort key, from `order:`. Pages without one sort last.
    pub order: u32,
    /// The part of the guide this note belongs to, from `stage:` — the
    /// heading it sits under in the table of contents.
    pub stage: &'static str,
    /// The note verbatim, frontmatter and footer included. What the graph
    /// reads: `type:` classifies the node, and most of the edges here come
    /// from the footer's wikilinks.
    pub source: &'static str,
    /// The note rendered for a reader — no frontmatter, no footer,
    /// wikilinks rewritten to `/guide/…`.
    pub html: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/guide_generated.rs"));

/// Look up a note by its URL slug.
#[must_use]
pub fn page(slug: &str) -> Option<&'static GuidePage> {
    GUIDE_PAGES.iter().find(|p| p.slug == slug)
}

/// The front door — the first note in reading order.
///
/// `Option`, though `build.rs` refuses to generate an empty guide: the
/// panic lints are the house rule, and "this cannot happen" is exactly
/// the claim they exist to stop anyone from making.
#[must_use]
pub fn first_page() -> Option<&'static GuidePage> {
    GUIDE_PAGES.first()
}

/// The notes either side of `slug`, in reading order.
///
/// Derived from `GUIDE_PAGES`, which is the same order the table of
/// contents renders, so the buttons cannot disagree with the sidebar.
#[must_use]
pub fn neighbours(slug: &str) -> (Option<&'static GuidePage>, Option<&'static GuidePage>) {
    let Some(at) = GUIDE_PAGES.iter().position(|p| p.slug == slug) else {
        return (None, None);
    };
    (
        at.checked_sub(1).and_then(|i| GUIDE_PAGES.get(i)),
        at.checked_add(1).and_then(|i| GUIDE_PAGES.get(i)),
    )
}

/// The guide as the graph builder wants it.
fn wiki_files() -> Vec<WikiFile> {
    GUIDE_PAGES
        .iter()
        .map(|p| WikiFile {
            name: format!("{}.md", p.slug),
            // The route, not a filesystem path: the graph surfaces this
            // for click-to-open, and in a browser "open" means navigate.
            path: format!("/guide/{}", p.slug),
            content: p.source.to_string(),
        })
        .collect()
}

/// The guide's knowledge graph — nodes, relevance-weighted edges, and
/// communities detected from the link structure rather than declared.
///
/// Built rather than cached: it is a pure function of text baked into the
/// binary, and the builder is fast enough to run on demand. Callers hold
/// it in a `use_memo`.
#[must_use]
pub fn graph() -> WikiGraph {
    view_knowledge_graph::build_wiki_graph(&wiki_files())
}

/// Notes that link *to* `slug` — the wiki's answer to "what leads here".
#[must_use]
pub fn backlinks(graph: &WikiGraph, slug: &str) -> Vec<&'static GuidePage> {
    let mut out: Vec<&'static GuidePage> = graph
        .edges
        .iter()
        .filter(|e| e.target == slug && e.source != slug)
        .filter_map(|e| page(&e.source))
        .collect();
    out.sort_by_key(|p| p.order);
    out.dedup_by_key(|p| p.slug);
    out
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

/// Wikilink targets in a note, ignoring the `|label` half.
///
/// Only the tests need this — the rendered page gets its links from
/// `build.rs`, and the graph finds its own. It lives here rather than in
/// the test module because it is a fact about the note format, not about
/// the test.
#[cfg(test)]
#[must_use]
pub fn wikilink_targets(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some((_, after_open)) = rest.split_once("[[") {
        let Some((inner, after_close)) = after_open.split_once("]]") else {
            break;
        };
        out.push(
            inner
                .split_once('|')
                .map_or(inner, |(t, _)| t)
                .trim()
                .to_owned(),
        );
        rest = after_close;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_is_not_empty() {
        assert!(GUIDE_PAGES.len() >= 5, "the whole guide should compile in");
    }

    #[test]
    fn pages_are_ordered_by_frontmatter() {
        let orders: Vec<u32> = GUIDE_PAGES.iter().map(|p| p.order).collect();
        let mut sorted = orders.clone();
        sorted.sort_unstable();
        assert_eq!(orders, sorted, "guide pages must arrive in reading order");
    }

    #[test]
    fn slugs_are_unique() {
        let mut slugs: Vec<&str> = GUIDE_PAGES.iter().map(|p| p.slug).collect();
        slugs.sort_unstable();
        let before = slugs.len();
        slugs.dedup();
        assert_eq!(before, slugs.len(), "two guide pages share a slug");
    }

    #[test]
    fn every_page_has_a_title_a_blurb_and_a_body() {
        for p in GUIDE_PAGES {
            assert!(!p.title.is_empty(), "guide page `{}` has no title", p.slug);
            assert!(
                !p.blurb.is_empty(),
                "guide page `{}` has no `blurb:` for the table of contents",
                p.slug
            );
            assert!(p.source.len() > 100, "guide page `{}` looks empty", p.slug);
            assert!(
                p.html.contains("<h1>"),
                "guide page `{}` lost its own `# heading`",
                p.slug
            );
        }
    }

    #[test]
    fn the_rendered_page_drops_the_frontmatter_and_the_footer() {
        for p in GUIDE_PAGES {
            assert!(
                !p.html.contains("stage:"),
                "guide page `{}` carried its frontmatter into the rendered body",
                p.slug
            );
            assert!(
                !p.html.contains("Up: "),
                "guide page `{}` prints its nav footer as well as the buttons",
                p.slug
            );
        }
    }

    #[test]
    fn wikilinks_render_as_routes() {
        let index = first_page().expect("the guide has a front door");
        assert!(
            index.html.contains("href=\"/guide/"),
            "the front door's wikilinks did not become links"
        );
        assert!(
            !index.html.contains("[["),
            "a raw wikilink survived into the rendered page"
        );
    }

    #[test]
    fn every_wikilink_points_at_a_real_page() {
        // A dead `[[link]]` renders as a link that goes nowhere. This is
        // what notices.
        for p in GUIDE_PAGES {
            for target in wikilink_targets(p.source) {
                assert!(
                    page(&target).is_some(),
                    "guide page `{}` links to `[[{target}]]`, which does not exist",
                    p.slug
                );
            }
        }
    }

    /// The slug a `Previous:`/`Next:`/`Up:` footer entry points at.
    ///
    /// Read from `source`, not `html`: the site strips the footer before
    /// rendering — it shows real buttons instead — but the note keeps it,
    /// because those wikilinks are most of the graph's edges.
    fn footer_link(source: &str, label: &str) -> Option<String> {
        let line = source.lines().find(|l| l.contains("Up: [["))?;
        let after = line.split_once(&format!("{label}: [["))?.1;
        let target = after.split_once("]]")?.0;
        Some(target.split('|').next().unwrap_or(target).to_owned())
    }

    #[test]
    fn the_chapters_form_one_unbroken_chain() {
        // The footer and the `order:` are written in different places and
        // nothing else compares them. This does: a chapter that points at
        // the wrong neighbour disagrees with the table of contents, and a
        // reader following the buttons gets a different guide from a
        // reader following the sidebar.
        //
        // Everything after the front door, which carries no footer — it
        // opens the guide rather than continuing it.
        let chapters: Vec<_> = GUIDE_PAGES.iter().skip(1).collect();
        assert!(chapters.len() > 1, "the guide should have chapters");

        for (i, p) in chapters.iter().enumerate() {
            match chapters.get(i.saturating_add(1)) {
                Some(next) => assert_eq!(
                    footer_link(p.source, "Next").as_deref(),
                    Some(next.slug),
                    "`{}` should send the reader to `{}`",
                    p.slug,
                    next.slug
                ),
                // The last chapter closes the tour instead of pointing on.
                None => assert!(
                    footer_link(p.source, "Next").is_none(),
                    "`{}` is the last chapter and should not have a Next",
                    p.slug
                ),
            }
            if i > 0 {
                assert_eq!(
                    footer_link(p.source, "Previous").as_deref(),
                    chapters.get(i.saturating_sub(1)).map(|q| q.slug),
                    "`{}` should come back to the chapter before it",
                    p.slug
                );
            }
            assert_eq!(
                footer_link(p.source, "Up").as_deref(),
                first_page().map(|f| f.slug),
                "`{}` should link up to the front door, so no chapter is a dead end",
                p.slug
            );
        }
    }

    #[test]
    fn every_chapter_belongs_to_a_stage_and_the_stages_run_in_order() {
        // The table of contents emits a heading whenever the stage
        // changes, so a stage whose chapters are not contiguous would
        // print its heading twice and split the run under it.
        let mut seen: Vec<&str> = Vec::new();
        let mut current = "";
        for p in GUIDE_PAGES {
            assert!(
                !p.stage.is_empty(),
                "chapter `{}` has no `stage:` in its frontmatter",
                p.slug
            );
            if p.stage != current {
                assert!(
                    !seen.contains(&p.stage),
                    "stage `{}` appears twice — `{}` is out of order",
                    p.stage,
                    p.slug
                );
                seen.push(p.stage);
                current = p.stage;
            }
        }
    }

    #[test]
    fn the_graph_has_a_node_per_page() {
        assert_eq!(
            graph().nodes.len(),
            GUIDE_PAGES.len(),
            "every guide page should be a node"
        );
    }

    #[test]
    fn the_graph_is_connected_by_wikilinks() {
        // The guide cross-references itself throughout — that web is the
        // whole reason it is a vault rather than a chapter list. If the
        // edges vanish, either the links broke or the builder stopped
        // seeing them.
        let g = graph();
        assert!(
            g.edges.len() >= GUIDE_PAGES.len(),
            "only {} edges across {} pages — the wikilink web is missing",
            g.edges.len(),
            GUIDE_PAGES.len()
        );
    }

    #[test]
    fn the_front_door_leads_somewhere() {
        let g = graph();
        let first = first_page().expect("the guide has a front door");
        assert!(
            g.edges.iter().any(|e| e.source == first.slug),
            "the front door `{}` links to nothing — the guide has no entry path",
            first.slug
        );
    }

    #[test]
    fn backlinks_are_the_reverse_of_links() {
        let g = graph();
        let back = backlinks(&g, "the-four-files");
        assert!(
            !back.is_empty(),
            "nothing links to `the-four-files`, which the front door should"
        );
        assert!(
            back.iter().all(|p| p.slug != "the-four-files"),
            "a page must not be its own backlink"
        );
    }

    #[test]
    fn the_local_graph_is_the_page_and_its_neighbours() {
        let g = graph();
        let local = local_graph(&g, "recipes-cues-effects");

        assert!(
            local.nodes.iter().any(|n| n.id == "recipes-cues-effects"),
            "the local graph must contain the page it is about"
        );
        assert!(
            local.nodes.len() > 1,
            "`recipes-cues-effects` should have neighbours — it is a hub"
        );
        let ids: Vec<&str> = local.nodes.iter().map(|n| n.id.as_str()).collect();
        for e in &local.edges {
            assert!(
                ids.contains(&e.source.as_str()) && ids.contains(&e.target.as_str()),
                "edge {} -> {} dangles outside the local node set",
                e.source,
                e.target
            );
        }
    }

    #[test]
    fn an_unknown_slug_gives_an_empty_local_graph_not_a_panic() {
        let g = graph();
        assert!(local_graph(&g, "no-such-page").nodes.is_empty());
    }
}
