//! `no_std`-compatible core, once there is a merge stack to make that true.
//! For now: a placeholder crate boundary so the domain split in
//! `docs/domain/DOMAIN.md` exists in the workspace from day one, rather than
//! being retrofitted once `ignition-ui` has already reached into internals.

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
pub use programmer::{FADERS, Fader, KeyAction, Master, MasterMode, Programmer};

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
