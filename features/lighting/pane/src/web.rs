//! The room in a browser: Ignition's web renderer (`ignition_viz::run_web`,
//! Bevy on WebGPU) in a canvas of the page's, following the session the
//! way the desktop's renderer does.
//!
//! Natively the room is a program (`ignition --process-view`) the app
//! runs; a browser runs no programs, so here the visualizer is in the
//! page, compiled into the app's wasm with this extension. One room per
//! page: Bevy's event loop is made once, so the canvas is made once too,
//! kept, and moved into whichever view shows the room (`show`, or
//! studio-web's `lighting`).
//!
//! What it plays:
//!
//! - **a session** (`ignition.session`, `<url>#<doc>@<lane>`, which
//!   `IGNITION_SESSION_ROOM` sets): the lane's cue list read through the
//!   host's `fts.session` as this extension (`ignition_session::load`),
//!   the session's tempo map, and its host's transport followed through
//!   presence (`crate::session::follow_session`): every frame the song's
//!   player is put where the session's playhead is, as the desktop's
//!   `--clock session:…` does;
//! - **a cue list handed in** (`IGNITION_ROOM_CUES`), stepped through by
//!   `IGNITION_ROOM_GO` and `IGNITION_ROOM_CUE`, or a press on a cue of the
//!   `cues` view: the room as an operator's, with no session.
//!
//! The room is the recital's (`data/venues/room138-cbu`, the default show's
//! venue), its documents compiled in; the visualizer's models are served
//! beside the app (`ignition_viz::WEB_ASSETS`).

use std::cell::{Cell, RefCell};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use crdt::sync::PresencePeer;
use dioxus::prelude::*;
use ignition_core::{Class, CueList, CuePlayer, Show};
use ignition_viz::Venue;
use ignition_viz::playback::Playback;
use ignition_viz::venue::VenueFiles;
use serde_json::{Value, json};
use session::LaneId;
use session::TempoMap;
use web_sys::wasm_bindgen::JsCast as _;

use crate::cues::Place;

/// The canvas the room is drawn in, and the element of a view it sits in.
const CANVAS: &str = "ignition-room";
const HOST: &str = "ignition-room-host";

/// The recital's room, as the desk reads it from its directory.
mod room {
    pub const FIXTURES: &str = include_str!("../../data/venues/room138-cbu/fixtures.json");
    pub const ROOM: &str = include_str!("../../data/venues/room138-cbu/room.json");
    pub const SCREENS: &str = include_str!("../../data/venues/room138-cbu/screens.json");
    pub const PROPS: &str = include_str!("../../data/venues/room138-cbu/props.json");
    pub const GROUPS: &str = include_str!("../../data/venues/room138-cbu/groups.json");
    pub const PALETTES: &str = include_str!("../../data/venues/room138-cbu/palettes.json");
    pub const BINDING: &str = include_str!("../../data/venues/room138-cbu/profile.json");
    /// The profile the room implements (its named tricks, its looks).
    pub const PROFILE: &str = include_str!("../../data/profiles/ignition.ig-profile");
}

/// The recital's room, from its documents.
fn venue() -> Result<Venue, String> {
    Venue::from_files(&VenueFiles {
        fixtures: room::FIXTURES.to_owned(),
        room: room::ROOM.to_owned(),
        screens: room::SCREENS.to_owned(),
        props: room::PROPS.to_owned(),
        groups: Some(room::GROUPS.to_owned()),
        palettes: Some(room::PALETTES.to_owned()),
        profile: Some(room::BINDING.to_owned()),
        areas: None,
        profile_document: Some(room::PROFILE.to_owned()),
        // A page sends no DMX and keeps no local layer.
        dmx: None,
        layer: None,
    })
    .map_err(|e| e.to_string())
}

/// What the page asks of the room, taken at the start of a frame.
enum Ask {
    /// Play this list (the song's player starts on it).
    List(CueList),
    /// To the end of this cue's fade.
    Cue(usize),
    /// The next cue.
    Go,
}

/// What the room is doing, as of its last frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Now {
    /// The list's name.
    pub name: String,
    /// Its cues: number and name, in GO order.
    pub cues: Vec<(f32, String)>,
    /// The cue standing.
    pub cue: Option<usize>,
}

/// The page's end of the visualizer (`ignition_viz::WebDesk`).
#[derive(Default)]
struct Desk {
    asks: Mutex<Vec<Ask>>,
    now: Mutex<Now>,
}

impl Desk {
    fn ask(&self, ask: Ask) {
        self.asks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(ask);
    }
}

fn desk() -> Arc<Desk> {
    static DESK: OnceLock<Arc<Desk>> = OnceLock::new();
    Arc::clone(DESK.get_or_init(Arc::default))
}

