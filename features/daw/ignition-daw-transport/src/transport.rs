//! Playing the project, and telling the lights where it is.
//!
//! The DAW backend is embedded rather than talked to over a wire: the
//! project *is* the show, and a console that has to ask another process
//! where the music got to has already added a hop that can drift. This
//! owns a `Standalone`, its cpal output stream, and the tempo map that
//! turns its playhead into bars.
//!
//! What it deliberately does **not** own is the cue player. This
//! reports a position; whether anything follows it is the caller's
//! business, which is what keeps "manually runnable" true — unplug the
//! transport and the same cue list still steps on GO.
//!
//! The DAW is one [`TransportSource`] among several. Timecode — LTC,
//! MTC, Art-Net — lives in [`crate::timecode`]; a [`TapClock`] runs a
//! list with no transport at all. [`SourceTransport`] takes any of them
//! and does the one thing they share: seconds through the tempo map to
//! bars, holding still when the source is lost.

use ignition_daw_proto::{Bars, SongMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(feature = "play")]
use anyhow::{Context, Result};
#[cfg(feature = "play")]
use daw::standalone::media_bay::ProjectRelativeResolver;
#[cfg(feature = "play")]
use daw::standalone::project_loader::load_rpp_via_bay;
#[cfg(feature = "play")]
use daw::standalone::sync::Standalone;
#[cfg(feature = "play")]
use daw_proto::ProjectContext;
#[cfg(feature = "play")]
use daw_proto::transport::service::Transport as TransportService;
#[cfg(feature = "play")]
use std::path::{Path, PathBuf};

/// Where a musical position comes from, in seconds.
///
/// The engine wants a `Bars` per frame and does not care who computed
/// it; this is the seam. A source answers three questions — where, is
/// it moving, and has it gone away — and [`SourceTransport`] does the
/// rest. `seconds()` is `None` until the source has said anything.
// r[impl song.transport.sources] - one interface for the DAW, timecode and a tap clock
// r[impl song.transport.position-per-frame] - the engine is handed a position, not a source
pub trait TransportSource: Send {
    fn seconds(&self) -> Option<f64>;
    fn is_playing(&self) -> bool;
    /// Had a position, and lost it — the timecode stopped, the port
    /// went away. The player holds rather than running free.
    fn lost(&self) -> bool;
}

/// A song clocked by any [`TransportSource`].
///
/// Holds the last good position while the source is lost, rather than
/// letting it run on or snapping to the top: a band whose LTC drops for
/// a second wants the lights to stay where they were, not to restart
/// the song.
pub struct SourceTransport {
    source: Box<dyn TransportSource>,
    song: SongMap,
    held: Mutex<Option<f64>>,
}

impl SourceTransport {
    // r[impl song.transport.sources] - seconds mapped through the tempo map to bars
    pub fn from_source(source: impl TransportSource + 'static, song: SongMap) -> Self {
        Self {
            source: Box::new(source),
            song,
            held: Mutex::new(None),
        }
    }

    pub const fn song(&self) -> &SongMap {
        &self.song
    }

    pub fn source(&self) -> &dyn TransportSource {
        self.source.as_ref()
    }

    /// Playing, as far as the cue player is concerned: the source is
    /// moving and has not been lost. A stopped or lost transport fires
    /// nothing.
    // r[impl song.transport.stopped-fires-nothing]
    pub fn is_playing(&self) -> bool {
        self.source.is_playing() && !self.source.lost()
    }

    /// The playhead in seconds — the source's own, or the last good one
    /// while it is lost.
    // r[impl song.transport.sources] - holds, does not run free, when lost
    pub fn seconds(&self) -> f64 {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.source.lost()
            && let Some(s) = self.source.seconds()
        {
            *held = Some(s);
        }
        held.unwrap_or(0.0)
    }

    /// The playhead, in bars and beats.
    // r[impl song.transport.position-per-frame]
    pub fn position(&self) -> Bars {
        self.song.tempo.position_at(self.seconds())
    }
}

