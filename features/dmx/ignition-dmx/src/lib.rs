//! The DMX transport: one socket set, and a frame out of it per universe
//! at a bounded rate.
//!
//! The facade of the `dmx` feature. It owns the sockets, the rate
//! limiting and the Art-Net poll/reply loop; it owns no packet layout.
//! Every byte on the wire is built by a backend
//! (`ignition-dmx-sacn`, `ignition-dmx-artnet`), which is what lets a
//! universe carry either protocol, or both, without this file knowing
//! how either is framed.
//!
//! ```text
//! encoder frame: HashMap<u16, [u8; 512]>
//!        │
//!        ▼
//!   Sender::send(&frame, now)      per universe: changed? due? keep-alive?
//!        ├── sacn::data_packet ──▶ multicast 239.255.x.y:5568 / unicast
//!        ├── artnet::art_dmx ────▶ broadcast :6454 / unicast
//!        └── Sink::frame(universe, &bytes)   the visualizer's loopback
//! ```
//!
//! The contract all three crates agree on -- `OutputConfig`, `Sink`,
//! `Status` -- is `ignition-dmx-proto`, re-exported below so a consumer
//! needs one dependency rather than two.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub use ignition_dmx_artnet::PortAddress;
pub use ignition_dmx_proto::*;

/// The backends, re-exported under the names this crate's own code and
/// its tests have always used them by.
pub use ignition_dmx_artnet as artnet;
pub use ignition_dmx_sacn as sacn;

// ---------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------

/// Decides, per universe, whether a frame goes out now: a changed frame
/// no sooner than `1/max_hz` after the last, an unchanged one no later
/// than `1/keepalive_hz` after the last.
// r[impl dmx.rate]
#[derive(Debug, Clone)]
pub struct RateLimiter {
    min_interval: Duration,
    keepalive: Duration,
    last_sent: Option<Instant>,
    last_frame: Option<[u8; 512]>,
    pending: bool,
}

impl RateLimiter {
    #[must_use]
    pub fn new(max_hz: f32, keepalive_hz: f32) -> Self {
        let max_hz = if max_hz.is_finite() && max_hz > 0.0 {
            max_hz
        } else {
            default_max_hz()
        };
        let keepalive_hz = if keepalive_hz.is_finite() && keepalive_hz > 0.0 {
            keepalive_hz
        } else {
            default_keepalive_hz()
        };
        Self {
            min_interval: Duration::from_secs_f32(1.0 / max_hz),
            keepalive: Duration::from_secs_f32(1.0 / keepalive_hz),
            last_sent: None,
            last_frame: None,
            pending: false,
        }
    }

    /// Should `frame` be sent at `now`? Call once per encoder tick;
    /// returns `true` at most `max_hz` times a second and at least
    /// `keepalive_hz` times a second once anything has been sent.
    pub fn should_send(&mut self, frame: &[u8; 512], now: Instant) -> bool {
        let changed = self.last_frame.as_ref() != Some(frame);
        // A change we could not send yet stays owed until the window opens,
        // even if the very next frame happens to equal the last one sent.
        self.pending |= changed;
        let send = match self.last_sent {
            None => true,
            Some(last) => {
                let since = now.saturating_duration_since(last);
                (self.pending && since >= self.min_interval) || since >= self.keepalive
            }
        };
        if send {
            self.last_sent = Some(now);
            self.last_frame = Some(*frame);
            self.pending = false;
        }
        send
    }

    /// Forget the last frame so the next tick sends regardless.
    pub const fn reset(&mut self) {
        self.last_sent = None;
        self.last_frame = None;
        self.pending = false;
    }
}

// ---------------------------------------------------------------------
// Sender
// ---------------------------------------------------------------------

/// Where the sockets bind. The defaults are what a real rig wants; tests
/// bind loopback on ephemeral ports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindAddrs {
    pub sacn: SocketAddr,
    pub artnet: SocketAddr,
}

impl Default for BindAddrs {
    fn default() -> Self {
        Self {
            sacn: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            artnet: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), artnet::ARTNET_PORT),
        }
    }
}

struct UniverseState {
    limiter: RateLimiter,
    sacn_seq: u8,
    artnet_seq: u8,
    sent_at: VecDeque<Instant>,
    last_sent: Option<Instant>,
}

struct Inner {
    universes: HashMap<u16, UniverseState>,
    sink: Option<Box<dyn Sink>>,
    errors: Vec<String>,
    /// The first error per (universe, protocol) target, so a dead node
    /// does not flood the list every tick.
    reported: std::collections::HashSet<String>,
}