/// The song's player, with the show it plays against (created empty
/// when the room has none yet).
fn with_song(playback: &mut Playback, take: impl FnOnce(&mut CuePlayer, &Show<'_>)) {
    let Playback {
        playbacks,
        groups,
        rig,
        palettes,
        speeds,
        profile,
        library,
        bundles,
        looks,
        named_tricks,
        focus_overrides,
        ..
    } = playback;
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
        ..Show::new(&*groups, &*rig)
    };
    if playbacks.of_class(Class::Song).is_none() {
        playbacks.push(Class::Song, CuePlayer::new(Vec::new()));
    }
    if let Some(player) = playbacks.of_class(Class::Song) {
        take(player, &show);
    }
}

impl ignition_viz::WebDesk for Desk {
    fn drain(&self, playback: &mut Playback) {
        let asks = std::mem::take(&mut *self.asks.lock().unwrap_or_else(PoisonError::into_inner));
        for ask in asks {
            match ask {
                Ask::List(list) => {
                    playback.triggers = ignition_core::TriggerBus::new(list.triggers.clone());
                    with_song(playback, |player, show| player.replace_list(&list, show));
                    let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
                    now.name.clone_from(&list.name);
                    now.cues = list
                        .cues
                        .iter()
                        .enumerate()
                        .map(|(i, cue)| (list.number_of(i), cue.name.clone()))
                        .collect();
                }
                Ask::Cue(index) => with_song(playback, |player, show| {
                    player.jump_to_end_of(index, show);
                }),
                Ask::Go => with_song(playback, CuePlayer::go),
            }
        }
        // The session's playhead, when the room follows one.
        FOLLOWING.with(|f| {
            if let Some(following) = f.borrow_mut().as_mut() {
                following.frame(playback);
            }
        });
    }

    fn publish(&self, playback: &Playback) {
        let cue = playback.song().and_then(CuePlayer::current_index);
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        if now.cue != cue {
            now.cue = cue;
        }
    }
}

