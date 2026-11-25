//! Turning a song into a light show — and following it while it plays.
//!
//! The `daw` feature of the lighting domain, in one crate: the offline
//! half takes a song's shape — sections, tempo, lyrics, a hit chart —
//! and produces the cues, effects and triggers that light it, then holds
//! the result to the cue-design guide with [`lint`]; the live half
//! ([`transport`], [`timecode`]) says where the playhead is right now.
//!
//! A song's shape is read from the project it lives in: a REAPER project
//! ([`reaper`]) or a song of an Ableton Live set ([`ableton`], the
//! `ableton` feature). The musical vocabulary itself is
//! `ignition-daw-proto`, a crate of its own only because `ignition-show`
//! sits between it and this one.

pub mod draft;

pub use draft::{Edits, merge, reposition, reposition_from_sidecar};

#[cfg(feature = "ableton")]
pub mod ableton;
pub mod beat_clock;
pub mod chart;
pub mod generate;
pub mod lint;
pub mod lyrics;
pub mod mib;
pub mod reaper;
pub mod timecode;
pub mod transport;

pub use beat_clock::{BeatSource, MidiClockDecoder};
pub use generate::{Kind, Roles, generate};
pub use ignition_daw_proto::{Bars, Section, SongMap, TempoMap, TempoPoint, TimeSignature};
pub use mib::{set_class_timing, set_mib};
pub use reaper::{from_rpp, load, point};
#[cfg(feature = "play")]
pub use transport::SongTransport;
pub use transport::{SourceTransport, TapClock, TransportSource};
