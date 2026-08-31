//! Addressing the rig: what you select, and how a value spreads over it.
//!
//! The other leaf of the domain, beside `ignition-colour`. A
//! [`selection::Selection`] is a live question — "the movers", "every
//! odd Par in the air" — answered against the rig as patched, in a
//! defined order; [`tricks`] then addresses part of that answer
//! (blocks, wings, shuffles, shifts, mirrors) and spreads a value
//! across it. [`focus`] turns a point in the room into pan and tilt for
//! whichever fixture is being asked.
//!
//! See `docs/spec/groups.md`, `docs/spec/tricks.md`, `docs/spec/focus.md`.
//!
//! Knows nothing about cues, recipes or time. The layers above resolve a
//! template *through* this crate; this crate never calls back up.

pub mod focus;
pub mod group;
pub mod selection;
pub mod tricks;

pub use focus::{pan_tilt_deg_along, pan_tilt_deg_to_point};
pub use group::Group;
pub use selection::{Axis, Cmp, Dir, FixtureInfo, Order, Rig, Selection, Where};
pub use tricks::{Trick, Units};

pub use ignition_proto::{Attribute, ChanId, PatchEntry, Placement, Quat, Vec3};
