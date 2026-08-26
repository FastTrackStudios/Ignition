//! Moving pictures for the screen canvases.
//!
//! `canvas.rs` already answers "which piece of the source does this panel
//! show". This module answers the other half — "what is the source
//! showing *now*" — and deliberately nothing else: it hands out RGBA
//! frames and has never heard of Bevy, the venue, or a cue. `spawn.rs`
//! is the only place that turns a frame into a texture upload.
//!
//! Three things shape the design.
//!
//! **The clock is supplied, not free-running.** [`VideoSource::frame_at`]
//! takes the time to present, in seconds. A media server that runs its
//! clips off their own wall clock cannot be scrubbed with the song, and
//! scrubbing is the whole point here: drag the progress bar and the
//! graphics have to move with the music, backwards included. So nothing
//! in here reads a clock; the caller owns it. (See `spawn.rs`'s
//! `CanvasClock` for how that reaches the ECS, and the studio for where
//! the number comes from.)
//!
//! **Decoding never touches the render thread.** Each source owns a
//! worker thread that decodes ahead into a small bounded queue; the
//! render thread only ever drains what has already arrived. A late or
//! dropped frame just means the last one stays up for another 16 ms,
//! which nobody in the room can see. A *stalled* frame is a dropped
//! visualizer frame, which everybody can.
//!
//! **Two backends, both optional.** Feature `hap` is a pure-Rust reader
//! for Vidvox HAP (`.mov`) — the codec Resolume, VDMX and the rest of
//! this world actually run, all-intra so a scrub is exact, and no system
//! library to install. Feature `ffmpeg` links libav* and covers
//! everything else (h.264 mp4, WebM). With neither on, this module still
//! compiles and every canvas is a still, exactly as before.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub mod export;

#[cfg(feature = "ffmpeg")]
mod ffmpeg;
#[cfg(feature = "hap")]
mod hap;

/// How many decoded frames may sit between the worker and the renderer.
///
/// Small on purpose. A 1080p RGBA frame is 8 MB, so this is a memory
/// budget as much as a latency one — and a deep queue is worse for a
/// scrub anyway, since every frame in it is work that has to be thrown
/// away the moment the operator drags the playhead.
const QUEUE_DEPTH: usize = 3;

/// How far the clock may fall behind the frame on screen before the
/// decoder is told to seek rather than to keep grinding forward.
///
/// Only consulted when the decoder has nothing queued ahead of the
/// requested time — if the next frame is already in hand, the frame on
/// screen is correct however wide the gap looks.
const LATE_TOLERANCE: f64 = 0.5;

/// Slack on a backwards jump, so ordinary float noise on a clock that is
/// standing still is not read as a scrub.
const BACK_TOLERANCE: f64 = 1.0 / 240.0;

#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error(
        "no video backend is compiled in for {0} — build with --features hap or --features ffmpeg"
    )]
    NoBackend(String),
    #[error("{0}")]
    Backend(String),
    #[error("the clip has no frames")]
    Empty,
}

/// What a canvas source path is, decided by its extension alone.
///
/// Extension rather than sniffing, and no new syntax in
/// `--canvas main=...`: an operator writing a config should not have to
/// declare what a `.mov` is, and every file that reaches here came off a
/// disk where the extension is already the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    /// A single image, loaded through Bevy's asset server as before.
    Still,
    /// A clip, decoded by this module.
    Video,
    /// Generated content — a `proc:` string naming a
    /// `ignition_core::canvas::CanvasRecipe`, rendered by
    /// `crate::canvas::ProceduralSource` on the same clock as a clip.
    // r[impl canvas.clip-is-a-source] - a clip and a procedural source are two kinds of the same thing
    Procedural,
}

/// Classifies a canvas source by extension.
///
/// Anything unrecognised is a still: the asset server will report a real
/// error for a file it cannot load, which is a better failure than this
/// module opening a decoder on a `.txt` and reporting something vaguer.
pub fn content_kind(path: &str) -> ContentKind {
    if path.starts_with(crate::canvas::PROC_PREFIX) {
        return ContentKind::Procedural;
    }
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "mov" | "mp4" | "m4v" | "webm" | "mkv" | "avi" => ContentKind::Video,
        _ => ContentKind::Still,
    }
}

