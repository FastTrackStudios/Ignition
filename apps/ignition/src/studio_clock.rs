//! The show following Studio's playback: a running `studio engine`'s
//! transport, as a playhead in beats of the Live set.
//!
//! Studio plays the recital as a set of split songs, each its own project
//! in the engine, each starting at its song's locator — the same zero the
//! show's cue lists count from. The engine streams every project's stamped
//! playhead (`TransportSync::positions`, one frame per audio buffer); this
//! takes the song that is playing (or, stopped, the one last moved), maps
//! its seconds onto its range of the Live set, and hands the runner a set
//! beat. Nothing is polled: the playhead between two frames is carried on
//! at its rate, never more than a quarter second.
//!
//! [`StudioPlayhead`] is the decision, pure and tested; [`StudioClock`]
//! connects it to an engine over the engine's socket and keeps it
//! connected.

use crate::night::Night;
use ignition_daw::BeatSource;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One frame of a project's playhead, as the engine stamps it.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub project: String,
    pub seconds: f64,
    pub playing: bool,
    pub rate: f64,
}

#[derive(Debug, Clone, Copy)]
struct Heard {
    seconds: f64,
    playing: bool,
    rate: f64,
    at: Instant,
}

/// Which of the night's songs is up, and where its playhead is.
#[derive(Debug, Default)]
pub struct StudioPlayhead {
    /// Engine project → index into the night's songs. A project that is
    /// no song of the night (an unlit one, something else open) maps to
    /// `None` and is never followed.
    songs: HashMap<String, Option<usize>>,
    last: HashMap<String, Heard>,
    current: Option<String>,
}

/// What [`StudioPlayhead::observe`] needs from the caller.
#[derive(Debug, PartialEq, Eq)]
pub enum Observed {
    Ok,
    /// A project it has not been told the name of: look it up and
    /// [`StudioPlayhead::name`] it.
    Unknown(String),
}

impl StudioPlayhead {
    /// Says which song a project is, from its name and path: the split
    /// songs are `<slug>/<Title>.session`, matched to the night's songs by
    /// slug (folder or file name).
    pub fn name(&mut self, slugs: &[&str], project: &str, name: &str, path: &str) {
        let song = song_for(slugs, name, path);
        if song.is_none() {
            tracing::debug!(
                project.name = name,
                "studio clock: a project that is no lit song"
            );
        }
        self.songs.insert(project.to_owned(), song);
    }

    /// One frame. The song followed is the one playing; while nothing
    /// plays, the one whose playhead last moved (a locate, a song picked).
    pub fn observe(&mut self, frame: &Frame, now: Instant) -> Observed {
        let Some(song) = self.songs.get(&frame.project) else {
            return Observed::Unknown(frame.project.clone());
        };
        let heard = Heard {
            seconds: frame.seconds,
            playing: frame.playing,
            rate: frame.rate,
            at: now,
        };
        // A project's first frame is where it stood, not a move: on
        // connecting, every open song reports once.
        let moved = self.last.get(&frame.project).is_some_and(|h| {
            (h.seconds - frame.seconds).abs() > 1e-6 || h.playing != frame.playing
        });
        let current_plays = self
            .current
            .as_ref()
            .and_then(|c| self.last.get(c))
            .is_some_and(|h| h.playing);
        if song.is_some() && (frame.playing || (moved && !current_plays) || self.current.is_none())
        {
            self.current = Some(frame.project.clone());
        }
        self.last.insert(frame.project.clone(), heard);
        Observed::Ok
    }

    /// The followed song and its playhead at `now`: seconds into the song,
    /// and whether it plays.
    #[must_use]
    pub fn now(&self, now: Instant) -> Option<(usize, f64, bool)> {
        let project = self.current.as_ref()?;
        let song = (*self.songs.get(project)?)?;
        let heard = self.last.get(project)?;
        let seconds = if heard.playing {
            let since = now.saturating_duration_since(heard.at).as_secs_f64();
            since.clamp(0.0, 0.25).mul_add(heard.rate, heard.seconds)
        } else {
            heard.seconds
        };
        Some((song, seconds, heard.playing))
    }
}