/// The transmit side. One per engine; `Clone`-free, share it in an `Arc`.
pub struct Sender {
    config: OutputConfig,
    source_name: String,
    cid: [u8; 16],
    sacn_socket: Option<UdpSocket>,
    artnet_socket: Option<Arc<UdpSocket>>,
    enabled: AtomicBool,
    stopped: AtomicBool,
    inner: Mutex<Inner>,
}

impl Sender {
    /// Open the sockets and get ready to send. Never panics and never
    /// fails: a socket that would not bind is reported in [`Sender::status`],
    /// and the other protocol still runs.
    // r[impl dmx.output-toggle]
    #[must_use]
    pub fn bind(config: &OutputConfig, source_name: &str) -> Self {
        Self::bind_at(config.clone(), source_name, BindAddrs::default())
    }

    /// [`Sender::bind`] with explicit bind addresses.
    #[must_use]
    pub fn bind_at(config: OutputConfig, source_name: &str, addrs: BindAddrs) -> Self {
        let mut errors = Vec::new();
        let wants_sacn = config.universes.values().any(|u| u.sacn.is_some());
        let wants_artnet = config.universes.values().any(|u| u.artnet.is_some());

        let sacn_socket = if wants_sacn {
            match UdpSocket::bind(addrs.sacn) {
                Ok(s) => {
                    let _ = s.set_multicast_ttl_v4(4);
                    let _ = s.set_multicast_loop_v4(true);
                    Some(s)
                }
                Err(e) => {
                    errors.push(format!("sACN: bind {} failed: {e}", addrs.sacn));
                    None
                }
            }
        } else {
            None
        };

        let artnet_socket = if wants_artnet {
            match UdpSocket::bind(addrs.artnet).or_else(|first| {
                // 6454 is taken (often by our own visualizer's listener).
                // We can still *send* from any port; we just will not hear
                // ArtPoll, so say so.
                let fallback = SocketAddr::new(addrs.artnet.ip(), 0);
                UdpSocket::bind(fallback).inspect(|_| {
                    errors.push(format!(
                        "Art-Net: bind {} failed ({first}); sending from an ephemeral port, \
                         ArtPoll discovery will not be answered",
                        addrs.artnet
                    ));
                })
            }) {
                Ok(s) => {
                    if let Err(e) = s.set_broadcast(true) {
                        errors.push(format!("Art-Net: set_broadcast failed: {e}"));
                    }
                    let _ = s.set_read_timeout(Some(Duration::from_millis(250)));
                    Some(Arc::new(s))
                }
                Err(e) => {
                    errors.push(format!("Art-Net: bind failed: {e}"));
                    None
                }
            }
        } else {
            None
        };

        let universes = config
            .universes
            .iter()
            .filter(|(_, u)| u.sacn.is_some() || u.artnet.is_some())
            .map(|(n, u)| {
                (
                    *n,
                    UniverseState {
                        limiter: RateLimiter::new(u.max_hz, u.keepalive_hz),
                        sacn_seq: 0,
                        artnet_seq: 0,
                        sent_at: VecDeque::new(),
                        last_sent: None,
                    },
                )
            })
            .collect();

        Self {
            cid: stable_cid(source_name),
            config,
            source_name: source_name.to_string(),
            sacn_socket,
            artnet_socket,
            enabled: AtomicBool::new(true),
            stopped: AtomicBool::new(false),
            inner: Mutex::new(Inner {
                universes,
                sink: None,
                errors,
                reported: std::collections::HashSet::default(),
            }),
        }
    }

    /// Attach the loopback: every universe sent also lands here, as the
    /// same 512 bytes the packets carried.
    // r[impl dmx.loopback]
    #[must_use]
    pub fn with_sink(self, sink: Box<dyn Sink>) -> Self {
        self.set_sink(Some(sink));
        self
    }

    pub fn set_sink(&self, sink: Option<Box<dyn Sink>>) {
        self.lock().sink = sink;
    }

    pub const fn config(&self) -> &OutputConfig {
        &self.config
    }

    pub fn source_name(&self) -> &str {
        &self.source_name
    }

    /// The component identifier every sACN packet from this process carries.
    pub const fn cid(&self) -> &[u8; 16] {
        &self.cid
    }

    /// The desk toggle. Off means nothing leaves the socket — not even
    /// keep-alives — and the next enable resends every universe at once.
    // r[impl dmx.output-toggle]
    pub fn set_enabled(&self, enabled: bool) {
        let was = self.enabled.swap(enabled, Ordering::SeqCst);
        if enabled && !was {
            for u in self.lock().universes.values_mut() {
                u.limiter.reset();
            }
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst) && !self.stopped.load(Ordering::SeqCst)
    }

