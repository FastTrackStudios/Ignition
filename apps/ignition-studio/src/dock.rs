//! The docking system: every window is a tree of splits whose leaves
//! are tabbed panes, the way MaxPane tiles REAPER — the same logic,
//! drawn by Dioxus instead of reparented HWNDs.
//!
//! The first half of this file is the model: [`DockNode`], its
//! operations (split, tab, remove, move, ratio, presets), the
//! five-zone drop hit test and the layout arithmetic that says where
//! every leaf and splitter lands in a rectangle. All of it is plain
//! data with no window in sight, so all of it is tested. The second
//! half is the [`Dock`] component that renders a tree, and the drag,
//! splitter and context-menu machinery on top of it.
//!
//! Blitz (Dioxus Native) has no pointer capture, so a drag is a
//! `mousedown` on the tab and `mousemove` / `mouseup` on the dock root,
//! which everything under the pointer bubbles to. Where a pane sits is
//! not read back from the DOM: [`layout`] computes the same rectangles
//! the CSS flexbox produces (`flex-grow` from the ratios, a fixed
//! splitter), so the drop-zone overlay and the splitter drag work from
//! arithmetic the tests cover.

// r[impl studio.dock] - windows are trees of splits with tabbed leaves
// r[impl studio.dock.tabs-are-handles] - the tab is the drag handle and the right-click target
// r[impl studio.dock.drop-zones] - five zones, MaxPane's
// r[impl studio.dock.presets] - the six presets, Console among them
// r[impl studio.dock.no-scroll] - panes fill their rectangle; nothing scrolls the window

use serde::{Deserialize, Serialize};

// ── Panes ────────────────────────────────────────────────────────────

/// A pane: one implementation, hostable by any leaf of any window.
/// The names are the ones the layout file uses (`cue_list`, ...), and
/// double as CSS classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    CueList,
    Visualizer,
    Transport,
    Faders,
    Looks,
    Macros,
    Groups,
    Colours,
    Splits,
    Focus,
    Effects,
    /// Movement effects only. They read differently from the rest —
    /// what matters is where the beams go, not what the rig does — and
    /// a column of them beside the others is easier to scan than a
    /// hundred and thirty-one of everything mixed together.
    Movers,
    Tricks,
    Bundles,
    Programmer,
    Library,
    Desk,
    Output,
    CommandLine,
    Canvases,
    Cameras,
    Lyrics,
    /// The programme camera — the cut — beside the wide Visualizer.
    // r[impl viz.programme-view] - a pane of its own
    Programme,
}

impl PaneKind {
    pub const ALL: [PaneKind; 23] = [
        PaneKind::CueList,
        PaneKind::Visualizer,
        PaneKind::Transport,
        PaneKind::Faders,
        PaneKind::Looks,
        PaneKind::Macros,
        PaneKind::Groups,
        PaneKind::Colours,
        PaneKind::Splits,
        PaneKind::Focus,
        PaneKind::Effects,
        PaneKind::Movers,
        PaneKind::Tricks,
        PaneKind::Bundles,
        PaneKind::Programmer,
        PaneKind::Library,
        PaneKind::Desk,
        PaneKind::Output,
        PaneKind::CommandLine,
        PaneKind::Canvases,
        PaneKind::Cameras,
        PaneKind::Lyrics,
        PaneKind::Programme,
    ];

    /// The name on the tab.
    pub fn label(self) -> &'static str {
        match self {
            PaneKind::CueList => "Cue List",
            PaneKind::Visualizer => "Visualizer",
            PaneKind::Transport => "Transport",
            PaneKind::Faders => "Faders",
            PaneKind::Looks => "Looks",
            PaneKind::Macros => "Macros",
            PaneKind::Groups => "Groups",
            PaneKind::Colours => "Colours",
            PaneKind::Splits => "Splits",
            PaneKind::Focus => "Focus",
            PaneKind::Effects => "Effects",
            PaneKind::Movers => "Movers",
            PaneKind::Tricks => "Tricks",
            PaneKind::Bundles => "Bundles",
            PaneKind::Programmer => "Programmer",
            PaneKind::Library => "Library",
            PaneKind::Desk => "Desk",
            PaneKind::Output => "Output",
            PaneKind::CommandLine => "Command Line",
            PaneKind::Canvases => "Canvases",
            PaneKind::Cameras => "Cameras",
            PaneKind::Lyrics => "Lyrics",
            PaneKind::Programme => "Programme",
        }
    }

    /// The layout-file spelling, which doubles as a CSS class.
    pub fn key(self) -> &'static str {
        match self {
            PaneKind::CueList => "cue_list",
            PaneKind::Visualizer => "visualizer",
            PaneKind::Transport => "transport",
            PaneKind::Faders => "faders",
            PaneKind::Looks => "looks",
            PaneKind::Macros => "macros",
            PaneKind::Groups => "groups",
            PaneKind::Colours => "colours",
            PaneKind::Splits => "splits",
            PaneKind::Focus => "focus",
            PaneKind::Effects => "effects",
            PaneKind::Movers => "movers",
            PaneKind::Tricks => "tricks",
            PaneKind::Bundles => "bundles",
            PaneKind::Programmer => "programmer",
            PaneKind::Library => "library",
            PaneKind::Desk => "desk",
            PaneKind::Output => "output",
            PaneKind::CommandLine => "command_line",
            PaneKind::Canvases => "canvases",
            PaneKind::Cameras => "cameras",
            PaneKind::Lyrics => "lyrics",
            PaneKind::Programme => "programme",
        }
    }
}

// ── The tree ─────────────────────────────────────────────────────────

/// Which way a split lays its children: `Row` side by side, `Col`
/// stacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    Row,
    Col,
}

/// A position in the tree: the child index at each split from the
/// root down. The root is the empty path.
pub type Path = Vec<usize>;

/// A window's content. A `Split` lays its children along an axis with
/// one ratio each (they are normalised to sum to one whenever they are
/// used); a `Tabs` is a leaf holding one or more panes with one shown.
///
/// Serialised externally tagged, so a layout file reads
/// `{"split": {"axis": "col", "ratios": [0.8, 0.2], "children": [...]}}`
/// and `{"tabs": {"panes": ["cue_list"], "active": 0}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockNode {
    Split {
        axis: Axis,
        #[serde(default)]
        ratios: Vec<f32>,
        children: Vec<DockNode>,
    },
    Tabs {
        panes: Vec<PaneKind>,
        #[serde(default)]
        active: usize,
    },
}

