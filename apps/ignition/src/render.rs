//! `ignition --set <als> render --song <name> --out <file.mp4>`: a song's
//! light show rendered offline to a video, in sync with its audio.
//!
//! Not real time: frame `i` is the show exactly `i / fps` seconds after
//! the song's locator, however long the GPU takes to draw it. The
//! playhead goes through the set's own tempo map (ramps and meter
//! changes included) and the same [`crate::Follower`] the live runner
//! uses puts the song's cue list there, so a video and a night agree on
//! every cue. Frames are piped raw into `ffmpeg`, which encodes H.264
//! and muxes the song's audio from its first sample — the song's start
//! is frame 0 is the audio's sample 0.
//!
//! Beside the video: `<out>.cues.tsv`, the frame each cue lands on, and
//! the cues as the MP4's chapters, so a reviewer can jump to "Chorus 2".

use crate::night::{Night, NightSong};
use anyhow::{Context as _, Result};
use ignition_core::{Bars, CuePlayer};
use ignition_daw::BeatSource;
use ignition_viz::playback::{LoadOptions, Playback};
use ignition_viz::video::driven::{Driver, run_driven};
use ignition_viz::{Venue, VizConfig};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// What `render` was asked for.
#[derive(Debug, Clone)]
pub struct RenderArgs {
    /// The song: its title, locator or slug, or its number in the night.
    pub song: String,
    pub out: PathBuf,
    /// The song's audio, starting at its locator; muxed from sample 0.
    pub audio: Option<PathBuf>,
    pub camera: String,
    pub fps: u32,
    pub width: u32,
    pub height: u32,
    /// Seconds into the song to start and stop at (a part of it, for a check).
    pub from: f64,
    pub to: Option<f64>,
    /// At least this long (seconds from the song's start): a recording
    /// that runs past the song's end on the set is covered, the rig
    /// holding where the song left it.
    pub min_length: f64,
    pub ffmpeg: PathBuf,
    pub crf: u32,
    pub codec: Codec,
}

/// The video encoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// NVIDIA's hardware encoder (the default): the GPU converts and encodes.
    Nvenc,
    /// libx264 on the CPU.
    X264,
}

const USAGE: &str = "render --song <title|locator|n> --out <file.mp4> [--audio <file>] \
                     [--camera Wide] [--fps 30] [--size 1920x1080] [--from <s>] [--to <s>] [--min-length <s>] \
                     [--crf 21] [--codec nvenc|x264] [--draft] [--ffmpeg <path>]";

/// Parses what follows `render` on the command line.
///
/// # Errors
///
/// An unknown flag, a missing value, or no `--song` / `--out`.
pub fn parse(it: &mut dyn Iterator<Item = String>) -> Result<RenderArgs> {
    let mut song = None;
    let mut out = None;
    let mut args = RenderArgs {
        song: String::new(),
        out: PathBuf::new(),
        audio: None,
        camera: "Wide".to_string(),
        fps: 30,
        width: 1920,
        height: 1080,
        from: 0.0,
        to: None,
        min_length: 0.0,
        ffmpeg: std::env::var_os("IGNITION_FFMPEG").map_or_else(|| "ffmpeg".into(), PathBuf::from),
        crf: 21,
        codec: Codec::Nvenc,
    };
    while let Some(arg) = it.next() {
        let mut next = |what: &str| {
            it.next()
                .with_context(|| format!("render {arg} needs {what} ({USAGE})"))
        };
        match arg.as_str() {
            "--song" => song = Some(next("a song")?),
            "--out" => out = Some(PathBuf::from(next("a .mp4")?)),
            "--audio" => args.audio = Some(PathBuf::from(next("an audio file")?)),
            "--camera" => args.camera = next("a camera preset")?,
            "--fps" => args.fps = next("frames per second")?.parse()?,
            "--size" => {
                let size = next("WxH")?;
                let (w, h) = size
                    .split_once('x')
                    .with_context(|| format!("--size {size}: WxH"))?;
                (args.width, args.height) = (w.parse()?, h.parse()?);
            }
            "--from" => args.from = next("seconds")?.parse()?,
            "--to" => args.to = Some(next("seconds")?.parse()?),
            "--min-length" => args.min_length = next("seconds")?.parse()?,
            "--crf" => args.crf = next("a quality (NVENC CQ / x264 CRF)")?.parse()?,
            "--codec" => {
                args.codec = match next("nvenc or x264")?.as_str() {
                    "nvenc" => Codec::Nvenc,
                    "x264" => Codec::X264,
                    other => anyhow::bail!("--codec {other}: nvenc or x264"),
                };
            }
            // 720p: a draft of the whole night in minutes.
            "--draft" => (args.width, args.height) = (1280, 720),
            "--ffmpeg" => args.ffmpeg = PathBuf::from(next("the ffmpeg binary")?),
            other => anyhow::bail!("render: unknown argument {other} ({USAGE})"),
        }
    }
    args.song = song.with_context(|| format!("render needs --song ({USAGE})"))?;
    args.out = out.with_context(|| format!("render needs --out ({USAGE})"))?;
    anyhow::ensure!(args.fps > 0, "--fps must be positive");
    anyhow::ensure!(
        args.width.is_multiple_of(2) && args.height.is_multiple_of(2),
        "--size: H.264 4:2:0 needs an even width and height"
    );
    Ok(args)
}