/// A clock that runs on its own at a tapped tempo.
///
/// With no transport at all a list still has to run on time — cue
/// follows against the `Tap` master. This is the `Tap` master as a
/// position: it starts at bar 1 when started, advances at its bpm, and
/// re-times itself from taps. It is never lost; nothing feeds it.
// r[impl song.transport.follow] - a position with no transport, from the Tap master
#[derive(Debug)]
pub struct TapClock {
    inner: Mutex<TapState>,
}

#[derive(Debug, Clone)]
struct TapState {
    bpm: f64,
    beats_per_bar: f64,
    /// Beats elapsed up to `since`; the tempo may have changed there.
    beats: f64,
    since: Instant,
    running: bool,
    taps: Vec<Instant>,
}

/// Taps further apart than this start a new tempo rather than average
/// into the old one.
const TAP_RESET: Duration = Duration::from_secs(2);

impl TapClock {
    /// A stopped clock at `bpm`, positioned at bar 1.
    #[must_use]
    pub const fn new(bpm: f64, now: Instant) -> Self {
        Self {
            inner: Mutex::new(TapState {
                bpm: bpm.max(1.0),
                beats_per_bar: 4.0,
                beats: 0.0,
                since: now,
                running: false,
                taps: Vec::new(),
            }),
        }
    }

    /// A clock that takes its signature from the song.
    #[must_use]
    pub fn for_song(bpm: f64, song: &SongMap, now: Instant) -> Self {
        let clock = Self::new(bpm, now);
        clock.lock().beats_per_bar =
            f64::from(song.tempo.at(Bars::START).time_signature.numerator.max(1));
        clock
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TapState> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn start(&self, now: Instant) {
        let mut s = self.lock();
        s.freeze(now);
        s.running = true;
    }

    pub fn stop(&self, now: Instant) {
        let mut s = self.lock();
        s.freeze(now);
        s.running = false;
    }

    pub fn is_running(&self) -> bool {
        self.lock().running
    }

    pub fn bpm(&self) -> f64 {
        self.lock().bpm
    }

    /// Sets the tempo outright, without disturbing the position.
    pub fn set_bpm(&self, bpm: f64, now: Instant) {
        let mut s = self.lock();
        s.freeze(now);
        s.bpm = bpm.max(1.0);
    }

    /// A tap. Two or more within two seconds of each other set the
    /// tempo to their average interval; the first after a gap only
    /// starts counting.
    pub fn tap(&self, now: Instant) {
        let mut s = self.lock();
        if s.taps
            .last()
            .is_some_and(|last| now.saturating_duration_since(*last) > TAP_RESET)
        {
            s.taps.clear();
        }
        s.taps.push(now);
        if s.taps.len() > 8 {
            s.taps.remove(0);
        }
        let taps_len = s.taps.len();
        if let (Some(&first), Some(&last)) = (s.taps.first(), s.taps.last())
            && taps_len >= 2
        {
            let span = last.saturating_duration_since(first).as_secs_f64();
            // `taps_len >= 2` above, so the intervals count is at least
            // one; the fallback below is never actually hit.
            let intervals = u32::try_from(taps_len.saturating_sub(1)).unwrap_or(1);
            let interval = span / f64::from(intervals);
            if interval > 0.0 {
                s.freeze(now);
                s.bpm = 60.0 / interval;
            }
        }
    }

    /// Beats elapsed since the clock was started.
    pub fn beats_at(&self, now: Instant) -> f64 {
        self.lock().beats_at(now)
    }

    /// Seconds elapsed at the current tempo — what the transport maps
    /// through a song's tempo map. The clock's own bars are the truth;
    /// this is beats at the tap tempo, so a `SourceTransport` over a
    /// song at the same tempo agrees with `position_at`.
    pub fn seconds_at(&self, now: Instant) -> f64 {
        let s = self.lock();
        s.beats_at(now) * 60.0 / s.bpm
    }

    /// The position, counted from bar 1 at the start.
    #[must_use]
    pub fn position_at(&self, now: Instant) -> Bars {
        let s = self.lock();
        let beats = s.beats_at(now);
        let bar = (beats / s.beats_per_bar).floor();
        Bars::new(
            bar_number(bar).saturating_add(1),
            bar.mul_add(-s.beats_per_bar, beats) + 1.0,
        )
    }
}

/// A bar number floored from a beat count that only ever grows forward
/// from zero. `bar` is always non-negative and, for anything a real
/// session runs to, nowhere near `u32::MAX`, so this cast cannot lose a
/// sign or wrap the count backwards.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "bar is a non-negative floor of a forward-only clock; see the doc comment"
)]
const fn bar_number(bar: f64) -> u32 {
    bar as u32
}

