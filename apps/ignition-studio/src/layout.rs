//! An operator's window layout: which panels live in which window, on
//! which monitor, and how that window sits on it.
//!
//! The layout is the `windows` key of the operator file
//! (`data/operators/<name>.ig-user`). The rest of that file —
//! favourites, default mode and view — belongs to `operators.rs`; this
//! module reads the file as loose JSON, takes `windows`, and writes back
//! read-modify-write so nothing else in it is touched. Two modules, one
//! file, no shared schema beyond the key name.
//!
//! Nothing here knows about winit. Monitors are names and positions,
//! sizes are pixels, and the one piece of geometry — a docked region on
//! a monitor — is arithmetic, so it is all testable without a display.

// r[impl studio.operators.layout] - per window: monitor, fullscreen or docked region, tree, view

use crate::dock::{Axis, DockNode, PaneKind, Preset};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The panel vocabulary of layout files written before the dock: a flat
/// `panels` array. Still read, never written; each maps onto the panes
/// that replaced it (`migrate`).
// r[impl studio.panels] - the old panel names still open
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    CueList,
    Visualizer,
    Transport,
    Busking,
    Palettes,
    Library,
    Programmer,
    CommandLine,
    Output,
    Canvases,
    Cameras,
    Lyrics,
}

impl Panel {
    /// The panes an old panel stands for. `busking` was the whole Live
    /// view, so it becomes the Console preset; `palettes` and `library`
    /// were tab strips, so they become tabs.
    pub fn panes(self) -> Vec<PaneKind> {
        match self {
            Panel::CueList => vec![PaneKind::CueList],
            Panel::Visualizer => vec![PaneKind::Visualizer],
            Panel::Transport => vec![PaneKind::Transport],
            Panel::Busking => Preset::Console.build(&[]).panes(),
            Panel::Palettes => vec![
                PaneKind::Colours,
                PaneKind::Splits,
                PaneKind::Focus,
                PaneKind::Groups,
            ],
            Panel::Library => vec![
                PaneKind::Effects,
                PaneKind::Tricks,
                PaneKind::Bundles,
                PaneKind::Looks,
                PaneKind::Macros,
                PaneKind::Library,
            ],
            Panel::Programmer => vec![PaneKind::Programmer],
            Panel::CommandLine => vec![PaneKind::CommandLine],
            Panel::Output => vec![PaneKind::Output],
            Panel::Canvases => vec![PaneKind::Canvases],
            Panel::Cameras => vec![PaneKind::Cameras],
            Panel::Lyrics => vec![PaneKind::Lyrics],
        }
    }
}

/// An old `panels` array as a tree: one `Tabs` leaf of every pane the
/// panels stood for, except that a `busking` panel keeps the Console
/// shape (with the other panels tabbed into its first column) — a
/// tab strip of eight busking panes is not what that window was.
// r[impl studio.dock] - old layouts open as one tabbed leaf
pub fn migrate(panels: &[Panel]) -> DockNode {
    let mut panes: Vec<PaneKind> = Vec::new();
    for panel in panels {
        for pane in panel.panes() {
            if !panes.contains(&pane) {
                panes.push(pane);
            }
        }
    }
    if panels.contains(&Panel::Busking) {
        let others: Vec<PaneKind> = panes
            .iter()
            .copied()
            .filter(|p| !Panel::Busking.panes().contains(p))
            .collect();
        return Preset::Console.build(&others);
    }
    DockNode::tabs(panes)
}

/// Where on a monitor a docked window sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    Left,
    Right,
    #[serde(alias = "center")]
    Centre,
}

/// Fullscreen on the monitor, or a docked region of it.
///
/// Serialised as the string `"fullscreen"` or
/// `{"docked": {"region": "right", "fraction": 0.5}}`.
// r[impl studio.windows.wayland] - placement is intent: a monitor and a region, not pixels
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Fullscreen,
    Docked {
        region: Region,
        /// Of the monitor's width, 0–1.
        fraction: f32,
    },
}