/// The song `name` names: a number in the night, its title, locator or slug.
fn find_song<'n>(night: &'n Night, name: &str) -> Result<&'n NightSong> {
    if let Ok(n) = name.parse::<usize>()
        && let Some(song) = n.checked_sub(1).and_then(|i| night.songs.get(i))
    {
        return Ok(song);
    }
    let slug = slug(name);
    night
        .songs
        .iter()
        .find(|s| {
            s.title.eq_ignore_ascii_case(name)
                || s.live.name.eq_ignore_ascii_case(name)
                || s.live.slug == slug
                || self::slug(&s.title) == slug
        })
        .with_context(|| format!("the night has no lit song {name:?}"))
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c != '\'' && !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

// ---------------------------------------------------------------------------
// The frame clock: pure arithmetic
// ---------------------------------------------------------------------------

/// Which moment each frame is: frame `i` is `from + i / fps` seconds after
/// the song's start, on the set's clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameClock {
    /// Seconds from the top of the set to the song's frame 0.
    pub start_secs: f64,
    pub fps: u32,
    pub frames: u32,
}

impl FrameClock {
    /// `duration` seconds of a song starting `start_secs` into the set.
    #[must_use]
    pub fn new(start_secs: f64, duration: f64, fps: u32) -> Self {
        let fps = fps.max(1);
        // Every frame whose moment is inside the range; a hair of slack
        // so a duration that is a whole number of frames is not one over.
        let frames = (duration.max(0.0) * f64::from(fps) - 1e-6).ceil().max(0.0);
        Self {
            start_secs,
            fps,
            frames: u32_of(frames),
        }
    }

    /// Seconds after frame 0 that frame `index` shows.
    #[must_use]
    pub fn secs(&self, index: u32) -> f64 {
        f64::from(index) / f64::from(self.fps)
    }

    /// The beat of the set frame `index` shows, through the set's tempo map.
    #[must_use]
    pub fn set_beat(&self, beat_at: impl Fn(f64) -> f64, index: u32) -> f64 {
        beat_at(self.start_secs + self.secs(index))
    }

    /// The first frame showing a moment at or after `secs` from the top of
    /// the set: where something that happens then is first seen.
    #[cfg(test)]
    #[must_use]
    pub fn frame_at(&self, secs: f64) -> u32 {
        u32_of(
            ((secs - self.start_secs) * f64::from(self.fps) - 1e-6)
                .ceil()
                .max(0.0),
        )
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a frame count: whole, non-negative and far below u32::MAX (clamped)"
)]
fn u32_of(v: f64) -> u32 {
    v.clamp(0.0, f64::from(u32::MAX)) as u32
}