impl TapState {
    fn beats_at(&self, now: Instant) -> f64 {
        if !self.running {
            return self.beats;
        }
        self.beats + now.saturating_duration_since(self.since).as_secs_f64() * self.bpm / 60.0
    }

    /// Banks the beats so far, so a tempo change applies from here on.
    fn freeze(&mut self, now: Instant) {
        self.beats = self.beats_at(now);
        self.since = now;
    }
}

// r[impl song.transport.follow] - usable wherever a transport is
impl TransportSource for TapClock {
    fn seconds(&self) -> Option<f64> {
        Some(self.seconds_at(Instant::now()))
    }
    fn is_playing(&self) -> bool {
        self.is_running()
    }
    fn lost(&self) -> bool {
        false
    }
}

/// Output buffer to ask cpal for, in frames.
///
/// The backend's playhead advances once per audio callback, so at the
/// device default (often 1024 or more at 48 kHz, ~21 ms) it ticks at
/// well under the frame rate. 256 is ~5 ms: fine enough that the
/// extrapolation in [`PlayheadClock`] rarely has to reach far, and
/// still comfortably safe on a stock desktop stack.
#[cfg(feature = "play")]
const OUTPUT_BUFFER_FRAMES: u32 = 256;

/// Furthest the clock will run ahead of the last value the audio
/// callback wrote. A stalled stream (device unplugged, xrun storm)
/// should read as *stuck*, not as a playhead sailing off on its own.
const MAX_EXTRAPOLATION_SECS: f64 = 0.25;

/// Smooths the audio callback's stepwise playhead into a per-frame one.
///
/// The backend reports the position the last callback got to, and
/// nothing between callbacks — read every frame it is a staircase, and
/// a moving head following it steps at the callback rate. Between two
/// reports the song is still playing at 1 s/s, so this adds the wall
/// time since the last step. It re-syncs the instant the raw value
/// changes (which also covers a locate, where the raw value jumps), and
/// while paused simply returns what the backend says.
#[derive(Debug, Clone)]
pub struct PlayheadClock {
    last_raw: f64,
    since: Instant,
    /// The last value handed out, so a re-sync to a raw value that is
    /// *behind* the extrapolated one (the callback landed a shade later
    /// than we guessed) does not show as the song ticking backwards.
    last_out: f64,
}

impl PlayheadClock {
    #[must_use]
    pub const fn new(now: Instant) -> Self {
        Self {
            last_raw: 0.0,
            since: now,
            last_out: 0.0,
        }
    }

    /// The position to report for a raw reading `raw` taken at `now`.
    // The backend hands back the exact `f64` it last wrote, unchanged
    // until the next callback; `!=` here is "did a new step arrive",
    // not a comparison of two independently computed floats, so an
    // epsilon would only make a genuine step go undetected.
    #[expect(
        clippy::float_cmp,
        reason = "raw is the backend's own unmodified step value; see the comment above"
    )]
    pub fn observe(&mut self, raw: f64, playing: bool, now: Instant) -> f64 {
        if raw != self.last_raw {
            // A genuine step from the callback, or a locate. A backward
            // jump larger than one buffer's worth is a locate and must
            // be honoured; a smaller one is the callback landing behind
            // our guess, and is held rather than shown.
            let moved_back = raw < self.last_out;
            let small = self.last_out - raw < MAX_EXTRAPOLATION_SECS;
            self.last_raw = raw;
            self.since = now;
            self.last_out = if playing && moved_back && small {
                self.last_out
            } else {
                raw
            };
            return self.last_out;
        }
        if !playing {
            self.last_out = raw;
            return raw;
        }
        let elapsed = now
            .saturating_duration_since(self.since)
            .as_secs_f64()
            .min(MAX_EXTRAPOLATION_SECS);
        let out = raw + elapsed;
        self.last_out = out.max(self.last_out);
        self.last_out
    }
}

