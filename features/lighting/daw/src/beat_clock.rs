//! A playhead in *beats of the set*, for a show that is one long
//! arrangement — a Live set with every song end to end.
//!
//! [`crate::transport`] reports seconds into one song. A night that is one
//! arrangement is easier to follow in beats of the arrangement: that is
//! what MIDI beat clock carries (Ableton Live sends it, with a Song
//! Position Pointer on every locate, when its sync output is on), and a
//! beat is what the set's locators and section cards are written in.
//! [`crate::ableton::LiveSet`] then says which song a beat is and where
//! in it.
//!
//! The [`MidiClockDecoder`] owns no device and is always built and tested;
//! only the port that feeds it ([`MidiClockPort`], the `midi-clock`
//! feature) costs a MIDI stack.

use std::time::{Duration, Instant};

/// Where a set's playhead is, in beats of the arrangement.
pub trait BeatSource: Send {
    /// The playhead, in quarter-note beats from the top of the set;
    /// `None` until the source has said anything.
    fn beats(&self, now: Instant) -> Option<f64>;
    fn is_playing(&self) -> bool;
}

/// MIDI clock ticks per quarter note.
const PPQN: f64 = 24.0;

/// MIDI beat clock and Song Position Pointer, decoded into a playhead.
///
/// - `F2 lsb msb` (Song Position Pointer): the position, in sixteenths.
/// - `FA` (Start): from the top. `FB` (Continue): from the pointer.
/// - `FC` (Stop): holds where it is.
/// - `F8` (Clock): a 24th of a beat, counted only while playing.
///
/// Between ticks the position is carried forward at the rate the last
/// ticks arrived, never more than one tick ahead, so a light locked to
/// the beat does not step at 24 ppqn.
#[derive(Debug, Clone, Default)]
pub struct MidiClockDecoder {
    base_beats: f64,
    ticks: u64,
    playing: bool,
    heard: bool,
    last_tick: Option<Instant>,
    tick: Option<Duration>,
}

impl MidiClockDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One MIDI message (a status byte and its data).
    pub fn message(&mut self, bytes: &[u8], now: Instant) {
        match bytes {
            [0xF2, lsb, msb, ..] => {
                let sixteenths = u16::from(*lsb & 0x7F) | (u16::from(*msb & 0x7F) << 7);
                self.base_beats = f64::from(sixteenths) / 4.0;
                self.ticks = 0;
                self.last_tick = None;
                self.heard = true;
            }
            [0xFA, ..] => {
                self.base_beats = 0.0;
                self.ticks = 0;
                self.playing = true;
                self.last_tick = None;
                self.heard = true;
            }
            [0xFB, ..] => {
                self.playing = true;
                self.last_tick = None;
                self.heard = true;
            }
            [0xFC, ..] => {
                // Hold at the last tick heard: the beats so far become
                // the base, so a Continue picks up from here.
                self.base_beats = self.counted();
                self.ticks = 0;
                self.playing = false;
                self.last_tick = None;
            }
            [0xF8, ..] if self.playing => {
                self.ticks = self.ticks.saturating_add(1);
                if let Some(last) = self.last_tick {
                    let gap = now.saturating_duration_since(last);
                    // Smooth the rate: a tick's own jitter is a large
                    // fraction of a tick.
                    self.tick = Some(match self.tick {
                        Some(t) => (t * 7 + gap) / 8,
                        None => gap,
                    });
                }
                self.last_tick = Some(now);
            }
            _ => {}
        }
    }

    fn counted(&self) -> f64 {
        #[expect(
            clippy::as_conversions,
            clippy::cast_precision_loss,
            reason = "a tick count within one night is far below 2^52"
        )]
        let ticks = self.ticks as f64;
        self.base_beats + ticks / PPQN
    }

    /// The playhead at `now`.
    #[must_use]
    pub fn beats_at(&self, now: Instant) -> Option<f64> {
        if !self.heard {
            return None;
        }
        let mut beats = self.counted();
        if self.playing
            && let (Some(last), Some(tick)) = (self.last_tick, self.tick)
            && !tick.is_zero()
        {
            let since = now.saturating_duration_since(last).as_secs_f64();
            let fraction = (since / tick.as_secs_f64()).min(1.0);
            beats += fraction / PPQN;
        }
        Some(beats)
    }

    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playing
    }
}

/// A MIDI input port decoded as beat clock.
#[cfg(feature = "midi-clock")]
pub struct MidiClockPort {
    _connection: midir::MidiInputConnection<()>,
    state: std::sync::Arc<std::sync::Mutex<MidiClockDecoder>>,
    /// The port's name, as the system calls it.
    pub name: String,
}