/// What the room is doing now.
#[must_use]
pub fn now() -> Now {
    desk()
        .now
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// Takes cue `index` of the room's list (to the end of its fade).
pub fn take(index: usize) {
    desk().ask(Ask::Cue(index));
}

/// The session the room follows: its list's lane, its tempo, its host's
/// transport through presence.
struct Following {
    peer: PresencePeer,
    tempo: TempoMap,
    /// Where the playhead was last frame, and whether it played.
    last: Option<(f64, bool)>,
}

impl Following {
    /// The song's player where the session's playhead is, as the desktop's
    /// session clock puts it (its time, then a seek to its bar).
    fn frame(&mut self, playback: &mut Playback) {
        let Some((seconds, playing)) = crate::session::leader_now(&self.peer) else {
            return;
        };
        // Stopped where it stopped: nothing moves, and a cue taken by hand
        // meanwhile stands.
        if !playing
            && self
                .last
                .is_some_and(|(at, was)| !was && (at - seconds).abs() < 1e-6)
        {
            return;
        }
        self.last = Some((seconds, playing));
        let position = ignition_session::bars_of(&self.tempo, session::Time::Seconds(seconds));
        #[allow(clippy::cast_possible_truncation, clippy::as_conversions)]
        let secs = seconds as f32;
        with_song(playback, |player, show| {
            player.set_clock(secs);
            player.seek(position, show);
        });
    }
}

thread_local! {
    /// The session followed, if any (presence is the page's, never sent).
    static FOLLOWING: RefCell<Option<Following>> = const { RefCell::new(None) };
    /// Whether the visualizer runs (it is started once a page).
    static STARTED: Cell<bool> = const { Cell::new(false) };
    /// The room's canvas, kept while no view shows it.
    static CANVAS_EL: RefCell<Option<web_sys::HtmlCanvasElement>> = const { RefCell::new(None) };
    /// Why the room is not drawn, if it is not.
    static FAILED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The room's canvas, made the first time.
fn canvas(document: &web_sys::Document) -> Result<web_sys::HtmlCanvasElement, String> {
    if let Some(canvas) = CANVAS_EL.with(|c| c.borrow().clone()) {
        return Ok(canvas);
    }
    let canvas: web_sys::HtmlCanvasElement = document
        .create_element("canvas")
        .map_err(|_| "the page would not make a canvas".to_owned())?
        .dyn_into()
        .map_err(|_| "the page would not make a canvas".to_owned())?;
    canvas.set_id(CANVAS);
    let _ = canvas.set_attribute(
        "style",
        "width: 100%; height: 100%; display: block; outline: none; touch-action: none;",
    );
    let _ = canvas.set_attribute("tabindex", "0");
    CANVAS_EL.with(|c| *c.borrow_mut() = Some(canvas.clone()));
    Ok(canvas)
}

/// Puts the room's canvas in the view's element (moving it from a view
/// that showed it before).
fn attach() -> Result<(), String> {
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let host = document
        .get_element_by_id(HOST)
        .ok_or("the room's view is not in the page yet")?;
    let canvas = canvas(&document)?;
    if canvas.parent_element().as_ref() != Some(&host) {
        host.append_child(&canvas)
            .map_err(|_| "the page would not take the room's canvas".to_owned())?;
    }
    Ok(())
}

/// Draws the room in the view's element: the visualizer started the first
/// time, the canvas moved in after.
async fn show_room() -> Result<(), String> {
    attach()?;
    if let Some(why) = FAILED.with(|f| f.borrow().clone()) {
        return Err(why);
    }
    if STARTED.get() {
        return Ok(());
    }
    STARTED.set(true);
    let started = start().await;
    if let Err(why) = &started {
        tracing::warn!(lighting.room = "refused", lighting.why = %why, "lighting: the room is not drawn");
        FAILED.with(|f| *f.borrow_mut() = Some(why.clone()));
    }
    started
}

async fn start() -> Result<(), String> {
    webgpu().await?;
    let venue = venue().map_err(|e| format!("the room did not read: {e}"))?;
    let assets = ignition_viz::WEB_ASSETS.to_string();
    let desk: Arc<dyn ignition_viz::WebDesk> = desk();
    ignition_viz::run_web(
        venue,
        &ignition_viz::WebShow {
            cuelist: None,
            profile: Some(room::PROFILE),
            song: None,
        },
        &format!("#{CANVAS}"),
        &assets,
        Some(desk),
    );
    // Detached: it follows the setting for as long as the page.
    drop(architect::platform::spawn(follow_the_session()));
    Ok(())
}

/// A browser that gives no WebGPU adapter draws a black room: say why
/// instead (the visualizer is WebGPU, not WebGL 2).
async fn webgpu() -> Result<(), String> {
    use web_sys::wasm_bindgen::JsValue;
    const NO_WEBGPU: &str = "This browser has no WebGPU, which the room needs.";
    const NO_ADAPTER: &str = "This browser has WebGPU but gave no adapter (on Linux, Chrome needs chrome://flags/#enable-unsafe-webgpu).";
    let window = web_sys::window().ok_or("no window")?;
    let gpu = js_sys::Reflect::get(&window.navigator(), &JsValue::from_str("gpu"))
        .map_err(|_| NO_WEBGPU.to_owned())?;
    if gpu.is_undefined() || gpu.is_null() {
        return Err(NO_WEBGPU.to_owned());
    }
    let request = js_sys::Reflect::get(&gpu, &JsValue::from_str("requestAdapter"))
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| NO_WEBGPU.to_owned())?;
    let promise = request
        .call0(&gpu)
        .ok()
        .and_then(|p| p.dyn_into::<js_sys::Promise>().ok())
        .ok_or_else(|| NO_ADAPTER.to_owned())?;
    let adapter = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|_| NO_ADAPTER.to_owned())?;
    if adapter.is_null() || adapter.is_undefined() {
        return Err(NO_ADAPTER.to_owned());
    }
    Ok(())
}

/// The setting naming the session, read once a second; another session
/// is read and followed afresh.
async fn follow_the_session() {
    let mut followed: Option<Place> = None;
    loop {
        let place = crate::cues::setting().await;
        if place != followed {
            followed.clone_from(&place);
            FOLLOWING.with(|f| *f.borrow_mut() = None);
            if let Some(place) = place {
                match follow(&place).await {
                    Ok(following) => FOLLOWING.with(|f| *f.borrow_mut() = Some(following)),
                    Err(why) => tracing::warn!(
                        lighting.session = "refused",
                        lighting.why = %why,
                        "lighting: the room does not follow the session"
                    ),
                }
            }
        }
        architect::platform::sleep(Duration::from_secs(1)).await;
    }
}

/// The session's list and tempo through `fts.session` as this extension
/// (the calls the cue list view and the desktop make), and its transport.
async fn follow(place: &Place) -> Result<Following, String> {
    let ask = crate::session::host_ask()?;
    let lane = LaneId::from(place.lane.as_str());
    let list = ignition_session::load(&ask, &place.doc, &lane)
        .await
        .map_err(|e| e.to_string())?;
    let tempo = ignition_session::tempo(&ask, &place.doc)
        .await
        .map_err(|e| e.to_string())?;
    let peer = crate::session::follow_session(&place.url, &place.doc)?;
    desk().ask(Ask::List(list));
    Ok(Following {
        peer,
        tempo,
        last: None,
    })
}

