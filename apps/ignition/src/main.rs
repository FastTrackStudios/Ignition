//! `ignition` — a night of lighting, following a Live set's playhead.
//!
//! ```text
//! ignition --set "<path>/Sr. Recital Showcase.als"          the operator's clock
//! ignition --set … --clock midi:IAC                          Ableton's MIDI clock
//! ignition --set … --output                                  …and send the rig DMX
//! ignition --set … --check                                   load, report, exit
//! ignition --set … --at 3122                                 which cue is at a beat
//! ignition --midi-ports                                      list MIDI inputs
//! ```
//!
//! The show defaults to `features/lighting/data/shows/jt-recital.ig-show`
//! (`--show` for another). The Live set is not in the repo: `--set`, or
//! `IGNITION_LIVE_SET`.
//!
//! **The clock.** `--clock internal` (the default) is the operator's: the
//! terminal takes a song number to cue it up and Enter to go/stop, `b 17`
//! to jump to bar 17 of the current song. `--clock midi:<port>` follows
//! Ableton Live's MIDI beat clock and Song Position Pointer (Live:
//! Preferences → Link, Tempo & MIDI → the port's Sync output on) — every
//! locate, every song `AbleSet` jumps to, lands on the right cue.

mod night;
#[cfg(target_os = "linux")]
mod process_view;
mod render;
mod studio_clock;

use anyhow::{Context, Result};
use ignition_core::{Bars, Class, CuePlayer, Show, TriggerBus};
use ignition_daw::BeatSource;
use ignition_viz::playback::{LoadOptions, Playback};
use ignition_viz::{Follow, Venue, VizConfig};
use night::Night;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const DEFAULT_SHOW: &str = "data/shows/jt-recital.ig-show";

