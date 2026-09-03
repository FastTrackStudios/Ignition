//! Sound-in: a beat and band levels from an audio input.
//!
//! A support act with no song map still gets effects in time, and a
//! blinder can follow the kick — `r[playback.sound-in]`. The detector
//! is pure DSP over mono samples and is tested on a synthesised click
//! track; the `cpal` capture thread behind the `sound` feature is the
//! only part that needs a device, and a machine without one logs and
//! carries on.
//!
//! What it does, in order:
//!
//! 1. Splits the signal into three bands with first-order filters —
//!    low below ~200 Hz, high above ~2 kHz, mid between — and reports
//!    each band's RMS per hop, smoothed, as `Command::SoundLevels`.
//! 2. Takes the energy of each hop and its *flux* (how much louder
//!    than the recent average it got), and calls an onset where the
//!    flux clears an adaptive threshold and enough time has passed
//!    since the last one.
//! 3. Every second, estimates a tempo from the onsets of the last six
//!    seconds: every inter-onset interval, folded into 60–200 BPM, is
//!    binned, and the fullest bin wins. That goes out as
//!    `Command::Tap(bpm)`, retuning the `Tap` master — the same master
//!    the `T` key taps, so a show written against it follows either.

// The detector is pure DSP, tested on a synthesised click track; only
// the `sound` feature's capture thread reaches it at run time.
#![cfg_attr(not(feature = "sound"), allow(dead_code))]

use crate::command::Sender;
use crate::num;

/// Samples per analysis hop. ~11.6 ms at 44.1 kHz: fine enough that an
/// onset lands within a frame, coarse enough that a hop's energy is a
/// number rather than noise.
const HOP: usize = 512;
/// How much history a tempo is estimated over.
const WINDOW_SECS: f32 = 6.0;
/// Two onsets closer than this are one hit.
const MIN_GAP_SECS: f32 = 0.1;
/// Tempo range the estimate is folded into. Anything faster is heard
/// as its half, anything slower as its double — the octave ambiguity
/// every beat tracker has, resolved toward where music mostly lives.
const BPM_MIN: f32 = 60.0;
const BPM_MAX: f32 = 200.0;

/// Band levels, 0..=1, smoothed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Bands {
    pub low: f32,
    pub mid: f32,
    pub high: f32,
}

/// What one hop of audio produced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// An onset at this many seconds into the stream.
    Onset(f32),
    /// A new tempo estimate.
    Tempo(f32),
}

/// A one-pole low-pass: `y += k * (x - y)`.
#[derive(Debug, Clone, Copy)]
struct OnePole {
    k: f32,
    y: f32,
}

impl OnePole {
    fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        // The usual RC approximation; exact enough for a band meter.
        let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
        let dt = 1.0 / sample_rate;
        Self {
            k: dt / (rc + dt),
            y: 0.0,
        }
    }

    fn step(&mut self, x: f32) -> f32 {
        self.y += self.k * (x - self.y);
        self.y
    }
}

/// The onset and tempo detector. Feed it mono samples; take events.
#[derive(Debug, Clone)]
pub struct Detector {
    sample_rate: f32,
    low: OnePole,
    high_cut: OnePole,
    /// Partial hop, waiting to fill.
    hop: Vec<f32>,
    /// Per-band sums of squares for the hop in progress.
    acc: [f32; 3],
    /// Samples consumed so far — the clock.
    samples: u64,
    /// Recent hop energies, for the flux baseline.
    energies: Vec<f32>,
    /// Recent flux values, for the adaptive threshold.
    fluxes: Vec<f32>,
    /// Onset times within the window, seconds.
    onsets: Vec<f32>,
    last_onset: Option<f32>,
    last_estimate_at: f32,
    pub bands: Bands,
    pub tempo: Option<f32>,
}