/// The two views a window can show — see `r[studio.views]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum View {
    #[default]
    Program,
    Live,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Program => "Program",
            View::Live => "Live",
        }
    }
    pub fn other(self) -> View {
        match self {
            View::Program => View::Live,
            View::Live => View::Program,
        }
    }
}

/// One window of the layout: where it sits and the dock tree it draws.
///
/// Written with a `tree`; read with either a `tree` or the older
/// `panels` array, which becomes a tree on the way in.
// r[impl studio.dock] - a window is a tree
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawWindowSpec")]
pub struct WindowSpec {
    /// An output name (`DP-4`), a `x,y` corner, `left` / `centre` /
    /// `right` by position, or `primary`. Empty means the compositor's
    /// choice.
    #[serde(default)]
    pub monitor: String,
    #[serde(default = "fullscreen")]
    pub placement: Placement,
    pub tree: DockNode,
    #[serde(default)]
    pub view: View,
}

#[derive(Deserialize)]
struct RawWindowSpec {
    #[serde(default)]
    monitor: String,
    #[serde(default = "fullscreen")]
    placement: Placement,
    #[serde(default)]
    tree: Option<DockNode>,
    #[serde(default)]
    panels: Vec<Panel>,
    #[serde(default)]
    view: View,
}

impl From<RawWindowSpec> for WindowSpec {
    fn from(raw: RawWindowSpec) -> Self {
        let mut tree = raw.tree.unwrap_or_else(|| migrate(&raw.panels));
        tree.normalize();
        WindowSpec {
            monitor: raw.monitor,
            placement: raw.placement,
            tree,
            view: raw.view,
        }
    }
}

fn fullscreen() -> Placement {
    Placement::Fullscreen
}

impl WindowSpec {
    /// The panes the window holds, leaves left to right.
    pub fn panes(&self) -> Vec<PaneKind> {
        self.tree.panes()
    }

    /// `Ignition Studio — Cue List, Transport`. The title is what a
    /// compositor rule matches on, so it is deterministic in the panes.
    pub fn title(&self) -> String {
        let panes = self.panes();
        if panes.is_empty() {
            return "Ignition Studio".to_string();
        }
        let names: Vec<&str> = panes.iter().map(|p| p.label()).collect();
        format!("Ignition Studio — {}", names.join(", "))
    }
}

/// The whole layout: every window, in the order they open. The first is
/// the launch window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub windows: Vec<WindowSpec>,
}

impl Layout {
    /// Today's single-window studio, for when no operator is selected.
    /// The window is placed the way it always was — `IGNITION_MONITOR`
    /// and `IGNITION_FULLSCREEN`, read by `place_window` in `main.rs`,
    /// which is why the monitor is left blank here.
    pub fn default_single_window() -> Self {
        Self {
            windows: vec![WindowSpec {
                monitor: String::new(),
                placement: Placement::Fullscreen,
                // Cue list down the left; transport over the visualizer
                // over the console.
                tree: DockNode::split_with(
                    Axis::Row,
                    vec![0.18, 0.82],
                    vec![
                        DockNode::tab(PaneKind::CueList),
                        DockNode::split_with(
                            Axis::Col,
                            vec![0.07, 0.43, 0.5],
                            vec![
                                DockNode::tab(PaneKind::Transport),
                                DockNode::tab(PaneKind::Visualizer),
                                Preset::Console.build(&[]),
                            ],
                        ),
                    ],
                ),
                view: View::Live,
            }],
        }
    }
}

/// A monitor as the layout needs to see it: its name, its top-left
/// corner and its size, all in physical pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorInfo {
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The pixels a docked region covers on a monitor. Full height, a
/// fraction of the width, against the named edge (or centred). The
/// fraction is clamped to (0, 1]; a window of no width is not a window.
// r[impl studio.windows.wayland] - region → size, the studio's half of the placement
pub fn docked_rect(monitor: &MonitorInfo, region: Region, fraction: f32) -> PixelRect {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.05, 1.0)
    } else {
        1.0
    };
    let width = ((monitor.width as f32) * fraction).round().max(1.0) as u32;
    let x = match region {
        Region::Left => monitor.x,
        Region::Right => monitor.x + (monitor.width - width) as i32,
        Region::Centre => monitor.x + ((monitor.width - width) / 2) as i32,
    };
    PixelRect {
        x,
        y: monitor.y,
        width,
        height: monitor.height,
    }
}

