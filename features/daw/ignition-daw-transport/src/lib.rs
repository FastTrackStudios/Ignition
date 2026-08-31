//! Where the playhead comes from, and how it is decoded.
//!
//! The `daw` feature's transport half, split from the facade because the
//! two answer different questions. `ignition-daw` asks *what shape is
//! this song* — sections, tempo, the hit chart — and answers it once,
//! offline, from a project file. This crate asks *where is the song
//! now*, sixty times a second, and the answer arrives from a device.
//!
//! [`transport::TransportSource`] is the seam: one trait, implemented by
//! an embedded DAW (`play`), a MIDI Time Code port (`mtc`), an LTC
//! audio input (`ltc`) or an Art-Net timecode socket, and consumed by a
//! studio that does not care which.
//!
//! The *decoders* are unconditional and always tested — `MtcDecoder`,
//! `LtcDecoder` and the Art-Net timecode pair turn bytes or samples
//! into a [`timecode::Timecode`] and own no device. Only the ports that
//! feed them are behind a cargo feature, because only the ports cost a
//! device stack: `midir` for MTC, `cpal` for LTC, and the whole
//! standalone DAW for `play`. That is why a timecode test suite runs on
//! a machine with no sound card.

pub mod timecode;
pub mod transport;

#[cfg(feature = "play")]
pub use transport::SongTransport;
pub use transport::{SourceTransport, TapClock, TransportSource};