impl Detector {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            low: OnePole::new(200.0, sample_rate),
            high_cut: OnePole::new(2000.0, sample_rate),
            hop: Vec::with_capacity(HOP),
            acc: [0.0; 3],
            samples: 0,
            energies: Vec::new(),
            fluxes: Vec::new(),
            onsets: Vec::new(),
            last_onset: None,
            last_estimate_at: 0.0,
            bands: Bands::default(),
            tempo: None,
        }
    }

    /// Seconds of audio consumed.
    pub fn now(&self) -> f32 {
        num::f32_of_u64(self.samples) / self.sample_rate
    }

    /// Consumes mono samples and returns whatever they produced.
    pub fn push(&mut self, samples: &[f32]) -> Vec<Event> {
        let mut events = Vec::new();
        for &x in samples {
            let low = self.low.step(x);
            let below_high = self.high_cut.step(x);
            let mid = below_high - low;
            let high = x - below_high;
            self.acc[0] += low * low;
            self.acc[1] += mid * mid;
            self.acc[2] += high * high;
            self.hop.push(x);
            self.samples = self.samples.saturating_add(1);
            if self.hop.len() == HOP {
                self.finish_hop(&mut events);
            }
        }
        events
    }

    fn finish_hop(&mut self, events: &mut Vec<Event>) {
        let n = num::f32_of_usize(HOP);
        let rms = |sum: f32| (sum / n).sqrt();
        // Meters move fast up and slow down, the way a VU does, so a
        // kick reads as a kick rather than as a wobble.
        let smooth = |current: f32, next: f32| {
            if next > current {
                next
            } else {
                (next - current).mul_add(0.25, current)
            }
        };
        let (l, m, h) = (rms(self.acc[0]), rms(self.acc[1]), rms(self.acc[2]));
        self.bands.low = smooth(self.bands.low, (l * 4.0).min(1.0));
        self.bands.mid = smooth(self.bands.mid, (m * 4.0).min(1.0));
        self.bands.high = smooth(self.bands.high, (h * 4.0).min(1.0));

        let energy: f32 = self.hop.iter().map(|x| x * x).sum::<f32>() / n;
        self.hop.clear();
        self.acc = [0.0; 3];

        // Flux against the mean of the last ~0.2 s: how much louder
        // than "just now" this hop is. Only rises count.
        let baseline = if self.energies.is_empty() {
            0.0
        } else {
            self.energies.iter().sum::<f32>() / num::f32_of_usize(self.energies.len())
        };
        let flux = (energy - baseline).max(0.0);
        self.energies.push(energy);
        if self.energies.len() > 16 {
            self.energies.remove(0);
        }

        // Adaptive threshold: mean plus a spread of the recent flux, so
        // a quiet room and a loud one both find their hits, with a
        // floor so silence does not fire on nothing.
        let (mean, std) = mean_std(&self.fluxes);
        let threshold = 2.5f32.mul_add(std, mean).max(1e-5);
        self.fluxes.push(flux);
        if self.fluxes.len() > 64 {
            self.fluxes.remove(0);
        }

        let now = self.now();
        let clear_of_last = self.last_onset.is_none_or(|t| now - t >= MIN_GAP_SECS);
        if flux > threshold && clear_of_last {
            self.last_onset = Some(now);
            self.onsets.push(now);
            events.push(Event::Onset(now));
        }
        self.onsets.retain(|t| now - t <= WINDOW_SECS);

        if now - self.last_estimate_at >= 1.0 {
            self.last_estimate_at = now;
            if let Some(bpm) = estimate_tempo(&self.onsets) {
                let changed = self.tempo.is_none_or(|t| (t - bpm).abs() > 0.5);
                self.tempo = Some(bpm);
                if changed {
                    events.push(Event::Tempo(bpm));
                }
            }
        }
    }
}

fn mean_std(values: &[f32]) -> (f32, f32) {
    if values.is_empty() {
        return (0.0, 0.0);
    }
    let n = num::f32_of_usize(values.len());
    let mean = values.iter().sum::<f32>() / n;
    let var = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
    (mean, var.sqrt())
}

/// Folds a BPM into the range by octaves.
fn fold(mut bpm: f32) -> f32 {
    // `loop` + `break` rather than `while <float comparison>`: clippy's
    // `while_float` flags a float condition on a `while` regardless of
    // the operator, on the grounds that a `while` invites `==`/`!=`
    // elsewhere in the file; this loop only ever compares with `<`/`>`,
    // so restructuring it says that plainly instead of suppressing the
    // lint.
    loop {
        if bpm >= BPM_MIN {
            break;
        }
        bpm *= 2.0;
    }
    loop {
        if bpm <= BPM_MAX {
            break;
        }
        bpm /= 2.0;
    }
    bpm
}

