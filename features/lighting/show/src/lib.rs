//! The show: recipes, cues, tracking, and the files they live in.
//!
//! The heart of the lighting domain, and one crate because it genuinely
//! is one thing. Nine modules — `recipe`, `cue`, `step`, `programmer`,
//! `profile`, `trigger`, `bump`, `canvas`, `show_file` — form a single
//! mutually-recursive component: a recipe cooks against a show and a cue
//! holds recipes; the programmer edits what a cue stores and a cue reads
//! what the programmer holds; a profile names roles a recipe resolves
//! and is validated by rules a cue obeys. Rust crates cannot be
//! circular, and prying these apart would be a redesign of what a cue
//! *means*, not a move of files. So they sit together, deliberately, and
//! the seams that were real were taken instead: colour and rig below,
//! effects and playback above.
//!
//! Below it: [`ignition_colour`] for what a value is, [`ignition_rig`]
//! for what it lands on, `ignition_daw_proto` for when. All three are
//! re-exported here under the names the domain has always used, so a
//! recipe still says `crate::music::Bars` and `crate::Selection`.
//!
//! See `docs/spec/recipes.md`, `docs/spec/cues.md`, `docs/spec/profile.md`.

pub mod bump;
pub mod canvas;
pub mod cue;
mod num;
pub mod profile;
pub mod programmer;
pub mod recipe;
pub mod show_file;
pub mod step;
pub mod trigger;

pub use bump::Kind as BumpKind;
pub use cue::{Cue, CueList, CuePlayer, CueValue};
pub use profile::{Bindings, Gap, Profile, Role, RoleKind};
pub use programmer::{AttrFilter, FADERS, Fader, KeyAction, Master, MasterMode, Programmer};
pub use recipe::{
    Cook, CueCook, Emit, Expansion, FocusDeltaEmit, Recipe, RecipeApply, RecipeRef, Show, Status,
    cook_cue, cook_list, expand_recipe, expand_recipe_full, unresolved,
};
pub use show_file::{
    Finding, Report, ShowDocument, ShowFile, SongBinding, VenueLayer, VenueManifest, apply_layer,
    check_ig_show, check_show_against_profile, check_venue_against_profile,
};
pub use step::{Direction, Ease, Play, Speed, SpeedMasters, Step, Timing, Waveform};
pub use trigger::{Trigger, TriggerBus};

// ── The layers below, under the names this crate has always used ─────
//
// A `pub use` of a module, not a re-declaration: `crate::selection::…`
// and `crate::color::…` resolve exactly as they did when all of this was
// one crate, which is why twenty-two thousand lines of cue and recipe
// code needed no edit to be split out.
pub use ignition_colour::{
    ColorPreset, ColorSplit, Distribute, FocusPointPreset, Palettes, Ref, SplitProblem,
};
pub use ignition_colour::{color, preset};
pub use ignition_daw_proto as music;
pub use ignition_daw_proto::{
    Bars, Position, Section, SongMap, TempoMap, TempoPoint, TimeSignature,
};
pub use ignition_proto::{Attribute, ChanId, ColorChannel, PatchEntry, Placement, Quat, Vec3};
pub use ignition_rig::{
    Axis, Cmp, Dir, FixtureInfo, Group, Order, Rig, Selection, Trick, Units, Where,
    pan_tilt_deg_along, pan_tilt_deg_to_point,
};
pub use ignition_rig::{focus, group, selection, tricks};
