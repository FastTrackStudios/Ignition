//! The Live view in Safari.
//!
//! One WebSocket to the studio (`/ws` on the page's own host). The
//! first frame is the bootstrap — surface, desk banks, operator,
//! profile — and every frame after is a playhead. Commands go the
//! other way as JSON. The components are `ignition-live-ui`'s: this
//! file is the bridge and the page, nothing an operator sees.

// r[impl studio.touch.ipad] - the web host of the shared Live view
// r[impl studio.touch.presence] - the playhead the studio publishes is what this draws

use dioxus::prelude::*;
use futures_util::StreamExt;
use ignition_live_ui::live::{LIVE_CSS, Views};
use ignition_live_ui::{Bootstrap, Command, Playhead, PlayheadFeed, ServerMessage};
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{MessageEvent, WebSocket};

const BASE_CSS: &str = include_str!("base.css");
const MANIFEST: Asset = asset!("/assets/manifest.json");

fn main() {
    dioxus::launch(App);
}

thread_local! {
    /// The open socket, if any. A thread-local rather than a signal
    /// because the sender installed in `ignition_live_ui` is a plain
    /// function with no scope to read a signal from.
    static SOCKET: RefCell<Option<WebSocket>> = const { RefCell::new(None) };
}

/// What the socket's JS callbacks hand the Dioxus side. Signals are
/// written from a spawned task, never from inside a JS callback, so
/// every write happens with the runtime current.
/// Not boxed, for the same reason `ServerMessage` is not — this wraps it
/// straight off the socket, and `Message` is the whole traffic.
#[allow(clippy::large_enum_variant)]
enum Incoming {
    Open,
    Message(ServerMessage),
    Closed,
}

#[component]
fn App() -> Element {
    let mut playhead = use_signal(Playhead::default);
    use_context_provider(|| PlayheadFeed(playhead));
    let mut boot = use_signal(|| Option::<Bootstrap>::None);
    let mut online = use_signal(|| false);

    // Outgoing: `send` anywhere in the tree drops a command into this
    // channel; one task writes them to whichever socket is open.
    let (out_tx, mut out_rx) = futures_channel::mpsc::unbounded::<Command>();
    use_hook(move || {
        ignition_live_ui::install(move |command: Command| {
            let _ = out_tx.unbounded_send(command);
        });
        spawn(async move {
            while let Some(command) = out_rx.next().await {
                let text = serde_json::to_string(&command).expect("json");
                SOCKET.with(|s| {
                    if let Some(ws) = s.borrow().as_ref() {
                        let _ = ws.send_with_str(&text);
                    }
                });
            }
        });
    });

    // Incoming: connect, and reconnect for as long as the page is open.
    let (in_tx, mut in_rx) = futures_channel::mpsc::unbounded::<Incoming>();
    use_hook(move || {
        connect(in_tx.clone());
        spawn(async move {
            while let Some(event) = in_rx.next().await {
                match event {
                    Incoming::Open => online.set(true),
                    Incoming::Message(ServerMessage::Hello(b)) => {
                        if let Some(profile) = b.profile.clone() {
                            ignition_live_ui::library::install_profile(profile);
                        }
                        boot.set(Some(*b));
                    }
                    Incoming::Message(ServerMessage::Playhead(p)) => {
                        if p != playhead() {
                            playhead.set(p);
                        }
                    }
                    Incoming::Closed => {
                        online.set(false);
                        SOCKET.with(|s| *s.borrow_mut() = None);
                        // Back off a little, then try again — the studio
                        // may be restarting.
                        let tx = in_tx.clone();
                        let retry = Closure::once(move || connect(tx));
                        if let Some(window) = web_sys::window() {
                            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                                retry.as_ref().unchecked_ref(),
                                1500,
                            );
                        }
                        retry.forget();
                    }
                }
            }
        });
    });

    rsx! {
        document::Title { "Ignition Live" }
        // No pinch-zoom: a fader drag must never become a zoom, and
        // the layout is sized for the device already.
        document::Meta {
            name: "viewport",
            content: "width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no, viewport-fit=cover",
        }
        // Add to Home Screen opens fullscreen, no Safari chrome.
        document::Meta { name: "apple-mobile-web-app-capable", content: "yes" }
        document::Meta { name: "mobile-web-app-capable", content: "yes" }
        document::Meta { name: "apple-mobile-web-app-status-bar-style", content: "black-translucent" }
        document::Meta { name: "apple-mobile-web-app-title", content: "Ignition Live" }
        document::Meta { name: "theme-color", content: "#0b0b0d" }
        document::Link { rel: "manifest", href: MANIFEST }
        style { {BASE_CSS} }
        style { {LIVE_CSS} }
        div { class: "page",
            if !online() {
                div { class: "offline", "no studio — reconnecting" }
            }
            match boot() {
                Some(boot) => rsx! { ignition_live_ui::pointer::PointerRoot { Views { boot } } },
                None => rsx! { div { class: "connecting", "connecting to the studio" } },
            }
        }
    }
}

/// Open the socket to the page's own host and route its events into
/// the channel. The closures are leaked deliberately: they live as
/// long as the socket, which lives as long as the page.
fn connect(tx: futures_channel::mpsc::UnboundedSender<Incoming>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let location = window.location();
    let secure = location.protocol().is_ok_and(|p| p == "https:");
    let Ok(host) = location.host() else {
        return;
    };
    let url = format!("{}://{host}/ws", if secure { "wss" } else { "ws" });
    let Ok(ws) = WebSocket::new(&url) else {
        let _ = tx.unbounded_send(Incoming::Closed);
        return;
    };

    let on_open = {
        let tx = tx.clone();
        Closure::<dyn FnMut()>::new(move || {
            let _ = tx.unbounded_send(Incoming::Open);
        })
    };
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();

    let on_message = {
        let tx = tx.clone();
        Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            if let Some(text) = event.data().as_string()
                && let Ok(message) = serde_json::from_str::<ServerMessage>(&text)
            {
                let _ = tx.unbounded_send(Incoming::Message(message));
            }
        })
    };
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    on_message.forget();

    let on_close = {
        let tx = tx.clone();
        Closure::<dyn FnMut()>::new(move || {
            let _ = tx.unbounded_send(Incoming::Closed);
        })
    };
    ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    // An error is followed by a close; nothing to do twice.
    on_close.forget();

    SOCKET.with(|s| *s.borrow_mut() = Some(ws));
}