/// The tempo the onsets imply, or `None` with too few to say.
///
/// Each onset is paired with the next few after it; a pair `k` onsets
/// apart is taken to span `k` beats, so its interval says the same
/// tempo the adjacent pairs do rather than a sub-harmonic of it — the
/// bin for 60 would otherwise collect every two-beat pair and outvote
/// 120. Folded into range and binned at one BPM; the fullest bin wins,
/// and its neighbours refine it to a weighted centre so 120 does not
/// read as 119 because of one hop.
pub fn estimate_tempo(onsets: &[f32]) -> Option<f32> {
    if onsets.len() < 4 {
        return None;
    }
    let mut bins = vec![0.0f32; num::usize_of_f32(BPM_MAX - BPM_MIN).saturating_add(2)];
    for (i, &a) in onsets.iter().enumerate() {
        for (k, &b) in onsets.iter().enumerate().skip(i.saturating_add(1)).take(4) {
            // `k` only ever reaches here from `skip(i + 1)`, so it is
            // always strictly greater than `i`; `saturating_sub` states
            // that instead of asserting it.
            let span = num::f32_of_usize(k.saturating_sub(i));
            let gap = b - a;
            if gap <= 0.0 || gap > 2.0 * span {
                continue;
            }
            let bpm = fold(60.0 * span / gap);
            let bin = num::usize_of_f32((bpm - BPM_MIN).round());
            if let Some(slot) = bins.get_mut(bin) {
                // The nearest pair is the most trustworthy; a span of
                // several is only a check on it.
                *slot += 1.0 / span;
            }
        }
    }
    let (best, weight) = bins.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    if *weight <= 0.0 {
        return None;
    }
    let lo = best.saturating_sub(1);
    let hi = best.saturating_add(1).min(bins.len().saturating_sub(1));
    let mut weighted = 0.0;
    let mut den = 0.0;
    for (i, w) in bins.iter().enumerate().take(hi.saturating_add(1)).skip(lo) {
        weighted += w * (BPM_MIN + num::f32_of_usize(i));
        den += w;
    }
    Some(weighted / den)
}

/// Starts capture on the default input, if this build has one.
pub fn start(tx: Sender) {
    capture::start(tx);
}

#[cfg(feature = "sound")]
mod capture {
    use super::{Detector, Event};
    use crate::command::{Command, Sender};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    /// Opens the default input device and runs the detector in its
    /// callback. Levels go out every few hops, a tempo when it moves.
    /// The stream has to be kept alive, so the thread parks on it.
    pub fn start(tx: Sender) {
        if std::env::var("IGNITION_SOUND").is_ok_and(|v| v == "0") {
            tracing::info!("sound: disabled by IGNITION_SOUND=0");
            return;
        }
        std::thread::Builder::new()
            .name("sound-in".into())
            .spawn(move || {
                let host = cpal::default_host();
                let Some(device) = host.default_input_device() else {
                    tracing::warn!("sound: no input device; no beat from the room");
                    return;
                };
                let config = match device.default_input_config() {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(error = %e, "sound: input has no usable config");
                        return;
                    }
                };
                let channels = config.channels() as usize;
                let rate = config.sample_rate() as f32;
                let mut detector = Detector::new(rate);
                let mut mono = Vec::new();
                let mut since_levels = 0usize;
                let stream = device.build_input_stream(
                    config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        mono.clear();
                        mono.extend(
                            data.chunks(channels.max(1))
                                .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32),
                        );
                        for event in detector.push(&mono) {
                            if let Event::Tempo(bpm) = event {
                                tracing::info!(bpm = format!("{bpm:.1}"), "sound: tempo");
                                let _ = tx.send(Command::Tap(bpm));
                            }
                        }
                        since_levels += mono.len();
                        // ~30 Hz, whatever the buffer size.
                        if since_levels as f32 >= rate / 30.0 {
                            since_levels = 0;
                            let b = detector.bands;
                            let _ = tx.send(Command::SoundLevels {
                                low: b.low,
                                mid: b.mid,
                                high: b.high,
                            });
                        }
                    },
                    |e| tracing::warn!(error = %e, "sound: stream error"),
                    None,
                );
                match stream {
                    Ok(stream) => {
                        if let Err(e) = stream.play() {
                            tracing::warn!(error = %e, "sound: cannot start capture");
                            return;
                        }
                        tracing::info!(
                            device = ?device.description().ok().map(|d| d.name().to_string()),
                            rate,
                            channels,
                            "sound: listening"
                        );
                        loop {
                            std::thread::park();
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "sound: cannot open input"),
                }
            })
            .ok();
    }
}

#[cfg(not(feature = "sound"))]
mod capture {
    use crate::command::Sender;