    /// Offer the encoder's frame. Each configured universe present in
    /// `frame` goes out if its rate limiter says so; the rest wait.
    // r[impl dmx.one-frame]
    // r[impl dmx.rate]
    // r[impl dmx.sequence]
    // The lock genuinely needs to stay held for the whole per-universe loop
    // below: a sequence number, `sent_at` and the loopback sink all have to
    // move together per frame, and dropping it mid-loop to placate the lint
    // would let a concurrent `send`/`stop` interleave universes.
    #[expect(
        clippy::significant_drop_tightening,
        reason = "the lock is held for the whole per-universe update on purpose; see the comment above"
    )]
    pub fn send(&self, frame: &HashMap<u16, [u8; 512]>, now: Instant) {
        if !self.is_enabled() {
            return;
        }
        let mut inner = self.lock();
        let inner = &mut *inner;
        for (number, cfg) in &self.config.universes {
            if !cfg.enabled {
                continue;
            }
            let Some(data) = frame.get(number) else {
                continue;
            };
            let Some(state) = inner.universes.get_mut(number) else {
                continue;
            };
            if !state.limiter.should_send(data, now) {
                continue;
            }
            if let (Some(sacn), Some(sock)) = (&cfg.sacn, &self.sacn_socket) {
                let pkt = sacn::data_packet(
                    &self.cid,
                    &self.source_name,
                    sacn.priority,
                    state.sacn_seq,
                    *number,
                    data,
                );
                state.sacn_seq = state.sacn_seq.wrapping_add(1);
                send_sacn(
                    sock,
                    sacn,
                    *number,
                    &pkt,
                    &mut inner.errors,
                    &mut inner.reported,
                );
            }
            if let (Some(art), Some(sock)) = (&cfg.artnet, &self.artnet_socket) {
                let pkt = artnet::art_dmx(state.artnet_seq, PortAddress::from(art), data);
                // Art-Net's sequence is 1..=255; 0 means "not used".
                state.artnet_seq = match state.artnet_seq.wrapping_add(1) {
                    0 => 1,
                    n => n,
                };
                send_artnet(
                    sock,
                    art,
                    *number,
                    &pkt,
                    &mut inner.errors,
                    &mut inner.reported,
                );
            }
            state.last_sent = Some(now);
            state.sent_at.push_back(now);
            while let Some(t) = state.sent_at.front() {
                if now.saturating_duration_since(*t) > Duration::from_secs(1) {
                    state.sent_at.pop_front();
                } else {
                    break;
                }
            }
            // The loopback sees the frame whether or not a socket took it:
            // the visualizer must show the bytes the engine *meant* to send.
            if let Some(sink) = inner.sink.as_mut() {
                sink.frame(*number, data);
            }
        }
    }

    /// Leave every sACN universe: the stream-terminated frame, three
    /// times, so nodes release it at once. Output is disabled afterwards;
    /// `set_enabled(true)` starts it again.
    // r[impl dmx.sacn.addressing]
    // Same reasoning as `send`: the lock spans the whole per-universe
    // terminate loop so a concurrent caller cannot see a half-terminated set.
    #[expect(
        clippy::significant_drop_tightening,
        reason = "the lock is held for the whole per-universe terminate loop on purpose"
    )]
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        let mut inner = self.lock();
        let inner = &mut *inner;
        let Some(sock) = &self.sacn_socket else {
            return;
        };
        for (number, cfg) in &self.config.universes {
            let (Some(sacn), Some(state)) = (&cfg.sacn, inner.universes.get_mut(number)) else {
                continue;
            };
            if state.last_sent.is_none() {
                continue; // never claimed, nothing to release
            }
            for _ in 0..3 {
                let pkt = sacn::terminate_packet(
                    &self.cid,
                    &self.source_name,
                    sacn.priority,
                    state.sacn_seq,
                    *number,
                );
                state.sacn_seq = state.sacn_seq.wrapping_add(1);
                send_sacn(
                    sock,
                    sacn,
                    *number,
                    &pkt,
                    &mut inner.errors,
                    &mut inner.reported,
                );
            }
            state.limiter.reset();
        }
    }

    /// Start again after [`Sender::stop`].
    pub fn restart(&self) {
        self.stopped.store(false, Ordering::SeqCst);
        self.set_enabled(true);
    }

    /// What the desk shows.
    // r[impl dmx.output-toggle]
    pub fn status(&self) -> Status {
        let inner = self.lock();
        let has_socket = self.sacn_socket.is_some() || self.artnet_socket.is_some();
        let per_universe = self
            .config
            .universes
            .iter()
            .filter(|(_, u)| u.enabled)
            .map(|(n, u)| {
                let mut protocols = Vec::new();
                if u.sacn.is_some() && self.sacn_socket.is_some() {
                    protocols.push(Protocol::Sacn);
                }
                if u.artnet.is_some() && self.artnet_socket.is_some() {
                    protocols.push(Protocol::Artnet);
                }
                let state = inner.universes.get(n);
                UniverseStatus {
                    universe: *n,
                    protocols,
                    last_sent: state.and_then(|s| s.last_sent),
                    hz: state.map_or(0.0, |s| sent_count_as_hz(s.sent_at.len())),
                }
            })
            .collect::<Vec<_>>();
        Status {
            enabled: self.is_enabled(),
            sending: self.is_enabled()
                && has_socket
                && per_universe.iter().any(|u| !u.protocols.is_empty()),
            per_universe,
            errors: inner.errors.clone(),
        }
    }

    /// The address a poll reply should advertise: the Art-Net socket's
    /// own, or the unspecified address when it is not bound to one.
    fn advertised_ip(&self) -> Ipv4Addr {
        match self
            .artnet_socket
            .as_ref()
            .and_then(|s| s.local_addr().ok())
        {
            Some(SocketAddr::V4(a)) if !a.ip().is_unspecified() => *a.ip(),
            _ => Ipv4Addr::UNSPECIFIED,
        }
    }

    /// The `ArtPollReply` packets that describe this source: one per
    /// (net, sub-net) page of up to four universes.
    // r[impl dmx.artnet.addressing]
    pub fn poll_replies(&self) -> Vec<Vec<u8>> {
        let mut pages: BTreeMap<(u8, u8), Vec<u8>> = BTreeMap::new();
        for cfg in self.config.universes.values().filter(|u| u.enabled) {
            if let Some(a) = &cfg.artnet {
                let pa = PortAddress::from(a);
                pages
                    .entry((pa.net, pa.subnet))
                    .or_default()
                    .push(pa.universe);
            }
        }
        let ip = self.advertised_ip();
        let mut out = Vec::new();
        let mut bind_index = 1u8;
        for ((net, subnet), universes) in pages {
            for chunk in universes.chunks(4) {
                out.push(artnet::art_poll_reply(&artnet::PollReply {
                    ip,
                    short_name: "Ignition".into(),
                    long_name: format!("Ignition - {}", self.source_name),
                    node_report: format!("#0001 [{bind_index:04}] Ignition"),
                    net,
                    subnet,
                    universes: chunk.to_vec(),
                    bind_index,
                }));
                bind_index = bind_index.wrapping_add(1).max(1);
            }
        }
        out
    }

    /// Read whatever is waiting on the Art-Net socket (up to the socket's
    /// read timeout, 250 ms) and answer each `ArtPoll`. Returns how many
    /// polls were answered. Drive this from a thread, or use
    /// [`Sender::poll_reply_loop`].
    // r[impl dmx.artnet.addressing]
    pub fn respond_to_polls(&self) -> usize {
        let Some(sock) = &self.artnet_socket else {
            return 0;
        };
        let mut buf = [0u8; 1024];
        let mut answered: usize = 0;
        loop {
            match sock.recv_from(&mut buf) {
                Ok((len, from)) => {
                    // `recv_from` never returns a length past the buffer it
                    // filled, but the data is still bytes off the wire, and
                    // `get` costs nothing here for the guarantee.
                    let Some(bytes) = buf.get(..len) else {
                        continue;
                    };
                    if let Some(artnet::Packet::Poll(_)) = artnet::parse(bytes) {
                        let target = artnet::reply_target(from);
                        for reply in self.poll_replies() {
                            if let Err(e) = sock.send_to(&reply, target) {
                                self.lock()
                                    .errors
                                    .push(format!("Art-Net: poll reply to {target} failed: {e}"));
                            }
                        }
                        answered = answered.saturating_add(1);
                    }
                    // Anything else on the port (another controller's
                    // ArtDmx, replies) is not ours to handle.
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    return answered;
                }
                Err(e) => {
                    self.lock()
                        .errors
                        .push(format!("Art-Net: recv failed: {e}"));
                    return answered;
                }
            }
        }
    }

    /// Answer polls until `stop()` is called. Meant for a dedicated thread:
    /// `let s = sender.clone(); thread::spawn(move || s.poll_reply_loop());`
    pub fn poll_reply_loop(&self) {
        while !self.stopped.load(Ordering::SeqCst) {
            if self.artnet_socket.is_none() {
                return;
            }
            self.respond_to_polls();
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// `sent_at.len()` — how many frames went out in the trailing second — as
/// a rate for [`Status`]. Bounded above by whatever `max_hz` a venue
/// config can hold (nothing sane runs a universe past a few hundred Hz),
/// nowhere near where an `f32` mantissa starts dropping counts.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "sent_at.len() is bounded by realistic frame rates; see the doc comment"
)]
const fn sent_count_as_hz(count: usize) -> f32 {
    count as f32
}

