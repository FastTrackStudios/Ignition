//! Lighting on a session: the show's cue list kept in a session (the
//! session primitive) through its host's `fts.session`, and taken from
//! the session's transport.
//!
//! - `IGNITION_SESSION_STORE {doc, lane, song}`: stores the cue list of a
//!   song's show document (`song`, a path) as lane `lane` of session `doc`.
//! - `IGNITION_SESSION_CUES {doc, lane}`: the list read back through the
//!   service, in GO order: number, name, bar (`null`: the operator's).
//! - `IGNITION_SESSION_FOLLOW {url, doc, lane}`: follows the session
//!   host's transport (its presence and clock at `url`) and takes the
//!   list's cues as the playhead crosses them (`ignition_session::CueFollower`).
//! - `IGNITION_SESSION_TAKEN`: what the follower took: each cue, the
//!   session time it was taken at, and the clock estimate.
//! - `IGNITION_SESSION_SONG {doc, lane, live_set, locator, cues, bar_one?, tempo?}`: a
//!   recital song into a session: its cue list (`cues`, a song file whose
//!   positions are relative to the song's sections) resolved against the
//!   Live set's song `locator`, as the night resolves it, and the set's
//!   tempo over the song made the session's, so a cue's bar and time agree
//!   with the room's; `bar_one` (a session bar) is where the chart's bar
//!   1 is, after a count-in (session `bar_one`); `tempo: false` leaves the
//!   session's tempo map to its host (the DAW engine).
//! - `IGNITION_SESSION_ROOM {url, doc, song}`: the room follows the
//!   session: its tempo read through the service as this extension, then
//!   the room's clock set to the session host's transport
//!   (`--clock session:<url>#<doc>@<song>`; the room restarts on it).
//!
//! Every read and write is a host service call made as this extension
//! (`HostRequest::Service`), so the session's provider answers for
//! `ignition`'s data only, and a read-only share link's guest copy can
//! only read.

use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use architect::connect::{Endpoint, pool};
use architect_extensions::hostcall::HostRequest;
use architect_extensions::linked::Handle;
use crdt::clock::SharedClock;
use crdt::presence::clock_ms;
use crdt::sync::{DocPresenceClient, PresencePeer};
use ignition_daw::ableton::{LiveSet, LiveSetSong};
use ignition_session::{
    Ask, AskFuture, CueFollower, Take, load, set_bar_one, set_tempo, store, tempo,
};
use ignition_show::CueList;
use ignition_show::show_file::ShowDocument;
use serde_json::{Value, json};
use session::LaneId;
use session::doc::transport_of;
use session::service::{TempoChange, TempoMapData};

/// `fts.session` through the host, as this extension.
pub(crate) struct Host(Handle);

impl Ask for Host {
    fn ask(&self, method: &'static str, args: String) -> AskFuture<'_> {
        Box::pin(async move {
            let args: Value = serde_json::from_str(&args).map_err(|e| e.to_string())?;
            let reply = self
                .0
                .host(HostRequest::Service {
                    service: "fts.session".to_owned(),
                    method: method.to_owned(),
                    args,
                })
                .await
                .map_err(|e| format!("{e:?}"))?;
            Ok(reply.to_string())
        })
    }
}

fn host() -> Result<Host, String> {
    crate::ext::handle().map(Host)
}

/// `fts.session` through the host, as this extension: for the views.
pub(crate) fn host_ask() -> Result<Host, String> {
    host()
}

fn text(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("`{key}` is missing"))
}

/// Stores a song's cue list in the session.
pub async fn store_song(args: Value) -> Result<Value, String> {
    let doc = text(&args, "doc")?;
    let lane = LaneId::from(text(&args, "lane")?.as_str());
    let song = ShowDocument::load(text(&args, "song")?).map_err(|e| e.to_string())?;
    store(&host()?, &doc, &lane, &song.list)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "stored": song.list.cues.len(), "lane": lane.as_str() }))
}

/// The cue list read back through the service.
pub async fn cues(args: Value) -> Result<Value, String> {
    let doc = text(&args, "doc")?;
    let lane = LaneId::from(text(&args, "lane")?.as_str());
    let list = load(&host()?, &doc, &lane)
        .await
        .map_err(|e| e.to_string())?;
    let cues: Vec<Value> = list
        .cues
        .iter()
        .map(|c| json!({ "number": c.number, "name": c.name, "bar": c.position().map(|b| b.bar) }))
        .collect();
    Ok(json!({ "name": list.name, "cues": cues }))
}