/// Which of `monitors` a spec's `monitor` string means. Name first
/// (case-insensitive), then an `x,y` corner, then `left` / `centre` /
/// `right` by position, then `primary` (the caller's choice, passed in).
/// `None` when nothing matches — the caller falls back to primary.
pub fn pick_monitor_index(
    monitors: &[MonitorInfo],
    want: &str,
    primary: Option<usize>,
) -> Option<usize> {
    let want = want.trim();
    if want.is_empty() || monitors.is_empty() {
        return None;
    }
    if let Some(i) = monitors.iter().position(|m| {
        m.name
            .as_deref()
            .is_some_and(|n| n.eq_ignore_ascii_case(want))
    }) {
        return Some(i);
    }
    if let Some((x, y)) = want
        .split_once(',')
        .and_then(|(x, y)| Some((x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?)))
        && let Some(i) = monitors.iter().position(|m| m.x == x && m.y == y)
    {
        return Some(i);
    }
    let mut by_x: Vec<usize> = (0..monitors.len()).collect();
    by_x.sort_by_key(|&i| monitors[i].x);
    match want.to_ascii_lowercase().as_str() {
        "left" => by_x.first().copied(),
        "right" => by_x.last().copied(),
        "centre" | "center" | "middle" => by_x.get(by_x.len() / 2).copied(),
        "primary" => primary,
        _ => None,
    }
}

/// Where operator files live, relative to the working directory like
/// every other data path the studio opens.
pub const DIR: &str = "data/operators";

/// The operator whose layout to restore: `IGNITION_OPERATOR`, or none.
pub fn selected_operator() -> Option<String> {
    std::env::var("IGNITION_OPERATOR")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn path_of(name: &str) -> PathBuf {
    Path::new(DIR).join(format!("{name}.ig-user"))
}

/// The `windows` of an operator file. A file with no `windows` key is a
/// valid operator with no layout, and reads as the single window.
pub fn parse(json: &str) -> anyhow::Result<Layout> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    match value.get("windows") {
        Some(windows) => Ok(Layout {
            windows: serde_json::from_value(windows.clone())?,
        }),
        None => Ok(Layout::default_single_window()),
    }
}

pub fn load(name: &str) -> anyhow::Result<Layout> {
    let path = path_of(name);
    let raw =
        std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    let layout = parse(&raw)?;
    if layout.windows.is_empty() {
        anyhow::bail!("{}: layout names no windows", path.display());
    }
    Ok(layout)
}

/// The file with its `windows` replaced and everything else — the
/// other module's favourites included — carried through untouched.
pub fn merge_into(existing: &str, layout: &Layout) -> anyhow::Result<String> {
    let mut value: serde_json::Value = if existing.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(existing)?
    };
    let serde_json::Value::Object(map) = &mut value else {
        anyhow::bail!("operator file is not a JSON object");
    };
    map.insert("windows".into(), serde_json::to_value(&layout.windows)?);
    Ok(serde_json::to_string_pretty(&value)? + "\n")
}