/// Where a supplied time lands inside a clip that is shorter than the
/// song.
///
/// `rem_euclid` rather than `%` because the caller's clock can go
/// negative — a count-in before bar 1 is a negative song position, and
/// `%` would hand back a negative time that no frame index matches.
pub fn wrap_time(secs: f64, duration: f64) -> f64 {
    if !duration.is_finite() || duration <= 0.0 || !secs.is_finite() {
        return 0.0;
    }
    secs.rem_euclid(duration)
}

/// One decoded frame on its way from the worker to the renderer.
pub(crate) struct Frame {
    /// Presentation time in seconds from the start of the clip.
    pts: f64,
    /// Tightly packed RGBA8, `width * height * 4` bytes.
    rgba: Vec<u8>,
    /// Which seek this frame belongs to — see [`SeekRequest`].
    generation: u64,
}

/// The renderer's standing request to the worker: "be at this time".
///
/// Last write wins, and the worker only ever sees the most recent one.
/// Dragging a progress bar produces a seek request per frame, and
/// servicing all of them in order would mean the decoder chasing a
/// position the operator left several hundred milliseconds ago.
#[derive(Default)]
struct SeekRequest(Mutex<Option<(f64, u64)>>);

impl SeekRequest {
    fn put(&self, secs: f64, generation: u64) {
        *self.0.lock().expect("seek request lock") = Some((secs, generation));
    }

    fn take(&self) -> Option<(f64, u64)> {
        self.0.lock().expect("seek request lock").take()
    }

    fn pending(&self) -> bool {
        self.0.lock().expect("seek request lock").is_some()
    }
}

/// What a backend has to be able to do: hand over the next frame, and
/// jump.
///
/// Sequential-plus-seek rather than random access, because that is the
/// shape both backends genuinely have — ffmpeg decodes forward from a
/// keyframe whatever you ask it for. HAP could do better (every frame is
/// independent) and its `seek` is exact rather than approximate, but it
/// costs nothing to express that through the same door.
pub(crate) trait Decoder: Send {
    /// The next frame in order, or `None` at the end of the clip.
    fn next_frame(&mut self) -> Result<Option<Frame>, VideoError>;
    /// Position so the next `next_frame` is at or just before `secs`.
    fn seek(&mut self, secs: f64) -> Result<(), VideoError>;
}

/// What a backend reports about a clip once it is open.
pub(crate) struct Meta {
    pub width: u32,
    pub height: u32,
    pub duration: f64,
}

/// A clip, decoding on its own thread, presented against a clock the
/// caller supplies.
pub struct VideoSource {
    width: u32,
    height: u32,
    duration: f64,
    frames: Receiver<Frame>,
    seek: Arc<SeekRequest>,
    cursor: Cursor,
    /// The seek this source is currently expecting frames for. Frames
    /// stamped with anything older were decoded before the last scrub
    /// and are thrown away rather than flashed on screen.
    generation: u64,
    /// The time last asked for, so a scrub in progress does not re-ask
    /// for a seek it has already asked for.
    requested: Option<f64>,
}

impl VideoSource {
    /// Opens a clip and starts its decoder thread.
    ///
    /// Which backend runs is decided here rather than by the caller: a
    /// canvas path is content, not a build configuration, and a show
    /// file that named its decoder would break the moment the same show
    /// ran on a build without it.
    pub fn open(path: &Path) -> Result<Self, VideoError> {
        let (meta, decoder) = Self::open_backend(path)?;
        let (tx, frames) = sync_channel(QUEUE_DEPTH);
        let seek = Arc::new(SeekRequest::default());
        let worker_seek = Arc::clone(&seek);
        // Named so it is identifiable in a profiler or a hung-thread
        // dump — one of these exists per canvas, and "which clip" is the
        // first question anyone asks.
        let name = format!(
            "ignition-video:{}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("clip")
        );
        std::thread::Builder::new()
            .name(name)
            .spawn(move || pump(decoder, tx, worker_seek))
            .map_err(|e| VideoError::Backend(format!("spawning the decoder thread: {e}")))?;

        Ok(Self {
            width: meta.width,
            height: meta.height,
            duration: meta.duration,
            frames,
            seek,
            cursor: Cursor::default(),
            generation: 0,
            requested: None,
        })
    }

