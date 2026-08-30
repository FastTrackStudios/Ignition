//! The Live view, served to an iPad.
//!
//! An HTTP server in the studio process: `/` is the `ignition-live-web`
//! wasm bundle — the same Dioxus components the desk mounts, compiled
//! for the browser — and `/ws` is the wire. Commands come in as JSON
//! and go onto the one command channel, exactly as a click on the desk
//! or a MIDI fader does; the playhead the widget publishes goes out to
//! every connected client, so a fader moved on the iPad shows moved on
//! the desk and the other way round within one tick.
//!
//! Opt-in. Nothing listens unless `IGNITION_LIVE=1` or
//! `IGNITION_LIVE_PORT` is set — there is no auth, and the socket has a
//! hand on the rig. Bound to every interface, because the point is
//! another device on the venue's Wi-Fi. See `docs/ops/ipad-live.md`.

// r[impl studio.touch.ipad] - the studio serves the same components over HTTP + WS
// r[impl studio.touch.presence] - every client gets every playhead

use crate::command::{Command, Sender, StateRx};
use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use ignition_live_ui::{Bootstrap, ServerMessage, Surface};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The port when `IGNITION_LIVE_PORT` does not say otherwise.
pub const DEFAULT_PORT: u16 = 8420;

/// How often, at most, a client is sent the playhead. The desk polls
/// at 30 Hz; over Wi-Fi to a browser, 20 is plenty for a fader to
/// look alive and leaves the radio idle between moves — and a client
/// is only sent a playhead that changed at all.
const FANOUT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

/// What every connection shares.
pub struct Server {
    /// The one command channel; cloned per connection.
    tx: Mutex<Sender>,
    state: StateRx,
    boot: Bootstrap,
    /// Where the built web app is, if it has been built.
    dist: Option<PathBuf>,
}

impl Server {
    pub fn new(tx: Sender, state: StateRx, boot: Bootstrap, dist: Option<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            tx: Mutex::new(tx),
            state,
            boot,
            dist,
        })
    }
}

/// Whether the operator asked for the server at all.
fn wanted() -> bool {
    std::env::var("IGNITION_LIVE").is_ok_and(|v| v == "1")
        || std::env::var("IGNITION_LIVE_PORT").is_ok()
}

fn port() -> u16 {
    std::env::var("IGNITION_LIVE_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT)
}

/// The built bundle: `IGNITION_LIVE_DIST`, else the crate's `dist`
/// from the workspace root or beside this crate.
pub fn dist_dir() -> Option<PathBuf> {
    let candidates = [
        std::env::var("IGNITION_LIVE_DIST").ok().map(PathBuf::from),
        Some(PathBuf::from("apps/ignition-live-web/dist")),
        Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ignition-live-web/dist")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|d| d.join("index.html").is_file())
}

/// Start serving if asked to. Returns the URLs to type into Safari —
/// empty when not serving, or when the port would not bind.
pub fn start(tx: Sender, state: StateRx, surface: Surface) -> Vec<String> {
    if !wanted() {
        tracing::info!(
            "live-web: not serving; set IGNITION_LIVE=1 to serve the Live view to an iPad on port {DEFAULT_PORT}"
        );
        return Vec::new();
    }
    let port = port();
    let listener = match std::net::TcpListener::bind(("0.0.0.0", port)) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(port, error = %e, "live-web: could not bind; not serving");
            return Vec::new();
        }
    };
    let urls: Vec<String> = lan_addresses()
        .into_iter()
        .map(|ip| format!("http://{ip}:{port}"))
        .collect();
    let dist = dist_dir();
    match &dist {
        Some(d) => tracing::info!(dir = %d.display(), "live-web: serving the built web app"),
        None => tracing::warn!(
            "live-web: no built web app; `just live-web` builds it. Serving a note instead."
        ),
    }
    for url in &urls {
        tracing::info!(%url, "live-web: Live view for an iPad");
        println!("Live view for an iPad: {url}");
    }
    let boot = Bootstrap {
        surface,
        banks: ignition_live_ui::desk::load(&crate::venue_dir()),
        operator: ignition_live_ui::operators::Operator::current(),
        profile: Some(ignition_live_ui::library::profile().clone()),
        lan: urls.clone(),
    };
    let server = Server::new(tx, state, boot, dist);
    tokio::spawn(async move {
        if let Err(e) = serve(listener, server).await {
            tracing::warn!(error = %e, "live-web: server stopped");
        }
    });
    urls
}

