//! Timecode: three wire formats, one number.
//!
//! LTC on an audio input, MIDI Time Code on a MIDI port and Art-Net
//! timecode on the network all carry the same thing — hours, minutes,
//! seconds, frames, at a frame rate — and all end up as seconds, which
//! the tempo map turns into bars. The decoders here are pure: they eat
//! bytes or samples and produce a [`Timecode`], so every one of them is
//! tested against a synthesised stream and none needs hardware. The
//! sources that own a port, a socket or an audio stream sit at the
//! bottom and are thin.
//!
//! What every source has in common is that a stop looks like a loss:
//! the frames just stop arriving. So a [`TimecodeState`] reports
//! `playing` while frames are fresh, and `lost` once it had a lock and
//! they stopped; the player holds in either case, which is what
//! `r[song.transport.sources]` asks for.

use crate::transport::TransportSource;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Frames per second, as the three formats name them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRate {
    /// 24 fps, film.
    Film24,
    /// 25 fps, EBU.
    Ebu25,
    /// 29.97 fps drop-frame, NTSC.
    DropFrame2997,
    /// 30 fps, SMPTE non-drop.
    Smpte30,
}

impl FrameRate {
    /// The nominal frame count per second — what the frame *number*
    /// counts up to, which for drop-frame is 30, not 29.97.
    pub fn frames_per_second(self) -> u32 {
        match self {
            Self::Film24 => 24,
            Self::Ebu25 => 25,
            Self::DropFrame2997 | Self::Smpte30 => 30,
        }
    }

    /// Real frames per second of wall time.
    pub fn fps(self) -> f64 {
        match self {
            Self::DropFrame2997 => 30.0 / 1.001,
            other => other.frames_per_second() as f64,
        }
    }

    /// The two-bit rate code MTC and Art-Net share: 0 = 24, 1 = 25,
    /// 2 = 29.97 drop, 3 = 30.
    pub fn from_code(code: u8) -> Self {
        match code & 0b11 {
            0 => Self::Film24,
            1 => Self::Ebu25,
            2 => Self::DropFrame2997,
            _ => Self::Smpte30,
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Self::Film24 => 0,
            Self::Ebu25 => 1,
            Self::DropFrame2997 => 2,
            Self::Smpte30 => 3,
        }
    }
}

/// One timecode frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timecode {
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
    pub frames: u8,
    pub rate: FrameRate,
}

impl Timecode {
    pub fn new(hours: u8, minutes: u8, seconds: u8, frames: u8, rate: FrameRate) -> Self {
        Self {
            hours,
            minutes,
            seconds,
            frames,
            rate,
        }
    }

    /// Seconds since 00:00:00:00.
    ///
    /// Drop-frame counts frames the way the standard does: two frame
    /// numbers are skipped at the top of every minute except each
    /// tenth, so the frame *number* runs ahead of the clock and the
    /// skipped numbers have to be taken back out before dividing.
    pub fn to_seconds(self) -> f64 {
        let per_second = self.rate.frames_per_second() as u64;
        let minutes = self.hours as u64 * 60 + self.minutes as u64;
        let mut frames = (minutes * 60 + self.seconds as u64) * per_second + self.frames as u64;
        if self.rate == FrameRate::DropFrame2997 {
            frames -= 2 * (minutes - minutes / 10);
        }
        frames as f64 / self.rate.fps()
    }

    /// The frame `n` frames after this one, within the same hour.
    /// Used by the encoders in tests and by nothing in the field.
    #[cfg(test)]
    pub fn advance(self, n: u32) -> Self {
        let per_second = self.rate.frames_per_second();
        let mut total = ((self.hours as u32 * 60 + self.minutes as u32) * 60 + self.seconds as u32)
            * per_second
            + self.frames as u32
            + n;
        let seconds = total / per_second;
        total %= per_second;
        Self {
            hours: ((seconds / 3600) % 24) as u8,
            minutes: ((seconds / 60) % 60) as u8,
            seconds: (seconds % 60) as u8,
            frames: total as u8,
            rate: self.rate,
        }
    }
}

/// How long frames may go missing before a locked source is lost.
///
/// Generous against a frame period (a 24 fps frame is 42 ms) so a
/// jittery MIDI stack does not flap between locked and lost; short
/// enough that a stopped transport holds within a quarter of a beat.
pub const LOST_AFTER: Duration = Duration::from_millis(250);