/// A loaded project, playing or ready to.
#[cfg(feature = "play")]
pub struct SongTransport {
    daw: Standalone,
    ctx: ProjectContext,
    /// The cpal output stream. Dropping it stops the audio, so it is
    /// held even though nothing calls it — hence the name rather than an
    /// `_engine` that reads like an oversight.
    _output: daw::standalone::audio_engine::AudioEngine,
    song: SongMap,
    /// Behind a mutex only because every reader takes `&self`; it is
    /// touched once per frame from one thread and never contended.
    clock: Mutex<PlayheadClock>,
}

#[cfg(feature = "play")]
impl SongTransport {
    /// Loads a project, decodes its audio and opens an output stream.
    ///
    /// The song map comes from the same file in the same pass, so the
    /// tempo the lights convert with is by construction the tempo the
    /// audio is playing at.
    ///
    /// **Must be called with a Tokio runtime current.** The backend's
    /// service layer spawns tasks, and without one this panics inside
    /// architect rather than returning an error — so a caller that is
    /// not already async needs `rt.enter()` held for the lifetime of
    /// the transport, not just this call.
    ///
    /// # Errors
    ///
    /// Fails if the project file cannot be read or parsed, if its audio
    /// fails to load, if the output stream cannot be opened, or if the
    /// project does not decode into a valid [`SongMap`].
    // r[impl song.transport.position-per-frame] - tempo map from the same project in the same pass as the audio
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let name = path.file_stem().map_or_else(
            || "project".to_string(),
            |s| s.to_string_lossy().into_owned(),
        );
        let dir = path
            .parent()
            .map_or_else(|| PathBuf::from("."), PathBuf::from);

        let daw = Standalone::new();
        // Source paths in a project are relative to it. Without this the
        // project loads, reports zero decoded sources, and plays
        // silence — which looks like an audio-device problem and is not.
        daw.media_bay()
            .set_file_resolver(Box::new(ProjectRelativeResolver::new(dir)));

        let (project, audio) =
            load_rpp_via_bay(&daw, &name, path.to_string_lossy().as_ref(), &text)
                .map_err(|e| anyhow::anyhow!("loading {}: {e}", path.display()))?;
        for (take, err) in &audio.failed {
            tracing::warn!(take, error = %err, "song: a source failed to decode");
        }
        tracing::info!(
            name = %name,
            tracks = project.track_count,
            items = project.item_count,
            decoded = audio.loaded,
            failed = audio.failed.len(),
            "song: project loaded"
        );

        let ctx = ProjectContext::Project(project.project_guid.clone());
        let output = open_output(&daw, &project.project_guid)
            .map_err(|e| anyhow::anyhow!("opening the audio output: {e}"))?;
        let song = ignition_daw_reaper::from_rpp(&text, &name)?;