fn record(
    errors: &mut Vec<String>,
    reported: &mut std::collections::HashSet<String>,
    key: String,
    msg: String,
) {
    if reported.insert(key) {
        errors.push(msg);
    }
}

/// `i` is the enumerate index of a 16-byte CID's 8-byte halves — always 0
/// or 1 — widened to a fixed-width type so the hash salt (and so the CID
/// itself) does not change between a 32-bit and a 64-bit build.
#[expect(
    clippy::as_conversions,
    reason = "usize -> u64 is infallible on every target this ships to; see the doc comment"
)]
const fn chunk_salt(i: usize) -> u64 {
    i as u64
}

fn send_sacn(
    sock: &UdpSocket,
    cfg: &SacnOutput,
    universe: u16,
    pkt: &[u8],
    errors: &mut Vec<String>,
    reported: &mut std::collections::HashSet<String>,
) -> bool {
    let mut ok = false;
    if cfg.multicast {
        let target = sacn::multicast_addr(universe);
        match sock.send_to(pkt, target) {
            Ok(_) => ok = true,
            Err(e) => record(
                errors,
                reported,
                format!("sacn/{universe}/multicast"),
                format!("sACN: universe {universe} multicast to {target} failed: {e}"),
            ),
        }
    }
    for target in &cfg.unicast {
        match sock.send_to(pkt, target) {
            Ok(_) => ok = true,
            Err(e) => record(
                errors,
                reported,
                format!("sacn/{universe}/{target}"),
                format!("sACN: universe {universe} unicast to {target} failed: {e}"),
            ),
        }
    }
    ok
}

