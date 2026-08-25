//! `no_std`-compatible core, once there is a merge stack to make that true.
//! For now: a placeholder crate boundary so the domain split in
//! `docs/domain/DOMAIN.md` exists in the workspace from day one, rather than
//! being retrofitted once `ignition-ui` has already reached into internals.

pub use ignition_proto::{Attribute, ChanId, PatchEntry, Placement, Quat, Vec3};

pub mod cue;
pub use cue::{Cue, CueList, CuePlayer, CueValue};