/// The lighting data root (`features/lighting`): under the working
/// directory first — a binary in a shared target dir sits beside a
/// different checkout than the one it is run in — then the checkout.
fn lighting_root() -> PathBuf {
    if let Some(root) = std::env::var_os("IGNITION_ROOT").map(PathBuf::from)
        && root.join("data/venues").is_dir()
    {
        return root;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    [cwd.join("features/lighting"), cwd.clone()]
        .into_iter()
        .find(|dir| dir.join("data/venues").is_dir())
        .unwrap_or_else(|| utils::paths::checkout_path("features/lighting"))
}

#[derive(Debug)]
enum ClockArg {
    Internal,
    Midi(String),
    /// A running Studio engine's transport: its socket, or the newest.
    Studio(Option<PathBuf>),
}

#[derive(Debug)]
struct Args {
    show: PathBuf,
    set: Option<PathBuf>,
    clock: ClockArg,
    output: bool,
    check: bool,
    at: Option<f64>,
    midi_ports: bool,
    start: Option<usize>,
    camera: Option<String>,
    /// Follow the clock in the terminal, no window: what the show would do.
    watch: bool,
    /// Draw into an Architect pane (a process view) instead of a window.
    process_view: bool,
    /// `render …`: a song to a video, offline (`render.rs`).
    render: Option<render::RenderArgs>,
}

fn parse_args() -> Result<Args> {
    let mut args = Args {
        show: lighting_root().join(DEFAULT_SHOW),
        set: std::env::var_os("IGNITION_LIVE_SET").map(PathBuf::from),
        clock: ClockArg::Internal,
        output: false,
        check: false,
        at: None,
        midi_ports: false,
        start: None,
        camera: None,
        watch: false,
        process_view: std::env::var_os("ARCHITECT_VIEW_FD").is_some(),
        render: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        // `--flag=value` as well as `--flag value`: what an Architect
        // process view's arguments give (`--set={{setting:ignition.live_set}}`,
        // architect#838). An empty value is a setting left empty: as if the
        // flag were not given, so its default holds.
        let (arg, mut inline) = match arg.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => {
                (flag.to_owned(), Some(value.to_owned()))
            }
            _ => (arg, None),
        };
        if inline.as_deref() == Some("") {
            continue;
        }
        let mut next = |what: &str| match inline.take() {
            Some(value) => Ok(value),
            None => it.next().with_context(|| format!("{arg} needs {what}")),
        };
        match arg.as_str() {
            "--show" => args.show = PathBuf::from(next("a .ig-show")?),
            "--set" => args.set = Some(PathBuf::from(next("a .als")?)),
            "--clock" => {
                let value = next("internal, studio[:<socket>] or midi:<port>")?;
                args.clock = match value.split_once(':') {
                    Some(("midi", port)) => ClockArg::Midi(port.to_string()),
                    Some(("studio", socket)) => ClockArg::Studio(Some(PathBuf::from(socket))),
                    None if value == "internal" => ClockArg::Internal,
                    None if value == "studio" => ClockArg::Studio(None),
                    None if value == "midi" => ClockArg::Midi(String::new()),
                    _ => anyhow::bail!(
                        "--clock {value}: use internal, studio[:<socket>] or midi:<port>"
                    ),
                };
            }
            "--output" => args.output = inline.as_deref().is_none_or(|v| v == "true"),
            "--check" => args.check = true,
            "--at" => args.at = Some(next("a beat of the set")?.parse()?),
            "--start" => args.start = Some(next("a song number")?.parse()?),
            "--camera" => args.camera = Some(next("a camera preset")?),
            "--midi-ports" => args.midi_ports = true,
            "--watch" => args.watch = true,
            "--process-view" => args.process_view = true,
            "render" => args.render = Some(render::parse(&mut it)?),
            "-h" | "--help" => {
                println!("{}", include_str!("usage.txt"));
                args.midi_ports = false;
                args.check = true;
                args.set = None;
                return Ok(args);
            }
            other => anyhow::bail!("unknown argument {other} (try --help)"),
        }
    }
    Ok(args)
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,wgpu=warn,naga=warn")),
        )
        .init();
    let args = parse_args()?;
    if args.midi_ports {
        return list_midi_ports();
    }
    let Some(set_path) = args.set.clone() else {
        if args.check {
            return Ok(());
        }
        anyhow::bail!("which Live set? --set <path to .als>, or IGNITION_LIVE_SET");
    };
    let night = Night::load(&args.show, &set_path)?;
    if let Some(render) = &args.render {
        return render::run(night, render);
    }
    print_running_order(&night);
    if let Some(beat) = args.at {
        return report_at(&night, beat);
    }
    if args.check {
        return Ok(());
    }

    let night = Arc::new(night);
    let (clock, stopper) = make_clock(&args, &night)?;
    if args.watch {
        return watch(&night, clock.as_ref());
    }

    // The profile, the GDTF library and the venue's relative files are
    // found from the lighting data root, as Ignition always has.
    let venue_dir = absolute(&night.venue)?;
    let root = lighting_root();
    if root.is_dir() {
        std::env::set_current_dir(&root).with_context(|| format!("entering {}", root.display()))?;
    }
    let venue = Venue::load(&venue_dir)?;
    let playback = Playback::load(&venue, LoadOptions::default())?;
    let (min, max) = venue.bounds();
    let cameras = ignition_viz::Cameras::load_or_builtin(&venue_dir, min, max);
    let mut config = VizConfig::headless(venue, 1600, 900);
    config.overlay = true;
    config.output = args.output;
    config.cameras = cameras;
    config.camera_preset = args.camera.clone();
    config.haze = 0.8;
    config.assets_dir = root.join("viz/assets").to_string_lossy().into_owned();
    let gdtf = ignition_viz::gdtf_geometry::GdtfLibrary::load_default();

    let follower = Arc::new(Follower {
        night,
        clock: Mutex::new(clock),
        current: Mutex::new(None),
    });
    if args.process_view {
        #[cfg(target_os = "linux")]
        let served = process_view::serve(process_view::Show {
            config,
            playback,
            gdtf,
            follow: follower,
        });
        #[cfg(not(target_os = "linux"))]
        let served: Result<()> = Err(anyhow::anyhow!("process views are Linux only"));
        if let Some(stopper) = stopper {
            stopper.stop();
        }
        return served;
    }
    ignition_viz::run_following(config, playback, Some(gdtf), follower);
    // The window closed: let the engine go cleanly.
    if let Some(stopper) = stopper {
        stopper.stop();
    }
    Ok(())
}

/// The playhead source `--clock` names.
fn make_clock(
    args: &Args,
    night: &Arc<Night>,
) -> Result<(Box<dyn BeatSource>, Option<studio_clock::Stopper>)> {
    Ok(match &args.clock {
        ClockArg::Internal => {
            let clock = Arc::new(OperatorClock::new(Arc::clone(night)));
            if let Some(n) = args.start {
                clock.cue_song(n.saturating_sub(1));
            }
            spawn_console(Arc::clone(&clock));
            (Box::new(SharedClock(clock)), None)
        }
        ClockArg::Midi(port) => (open_midi_clock(port)?, None),
        ClockArg::Studio(socket) => {
            let clock = studio_clock::StudioClock::spawn(Arc::clone(night), socket.clone());
            let stopper = clock.stopper();
            (Box::new(clock), Some(stopper))
        }
    })
}

/// `--watch`: where the clock says the show is, twice a second, until
/// Ctrl-C — the follower's own lookup, with no window.
fn watch(night: &Night, clock: &dyn BeatSource) -> Result<()> {
    let mut last = String::new();
    loop {
        let line = clock.beats(Instant::now()).map_or_else(
            || "waiting for the clock".to_string(),
            |beat| describe(night, beat),
        );
        let line = format!("{}{line}", if clock.is_playing() { "▶ " } else { "■ " });
        if line != last {
            println!("{line}");
            last = line;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    })
}

