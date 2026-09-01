//! The docking system: every window is a tree of splits whose leaves
//! are tabbed panes, the way MaxPane tiles REAPER — the same logic,
//! drawn by Dioxus instead of reparented HWNDs.
//!
//! Split along the line the file always described in its own opening
//! paragraph: the model, then the component.
//!
//! * [`pane`] — what a leaf can host.
//! * [`tree`] — [`DockNode`] and its operations: split, tab, remove,
//!   move, ratio, solo. Plain data, no window in sight.
//! * [`preset`] — the six shipped layouts, Console among them.
//! * [`geometry`] — the five-zone drop hit test and the layout
//!   arithmetic that says where every leaf and splitter lands in a
//!   rectangle. Pure maths: it imports no Dioxus at all, which is why
//!   every drop zone and splitter position is covered by ordinary unit
//!   tests rather than by driving a window.
//! * [`view`] — the [`view::Dock`] component that renders a tree, and
//!   the drag, splitter and context-menu machinery on top of it.
//!
//! Blitz (Dioxus Native) has no pointer capture, so a drag is a
//! `mousedown` on the tab and `mousemove` / `mouseup` on the dock root,
//! which everything under the pointer bubbles to. Where a pane sits is
//! not read back from the DOM: [`geometry::layout`] computes the same
//! rectangles the CSS flexbox produces (`flex-grow` from the ratios, a
//! fixed splitter), so the drop-zone overlay and the splitter drag work
//! from arithmetic the tests cover.

// r[impl studio.dock] - windows are trees of splits with tabbed leaves
// r[impl studio.dock.tabs-are-handles] - the tab is the drag handle and the right-click target
// r[impl studio.dock.drop-zones] - five zones, MaxPane's
// r[impl studio.dock.presets] - the six presets, Console among them
// r[impl studio.dock.no-scroll] - panes fill their rectangle; nothing scrolls the window

pub mod geometry;
pub mod pane;
pub mod preset;
pub mod tree;
pub mod view;

// Flat re-export. `crate::dock::DockNode` is what the studio, the
// layout file and the window code have always written, and the split is
// an internal matter — no call site should have to know which of the
// five modules a name landed in.
// Only what the rest of the studio actually reaches for. The modules
// keep everything else public to each other and to their own tests, so
// a name missing from this list is a name nothing outside `dock` needs
// — not a name that is private.
pub use geometry::{DropZone, Rect, TAB_BAR, fraction_for, hit_test, layout, zone_rect};
pub use pane::PaneKind;
pub use preset::Preset;
pub use tree::{Axis, DockNode, DockState, Path};
pub use view::Dock;

/// Fixtures shared by the modules' own test suites.
///
/// `console()` is the tree the desk actually opens on, so it is the
/// honest starting point for a test about splitting, tabbing or
/// dropping — and it is wanted by `tree`, `preset` and `geometry`
/// alike, which is why it sits here rather than in any one of them.
#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    pub fn console() -> DockNode {
        Preset::Console.build(&[])
    }
}