/// Serve on an already-bound std listener until the process ends.
pub async fn serve(listener: std::net::TcpListener, server: Arc<Server>) -> std::io::Result<()> {
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    axum::serve(listener, router(server)).await
}

pub fn router(server: Arc<Server>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/ws", get(ws_upgrade))
        .fallback(get(asset))
        .with_state(server)
}

/// The machine's routable IPv4s, best effort and without a dependency:
/// the address the kernel would source a packet to the LAN from, then
/// every other local host address `/proc/net/fib_trie` lists (Linux;
/// elsewhere only the first). Loopback is left out — the iPad cannot
/// reach it.
pub fn lan_addresses() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0")
        && sock.connect("10.255.255.255:1").is_ok()
        && let Ok(addr) = sock.local_addr()
        && !addr.ip().is_loopback()
        && !addr.ip().is_unspecified()
    {
        out.push(addr.ip().to_string());
    }
    if let Ok(fib) = std::fs::read_to_string("/proc/net/fib_trie") {
        for ip in local_ipv4s_in_fib_trie(&fib) {
            if !out.contains(&ip) {
                out.push(ip);
            }
        }
    }
    out
}

/// The `/32 host LOCAL` entries of `/proc/net/fib_trie`, minus loopback.
fn local_ipv4s_in_fib_trie(fib: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lines: Vec<&str> = fib.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.contains("/32 host LOCAL") {
            continue;
        }
        let Some(prev) = i.checked_sub(1).and_then(|j| lines.get(j)) else {
            continue;
        };
        let Some(ip) = prev.split("-- ").nth(1).map(str::trim) else {
            continue;
        };
        let Ok(parsed) = ip.parse::<std::net::Ipv4Addr>() else {
            continue;
        };
        if parsed.is_loopback() || out.iter().any(|o| o == ip) {
            continue;
        }
        out.push(ip.to_string());
    }
    out
}

/// `/`: the bundle's page, or a note saying how to build it.
async fn index(State(server): State<Arc<Server>>) -> Response {
    match &server.dist {
        Some(dist) => match std::fs::read(dist.join("index.html")) {
            Ok(bytes) => Html(bytes).into_response(),
            Err(_) => Html(FALLBACK.to_string()).into_response(),
        },
        None => Html(FALLBACK.to_string()).into_response(),
    }
}

/// Anything else: a file from the bundle, by path.
async fn asset(State(server): State<Arc<Server>>, uri: axum::http::Uri) -> Response {
    let Some(dist) = &server.dist else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let rel = uri.path().trim_start_matches('/');
    // No `..`: the bundle is the only thing served.
    if rel.split('/').any(|seg| seg == "..") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = dist.join(rel);
    let Ok(bytes) = std::fs::read(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        Some("json") | Some("webmanifest") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, mime)], bytes).into_response()
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(server): State<Arc<Server>>) -> Response {
    ws.on_upgrade(move |socket| connection(socket, server))
}