fn send_artnet(
    sock: &UdpSocket,
    cfg: &ArtnetOutput,
    universe: u16,
    pkt: &[u8],
    errors: &mut Vec<String>,
    reported: &mut std::collections::HashSet<String>,
) -> bool {
    let mut ok = false;
    if cfg.broadcast {
        let target = SocketAddr::new(IpAddr::V4(cfg.broadcast_addr), artnet::ARTNET_PORT);
        match sock.send_to(pkt, target) {
            Ok(_) => ok = true,
            Err(e) => record(
                errors,
                reported,
                format!("artnet/{universe}/broadcast"),
                format!("Art-Net: universe {universe} broadcast to {target} failed: {e}"),
            ),
        }
    }
    for target in &cfg.unicast {
        match sock.send_to(pkt, target) {
            Ok(_) => ok = true,
            Err(e) => record(
                errors,
                reported,
                format!("artnet/{universe}/{target}"),
                format!("Art-Net: universe {universe} unicast to {target} failed: {e}"),
            ),
        }
    }
    ok
}

/// A CID that is the same every time this machine runs this source name.
///
/// So a node sees one source across restarts (E1.31 §6.2.3 wants the CID
/// to persist), and different for two Ignitions on two machines.
// r[impl dmx.sacn.priority]
#[must_use]
pub fn stable_cid(source_name: &str) -> [u8; 16] {
    let host = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .unwrap_or_default();
    let host = host.trim();
    let mut out = [0u8; 16];
    for (i, chunk) in out.chunks_mut(8).enumerate() {
        let mut h = DefaultHasher::new();
        // Two independent halves from the same inputs; the salt makes
        // them differ.
        (chunk_salt(i), "ignition-io", host, source_name).hash(&mut h);
        chunk.copy_from_slice(&h.finish().to_be_bytes());
    }
    // Mark it as a version-4 (random-style) UUID with the RFC variant so
    // tools that pretty-print CIDs are not confused.
    out[6] = (out[6] & 0x0f) | 0x40;
    out[8] = (out[8] & 0x3f) | 0x80;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn cfg_json() -> &'static str {
        r#"{ "universes": { "1": { "sacn": { "priority": 100 }, "artnet": { "net": 0, "subnet": 0, "universe": 0 } } } }"#
    }

    /// r[verify dmx.venue-config]
    #[test]
    fn config_deserialises_the_venue_shape_with_defaults_and_round_trips() {
        let cfg: OutputConfig = serde_json::from_str(cfg_json()).unwrap();
        let u = &cfg.universes[&1];
        assert!(u.enabled);
        // Exact equality is right here: these came straight from serde's
        // `#[serde(default)]` constants, not through any float arithmetic.
        assert!((u.max_hz - 44.0).abs() < f32::EPSILON);
        assert!((u.keepalive_hz - 1.0).abs() < f32::EPSILON);
        let s = u.sacn.as_ref().unwrap();
        assert_eq!(s.priority, 100);
        assert!(s.multicast);
        assert!(s.unicast.is_empty());
        let a = u.artnet.as_ref().unwrap();
        assert_eq!(PortAddress::from(a), PortAddress::new(0, 0, 0));
        assert!(a.broadcast);
        assert_eq!(a.broadcast_addr, Ipv4Addr::BROADCAST);

        let json = serde_json::to_string(&cfg).unwrap();
        let back: OutputConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cfg);

        // From a manifest's flattened extras.
        let mut extra = serde_json::Map::new();
        extra.insert("dmx".into(), serde_json::from_str(cfg_json()).unwrap());
        assert_eq!(OutputConfig::from_venue_extra(&extra).unwrap(), cfg);
        assert!(
            !OutputConfig::from_venue_extra(&serde_json::Map::default())
                .unwrap()
                .has_output()
        );
    }

    /// r[verify dmx.rate]
    #[test]
    fn rate_limiter_caps_at_44_hz_and_keeps_alive_at_1_hz() {
        let mut rl = RateLimiter::new(44.0, 1.0);
        let t0 = Instant::now();
        let a = [1u8; 512];
        let b = [2u8; 512];
        assert!(rl.should_send(&a, t0), "first frame always goes");
        assert!(
            !rl.should_send(&b, t0 + Duration::from_millis(5)),
            "changed but too soon"
        );
        assert!(
            !rl.should_send(&b, t0 + Duration::from_millis(20)),
            "still inside 1/44 s"
        );
        assert!(
            rl.should_send(&b, t0 + Duration::from_millis(23)),
            "window open, change owed"
        );
        // Unchanged: nothing until the keep-alive.
        let t1 = t0 + Duration::from_millis(23);
        assert!(!rl.should_send(&b, t1 + Duration::from_millis(500)));
        assert!(!rl.should_send(&b, t1 + Duration::from_millis(999)));
        assert!(
            rl.should_send(&b, t1 + Duration::from_secs(1)),
            "keep-alive at 1 Hz"
        );

        // Over one simulated second of a changing frame, at most 44 go out.
        let mut rl = RateLimiter::new(44.0, 1.0);
        let mut sent = 0;
        for i in 0..1000u64 {
            let mut f = [0u8; 512];
            // Same wrap `as u8` gave: only distinctness across frames
            // matters here, not the exact byte.
            f[0] = u8::try_from(i % 256).unwrap_or(0);
            if rl.should_send(&f, t0 + Duration::from_millis(i)) {
                sent += 1;
            }
        }
        assert!((40..=45).contains(&sent), "sent {sent} in 1 s at 44 Hz");
    }

    #[test]
    fn a_change_that_arrived_too_soon_is_not_lost_when_the_frame_reverts() {
        let mut rl = RateLimiter::new(44.0, 1.0);
        let t0 = Instant::now();
        assert!(rl.should_send(&[0; 512], t0));
        assert!(!rl.should_send(&[9; 512], t0 + Duration::from_millis(1)));
        // The encoder is back to the old bytes, but a receiver that missed
        // the blip is still in sync, so nothing is owed... except we said
        // pending stays owed. Either is defensible; we choose to resend so a
        // node that *did* see the blip is guaranteed to see the revert.
        assert!(rl.should_send(&[0; 512], t0 + Duration::from_millis(30)));
    }

    #[test]
    fn cid_is_stable_for_a_name_and_differs_between_names() {
        let a = stable_cid("Ignition");
        assert_eq!(a, stable_cid("Ignition"));
        assert_ne!(a, stable_cid("Other"));
        assert_eq!(a[6] >> 4, 4, "version nibble");
        assert_eq!(a[8] & 0xc0, 0x80, "variant bits");
    }

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    fn local_receiver() -> (UdpSocket, SocketAddr) {
        let s = UdpSocket::bind(loopback(0)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let addr = s.local_addr().unwrap();
        (s, addr)
    }

    fn test_sender(sacn_to: SocketAddr, artnet_to: SocketAddr) -> Sender {
        let mut cfg = OutputConfig::default();
        cfg.universes.insert(
            1,
            UniverseOutput {
                sacn: Some(SacnOutput {
                    priority: 120,
                    unicast: vec![sacn_to],
                    multicast: false,
                }),
                artnet: Some(ArtnetOutput {
                    net: 0,
                    subnet: 1,
                    universe: 2,
                    unicast: vec![artnet_to],
                    broadcast: false,
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        Sender::bind_at(
            cfg,
            "Ignition test",
            BindAddrs {
                sacn: loopback(0),
                artnet: loopback(0),
            },
        )
    }

    fn recv(s: &UdpSocket) -> Vec<u8> {
        let mut buf = [0u8; 1024];
        let (n, _) = s.recv_from(&mut buf).expect("a packet within the timeout");
        buf[..n].to_vec()
    }

    /// r[verify dmx.protocols]
    /// r[verify dmx.one-frame]
    /// r[verify dmx.sacn.priority]
    /// r[verify dmx.sequence]
    /// r[verify dmx.loopback]
    #[test]
    fn what_is_sent_on_loopback_is_what_the_receiver_parses_and_what_the_sink_saw() {
        let (sacn_rx, sacn_addr) = local_receiver();
        let (art_rx, art_addr) = local_receiver();
        let (tx, sink_rx) = mpsc::channel::<(u16, [u8; 512])>();
        let sender =
            test_sender(sacn_addr, art_addr).with_sink(Box::new(move |u: u16, d: &[u8; 512]| {
                tx.send((u, *d)).unwrap();
            }));
        let st = sender.status();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!(st.sending);
        assert_eq!(
            st.per_universe[0].protocols,
            vec![Protocol::Sacn, Protocol::Artnet]
        );

        let mut data = [0u8; 512];
        data[0] = 200;
        data[511] = 17;
        let mut frame = HashMap::new();
        frame.insert(1u16, data);
        frame.insert(7u16, [3u8; 512]); // unconfigured: must not be sent
        let t0 = Instant::now();
        sender.send(&frame, t0);

        let s = sacn::parse(&recv(&sacn_rx)).unwrap();
        assert_eq!(s.cid, *sender.cid());
        assert_eq!(s.source_name, "Ignition test");
        assert_eq!(s.priority, 120);
        assert_eq!(s.sequence, 0);
        assert_eq!(s.universe, 1);
        assert_eq!(s.data, data.to_vec());

        match artnet::parse(&recv(&art_rx)).unwrap() {
            artnet::Packet::Dmx {
                sequence,
                port_address,
                data: d,
            } => {
                assert_eq!(sequence, 0);
                assert_eq!(port_address, PortAddress::new(0, 1, 2));
                assert_eq!(d, data.to_vec());
            }
            other => panic!("{other:?}"),
        }
        let (u, d) = sink_rx.recv().unwrap();
        assert_eq!((u, d), (1, data));
        assert!(sink_rx.try_recv().is_err(), "universe 7 is not configured");

        // Second frame, changed, after the rate window: sequence advances.
        data[1] = 1;
        frame.insert(1, data);
        sender.send(&frame, t0 + Duration::from_millis(30));
        assert_eq!(sacn::parse(&recv(&sacn_rx)).unwrap().sequence, 1);
        let st = sender.status();
        assert!((st.per_universe[0].hz - 2.0).abs() < f32::EPSILON);
        assert!(st.per_universe[0].last_sent.is_some());
    }

    /// r[verify dmx.sequence]
    #[test]
    fn sequence_numbers_wrap_at_255() {
        let (sacn_rx, sacn_addr) = local_receiver();
        let (art_rx, art_addr) = local_receiver();
        let sender = test_sender(sacn_addr, art_addr);
        let t0 = Instant::now();
        let mut frame = HashMap::new();
        let mut last_sacn = 0u8;
        let mut last_art = 0u8;
        for i in 0..260u32 {
            let mut d = [0u8; 512];
            // Same wrap `as u8` gave for the low byte; `i` never exceeds
            // 260 so the high byte fits `u8` without truncating anything.
            d[0] = u8::try_from(i % 256).unwrap_or(0);
            d[1] = u8::try_from(i >> 8).unwrap_or(0);
            frame.insert(1u16, d);
            sender.send(&frame, t0 + Duration::from_millis(u64::from(i) * 25));
            last_sacn = sacn::parse(&recv(&sacn_rx)).unwrap().sequence;
            if let Some(artnet::Packet::Dmx { sequence, .. }) = artnet::parse(&recv(&art_rx)) {
                last_art = sequence;
            }
        }
        // 260 sACN sends: 0..=255 then 0,1,2,3.
        assert_eq!(last_sacn, 3);
        // Art-Net skips 0 after wrapping: 0..=255 then 1,2,3,4.
        assert_eq!(last_art, 4);
    }

    /// r[verify dmx.sacn.addressing]
    /// r[verify dmx.output-toggle]
    #[test]
    fn stop_terminates_three_times_and_the_toggle_silences_output() {
        let (sacn_rx, sacn_addr) = local_receiver();
        let (_art_rx, art_addr) = local_receiver();
        let sender = test_sender(sacn_addr, art_addr);
        let mut frame = HashMap::new();
        frame.insert(1u16, [5u8; 512]);
        let t0 = Instant::now();

        sender.set_enabled(false);
        assert!(!sender.status().sending);
        sender.send(&frame, t0);
        assert!(
            sacn_rx.recv_from(&mut [0u8; 1024]).is_err(),
            "disabled sends nothing"
        );

        sender.set_enabled(true);
        sender.send(&frame, t0 + Duration::from_secs(1));
        assert!(!sacn::parse(&recv(&sacn_rx)).unwrap().is_terminated());

        sender.stop();
        for i in 0..3 {
            let p = sacn::parse(&recv(&sacn_rx)).unwrap();
            assert!(p.is_terminated(), "terminate #{i}");
            assert_eq!(p.sequence, 1 + i);
            assert_eq!(p.universe, 1);
        }
        assert!(
            sacn_rx.recv_from(&mut [0u8; 1024]).is_err(),
            "exactly three"
        );
        assert!(!sender.status().sending);
        sender.send(&frame, t0 + Duration::from_secs(2));
        assert!(
            sacn_rx.recv_from(&mut [0u8; 1024]).is_err(),
            "stopped sends nothing"
        );
    }

    /// r[verify dmx.artnet.addressing]
    #[test]
    fn art_poll_is_answered_with_a_reply_naming_the_source() {
        let (_sacn_rx, sacn_addr) = local_receiver();
        let (poller, poller_addr) = local_receiver();
        let sender = test_sender(sacn_addr, poller_addr);
        let our_port = sender.artnet_socket.as_ref().unwrap().local_addr().unwrap();
        let mut poll = Vec::new();
        poll.extend_from_slice(b"Art-Net\0");
        poll.extend_from_slice(&artnet::OP_POLL.to_le_bytes());
        poll.extend_from_slice(&[0, 14, 0, 0]);
        poller.send_to(&poll, our_port).unwrap();

        assert_eq!(sender.respond_to_polls(), 1);
        match artnet::parse(&recv(&poller)).unwrap() {
            artnet::Packet::PollReply {
                short_name,
                long_name,
                style,
                net,
                subnet,
                universes,
            } => {
                assert_eq!(short_name, "Ignition");
                assert_eq!(long_name, "Ignition - Ignition test");
                assert_eq!(style, artnet::STYLE_CONTROLLER);
                assert_eq!((net, subnet), (0, 1));
                assert_eq!(universes, vec![2]);
            }
            other => panic!("{other:?}"),
        }
    }

    /// r[verify dmx.output-toggle]
    #[test]
    fn a_socket_that_will_not_bind_is_a_status_error_not_a_panic() {
        let (holder, held) = local_receiver();
        let mut cfg = OutputConfig::default();
        cfg.universes.insert(
            1,
            UniverseOutput {
                sacn: Some(SacnOutput::default()),
                artnet: Some(ArtnetOutput::default()),
                ..Default::default()
            },
        );
        // Art-Net's preferred port is taken; it falls back and says so.
        // sACN is asked to bind a port that is also taken: hard error.
        let sender = Sender::bind_at(
            cfg,
            "x",
            BindAddrs {
                sacn: held,
                artnet: held,
            },
        );
        drop(holder);
        let st = sender.status();
        assert_eq!(st.errors.len(), 2, "{:?}", st.errors);
        assert!(st.errors[0].starts_with("sACN: bind"));
        assert!(st.errors[1].contains("ArtPoll discovery will not be answered"));
        assert!(st.sending, "Art-Net still runs from its fallback port");
        assert_eq!(st.per_universe[0].protocols, vec![Protocol::Artnet]);
    }
}