/// The tab-bar height in CSS pixels, and the splitter thickness. Both
/// are repeated in `dock.css`; a test checks the stylesheet agrees.
/// The chrome is hairline-thin on purpose: the content gets the pixels.
pub const TAB_BAR: f32 = 18.0;
pub const SPLITTER: f32 = 2.0;
/// How far either side of a splitter a press still grabs it. Six
/// pixels of target in all, which is enough for a mouse and narrow
/// enough that a press near the edge of a pane lands in the pane.
pub const SPLITTER_GRAB: f32 = 2.0;

impl DockNode {
    pub fn tabs(panes: impl Into<Vec<PaneKind>>) -> Self {
        DockNode::Tabs {
            panes: panes.into(),
            active: 0,
        }
    }

    pub fn tab(pane: PaneKind) -> Self {
        Self::tabs(vec![pane])
    }

    /// A split with equal ratios.
    pub fn split(axis: Axis, children: Vec<DockNode>) -> Self {
        let n = children.len().max(1);
        DockNode::Split {
            axis,
            ratios: vec![1.0 / n as f32; children.len()],
            children,
        }
    }

    /// A split with the ratios given (normalised on use).
    pub fn split_with(axis: Axis, ratios: Vec<f32>, children: Vec<DockNode>) -> Self {
        DockNode::Split {
            axis,
            ratios,
            children,
        }
    }

    pub fn empty() -> Self {
        Self::tabs(Vec::new())
    }

    pub fn is_empty(&self) -> bool {
        self.panes().is_empty()
    }

    /// Every pane in the tree, leaves left to right / top to bottom.
    pub fn panes(&self) -> Vec<PaneKind> {
        let mut out = Vec::new();
        self.collect_panes(&mut out);
        out
    }

    fn collect_panes(&self, out: &mut Vec<PaneKind>) {
        match self {
            DockNode::Tabs { panes, .. } => out.extend(panes.iter().copied()),
            DockNode::Split { children, .. } => {
                for c in children {
                    c.collect_panes(out);
                }
            }
        }
    }

    pub fn contains(&self, pane: PaneKind) -> bool {
        self.panes().contains(&pane)
    }

    /// Every leaf with its path.
    pub fn leaves(&self) -> Vec<(Path, &[PaneKind], usize)> {
        let mut out = Vec::new();
        self.collect_leaves(Vec::new(), &mut out);
        out
    }

    fn collect_leaves<'a>(&'a self, path: Path, out: &mut Vec<(Path, &'a [PaneKind], usize)>) {
        match self {
            DockNode::Tabs { panes, active } => out.push((path, panes, *active)),
            DockNode::Split { children, .. } => {
                for (i, c) in children.iter().enumerate() {
                    let mut p = path.clone();
                    p.push(i);
                    c.collect_leaves(p, out);
                }
            }
        }
    }

    pub fn at(&self, path: &[usize]) -> Option<&DockNode> {
        let mut node = self;
        for &i in path {
            let DockNode::Split { children, .. } = node else {
                return None;
            };
            node = children.get(i)?;
        }
        Some(node)
    }

    pub fn at_mut(&mut self, path: &[usize]) -> Option<&mut DockNode> {
        let mut node = self;
        for &i in path {
            let DockNode::Split { children, .. } = node else {
                return None;
            };
            node = children.get_mut(i)?;
        }
        Some(node)
    }

    /// Where a pane is: the path of its leaf and its index in it.
    pub fn find(&self, pane: PaneKind) -> Option<(Path, usize)> {
        self.leaves()
            .into_iter()
            .find_map(|(path, panes, _)| panes.iter().position(|p| *p == pane).map(|i| (path, i)))
    }

    /// The first leaf's path — where a pane with nowhere better to go
    /// is tabbed in.
    pub fn first_leaf(&self) -> Path {
        self.leaves()
            .first()
            .map(|(p, _, _)| p.clone())
            .unwrap_or_default()
    }

    /// Show `pane` in its leaf.
    pub fn activate(&mut self, pane: PaneKind) -> bool {
        let Some((path, index)) = self.find(pane) else {
            return false;
        };
        if let Some(DockNode::Tabs { active, .. }) = self.at_mut(&path) {
            *active = index;
        }
        true
    }

    /// Replace the node at `path` by a split of it and a new leaf
    /// holding `pane`, `after` deciding which side the new leaf takes.
    pub fn split_at(&mut self, path: &[usize], axis: Axis, pane: PaneKind, after: bool) -> bool {
        let Some(node) = self.at_mut(path) else {
            return false;
        };
        let old = std::mem::replace(node, DockNode::empty());
        let fresh = DockNode::tab(pane);
        let children = if after {
            vec![old, fresh]
        } else {
            vec![fresh, old]
        };
        *node = DockNode::split(axis, children);
        self.normalize();
        true
    }

    /// Add `pane` to the leaf at `path` at `index` (clamped) and show it.
    pub fn insert_tab(&mut self, path: &[usize], index: usize, pane: PaneKind) -> bool {
        match self.at_mut(path) {
            Some(DockNode::Tabs { panes, active }) => {
                let index = index.min(panes.len());
                panes.insert(index, pane);
                *active = index;
                true
            }
            _ => false,
        }
    }

    /// Take `pane` out, collapsing the leaf and any single-child split
    /// it leaves behind. False when it was not there.
    pub fn remove(&mut self, pane: PaneKind) -> bool {
        let Some((path, index)) = self.find(pane) else {
            return false;
        };
        if let Some(DockNode::Tabs { panes, active }) = self.at_mut(&path) {
            panes.remove(index);
            if *active >= panes.len() {
                *active = panes.len().saturating_sub(1);
            } else if *active > index {
                *active -= 1;
            }
        }
        self.normalize();
        true
    }

