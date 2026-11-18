//! Ignition in a session: cue lists kept in the session primitive
//! (FastTrackStudios/session), so a lighting show lives in the same song
//! as the chart, the lyrics and the DAW's tracks, and follows the same
//! transport.
//!
//! The session knows nothing about lighting. Ignition stores what it needs
//! as its own data there, through `fts.session` (the host's service, asked
//! through an [`Ask`]), and owns its logic:
//!
//! | Ignition                  | in the session |
//! |---------------------------|----------------|
//! | a cue list                | a lane, kind `ignition.cues`, owned by `ignition` |
//! | a cue with a position     | an item on that lane, kind `ignition.cue`, at its bar in musical time; the whole cue as the item's payload |
//! | a cue with no position    | a key of Ignition's map entry `untimed:<lane>`, by cue number |
//! | the list's own settings   | Ignition's register `list:<lane>` (name, wrap, restart, triggers) |
//!
//! A cue's bar is the session's bar: [`store`] places it with the
//! session's tempo map, [`load`] reads it back from the item's time, so a
//! tempo edit anywhere moves every cue with the song.
//!
//! **Two ways to take cues, as Ignition already has them** (a cue's doc:
//! "`at` is what a clock uses; order is what a person pressing GO uses;
//! both land in the same state"):
//!
//! - **By hand**: [`load`] returns the list in cue-number order, timed and
//!   untimed cues together, and `ignition_show::CuePlayer::go` steps it.
//!   Which cue is live is the player's, not the session's.
//! - **By the clock**: a [`CueFollower`] follows the session's transport
//!   (`session::Transport`, through `session::sync::Tracker`) and says
//!   which cues the playhead crossed ([`Take::Go`]), or which cue's state
//!   holds where it jumped to ([`Take::Seek`]). Untimed cues are never the
//!   clock's.

use std::collections::{BTreeMap, BTreeSet};

use ignition_daw_proto::Bars;
use ignition_show::{Cue, CueList};
use session::extension::{call_json, response_of};
use session::service::{Request, Response};
use session::sync::{Schedule, Tracker};
use session::{Entry, ExtensionId, Item, ItemId, Lane, LaneId, TempoMap, Time, Transport, Value};