        Ok(Self {
            daw,
            ctx,
            _output: output,
            song,
            clock: Mutex::new(PlayheadClock::new(Instant::now())),
        })
    }

    pub const fn song(&self) -> &SongMap {
        &self.song
    }

    pub fn play(&self) {
        if let Err(e) = TransportService::play(&self.daw, self.ctx.clone()) {
            tracing::warn!(error = ?e, "song: play failed");
        }
    }

    pub fn stop(&self) {
        if let Err(e) = TransportService::stop(&self.daw, self.ctx.clone()) {
            tracing::warn!(error = ?e, "song: stop failed");
        }
    }

    pub fn is_playing(&self) -> bool {
        use daw_proto::transport::PlayState;
        matches!(
            TransportService::get_play_state(&self.daw, self.ctx.clone()),
            PlayState::Playing | PlayState::Recording
        )
    }

    /// The playhead, in seconds — smoothed between audio callbacks, see
    /// [`PlayheadClock`]. `raw_seconds` is the backend's own value.
    // r[impl song.transport.position-per-frame] - a per-frame position, not a per-buffer one
    pub fn seconds(&self) -> f64 {
        let raw = self.raw_seconds();
        let playing = self.is_playing();
        self.clock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observe(raw, playing, Instant::now())
    }

    /// The playhead as the backend last reported it, stepping once per
    /// audio callback.
    pub fn raw_seconds(&self) -> f64 {
        TransportService::get_position(&self.daw, self.ctx.clone())
    }

    /// The playhead, in bars and beats.
    ///
    /// This is the whole point of the module: the lights are told
    /// *where in the song* the music is, not how many seconds have
    /// passed, so a tempo change or a section edit moves them with it.
    // r[impl song.transport.position-per-frame]
    pub fn position(&self) -> Bars {
        self.song.tempo.position_at(self.seconds())
    }

    /// Moves the playhead to a musical position — how a section is
    /// looped, and how "start from the last chorus" works.
    // r[impl song.transport.seek-is-a-locate] - the transport side: a locate moves the playhead only
    pub fn locate(&self, position: Bars) {
        let seconds = self.song.tempo.seconds_at(position);
        if let Err(e) = TransportService::set_position(&self.daw, self.ctx.clone(), seconds) {
            tracing::warn!(error = ?e, "song: locate failed");
        }
    }

    /// How long the song runs, in seconds.
    ///
    /// From the last section's end rather than the decoded audio's
    /// length, so it agrees with the timeline the cues are written
    /// against. A project whose audio runs past its final section — a
    /// tail, a stray region — would otherwise give a progress bar that
    /// never quite reaches the end.
    pub fn length(&self) -> f64 {
        self.song
            .sections
            .last()
            .map(|s| self.song.tempo.seconds_at(s.end(&self.song.tempo)))
            .unwrap_or_default()
    }

    /// Moves the playhead to a fraction of the song, 0..=1 — a scrub.
    ///
    /// Seconds rather than bars deliberately: dragging a bar is a
    /// gesture about *time*, and snapping it to the nearest musical
    /// position would make the handle refuse to sit where it was
    /// dropped.
    pub fn scrub(&self, fraction: f32) {
        let seconds = self.length() * f64::from(fraction.clamp(0.0, 1.0));
        if let Err(e) = TransportService::set_position(&self.daw, self.ctx.clone(), seconds) {
            tracing::warn!(error = ?e, "song: scrub failed");
        }
    }

    /// Moves the playhead to the start of a named section.
    pub fn locate_section(&self, name: &str) -> bool {
        self.song
            .section(name)
            .map(|s| s.start)
            .is_some_and(|start| {
                self.locate(start);
                true
            })
    }
}

/// The embedded DAW as a [`TransportSource`]. Never lost: the audio
/// engine is in-process, and a stalled stream reads as *stuck* through
/// [`PlayheadClock`] rather than as a lost signal.
#[cfg(feature = "play")]
pub struct DawSource(pub SongTransport);

#[cfg(feature = "play")]
impl TransportSource for DawSource {
    fn seconds(&self) -> Option<f64> {
        Some(self.0.seconds())
    }
    fn is_playing(&self) -> bool {
        self.0.is_playing()
    }
    fn lost(&self) -> bool {
        false
    }
}

#[cfg(feature = "play")]
impl SongTransport {
    /// This transport as a generic source — for a player written
    /// against [`SourceTransport`] that wants the DAW today and LTC
    /// tomorrow.
    pub fn into_source(self) -> SourceTransport {
        let song = self.song.clone();
        SourceTransport::from_source(DawSource(self), song)
    }
}

