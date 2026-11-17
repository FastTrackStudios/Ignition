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
- `src/resolve.rs`, `src/stylo.rs`: frame-stage `tracing` spans on the
  target `ignition::profile` — `blitz.resolve`, `blitz.style`,
  `blitz.layout` — behind this crate's `tracing` feature. Same profiler
  as the `dioxus-native-vendored` hunks: `r[studio.profiling]`,
  `docs/ops/profiling.md`, `crates/ignition-profile`.
- `src/resolve.rs`, `src/mutator.rs`: three hunks that together stop a
  frame which changes nothing from costing anything, marked
  `IGNITION PATCH (perf)`. `resolve` skips box construction and taffy
  when no node carries damage that layout has to answer for — the check
  sits after `resolve_stylist`, which is what discovers damage, and a
  scroll animation or a non-incremental document never skips.
  `set_attribute` no longer marks `ALL_DAMAGE` and a subtree restyle for
  `img src` specifically (upstream does it for every attribute, with a
  `TODO: make this fine grained` beside it), and `load_image` marks only
  `REPAINT` when the new picture is the same size as the old. The studio
  animates thumbnails by swapping `src` twelve times a second across a
  screenful of cards; that was a full layout of 5,509 nodes on half of
  all frames. See `r[studio.profiling]` and `docs/ops/profiling.md`.