/// One cue taking over, as a frame of the video.
#[derive(Debug, Clone, PartialEq)]
pub struct CueMark {
    pub frame: u32,
    pub secs: f64,
    pub position: Bars,
    pub name: String,
}

/// The frame each cue of the song takes over on: walk the frames, ask
/// the cue list which cue a clock at that bar shows (`index_at`, what
/// `seek` plays), and note every change.
#[must_use]
pub fn cue_marks(
    clock: &FrameClock,
    player: &CuePlayer,
    position_at: impl Fn(u32) -> Bars,
) -> Vec<CueMark> {
    let mut marks: Vec<CueMark> = Vec::new();
    let mut last = None;
    for index in 0..clock.frames {
        let position = position_at(index);
        let cue = player.index_at(position);
        if cue != last {
            last = cue;
            let name = cue
                .and_then(|c| player.cues().get(c))
                .map_or_else(|| "(before the first cue)".to_string(), |c| c.name.clone());
            marks.push(CueMark {
                frame: index,
                secs: clock.secs(index),
                position,
                name,
            });
        }
    }
    marks
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

/// A playhead that is wherever the render last put it.
struct FixedBeat(Arc<Mutex<f64>>);

impl BeatSource for FixedBeat {
    fn beats(&self, _now: Instant) -> Option<f64> {
        Some(
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    fn is_playing(&self) -> bool {
        true
    }
}

/// Renders one song of the night to `args.out`.
///
/// # Errors
///
/// The song is not in the night, the venue does not load, ffmpeg does not
/// start or fails, or a frame is never captured.
pub fn run(night: Night, args: &RenderArgs) -> Result<()> {
    let night = Arc::new(night);
    let index = {
        let song = find_song(&night, &args.song)?;
        night
            .songs
            .iter()
            .position(|s| std::ptr::eq(s, song))
            .context("the song is in the night")?
    };
    let song = night.songs.get(index).context("the song is in the night")?;
    let song_start = night.set.seconds_at(song.live.start_beats);
    let song_len = night.set.seconds_at(song.live.end_beats) - song_start;
    // Past the song's end the follower finds no song and the rig holds.
    let to = args.to.unwrap_or_else(|| song_len.max(args.min_length));
    anyhow::ensure!(
        to > args.from,
        "render: --from {} is past --to {to}",
        args.from
    );
    let clock = FrameClock::new(song_start + args.from, to - args.from, args.fps);

    // The cues, in frames — the sidecar, the chapters, and the check.
    let player = CuePlayer::new(song.list.cues.clone());
    let set = &night.set;
    let marks = cue_marks(&clock, &player, |i| {
        song.live.position(clock.set_beat(|s| set.beat_at(s), i))
    });
    let out = absolute(&args.out)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    write_cue_sheet(&out, &marks)?;
    let chapters = out.with_extension("chapters.txt");
    write_chapters(&chapters, &song.title, &marks, clock)?;
    let audio = args.audio.as_deref().map(absolute).transpose()?;

    println!(
        "render: {} — {:.3}s..{:.3}s of the song (set {:.3}s), {} frames at {} fps, {}x{}, camera {}",
        song.title,
        args.from,
        to,
        clock.start_secs,
        clock.frames,
        clock.fps,
        args.width,
        args.height,
        args.camera
    );

    // The rig, as the runner loads it for a window.
    let venue_dir = crate::absolute(&night.venue)?;
    let root = crate::lighting_root();
    if root.is_dir() {
        std::env::set_current_dir(&root).with_context(|| format!("entering {}", root.display()))?;
    }
    let venue = Venue::load(&venue_dir)?;
    let playback = Playback::load(&venue, LoadOptions::default())?;
    let (min, max) = venue.bounds();
    let cameras = ignition_viz::Cameras::load_or_builtin(&venue_dir, min, max);
    anyhow::ensure!(
        cameras
            .presets
            .iter()
            .any(|p| p.name.eq_ignore_ascii_case(&args.camera)),
        "the venue has no camera {:?}",
        args.camera
    );
    let mut config = VizConfig::headless(venue, args.width, args.height);
    config.overlay = false;
    config.output = false;
    config.cameras = cameras;
    config.camera_preset = Some(args.camera.clone());
    // The room says how much haze hangs in it and how the camera is set
    // (`look` in its manifest); a room that does not is normally hazed.
    ignition_viz::venue_watch::watch(&venue_dir);
    let (haze, exposure, output) = crate::night::venue_look(&venue_dir);
    if let Some(output) = output {
        ignition_viz::fixture_profile::set_output_scale(output);
    }
    // A lit fixture shows its light on its own face, as a real one does:
    // without it a lamp coming on is only more light on the floor, and a
    // sweep through a tower's lamps cannot be told from a swell.
    config.body_glow = true;
    config.haze = haze.unwrap_or(0.8);
    if let Some(ambient) = crate::night::venue_ambient(&venue_dir) {
        config.ambient = ambient;
    }
    if let Some(stops) = exposure {
        config.exposure = stops;
    }
    config.assets_dir = root.join("viz/assets").to_string_lossy().into_owned();
    let gdtf = ignition_viz::gdtf_geometry::GdtfLibrary::load_default();

    let partial = out.with_extension("partial.mp4");
    let encoder = Encoder::spawn(
        args,
        &partial,
        audio.as_deref(),
        &chapters,
        &song.title,
        clock,
    )?;
    let beat = Arc::new(Mutex::new(clock.set_beat(|s| set.beat_at(s), 0)));
    let follower = crate::Follower {
        night: Arc::clone(&night),
        clock: Mutex::new(Box::new(FixedBeat(Arc::clone(&beat)))),
        current: Mutex::new(None),
        live: Mutex::default(),
    };
    let mut driver = SongDriver {
        night: Arc::clone(&night),
        clock,
        beat,
        follower,
        encoder,
        began: Instant::now(),
    };
    let rendered = run_driven(
        config,
        playback,
        Some(gdtf),
        clock.fps,
        clock.frames,
        &mut driver,
    );
    let SongDriver { encoder, began, .. } = driver;
    let finished = encoder.finish();
    rendered?;
    finished?;
    std::fs::rename(&partial, &out)
        .with_context(|| format!("moving {} to {}", partial.display(), out.display()))?;
    let _ = std::fs::remove_file(&chapters);
    println!(
        "render: wrote {} ({} frames in {:.0}s)",
        out.display(),
        clock.frames,
        began.elapsed().as_secs_f64()
    );
    Ok(())
}

fn absolute(path: &Path) -> Result<PathBuf> {
    crate::absolute(path)
}

struct SongDriver {
    night: Arc<Night>,
    clock: FrameClock,
    beat: Arc<Mutex<f64>>,
    follower: crate::Follower,
    encoder: Encoder,
    began: Instant,
}

impl Driver for SongDriver {
    fn position(&mut self, playback: &mut Playback, index: u32) {
        let set = &self.night.set;
        let beat = self.clock.set_beat(|s| set.beat_at(s), index);
        *self
            .beat
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = beat;
        ignition_viz::Follow::follow(&self.follower, playback);
    }

    fn frame(&mut self, index: u32, rgba: Vec<u8>, width: u32, height: u32) -> Result<()> {
        anyhow::ensure!(
            (width, height) == (self.encoder.width, self.encoder.height),
            "frame {index} is {width}x{height}, the video {}x{}",
            self.encoder.width,
            self.encoder.height
        );
        self.encoder.push(rgba)?;
        let done = index.saturating_add(1);
        if done.is_multiple_of(self.clock.fps.saturating_mul(15)) || done == self.clock.frames {
            let elapsed = self.began.elapsed().as_secs_f64();
            let rate = f64::from(done) / elapsed.max(1e-3);
            let left = f64::from(self.clock.frames.saturating_sub(done)) / rate.max(1e-3);
            println!(
                "  {done}/{} frames ({:.0}s of video), {rate:.1} fps, ~{left:.0}s left",
                self.clock.frames,
                self.clock.secs(done)
            );
        }
        Ok(())
    }

    fn finished(&mut self, stats: &ignition_viz::video::driven::Stats) {
        let secs = stats.total.as_secs_f64().max(1e-3);
        println!(
            "render: {} frames in {secs:.1}s = {:.1} fps ({:.2}x real time); \
             updates {} ({:.1} ms each, {:.1} ms polling the GPU), hand-off {:.1} ms a frame",
            stats.frames,
            f64::from(stats.frames) / secs,
            self.clock.secs(stats.frames) / secs,
            stats.updates,
            stats.step.as_secs_f64() * 1e3 / f64::from(stats.updates.max(1)),
            stats.wait.as_secs_f64() * 1e3 / f64::from(stats.updates.max(1)),
            stats.hand_off.as_secs_f64() * 1e3 / f64::from(stats.frames.max(1)),
        );
    }
}

/// `ffmpeg`, reading raw RGBA frames on stdin, written from a thread of
/// its own: the render hands a frame over and goes on drawing, and only
/// waits when [`QUEUE`] frames are already waiting for the encoder.
struct Encoder {
    child: Child,
    frames: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    writer: Option<std::thread::JoinHandle<std::io::Result<()>>>,
    width: u32,
    height: u32,
}

/// Frames between the render and the encoder (a 1080p frame is 8 MB).
const QUEUE: usize = 8;

impl Encoder {
    fn spawn(
        args: &RenderArgs,
        out: &Path,
        audio: Option<&Path>,
        chapters: &Path,
        title: &str,
        clock: FrameClock,
    ) -> Result<Self> {
        let duration = format!("{:.6}", clock.secs(clock.frames));
        let mut cmd = Command::new(&args.ffmpeg);
        cmd.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y"])
            .args(["-f", "rawvideo", "-pix_fmt", "rgba"])
            .args(["-s", &format!("{}x{}", args.width, args.height)])
            .args(["-framerate", &args.fps.to_string(), "-i", "pipe:0"]);
        if let Some(audio) = audio {
            // Seek on the input, decoding: sample-accurate, so `--from`
            // cuts the audio exactly where frame 0 is.
            cmd.args(["-ss", &format!("{:.6}", args.from), "-t", &duration])
                .arg("-i")
                .arg(audio);
        }
        cmd.arg("-i").arg(chapters);
        let meta = if audio.is_some() { "2" } else { "1" };
        cmd.args(["-map", "0:v"]);
        if audio.is_some() {
            cmd.args(["-map", "1:a", "-c:a", "aac", "-b:a", "192k"]);
        }
        cmd.args(["-map_metadata", meta, "-map_chapters", meta]);
        match args.codec {
            // The GPU's encoder, given the RGBA as it is: its own colour
            // conversion, no CPU pass over the frame.
            // No CPU pass: NVENC converts RGBA to 4:2:0 itself, with the
            // BT.601 matrix (measured: pure red decoded as 709 came back
            // (255, 23, 0)), so the stream says 601 (`bt470bg`) and a
            // player decodes it exactly (within 2 levels).
            Codec::Nvenc => cmd
                .args(["-c:v", "h264_nvenc", "-preset", "p4", "-tune", "hq"])
                .args(["-rc", "vbr", "-cq", &args.crf.to_string(), "-b:v", "0"])
                .args(["-profile:v", "high", "-colorspace", "bt470bg"]),
            Codec::X264 => cmd
                .args([
                    "-vf",
                    "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p",
                ])
                .args(["-c:v", "libx264", "-preset", "medium"])
                .args(["-crf", &args.crf.to_string(), "-colorspace", "bt709"]),
        };
        // Frames 1:1, no rate conversion: with `-r` the AAC priming (its
        // first packet before 0) duplicated frame 0 and put every cue a
        // frame late (measured, a snap on 60 shown on 61).
        cmd.args(["-fps_mode", "passthrough"])
            .args(["-color_primaries", "bt709", "-color_trc", "bt709"])
            .args(["-color_range", "tv"])
            .args(["-metadata", &format!("title={title} — Lights")])
            .args(["-movflags", "+faststart", "-f", "mp4"])
            .arg(out)
            .stdin(Stdio::piped());
        let mut child = cmd
            .spawn()
            .with_context(|| format!("starting {} (IGNITION_FFMPEG)", args.ffmpeg.display()))?;
        let mut stdin: ChildStdin = child.stdin.take().context("ffmpeg's stdin")?;
        let (frames, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(QUEUE);
        let writer = std::thread::Builder::new()
            .name("ignition-encode".into())
            .spawn(move || {
                for frame in rx {
                    stdin.write_all(&frame)?;
                }
                stdin.flush()
            })
            .context("starting the encoder's writer")?;
        Ok(Self {
            child,
            frames: Some(frames),
            writer: Some(writer),
            width: args.width,
            height: args.height,
        })
    }

    fn push(&mut self, rgba: Vec<u8>) -> Result<()> {
        let frames = self.frames.as_ref().context("the encoder is finished")?;
        frames
            .send(rgba)
            .map_err(|_| anyhow::anyhow!("ffmpeg stopped reading frames (see its error above)"))
    }

    fn finish(mut self) -> Result<()> {
        drop(self.frames.take());
        if let Some(writer) = self.writer.take() {
            writer
                .join()
                .map_err(|_| anyhow::anyhow!("the encoder's writer panicked"))?
                .context("writing the frames to ffmpeg")?;
        }
        let status = self.child.wait().context("waiting for ffmpeg")?;
        anyhow::ensure!(status.success(), "ffmpeg failed: {status}");
        Ok(())
    }
}

/// `<out>.cues.tsv`: frame, seconds, bar, beat, cue.
fn write_cue_sheet(out: &Path, marks: &[CueMark]) -> Result<()> {
    let path = out.with_extension("cues.tsv");
    let mut text = String::from("frame\tseconds\tbar\tbeat\tcue\n");
    for mark in marks {
        text.push_str(&format!(
            "{}\t{:.3}\t{}\t{:.3}\t{}\n",
            mark.frame, mark.secs, mark.position.bar, mark.position.beat, mark.name
        ));
    }
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
}

/// The cues as ffmetadata chapters (milliseconds).
fn write_chapters(path: &Path, title: &str, marks: &[CueMark], clock: FrameClock) -> Result<()> {
    let escape = |s: &str| {
        s.chars()
            .flat_map(|c| {
                let escaped = matches!(c, '=' | ';' | '#' | '\\' | '\n');
                escaped
                    .then_some('\\')
                    .into_iter()
                    .chain(std::iter::once(c))
            })
            .collect::<String>()
    };
    let ms = |secs: f64| u64::from(u32_of((secs * 1000.0).round()));
    let end = clock.secs(clock.frames);
    let mut text = format!(";FFMETADATA1\ntitle={} — Lights\n", escape(title));
    for (i, mark) in marks.iter().enumerate() {
        let next = marks.get(i.saturating_add(1)).map_or(end, |m| m.secs);
        text.push_str(&format!(
            "\n[CHAPTER]\nTIMEBASE=1/1000\nSTART={}\nEND={}\ntitle={}\n",
            ms(mark.secs),
            ms(next),
            escape(&mark.name)
        ));
    }
    std::fs::write(path, text).with_context(|| format!("writing {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A song in 4/4 at a constant tempo: seconds -> set beats, and a
    /// set beat -> bar and beat of the song starting at `start_beat`.
    fn steady(bpm: f64) -> impl Fn(f64) -> f64 {
        move |secs| secs * bpm / 60.0
    }

    fn bars_of(beat: f64, start_beat: f64) -> Bars {
        let into = (beat - start_beat).max(0.0);
        let bar = (into / 4.0 + 1e-9).floor();
        Bars::new(u32_of(bar) + 1, into - bar * 4.0 + 1.0)
    }

    #[test]
    fn frame_zero_is_the_song_start_and_frames_step_by_one_over_fps() {
        let clock = FrameClock::new(321.0, 10.0, 30);
        assert_eq!(clock.frames, 300);
        assert_eq!(clock.secs(0), 0.0);
        assert!((clock.secs(30) - 1.0).abs() < 1e-12);
        let beat_at = steady(120.0);
        assert!((clock.set_beat(&beat_at, 0) - 642.0).abs() < 1e-9);
        assert_eq!(clock.frame_at(321.0), 0);
    }

    #[test]
    fn a_whole_number_of_frames_is_not_one_over() {
        assert_eq!(FrameClock::new(0.0, 123.0, 30).frames, 3690);
        assert_eq!(FrameClock::new(0.0, 263.457_917, 30).frames, 7904);
    }

    /// A cue at bar 9 of a 120 bpm song starting at the top of the set is
    /// 32 beats = 16 s in: frame 480 at 30 fps, and not one frame early.
    #[test]
    fn a_cue_lands_on_the_frame_of_its_beat() {
        let list: ignition_core::CueList = serde_json::from_str(
            r#"{"cues":[
                {"name":"Dark","at":{"section":"Intro","ordinal":0,"bars":0,"beat":1.0},"resolved":{"bar":1,"beat":1.0}},
                {"name":"Hit","at":{"section":"Intro","ordinal":0,"bars":8,"beat":1.0},"resolved":{"bar":9,"beat":1.0}},
                {"name":"Off-beat","at":{"section":"Intro","ordinal":0,"bars":10,"beat":2.5},"resolved":{"bar":11,"beat":2.5}}
            ]}"#,
        )
        .unwrap();
        let player = CuePlayer::new(list.cues);
        let beat_at = steady(120.0);
        let clock = FrameClock::new(0.0, 30.0, 30);
        let marks = cue_marks(&clock, &player, |i| {
            bars_of(clock.set_beat(&beat_at, i), 0.0)
        });
        let frames: Vec<(u32, &str)> = marks.iter().map(|m| (m.frame, m.name.as_str())).collect();
        // Bar 11 beat 2.5 = 41.5 beats = 20.75 s = frame 622.5 -> first seen on 623.
        assert_eq!(frames, vec![(0, "Dark"), (480, "Hit"), (623, "Off-beat")]);
        assert_eq!(clock.frame_at(16.0), 480);
        assert_eq!(clock.frame_at(20.75), 623);
    }

    /// A song later in the set, on a ramp: frame 0 is its own start, and
    /// the cue lands where the ramped clock says, not where a steady one would.
    #[test]
    fn frames_follow_the_sets_tempo_map_not_a_constant_tempo() {
        // 100 bpm for the first 60 s of the set (100 beats), then 150 bpm.
        let beat_at = |secs: f64| {
            if secs < 60.0 {
                secs * 100.0 / 60.0
            } else {
                100.0 + (secs - 60.0) * 150.0 / 60.0
            }
        };
        // The song starts at set beat 80 (48 s), its bar 9 is set beat 112:
        // 60 s + 12 beats at 150 bpm = 64.8 s, 16.8 s after the song's start.
        let clock = FrameClock::new(48.0, 30.0, 30);
        let list: ignition_core::CueList = serde_json::from_str(
            r#"{"cues":[
                {"name":"A","at":{"section":"Intro","ordinal":0,"bars":0,"beat":1.0},"resolved":{"bar":1,"beat":1.0}},
                {"name":"B","at":{"section":"Intro","ordinal":0,"bars":8,"beat":1.0},"resolved":{"bar":9,"beat":1.0}}
            ]}"#,
        )
        .unwrap();
        let player = CuePlayer::new(list.cues);
        let marks = cue_marks(&clock, &player, |i| {
            bars_of(clock.set_beat(beat_at, i), 80.0)
        });
        assert_eq!(marks.get(1).map(|m| m.frame), Some(504));
        assert!((clock.secs(504) - 16.8).abs() < 1e-9);
    }

    #[test]
    fn songs_are_found_by_title_locator_slug_or_number() {
        assert_eq!(slug("Heart Isn't Ready"), "heart-isnt-ready");
        assert_eq!(slug("Know Me (Extended)"), "know-me-extended");
        assert_eq!(slug("Hey!"), "hey");
    }
}