    #[allow(unused_variables)]
    fn open_backend(path: &Path) -> Result<(Meta, Box<dyn Decoder>), VideoError> {
        // HAP first, and not only because it needs no system library:
        // ffmpeg can decode HAP too, but only by unpacking it to RGBA
        // through its generic path, which is strictly more work than
        // reading the same file directly.
        #[cfg(feature = "hap")]
        match hap::open(path) {
            Ok(opened) => return Ok(opened),
            // Not every `.mov` is HAP, and a `.mp4` almost never is.
            // Falling through rather than failing is what makes the
            // extension the only thing an operator has to get right.
            Err(error) => {
                tracing::debug!(path = %path.display(), %error, "video: not a HAP file")
            }
        }

        #[cfg(feature = "ffmpeg")]
        {
            ffmpeg::open(path)
        }
        #[cfg(not(feature = "ffmpeg"))]
        {
            Err(VideoError::NoBackend(path.display().to_string()))
        }
    }

    /// Pixel dimensions of the clip — the size the canvas texture has to
    /// be allocated at.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Clip length in seconds.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// The frame that belongs on screen at `secs`, or `None` when that
    /// is the frame already on screen.
    ///
    /// `None` is the common case at 120 fps against a 30 fps clip, and
    /// it means "nothing to upload" rather than "nothing to show" — the
    /// caller leaves the texture alone. Never blocks: if the decoder has
    /// not caught up, the last frame stays up.
    ///
    /// `secs` is a position in the *song*, not in the clip. It is
    /// wrapped, so a 12 s loop behind a four-minute song just plays
    /// twenty times.
    pub fn frame_at(&mut self, secs: f64) -> Option<&[u8]> {
        let t = wrap_time(secs, self.duration);

        while let Ok(frame) = self.frames.try_recv() {
            // Frames from before the last scrub. Dropping them here
            // rather than in the worker keeps the worker's job to one
            // thing, and the cost is a few frames of decode after a
            // seek — which was being thrown away either way.
            if frame.generation == self.generation {
                // The seek has landed; a later drift may ask again.
                self.requested = None;
                self.cursor.push(frame);
            }
        }

        let changed = self.cursor.advance_to(t);
        if self.cursor.needs_seek(t) {
            self.request_seek(t);
        }
        if changed {
            self.cursor.current().map(|f| f.rgba.as_slice())
        } else {
            None
        }
    }

    fn request_seek(&mut self, t: f64) {
        // A drag produces a seek request per rendered frame. Re-asking
        // for somewhere the worker is already on its way to would keep
        // resetting its decode, so the playhead has to have actually
        // moved before we ask again.
        //
        // And a request that has not landed yet stands for the whole
        // stretch the clock will cover while it does. A seek on a clip
        // with sparse keyframes decodes forward from the last one for
        // a good fraction of a second; re-asking every frame the clock
        // moved a 240th further threw that decode away each time, and
        // the picture never advanced at all — one frame in two seconds
        // of play, on a clip that decodes fine.
        if self
            .requested
            .is_some_and(|prev| t + BACK_TOLERANCE >= prev && t < prev + LATE_TOLERANCE)
        {
            return;
        }
        self.requested = Some(t);
        self.generation += 1;
        self.cursor.discard_pending();
        self.seek.put(t, self.generation);
    }
}

/// Which decoded frame belongs on screen, and whether the decoder is
/// anywhere near the right part of the clip.
///
/// Split out from the thread plumbing because this is the part with the
/// awkward cases — the loop point, a backwards scrub, the first frame —
/// and it is worth being able to test them without a video file.
#[derive(Default)]
pub(crate) struct Cursor {
    /// Decoded and not yet due, oldest first.
    pending: VecDeque<Frame>,
    current: Option<Frame>,
}

impl Cursor {
    fn push(&mut self, frame: Frame) {
        self.pending.push_back(frame);
    }

    fn current(&self) -> Option<&Frame> {
        self.current.as_ref()
    }

    fn discard_pending(&mut self) {
        self.pending.clear();
    }

    /// Takes every frame that has come due at `secs`. Returns whether
    /// the frame on screen changed.
    fn advance_to(&mut self, secs: f64) -> bool {
        let mut changed = false;
        while let Some(front) = self.pending.front() {
            match &self.current {
                // Nothing on screen yet. Show whatever has arrived even
                // if it is not due — a black panel while the clock walks
                // up to the first frame is worse than being early.
                None => {}
                Some(current) => {
                    if front.pts < current.pts {
                        // The decoder has wrapped to the top of the clip
                        // while the caller's clock is still near the
                        // end. Following it now would eat the last
                        // moment of every loop, so wait for the clock to
                        // wrap too.
                        if secs >= current.pts {
                            break;
                        }
                    } else if front.pts > secs {
                        break;
                    }
                }
            }
            self.current = self.pending.pop_front();
            changed = true;
        }
        changed
    }