fn print_running_order(night: &Night) {
    println!("{} — {} lit songs", night.name, night.songs.len());
    let mut set = "";
    for (i, song) in night.songs.iter().enumerate() {
        if song.set != set {
            set = &song.set;
            println!("  {set}");
        }
        println!(
            "  {:>3}. {:<40} beats {:>6.0}..{:<6.0} at {:>9.3}s {:>3} sections {:>3} cues",
            i.saturating_add(1),
            song.title,
            song.live.start_beats,
            song.live.end_beats,
            night.set.seconds_at(song.live.start_beats),
            song.live.song.sections.len(),
            song.list.cues.len()
        );
    }
    for set in night.sets.iter().filter(|s| !s.lit) {
        println!("  {} (not lit): {}", set.name, set.songs.join(", "));
    }
}

fn report_at(night: &Night, beat: f64) -> Result<()> {
    println!("{}", describe(night, beat));
    Ok(())
}

/// The song, bar, section and cue at a beat of the set, in a line.
fn describe(night: &Night, beat: f64) -> String {
    let Some(song) = night.song_at(beat).and_then(|i| night.songs.get(i)) else {
        return format!("beat {beat:.1}: between songs (the rig holds the last song's end)");
    };
    let position = song.live.position(beat);
    let player = CuePlayer::new(song.list.cues.clone());
    let cue = player
        .index_at(position)
        .and_then(|c| song.list.cues.get(c))
        .map_or("(before the first cue)", |c| c.name.as_str());
    let section = song
        .live
        .song
        .section_at(position)
        .map_or("", |s| s.name.as_str());
    format!(
        "beat {beat:.1}: {} — bar {} beat {:.2}, section {section:?}, cue {cue:?}, {:.1} bpm",
        song.title,
        position.bar,
        position.beat,
        night.set.bpm_at(beat)
    )
}

#[cfg(feature = "midi-clock")]
fn list_midi_ports() -> Result<()> {
    for name in ignition_daw::beat_clock::MidiClockPort::ports()? {
        println!("{name}");
    }
    Ok(())
}

#[cfg(not(feature = "midi-clock"))]
fn list_midi_ports() -> Result<()> {
    anyhow::bail!("built without the midi-clock feature")
}

#[cfg(feature = "midi-clock")]
fn open_midi_clock(port: &str) -> Result<Box<dyn BeatSource>> {
    let port = ignition_daw::beat_clock::MidiClockPort::open(port)?;
    tracing::info!(port = %port.name, "clock: following MIDI beat clock");
    Ok(Box::new(port))
}

#[cfg(not(feature = "midi-clock"))]
fn open_midi_clock(_port: &str) -> Result<Box<dyn BeatSource>> {
    anyhow::bail!("built without the midi-clock feature")
}

// ---------------------------------------------------------------------------
// The operator's clock
// ---------------------------------------------------------------------------

/// A playhead the operator runs: cue a song, GO, stop. It moves through
/// the set's own tempo map, ramps included, so a song's bars land where
/// the band plays them.
struct OperatorClock {
    night: Arc<Night>,
    state: Mutex<ClockState>,
}

#[derive(Debug, Clone, Copy)]
struct ClockState {
    /// Where the playhead is parked, or where it was when it started.
    from_beat: f64,
    started: Option<Instant>,
}