/// One client: the bootstrap, then the playhead whenever it changes,
/// while every command it sends goes down the channel.
async fn connection(mut socket: WebSocket, server: Arc<Server>) {
    let hello = ServerMessage::Hello(Box::new(server.boot.clone()));
    if socket
        .send(Message::Text(
            serde_json::to_string(&hello).expect("json").into(),
        ))
        .await
        .is_err()
    {
        return;
    }
    let tx = server.tx.lock().expect("sender mutex").clone();
    let mut state = server.state.clone();
    // The first playhead unconditionally, so a client that connects
    // mid-song draws the right thing before anything moves.
    state.mark_changed();
    loop {
        tokio::select! {
            changed = state.changed() => {
                if changed.is_err() {
                    break;
                }
                let playhead = state.borrow_and_update().clone();
                let text = serde_json::to_string(&ServerMessage::Playhead(playhead)).expect("json");
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
                tokio::time::sleep(FANOUT_INTERVAL).await;
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => match serde_json::from_str::<Command>(&text) {
                        Ok(command) => {
                            let _ = tx.send(command);
                        }
                        Err(e) => tracing::warn!(error = %e, "live-web: bad command"),
                    },
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
        }
    }
}

/// What `/` says until the web app has been built.
const FALLBACK: &str = r#"<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Ignition Live</title>
<body style="background:#0b0b0d;color:#e6e6e6;font-family:sans-serif;padding:2em">
<h1>Ignition Live</h1>
<p>The studio is serving, but the web app has not been built.</p>
<p>On the studio machine run <code>just live-web</code>, then reload this page.</p>
</body>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use ignition_live_ui::Playhead;
    use tokio_tungstenite::tungstenite;

    fn boot() -> Bootstrap {
        Bootstrap {
            surface: Surface::default(),
            banks: Vec::new(),
            operator: ignition_live_ui::operators::Operator::starter("test"),
            profile: None,
            lan: vec!["http://10.0.0.9:8420".into()],
        }
    }

    /// The whole loop in one process: a client connects, is told the
    /// bootstrap, sends a command that lands on the studio's channel,
    /// and sees the playhead the engine publishes.
    // r[impl studio.touch.ipad] - the wire works end to end
    // r[impl studio.touch.presence] - a published playhead reaches the client
    ///
    /// The types on the wire are the desk's own — `Command` in,
    /// `ServerMessage::Playhead` out, both from `ignition_live_ui` —
    /// so there is no second touch UI to keep in step with this one and
    /// no translation layer to disagree at.
    ///
    /// r[verify studio.touch.ipad]
    #[tokio::test]
    async fn a_client_sends_commands_and_receives_the_playhead() {
        let (tx, rx) = crate::command::channel();
        let (state_tx, state_rx) = crate::command::state_channel();
        let server = Server::new(tx, state_rx, boot(), None);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(listener, server));

        let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
            .await
            .expect("connect");

        // Hello first.
        let hello = ws.next().await.unwrap().unwrap().into_text().unwrap();
        let ServerMessage::Hello(boot) = serde_json::from_str(&hello).unwrap() else {
            panic!("first message is the bootstrap: {hello}");
        };
        assert_eq!(boot.lan, vec!["http://10.0.0.9:8420".to_string()]);
        // Then the current playhead, even before anything moved.
        let first = ws.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(matches!(
            serde_json::from_str(&first).unwrap(),
            ServerMessage::Playhead(_)
        ));

        // A command goes down the studio's channel, as the desk's would.
        let json = serde_json::to_string(&Command::Level(2, 0.75)).unwrap();
        ws.send(tungstenite::Message::Text(json.into()))
            .await
            .unwrap();
        let got =
            tokio::task::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(5)))
                .await
                .unwrap()
                .expect("command arrived");
        assert!(matches!(got, Command::Level(2, v) if (v - 0.75).abs() < 1e-6));

        // The engine publishes; the client sees it.
        state_tx.send_modify(|p| {
            p.cue = Some(4);
            p.grand = 0.5;
        });
        let text = ws.next().await.unwrap().unwrap().into_text().unwrap();
        let ServerMessage::Playhead(playhead) = serde_json::from_str(&text).unwrap() else {
            panic!("a playhead: {text}");
        };
        assert_eq!(playhead.cue, Some(4));
        assert_eq!(playhead.grand, 0.5);
        let _ = Playhead::default();
    }

    /// Two clients, one rig: what one moves, the other sees.
    ///
    /// This is the whole of "presence" and it is not free — a server
    /// that kept one connection's playhead, or fanned out only to the
    /// client whose command caused the change, would pass every
    /// single-client test above and still leave the desk and the iPad
    /// showing different faders. Which is worse than either being
    /// wrong, because the operator then has no way to tell which one
    /// the rig is actually on.
    ///
    /// r[verify studio.touch.presence]
    #[tokio::test]
    async fn two_clients_see_the_same_state_within_a_tick() {
        let (tx, rx) = crate::command::channel();
        let (state_tx, state_rx) = crate::command::state_channel();
        let server = Server::new(tx, state_rx, boot(), None);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(listener, server));

        let mut clients = Vec::new();
        for _ in 0..2 {
            let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/ws"))
                .await
                .expect("connect");
            // Hello, then the playhead as it stands.
            let hello = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert!(matches!(
                serde_json::from_str(&hello).unwrap(),
                ServerMessage::Hello(_)
            ));
            let first = ws.next().await.unwrap().unwrap().into_text().unwrap();
            assert!(matches!(
                serde_json::from_str(&first).unwrap(),
                ServerMessage::Playhead(_)
            ));
            clients.push(ws);
        }

        // The first client moves a fader. It goes down the one command
        // channel, exactly as a click on the desk would.
        let json = serde_json::to_string(&Command::Level(2, 0.75)).unwrap();
        clients[0]
            .send(tungstenite::Message::Text(json.into()))
            .await
            .unwrap();
        let got =
            tokio::task::spawn_blocking(move || rx.recv_timeout(std::time::Duration::from_secs(5)))
                .await
                .unwrap()
                .expect("the command reached the studio");
        assert!(matches!(got, Command::Level(2, v) if (v - 0.75).abs() < 1e-6));

        // The studio applies it and publishes one playhead. Both
        // clients see it — including the one that did not send it, and
        // including the one that did, which is what keeps a client from
        // trusting its own optimistic copy.
        state_tx.send_modify(|p| {
            p.cue = Some(7);
            p.grand = 0.75;
        });
        for (i, ws) in clients.iter_mut().enumerate() {
            let text = tokio::time::timeout(std::time::Duration::from_secs(5), ws.next())
                .await
                .unwrap_or_else(|_| panic!("client {i} was never told"))
                .unwrap()
                .unwrap()
                .into_text()
                .unwrap();
            let ServerMessage::Playhead(playhead) = serde_json::from_str(&text).unwrap() else {
                panic!("client {i}: a playhead, not {text}");
            };
            assert_eq!(playhead.cue, Some(7), "client {i}");
            assert_eq!(playhead.grand, 0.75, "client {i}");
        }
    }

    /// Without a built bundle, `/` explains itself rather than 404ing.
    #[tokio::test]
    async fn the_index_says_how_to_build_when_there_is_no_bundle() {
        let (tx, _rx) = crate::command::channel();
        let (_state_tx, state_rx) = crate::command::state_channel();
        let server = Server::new(tx, state_rx, boot(), None);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(serve(listener, server));
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut body = String::new();
        stream.read_to_string(&mut body).await.unwrap();
        assert!(body.starts_with("HTTP/1.1 200"));
        assert!(body.contains("just live-web"));
    }

    #[test]
    fn fib_trie_local_hosts_are_found() {
        let fib = "Main:\n  +-- 0.0.0.0/0 3 0 5\n     |-- 127.0.0.1\n        /32 host LOCAL\n     |-- 192.168.1.20\n        /32 host LOCAL\n     |-- 192.168.1.255\n        /32 link BROADCAST\n";
        assert_eq!(
            local_ipv4s_in_fib_trie(fib),
            vec!["192.168.1.20".to_string()]
        );
    }
}