/// What `Standalone::attach_audio_engine` does, with one difference: the
/// output stream is asked for a small buffer. The backend's own helper
/// takes `AudioIoPrefs::default()`, which is the device default, and
/// the playhead only advances once per callback — so the buffer size
/// is the resolution of the clock the lights follow. Falls back to the
/// backend's default if the device refuses the request.
#[cfg(feature = "play")]
fn open_output(
    daw: &Standalone,
    guid: &str,
) -> std::result::Result<daw::standalone::audio_engine::AudioEngine, String> {
    use daw::standalone::audio_engine::AudioEngine;
    let bundle = daw.transport_engine_for(guid);
    // The callback is the clock from here on; the soft clock would
    // otherwise fight it.
    bundle.disable_soft_clock();
    let track_count = daw.read_project(guid, |p| p.tracks.len()).unwrap_or(0);
    daw.set_meters(daw::standalone::metering::Meters::new(track_count));
    let prefs = daw_audio_io::AudioIoPrefs {
        buffer_size: OUTPUT_BUFFER_FRAMES,
        ..Default::default()
    };
    match AudioEngine::with_project_prefs(
        daw.clone(),
        guid.to_string(),
        bundle.shared.clone(),
        &prefs,
    ) {
        Ok(engine) => Ok(engine),
        Err(e) => {
            tracing::warn!(
                error = %e,
                frames = OUTPUT_BUFFER_FRAMES,
                "song: small output buffer refused, using the device default"
            );
            AudioEngine::with_project(daw.clone(), guid.to_string(), bundle.shared.clone())
        }
    }
}

#[cfg(test)]
mod clock_tests {
    use super::*;
    use std::time::Duration;

    fn at(t0: Instant, ms: u64) -> Instant {
        // `checked_add` over a bare `+`: an `Instant` addition can in
        // principle overflow, and the milliseconds here are always small
        // test offsets, so the fallback to `t0` never actually triggers.
        t0.checked_add(Duration::from_millis(ms)).unwrap_or(t0)
    }

    #[test]
    /// r[verify song.transport.position-per-frame]
    fn a_playing_clock_advances_between_callbacks_and_resyncs_on_each() {
        let t0 = Instant::now();
        let mut clock = PlayheadClock::new(t0);
        assert!((clock.observe(1.000, true, at(t0, 0)) - 1.000).abs() < 1e-9);
        // Same raw value 8 ms later: extrapolated.
        let a = clock.observe(1.000, true, at(t0, 8));
        assert!((a - 1.008).abs() < 1e-6, "{a}");
        // The callback lands at 1.021 (a 21 ms buffer): re-synced.
        let b = clock.observe(1.021, true, at(t0, 21));
        assert!((b - 1.021).abs() < 1e-9);
        // Never backwards: our guess got ahead of a late callback.
        let c = clock.observe(1.021, true, at(t0, 40));
        let d = clock.observe(1.035, true, at(t0, 41));
        assert!(d >= c, "{d} < {c}");
    }

    #[test]
    fn a_paused_clock_reports_the_raw_value_and_a_stall_is_capped() {
        let t0 = Instant::now();
        let mut clock = PlayheadClock::new(t0);
        assert!((clock.observe(5.0, false, at(t0, 0)) - 5.0).abs() < 1e-9);
        assert!((clock.observe(5.0, false, at(t0, 500)) - 5.0).abs() < 1e-9);
        // Playing but the callback never moves: run ahead only so far.
        clock.observe(5.0, true, at(t0, 1000));
        let stuck = clock.observe(5.0, true, at(t0, 5000));
        assert!(
            (stuck - (5.0 + MAX_EXTRAPOLATION_SECS)).abs() < 1e-6,
            "{stuck}"
        );
    }