    /// Empty leaves go, single-child splits collapse into their child,
    /// same-axis nested splits flatten (their ratios scaled into the
    /// parent's), and ratios are kept summing to one. The root may end
    /// up an empty leaf; nothing else may.
    pub fn normalize(&mut self) {
        if let DockNode::Split {
            axis,
            ratios,
            children,
        } = self
        {
            let axis = *axis;
            if ratios.len() != children.len() {
                *ratios = vec![1.0 / children.len().max(1) as f32; children.len()];
            }
            let mut next_children = Vec::new();
            let mut next_ratios = Vec::new();
            for (mut child, ratio) in std::mem::take(children)
                .into_iter()
                .zip(std::mem::take(ratios))
            {
                child.normalize();
                match child {
                    DockNode::Tabs { ref panes, .. } if panes.is_empty() => {}
                    DockNode::Split {
                        axis: inner,
                        ratios: inner_ratios,
                        children: inner_children,
                    } if inner == axis => {
                        let sum: f32 = inner_ratios.iter().sum::<f32>().max(f32::EPSILON);
                        for (c, r) in inner_children.into_iter().zip(inner_ratios) {
                            next_children.push(c);
                            next_ratios.push(ratio * r / sum);
                        }
                    }
                    other => {
                        next_children.push(other);
                        next_ratios.push(ratio);
                    }
                }
            }
            let sum: f32 = next_ratios.iter().sum();
            if sum > f32::EPSILON {
                for r in &mut next_ratios {
                    *r /= sum;
                }
            } else {
                next_ratios = vec![1.0 / next_children.len().max(1) as f32; next_children.len()];
            }
            match next_children.len() {
                0 => *self = DockNode::empty(),
                1 => *self = next_children.pop().expect("one child"),
                _ => {
                    *children = next_children;
                    *ratios = next_ratios;
                }
            }
        }
    }

    /// The splitter between children `index` and `index + 1` of the
    /// split at `path`, moved so the first of the pair takes `fraction`
    /// of what the two share (clamped so neither vanishes).
    pub fn set_split(&mut self, path: &[usize], index: usize, fraction: f32) -> bool {
        let Some(DockNode::Split { ratios, .. }) = self.at_mut(path) else {
            return false;
        };
        if index + 1 >= ratios.len() {
            return false;
        }
        let pair = ratios[index] + ratios[index + 1];
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.05, 0.95)
        } else {
            0.5
        };
        ratios[index] = pair * fraction;
        ratios[index + 1] = pair * (1.0 - fraction);
        true
    }

    /// Double-click on a splitter: the pair back to half and half.
    pub fn reset_split(&mut self, path: &[usize], index: usize) -> bool {
        self.set_split(path, index, 0.5)
    }

    /// Move `pane` (wherever it is, in this tree or not yet in it) to
    /// the leaf at `target` according to the drop zone. Works when the
    /// pane already lives in the target leaf: an edge zone splits it
    /// off, a tab zone reorders it.
    pub fn drop_pane(&mut self, pane: PaneKind, target: &[usize], zone: DropZone) -> bool {
        let Some(DockNode::Tabs {
            panes: target_panes,
            active,
        }) = self.at(target)
        else {
            return false;
        };
        let same_leaf = target_panes.contains(&pane);
        if same_leaf {
            match zone {
                DropZone::Centre => return self.activate(pane),
                DropZone::TabBar(index) => {
                    let Some(DockNode::Tabs { panes, active }) = self.at_mut(target) else {
                        return false;
                    };
                    let from = panes.iter().position(|p| *p == pane).expect("in leaf");
                    panes.remove(from);
                    let index = index.min(panes.len() + 1);
                    let index = if index > from { index - 1 } else { index };
                    let index = index.min(panes.len());
                    panes.insert(index, pane);
                    *active = index;
                    return true;
                }
                _ => {
                    if target_panes.len() == 1 {
                        // Splitting a leaf off from itself changes nothing.
                        return true;
                    }
                    // The leaf keeps its other panes, so its path holds.
                    let target = target.to_vec();
                    self.remove_from_leaf(pane);
                    return self.split_at(&target, zone.axis(), pane, zone.after());
                }
            }
        }
        // A different leaf: anchor the target by a pane it keeps, so
        // the path can be found again once the source leaf has gone.
        let anchor = target_panes.get(*active).or(target_panes.first()).copied();
        self.remove(pane);
        let target = match anchor {
            Some(anchor) => match self.find(anchor) {
                Some((path, _)) => path,
                None => return false,
            },
            None => target.to_vec(),
        };
        match zone {
            DropZone::Centre => {
                let len = match self.at(&target) {
                    Some(DockNode::Tabs { panes, .. }) => panes.len(),
                    _ => 0,
                };
                self.insert_tab(&target, len, pane)
            }
            DropZone::TabBar(index) => self.insert_tab(&target, index, pane),
            edge => self.split_at(&target, edge.axis(), pane, edge.after()),
        }
    }

    /// Remove without normalising, for a same-leaf split-off where the
    /// leaf must keep its path.
    fn remove_from_leaf(&mut self, pane: PaneKind) {
        if let Some((path, index)) = self.find(pane)
            && let Some(DockNode::Tabs { panes, active }) = self.at_mut(&path)
        {
            panes.remove(index);
            if *active >= panes.len() {
                *active = panes.len().saturating_sub(1);
            } else if *active > index {
                *active -= 1;
            }
        }
    }

    /// Tab `pane` into the first leaf (or become a leaf of it), for a
    /// pane coming home from a closed window.
    pub fn adopt(&mut self, pane: PaneKind) {
        if self.contains(pane) {
            return;
        }
        if self.is_empty() {
            *self = DockNode::tab(pane);
            return;
        }
        let path = self.first_leaf();
        let len = match self.at(&path) {
            Some(DockNode::Tabs { panes, .. }) => panes.len(),
            _ => 0,
        };
        self.insert_tab(&path, len, pane);
    }
}

// ── Solo ─────────────────────────────────────────────────────────────

/// A window's dock: the tree, and — while one pane is soloed — the
/// whole tree it will go back to. Only `tree` is drawn; only
/// [`Dock::persisted`] is saved.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DockState {
    pub tree: DockNode,
    pub solo: Option<DockNode>,
}

impl Default for DockNode {
    fn default() -> Self {
        DockNode::empty()
    }
}

impl DockState {
    pub fn new(tree: DockNode) -> Self {
        Self { tree, solo: None }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn is_solo(&self) -> bool {
        self.solo.is_some()
    }

    /// Show only `pane`, remembering the tree.
    pub fn solo(&mut self, pane: PaneKind) -> bool {
        self.restore();
        if !self.tree.contains(pane) {
            return false;
        }
        let full = std::mem::replace(&mut self.tree, DockNode::tab(pane));
        self.solo = Some(full);
        true
    }

    /// Back to the whole tree.
    pub fn restore(&mut self) {
        if let Some(full) = self.solo.take() {
            self.tree = full;
        }
    }

    /// The tree a layout file should hold: the remembered one when
    /// soloed.
    pub fn persisted(&self) -> &DockNode {
        self.solo.as_ref().unwrap_or(&self.tree)
    }

    /// Any structural edit leaves solo first, so what is edited is the
    /// real tree.
    pub fn edit(&mut self, f: impl FnOnce(&mut DockNode)) {
        self.restore();
        f(&mut self.tree);
    }
}

// ── Presets ──────────────────────────────────────────────────────────

/// MaxPane's five layouts, and the console: faders along the bottom,
/// the busking panes across the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    TwoColumns,
    LeftRightSplit,
    ThreeColumns,
    Grid2x2,
    TopBottom,
    Console,
}