/// What a timecode source knows: the last frame, when it arrived, and
/// whether frames are still arriving.
#[derive(Debug, Clone)]
pub struct TimecodeState {
    last: Option<(Timecode, Instant)>,
    /// Set once a frame has been seen. Distinguishes "never locked" —
    /// idle, nothing to hold — from "locked and then lost".
    locked: bool,
}

impl Default for TimecodeState {
    fn default() -> Self {
        Self::new()
    }
}

impl TimecodeState {
    pub fn new() -> Self {
        Self {
            last: None,
            locked: false,
        }
    }

    pub fn observe(&mut self, timecode: Timecode, now: Instant) {
        self.last = Some((timecode, now));
        self.locked = true;
    }

    pub fn last(&self) -> Option<Timecode> {
        self.last.map(|(t, _)| t)
    }

    pub fn seconds(&self) -> Option<f64> {
        self.last.map(|(t, _)| t.to_seconds())
    }

    /// Frames arrived recently.
    pub fn is_playing_at(&self, now: Instant) -> bool {
        self.last
            .is_some_and(|(_, at)| now.saturating_duration_since(at) < LOST_AFTER)
    }

    /// Had a lock, and the frames stopped.
    // r[impl song.transport.sources] - a source reports when it is lost
    pub fn lost_at(&self, now: Instant) -> bool {
        self.locked && !self.is_playing_at(now)
    }
}

/// A [`TransportSource`] over a shared [`TimecodeState`] that some
/// thread — a MIDI callback, a UDP listener, an audio callback — feeds.
#[derive(Debug, Clone, Default)]
pub struct SharedTimecode(pub Arc<Mutex<TimecodeState>>);

impl SharedTimecode {
    pub fn observe(&self, timecode: Timecode) {
        self.lock().observe(timecode, Instant::now());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TimecodeState> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

// r[impl song.transport.sources] - timecode is one more way to derive a position
impl TransportSource for SharedTimecode {
    fn seconds(&self) -> Option<f64> {
        self.lock().seconds()
    }
    fn is_playing(&self) -> bool {
        self.lock().is_playing_at(Instant::now())
    }
    fn lost(&self) -> bool {
        self.lock().lost_at(Instant::now())
    }
}

// ── MIDI Time Code ───────────────────────────────────────────────────

/// Decodes MIDI Time Code: the eight quarter-frame messages (`F1 nd`)
/// that spell out one frame two frames late, and the full-frame sysex
/// (`F0 7F 7F 01 01 hh mm ss ff F7`) a locate sends.
#[derive(Debug, Clone, Default)]
pub struct MtcDecoder {
    nibbles: [Option<u8>; 8],
}

impl MtcDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one MIDI message. Returns a frame when one completes.
    ///
    /// Quarter frames are sent piece 0 first at the start of a frame and
    /// take two frames to arrive, so by the time piece 7 lands the
    /// transport is two frames past the time spelled out; the returned
    /// frame is corrected for that, as the MTC spec says a receiver
    /// should. A full-frame message is current as sent.
    pub fn feed(&mut self, message: &[u8]) -> Option<Timecode> {
        match message {
            [0xF1, data] => {
                let piece = (data >> 4) & 0x7;
                let nibble = data & 0xF;
                if piece == 0 {
                    // Piece 0 opens a new frame; anything half-collected
                    // from before is a torn set.
                    self.nibbles = [None; 8];
                }
                self.nibbles[piece as usize] = Some(nibble);
                if piece != 7 || self.nibbles.iter().any(Option::is_none) {
                    return None;
                }
                let n = |i: usize| self.nibbles[i].unwrap_or(0);
                let frames = n(0) | ((n(1) & 0x1) << 4);
                let seconds = n(2) | ((n(3) & 0x3) << 4);
                let minutes = n(4) | ((n(5) & 0x3) << 4);
                let hours = n(6) | ((n(7) & 0x1) << 4);
                let rate = FrameRate::from_code((n(7) >> 1) & 0x3);
                self.nibbles = [None; 8];
                Some(two_frames_on(Timecode::new(
                    hours, minutes, seconds, frames, rate,
                )))
            }
            [0xF0, 0x7F, _, 0x01, 0x01, hh, mm, ss, ff, 0xF7] => Some(Timecode::new(
                hh & 0x1F,
                mm & 0x3F,
                ss & 0x3F,
                ff & 0x1F,
                FrameRate::from_code((hh >> 5) & 0x3),
            )),
            _ => None,
        }
    }
}

/// The frame two frames after `t` — the MTC quarter-frame latency.
fn two_frames_on(t: Timecode) -> Timecode {
    let per_second = t.rate.frames_per_second();
    let mut frames = t.frames as u32 + 2;
    let mut seconds = t.seconds as u32;
    let mut minutes = t.minutes as u32;
    let mut hours = t.hours as u32;
    if frames >= per_second {
        frames -= per_second;
        seconds += 1;
        if seconds == 60 {
            seconds = 0;
            minutes += 1;
            if minutes == 60 {
                minutes = 0;
                hours = (hours + 1) % 24;
            }
        }
    }
    Timecode::new(
        hours as u8,
        minutes as u8,
        seconds as u8,
        frames as u8,
        t.rate,
    )
}

/// MIDI Time Code from a MIDI input port.
#[cfg(feature = "mtc")]
pub struct MtcSource {
    state: SharedTimecode,
    _connection: midir::MidiInputConnection<()>,
}

#[cfg(feature = "mtc")]
impl MtcSource {
    /// Opens the first input port whose name contains `port`, or the
    /// first port at all when `port` is empty.
    pub fn open(port: &str) -> anyhow::Result<Self> {
        let input = midir::MidiInput::new("ignition mtc")?;
        let ports = input.ports();
        let chosen = ports
            .iter()
            .find(|p| {
                port.is_empty()
                    || input
                        .port_name(p)
                        .map(|n| n.contains(port))
                        .unwrap_or(false)
            })
            .ok_or_else(|| anyhow::anyhow!("no MIDI input port matching {port:?}"))?;
        let state = SharedTimecode::default();
        let feed = state.clone();
        let mut decoder = MtcDecoder::new();
        let connection = input
            .connect(
                chosen,
                "ignition mtc",
                move |_stamp, message, ()| {
                    if let Some(t) = decoder.feed(message) {
                        feed.observe(t);
                    }
                },
                (),
            )
            .map_err(|e| anyhow::anyhow!("connecting MIDI input: {e}"))?;
        Ok(Self {
            state,
            _connection: connection,
        })
    }

