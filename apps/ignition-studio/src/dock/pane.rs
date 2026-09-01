//! What a leaf can host.
//!
//! One `PaneKind` per pane implementation. The names are the ones the
//! layout file uses (`cue_list`, ...) and double as CSS classes, so
//! renaming a variant is a file-format change.

use serde::{Deserialize, Serialize};

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