/// The share of a window's height the Console preset gives the fader
/// row. The fader pane is built to fit this band on a 1440-high
/// window — see `console_fits_a_1440p_monitor`.
pub const CONSOLE_FADERS_BAND: f32 = 0.27;

/// How wide each of the Console's top panes is, as a share of the row.
///
/// Not even shares: each pane holds a different shape and wants a whole
/// number of columns of it. Looks are three cards across, groups and
/// focus a single column of names, colours three discs, effects two
/// cards, movers and macros one card each. The widths follow from that
/// — and the slack goes to Looks, which is the pane most reached for.
/// The CSS pins the column counts so the two cannot drift apart.
pub const CONSOLE_TOP_RATIOS: [f32; 7] = [0.36, 0.07, 0.11, 0.07, 0.20, 0.095, 0.095];

/// The panes across the top of the Console preset, left to right.
pub const CONSOLE_TOP: [PaneKind; 7] = [
    PaneKind::Looks,
    PaneKind::Groups,
    PaneKind::Colours,
    PaneKind::Focus,
    PaneKind::Effects,
    PaneKind::Movers,
    PaneKind::Macros,
];

impl Preset {
    pub const ALL: [Preset; 6] = [
        Preset::TwoColumns,
        Preset::LeftRightSplit,
        Preset::ThreeColumns,
        Preset::Grid2x2,
        Preset::TopBottom,
        Preset::Console,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::TwoColumns => "Two columns",
            Preset::LeftRightSplit => "Left + right split",
            Preset::ThreeColumns => "Three columns",
            Preset::Grid2x2 => "2×2 grid",
            Preset::TopBottom => "Top + bottom",
            Preset::Console => "Console",
        }
    }

    /// The preset's shape with empty leaves.
    fn shape(self) -> DockNode {
        let e = DockNode::empty;
        match self {
            Preset::TwoColumns => DockNode::split(Axis::Row, vec![e(), e()]),
            Preset::LeftRightSplit => DockNode::split(
                Axis::Row,
                vec![e(), DockNode::split(Axis::Col, vec![e(), e()])],
            ),
            Preset::ThreeColumns => DockNode::split(Axis::Row, vec![e(), e(), e()]),
            Preset::Grid2x2 => DockNode::split(
                Axis::Col,
                vec![
                    DockNode::split(Axis::Row, vec![e(), e()]),
                    DockNode::split(Axis::Row, vec![e(), e()]),
                ],
            ),
            Preset::TopBottom => DockNode::split(Axis::Col, vec![e(), e()]),
            Preset::Console => DockNode::split_with(
                Axis::Col,
                vec![1.0 - CONSOLE_FADERS_BAND, CONSOLE_FADERS_BAND],
                vec![
                    DockNode::split_with(
                        Axis::Row,
                        CONSOLE_TOP_RATIOS.to_vec(),
                        CONSOLE_TOP.iter().map(|p| DockNode::tab(*p)).collect(),
                    ),
                    DockNode::tab(PaneKind::Faders),
                ],
            ),
        }
    }

    /// The preset laid out over `panes`: one pane per leaf in order,
    /// the rest tabbed into the last leaf, leaves nobody fills dropped.
    /// The Console preset names its own panes; any others of the
    /// window's are tabbed into its first top pane.
    pub fn build(self, panes: &[PaneKind]) -> DockNode {
        let mut tree = self.shape();
        let leaves: Vec<Path> = tree.leaves().into_iter().map(|(p, _, _)| p).collect();
        match self {
            Preset::Console => {
                let first = leaves.first().cloned().unwrap_or_default();
                for pane in panes {
                    if !tree.contains(*pane) {
                        let len = match tree.at(&first) {
                            Some(DockNode::Tabs { panes, .. }) => panes.len(),
                            _ => 0,
                        };
                        tree.insert_tab(&first, len, *pane);
                    }
                }
                if let Some(DockNode::Tabs { active, .. }) = tree.at_mut(&first) {
                    *active = 0;
                }
            }
            _ => {
                let mut seen = Vec::new();
                for (i, pane) in panes.iter().enumerate() {
                    if seen.contains(pane) {
                        continue;
                    }
                    seen.push(*pane);
                    let leaf = &leaves[i.min(leaves.len() - 1)];
                    let len = match tree.at(leaf) {
                        Some(DockNode::Tabs { panes, .. }) => panes.len(),
                        _ => 0,
                    };
                    tree.insert_tab(leaf, len, *pane);
                }
                for leaf in &leaves {
                    if let Some(DockNode::Tabs { active, .. }) = tree.at_mut(leaf) {
                        *active = 0;
                    }
                }
            }
        }
        tree.normalize();
        tree
    }
}

// ── Geometry ─────────────────────────────────────────────────────────

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