/// Seconds into a song of the night as a beat of its Live set: the split
/// song's zero is the song's locator.
#[must_use]
pub fn set_beat(night: &Night, song: usize, seconds: f64) -> Option<f64> {
    let live = &night.songs.get(song)?.live;
    let start = night.set.seconds_at(live.start_beats);
    Some(night.set.beat_at(start + seconds))
}

/// The night's songs' slugs, in its order.
#[must_use]
pub fn slugs(night: &Night) -> Vec<&str> {
    night.songs.iter().map(|s| s.live.slug.as_str()).collect()
}

/// The night's song a project is, by slug.
fn song_for(slugs: &[&str], name: &str, path: &str) -> Option<usize> {
    let path = Path::new(path);
    let candidates = [
        Some(name),
        path.file_stem().and_then(|s| s.to_str()),
        path.parent()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str()),
    ];
    candidates.into_iter().flatten().find_map(|candidate| {
        let stem = candidate.strip_suffix(".session").unwrap_or(candidate);
        let slug = ableton_slug(stem);
        slugs.iter().position(|s| *s == slug)
    })
}

/// `ableton::setlist::slug`'s rule, which named the night's songs.
fn ableton_slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().filter(|c| *c != '\'' && *c != '\u{2019}') {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_owned()
}

// ---------------------------------------------------------------------------
// The engine
// ---------------------------------------------------------------------------