/// "Save layout": read-modify-write of the operator file.
pub fn save(name: &str, layout: &Layout) -> anyhow::Result<PathBuf> {
    let path = path_of(name);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut merged: serde_json::Value = serde_json::from_str(&merge_into(&existing, layout)?)?;
    if let Some(map) = merged.as_object_mut()
        && !map.contains_key("name")
    {
        map.insert("name".into(), serde_json::Value::String(name.to_string()));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&merged)? + "\n")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODY: &str = r#"{
      "name": "cody",
      "windows": [
        { "monitor": "DP-5", "placement": "fullscreen", "panels": ["cue_list"], "view": "live" },
        { "monitor": "DP-4", "placement": { "docked": { "region": "right", "fraction": 0.5 } },
          "panels": ["visualizer", "transport"] },
        { "monitor": "DP-3", "placement": "fullscreen",
          "panels": ["busking", "palettes", "library"], "view": "live" }
      ],
      "favourites": { "effects": ["chase"] }
    }"#;

    /// r[verify studio.operators.layout]
    #[test]
    fn the_shipped_layout_parses() {
        let layout = parse(CODY).unwrap();
        assert_eq!(layout.windows.len(), 3);
        assert_eq!(layout.windows[0].monitor, "DP-5");
        assert_eq!(layout.windows[0].placement, Placement::Fullscreen);
        assert_eq!(layout.windows[0].tree, DockNode::tab(PaneKind::CueList));
        assert_eq!(layout.windows[0].view, View::Live);
        assert_eq!(
            layout.windows[1].placement,
            Placement::Docked {
                region: Region::Right,
                fraction: 0.5
            }
        );
        // No view named: Program, the default.
        assert_eq!(layout.windows[1].view, View::Program);
        // The old busking window becomes the Console, with the palettes'
        // and library's panes tabbed into its first column.
        let third = &layout.windows[2].tree;
        assert!(matches!(
            third,
            DockNode::Split {
                axis: Axis::Col,
                ..
            }
        ));
        assert!(third.contains(PaneKind::Faders));
        assert!(third.contains(PaneKind::Splits));
        assert!(third.contains(PaneKind::Library));
        assert_eq!(third.find(PaneKind::Looks), Some((vec![0, 0], 0)));
    }

    /// r[verify studio.dock]
    #[test]
    fn an_old_panels_array_becomes_one_tabbed_leaf() {
        let layout =
            parse(r#"{"windows":[{"monitor":"DP-4","panels":["visualizer","transport"]}]}"#)
                .unwrap();
        assert_eq!(
            layout.windows[0].tree,
            DockNode::tabs(vec![PaneKind::Visualizer, PaneKind::Transport])
        );
        // Written back, it is a tree, not a panels array.
        let json = serde_json::to_string(&layout.windows[0]).unwrap();
        assert!(
            json.contains(r#""tree""#) && !json.contains(r#""panels""#),
            "{json}"
        );
        // A tree wins over panels when both are present.
        let both =
            parse(r#"{"windows":[{"panels":["cue_list"],"tree":{"tabs":{"panes":["faders"]}}}]}"#)
                .unwrap();
        assert_eq!(both.windows[0].tree, DockNode::tab(PaneKind::Faders));
        // A tree with an empty leaf inside is tidied on the way in.
        let messy = parse(
            r#"{"windows":[{"tree":{"split":{"axis":"row","children":[
                {"tabs":{"panes":[]}},{"tabs":{"panes":["looks"]}}]}}}]}"#,
        )
        .unwrap();
        assert_eq!(messy.windows[0].tree, DockNode::tab(PaneKind::Looks));
    }

    /// r[verify studio.operators.layout]
    #[test]
    fn the_shipped_file_on_disk_parses() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/operators/cody.ig-user"
        );
        let raw = std::fs::read_to_string(path).expect("data/operators/cody.ig-user");
        let layout = parse(&raw).unwrap();
        assert_eq!(layout.windows.len(), 3);
        // DP-5: the cue list, with the transport as a thin band under
        // it. Where the song is and which cue is standing are the same
        // question, and answering it should not mean looking at another
        // monitor.
        let DockNode::Split {
            axis,
            ratios,
            children,
        } = &layout.windows[0].tree
        else {
            panic!("DP-5 is a split")
        };
        assert_eq!(*axis, Axis::Col);
        assert_eq!(children[0], DockNode::tab(PaneKind::CueList));
        assert_eq!(children[1], DockNode::tab(PaneKind::Transport));
        assert!(
            ratios[1] < 0.08,
            "the transport is a bar, not a panel: {ratios:?}"
        );
        // DP-4: the ultrawide, fullscreen, split down the middle. It is
        // two 1440p screens' worth of pixels, so it holds two 2560×1440
        // views side by side — the cut on the left, the wide shot on the
        // right. Both live, neither hiding the other behind a tab.
        assert_eq!(layout.windows[1].placement, Placement::Fullscreen);
        let DockNode::Split { axis, children, .. } = &layout.windows[1].tree else {
            panic!("DP-4 is a split")
        };
        assert_eq!(*axis, Axis::Row);
        assert_eq!(children[0], DockNode::tab(PaneKind::Programme));
        assert_eq!(children[1], DockNode::tab(PaneKind::Visualizer));
        // DP-3: the Console, Live. The *panes* are the Console's, in
        // the Console's order — Colours being one pane that holds the
        // whole palette, gels and multi-colour alike, so no leaf is
        // needed for splits. The ratios are deliberately not checked:
        // they are what the operator last dragged the splitters to, and
        // a test that pins them fails every time somebody makes the
        // effects column wider, which is the file working as intended.
        assert_eq!(layout.windows[2].view, View::Live);
        let DockNode::Split { children, .. } = &layout.windows[2].tree else {
            panic!("DP-3 is a split")
        };
        let DockNode::Split { children: top, .. } = &children[0] else {
            panic!("the Console's top row is a split")
        };
        let got: Vec<PaneKind> = top
            .iter()
            .map(|leaf| match leaf {
                DockNode::Tabs { panes, active } => panes[*active],
                _ => panic!("every top leaf is tabs"),
            })
            .collect();
        assert_eq!(got, crate::dock::CONSOLE_TOP.to_vec());
        assert_eq!(children[1], DockNode::tab(PaneKind::Faders));
    }

    /// r[verify studio.operators.layout]
    #[test]
    fn a_file_with_no_windows_is_the_single_window() {
        let layout = parse(r#"{"name":"x","favourites":{}}"#).unwrap();
        assert_eq!(layout, Layout::default_single_window());
        let panes = layout.windows[0].panes();
        assert!(panes.contains(&PaneKind::CueList) && panes.contains(&PaneKind::Faders));
    }

    /// r[verify studio.operators.layout]
    #[test]
    fn saving_keeps_the_other_keys() {
        let mut layout = parse(CODY).unwrap();
        layout.windows.truncate(1);
        let merged = merge_into(CODY, &layout).unwrap();
        let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(value["favourites"]["effects"][0], "chase");
        assert_eq!(value["name"], "cody");
        assert_eq!(value["windows"].as_array().unwrap().len(), 1);
        // And what was written reads back as what was saved.
        assert_eq!(parse(&merged).unwrap(), layout);
    }

    /// r[verify studio.operators.layout]
    #[test]
    fn placement_round_trips_in_both_spellings() {
        let docked: Placement =
            serde_json::from_str(r#"{"docked":{"region":"center","fraction":0.25}}"#).unwrap();
        assert_eq!(
            docked,
            Placement::Docked {
                region: Region::Centre,
                fraction: 0.25
            }
        );
        assert_eq!(
            serde_json::to_string(&Placement::Fullscreen).unwrap(),
            r#""fullscreen""#
        );
        assert_eq!(
            serde_json::to_string(&docked).unwrap(),
            r#"{"docked":{"region":"centre","fraction":0.25}}"#
        );
    }

    fn ultrawide() -> MonitorInfo {
        MonitorInfo {
            name: Some("DP-4".into()),
            x: 1440,
            y: 0,
            width: 5120,
            height: 1440,
        }
    }

    /// r[verify studio.windows.wayland]
    #[test]
    fn a_docked_region_is_a_slice_of_the_monitor() {
        let m = ultrawide();
        let right = docked_rect(&m, Region::Right, 0.5);
        assert_eq!(
            right,
            PixelRect {
                x: 1440 + 2560,
                y: 0,
                width: 2560,
                height: 1440
            }
        );
        let left = docked_rect(&m, Region::Left, 0.25);
        assert_eq!((left.x, left.width), (1440, 1280));
        let centre = docked_rect(&m, Region::Centre, 0.5);
        assert_eq!((centre.x, centre.width), (1440 + 1280, 2560));
        // Rounding: a third of 5120 is 1706.67 → 1707.
        assert_eq!(docked_rect(&m, Region::Left, 1.0 / 3.0).width, 1707);
    }

    /// r[verify studio.windows.wayland]
    #[test]
    fn a_bad_fraction_never_makes_a_zero_width_window() {
        let m = ultrawide();
        assert!(docked_rect(&m, Region::Left, 0.0).width > 0);
        assert!(docked_rect(&m, Region::Left, -1.0).width > 0);
        assert_eq!(docked_rect(&m, Region::Left, 7.0).width, 5120);
        assert_eq!(docked_rect(&m, Region::Left, f32::NAN).width, 5120);
    }

    fn desk() -> Vec<MonitorInfo> {
        vec![
            MonitorInfo {
                name: Some("DP-4".into()),
                x: 1440,
                y: 0,
                width: 5120,
                height: 1440,
            },
            MonitorInfo {
                name: Some("DP-5".into()),
                x: 0,
                y: 0,
                width: 1440,
                height: 2560,
            },
            MonitorInfo {
                name: Some("DP-3".into()),
                x: 6560,
                y: 0,
                width: 2560,
                height: 1440,
            },
        ]
    }

    /// r[verify studio.operators.layout]
    #[test]
    fn monitors_resolve_by_name_corner_and_position() {
        let m = desk();
        assert_eq!(pick_monitor_index(&m, "dp-5", None), Some(1));
        assert_eq!(pick_monitor_index(&m, "6560,0", None), Some(2));
        assert_eq!(pick_monitor_index(&m, "left", None), Some(1));
        assert_eq!(pick_monitor_index(&m, "centre", None), Some(0));
        assert_eq!(pick_monitor_index(&m, "right", None), Some(2));
        assert_eq!(pick_monitor_index(&m, "primary", Some(0)), Some(0));
        assert_eq!(pick_monitor_index(&m, "", Some(0)), None);
        assert_eq!(pick_monitor_index(&m, "HDMI-1", Some(0)), None);
    }

    #[test]
    fn a_window_title_names_its_panels() {
        let spec = WindowSpec {
            monitor: String::new(),
            placement: Placement::Fullscreen,
            tree: DockNode::tabs(vec![PaneKind::Visualizer, PaneKind::Transport]),
            view: View::Live,
        };
        assert_eq!(spec.title(), "Ignition Studio — Visualizer, Transport");
    }

    /// Every panel the spec names still opens, and each one lands on
    /// panes rather than nothing.
    ///
    /// The dock replaced the flat panel list, but layout files written
    /// before it are still on operators' disks. A name that quietly
    /// mapped to nothing would open an empty window and look like a
    /// broken layout rather than a migration that missed one.
    ///
    /// r[verify studio.panels]
    #[test]
    fn every_named_panel_still_opens_onto_panes() {
        let named = [
            Panel::CueList,
            Panel::Visualizer,
            Panel::Transport,
            Panel::Busking,
            Panel::Palettes,
            Panel::Library,
            Panel::Programmer,
            Panel::CommandLine,
            Panel::Output,
            Panel::Canvases,
            Panel::Cameras,
            Panel::Lyrics,
        ];
        for panel in named {
            assert!(
                !panel.panes().is_empty(),
                "{panel:?} migrates to no pane, so an old layout naming it opens empty"
            );
        }

        // And a panel is not tied to one window: the same pane kind can
        // be placed in any of them, which is what "each of which can
        // live in any window" means for the dock.
        let anywhere = Panel::Visualizer.panes();
        for placement in [
            Placement::Fullscreen,
            Placement::Docked {
                region: Region::Right,
                fraction: 0.5,
            },
        ] {
            let spec = WindowSpec {
                monitor: String::new(),
                placement,
                tree: DockNode::tabs(anywhere.clone()),
                view: View::Program,
            };
            assert!(!spec.tree.panes().is_empty(), "a pane refused a window");
        }
    }
}