    pub fn state(&self) -> &SharedTimecode {
        &self.state
    }
}

#[cfg(feature = "mtc")]
impl TransportSource for MtcSource {
    fn seconds(&self) -> Option<f64> {
        self.state.seconds()
    }
    fn is_playing(&self) -> bool {
        self.state.is_playing()
    }
    fn lost(&self) -> bool {
        self.state.lost()
    }
}

// ── Art-Net timecode ─────────────────────────────────────────────────

/// The Art-Net port.
pub const ARTNET_PORT: u16 = 6454;
/// `OpTimeCode`, little-endian on the wire.
pub const ARTNET_OP_TIMECODE: u16 = 0x9700;

/// Parses an `ArtTimeCode` packet: the `Art-Net\0` id, the op-code
/// (low byte first), the protocol version (high byte first, 14), two
/// filler bytes, then frames, seconds, minutes, hours and the type.
pub fn parse_artnet_timecode(packet: &[u8]) -> Option<Timecode> {
    let body = packet.strip_prefix(b"Art-Net\0")?;
    if body.len() < 11 {
        return None;
    }
    let op = u16::from_le_bytes([body[0], body[1]]);
    if op != ARTNET_OP_TIMECODE {
        return None;
    }
    let version = u16::from_be_bytes([body[2], body[3]]);
    if version < 14 {
        return None;
    }
    let (frames, seconds, minutes, hours, kind) = (body[6], body[7], body[8], body[9], body[10]);
    if kind > 3 || frames > 29 || seconds > 59 || minutes > 59 || hours > 23 {
        return None;
    }
    Some(Timecode::new(
        hours,
        minutes,
        seconds,
        frames,
        FrameRate::from_code(kind),
    ))
}

/// Builds an `ArtTimeCode` packet — for tests, and for anything that
/// wants to *send* the show's own position onto the network.
pub fn artnet_timecode_packet(t: Timecode) -> [u8; 19] {
    let mut p = [0u8; 19];
    p[..8].copy_from_slice(b"Art-Net\0");
    p[8..10].copy_from_slice(&ARTNET_OP_TIMECODE.to_le_bytes());
    p[10..12].copy_from_slice(&14u16.to_be_bytes());
    p[14] = t.frames;
    p[15] = t.seconds;
    p[16] = t.minutes;
    p[17] = t.hours;
    p[18] = t.rate.code();
    p
}

/// Art-Net timecode from the network. Needs nothing but `std`: a UDP
/// socket on port 6454 and a thread that parses what arrives.
pub struct ArtNetTimecodeSource {
    state: SharedTimecode,
    _listener: std::thread::JoinHandle<()>,
}

impl ArtNetTimecodeSource {
    /// Listens on every interface at the Art-Net port.
    pub fn bind() -> anyhow::Result<Self> {
        Self::bind_to((std::net::Ipv4Addr::UNSPECIFIED, ARTNET_PORT))
    }