impl OperatorClock {
    fn new(night: Arc<Night>) -> Self {
        let from_beat = night.songs.first().map_or(0.0, |s| s.live.start_beats);
        Self {
            night,
            state: Mutex::new(ClockState {
                from_beat,
                started: None,
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, ClockState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn beat_now(&self, state: ClockState, now: Instant) -> f64 {
        state.started.map_or(state.from_beat, |t| {
            let secs = now.saturating_duration_since(t).as_secs_f64();
            self.night
                .set
                .beat_at(self.night.set.seconds_at(state.from_beat) + secs)
        })
    }

    fn toggle(&self) {
        let now = Instant::now();
        let mut state = self.state();
        if state.started.is_some() {
            state.from_beat = self.beat_now(*state, now);
            state.started = None;
            println!("stopped at beat {:.1}", state.from_beat);
        } else {
            state.started = Some(now);
            println!("GO from beat {:.1}", state.from_beat);
        }
    }

    fn park(&self, beat: f64) {
        let mut state = self.state();
        state.from_beat = beat;
        state.started = None;
    }

    fn cue_song(&self, index: usize) {
        if let Some(song) = self.night.songs.get(index) {
            self.park(song.live.start_beats);
            println!(
                "cued {}. {} — Enter to GO",
                index.saturating_add(1),
                song.title
            );
        } else {
            println!("no song {}", index.saturating_add(1));
        }
    }

    /// Bar `bar` of the song the playhead is in (or was last in).
    fn cue_bar(&self, bar: u32) {
        let now = Instant::now();
        let beat = self.beat_now(*self.state(), now);
        let Some(song) = self
            .night
            .song_at(beat)
            .and_then(|i| self.night.songs.get(i))
        else {
            println!("not in a song");
            return;
        };
        let mut b = song.live.start_beats;
        while b < song.live.end_beats && song.live.position(b) < Bars::bar(bar) {
            b += 0.25;
        }
        self.park(b);
        println!("cued {} bar {bar} — Enter to GO", song.title);
    }
}

/// The operator clock, shared between the console thread and the show.
struct SharedClock(Arc<OperatorClock>);

impl BeatSource for SharedClock {
    fn beats(&self, now: Instant) -> Option<f64> {
        let state = *self.0.state();
        Some(self.0.beat_now(state, now))
    }

    fn is_playing(&self) -> bool {
        self.0.state().started.is_some()
    }
}

/// The operator's console: the terminal the runner was started from.
fn spawn_console(clock: Arc<OperatorClock>) {
    println!("console: <n> cue song n · Enter GO/stop · b <bar> cue a bar · l list (Ctrl-C quits)");
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut line = String::new();
        loop {
            line.clear();
            if stdin.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let cmd = line.trim();
            if cmd.is_empty() {
                clock.toggle();
            } else if cmd == "l" {
                print_running_order(&clock.night);
            } else if let Some(bar) = cmd.strip_prefix("b ") {
                match bar.trim().parse() {
                    Ok(bar) => clock.cue_bar(bar),
                    Err(_) => println!("b <bar number>"),
                }
            } else if let Ok(n) = cmd.parse::<usize>() {
                clock.cue_song(n.saturating_sub(1));
            } else {
                println!("? <n> · Enter · b <bar> · l");
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Following
// ---------------------------------------------------------------------------

/// Puts the song list where the set's playhead is, every frame.
///
/// The playhead is in beats of the set. Inside a lit song, that song's
/// cue list is the Song player (loaded when the playhead enters it), and
/// the player seeks to the song's own bar — so a jump, a restart or a
/// song played out of the set's order lands on the right cue, the way
/// `r[cues.seek]` promises. Between songs the rig holds where the last
/// one left it (its END blackout).
struct Follower {
    night: Arc<Night>,
    clock: Mutex<Box<dyn BeatSource>>,
    current: Mutex<Option<usize>>,
}

impl Follow for Follower {
    fn follow(&self, playback: &mut Playback) {
        let now = Instant::now();
        let beat = {
            let clock = self
                .clock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            clock.beats(now)
        };
        let Some(beat) = beat else { return };
        let Some(index) = self.night.song_at(beat) else {
            return;
        };
        let Some(song) = self.night.songs.get(index) else {
            return;
        };
        let mut current = self
            .current
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entering = *current != Some(index);
        let position = song.live.position(beat);
        let bpm = self.night.set.bpm_at(beat);
        let secs =
            self.night.set.seconds_at(beat) - self.night.set.seconds_at(song.live.start_beats);
        #[expect(
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "a tempo and a song's seconds fit an f32 the show clock runs in"
        )]
        let (bpm, secs) = (bpm as f32, secs as f32);

        let Playback {
            playbacks,
            groups,
            rig,
            palettes,
            speeds,
            profile,
            triggers,
            library,
            bundles,
            looks,
            named_tricks,
            focus_overrides,
            ..
        } = playback;
        speeds.insert("Song".to_string(), bpm);
        let show = Show {
            groups: &*groups,
            palettes: &*palettes,
            rig: &*rig,
            speeds: &*speeds,
            roles: &*profile,
            library: &*library,
            bundles: &*bundles,
            looks: &*looks,
            named_tricks: &*named_tricks,
            focus_overrides: &*focus_overrides,
            tempo: Some(&song.live.song.tempo),
            ..Show::new(&*groups, &*rig)
        };
        if entering {
            tracing::info!(song = %song.title, set = %song.set, bar = position.bar, "show: song");
            *triggers = TriggerBus::new(song.list.triggers.clone());
            match playbacks.of_class(Class::Song) {
                Some(player) => player.replace_list(&song.list, &show),
                None => {
                    playbacks.push(Class::Song, CuePlayer::new(song.list.cues.clone()));
                }
            }
            *current = Some(index);
        }
        triggers.advance(position, secs);
        if let Some(player) = playbacks.of_class(Class::Song) {
            player.set_clock(secs);
            player.seek(position, &show);
        }
    }
}
