//! The DMX wire contract: what a venue configures, what a frame is, and
//! where the bytes are echoed to.
//!
//! The leaf of the `dmx` feature. It names the things the facade and both
//! backends have to agree on and depends on neither of them — a codec
//! crate can be unit-tested against these types with no socket in the
//! process, and `ignition-viz` can read a venue's `OutputConfig` without
//! linking a transport at all, which is the whole reason this is its own
//! crate.
//!
//! Configuration is the venue's (`r[dmx.venue-config]`): `OutputConfig`
//! deserialises from the `"dmx"` key of `venue.ig-venue`, and a show
//! never carries any of it.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Instant;

// ---------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------

/// The venue's DMX output: which universes go where.
///
/// Lives under `"dmx"` in `venue.ig-venue`; see [`OutputConfig::from_venue_extra`].
// r[impl dmx.venue-config]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Keyed by the encoder's universe number (the sACN universe).
    #[serde(default)]
    pub universes: BTreeMap<u16, UniverseOutput>,
}

/// One universe's protocols and rate.
// r[impl dmx.protocols]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniverseOutput {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sacn: Option<SacnOutput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artnet: Option<ArtnetOutput>,
    /// Ceiling on how often a changed frame goes out. DMX itself refreshes
    /// at ~44 Hz, so sending faster only burns the network.
    #[serde(default = "default_max_hz")]
    pub max_hz: f32,
    /// How often an *unchanged* frame is resent so nodes keep holding it.
    #[serde(default = "default_keepalive_hz")]
    pub keepalive_hz: f32,
}

impl Default for UniverseOutput {
    fn default() -> Self {
        Self {
            enabled: true,
            sacn: None,
            artnet: None,
            max_hz: default_max_hz(),
            keepalive_hz: default_keepalive_hz(),
        }
    }
}

/// sACN for one universe.
// r[impl dmx.sacn.priority]
// r[impl dmx.sacn.addressing]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SacnOutput {
    /// 0..=200; 100 is the E1.31 default and what a house desk usually sends.
    #[serde(default = "default_priority")]
    pub priority: u8,
    /// Extra unicast targets, as `ip:port` (a receiver listens on 5568).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unicast: Vec<SocketAddr>,
    /// Send to the universe's multicast group. On by default.
    #[serde(default = "default_true")]
    pub multicast: bool,
}

impl Default for SacnOutput {
    fn default() -> Self {
        Self {
            priority: default_priority(),
            unicast: Vec::new(),
            multicast: true,
        }
    }
}

/// Art-Net for one universe.
// r[impl dmx.artnet.addressing]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtnetOutput {
    #[serde(default)]
    pub net: u8,
    #[serde(default)]
    pub subnet: u8,
    #[serde(default)]
    pub universe: u8,
    /// Unicast nodes, as `ip:port` (a node listens on 6454).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unicast: Vec<SocketAddr>,
    /// Broadcast to the subnet. On by default.
    #[serde(default = "default_true")]
    pub broadcast: bool,
    /// The broadcast address. The limited broadcast `255.255.255.255`
    /// reaches the primary interface; a venue with a dedicated lighting
    /// NIC sets its directed broadcast (e.g. `2.255.255.255`).
    #[serde(default = "default_broadcast_addr")]
    pub broadcast_addr: Ipv4Addr,
}

impl Default for ArtnetOutput {
    fn default() -> Self {
        Self {
            net: 0,
            subnet: 0,
            universe: 0,
            unicast: Vec::new(),
            broadcast: true,
            broadcast_addr: default_broadcast_addr(),
        }
    }
}

pub fn default_true() -> bool {
    true
}
pub fn default_max_hz() -> f32 {
    44.0
}
pub fn default_keepalive_hz() -> f32 {
    1.0
}
pub fn default_priority() -> u8 {
    100
}
pub fn default_broadcast_addr() -> Ipv4Addr {
    Ipv4Addr::BROADCAST
}

impl OutputConfig {
    /// The key this config lives under in a venue manifest.
    pub const VENUE_KEY: &'static str = "dmx";

    /// Read the `"dmx"` entry out of a venue manifest's flattened extras
    /// (`VenueManifest::extra`). A venue without one has no output.
    // r[impl dmx.venue-config]
    pub fn from_venue_extra(
        extra: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Self, serde_json::Error> {
        match extra.get(Self::VENUE_KEY) {
            Some(v) => serde_json::from_value(v.clone()),
            None => Ok(Self::default()),
        }
    }

    /// True if at least one enabled universe has a protocol.
    pub fn has_output(&self) -> bool {
        self.universes
            .values()
            .any(|u| u.enabled && (u.sacn.is_some() || u.artnet.is_some()))
    }
}

// ---------------------------------------------------------------------
// Loopback
// ---------------------------------------------------------------------

/// Where the exact bytes that went to the socket also go — the
/// visualizer's receive path, so it renders what the rig receives.
// r[impl dmx.loopback]
pub trait Sink: Send {
    /// One universe's 512 slots, exactly as carried in the packet(s) just sent.
    fn frame(&mut self, universe: u16, data: &[u8; 512]);
}

impl<F: FnMut(u16, &[u8; 512]) + Send> Sink for F {
    fn frame(&mut self, universe: u16, data: &[u8; 512]) {
        self(universe, data)
    }
}

// ---------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Protocol {
    Sacn,
    Artnet,
}

/// What the desk shows: sending or not, which universes, at what rate,
/// and every socket error.
// r[impl dmx.output-toggle]
#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    /// The desk toggle.
    pub enabled: bool,
    /// Enabled, a socket is open, and at least one universe is configured.
    pub sending: bool,
    pub per_universe: Vec<UniverseStatus>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UniverseStatus {
    pub universe: u16,
    pub protocols: Vec<Protocol>,
    pub last_sent: Option<Instant>,
    /// Frames sent in the last second.
    pub hz: f32,
}
