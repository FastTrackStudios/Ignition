//! Where everything lands, as arithmetic.
//!
//! Pane rectangles are not read back from the DOM. [`layout`] computes
//! the same boxes the CSS flexbox produces — `flex-grow` from the
//! ratios, a fixed splitter — so the drop-zone overlay and the splitter
//! drag work from numbers rather than from a measured document.
//!
//! Nothing here imports Dioxus. That is the point of the module: the
//! five drop zones, the edge bands and every splitter position are
//! covered by ordinary unit tests, with no window to drive and no
//! renderer to wait for. `the_stylesheet_agrees_with_the_geometry` is
//! what keeps the CSS honest about the constants.

use super::pane::PaneKind;
use super::tree::{Axis, DockNode, Path};

/// The tab-bar height in CSS pixels, and the splitter thickness. Both
/// are repeated in `dock.css`; `the_stylesheet_agrees_with_the_geometry`
/// checks the sheet agrees. The chrome is hairline-thin on purpose: the
/// content gets the pixels.
pub const TAB_BAR: f32 = 18.0;
pub const SPLITTER: f32 = 2.0;

/// How far either side of a splitter a press still grabs it. Six
/// pixels of target in all, which is enough for a mouse and narrow
/// enough that a press near the edge of a pane lands in the pane.
pub const SPLITTER_GRAB: f32 = 2.0;

/// A rectangle in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// Where a drop lands on a leaf — `MaxPane`'s five zones, with the tab
/// bar carrying the insertion index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    Left,
    Right,
    Top,
    Bottom,
    /// Into the leaf as a new tab, at the end.
    Centre,
    /// Into the leaf as a new tab, at this index.
    TabBar(usize),
}

impl DropZone {
    /// The axis a split for this edge runs on.
    pub const fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Row,
            _ => Axis::Col,
        }
    }
    /// Whether the new leaf comes after the old one on that axis.
    pub const fn after(self) -> bool {
        matches!(self, Self::Right | Self::Bottom)
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Left => "split left",
            Self::Right => "split right",
            Self::Top => "split above",
            Self::Bottom => "split below",
            Self::Centre => "add as tab",
            Self::TabBar(_) => "insert tab",
        }
    }
}

/// How wide an edge band is for a body of `dim`: the outer quarter,
/// no less than 24px, and never so wide that two bands meet.
pub fn edge_band(dim: f32) -> f32 {
    let mut band = dim * 0.25;
    if band < 24.0 {
        band = 24.0;
    }
    if band * 2.0 > dim / 2.0 {
        band = dim / 4.0;
    }
    band
}

/// The drop zone for a pointer at (`x`, `y`) over a leaf occupying
/// `rect`. `tab_edges` are the x coordinates of the right edge of each
/// tab, left to right, for the insertion index; empty means "unknown,
/// append". `None` when the pointer is outside the leaf.
pub fn hit_test(rect: Rect, tab_edges: &[f32], x: f32, y: f32) -> Option<DropZone> {
    if !rect.contains(x, y) {
        return None;
    }
    if y < rect.y + TAB_BAR {
        let index = tab_edges
            .iter()
            .position(|edge| x < *edge)
            .unwrap_or(tab_edges.len());
        return Some(DropZone::TabBar(index));
    }
    let body_top = rect.y + TAB_BAR;
    let w = rect.w;
    let h = rect.h - TAB_BAR;
    if h <= 0.0 {
        return Some(DropZone::TabBar(tab_edges.len()));
    }
    let rel_x = x - rect.x;
    let rel_y = y - body_top;
    let band_h = edge_band(w);
    let band_v = edge_band(h);
    let dist = [
        (DropZone::Left, rel_x, band_h),
        (DropZone::Right, w - rel_x - 1.0, band_h),
        (DropZone::Top, rel_y, band_v),
        (DropZone::Bottom, h - rel_y - 1.0, band_v),
    ];
    // In a corner the closer edge wins.
    dist.iter()
        .filter(|(_, d, band)| d < band)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(zone, _, _)| *zone)
        .or(Some(DropZone::Centre))
}