    #[test]
    fn a_locate_backwards_is_honoured() {
        let t0 = Instant::now();
        let mut clock = PlayheadClock::new(t0);
        clock.observe(30.0, true, at(t0, 0));
        clock.observe(30.0, true, at(t0, 10));
        assert!((clock.observe(4.0, true, at(t0, 11)) - 4.0).abs() < 1e-9);
    }

    /// A source scripted by the test.
    struct Scripted(std::sync::Mutex<(Option<f64>, bool, bool)>);
    impl Scripted {
        fn set(&self, seconds: Option<f64>, playing: bool, lost: bool) {
            *self.0.lock().unwrap() = (seconds, playing, lost);
        }
    }
    impl TransportSource for &'static Scripted {
        fn seconds(&self) -> Option<f64> {
            self.0.lock().unwrap().0
        }
        fn is_playing(&self) -> bool {
            self.0.lock().unwrap().1
        }
        fn lost(&self) -> bool {
            self.0.lock().unwrap().2
        }
    }

    /// r[verify song.transport.sources] - seconds become bars; a lost source holds
    /// r[verify song.transport.stopped-fires-nothing]
    #[test]
    fn a_source_transport_maps_to_bars_and_holds_when_lost() {
        let source: &'static Scripted =
            Box::leak(Box::new(Scripted(Mutex::new((None, false, false)))));
        let song = SongMap {
            name: "t".into(),
            tempo: ignition_daw_proto::TempoMap::constant(
                120.0,
                ignition_daw_proto::TimeSignature::default(),
            ),
            sections: Vec::new(),
        };
        let t = SourceTransport::from_source(source, song);
        // Nothing yet: the top.
        assert_eq!(t.position(), Bars::START);
        assert!(!t.is_playing());
        // Two seconds at 120 is one bar.
        source.set(Some(2.0), true, false);
        assert_eq!(t.position(), Bars::bar(2));
        assert!(t.is_playing());
        // The signal goes away and its last value is stale garbage: the
        // position holds where it was, and nothing fires.
        source.set(Some(0.0), false, true);
        assert_eq!(t.position(), Bars::bar(2));
        assert!(!t.is_playing());
        // It comes back further on: followed again.
        source.set(Some(4.0), true, false);
        assert_eq!(t.position(), Bars::bar(3));
    }

    /// r[verify song.transport.follow]
    #[test]
    fn a_tap_clock_advances_on_its_own_and_retimes_from_taps() {
        let t0 = Instant::now();
        let clock = TapClock::new(120.0, t0);
        // Stopped: it sits at the top.
        assert_eq!(clock.position_at(at(t0, 5000)), Bars::START);
        clock.start(at(t0, 0));
        // 120 bpm: a beat every 500 ms, a bar every 2 s.
        assert_eq!(clock.position_at(at(t0, 500)), Bars::new(1, 2.0));
        assert_eq!(clock.position_at(at(t0, 2000)), Bars::bar(2));
        assert!((clock.seconds_at(at(t0, 2000)) - 2.0).abs() < 1e-9);
        // Four taps a second apart: 60 bpm from here on, position kept.
        for i in 0..4 {
            clock.tap(at(t0, 2000 + i * 1000));
        }
        assert!((clock.bpm() - 60.0).abs() < 1e-9, "{}", clock.bpm());
        // 5 s in: 6 beats at 120 (3 s, the tempo only changes at the
        // second tap) + 2 beats at 60 (2 s) = 8 beats: bar 3.
        assert_eq!(clock.position_at(at(t0, 5000)), Bars::new(3, 1.0));
        // Stop, wait, start: no time passes while stopped.
        clock.stop(at(t0, 5000));
        assert_eq!(clock.position_at(at(t0, 9000)), Bars::new(3, 1.0));
        clock.start(at(t0, 9000));
        assert_eq!(clock.position_at(at(t0, 10000)), Bars::new(3, 2.0));
        // ...and it is a source like any other, never lost.
        assert!(!TransportSource::lost(&clock));
        assert!(TransportSource::is_playing(&clock));
    }
}