    pub fn bind_to(addr: impl std::net::ToSocketAddrs) -> anyhow::Result<Self> {
        let socket = std::net::UdpSocket::bind(addr)?;
        let state = SharedTimecode::default();
        let feed = state.clone();
        let listener = std::thread::Builder::new()
            .name("artnet-timecode".into())
            .spawn(move || {
                let mut buf = [0u8; 64];
                while let Ok((n, _)) = socket.recv_from(&mut buf) {
                    if let Some(t) = parse_artnet_timecode(&buf[..n]) {
                        feed.observe(t);
                    }
                }
            })?;
        Ok(Self {
            state,
            _listener: listener,
        })
    }

    pub fn state(&self) -> &SharedTimecode {
        &self.state
    }
}

impl TransportSource for ArtNetTimecodeSource {
    fn seconds(&self) -> Option<f64> {
        self.state.seconds()
    }
    fn is_playing(&self) -> bool {
        self.state.is_playing()
    }
    fn lost(&self) -> bool {
        self.state.lost()
    }
}

// ── Linear Time Code ─────────────────────────────────────────────────

/// The LTC sync word, bits 64..80 of every frame in transmission order.
pub const LTC_SYNC: [bool; 16] = [
    false, false, true, true, true, true, true, true, true, true, true, true, true, true, false,
    true,
];

/// Decodes LTC from an audio sample stream.
///
/// LTC is biphase mark: every bit period starts with a transition, and a
/// `1` has a second one in the middle. So the decoder watches zero
/// crossings, measures the gap since the last, and calls a gap of about
/// one period a `0` and two gaps of about half a period a `1` — with the
/// period itself learned from the stream, so 24, 25 and 30 fps all lock
/// without being told. Eighty bits make a frame; the last sixteen are
/// the sync word, and the frame is decoded when it appears.
///
/// The frame rate is not written into LTC. It is inferred from how long
/// a frame took, snapped to the nearest standard rate, and read as
/// drop-frame when the frame's own flag says so.
#[derive(Debug, Clone)]
pub struct LtcDecoder {
    sample_rate: f64,
    /// Learned bit period, in samples.
    period: f64,
    last_sample: f32,
    /// Samples since the last transition.
    since: f64,
    /// A half-period gap has been seen and its partner is awaited.
    half: bool,
    bits: std::collections::VecDeque<bool>,
    /// Samples since the last sync word, for the frame-rate estimate.
    frame_samples: f64,
    /// Bits since the last sync word — 80 per frame when locked, and
    /// something else after a dropout.
    frame_bits: usize,
}

impl LtcDecoder {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            // Start between 25 and 30 fps; the stream corrects it within
            // a few bits either way.
            period: sample_rate / (80.0 * 27.0),
            last_sample: 0.0,
            since: 0.0,
            half: false,
            bits: std::collections::VecDeque::with_capacity(80),
            frame_samples: 0.0,
            frame_bits: 0,
        }
    }

    /// Feeds samples; returns every frame completed within them.
    pub fn feed(&mut self, samples: &[f32]) -> Vec<Timecode> {
        let mut out = Vec::new();
        for &s in samples {
            self.since += 1.0;
            self.frame_samples += 1.0;
            let crossed = (s >= 0.0) != (self.last_sample >= 0.0);
            self.last_sample = s;
            if !crossed {
                continue;
            }
            let gap = self.since;
            self.since = 0.0;
            if gap > 0.75 * self.period {
                // A full period: a `0`. A half left dangling before it
                // was noise, and is dropped.
                self.period = 0.9 * self.period + 0.1 * gap;
                self.half = false;
                self.push(false, &mut out);
            } else if gap > 0.25 * self.period {
                self.period = 0.9 * self.period + 0.2 * gap;
                if self.half {
                    self.half = false;
                    self.push(true, &mut out);
                } else {
                    self.half = true;
                }
            }
            // Anything shorter is a glitch between samples; ignored.
        }
        out
    }

    fn push(&mut self, bit: bool, out: &mut Vec<Timecode>) {
        if self.bits.len() == 80 {
            self.bits.pop_front();
        }
        self.bits.push_back(bit);
        self.frame_bits += 1;
        if self.bits.len() < 80 || !self.bits.iter().skip(64).copied().eq(LTC_SYNC) {
            return;
        }
        let locked = self.frame_bits == 80;
        let frame_seconds = self.frame_samples / self.sample_rate;
        self.frame_samples = 0.0;
        self.frame_bits = 0;
        let bits: Vec<bool> = self.bits.iter().copied().collect();
        self.bits.clear();
        // The first sync after a dropout tells us nothing about the
        // frame period; the period from it would be garbage.
        let fps = if locked {
            1.0 / frame_seconds
        } else {
            80.0 * self.period_fps()
        };
        out.push(decode_ltc_bits(&bits, fps));
    }

    /// Frames per second implied by the learned bit period alone.
    fn period_fps(&self) -> f64 {
        self.sample_rate / (80.0 * self.period) / 80.0
    }
}

