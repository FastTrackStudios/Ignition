//! Compile the guide vault, `docs/guides/*.md`, into the site.
//!
//! The guide is a **vault**, not a chapter list: the notes carry
//! frontmatter, they cross-reference each other as `[[slug]]`, and the
//! site draws that web as a knowledge graph. This is the same shape
//! Task's wiki has and the same shape Keyflow's guide has, so the same
//! tooling reads all three — `ssg-build`, which is what this file is now
//! a call to.
//!
//! It replaces about two hundred lines that did the job here alone: a
//! frontmatter reader, a wikilink rewriter, a nav-footer stripper and a
//! markdown pass. Ignition's version was the *right* one — it rendered
//! at build time while the other sites shipped markdown to the browser —
//! and that is the shape the shared crate took.
//!
//! Both forms of each note survive the move, because they are not
//! interchangeable:
//!
//! - `source` is the note **verbatim**, frontmatter and nav footer
//!   included. That is what the graph builder reads — `type:` classifies
//!   the node, and most of the edges in this guide come from the
//!   footer's `Previous:`/`Next:`/`Up:` wikilinks.
//! - `html` is the note rendered for a reader: frontmatter gone, footer
//!   gone (the page draws real buttons from the same order the table of
//!   contents uses), and `[[wikilinks]]` rewritten to `/guide/…` links.
//!
//! Reading outside the crate is what a build script is for. `include_str!`
//! across the boundary would be invisible to cargo and would fail at
//! compile time rather than resolution time; the `cargo:rerun-if-changed`
//! lines `emit` prints are what make editing a guide page rebuild the
//! site.

fn main() {
    ssg_build::Vault::at("../../docs/guides")
        .link_base("/guide")
        .emit();
}
