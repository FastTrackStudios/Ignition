# blitz-dom — vendored copy

Upstream: crates.io `blitz-dom` 0.3.0-beta.1
(https://github.com/dioxuslabs/blitz), licensed MIT OR Apache-2.0 — see
`LICENSE-MIT` and `LICENSE-APACHE`, copied from the upstream repository.
The workspace `[patch.crates-io]` table points every `blitz-dom = "=0.3.0-beta.1"`
dependency (the studio app and the vendored `dioxus-native`) here.

## Why

The studio crashed with

    panicked at blitz-dom-0.3.0-beta.1/src/traversal.rs:320:32:
    index out of bounds: the len is 1 but the index is 18446744073709551615

`compare_document_order` builds root-to-node ancestor chains for the two
selection endpoints and indexes `chain_a[common_depth - 1]`. When a mouse
drag over the dock (tab drag, splitter, fader, tile) starts a Blitz text
selection and a Dioxus re-render then removes the anchor node, that node's
chain no longer starts at the root, `common_depth` is 0 and the index
underflows. Upstream only `debug_assert!`s that this "is impossible".

## What changed (every hunk is marked `IGNITION PATCH`)

- `src/mutator.rs` `process_removed_subtree`: the real fix. Alongside the
  existing hover/active clearing, a removed node that is the text
  selection's anchor or focus (`node_or_parent`) clears the selection, and
  a removed `mousedown_node_id` is cleared. Upstream 0.3.0-beta.1 does not
  do this; the selection kept stale node ids across removals.
- `src/traversal.rs` `compare_document_order`: returns `Ordering::Equal`
  when the chains share no ancestor instead of indexing `common_depth - 1`.
- `src/traversal.rs` `collect_inline_roots_in_range`: returns an empty
  `Vec` when either resolved anchor has no path to the root
  (new `BaseDocument::is_attached_to_root`).
- `src/traversal.rs`: `detached_node_tests` covering all three.
- `Cargo.toml`: generated header replaced, `publish = false`.