    /// Whether the decoder should be told to jump rather than left to
    /// decode its way to `secs`.
    fn needs_seek(&self, secs: f64) -> bool {
        let Some(current) = &self.current else {
            // Nothing decoded yet: the worker is already reading from
            // wherever it was told to start.
            return false;
        };
        if let Some(front) = self.pending.front()
            && front.pts > secs
            && front.pts >= current.pts
        {
            // The next frame is in hand and not due yet, so what is on
            // screen is right — however far apart the two timestamps
            // look.
            return false;
        }
        secs + BACK_TOLERANCE < current.pts || secs > current.pts + LATE_TOLERANCE
    }
}

/// The decoder thread: decode forward, loop at the end, and honour the
/// standing seek request.
///
/// Looping lives here rather than in the caller because it is the
/// decoder that knows it hit the end. The caller's clock is the song's,
/// and the song does not end when the clip does.
fn pump(mut decoder: Box<dyn Decoder>, frames: SyncSender<Frame>, seek: Arc<SeekRequest>) {
    let mut generation = 0;
    // A clip that yields no frames twice running is empty or broken.
    // Without this the loop-at-EOF path is a busy spin on a bad file.
    let mut empty_passes = 0;

    loop {
        if let Some((secs, wanted)) = seek.take() {
            generation = wanted;
            if let Err(error) = decoder.seek(secs) {
                tracing::warn!(%error, secs, "video: seek failed; the clip stops here");
                return;
            }
        }

        let frame = match decoder.next_frame() {
            Ok(Some(frame)) => {
                empty_passes = 0;
                Frame {
                    generation,
                    ..frame
                }
            }
            Ok(None) => {
                empty_passes += 1;
                if empty_passes > 1 {
                    tracing::warn!("video: the clip produced no frames; nothing to loop");
                    return;
                }
                if let Err(error) = decoder.seek(0.0) {
                    tracing::warn!(%error, "video: could not loop back to the start");
                    return;
                }
                continue;
            }
            Err(error) => {
                tracing::warn!(%error, "video: decode failed; the clip stops here");
                return;
            }
        };

        // Backpressure, but staying awake: a blocking `send` would sit
        // in the channel holding a frame nobody wants while a scrub goes
        // unanswered.
        let mut frame = frame;
        loop {
            match frames.try_send(frame) {
                Ok(()) => break,
                Err(TrySendError::Full(held)) => {
                    if seek.pending() {
                        // Whatever we are holding is about to be stale.
                        break;
                    }
                    frame = held;
                    std::thread::sleep(Duration::from_millis(2));
                }
                // The `VideoSource` is gone — this thread is the only
                // thing keeping the decoder alive, so it goes too.
                Err(TrySendError::Disconnected(_)) => return,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(pts: f64) -> Frame {
        Frame {
            pts,
            // One pixel, tagged with its own time so a test can tell
            // which frame it is looking at.
            rgba: vec![(pts * 10.0) as u8, 0, 0, 255],
            generation: 0,
        }
    }

    fn shown(cursor: &Cursor) -> Option<f64> {
        cursor.current().map(|f| f.pts)
    }

    #[test]
    fn extensions_pick_the_path() {
        assert_eq!(content_kind("clips/city.mp4"), ContentKind::Video);
        assert_eq!(content_kind("clips/city.mov"), ContentKind::Video);
        assert_eq!(content_kind("clips/city.webm"), ContentKind::Video);
        assert_eq!(
            content_kind("screens/clip-particles.png"),
            ContentKind::Still
        );
        assert_eq!(
            content_kind("screens/rockstars-logo.webp"),
            ContentKind::Still
        );
        assert_eq!(content_kind("screens/logo.jpg"), ContentKind::Still);
    }

    /// The venue's own files are whatever the operator's exporter wrote,
    /// which on a Mac is as likely to be `.MOV` as `.mov`.
    #[test]
    fn a_proc_string_is_procedural_not_a_file() {
        assert_eq!(content_kind("proc:rainbow"), ContentKind::Procedural);
        assert_eq!(
            content_kind(r#"proc:{"source":{"Solid":[1,0,0]}}"#),
            ContentKind::Procedural
        );
    }

    #[test]
    fn extension_matching_ignores_case() {
        assert_eq!(content_kind("CLIPS/CITY.MOV"), ContentKind::Video);
        assert_eq!(content_kind("CLIPS/CITY.PNG"), ContentKind::Still);
    }

    /// A path with no extension, or one nobody recognises, is a still —
    /// so the asset server reports the problem rather than the decoder.
    #[test]
    fn unknown_extensions_stay_on_the_still_path() {
        assert_eq!(content_kind("clips/city"), ContentKind::Still);
        assert_eq!(content_kind("clips/city.txt"), ContentKind::Still);
    }

    #[test]
    fn a_short_clip_loops_under_a_long_song() {
        // A 12 s loop, 100 s into the song: eight whole passes and 4 s.
        assert!((wrap_time(100.0, 12.0) - 4.0).abs() < 1e-9);
        assert!((wrap_time(12.0, 12.0) - 0.0).abs() < 1e-9);
        assert!((wrap_time(3.5, 12.0) - 3.5).abs() < 1e-9);
    }

    /// A count-in puts the transport before bar 1, so the position
    /// handed in is negative. `%` would answer -2.0, which is not a time
    /// in any clip.
    #[test]
    fn a_negative_song_position_wraps_into_the_clip() {
        assert!((wrap_time(-2.0, 12.0) - 10.0).abs() < 1e-9);
    }

    /// A still-image canvas and a clip that failed to report a length
    /// both arrive here as duration 0. Answering NaN would reach a frame
    /// index.
    #[test]
    fn a_zero_length_clip_reports_zero_rather_than_nan() {
        assert_eq!(wrap_time(5.0, 0.0), 0.0);
        assert_eq!(wrap_time(f64::NAN, 12.0), 0.0);
    }

    #[test]
    fn the_first_frame_goes_up_before_it_is_due() {
        let mut cursor = Cursor::default();
        cursor.push(frame(1.0));
        assert!(cursor.advance_to(0.0));
        assert_eq!(shown(&cursor), Some(1.0));
    }

    /// At 120 fps against a 30 fps clip most frames change nothing, and
    /// saying so is what stops an 8 MB texture upload per rendered
    /// frame.
    #[test]
    fn a_frame_that_is_still_current_reports_no_change() {
        let mut cursor = Cursor::default();
        cursor.push(frame(0.0));
        cursor.push(frame(1.0));
        assert!(cursor.advance_to(0.5));
        assert_eq!(shown(&cursor), Some(0.0));
        assert!(!cursor.advance_to(0.9));
        assert_eq!(shown(&cursor), Some(0.0));
    }

    /// A clock that has jumped forward skips the frames it passed rather
    /// than playing them out one per render.
    #[test]
    fn a_forward_jump_lands_on_the_frame_it_asked_for() {
        let mut cursor = Cursor::default();
        for i in 0..5 {
            cursor.push(frame(i as f64));
        }
        assert!(cursor.advance_to(3.2));
        assert_eq!(shown(&cursor), Some(3.0));
    }

    /// The loop point. The decoder wraps to the top of the clip before
    /// the caller's clock does, and following it early is what would eat
    /// the last moment of every pass.
    #[test]
    fn the_wrap_waits_for_the_callers_clock() {
        let mut cursor = Cursor::default();
        cursor.push(frame(9.8));
        cursor.push(frame(9.9));
        assert!(cursor.advance_to(9.85));
        assert_eq!(shown(&cursor), Some(9.8));

        // The decoder has looped; the clock has not.
        cursor.push(frame(0.0));
        cursor.push(frame(0.1));
        assert!(cursor.advance_to(9.95));
        assert_eq!(shown(&cursor), Some(9.9), "the loop point was jumped early");

        // Now it has.
        assert!(cursor.advance_to(0.05));
        assert_eq!(shown(&cursor), Some(0.0));
    }

    /// A backwards scrub cannot be served from the queue — everything in
    /// it is ahead of the playhead — so it has to become a seek.
    #[test]
    fn a_backwards_scrub_asks_for_a_seek() {
        let mut cursor = Cursor::default();
        cursor.push(frame(30.0));
        cursor.advance_to(30.0);
        assert!(cursor.needs_seek(4.0));
    }

    /// ...whereas normal playback does not, however sparse the clip is.
    /// A 1 fps clip is a second behind its own next frame by definition.
    #[test]
    fn a_queued_next_frame_means_the_current_one_is_right() {
        let mut cursor = Cursor::default();
        cursor.push(frame(0.0));
        cursor.push(frame(1.0));
        cursor.advance_to(0.0);
        assert!(!cursor.needs_seek(0.9));
    }

    /// Nothing decoded yet is not a reason to seek — the worker is
    /// already reading from wherever it was told to start, and asking it
    /// to jump would only throw that away.
    #[test]
    fn an_empty_cursor_does_not_ask_for_a_seek() {
        assert!(!Cursor::default().needs_seek(12.0));
    }

    /// A decoder that has fallen a long way behind with nothing queued
    /// is not going to catch up by decoding.
    #[test]
    fn a_stranded_decoder_is_told_to_jump() {
        let mut cursor = Cursor::default();
        cursor.push(frame(1.0));
        cursor.advance_to(1.0);
        assert!(cursor.needs_seek(1.0 + LATE_TOLERANCE + 0.1));
    }

    /// A fake backend, so the thread plumbing can be tested without a
    /// file: `frames` frames at 10 fps, and a `seek` that is exact.
    struct Fake {
        frames: u32,
        next: u32,
    }

    impl Decoder for Fake {
        fn next_frame(&mut self) -> Result<Option<Frame>, VideoError> {
            if self.next >= self.frames {
                return Ok(None);
            }
            let pts = self.next as f64 / 10.0;
            self.next += 1;
            Ok(Some(frame(pts)))
        }

        fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
            self.next = ((secs * 10.0).round() as u32).min(self.frames.saturating_sub(1));
            Ok(())
        }
    }

    /// The worker loops at the end of the clip by itself. The caller's
    /// clock is the song's and does not stop when the clip runs out.
    #[test]
    fn the_worker_loops_at_the_end_of_the_clip() {
        let (tx, rx) = sync_channel(QUEUE_DEPTH);
        let seek = Arc::new(SeekRequest::default());
        let worker = std::thread::spawn({
            let seek = Arc::clone(&seek);
            move || pump(Box::new(Fake { frames: 3, next: 0 }), tx, seek)
        });

        let got: Vec<f64> = (0..7)
            .map(|_| rx.recv().expect("the worker keeps producing").pts)
            .collect();
        assert_eq!(got, vec![0.0, 0.1, 0.2, 0.0, 0.1, 0.2, 0.0]);

        drop(rx);
        worker
            .join()
            .expect("the worker exits when the source does");
    }

    /// An empty clip must not spin the worker on its own end-of-file.
    #[test]
    fn an_empty_clip_stops_the_worker_rather_than_spinning() {
        let (tx, rx) = sync_channel(QUEUE_DEPTH);
        let seek = Arc::new(SeekRequest::default());
        let worker =
            std::thread::spawn(move || pump(Box::new(Fake { frames: 0, next: 0 }), tx, seek));
        worker.join().expect("the worker gives up on an empty clip");
        assert!(rx.recv().is_err());
    }

    /// A seek request is honoured, and frames decoded before it are
    /// stamped with the old generation so the renderer can drop them.
    #[test]
    fn a_seek_moves_the_worker_and_restamps_its_frames() {
        let (tx, rx) = sync_channel(QUEUE_DEPTH);
        let seek = Arc::new(SeekRequest::default());
        let worker = std::thread::spawn({
            let seek = Arc::clone(&seek);
            move || {
                pump(
                    Box::new(Fake {
                        frames: 100,
                        next: 0,
                    }),
                    tx,
                    seek,
                )
            }
        });

        assert_eq!(rx.recv().expect("a first frame").generation, 0);
        seek.put(5.0, 1);
        // The queue may still hold pre-seek frames; the generation is
        // what tells them apart, which is the point of the stamp.
        let after = std::iter::from_fn(|| rx.recv().ok())
            .find(|f| f.generation == 1)
            .expect("a frame from after the seek");
        assert!(after.pts >= 5.0, "seek landed at {}", after.pts);

        drop(rx);
        worker
            .join()
            .expect("the worker exits when the source does");
    }
}