/// Reads the 64 data bits of an LTC frame, given the measured rate.
fn decode_ltc_bits(bits: &[bool], measured_fps: f64) -> Timecode {
    let field = |start: usize, len: usize| -> u8 {
        (0..len)
            .map(|i| (bits[start + i] as u8) << i)
            .fold(0, |a, b| a | b)
    };
    let frames = field(0, 4) + 10 * field(8, 2);
    let drop = bits[10];
    let seconds = field(16, 4) + 10 * field(24, 3);
    let minutes = field(32, 4) + 10 * field(40, 3);
    let hours = field(48, 4) + 10 * field(56, 2);
    let rate = if drop {
        FrameRate::DropFrame2997
    } else if measured_fps < 24.5 {
        FrameRate::Film24
    } else if measured_fps < 27.5 {
        FrameRate::Ebu25
    } else {
        FrameRate::Smpte30
    };
    Timecode::new(hours, minutes, seconds, frames, rate)
}

/// The 80 bits of one LTC frame, in transmission order. Sets the
/// drop-frame flag from the rate and leaves the user bits clear.
pub fn encode_ltc_bits(t: Timecode) -> [bool; 80] {
    let mut bits = [false; 80];
    let mut put = |start: usize, len: usize, value: u8| {
        for i in 0..len {
            bits[start + i] = (value >> i) & 1 == 1;
        }
    };
    put(0, 4, t.frames % 10);
    put(8, 2, t.frames / 10);
    put(16, 4, t.seconds % 10);
    put(24, 3, t.seconds / 10);
    put(32, 4, t.minutes % 10);
    put(40, 3, t.minutes / 10);
    put(48, 4, t.hours % 10);
    put(56, 2, t.hours / 10);
    bits[10] = t.rate == FrameRate::DropFrame2997;
    bits[64..].copy_from_slice(&LTC_SYNC);
    bits
}

/// Renders frames as biphase-mark audio at `sample_rate`, continuing
/// from `level` (so consecutive calls join without a spurious edge).
pub fn encode_ltc_audio(frames: &[Timecode], sample_rate: f64, level: &mut f32) -> Vec<f32> {
    let mut out = Vec::new();
    for &t in frames {
        let period = sample_rate / (80.0 * t.rate.fps());
        for (i, bit) in encode_ltc_bits(t).into_iter().enumerate() {
            let start = (i as f64 * period).round() as usize;
            let mid = ((i as f64 + 0.5) * period).round() as usize;
            let end = ((i as f64 + 1.0) * period).round() as usize;
            *level = -*level;
            out.extend(std::iter::repeat_n(*level, mid - start));
            if bit {
                *level = -*level;
            }
            out.extend(std::iter::repeat_n(*level, end - mid));
        }
    }
    out
}

/// LTC from an audio input device.
#[cfg(feature = "ltc")]
pub struct LtcSource {
    state: SharedTimecode,
    _stream: cpal::Stream,
}

