//! Turning a song into a light show.
//!
//! The facade of the `daw` feature, and the half that thinks offline: it
//! takes a song's shape — sections, tempo, lyrics, a hit chart — and
//! produces the cues, effects and triggers that light it, then holds the
//! result to the cue-design guide with [`lint`].
//!
//! Its siblings answer the other questions. `ignition-daw-proto` is the
//! musical vocabulary all of them share, `ignition-daw-reaper` reads a
//! project file into it, and `ignition-daw-transport` says where the
//! playhead is right now. All three are re-exported below, so
//! `ignition_daw::` remains the one import a consumer needs.

pub mod draft;

pub use draft::{Edits, merge, reposition, reposition_from_sidecar};

pub mod chart;
pub mod generate;
pub mod lint;
pub mod mib;
pub use mib::{set_class_timing, set_mib};
pub mod hits;
pub mod lyrics;
pub use generate::{Kind, Roles, generate};

// The transport half and the REAPER reader are sibling crates of this
// one; re-exported so `ignition_daw::` stays the single import a
// consumer needs and no call site had to move.
pub use ignition_daw_proto::{Bars, Section, SongMap, TempoMap, TempoPoint, TimeSignature};
pub use ignition_daw_reaper::{from_rpp, load, point};
#[cfg(feature = "play")]
pub use ignition_daw_transport::SongTransport;
pub use ignition_daw_transport::{SourceTransport, TapClock, TransportSource};
pub use ignition_daw_transport::{timecode, transport};
