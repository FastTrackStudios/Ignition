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
use crdt::sync::{DocPresenceClient, PresencePeer};
use ignition_session::{Ask, AskFuture, CueFollower, Take, load, store, tempo};
use ignition_show::show_file::ShowDocument;
use serde_json::{Value, json};
use session::LaneId;
use session::clock::{SessionClockClient, SharedClock};
use session::doc::transport_of;
use session::sync::clock::now_micros_f64;

/// `fts.session` through the host, as this extension.
struct Host(Handle);

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
    let map = tempo(&host()?, &doc).await.map_err(|e| e.to_string())?;
    let clock = format!("session:{url}#{doc}@{song}");
    crate::ext::set_setting("ignition.clock", &clock).await?;
    Ok(json!({
        "clock": clock,
        "tempo_changes": map.points().len(),
        "bpm_at_start": map.bpm_at(0.0),
    }))
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

    let clock = SharedClock::default();
    let pinger = clock.clone();
    let host_clock: SessionClockClient = pool()
        .client(&Endpoint::url(&url))
        .await
        .map_err(|e| e.to_string())?;
    tokio::spawn(async move {
        let _ = pinger.run(&host_clock).await;
    });
    let id = uuid::Uuid::parse_str(&doc).map_err(|e| e.to_string())?;
    let presence: DocPresenceClient = pool()
        .client(&Endpoint::url(&url))
        .await
        .map_err(|e| e.to_string())?;
    let (peer, mut driver) = PresencePeer::new(id, 30_000);
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
                clock.offset_micros(),
                peer.states().get("lead/play").and_then(transport_of),
            ) else {
                continue;
            };
            follower.observe(leader);
            let now = now_micros_f64();
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
        "offset_us": t.clock.as_ref().and_then(SharedClock::offset_micros),
        "round_trip_us": t.clock.as_ref().and_then(SharedClock::round_trip_micros),
    })
}