/// Where a drop lands on a leaf — MaxPane's five zones, with the tab
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
    pub fn axis(self) -> Axis {
        match self {
            DropZone::Left | DropZone::Right => Axis::Row,
            _ => Axis::Col,
        }
    }
    /// Whether the new leaf comes after the old one on that axis.
    pub fn after(self) -> bool {
        matches!(self, DropZone::Right | DropZone::Bottom)
    }
    pub fn label(self) -> &'static str {
        match self {
            DropZone::Left => "split left",
            DropZone::Right => "split right",
            DropZone::Top => "split above",
            DropZone::Bottom => "split below",
            DropZone::Centre => "add as tab",
            DropZone::TabBar(_) => "insert tab",
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
                Axis::Row => Rect::new(r.x - SPLITTER_GRAB, r.y, r.w + 2.0 * SPLITTER_GRAB, r.h),
                Axis::Col => Rect::new(r.x, r.y - SPLITTER_GRAB, r.w, r.h + 2.0 * SPLITTER_GRAB),
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
                vec![1.0 / n as f32; n]
            };
            let total = match axis {
                Axis::Row => rect.w,
                Axis::Col => rect.h,
            };
            let avail = (total - SPLITTER * (n as f32 - 1.0)).max(0.0);
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
                if i + 1 < n {
                    let next = child_rects[i + 1];
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

// ── The component ────────────────────────────────────────────────────

pub use view::Dock;

pub mod view {
    use super::*;
    use crate::windows::{self, HostId};
    use dioxus::prelude::*;

    pub const DOCK_CSS: &str = include_str!("dock.css");

    /// The height of the mode strip above the dock, in CSS pixels —
    /// what turns a window-relative pointer into a dock-relative one.
    /// `.mode-strip` in `windows.rs` fixes the same number.
    pub const STRIP: f32 = 28.0;

    /// A tab being dragged.
    #[derive(Debug, Clone, PartialEq)]
    struct Drag {
        pane: PaneKind,
        start: (f32, f32),
        at: (f32, f32),
        /// Past the dead zone: the ghost shows and a drop counts.
        active: bool,
        /// The tab bar the pointer is over, and the index there.
        hover_tab: Option<(Path, usize)>,
        target: Option<(Path, DropZone)>,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct SplitDrag {
        path: Path,
        index: usize,
    }

    #[derive(Debug, Clone, PartialEq)]
    enum MenuKind {
        Tab { pane: PaneKind, path: Path },
        Bar { path: Path },
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Menu {
        at: (f32, f32),
        kind: MenuKind,
    }

    /// The window's whole dock: the tree from the host, plus the drag,
    /// splitter and menu state that lives only in this window.
    #[component]
    pub fn Dock(host: HostId, tree: DockNode, solo: bool) -> Element {
        let window = dioxus_native::use_window();
        let mut drag = use_signal(|| Option::<Drag>::None);
        let mut splitting = use_signal(|| Option::<SplitDrag>::None);
        let mut menu = use_signal(|| Option::<Menu>::None);

        let size = window.surface_size();
        let scale = window.scale_factor() as f32;
        let dock_rect = Rect::new(
            0.0,
            0.0,
            size.width as f32 / scale,
            (size.height as f32 / scale - STRIP).max(0.0),
        );
        let placed = layout(&tree, dock_rect);
        let placed_for_move = placed.clone();
        let placed_for_down = placed.clone();

        // Every pointer move, from a tab, a bar or the root: the ghost
        // follows, the zone updates, a splitter drags.
        let mut on_move = move |x: f32, y: f32, hover: Option<(Path, usize)>| {
            let (x, y) = (x, y - STRIP);
            if let Some(s) = splitting() {
                if let Some(sp) = placed_for_move.splitter(&s.path, s.index) {
                    let fraction = fraction_for(sp, x, y);
                    windows::edit_tree(host, |t| {
                        t.set_split(&s.path, s.index, fraction);
                    });
                }
                return;
            }
            let Some(mut d) = drag() else {
                return;
            };
            d.at = (x, y);
            if !d.active {
                let dx = x - d.start.0;
                let dy = y - d.start.1;
                if dx * dx + dy * dy < 16.0 {
                    drag.set(Some(d));
                    return;
                }
                d.active = true;
            }
            if let Some(h) = hover {
                d.hover_tab = Some(h);
            }
            d.target = placed_for_move.leaf_at(x, y).and_then(|leaf| {
                let zone = if y < leaf.rect.y + TAB_BAR {
                    let index = match &d.hover_tab {
                        Some((p, i)) if *p == leaf.path => *i,
                        _ => leaf.panes.len(),
                    };
                    DropZone::TabBar(index)
                } else {
                    hit_test(leaf.rect, &[], x, y)?
                };
                Some((leaf.path.clone(), zone))
            });
            drag.set(Some(d));
        };
        let mut on_move_from_child = on_move.clone();

        let mut on_up = move || {
            splitting.set(None);
            let Some(d) = drag.take() else {
                return;
            };
            if !d.active {
                return;
            }
            match d.target {
                Some((path, zone)) => {
                    windows::edit_tree(host, |t| {
                        t.drop_pane(d.pane, &path, zone);
                    });
                }
                None => {
                    // Off every pane: the tab becomes a window.
                    if let Some(Some(new)) = windows::with_host(|h| h.pop_out(host, d.pane)) {
                        windows::open(new);
                    }
                }
            }
        };

        let ghost = drag().filter(|d| d.active);
        let preview = ghost.as_ref().and_then(|d| {
            let (path, zone) = d.target.as_ref()?;
            let leaf = placed.leaf(path)?;
            Some((zone_rect(leaf.rect, *zone), zone.label()))
        });
        let root_class = if ghost.is_some() {
            "dock dragging"
        } else if splitting().is_some() {
            "dock splitting"
        } else {
            "dock"
        };

        rsx! {
            div {
                class: "{root_class}",
                onmousemove: move |e| {
                    let p = e.data.client_coordinates();
                    on_move(p.x as f32, p.y as f32, None);
                },
                onmouseup: move |_| on_up(),
                onmouseleave: move |_| {
                    splitting.set(None);
                    drag.set(None);
                },
                onmousedown: move |e| {
                    menu.set(None);
                    // A press within the grab band of a splitter drags
                    // it, whatever element is under the pointer — the
                    // bar is two pixels and nobody can hit that.
                    if e.data.trigger_button() != Some(dioxus::html::input_data::MouseButton::Primary) {
                        return;
                    }
                    let p = e.data.client_coordinates();
                    if let Some(sp) = placed_for_down.splitter_at(p.x as f32, p.y as f32 - STRIP) {
                        splitting.set(Some(SplitDrag { path: sp.path.clone(), index: sp.index }));
                    }
                },
                Node {
                    host,
                    node: tree.clone(),
                    path: Vec::new(),
                    solo,
                    drag,
                    splitting,
                    menu,
                    on_move: EventHandler::new(move |(x, y, hover): (f32, f32, Option<(Path, usize)>)| on_move_from_child(x, y, hover)),
                }
                if let Some((rect, label)) = preview {
                    div {
                        class: "drop-preview",
                        style: "left: {rect.x}px; top: {rect.y}px; width: {rect.w}px; height: {rect.h}px;",
                        span { "{label}" }
                    }
                }
                if let Some(d) = ghost {
                    if d.target.is_none() {
                        div { class: "drop-detach", "release to open a new window" }
                    }
                    div {
                        class: "tab-ghost",
                        style: "left: {d.at.0 + 12.0}px; top: {d.at.1 + 8.0}px;",
                        "{d.pane.label()}"
                    }
                }
                if let Some(m) = menu() {
                    ContextMenu { host, menu_at: m.at, kind: m.kind, tree: tree.clone(), solo, close: move |_| menu.set(None) }
                }
            }
        }
    }

    #[component]
    fn Node(
        host: HostId,
        node: DockNode,
        path: Path,
        solo: bool,
        drag: Signal<Option<Drag>>,
        splitting: Signal<Option<SplitDrag>>,
        menu: Signal<Option<Menu>>,
        on_move: EventHandler<(f32, f32, Option<(Path, usize)>)>,
    ) -> Element {
        match node {
            DockNode::Split {
                axis,
                ratios,
                children,
            } => {
                let sum: f32 = ratios.iter().sum::<f32>().max(f32::EPSILON);
                let n = children.len();
                rsx! {
                    div { class: if axis == Axis::Row { "dock-split row" } else { "dock-split col" },
                        for (i, child) in children.into_iter().enumerate() {
                            {
                                let grow = ratios.get(i).copied().unwrap_or(1.0 / n as f32) / sum;
                                let mut child_path = path.clone();
                                child_path.push(i);
                                let reset_path = path.clone();
                                rsx! {
                                    div { key: "c{i}", class: "dock-child", style: "flex-grow: {grow};",
                                        Node { host, node: child, path: child_path, solo, drag, splitting, menu, on_move }
                                    }
                                    if i + 1 < n {
                                        div {
                                            key: "s{i}",
                                            class: "dock-splitter",
                                            ondoubleclick: move |_| {
                                                windows::edit_tree(host, |t| {
                                                    t.reset_split(&reset_path, i);
                                                });
                                            },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            DockNode::Tabs { panes, active } => {
                let bar_path = path.clone();
                let bar_menu_path = path.clone();
                let shown = panes.get(active).copied();
                let bar_hover_len = panes.len();
                rsx! {
                    div { class: "dock-leaf",
                        div {
                            class: "tab-bar",
                            onmousemove: move |e| {
                                if drag().is_some() {
                                    e.stop_propagation();
                                    let p = e.data.client_coordinates();
                                    on_move.call((p.x as f32, p.y as f32, Some((bar_path.clone(), bar_hover_len))));
                                }
                            },
                            oncontextmenu: move |e| {
                                e.stop_propagation();
                                let p = e.data.client_coordinates();
                                menu.set(Some(Menu {
                                    at: (p.x as f32, p.y as f32 - STRIP),
                                    kind: MenuKind::Bar { path: bar_menu_path.clone() },
                                }));
                            },
                            for (i, pane) in panes.iter().copied().enumerate() {
                                {
                                    let tab_path = path.clone();
                                    let move_path = path.clone();
                                    let menu_path = path.clone();
                                    let is_target = drag().and_then(|d| d.target).is_some_and(|(p, z)| p == path && z == DropZone::TabBar(i));
                                    let class = match (i == active, is_target) {
                                        (true, true) => "tab on insert-before",
                                        (true, false) => "tab on",
                                        (false, true) => "tab insert-before",
                                        (false, false) => "tab",
                                    };
                                    rsx! {
                                        div {
                                            key: "{pane.key()}",
                                            class: "{class}",
                                            onmousedown: move |e| {
                                                e.stop_propagation();
                                                menu.set(None);
                                                if e.data.trigger_button() != Some(dioxus::html::input_data::MouseButton::Primary) {
                                                    return;
                                                }
                                                let p = e.data.client_coordinates();
                                                windows::edit_tree(host, |t| {
                                                    if let Some(DockNode::Tabs { active, .. }) = t.at_mut(&tab_path) {
                                                        *active = i;
                                                    }
                                                });
                                                drag.set(Some(Drag {
                                                    pane,
                                                    start: (p.x as f32, p.y as f32 - STRIP),
                                                    at: (p.x as f32, p.y as f32 - STRIP),
                                                    active: false,
                                                    hover_tab: None,
                                                    target: None,
                                                }));
                                            },
                                            onmousemove: move |e| {
                                                if drag().is_some() {
                                                    e.stop_propagation();
                                                    let p = e.data.client_coordinates();
                                                    on_move.call((p.x as f32, p.y as f32, Some((move_path.clone(), i))));
                                                }
                                            },
                                            oncontextmenu: move |e| {
                                                e.stop_propagation();
                                                let p = e.data.client_coordinates();
                                                menu.set(Some(Menu {
                                                    at: (p.x as f32, p.y as f32 - STRIP),
                                                    kind: MenuKind::Tab { pane, path: menu_path.clone() },
                                                }));
                                            },
                                            "{pane.label()}"
                                        }
                                    }
                                }
                            }
                            if solo {
                                span { class: "tab-note", "SOLO" }
                            }
                        }
                        div { class: "tab-body",
                            if let Some(pane) = shown {
                                windows::PaneBody { pane }
                            }
                        }
                    }
                }
            }
        }
    }

    /// The right-click menu: on a tab, the pane's moves; on the bar's
    /// empty space, the presets and the panes that can be added.
    #[component]
    fn ContextMenu(
        host: HostId,
        menu_at: (f32, f32),
        kind: MenuKind,
        tree: DockNode,
        solo: bool,
        close: EventHandler<()>,
    ) -> Element {
        let window = dioxus_native::use_window();
        let others: Vec<(HostId, String)> = windows::read_host(|h| {
            h.windows
                .iter()
                .filter(|w| w.id != host)
                .map(|w| (w.id, w.spec.title()))
                .collect()
        })
        .unwrap_or_default();
        let here = tree.panes();
        let is_launch = host == 0;
        rsx! {
            div {
                class: "dock-menu",
                style: "left: {menu_at.0}px; top: {menu_at.1}px;",
                onmousedown: move |e| e.stop_propagation(),
                oncontextmenu: move |e| e.stop_propagation(),
                match kind {
                    MenuKind::Tab { pane, path } => {
                        let p1 = path.clone();
                        let p2 = path.clone();
                        rsx! {
                            div { class: "menu-title", "{pane.label()}" }
                            button { class: "menu-item",
                                onclick: move |_| {
                                    close.call(());
                                    if let Some(Some(new)) = windows::with_host(|h| h.pop_out(host, pane)) {
                                        windows::open(new);
                                    }
                                },
                                "Detach to new window"
                            }
                            if !others.is_empty() {
                                div { class: "menu-sub", "Move to" }
                                for (id, title) in others.iter().cloned() {
                                    button { key: "w{id}", class: "menu-item indent",
                                        onclick: move |_| {
                                            close.call(());
                                            windows::with_host(|h| h.move_pane(host, pane, id));
                                        },
                                        "{title}"
                                    }
                                }
                            }
                            button { class: "menu-item",
                                onclick: move |_| {
                                    close.call(());
                                    windows::edit_tree(host, |t| { t.drop_pane(pane, &p1, DropZone::Right); });
                                },
                                "Split right with this pane"
                            }
                            button { class: "menu-item",
                                onclick: move |_| {
                                    close.call(());
                                    windows::edit_tree(host, |t| { t.drop_pane(pane, &p2, DropZone::Bottom); });
                                },
                                "Split down with this pane"
                            }
                            button { class: "menu-item",
                                onclick: move |_| {
                                    close.call(());
                                    if solo {
                                        windows::with_host(|h| h.restore(host));
                                    } else {
                                        windows::with_host(|h| h.solo(host, pane));
                                    }
                                },
                                if solo { "Restore layout" } else { "Solo this pane" }
                            }
                            button { class: "menu-item",
                                disabled: is_launch && here.len() == 1,
                                onclick: {
                                    let window = window.clone();
                                    move |_| {
                                        close.call(());
                                        let empty = windows::with_host(|h| h.close_pane(host, pane)).unwrap_or(false);
                                        if empty && !is_launch {
                                            dioxus_native::close_window(window.id());
                                        }
                                    }
                                },
                                "Close"
                            }
                        }
                    }
                    MenuKind::Bar { path } => {
                        let addable: Vec<PaneKind> = PaneKind::ALL.iter().copied().filter(|p| !here.contains(p)).collect();
                        rsx! {
                            div { class: "menu-title", "Layout" }
                            for preset in Preset::ALL {
                                button { key: "{preset.label()}", class: "menu-item",
                                    onclick: move |_| {
                                        close.call(());
                                        windows::with_host(|h| h.apply_preset(host, preset));
                                    },
                                    "{preset.label()}"
                                }
                            }
                            if solo {
                                button { class: "menu-item",
                                    onclick: move |_| {
                                        close.call(());
                                        windows::with_host(|h| h.restore(host));
                                    },
                                    "Restore layout"
                                }
                            }
                            if !addable.is_empty() {
                                div { class: "menu-sub", "Add pane" }
                                div { class: "menu-grid",
                                    for pane in addable {
                                        {
                                            let path = path.clone();
                                            rsx! {
                                                button { key: "{pane.key()}", class: "menu-item indent",
                                                    onclick: move |_| {
                                                        close.call(());
                                                        windows::edit_tree(host, |t| {
                                                            let len = match t.at(&path) {
                                                                Some(DockNode::Tabs { panes, .. }) => panes.len(),
                                                                _ => 0,
                                                            };
                                                            if !t.insert_tab(&path, len, pane) {
                                                                t.adopt(pane);
                                                            }
                                                        });
                                                    },
                                                    "{pane.label()}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PaneKind::*;

    fn console() -> DockNode {
        Preset::Console.build(&[])
    }

    /// r[verify studio.dock]
    #[test]
    fn a_tree_round_trips_through_json() {
        let tree = DockNode::split_with(
            Axis::Col,
            vec![0.8, 0.2],
            vec![DockNode::tab(Visualizer), DockNode::tab(Transport)],
        );
        let json = serde_json::to_string(&tree).unwrap();
        assert!(
            json.contains(r#""split""#) && json.contains(r#""tabs""#),
            "{json}"
        );
        let back: DockNode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tree);
        // A leaf without `active` reads.
        let leaf: DockNode = serde_json::from_str(r#"{"tabs":{"panes":["cue_list"]}}"#).unwrap();
        assert_eq!(leaf, DockNode::tab(CueList));
    }

    /// r[verify studio.dock]
    #[test]
    fn split_tab_and_remove_collapse_back() {
        let mut t = DockNode::tab(CueList);
        assert!(t.split_at(&[], Axis::Row, Visualizer, true));
        assert_eq!(t.panes(), vec![CueList, Visualizer]);
        assert!(
            matches!(&t, DockNode::Split { axis: Axis::Row, children, .. } if children.len() == 2)
        );
        assert!(t.insert_tab(&[1], 0, Transport));
        assert_eq!(
            t.at(&[1]),
            Some(&DockNode::Tabs {
                panes: vec![Transport, Visualizer],
                active: 0
            })
        );
        assert!(t.remove(Transport));
        assert!(t.remove(Visualizer));
        // The split collapsed into its one remaining leaf.
        assert_eq!(t, DockNode::tab(CueList));
        assert!(!t.remove(Visualizer));
        assert!(t.remove(CueList));
        assert!(t.is_empty());
    }

    /// r[verify studio.dock]
    #[test]
    fn same_axis_splits_flatten_and_ratios_scale() {
        let mut t = DockNode::tab(Looks);
        t.split_at(&[], Axis::Row, Groups, true);
        // Split the right leaf again on the same axis: three columns.
        t.split_at(&[1], Axis::Row, Colours, true);
        let DockNode::Split {
            children, ratios, ..
        } = &t
        else {
            panic!()
        };
        assert_eq!(children.len(), 3);
        assert_eq!(t.panes(), vec![Looks, Groups, Colours]);
        assert!((ratios[0] - 0.5).abs() < 1e-5 && (ratios[1] - 0.25).abs() < 1e-5);
    }

    /// r[verify studio.dock]
    #[test]
    fn moving_a_pane_between_leaves_keeps_the_target_findable() {
        let mut t = console();
        // Faders into the Looks column as a tab: the bottom leaf goes,
        // the Col split collapses, the root becomes the Row split.
        let looks = t.find(Looks).unwrap().0;
        assert!(t.drop_pane(Faders, &looks, DropZone::Centre));
        assert!(matches!(
            &t,
            DockNode::Split {
                axis: Axis::Row,
                ..
            }
        ));
        assert_eq!(t.find(Faders), Some((vec![0], 1)));
        // And back out to the bottom as a split below the root leaf.
        let effects = t.find(Effects).unwrap().0;
        assert!(t.drop_pane(Faders, &effects, DropZone::Bottom));
        assert_eq!(t.find(Faders).unwrap().0, vec![4, 1]);
        assert_eq!(t.panes().len(), CONSOLE_TOP.len() + 1);
    }

    /// r[verify studio.dock.drop-zones]
    #[test]
    fn dropping_on_the_pane_it_lives_in_reorders_or_splits_off() {
        let mut t = DockNode::tabs(vec![Looks, Groups, Colours]);
        assert!(t.drop_pane(Colours, &[], DropZone::TabBar(0)));
        assert_eq!(
            t,
            DockNode::Tabs {
                panes: vec![Colours, Looks, Groups],
                active: 0
            }
        );
        assert!(t.drop_pane(Colours, &[], DropZone::TabBar(3)));
        assert_eq!(t.panes(), vec![Looks, Groups, Colours]);
        assert!(t.drop_pane(Groups, &[], DropZone::Right));
        assert_eq!(t.panes(), vec![Looks, Colours, Groups]);
        assert!(matches!(
            &t,
            DockNode::Split {
                axis: Axis::Row,
                ..
            }
        ));
        // A lone pane split off from itself is left alone.
        let mut lone = DockNode::tab(Faders);
        assert!(lone.drop_pane(Faders, &[], DropZone::Left));
        assert_eq!(lone, DockNode::tab(Faders));
    }

    /// r[verify studio.dock]
    #[test]
    fn splitters_move_within_their_pair_and_reset_to_half() {
        let mut t = DockNode::split(
            Axis::Row,
            vec![
                DockNode::tab(Looks),
                DockNode::tab(Groups),
                DockNode::tab(Colours),
            ],
        );
        assert!(t.set_split(&[], 0, 0.2));
        let DockNode::Split { ratios, .. } = &t else {
            panic!()
        };
        let third = 1.0 / 3.0;
        assert!((ratios[0] - 2.0 * third * 0.2).abs() < 1e-5);
        assert!((ratios[1] - 2.0 * third * 0.8).abs() < 1e-5);
        assert!((ratios[2] - third).abs() < 1e-5);
        assert!(t.set_split(&[], 0, 0.0));
        let DockNode::Split { ratios, .. } = &t else {
            panic!()
        };
        assert!(ratios[0] > 0.0, "a pane never vanishes");
        assert!(t.reset_split(&[], 0));
        let DockNode::Split { ratios, .. } = &t else {
            panic!()
        };
        assert!((ratios[0] - ratios[1]).abs() < 1e-5);
        assert!(!t.set_split(&[], 2, 0.5));
    }

    /// r[verify studio.dock]
    #[test]
    fn solo_shows_one_pane_and_restore_brings_the_tree_back() {
        let mut d = DockState::new(console());
        let full = d.tree.clone();
        assert!(d.solo(Faders));
        assert_eq!(d.tree, DockNode::tab(Faders));
        assert_eq!(d.persisted(), &full);
        // An edit while soloed lands on the real tree.
        d.edit(|t| {
            t.remove(Effects);
        });
        assert!(!d.is_solo());
        assert_eq!(d.tree.panes().len(), CONSOLE_TOP.len() + 1 - 1);
        assert!(!d.solo(Effects));
        d.solo(Looks);
        d.restore();
        d.restore();
        assert_eq!(d.tree.panes().len(), CONSOLE_TOP.len() + 1 - 1);
    }

    /// r[verify studio.dock.presets]
    #[test]
    fn presets_take_the_panes_in_order_and_drop_empty_leaves() {
        let two = Preset::TwoColumns.build(&[CueList, Visualizer, Transport]);
        assert_eq!(two.at(&[0]), Some(&DockNode::tab(CueList)));
        assert_eq!(
            two.at(&[1]),
            Some(&DockNode::Tabs {
                panes: vec![Visualizer, Transport],
                active: 0
            })
        );
        let three = Preset::ThreeColumns.build(&[CueList]);
        assert_eq!(
            three,
            DockNode::tab(CueList),
            "unfilled columns are dropped"
        );
        let grid = Preset::Grid2x2.build(&[Looks, Groups, Colours, Focus]);
        assert_eq!(grid.leaves().len(), 4);
        assert_eq!(grid.panes(), vec![Looks, Groups, Colours, Focus]);
        let lr = Preset::LeftRightSplit.build(&[Visualizer, Transport, CueList]);
        assert_eq!(lr.find(CueList), Some((vec![1, 1], 0)));
        let tb = Preset::TopBottom.build(&[Visualizer, Transport]);
        assert!(matches!(
            tb,
            DockNode::Split {
                axis: Axis::Col,
                ..
            }
        ));
        for preset in Preset::ALL {
            let built = preset.build(&[Looks, Groups]);
            let json = serde_json::to_string(&built).unwrap();
            assert_eq!(serde_json::from_str::<DockNode>(&json).unwrap(), built);
        }
    }

    /// r[verify studio.dock.presets]
    #[test]
    fn the_console_preset_is_faders_under_the_busking_row() {
        let t = console();
        let DockNode::Split {
            axis,
            ratios,
            children,
        } = &t
        else {
            panic!()
        };
        assert_eq!(*axis, Axis::Col);
        assert!((ratios[1] - CONSOLE_FADERS_BAND).abs() < 1e-6);
        assert_eq!(children[1], DockNode::tab(Faders));
        assert_eq!(children[0].panes(), CONSOLE_TOP.to_vec());
        // A window's other panes ride along as tabs in the first column.
        let with = Preset::Console.build(&[CueList, Looks]);
        assert_eq!(
            with.at(&[0, 0]),
            Some(&DockNode::Tabs {
                panes: vec![Looks, CueList],
                active: 0
            })
        );
        assert_eq!(with.panes().len(), CONSOLE_TOP.len() + 2);
    }

    /// r[verify studio.dock.drop-zones]
    #[test]
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
    fn edge_bands_never_meet() {
        assert_eq!(edge_band(400.0), 100.0);
        assert_eq!(edge_band(60.0), 15.0);
        assert_eq!(edge_band(200.0), 50.0);
    }

    /// r[verify studio.dock.no-scroll]
    #[test]
    fn console_fits_a_1440p_monitor() {
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
        // Each pane holds a whole number of columns of its own shape,
        // and they are not the same number: three look cards, one
        // column of group names, three colour discs, one of focus
        // names, four effect cards. `(columns, tile width)`, in
        // `CONSOLE_TOP` order — kept in step with the column counts
        // pinned in `live.css`.
        const SHAPES: [(f32, f32); 7] = [
            (3.0, 240.0), // Looks — three cards, the pane most used
            (1.0, 150.0), // Groups — a name tile
            (3.0, 84.0),  // Colours — discs
            (1.0, 150.0), // Focus — a name tile
            (2.0, 180.0), // Effects — two cards, everything but movement
            (1.0, 200.0), // Movers — one card, wide enough for 16:9
            (1.0, 200.0), // Macros — the same
        ];
        for (pane, (columns, tile)) in CONSOLE_TOP.into_iter().zip(SHAPES) {
            let r = placed.rect_of(pane).unwrap();
            let want = columns * tile + (columns - 1.0) * 6.0 + 16.0;
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
        let css = view::DOCK_CSS;
        assert!(
            css.contains(&format!("height: {}px", TAB_BAR as u32)),
            "tab bar height"
        );
        assert!(
            css.contains(&format!("flex: 0 0 {}px", SPLITTER as u32)),
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