#[cfg(feature = "midi-clock")]
impl MidiClockPort {
    /// The names of the MIDI inputs on this machine.
    ///
    /// # Errors
    ///
    /// The MIDI system cannot be opened.
    pub fn ports() -> anyhow::Result<Vec<String>> {
        let input = midir::MidiInput::new("ignition-clock")?;
        Ok(input
            .ports()
            .iter()
            .filter_map(|p| input.port_name(p).ok())
            .collect())
    }

    /// Opens the first input whose name contains `matching`
    /// (case-insensitive).
    ///
    /// # Errors
    ///
    /// The MIDI system cannot be opened, no port matches, or the port
    /// refuses the connection.
    pub fn open(matching: &str) -> anyhow::Result<Self> {
        let mut input = midir::MidiInput::new("ignition-clock")?;
        // Clock and SPP are system real-time / common messages, which
        // midir drops unless asked for them.
        input.ignore(midir::Ignore::Sysex);
        let wanted = matching.to_lowercase();
        let port = input
            .ports()
            .into_iter()
            .find(|p| {
                input
                    .port_name(p)
                    .is_ok_and(|n| n.to_lowercase().contains(&wanted))
            })
            .ok_or_else(|| anyhow::anyhow!("no MIDI input matches {matching:?}"))?;
        let name = input.port_name(&port).unwrap_or_default();
        let state = std::sync::Arc::new(std::sync::Mutex::new(MidiClockDecoder::new()));
        let sink = std::sync::Arc::clone(&state);
        let connection = input
            .connect(
                &port,
                "ignition-clock-in",
                move |_stamp, bytes, ()| {
                    if let Ok(mut decoder) = sink.lock() {
                        decoder.message(bytes, Instant::now());
                    }
                },
                (),
            )
            .map_err(|e| anyhow::anyhow!("connecting to {name}: {e}"))?;
        Ok(Self {
            _connection: connection,
            state,
            name,
        })
    }
}

#[cfg(feature = "midi-clock")]
impl BeatSource for MidiClockPort {
    fn beats(&self, now: Instant) -> Option<f64> {
        self.state.lock().ok().and_then(|d| d.beats_at(now))
    }

    fn is_playing(&self) -> bool {
        self.state.lock().is_ok_and(|d| d.is_playing())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_song_position_pointer_then_continue_counts_from_the_pointer() {
        let t0 = Instant::now();
        let mut d = MidiClockDecoder::new();
        assert_eq!(d.beats_at(t0), None, "nothing heard yet");
        // Beat 2673 (Hey!) = 10692 sixteenths = 0x53 lsb, 0x53 msb.
        let spp = 2673 * 4;
        d.message(&[0xF2, (spp & 0x7F) as u8, ((spp >> 7) & 0x7F) as u8], t0);
        assert_eq!(d.beats_at(t0), Some(2673.0));
        d.message(&[0xFB], t0);
        for _ in 0..48 {
            d.message(&[0xF8], t0);
        }
        assert_eq!(d.beats_at(t0), Some(2675.0), "48 ticks are two beats");
        d.message(&[0xFC], t0);
        assert!(!d.is_playing());
        assert_eq!(d.beats_at(t0), Some(2675.0), "a stop holds");
        d.message(&[0xF8], t0);
        assert_eq!(
            d.beats_at(t0),
            Some(2675.0),
            "clock while stopped is ignored"
        );
    }

    #[test]
    fn start_is_the_top_of_the_set() {
        let t0 = Instant::now();
        let mut d = MidiClockDecoder::new();
        d.message(&[0xF2, 0x10, 0x00], t0);
        d.message(&[0xFA], t0);
        assert_eq!(d.beats_at(t0), Some(0.0));
        assert!(d.is_playing());
    }

    #[test]
    fn between_ticks_the_playhead_moves_at_most_one_tick() {
        let t0 = Instant::now();
        let tick = Duration::from_millis(20);
        let mut d = MidiClockDecoder::new();
        d.message(&[0xFA], t0);
        d.message(&[0xF8], t0);
        d.message(&[0xF8], t0 + tick);
        let at_tick = d.beats_at(t0 + tick).unwrap_or_default();
        let half = d.beats_at(t0 + tick + tick / 2).unwrap_or_default();
        let late = d.beats_at(t0 + tick * 10).unwrap_or_default();
        assert!((at_tick - 2.0 / PPQN).abs() < 1e-9);
        assert!((half - 2.5 / PPQN).abs() < 1e-6, "{half}");
        assert!((late - 3.0 / PPQN).abs() < 1e-9, "capped one tick ahead");
    }
}
