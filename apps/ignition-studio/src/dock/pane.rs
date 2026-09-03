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
    pub const ALL: [Self; 23] = [
        Self::CueList,
        Self::Visualizer,
        Self::Transport,
        Self::Faders,
        Self::Looks,
        Self::Macros,
        Self::Groups,
        Self::Colours,
        Self::Splits,
        Self::Focus,
        Self::Effects,
        Self::Movers,
        Self::Tricks,
        Self::Bundles,
        Self::Programmer,
        Self::Library,
        Self::Desk,
        Self::Output,
        Self::CommandLine,
        Self::Canvases,
        Self::Cameras,
        Self::Lyrics,
        Self::Programme,
    ];

    /// The name on the tab.
    pub const fn label(self) -> &'static str {
        match self {
            Self::CueList => "Cue List",
            Self::Visualizer => "Visualizer",
            Self::Transport => "Transport",
            Self::Faders => "Faders",
            Self::Looks => "Looks",
            Self::Macros => "Macros",
            Self::Groups => "Groups",
            Self::Colours => "Colours",
            Self::Splits => "Splits",
            Self::Focus => "Focus",
            Self::Effects => "Effects",
            Self::Movers => "Movers",
            Self::Tricks => "Tricks",
            Self::Bundles => "Bundles",
            Self::Programmer => "Programmer",
            Self::Library => "Library",
            Self::Desk => "Desk",
            Self::Output => "Output",
            Self::CommandLine => "Command Line",
            Self::Canvases => "Canvases",
            Self::Cameras => "Cameras",
            Self::Lyrics => "Lyrics",
            Self::Programme => "Programme",
        }
    }

    /// The layout-file spelling, which doubles as a CSS class.
    pub const fn key(self) -> &'static str {
        match self {
            Self::CueList => "cue_list",
            Self::Visualizer => "visualizer",
            Self::Transport => "transport",
            Self::Faders => "faders",
            Self::Looks => "looks",
            Self::Macros => "macros",
            Self::Groups => "groups",
            Self::Colours => "colours",
            Self::Splits => "splits",
            Self::Focus => "focus",
            Self::Effects => "effects",
            Self::Movers => "movers",
            Self::Tricks => "tricks",
            Self::Bundles => "bundles",
            Self::Programmer => "programmer",
            Self::Library => "library",
            Self::Desk => "desk",
            Self::Output => "output",
            Self::CommandLine => "command_line",
            Self::Canvases => "canvases",
            Self::Cameras => "cameras",
            Self::Lyrics => "lyrics",
            Self::Programme => "programme",
        }
    }
}