#[cfg(feature = "ltc")]
impl LtcSource {
    /// Opens the default input device and decodes its first channel.
    pub fn open() -> anyhow::Result<Self> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow::anyhow!("no audio input device"))?;
        let config = device.default_input_config()?;
        let channels = config.channels() as usize;
        let mut decoder = LtcDecoder::new(config.sample_rate() as f64);
        let state = SharedTimecode::default();
        let feed = state.clone();
        let stream = device.build_input_stream(
            config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mono: Vec<f32> = data.iter().step_by(channels.max(1)).copied().collect();
                for t in decoder.feed(&mono) {
                    feed.observe(t);
                }
            },
            |e| tracing::warn!(error = %e, "ltc: input stream error"),
            None,
        )?;
        stream.play()?;
        Ok(Self {
            state,
            _stream: stream,
        })
    }

    pub fn state(&self) -> &SharedTimecode {
        &self.state
    }
}

#[cfg(feature = "ltc")]
impl TransportSource for LtcSource {
    fn seconds(&self) -> Option<f64> {
        self.state.seconds()
    }
    fn is_playing(&self) -> bool {
        self.state.is_playing()
    }
    fn lost(&self) -> bool {
        self.state.lost()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc(h: u8, m: u8, s: u8, f: u8, rate: FrameRate) -> Timecode {
        Timecode::new(h, m, s, f, rate)
    }

    #[test]
    fn timecode_to_seconds_handles_every_rate() {
        assert_eq!(tc(0, 1, 0, 0, FrameRate::Ebu25).to_seconds(), 60.0);
        assert!((tc(0, 0, 1, 12, FrameRate::Film24).to_seconds() - 1.5).abs() < 1e-9);
        assert!((tc(1, 0, 0, 15, FrameRate::Smpte30).to_seconds() - 3600.5).abs() < 1e-9);
        // Drop-frame: after one hour the frame count has dropped 108
        // numbers, and the clock reads one real hour to the millisecond.
        let hour = tc(1, 0, 0, 0, FrameRate::DropFrame2997).to_seconds();
        assert!((hour - 3600.0).abs() < 0.004, "{hour}");
    }

    /// The quarter-frame set for a frame, in the order a sender emits it.
    fn quarter_frames(t: Timecode) -> Vec<[u8; 2]> {
        let hh = t.hours | (t.rate.code() << 5);
        let bytes = [t.frames, t.seconds, t.minutes, hh];
        let mut out = Vec::new();
        for (i, b) in bytes.iter().enumerate() {
            out.push([0xF1, ((i * 2) as u8) << 4 | (b & 0xF)]);
            out.push([0xF1, ((i * 2 + 1) as u8) << 4 | (b >> 4)]);
        }
        out
    }

    /// r[verify song.transport.sources] - MTC quarter-frame and full-frame
    #[test]
    fn mtc_quarter_frames_spell_a_frame_two_frames_late() {
        let mut d = MtcDecoder::new();
        let sent = tc(1, 23, 45, 10, FrameRate::Ebu25);
        let mut got = None;
        for qf in quarter_frames(sent) {
            assert!(got.is_none(), "a frame before the eighth piece");
            got = d.feed(&qf);
        }
        assert_eq!(got, Some(tc(1, 23, 45, 12, FrameRate::Ebu25)));
        // Across a second boundary at 30 fps.
        let sent = tc(0, 0, 59, 29, FrameRate::Smpte30);
        let got = quarter_frames(sent)
            .iter()
            .filter_map(|qf| d.feed(qf))
            .next();
        assert_eq!(got, Some(tc(0, 1, 0, 1, FrameRate::Smpte30)));
        // A torn set — joining mid-frame — yields nothing until a whole
        // one arrives.
        let mut d = MtcDecoder::new();
        let set = quarter_frames(tc(0, 0, 1, 0, FrameRate::Film24));
        assert!(set[4..].iter().all(|qf| d.feed(qf).is_none()));
        assert!(set[..7].iter().all(|qf| d.feed(qf).is_none()));
        assert_eq!(d.feed(&set[7]), Some(tc(0, 0, 1, 2, FrameRate::Film24)));
    }

    #[test]
    fn mtc_full_frame_is_a_locate() {
        let mut d = MtcDecoder::new();
        let msg = [0xF0, 0x7F, 0x7F, 0x01, 0x01, (2 << 5) | 3, 4, 5, 6, 0xF7];
        assert_eq!(d.feed(&msg), Some(tc(3, 4, 5, 6, FrameRate::DropFrame2997)));
        // A note-on is not timecode.
        assert_eq!(d.feed(&[0x90, 60, 100]), None);
    }

    /// r[verify song.transport.sources] - Art-Net ArtTimeCode
    #[test]
    fn artnet_timecode_round_trips_and_rejects_other_packets() {
        let t = tc(2, 3, 4, 5, FrameRate::Smpte30);
        let packet = artnet_timecode_packet(t);
        assert_eq!(&packet[..8], b"Art-Net\0");
        assert_eq!(packet[8], 0x00);
        assert_eq!(packet[9], 0x97);
        assert_eq!(parse_artnet_timecode(&packet), Some(t));
        // An ArtDmx packet (0x5000) is not timecode.
        let mut dmx = packet;
        dmx[8] = 0x00;
        dmx[9] = 0x50;
        assert_eq!(parse_artnet_timecode(&dmx), None);
        // Nor is a truncated one, nor a frame that does not exist.
        assert_eq!(parse_artnet_timecode(&packet[..18]), None);
        let mut bad = packet;
        bad[14] = 31;
        assert_eq!(parse_artnet_timecode(&bad), None);
    }

    #[test]
    fn artnet_timecode_arrives_over_a_socket() {
        let source = ArtNetTimecodeSource::bind_to("127.0.0.1:0").expect("bind");
        // Bound to an ephemeral port we cannot read back through the
        // source; go through the state directly to prove the plumbing
        // — a real deployment binds 6454.
        assert!(!source.lost());
        assert_eq!(source.seconds(), None);
        source.state().observe(tc(0, 0, 10, 0, FrameRate::Ebu25));
        assert_eq!(source.seconds(), Some(10.0));
        assert!(source.is_playing());
    }

    /// r[verify song.transport.sources] - the LTC decoder on a synthesised stream
    #[test]
    fn ltc_decodes_a_synthesised_biphase_stream_at_every_rate() {
        for rate in [
            FrameRate::Film24,
            FrameRate::Ebu25,
            FrameRate::Smpte30,
            FrameRate::DropFrame2997,
        ] {
            let sample_rate = 48_000.0;
            let start = tc(0, 12, 34, 5, rate);
            let frames: Vec<Timecode> = (0..6).map(|i| start.advance(i)).collect();
            let mut level = 0.8;
            let audio = encode_ltc_audio(&frames, sample_rate, &mut level);
            let mut d = LtcDecoder::new(sample_rate);
            // Fed in uneven chunks, as an audio callback would.
            let mut got = Vec::new();
            for chunk in audio.chunks(333) {
                got.extend(d.feed(chunk));
            }
            // The first frame is used to learn the period, and the last
            // is never finished — a bit is only known when the next
            // edge arrives, and the stream ends. Everything between must
            // come through, in order, at the right rate.
            assert!(got.len() >= 5, "{rate:?}: {} frames", got.len());
            let tail = &got[got.len() - 4..];
            assert_eq!(tail, &frames[1..5], "{rate:?}");
        }
    }

    #[test]
    fn ltc_bits_round_trip() {
        let t = tc(23, 59, 58, 29, FrameRate::DropFrame2997);
        let bits = encode_ltc_bits(t);
        assert_eq!(decode_ltc_bits(&bits, 29.97), t);
        assert!(bits[64..].iter().copied().eq(LTC_SYNC));
    }

    /// r[verify song.transport.sources] - a lost source says so; a stopped one is not "playing"
    #[test]
    fn a_source_that_goes_quiet_is_lost_and_one_that_never_locked_is_not() {
        let t0 = Instant::now();
        let mut s = TimecodeState::new();
        assert!(!s.lost_at(t0));
        assert!(!s.is_playing_at(t0));
        s.observe(tc(0, 0, 1, 0, FrameRate::Ebu25), t0);
        assert!(s.is_playing_at(t0 + Duration::from_millis(40)));
        assert!(!s.lost_at(t0 + Duration::from_millis(40)));
        assert!(!s.is_playing_at(t0 + Duration::from_secs(1)));
        assert!(s.lost_at(t0 + Duration::from_secs(1)));
        assert_eq!(s.seconds(), Some(1.0));
    }
}
