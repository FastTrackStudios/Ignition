//! The lighting domain, with no I/O and no renderer in it.
//!
//! Everything a console *means* lives here: the attribute model and patch
//! (`profile`, `selection`), what an operator selects and addresses
//! (`group`, `tricks`), what a value is (`color`, `focus`, `preset`), the
//! template layer that resolves against a live rig (`recipe`, `step`,
//! `effects`), the cue engine and its tracking (`cue`), the priority stack
//! everything merges through (`playbacks`, `programmer`, `bump`), the
//! musical clock cues are written against (`music`, `trigger`), and the
//! files all of it is stored in (`show_file`). Nothing in this crate opens
//! a socket, reads a frame or touches Bevy — `ignition-io` sends the
//! bytes, `ignition-viz` draws them, `ignition-song` supplies the
//! arrangement, and the studio composes the three.
//!
//! `no_std`-compatible is still the aim rather than the state: the merge
//! stack is here but `std` collections and `String` are used throughout,
//! so the `alloc`-only split has not been made yet.

pub use ignition_proto::{Attribute, ChanId, PatchEntry, Placement, Quat, Vec3};

pub mod cue;
pub use cue::{Cue, CueList, CuePlayer, CueValue};

pub mod group;
pub use group::Group;

pub mod color;
pub mod preset;
pub use preset::{
    ColorPreset, ColorSplit, Distribute, FocusPointPreset, Palettes, Ref, SplitProblem,
};

pub mod focus;
pub use focus::{pan_tilt_deg_along, pan_tilt_deg_to_point};

pub mod music;
pub use music::{Bars, Position, Section, SongMap, TempoMap, TempoPoint, TimeSignature};

pub mod bump;
pub mod canvas;
pub use bump::Kind as BumpKind;

pub mod effects;

pub mod profile;
pub use profile::{Bindings, Gap, Profile, Role, RoleKind};

pub mod show_file;
pub use show_file::{
    Finding, Report, ShowDocument, ShowFile, SongBinding, VenueLayer, VenueManifest, apply_layer,
    check_ig_show, check_show_against_profile, check_venue_against_profile,
};

pub mod tricks;
pub use tricks::{Trick, Units};

pub mod trigger;
pub use trigger::{Trigger, TriggerBus};

pub mod programmer;
pub use programmer::{AttrFilter, FADERS, Fader, KeyAction, Master, MasterMode, Programmer};

pub mod macros;
pub use macros::{HostRequest, MacroRunner};

pub mod playbacks;
pub use playbacks::{Class, Playback, Playbacks};

pub mod recipe;
pub mod step;
pub use step::{Direction, Ease, Play, Speed, SpeedMasters, Step, Timing, Waveform};

pub mod selection;
pub use selection::{Axis, Cmp, Dir, FixtureInfo, Order, Rig, Selection, Where};

pub use recipe::{
    Cook, CueCook, Emit, Expansion, FocusDeltaEmit, Recipe, RecipeApply, RecipeRef, Show, Status,
    cook_cue, cook_list, expand_recipe, expand_recipe_full, unresolved,
};