/// The newest running engine's socket: `$XDG_RUNTIME_DIR/fts/engine-<pid>.sock`
/// whose process is alive.
#[must_use]
pub fn find_engine() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(runtime.join("fts"))
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let pid = name.strip_prefix("engine-")?.strip_suffix(".sock")?;
            Path::new("/proc").join(pid).is_dir().then_some(())?;
            Some((entry.metadata().ok()?.modified().ok()?, entry.path()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().next().map(|(_, p)| p)
}

/// A playhead following a Studio engine.
pub struct StudioClock {
    night: Arc<Night>,
    state: Arc<Mutex<StudioPlayhead>>,
    stopper: Stopper,
}

/// Lets the engine go cleanly: the subscription cancelled, the
/// connection shut down and closed — never a reset on the engine's
/// socket (a user's Studio is on the other end).
#[derive(Clone)]
pub struct Stopper {
    stop: Arc<tokio::sync::Notify>,
    thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl Stopper {
    /// Disconnects and waits (at most two seconds) for it to be done.
    pub fn stop(&self) {
        self.stop.notify_one();
        let thread = self.thread.lock().ok().and_then(|mut t| t.take());
        if let Some(thread) = thread {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !thread.is_finished() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

impl StudioClock {
    /// Follows the engine at `socket` (or the newest one running), from a
    /// thread of its own, connecting again whenever the engine goes away.
    /// Ctrl-C or SIGTERM disconnects first, then ends the program.
    pub fn spawn(night: Arc<Night>, socket: Option<PathBuf>) -> Self {
        let state = Arc::new(Mutex::new(StudioPlayhead::default()));
        let stop = Arc::new(tokio::sync::Notify::new());
        let (thread_night, thread_state, thread_stop) =
            (Arc::clone(&night), Arc::clone(&state), Arc::clone(&stop));
        let thread = std::thread::Builder::new()
            .name("studio-clock".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(error) => {
                        tracing::error!(%error, "studio clock: no runtime");
                        return;
                    }
                };
                let ended_by_signal = runtime.block_on(async {
                    let follow = follow_forever(
                        thread_night,
                        thread_state,
                        socket,
                        Arc::clone(&thread_stop),
                    );
                    tokio::pin!(follow);
                    tokio::select! {
                        () = &mut follow => false,
                        () = terminated() => {
                            // Let the follow loop disconnect itself.
                            thread_stop.notify_one();
                            let _ = tokio::time::timeout(Duration::from_secs(2), &mut follow).await;
                            true
                        }
                    }
                });
                if ended_by_signal {
                    std::process::exit(130);
                }
            })
            .ok();
        Self {
            night,
            state,
            stopper: Stopper {
                stop,
                thread: Arc::new(Mutex::new(thread)),
            },
        }
    }

    /// What lets the engine go cleanly.
    #[must_use]
    pub fn stopper(&self) -> Stopper {
        self.stopper.clone()
    }
}

/// Ctrl-C or SIGTERM. The follow loop is then told to stop, and lets the
/// engine go itself (see [`follow`]) before the program ends.
async fn terminated() {
    let mut term = if let Ok(term) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        term
    } else {
        let _ = tokio::signal::ctrl_c().await;
        return;
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

impl BeatSource for StudioClock {
    fn beats(&self, now: Instant) -> Option<f64> {
        let (song, seconds, _) = self.state.lock().ok()?.now(now)?;
        set_beat(&self.night, song, seconds)
    }

    fn is_playing(&self) -> bool {
        self.state
            .lock()
            .ok()
            .and_then(|s| s.now(Instant::now()))
            .is_some_and(|(_, _, playing)| playing)
    }
}

async fn follow_forever(
    night: Arc<Night>,
    state: Arc<Mutex<StudioPlayhead>>,
    socket: Option<PathBuf>,
    stop: Arc<tokio::sync::Notify>,
) {
    let mut said_waiting = false;
    loop {
        let Some(path) = socket.clone().or_else(find_engine) else {
            if !said_waiting {
                tracing::warn!("studio clock: no Studio engine running; waiting for one");
                said_waiting = true;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        };
        said_waiting = false;
        match follow(&night, &state, &path, &stop).await {
            Ok(true) => {
                tracing::info!(socket = %path.display(), "studio clock: let the engine go");
                return;
            }
            Ok(false) => {
                tracing::warn!(socket = %path.display(), "studio clock: the engine's stream ended");
            }
            Err(error) => {
                tracing::warn!(socket = %path.display(), %error, "studio clock: lost the engine");
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// The DAW service lane's caller (what `studio::cli::connect` opens).
#[derive(Clone)]
struct DawLane {
    caller: vox::Caller,
}

impl vox::FromVoxLane for DawLane {
    const SERVICE_NAME: &'static str = "daw-cli";

    fn from_vox_lane(caller: vox::Caller, _connection: Option<vox::ConnectionHandle>) -> Self {
        Self { caller }
    }
}

/// Follows one engine until its stream ends (`Ok(false)`) or `stop` is
/// notified (`Ok(true)`, after a clean disconnect).
async fn follow(
    night: &Night,
    state: &Mutex<StudioPlayhead>,
    socket: &Path,
    stop: &tokio::sync::Notify,
) -> anyhow::Result<bool> {
    let stream = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::UnixStream::connect(socket),
    )
    .await
    .map_err(|_| anyhow::anyhow!("timed out connecting"))??;
    let connection = vox::initiator_on(vox_stream::StreamLink::unix(stream))
        .establish_connection()
        .await
        .map_err(|e| anyhow::anyhow!("handshake: {e:?}"))?;
    let lane = connection
        .open_lane::<DawLane>()
        .await
        .map_err(|e| anyhow::anyhow!("opening the daw lane: {e:?}"))?;
    let daw = daw_control::Daw::new(lane.caller);
    tracing::info!(socket = %socket.display(), "studio clock: following the engine");
    name_all(night, state, &daw).await;
    let mut positions = daw.transport_sync().positions();
    loop {
        let next = tokio::select! {
            next = positions.recv() => next,
            () = stop.notified() => {
                // The subscription first (dropping the stream cancels it
                // on the engine), then the connection, waited for.
                drop(positions);
                let _ = connection.shutdown();
                let _ = tokio::time::timeout(Duration::from_secs(1), connection.closed()).await;
                return Ok(true);
            }
        };
        let Some(frame) = next.map_err(|e| anyhow::anyhow!("the position stream: {e:?}"))? else {
            break;
        };
        let frame = frame.get();
        let observed = {
            let Ok(mut playhead) = state.lock() else {
                let _ = connection.shutdown();
                return Ok(false);
            };
            playhead.observe(
                &Frame {
                    project: frame.project_guid.clone(),
                    seconds: frame.playhead_seconds,
                    playing: frame.is_playing,
                    rate: if frame.playrate > 0.0 {
                        frame.playrate
                    } else {
                        1.0
                    },
                },
                Instant::now(),
            )
        };
        if let Observed::Unknown(project) = observed {
            name_one(night, state, &daw, &project).await;
        }
    }
    let _ = connection.shutdown();
    Ok(false)
}

async fn name_all(night: &Night, state: &Mutex<StudioPlayhead>, daw: &daw_control::Daw) {
    let Ok(projects) = daw.projects().await else {
        return;
    };
    for project in projects {
        let guid = project.guid().to_owned();
        name_one(night, state, daw, &guid).await;
    }
}

async fn name_one(
    night: &Night,
    state: &Mutex<StudioPlayhead>,
    daw: &daw_control::Daw,
    project: &str,
) {
    let (name, path) = match daw.project(project).await {
        Ok(p) => match p.info().await {
            Ok(info) => (info.name, info.path),
            Err(_) => (String::new(), String::new()),
        },
        Err(_) => (String::new(), String::new()),
    };
    if let Ok(mut playhead) = state.lock() {
        playhead.name(&slugs(night), project, &name, &path);
        if let Some(Some(song)) = playhead.songs.get(project)
            && let Some(s) = night.songs.get(*song)
        {
            tracing::info!(song = %s.title, "studio clock: a song of the night is open");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SLUGS: &[&str] = &["drive-to-la", "for-you", "fire"];

    fn frame(project: &str, seconds: f64, playing: bool) -> Frame {
        Frame {
            project: project.into(),
            seconds,
            playing,
            rate: 1.0,
        }
    }

    fn named() -> StudioPlayhead {
        let mut p = StudioPlayhead::default();
        let songs = "/x/recital/songs";
        p.name(
            SLUGS,
            "g-dtla",
            "Drive To LA",
            &format!("{songs}/drive-to-la/Drive To LA.session"),
        );
        p.name(
            SLUGS,
            "g-fire",
            "Fire.session",
            &format!("{songs}/fire/Fire.session"),
        );
        p.name(
            SLUGS,
            "g-other",
            "Berenstein",
            &format!("{songs}/berenstein/Berenstein.session"),
        );
        p
    }

    #[test]
    fn a_project_is_a_song_of_the_night_by_its_slug() {
        let p = named();
        assert_eq!(p.songs.get("g-dtla"), Some(&Some(0)));
        assert_eq!(p.songs.get("g-fire"), Some(&Some(2)));
        assert_eq!(
            p.songs.get("g-other"),
            Some(&None),
            "an unlit song is never followed"
        );
    }

    #[test]
    fn an_unknown_project_is_asked_about() {
        let mut p = named();
        let t = Instant::now();
        assert_eq!(
            p.observe(&frame("g-new", 1.0, true), t),
            Observed::Unknown("g-new".into())
        );
        assert_eq!(p.now(t), None);
    }

    #[test]
    fn the_playing_song_is_followed_and_carried_between_frames() {
        let mut p = named();
        let t = Instant::now();
        assert_eq!(p.observe(&frame("g-fire", 10.0, true), t), Observed::Ok);
        assert_eq!(p.now(t), Some((2, 10.0, true)));
        let later = p.now(t + Duration::from_millis(100)).unwrap_or_default();
        assert!(
            (later.1 - 10.1).abs() < 1e-9,
            "carried on at its rate: {later:?}"
        );
        let stalled = p.now(t + Duration::from_secs(5)).unwrap_or_default();
        assert!(
            (stalled.1 - 10.25).abs() < 1e-9,
            "never more than a quarter second"
        );
    }

    #[test]
    fn a_stopped_song_moved_takes_over_but_never_from_one_that_plays() {
        let mut p = named();
        let t = Instant::now();
        p.observe(&frame("g-fire", 10.0, true), t);
        // Another song's playhead moving while Fire plays does not steal it.
        p.observe(&frame("g-dtla", 3.0, false), t);
        assert_eq!(p.now(t).map(|n| n.0), Some(2));
        // Fire stops; then Drive To LA is located: it is up.
        p.observe(&frame("g-fire", 12.0, false), t);
        p.observe(&frame("g-dtla", 30.0, false), t);
        assert_eq!(p.now(t), Some((0, 30.0, false)));
        // A frame of Fire that did not move does not take it back.
        p.observe(&frame("g-fire", 12.0, false), t);
        assert_eq!(p.now(t).map(|n| n.0), Some(0));
        // Fire plays: it is up again.
        p.observe(&frame("g-fire", 12.0, true), t);
        assert_eq!(p.now(t).map(|n| n.0), Some(2));
    }

    #[test]
    fn connecting_does_not_hop_between_stopped_songs() {
        let mut p = named();
        let t = Instant::now();
        p.observe(&frame("g-fire", 0.0, false), t);
        p.observe(&frame("g-dtla", 0.0, false), t);
        assert_eq!(p.now(t).map(|n| n.0), Some(2), "the first heard stays up");
        p.observe(&frame("g-dtla", 0.0, true), t);
        assert_eq!(p.now(t).map(|n| n.0), Some(0), "until another plays");
    }

    #[test]
    fn an_unlit_song_playing_is_not_followed() {
        let mut p = named();
        let t = Instant::now();
        p.observe(&frame("g-other", 5.0, true), t);
        assert_eq!(p.now(t), None);
    }

    /// The chain end to end: a frame of a split song lands on its own range
    /// of the Live set, counted from the song's locator.
    #[test]
    fn seconds_into_a_split_song_are_beats_from_its_locator() {
        const SET: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Ableton MajorVersion="5" MinorVersion="12.0_12049" Creator="Ableton Live 12.2.5">
  <LiveSet>
    <MainTrack><DeviceChain><Mixer><Tempo><Manual Value="120" /></Tempo></Mixer></DeviceChain></MainTrack>
    <Tracks />
    <Locators><Locators>
      <Locator Id="1"><Time Value="0" /><Name Value="Drive To LA" /></Locator>
      <Locator Id="2"><Time Value="64" /><Name Value="SONG END" /></Locator>
      <Locator Id="3"><Time Value="72" /><Name Value="Fire" /></Locator>
      <Locator Id="4"><Time Value="136" /><Name Value="SONG END" /></Locator>
    </Locators></Locators>
  </LiveSet>
</Ableton>"#;
        let parsed = ableton::parse::parse_live_set(SET).unwrap_or_else(|e| panic!("{e}"));
        let set = ignition_daw::ableton::LiveSet::of(&parsed);
        let song = |name: &str| {
            let live = set.song(name).cloned().unwrap_or_else(|| panic!("{name}"));
            crate::night::NightSong {
                title: name.into(),
                set: "Recital Band Set".into(),
                live,
                list: ignition_core::CueList {
                    name: name.into(),
                    ..ignition_core::CueList::default()
                },
            }
        };
        let songs = vec![song("Drive To LA"), song("Fire")];
        let night = Night {
            name: "test".into(),
            venue: PathBuf::new(),
            set,
            sets: Vec::new(),
            songs,
        };
        let mut p = StudioPlayhead::default();
        p.name(&slugs(&night), "g-fire", "Fire", "/s/fire/Fire.session");
        let t = Instant::now();
        p.observe(&frame("g-fire", 2.0, true), t);
        let (song, seconds, _) = p.now(t).unwrap_or_default();
        let beat = set_beat(&night, song, seconds).unwrap_or_default();
        // Fire's locator is beat 72 (36 s at 120 bpm); 2 s in is 4 beats on.
        assert!((beat - 76.0).abs() < 1e-6, "{beat}");
        assert_eq!(night.song_at(beat), Some(1));
        assert_eq!(
            night.songs[1].live.position(beat),
            ignition_core::Bars::new(2, 1.0)
        );
    }
}