/// The rectangle the drop preview frames for a zone over a leaf.
pub fn zone_rect(rect: Rect, zone: DropZone) -> Rect {
    let body = Rect::new(
        rect.x,
        rect.y + TAB_BAR,
        rect.w,
        (rect.h - TAB_BAR).max(0.0),
    );
    match zone {
        DropZone::Left => Rect::new(body.x, body.y, body.w / 2.0, body.h),
        DropZone::Right => Rect::new(body.x + body.w / 2.0, body.y, body.w / 2.0, body.h),
        DropZone::Top => Rect::new(body.x, body.y, body.w, body.h / 2.0),
        DropZone::Bottom => Rect::new(body.x, body.y + body.h / 2.0, body.w, body.h / 2.0),
        DropZone::Centre => body,
        DropZone::TabBar(_) => Rect::new(rect.x, rect.y, rect.w, TAB_BAR),
    }
}

/// A leaf as laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLeaf {
    pub path: Path,
    pub panes: Vec<PaneKind>,
    pub active: usize,
    pub rect: Rect,
}

/// A splitter as laid out: between children `index` and `index + 1`
/// of the split at `path`; `pair` is the rectangle the two children
/// and the splitter share, for turning a pointer into a fraction.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSplitter {
    pub path: Path,
    pub index: usize,
    pub axis: Axis,
    pub rect: Rect,
    pub pair: Rect,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Placed {
    pub leaves: Vec<PlacedLeaf>,
    pub splitters: Vec<PlacedSplitter>,
}

impl Placed {
    pub fn leaf_at(&self, x: f32, y: f32) -> Option<&PlacedLeaf> {
        self.leaves.iter().find(|l| l.rect.contains(x, y))
    }
    pub fn leaf(&self, path: &[usize]) -> Option<&PlacedLeaf> {
        self.leaves.iter().find(|l| l.path == path)
    }
    pub fn splitter(&self, path: &[usize], index: usize) -> Option<&PlacedSplitter> {
        self.splitters
            .iter()
            .find(|s| s.path == path && s.index == index)
    }