/// The room follows session `doc`, hosted at `url`, as the night's song
/// `song`: the session is read through `fts.session` as this extension
/// (its tempo, so a session this extension may not read is refused here),
/// then the room's clock is set to its host's transport. The renderer is
/// a program, not this extension: it follows the transport (presence,
/// never the document) itself.
pub async fn room(args: Value) -> Result<Value, String> {
    let url = text(&args, "url")?;
    let doc = text(&args, "doc")?;
    let song = text(&args, "song")?;
    let lane = text(&args, "lane").unwrap_or_else(|_| "main".to_owned());
    let map = tempo(&host()?, &doc).await.map_err(|e| e.to_string())?;
    let clock = format!("session:{url}#{doc}@{song}");
    crate::ext::set_setting("ignition.clock", &clock).await?;
    // The cue list view follows the same session, its lane `lane`.
    crate::ext::set_setting(crate::cues::SETTING, &format!("{url}#{doc}@{lane}")).await?;
    Ok(json!({
        "clock": clock,
        "tempo_changes": map.points().len(),
        "bpm_at_start": map.bpm_at(0.0),
    }))
}

/// A recital song into a session: its cues resolved against the Live
/// set, the set's tempo over the song as the session's tempo map.
pub async fn song(args: Value) -> Result<Value, String> {
    let doc = text(&args, "doc")?;
    let lane = LaneId::from(text(&args, "lane")?.as_str());
    let set = LiveSet::read(text(&args, "live_set")?).map_err(|e| e.to_string())?;
    let locator = text(&args, "locator")?;
    let live = set
        .song(&locator)
        .ok_or_else(|| format!("the Live set has no song {locator:?}"))?
        .clone();
    let path = text(&args, "cues")?;
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
    let mut list: CueList = serde_json::from_str(&raw).map_err(|e| format!("{path}: {e}"))?;
    let unresolved = list.resolve_positions(&live.song);
    let tempo = song_tempo(&set, &live);
    let changes = tempo.points.len();
    let ask = host()?;
    // The set's tempo becomes the session's unless the session's host owns
    // its timing (the DAW engine: `tempo: false`), when the cues are placed
    // on the session's own tempo map.
    let keep_tempo = args.get("tempo").and_then(Value::as_bool) == Some(false);
    if !keep_tempo {
        set_tempo(&ask, &doc, tempo)
            .await
            .map_err(|e| e.to_string())?;
    }
    store(&ask, &doc, &lane, &list)
        .await
        .map_err(|e| e.to_string())?;
    // Where the chart's bar 1 is, when the multitracks start earlier (a
    // count-in): the session bar given, as the song's bar 1.
    let bar_one = args
        .get("bar_one")
        .and_then(Value::as_u64)
        .and_then(|b| u32::try_from(b).ok());
    if bar_one.is_some() {
        set_bar_one(&ask, &doc, bar_one)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(json!({
        "bar_one": bar_one,
        "stored": list.cues.len(),
        "timed": list.cues.iter().filter(|c| c.position().is_some()).count(),
        "unresolved": unresolved,
        "bpm": set.bpm_at(live.start_beats),
        "tempo_changes": if keep_tempo { 0 } else { changes },
        "tempo_set": !keep_tempo,
    }))
}

/// The Live set's tempo over `song`, in seconds from the song's start:
/// its tempo at the start, and a change wherever it differs at a beat.
#[must_use]
pub fn song_tempo(set: &LiveSet, song: &LiveSetSong) -> TempoMapData {
    let zero = set.seconds_at(song.start_beats);
    let first = set.bpm_at(song.start_beats);
    let mut points = vec![TempoChange {
        seconds: 0.0,
        bpm: first,
        meter: None,
        shape: None,
        tension: None,
    }];
    let mut last = first;
    let mut beat = song.start_beats;
    while beat < song.end_beats {
        let bpm = set.bpm_at(beat);
        if (bpm - last).abs() > 1e-6 {
            points.push(TempoChange {
                seconds: set.seconds_at(beat) - zero,
                bpm,
                meter: None,
                shape: None,
                tension: None,
            });
            last = bpm;
        }
        beat += 1.0;
    }
    TempoMapData {
        default_bpm: first,
        numerator: 4,
        denominator: 4,
        points,
    }
}

/// How long to wait before dialling a session's host again after `failed`
/// dials in a row came to nothing (0: it answered, then went): a quarter
/// second, doubling, at most five seconds. The room's renderer waits the
/// same (`ignition::session_clock::redial_after`).
#[must_use]
pub fn redial_after(failed: u32) -> std::time::Duration {
    const FIRST_MS: u64 = 250;
    const MOST_MS: u64 = 5_000;
    let doubled = FIRST_MS.saturating_mul(1_u64 << failed.min(16));
    std::time::Duration::from_millis(doubled.min(MOST_MS))
}

/// Joins session `doc`'s presence at `url` (a `ws://host:port/vox`), on a
/// thread of its own: what a view follows the transport with. Presence
/// carries the host's clock (the driver pings it into
/// [`PresencePeer::clock`]). The driver runs for as long as the process,
/// dialling the host again whenever it is not there.
///
/// # Errors
///
/// `doc` is not a session id, or the thread could not start.
pub fn follow_session(url: &str, doc: &str) -> Result<PresencePeer, String> {
    let id = uuid::Uuid::parse_str(doc).map_err(|e| e.to_string())?;
    let (peer, mut driver) = PresencePeer::new(id, 30_000);
    let url = url.to_owned();
    std::thread::Builder::new()
        .name("session-follow".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                let endpoint = Endpoint::url(&url);
                // As the room's clock: a host not up yet (a restored
                // layout) or gone and back (the engine restarting) is
                // dialled until it answers.
                let mut failed: u32 = 0;
                loop {
                    match pool().client::<DocPresenceClient>(&endpoint).await {
                        Ok(presence) => {
                            failed = 0;
                            let _ = driver.run(&presence).await;
                            pool().evict(&endpoint);
                        }
                        Err(_) => failed = failed.saturating_add(1),
                    }
                    tokio::time::sleep(redial_after(failed)).await;
                }
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(peer)
}

/// Where session `peer`'s leader is now, in session seconds, and whether
/// it plays: `None` before the clock is estimated or the leader has said.
#[must_use]
pub fn leader_now(peer: &PresencePeer) -> Option<(f64, bool)> {
    let leader = peer.states().get("lead/play").and_then(transport_of)?;
    let host_micros = peer.clock().now_ms()? * 1000.0;
    Some((leader.at(host_micros), leader.is_playing))
}

/// What the follower took.
#[derive(Default)]
struct Taken {
    log: Vec<Value>,
    clock: Option<SharedClock>,
}

fn taken() -> std::sync::MutexGuard<'static, Taken> {
    static TAKEN: OnceLock<Mutex<Taken>> = OnceLock::new();
    TAKEN
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Follows the session host's transport and takes the list's cues.
pub async fn follow(args: Value) -> Result<Value, String> {
    let url = text(&args, "url")?;
    let doc = text(&args, "doc")?;
    let lane = LaneId::from(text(&args, "lane")?.as_str());
    let ask = host()?;
    let list = load(&ask, &doc, &lane).await.map_err(|e| e.to_string())?;
    let map = tempo(&ask, &doc).await.map_err(|e| e.to_string())?;
    let mut follower = CueFollower::new(&list, &map);

    let id = uuid::Uuid::parse_str(&doc).map_err(|e| e.to_string())?;
    let presence: DocPresenceClient = pool()
        .client(&Endpoint::url(&url))
        .await
        .map_err(|e| e.to_string())?;
    let (peer, mut driver) = PresencePeer::new(id, 30_000);
    // The host's clock, estimated by the presence driver as it runs.
    let clock = peer.clock().clone();
    tokio::spawn(async move {
        let _ = driver.run(&presence).await;
    });
    {
        let mut t = taken();
        t.log.clear();
        t.clock = Some(clock.clone());
    }
    // Frames, 60 a second, off the action: the cues it takes go in the log.
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(16)).await;
            let (Some(offset), Some(leader)) = (
                clock.offset_ms().map(|ms| ms * 1000.0),
                peer.states().get("lead/play").and_then(transport_of),
            ) else {
                continue;
            };
            follower.observe(leader);
            // This process's clock the offset is to (Architect's).
            let now = clock_ms() * 1000.0;
            let take = follower.tick(now, offset);
            let at = leader.at(now + offset);
            let entry = match take {
                Take::Hold => continue,
                Take::Go(numbers) => json!({ "go": numbers, "at_seconds": at }),
                Take::Seek(number) => json!({ "seek": number, "at_seconds": at }),
            };
            taken().log.push(entry);
        }
    });
    Ok(json!({ "following": url, "cues": list.cues.len() }))
}

/// What the follower took, and the clock estimate.
pub fn report() -> Value {
    let t = taken();
    json!({
        "taken": t.log,
        "offset_us": t.clock.as_ref().and_then(SharedClock::offset_ms).map(|ms| ms * 1000.0),
        "round_trip_us": t.clock.as_ref().and_then(SharedClock::round_trip_ms).map(|ms| ms * 1000.0),
    })
}

#[cfg(test)]
mod tests {
    use super::redial_after;
    use std::time::Duration;

    #[test]
    fn a_followed_host_is_dialled_again_soon_then_every_few_seconds() {
        assert_eq!(redial_after(0), Duration::from_millis(250));
        assert_eq!(redial_after(2), Duration::from_millis(1_000));
        assert_eq!(redial_after(5), Duration::from_secs(5));
        assert_eq!(redial_after(u32::MAX), Duration::from_secs(5));
    }
}