    pub fn start(_tx: Sender) {
        tracing::debug!("sound: this build has no `sound` feature");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 44_100.0;

    /// A click track: a short decaying burst every beat, silence
    /// between. Deterministic — no noise — so a failure is the
    /// detector's, not the dice's.
    fn click_track(bpm: f32, secs: f32) -> Vec<f32> {
        let n = num::usize_of_f32(RATE * secs);
        let period = num::usize_of_f32(RATE * 60.0 / bpm);
        let click_len = num::usize_of_f32(RATE * 0.02);
        (0..n)
            .map(|i| {
                // `period` is built from a fixed, non-zero test `bpm`
                // above, but `checked_rem` says that instead of trusting
                // it silently the way a bare `%` would.
                let since = i.checked_rem(period).unwrap_or(0);
                if since < click_len {
                    let t = num::f32_of_usize(since) / RATE;
                    let env = (-t * 200.0).exp();
                    // A 1 kHz tone under the envelope; the sign
                    // alternation keeps it from being a DC step.
                    env * (2.0 * std::f32::consts::PI * 1000.0 * t).sin() * 0.8
                } else {
                    0.0
                }
            })
            .collect()
    }

    /// r[verify playback.sound-in]
    #[test]
    fn a_click_track_at_120_is_heard_at_120() {
        let mut detector = Detector::new(RATE);
        let audio = click_track(120.0, 8.0);
        let mut onsets = 0;
        let mut last_tempo = None;
        for chunk in audio.chunks(1024) {
            for event in detector.push(chunk) {
                match event {
                    Event::Onset(_) => onsets += 1,
                    Event::Tempo(bpm) => last_tempo = Some(bpm),
                }
            }
        }
        // Sixteen clicks in eight seconds; allow the first to be eaten
        // by an empty baseline.
        assert!((14..=16).contains(&onsets), "{onsets} onsets");
        let bpm = last_tempo.expect("a tempo was estimated");
        assert!((bpm - 120.0).abs() <= 2.0, "{bpm} BPM");
    }

    #[test]
    fn a_faster_track_folds_into_range_and_silence_says_nothing() {
        let mut detector = Detector::new(RATE);
        for chunk in click_track(90.0, 8.0).chunks(1024) {
            detector.push(chunk);
        }
        let bpm = detector.tempo.expect("tempo");
        assert!((bpm - 90.0).abs() <= 2.0, "{bpm}");

        let mut quiet = Detector::new(RATE);
        let silence = vec![0.0f32; num::usize_of_f32(RATE * 4.0)];
        let events = quiet.push(&silence);
        assert!(events.is_empty(), "{events:?}");
        assert_eq!(quiet.tempo, None);
    }

    #[test]
    fn the_bands_split_where_the_energy_is() {
        let tone = |hz: f32| -> Vec<f32> {
            (0..num::usize_of_f32(RATE))
                .map(|i| {
                    (2.0 * std::f32::consts::PI * hz * num::f32_of_usize(i) / RATE).sin() * 0.5
                })
                .collect()
        };
        let mut d = Detector::new(RATE);
        d.push(&tone(60.0));
        assert!(
            d.bands.low > d.bands.mid && d.bands.low > d.bands.high,
            "{:?}",
            d.bands
        );
        let mut d = Detector::new(RATE);
        d.push(&tone(6000.0));
        assert!(d.bands.high > d.bands.low, "{:?}", d.bands);
        let mut d = Detector::new(RATE);
        d.push(&tone(700.0));
        assert!(
            d.bands.mid > d.bands.low && d.bands.mid > d.bands.high,
            "{:?}",
            d.bands
        );
    }

    #[test]
    fn tempo_needs_enough_onsets_and_folds_octaves() {
        assert_eq!(estimate_tempo(&[0.0, 0.5]), None);
        let beats: Vec<f32> = (0..12).map(|i| num::f32_of_i32(i) * 0.5).collect();
        let bpm = estimate_tempo(&beats).unwrap();
        assert!((bpm - 120.0).abs() < 1.0, "{bpm}");
        // Sixteenths at 120 are 480 BPM and fold to 120.
        let fast: Vec<f32> = (0..48).map(|i| num::f32_of_i32(i) * 0.125).collect();
        let bpm = estimate_tempo(&fast).unwrap();
        assert!((bpm - 120.0).abs() < 1.0, "{bpm}");
    }
}
