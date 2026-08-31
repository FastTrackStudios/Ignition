//! The tree a window is: splits all the way down, tabbed leaves at the
//! bottom, and the operations that reshape it.
//!
//! Plain data with no window in sight — every operation here is a pure
//! function of the tree, which is what lets the whole of it be tested
//! without a renderer. [`DockState`] adds the one piece of state that
//! is not the tree itself: which pane, if any, is soloed, and the tree
//! to put back when it is not.

use super::geometry::DropZone;
use super::pane::PaneKind;
use serde::{Deserialize, Serialize};

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
        let old = std::mem::take(node);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::PaneKind::*;
    use crate::dock::fixtures::console;
    use crate::dock::preset::CONSOLE_TOP;

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
}