    /// The splitter under a press, the bar widened by `SPLITTER_GRAB`
    /// on both sides so a two-pixel line is an eight-pixel handle.
    pub fn splitter_at(&self, x: f32, y: f32) -> Option<&PlacedSplitter> {
        self.splitters.iter().find(|s| {
            let r = s.rect;
            let grown = match s.axis {
                Axis::Row => Rect::new(
                    r.x - SPLITTER_GRAB,
                    r.y,
                    2.0f32.mul_add(SPLITTER_GRAB, r.w),
                    r.h,
                ),
                Axis::Col => Rect::new(
                    r.x,
                    r.y - SPLITTER_GRAB,
                    r.w,
                    2.0f32.mul_add(SPLITTER_GRAB, r.h),
                ),
            };
            grown.contains(x, y)
        })
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn rect_of(&self, pane: PaneKind) -> Option<Rect> {
        self.leaves
            .iter()
            .find(|l| l.panes.contains(&pane))
            .map(|l| l.rect)
    }
}

/// Where every leaf and splitter lands when `tree` fills `rect` — the
/// same arithmetic the flexbox does: along a split's axis the space
/// left after the splitters is shared by ratio.
pub fn layout(tree: &DockNode, rect: Rect) -> Placed {
    let mut placed = Placed::default();
    place(tree, Vec::new(), rect, &mut placed);
    placed
}

fn place(node: &DockNode, path: Path, rect: Rect, out: &mut Placed) {
    match node {
        DockNode::Tabs { panes, active } => out.leaves.push(PlacedLeaf {
            path,
            panes: panes.clone(),
            active: *active,
            rect,
        }),
        DockNode::Split {
            axis,
            ratios,
            children,
        } => {
            let n = children.len();
            if n == 0 {
                return;
            }
            let sum: f32 = ratios.iter().take(n).sum();
            let ratios: Vec<f32> = if ratios.len() == n && sum > f32::EPSILON {
                ratios.iter().map(|r| r / sum).collect()
            } else {
                vec![1.0 / crate::num::f32_of_usize(n); n]
            };
            let total = match axis {
                Axis::Row => rect.w,
                Axis::Col => rect.h,
            };
            let avail = SPLITTER
                .mul_add(-(crate::num::f32_of_usize(n) - 1.0), total)
                .max(0.0);
            let mut cursor = match axis {
                Axis::Row => rect.x,
                Axis::Col => rect.y,
            };
            let mut child_rects = Vec::with_capacity(n);
            for ratio in &ratios {
                let size = avail * ratio;
                let r = match axis {
                    Axis::Row => Rect::new(cursor, rect.y, size, rect.h),
                    Axis::Col => Rect::new(rect.x, cursor, rect.w, size),
                };
                child_rects.push(r);
                cursor += size + SPLITTER;
            }
            for (i, (child, r)) in children.iter().zip(&child_rects).enumerate() {
                let mut p = path.clone();
                p.push(i);
                place(child, p, *r, out);
                let next_index = i.saturating_add(1);
                if next_index < n
                    && let Some(&next) = child_rects.get(next_index)
                {
                    let (srect, pair) = match axis {
                        Axis::Row => (
                            Rect::new(r.right(), rect.y, SPLITTER, rect.h),
                            Rect::new(r.x, rect.y, next.right() - r.x, rect.h),
                        ),
                        Axis::Col => (
                            Rect::new(rect.x, r.bottom(), rect.w, SPLITTER),
                            Rect::new(rect.x, r.y, rect.w, next.bottom() - r.y),
                        ),
                    };
                    out.splitters.push(PlacedSplitter {
                        path: path.clone(),
                        index: i,
                        axis: *axis,
                        rect: srect,
                        pair,
                    });
                }
            }
        }
    }
}

/// The fraction of a splitter's pair the first child should take for
/// the pointer to sit on the splitter.
pub fn fraction_for(splitter: &PlacedSplitter, x: f32, y: f32) -> f32 {
    let (pos, start, len) = match splitter.axis {
        Axis::Row => (x, splitter.pair.x, splitter.pair.w),
        Axis::Col => (y, splitter.pair.y, splitter.pair.h),
    };
    let usable = (len - SPLITTER).max(1.0);
    ((pos - start - SPLITTER / 2.0) / usable).clamp(0.05, 0.95)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::PaneKind::*;
    use crate::dock::fixtures::console;
    use crate::dock::preset::{CONSOLE_FADERS_BAND, CONSOLE_TOP};
    use crate::dock::{Preset, view};

    /// r[verify studio.dock.drop-zones]
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "zone_rect and edge_band compute exact pixel geometry from exact literal                   inputs with no accumulated rounding; exact equality is the property under                   test"
    )]
    fn the_five_zones_are_the_tab_bar_the_edges_and_the_centre() {
        let r = Rect::new(100.0, 50.0, 400.0, 300.0 + TAB_BAR);
        let body_top = 50.0 + TAB_BAR;
        assert_eq!(hit_test(r, &[], 50.0, 60.0), None);
        // The tab bar, with the index from the tab edges.
        assert_eq!(
            hit_test(r, &[160.0, 220.0], 130.0, 60.0),
            Some(DropZone::TabBar(0))
        );
        assert_eq!(
            hit_test(r, &[160.0, 220.0], 200.0, 60.0),
            Some(DropZone::TabBar(1))
        );
        assert_eq!(
            hit_test(r, &[160.0, 220.0], 400.0, 60.0),
            Some(DropZone::TabBar(2))
        );
        assert_eq!(
            hit_test(r, &[], 400.0, body_top - 0.1),
            Some(DropZone::TabBar(0))
        );
        // The body is 400 × 300 under the tab bar; bands are 100 wide,
        // 75 tall.
        assert_eq!(
            hit_test(r, &[], 110.0, body_top + 128.0),
            Some(DropZone::Left)
        );
        assert_eq!(
            hit_test(r, &[], 490.0, body_top + 128.0),
            Some(DropZone::Right)
        );
        assert_eq!(hit_test(r, &[], 300.0, body_top + 8.0), Some(DropZone::Top));
        assert_eq!(
            hit_test(r, &[], 300.0, body_top + 288.0),
            Some(DropZone::Bottom)
        );
        assert_eq!(
            hit_test(r, &[], 300.0, body_top + 128.0),
            Some(DropZone::Centre)
        );
        // Just inside the band on each side.
        assert_eq!(
            hit_test(r, &[], 199.0, body_top + 128.0),
            Some(DropZone::Left)
        );
        assert_eq!(
            hit_test(r, &[], 200.0, body_top + 128.0),
            Some(DropZone::Centre)
        );
        // In a corner the nearer edge wins.
        assert_eq!(
            hit_test(r, &[], 105.0, body_top + 58.0),
            Some(DropZone::Left)
        );
        assert_eq!(hit_test(r, &[], 150.0, body_top + 3.0), Some(DropZone::Top));
        // The preview frames half the body for an edge, all of it for
        // the centre.
        assert_eq!(
            zone_rect(r, DropZone::Right),
            Rect::new(300.0, body_top, 200.0, 300.0)
        );
        assert_eq!(
            zone_rect(r, DropZone::Centre),
            Rect::new(100.0, body_top, 400.0, 300.0)
        );
        assert_eq!(zone_rect(r, DropZone::TabBar(0)).h, TAB_BAR);
    }

    /// r[verify studio.dock.drop-zones]
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "zone_rect and edge_band compute exact pixel geometry from exact literal                   inputs with no accumulated rounding; exact equality is the property under                   test"
    )]
    fn edge_bands_never_meet() {
        assert_eq!(edge_band(400.0), 100.0);
        assert_eq!(edge_band(60.0), 15.0);
        assert_eq!(edge_band(200.0), 50.0);
    }

    /// r[verify studio.dock.no-scroll]
    #[test]
    #[expect(
        clippy::float_cmp,
        reason = "faders.w is the window's exact literal width with no fraction taken of                   it (the fader band is full width by design); exact equality is the                   property under test"
    )]
    fn console_fits_a_1440p_monitor() {
        // Each pane holds a whole number of columns of its own shape,
        // and they are not the same number: three look cards, one
        // column of group names, three colour discs, one of focus
        // names, four effect cards. `(columns, tile width)`, in
        // `CONSOLE_TOP` order — kept in step with the column counts
        // pinned in `live.css`. Declared first: clippy's `nursery`
        // group wants items ahead of the statements in their scope.
        const SHAPES: [(f32, f32); 7] = [
            (3.0, 240.0), // Looks — three cards, the pane most used
            (1.0, 150.0), // Groups — a name tile
            (3.0, 84.0),  // Colours — discs
            (1.0, 150.0), // Focus — a name tile
            (2.0, 180.0), // Effects — two cards, everything but movement
            (1.0, 200.0), // Movers — one card, wide enough for 16:9
            (1.0, 200.0), // Macros — the same
        ];
        // The DP-3 window: 2560 × 1440 fullscreen, the mode strip off
        // the top.
        let window = Rect::new(0.0, 0.0, 2560.0, 1440.0 - view::STRIP);
        let placed = layout(&console(), window);
        assert_eq!(placed.leaves.len(), CONSOLE_TOP.len() + 1);
        let faders = placed.rect_of(Faders).unwrap();
        // The fader pane's tallest column: tab 22 + head 44 + gaps +
        // badges 18 + track 120 + param 42 + label/value 26 + key 44
        // ≈ 340px. The band has to hold it without a scrollbar.
        assert!(faders.h >= 340.0, "fader band is {}px", faders.h);
        assert!((faders.bottom() - window.bottom()).abs() < 0.01);
        assert_eq!(faders.w, 2560.0);
        for (pane, (columns, tile)) in CONSOLE_TOP.into_iter().zip(SHAPES) {
            let r = placed.rect_of(pane).unwrap();
            let want = columns.mul_add(tile, (columns - 1.0) * 6.0) + 16.0;
            assert!(
                r.w >= want,
                "{pane:?} is {}px wide, too narrow for {columns} columns of {tile}px",
                r.w
            );
            assert!(r.h >= 900.0, "{pane:?} is {}px tall", r.h);
        }
        // Nothing hangs outside the window.
        for leaf in &placed.leaves {
            assert!(leaf.rect.right() <= window.right() + 0.01);
            assert!(leaf.rect.bottom() <= window.bottom() + 0.01);
        }
        // Splitters: one between each pair across the top row, and one
        // under the row.
        assert_eq!(placed.splitters.len(), CONSOLE_TOP.len());
        let under = placed.splitter(&[], 0).unwrap();
        assert_eq!(under.axis, Axis::Col);
        assert_eq!(under.pair, window);
        assert!(
            (fraction_for(under, 0.0, under.rect.y + SPLITTER / 2.0) - (1.0 - CONSOLE_FADERS_BAND))
                .abs()
                < 0.01
        );
    }
    /// r[verify studio.dock.no-scroll]
    #[test]
    fn the_stylesheet_agrees_with_the_geometry() {
        // The sheet itself, not a const: `dock.css` is compiled into
        // the one stylesheet now, so nothing else has reason to hold it.
        let css = include_str!("../dock.css");
        assert!(
            css.contains(&format!("height: {}px", crate::num::u32_of_f32(TAB_BAR))),
            "tab bar height"
        );
        assert!(
            css.contains(&format!("flex: 0 0 {}px", crate::num::u32_of_f32(SPLITTER))),
            "splitter size"
        );
        assert!(
            css.contains("overflow: hidden"),
            "panes clip, the window never scrolls"
        );
        // A nested split — a row of panes inside a column — gets its
        // main size from `flex-grow` but its cross size only from
        // stretching, and a stretched-but-indefinite size is `auto` to
        // Taffy. The leaf's `height: 100%` then resolves against
        // nothing and collapses to its tab bar, which is how five panes
        // came to draw as five tab bars over empty grey.
        assert!(
            css.contains(".dock-split.row > .dock-child { height: 100%; }"),
            "a row split's children need a definite height or nested leaves collapse"
        );
        assert!(
            css.contains(".dock-split.col > .dock-child { width: 100%; }"),
            "a column split's children need a definite width for the same reason"
        );
    }

    #[test]
    fn leaves_and_layout_agree_with_the_tree() {
        let t = Preset::LeftRightSplit.build(&[Visualizer, Transport, CueList]);
        let placed = layout(&t, Rect::new(0.0, 0.0, 1002.0, 502.0));
        let left = placed.leaf(&[0]).unwrap();
        assert_eq!(left.rect, Rect::new(0.0, 0.0, 500.0, 502.0));
        let top_right = placed.leaf(&[1, 0]).unwrap();
        assert_eq!(top_right.rect, Rect::new(502.0, 0.0, 500.0, 250.0));
        let bottom_right = placed.leaf(&[1, 1]).unwrap();
        assert_eq!(bottom_right.rect, Rect::new(502.0, 252.0, 500.0, 250.0));
        assert_eq!(
            placed.leaf_at(700.0, 300.0).map(|l| &l.path),
            Some(&vec![1, 1])
        );
        let s = placed.splitter(&[1], 0).unwrap();
        assert_eq!(s.rect, Rect::new(502.0, 250.0, 500.0, 2.0));
        assert!((fraction_for(s, 0.0, 100.0) - 0.2).abs() < 0.01);
        // The two-pixel bar is grabbed from two pixels either side —
        // six pixels of target in all. Wider than that and a press
        // meant for the edge of a pane grabs the splitter instead.
        assert_eq!(placed.splitter_at(700.0, 249.0).map(|s| s.index), Some(0));
        assert_eq!(placed.splitter_at(700.0, 253.0).map(|s| s.index), Some(0));
        assert!(placed.splitter_at(700.0, 247.0).is_none());
        assert!(placed.splitter_at(700.0, 255.0).is_none());
        assert_eq!(
            placed.splitter_at(499.0, 300.0).map(|s| &s.path),
            Some(&vec![])
        );
    }
}