/// Ignition's namespace in a session: its Architect extension id.
pub const EXTENSION: &str = "ignition";
/// A cue list's lane kind.
pub const LANE_KIND: &str = "ignition.cues";
/// A timed cue's item kind.
pub const CUE_KIND: &str = "ignition.cue";

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the session refused: {0}")]
    Session(String),
    #[error("a cue's data: {0}")]
    Data(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Asks `fts.session` on the host: `method` (`read`, `write`) with
/// `args` (JSON), answering JSON. In an Ignition extension this is the
/// host's service call (`HostRequest::Service`); in a host that holds
/// the session, `session::extension::answer` as `ignition`.
pub trait Ask {
    /// One call.
    ///
    /// # Errors
    ///
    /// The host refused it or it failed.
    fn ask(&self, method: &str, args: &str) -> std::result::Result<String, String>;
}

impl<F> Ask for F
where
    F: Fn(&str, &str) -> std::result::Result<String, String>,
{
    fn ask(&self, method: &str, args: &str) -> std::result::Result<String, String> {
        self(method, args)
    }
}

fn call(ask: &impl Ask, doc: &str, request: Request) -> Result<Response> {
    let method = session::service::action_for(&request);
    let args = call_json(doc, request).map_err(Error::Session)?;
    let reply = ask.ask(method, &args).map_err(Error::Session)?;
    response_of(&reply).map_err(Error::Session)
}

/// The session's tempo map, read through the service.
///
/// # Errors
///
/// The service refused, or its tempo map does not parse.
pub fn tempo(ask: &impl Ask, doc: &str) -> Result<TempoMap> {
    match call(ask, doc, Request::ReadStructure)? {
        Response::Structure { tempo, .. } => tempo
            .to_tempo_map()
            .map_err(|e| Error::Session(e.to_string())),
        other => Err(Error::Session(format!("not the structure: {other:?}"))),
    }
}

/// Where bar `bars` is in the session, in musical time.
#[must_use]
pub fn time_of(tempo: &TempoMap, bars: Bars) -> Time {
    let beat = bars.beat.max(1.0);
    let whole = beat.floor();
    tempo.at_bar(
        i32::try_from(bars.bar).unwrap_or(i32::MAX),
        // A beat number fits an i32: a bar has at most a few dozen.
        whole_beat(whole),
        beat - whole,
    )
}

#[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
const fn whole_beat(beat: f64) -> i32 {
    beat as i32
}

/// The bar a session time is at.
#[must_use]
pub fn bars_of(tempo: &TempoMap, at: Time) -> Bars {
    let bb = at.bar_beat(tempo);
    Bars::new(
        u32::try_from(bb.bar).unwrap_or(1),
        f64::from(bb.beat) + bb.fraction,
    )
}

/// The item a timed cue is kept as: `<lane>/<number>`.
#[must_use]
pub fn item_id(lane: &LaneId, number: f32) -> ItemId {
    ItemId(format!("{lane}/{number}"))
}

fn list_key(lane: &LaneId) -> String {
    format!("list:{lane}")
}

fn untimed_key(lane: &LaneId) -> String {
    format!("untimed:{lane}")
}

/// Stores `list` in session `doc` as lane `lane`: the lane, a timed item
/// per cue with a position, the untimed cues and the list's settings.
/// Cues it held before that `list` no longer has are removed.
///
/// # Errors
///
/// The service refused, or a cue would not encode.
pub fn store(ask: &impl Ask, doc: &str, lane: &LaneId, list: &CueList) -> Result<()> {
    let tempo = tempo(ask, doc)?;
    let ignition = ExtensionId::from(EXTENSION);
    call(
        ask,
        doc,
        Request::PutLane {
            id: lane.clone(),
            lane: Lane {
                owner: ignition.clone(),
                kind: LANE_KIND.to_owned(),
                name: list.name.clone(),
                order: 0.0,
                parent: None,
            },
        },
    )?;
    let mut timed = BTreeSet::new();
    let mut untimed = BTreeMap::new();
    for (index, cue) in list.cues.iter().enumerate() {
        let number = list.number_of(index);
        let mut cue = cue.clone();
        cue.number = Some(number);
        let payload = to_value(&cue)?;
        if let Some(bars) = cue.position() {
            let id = item_id(lane, number);
            call(
                ask,
                doc,
                Request::PutItem {
                    id: id.clone(),
                    item: Item {
                        owner: ignition.clone(),
                        lane: lane.clone(),
                        kind: CUE_KIND.to_owned(),
                        at: time_of(&tempo, bars),
                        end: None,
                        payload,
                    },
                },
            )?;
            timed.insert(id);
        } else {
            untimed.insert(number.to_string(), payload);
        }
    }
    // What this lane held that the list no longer has.
    if let Response::Timeline { items, .. } = call(ask, doc, Request::ReadTimeline)? {
        for (id, item) in items {
            if &item.lane == lane && item.owner == ignition && !timed.contains(&id) {
                call(ask, doc, Request::RemoveItem { id })?;
            }
        }
    }
    let held = read_data(ask, doc)?
        .map(&untimed_key(lane))
        .cloned()
        .unwrap_or_default();
    for key in held.keys().filter(|k| !untimed.contains_key(*k)) {
        call(
            ask,
            doc,
            Request::MapRemove {
                name: untimed_key(lane),
                key: key.clone(),
            },
        )?;
    }
    for (key, value) in untimed {
        call(
            ask,
            doc,
            Request::MapSet {
                name: untimed_key(lane),
                key,
                value,
            },
        )?;
    }
    let mut head = list.clone();
    head.cues.clear();
    call(
        ask,
        doc,
        Request::Set {
            name: list_key(lane),
            value: to_value(&head)?,
        },
    )?;
    Ok(())
}

fn read_data(ask: &impl Ask, doc: &str) -> Result<session::ExtensionData> {
    match call(ask, doc, Request::ReadData)? {
        Response::Data(data) => Ok(data),
        other => Err(Error::Session(format!("not Ignition's data: {other:?}"))),
    }
}

/// The cue list stored as lane `lane` of session `doc`, in cue-number
/// order (GO order). A timed cue's bar is read from its item's time
/// under the session's tempo map, so it is where the song now has it.
///
/// # Errors
///
/// The service refused, or a cue would not decode.
pub fn load(ask: &impl Ask, doc: &str, lane: &LaneId) -> Result<CueList> {
    let tempo = tempo(ask, doc)?;
    let data = read_data(ask, doc)?;
    let mut list: CueList = match data.get(&list_key(lane)) {
        Some(Entry::Register(head)) => from_value(head)?,
        _ => CueList::default(),
    };
    let mut cues: Vec<Cue> = Vec::new();
    if let Response::Timeline { items, .. } = call(ask, doc, Request::ReadTimeline)? {
        for item in items.values() {
            if &item.lane != lane || item.owner.as_str() != EXTENSION || item.kind != CUE_KIND {
                continue;
            }
            let mut cue: Cue = from_value(&item.payload)?;
            cue.resolved = Some(bars_of(&tempo, item.at));
            cues.push(cue);
        }
    }
    if let Some(untimed) = data.map(&untimed_key(lane)) {
        for payload in untimed.values() {
            cues.push(from_value(payload)?);
        }
    }
    cues.sort_by(|a, b| {
        a.number
            .unwrap_or(f32::MAX)
            .total_cmp(&b.number.unwrap_or(f32::MAX))
    });
    list.cues = cues;
    Ok(list)
}

/// What the clock says to do now.
#[derive(Debug, Clone, PartialEq)]
pub enum Take {
    /// Nothing.
    Hold,
    /// The playhead crossed these cues, in order: take each.
    Go(Vec<f32>),
    /// The playhead jumped (play from elsewhere, a seek, a stop): the
    /// state of this cue holds here (`None`: before the first).
    Seek(Option<f32>),
}

/// Takes a cue list's timed cues from the session's transport.
#[derive(Debug, Clone)]
pub struct CueFollower {
    tracker: Tracker,
    schedule: Schedule<f32>,
    shown: Option<f64>,
    playing: bool,
}

/// A step longer than this between two ticks is a jump, not playing.
const JUMP_SECONDS: f64 = 0.25;

/// Makes a crossing include a cue exactly at the playhead now
/// (`Schedule::between` excludes its end).
const CROSSING_EPSILON: f64 = 1e-9;

impl CueFollower {
    /// Follows `list`'s timed cues, placed with the session's `tempo`.
    #[must_use]
    pub fn new(list: &CueList, tempo: &TempoMap) -> Self {
        let events = list
            .cues
            .iter()
            .enumerate()
            .filter_map(|(index, cue)| {
                cue.position()
                    .map(|bars| (time_of(tempo, bars).seconds(tempo), list.number_of(index)))
            })
            .collect();
        Self {
            tracker: Tracker::default(),
            schedule: Schedule::new(events),
            shown: None,
            playing: false,
        }
    }

    /// The leader's transport, as presence brought it (shared clock).
    pub fn observe(&mut self, transport: Transport) {
        self.tracker.observe(transport);
    }

    /// What to take at `local_micros` (this clock), `offset_micros` being
    /// the shared clock minus this one. Call once per frame.
    pub fn tick(&mut self, local_micros: f64, offset_micros: f64) -> Take {
        let Some(now) = self.tracker.position(local_micros, offset_micros) else {
            return Take::Hold;
        };
        let playing = self.tracker.leader().is_some_and(|t| t.is_playing);
        let was = self.shown.replace(now);
        let was_playing = std::mem::replace(&mut self.playing, playing);
        let seek = || Take::Seek(self.schedule.last_at_or_before(now).map(|(_, n)| *n));
        match was {
            Some(before)
                if playing && was_playing && now >= before && now - before < JUMP_SECONDS =>
            {
                // Crossed: after where the last frame (or a seek) left the
                // playhead, up to and including now. A cue exactly where
                // the playhead stood was taken then.
                let crossed: Vec<f32> = self
                    .schedule
                    .between(before, now + CROSSING_EPSILON)
                    .iter()
                    .filter(|(at, _)| *at > before)
                    .map(|(_, n)| *n)
                    .collect();
                if crossed.is_empty() {
                    Take::Hold
                } else {
                    Take::Go(crossed)
                }
            }
            Some(before) if !playing && !was_playing && (now - before).abs() < 1e-9 => Take::Hold,
            _ => seek(),
        }
    }
}

/// A cue (or a list head) as a session value, through its JSON.
///
/// # Errors
///
/// It would not encode.
pub fn to_value<T: serde::Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value)
        .map(|json| value_of_json(&json))
        .map_err(|e| Error::Data(e.to_string()))
}

/// A session value back as a cue (or a list head).
///
/// # Errors
///
/// It is not one.
pub fn from_value<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T> {
    serde_json::from_value(json_of_value(value)).map_err(|e| Error::Data(e.to_string()))
}

fn value_of_json(json: &serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(*b),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map_or_else(|| Value::Float(n.as_f64().unwrap_or(0.0)), Value::Int),
        serde_json::Value::String(s) => Value::Text(s.clone()),
        serde_json::Value::Array(a) => Value::List(a.iter().map(value_of_json).collect()),
        serde_json::Value::Object(o) => Value::Map(
            o.iter()
                .map(|(k, v)| (k.clone(), value_of_json(v)))
                .collect(),
        ),
    }
}

fn json_of_value(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::from(*i),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Text(s) => serde_json::Value::String(s.clone()),
        Value::Bytes(b) => serde_json::Value::from(b.clone()),
        Value::List(l) => serde_json::Value::Array(l.iter().map(json_of_value).collect()),
        Value::Map(m) => serde_json::Value::Object(
            m.iter()
                .map(|(k, v)| (k.clone(), json_of_value(v)))
                .collect(),
        ),
    }
}