/// `IGNITION_ROOM_CUES {cues}`: the room plays this cue list (a song's cue
/// file, as an object or its text), standing before its first cue.
///
/// # Errors
///
/// `cues` is missing or is not a cue list.
pub async fn hand_cues(args: Value) -> Result<Value, String> {
    let cues = args.get("cues").ok_or("`cues` is required")?;
    let list: CueList = match cues {
        Value::String(text) => serde_json::from_str(text),
        other => serde_json::from_value(other.clone()),
    }
    .map_err(|e| format!("`cues` is not a cue list: {e}"))?;
    let reply = json!({ "name": list.name, "cues": list.cues.len() });
    desk().ask(Ask::List(list));
    Ok(reply)
}

/// `IGNITION_ROOM_CUE {index | number}`: the room to the end of that cue.
///
/// # Errors
///
/// Neither is given, or the room's list has no cue of that number.
pub async fn take_cue(args: Value) -> Result<Value, String> {
    let index = match (
        args.get("index").and_then(Value::as_u64),
        args.get("number").and_then(Value::as_f64),
    ) {
        (Some(index), _) => usize::try_from(index).map_err(|e| e.to_string())?,
        (None, Some(number)) => now()
            .cues
            .iter()
            .position(|(n, _)| (f64::from(*n) - number).abs() < 1e-3)
            .ok_or_else(|| format!("the room's list has no cue {number}"))?,
        (None, None) => return Err("`index` or `number` is required".to_owned()),
    };
    take(index);
    Ok(json!({ "cue": index }))
}

/// `IGNITION_ROOM_GO`: the room's next cue.
///
/// # Errors
///
/// None: it is asked of the room, which takes it at its next frame.
pub async fn go(_args: Value) -> Result<Value, String> {
    desk().ask(Ask::Go);
    Ok(json!({ "go": true }))
}

/// `IGNITION_ROOM_STATE`: what the room is doing.
#[must_use]
pub fn state() -> Value {
    let now = now();
    json!({
        "drawn": STARTED.get() && FAILED.with(|f| f.borrow().is_none()),
        "failed": FAILED.with(|f| f.borrow().clone()),
        "following": FOLLOWING.with(|f| f.borrow().is_some()),
        "name": now.name,
        "cues": now.cues.len(),
        "cue": now.cue,
    })
}

/// The room, in the view's element.
#[component]
pub fn Room() -> Element {
    let mut said = use_signal(|| Some("Lighting: starting the room…".to_owned()));
    use_future(move || async move {
        // The view's element is in the page once this has rendered.
        architect::platform::sleep(Duration::from_millis(0)).await;
        match show_room().await {
            Ok(()) => said.set(None),
            Err(why) => said.set(Some(why)),
        }
    });
    rsx! {
        div { style: "flex: 1; position: relative; display: flex; width: 100%; height: 100%; min-height: 320px; min-width: 0; background: #000;",
            div { id: HOST, style: "position: absolute; inset: 0;" }
            if let Some(why) = said() {
                div { style: "position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; padding: 20px; text-align: center; color: #aaa; font-size: 13px;",
                    "{why}"
                }
            }
        }
    }
}

/// The room's own list (one handed in, or the session's as the room
/// plays it), each cue a press that takes it: the cue list view's, with
/// no session to read.
#[component]
pub fn RoomCues() -> Element {
    let mut shown = use_signal(now);
    use_future(move || async move {
        loop {
            let at = now();
            if *shown.peek() != at {
                shown.set(at);
            }
            architect::platform::sleep(Duration::from_millis(100)).await;
        }
    });
    let at = shown();
    if at.cues.is_empty() {
        return rsx! {
            div { style: "flex: 1; display: flex; align-items: center; justify-content: center; padding: 20px; color: var(--muted-foreground); font-size: 13px; text-align: center;",
                "No cues yet: IGNITION_SESSION_ROOM points the room at a session, IGNITION_ROOM_CUES hands it a cue list."
            }
        };
    }
    rsx! {
        div { style: "flex: 1; display: flex; flex-direction: column; min-height: 0; min-width: 0; font-size: 13px;",
            div { style: "padding: 10px 12px; border-bottom: 1px solid var(--border); font-weight: 600; font-size: 14px;",
                "{at.name}"
            }
            div { style: "flex: 1; min-height: 0; overflow-y: scroll;",
                for (i, (number, name)) in at.cues.iter().enumerate() {
                    div {
                        key: "{i}",
                        "data-cue": "{i}",
                        style: if at.cue == Some(i) { "display: flex; gap: 10px; padding: 5px 12px; cursor: pointer; background: var(--accent); color: var(--accent-foreground); font-weight: 600;" } else { "display: flex; gap: 10px; padding: 5px 12px; cursor: pointer;" },
                        onclick: move |_| take(i),
                        span { style: "width: 34px; font-family: monospace;", "{number}" }
                        span { style: "flex: 1; min-width: 0; overflow: hidden;", "{name}" }
                    }
                }
            }
        }
    }
}
