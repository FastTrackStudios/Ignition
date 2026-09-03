//! The six shipped layouts.
//!
//! A preset is a function from "these panes, in this order" to a tree.
//! Console is the one the desk opens on and the only one with hand-set
//! ratios; the rest divide evenly. See `r[studio.dock.presets]`.

use super::pane::PaneKind;
use super::tree::{Axis, DockNode, Path};

/// `MaxPane`'s five layouts, and the console: faders along the bottom,
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
    pub const ALL: [Self; 6] = [
        Self::TwoColumns,
        Self::LeftRightSplit,
        Self::ThreeColumns,
        Self::Grid2x2,
        Self::TopBottom,
        Self::Console,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::TwoColumns => "Two columns",
            Self::LeftRightSplit => "Left + right split",
            Self::ThreeColumns => "Three columns",
            Self::Grid2x2 => "2×2 grid",
            Self::TopBottom => "Top + bottom",
            Self::Console => "Console",
        }
    }

    /// The preset's shape with empty leaves.
    fn shape(self) -> DockNode {
        let e = DockNode::empty;
        match self {
            Self::TwoColumns => DockNode::split(Axis::Row, vec![e(), e()]),
            Self::LeftRightSplit => DockNode::split(
                Axis::Row,
                vec![e(), DockNode::split(Axis::Col, vec![e(), e()])],
            ),
            Self::ThreeColumns => DockNode::split(Axis::Row, vec![e(), e(), e()]),
            Self::Grid2x2 => DockNode::split(
                Axis::Col,
                vec![
                    DockNode::split(Axis::Row, vec![e(), e()]),
                    DockNode::split(Axis::Row, vec![e(), e()]),
                ],
            ),
            Self::TopBottom => DockNode::split(Axis::Col, vec![e(), e()]),
            Self::Console => DockNode::split_with(
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
        if self == Self::Console {
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
        } else {
            let mut seen = Vec::new();
            for (i, pane) in panes.iter().enumerate() {
                if seen.contains(pane) {
                    continue;
                }
                seen.push(*pane);
                // `leaves` is a preset's own shape (`Self::shape`), never
                // empty for any real variant, but `.get` with a
                // saturating clamp keeps that a fact this reads rather
                // than one it assumes.
                let Some(leaf) = leaves.get(i.min(leaves.len().saturating_sub(1))) else {
                    continue;
                };
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
        tree.normalize();
        tree
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::DockNode;
    use crate::dock::PaneKind::*;
    use crate::dock::fixtures::console;

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
}
