//! Live DMX input — sACN (E1.31) and Art-Net, both listened for
//! concurrently since supporting both is cheap once one exists (see
//! `docs/research/lighting-console-landscape.md`'s DMX-architecture
//! research). Each protocol runs on its own OS thread and writes into a
//! shared per-universe buffer; the render loop only ever reads that buffer,
//! never touches a socket directly.
//!
//! This is Ignition's "actually emulate a live rig" layer: `fixtures.json`
//! stays the fixture's fixed *mount* pose (see `venue.rs`/
//! `docs/domain/norco-venue-reference.md`), and this module supplies the
//! *live* values — dimmer, colour, pan/tilt — that get composed on top of
//! that mount pose each frame. A fixture with no `ChannelMap`
//! (`channel_map.rs`) or no live packets yet simply renders at its static
//! default, exactly as before this module existed.

use ignition_proto::{Attribute, ChannelMap, ColorChannel, DmxAddress};
use sacn::packet::ACN_SDT_MULTICAST_PORT;
use sacn::receive::SacnReceiver;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Arc, RwLock};
use std::time::Duration;

const UNIVERSE_LEN: usize = 512;
const ARTNET_PORT: u16 = 6454;

/// Shared live state: one 512-byte frame per universe, last-write-wins
/// across whichever protocol delivered it. Cheap to clone (`Arc`) — hand a
/// copy to the render loop and to each listener thread.
#[derive(Clone, Default)]
pub struct DmxUniverses {
    inner: Arc<RwLock<HashMap<u16, [u8; UNIVERSE_LEN]>>>,
}

impl DmxUniverses {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces a universe's frame — what a received packet does, and
    /// what the loopback sink does with the frame that left the socket.
    pub fn write_universe(&self, universe: u16, values: &[u8]) {
        let mut map = self.inner.write().expect("dmx universes lock poisoned");
        let frame = map.entry(universe).or_insert([0u8; UNIVERSE_LEN]);
        let n = values.len().min(UNIVERSE_LEN);
        frame[..n].copy_from_slice(&values[..n]);
    }

    /// Sets one byte directly — used by `show.rs`'s cue-playback bridge to
    /// inject its own computed output into the same shared state real
    /// sACN/Art-Net packets land in, so a programmed show renders through
    /// exactly the same `resolve()` path as a real console's output, no
    /// separate code path in `scene.rs`. 0-based channel, matching
    /// `write_universe`'s convention. Marking a universe as "seen" this way
    /// (even a single byte) means `resolve()` no longer treats it as
    /// never-received — correct for a cue engine actually driving that
    /// universe, same as a real packet arriving would.
    // r[impl playback.no-merge-at-dmx] - a plain byte write; no priority or HTP lives here
    pub fn set_channel(&self, universe: u16, channel0: u16, value: u8) {
        self.set_channels([(universe, channel0, value)]);
    }

    /// Writes many `(universe, channel0, value)` bytes under one lock.
    /// A cue frame is a few hundred bytes, and a lock per byte was a
    /// few hundred lock round-trips per frame for no reason.
    pub fn set_channels(&self, values: impl IntoIterator<Item = (u16, u16, u8)>) {
        let mut map = self.inner.write().expect("dmx universes lock poisoned");
        for (universe, channel0, value) in values {
            let frame = map.entry(universe).or_insert([0u8; UNIVERSE_LEN]);
            if let Some(slot) = frame.get_mut(channel0 as usize) {
                *slot = value;
            }
        }
    }

    /// Every universe as it is right now — the frame the transmitter
    /// sends. One read lock, one copy; the sender reorders and wraps
    /// these bytes and decides nothing.
    // r[impl dmx.one-frame] - the transmitter's frame is this snapshot, nothing else
    pub fn snapshot(&self) -> HashMap<u16, [u8; UNIVERSE_LEN]> {
        self.inner
            .read()
            .expect("dmx universes lock poisoned")
            .clone()
    }

    /// Resolve one fixture's live attributes given where it's patched and
    /// its channel layout. Returns `None` only if `dmx`/`map` themselves are
    /// absent (the caller already has both by the time it calls this) —
    /// present-but-zero (e.g. blackout, or nothing has ever been sent) still
    /// resolves, just to zero/default values, matching real desk behaviour.
    pub fn resolve(&self, dmx: &DmxAddress, map: &ChannelMap) -> ResolvedAttributes {
        // A universe no packet has EVER arrived for is a different state
        // from "a packet arrived and every byte in it happens to be zero" —
        // the latter is a real, meaningful DMX state (e.g. an intentional
        // blackout, or a console that genuinely means pan-hard-left at
        // byte 0); the former just means nothing is connected yet. Without
        // this check, a fixture with a Pan/Tilt channel would resolve a
        // byte-0 "no signal" state through the same degrees-per-byte
        // formula real data uses, landing on -270°/-135° (nowhere near
        // neutral) purely because nothing had ever been received — the
        // same class of bug the dimmer default-to-off fix addressed, one
        // level up: universe-level rather than per-attribute.
        // One read lock for the whole fixture, not one per byte: this
        // runs for every fixture every frame.
        let universes = self.inner.read().expect("dmx universes lock poisoned");
        let Some(frame) = universes.get(&dmx.universe) else {
            return ResolvedAttributes::default();
        };

        let read = |offset: u16| -> u8 {
            let chan0 = dmx.start_channel.saturating_sub(1) + offset;
            frame.get(chan0 as usize).copied().unwrap_or(0)
        };
        // Coarse byte high, fine byte low when the fixture has a fine
        // channel; the plain 8-bit value otherwise. Either way the
        // result is a fraction of the full 16-bit range, so the
        // degrees formula is the same for both.
        let read_wide = |coarse: u16, fine: Option<u16>| -> f32 {
            match fine {
                Some(fine) => u16::from_be_bytes([read(coarse), read(fine)]) as f32 / 65535.0,
                None => read(coarse) as f32 / 255.0,
            }
        };
        let pan_fine = map.offset_of(&Attribute::PanFine);
        let tilt_fine = map.offset_of(&Attribute::TiltFine);

        let mut resolved = ResolvedAttributes::default();
        for (offset, attr) in &map.channels {
            let v = read(*offset);
            match attr {
                Attribute::Dimmer => resolved.dimmer = v as f32 / 255.0,
                Attribute::Pan => resolved.pan_deg = (read_wide(*offset, pan_fine) - 0.5) * 540.0,
                Attribute::Tilt => {
                    resolved.tilt_deg = (read_wide(*offset, tilt_fine) - 0.5) * 270.0
                }
                // Consumed alongside the coarse byte above.
                Attribute::PanFine | Attribute::TiltFine => {}
                Attribute::ColorAdd { channel } => {
                    let f = v as f32 / 255.0;
                    match channel {
                        ColorChannel::Red => resolved.color[0] = f,
                        ColorChannel::Green => resolved.color[1] = f,
                        ColorChannel::Blue => resolved.color[2] = f,
                        // White/Amber/UV/Lime add into all three channels as
                        // a cheap approximation — there's no real colour-
                        // mixing model here yet, just enough to make a
                        // white/amber channel visibly lighten the RGB.
                        ColorChannel::White | ColorChannel::Amber => {
                            resolved.color[0] = (resolved.color[0] + f * 0.6).min(1.0);
                            resolved.color[1] = (resolved.color[1] + f * 0.6).min(1.0);
                            resolved.color[2] = (resolved.color[2] + f * 0.6).min(1.0);
                        }
                        _ => {}
                    }
                    resolved.has_color = true;
                }
                Attribute::Zoom => resolved.zoom = Some(v as f32 / 255.0),
                Attribute::Strobe => resolved.strobe = Some(v as f32 / 255.0),
                Attribute::GoboWheel { .. } => resolved.gobo = Some(v),
                _ => {}
            }
        }
        // A fixture whose personality has no Dimmer channel at all (a bare
        // 3ch RGB par — see `channel_map.rs`) has no byte to read for
        // brightness; its colour bytes ARE its brightness (all-zero RGB is
        // off, anything else is on). Only fixtures with a real Dimmer
        // channel get `resolved.dimmer` from that channel above.
        let color_is_lit = resolved.color.iter().any(|c| *c > 0.001);
        if resolved.has_color
            && color_is_lit
            && !map
                .channels
                .iter()
                .any(|(_, a)| matches!(a, Attribute::Dimmer))
        {
            resolved.dimmer = 1.0;
        }
        resolved
    }
}

/// A fixture's live-resolved state for one frame, in the visualizer's own
/// units (0-1 for dimmer/colour, degrees for pan/tilt) — already converted
/// out of raw DMX bytes so `scene.rs` never touches a byte value.
#[derive(Debug, Clone, Copy)]
pub struct ResolvedAttributes {
    pub dimmer: f32,
    pub pan_deg: f32,
    pub tilt_deg: f32,
    pub color: [f32; 3],
    pub has_color: bool,
    /// The zoom byte as a fraction, when the personality has one.
    /// Decoded from the wire like everything else, so a zoom the
    /// encoder wrote is the zoom the beam draws.
    pub zoom: Option<f32>,
    /// The strobe byte as a fraction; zero is open.
    pub strobe: Option<f32>,
    /// The raw gobo-wheel byte. Carried so a snapshot of the decoded
    /// frame is complete; the renderer has no gobo texture yet.
    pub gobo: Option<u8>,
}

impl Default for ResolvedAttributes {
    fn default() -> Self {
        // Defaults to *off*, not full — a fixture with no live data yet
        // (nothing sent, or no channel map) must not read as lit. The one
        // exception (a fixture whose real personality has no separate
        // Dimmer channel at all, e.g. a bare 3ch RGB par) is handled
        // explicitly in `resolve()` below, not by defaulting every fixture
        // to "on."
        Self {
            dimmer: 0.0,
            pan_deg: 0.0,
            tilt_deg: 0.0,
            color: [0.0, 0.0, 0.0],
            has_color: false,
            zoom: None,
            strobe: None,
            gobo: None,
        }
    }
}

/// Spawn the sACN receiver thread. Listens on the standard E1.31 multicast
/// port for every universe 1..=`max_universe` — sACN's own multicast-per-
/// universe model means we have to subscribe up front rather than just
/// opening one socket, unlike Art-Net's single broadcast port.
pub fn spawn_sacn_listener(universes: DmxUniverses, max_universe: u16) {
    std::thread::spawn(move || {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), ACN_SDT_MULTICAST_PORT);
        let mut receiver = match SacnReceiver::with_ip(addr, None) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "ignition-viz: sACN receiver failed to start: {e:?} — no live sACN input"
                );
                return;
            }
        };
        let wanted: Vec<u16> = (1..=max_universe.max(1)).collect();
        if let Err(e) = receiver.listen_universes(&wanted) {
            eprintln!("ignition-viz: sACN listen_universes failed: {e:?}");
            return;
        }
        eprintln!("ignition-viz: sACN listening on universes 1..={max_universe}");
        loop {
            match receiver.recv(Some(Duration::from_secs(2))) {
                Ok(packets) => {
                    for p in packets {
                        universes.write_universe(p.universe, &p.values);
                    }
                }
                Err(sacn::error::errors::SacnError::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    // No data this tick — expected between cues/blackouts.
                }
                Err(e) => eprintln!("ignition-viz: sACN recv error: {e:?}"),
            }
        }
    });
}

/// Spawn the Art-Net receiver thread. Art-Net is a single UDP port carrying
/// `ArtDmx` packets tagged with a 15-bit port-address (net/sub-net/universe
/// folded into one number) — unlike sACN there's no per-universe
/// subscription, every packet that arrives gets decoded and filed.
pub fn spawn_artnet_listener(universes: DmxUniverses) {
    std::thread::spawn(move || {
        let socket = match UdpSocket::bind(("0.0.0.0", ARTNET_PORT)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("ignition-viz: Art-Net socket bind failed: {e} — no live Art-Net input");
                return;
            }
        };
        eprintln!("ignition-viz: Art-Net listening on UDP :{ARTNET_PORT}");
        let mut buf = [0u8; 1024];
        loop {
            let (len, _from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("ignition-viz: Art-Net recv error: {e}");
                    continue;
                }
            };
            match artnet_protocol::ArtCommand::from_buffer(&buf[..len]) {
                Ok(artnet_protocol::ArtCommand::Output(output)) => {
                    let universe: u16 = output.port_address.into();
                    let data: &Vec<u8> = output.data.as_ref();
                    universes.write_universe(universe, data);
                }
                Ok(_) => {}  // Poll/PollReply/other control traffic — ignore.
                Err(_) => {} // Non-Art-Net traffic on the port — ignore.
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(channels: Vec<(u16, Attribute)>) -> ChannelMap {
        ChannelMap {
            curves: Default::default(),
            footprint: channels.len() as u16 + 1,
            channels,
        }
    }

    #[test]
    fn resolves_dimmer_and_rgb_from_raw_bytes() {
        let universes = DmxUniverses::new();
        universes.write_universe(1, &{
            let mut u = [0u8; 512];
            u[9] = 255; // dimmer, offset 0 at start_channel 10
            u[10] = 128; // red
            u[11] = 64; // green
            u[12] = 32; // blue
            u
        });
        let m = map(vec![
            (0, Attribute::Dimmer),
            (
                1,
                Attribute::ColorAdd {
                    channel: ColorChannel::Red,
                },
            ),
            (
                2,
                Attribute::ColorAdd {
                    channel: ColorChannel::Green,
                },
            ),
            (
                3,
                Attribute::ColorAdd {
                    channel: ColorChannel::Blue,
                },
            ),
        ]);
        let addr = DmxAddress {
            universe: 1,
            start_channel: 10,
        };

        let resolved = universes.resolve(&addr, &m);
        assert!((resolved.dimmer - 1.0).abs() < 0.01);
        assert!((resolved.color[0] - 128.0 / 255.0).abs() < 0.01);
        assert!((resolved.color[1] - 64.0 / 255.0).abs() < 0.01);
        assert!((resolved.color[2] - 32.0 / 255.0).abs() < 0.01);
        assert!(resolved.has_color);
    }

    #[test]
    fn unpatched_universe_resolves_to_zero_not_a_panic() {
        let universes = DmxUniverses::new();
        let m = map(vec![(0, Attribute::Dimmer)]);
        let addr = DmxAddress {
            universe: 99,
            start_channel: 1,
        };
        let resolved = universes.resolve(&addr, &m);
        assert_eq!(resolved.dimmer, 0.0);
    }

    #[test]
    fn pan_tilt_centre_byte_resolves_to_zero_degrees() {
        let universes = DmxUniverses::new();
        universes.write_universe(2, &{
            let mut u = [0u8; 512];
            u[0] = 128; // ~centre of 0-255
            u[1] = 128;
            u
        });
        let m = map(vec![(0, Attribute::Pan), (1, Attribute::Tilt)]);
        let addr = DmxAddress {
            universe: 2,
            start_channel: 1,
        };
        let resolved = universes.resolve(&addr, &m);
        assert!(
            resolved.pan_deg.abs() < 2.0,
            "expected ~0deg, got {}",
            resolved.pan_deg
        );
        assert!(
            resolved.tilt_deg.abs() < 2.0,
            "expected ~0deg, got {}",
            resolved.tilt_deg
        );
    }

    #[test]
    fn a_second_fixtures_bytes_dont_bleed_into_the_first() {
        let universes = DmxUniverses::new();
        universes.write_universe(1, &{
            let mut u = [0u8; 512];
            u[0] = 10; // fixture A's dimmer (start_channel 1, offset 0)
            u[7] = 200; // fixture B's dimmer (start_channel 8, offset 0)
            u
        });
        let m = map(vec![(0, Attribute::Dimmer)]);
        let a = universes.resolve(
            &DmxAddress {
                universe: 1,
                start_channel: 1,
            },
            &m,
        );
        let b = universes.resolve(
            &DmxAddress {
                universe: 1,
                start_channel: 8,
            },
            &m,
        );
        assert!((a.dimmer - 10.0 / 255.0).abs() < 0.01);
        assert!((b.dimmer - 200.0 / 255.0).abs() < 0.01);
    }

    #[test]
    fn no_live_data_never_reads_as_lit() {
        // Regression: ResolvedAttributes::default() used to default dimmer
        // to 1.0 ("full on"), which meant any fixture with no live packets
        // ever received — the overwhelming majority of a real rig at any
        // given moment — still resolved as lit. Every fixture must default
        // to off.
        let universes = DmxUniverses::new(); // no universe ever written
        let m = map(vec![
            (0, Attribute::Dimmer),
            (
                1,
                Attribute::ColorAdd {
                    channel: ColorChannel::Red,
                },
            ),
            (
                2,
                Attribute::ColorAdd {
                    channel: ColorChannel::Green,
                },
            ),
            (
                3,
                Attribute::ColorAdd {
                    channel: ColorChannel::Blue,
                },
            ),
        ]);
        let resolved = universes.resolve(
            &DmxAddress {
                universe: 1,
                start_channel: 1,
            },
            &m,
        );
        assert_eq!(resolved.dimmer, 0.0);
    }

    #[test]
    fn never_received_universe_resolves_pan_tilt_to_neutral_not_the_raw_zero_byte() {
        // Regression: a Pan/Tilt channel run through the raw-byte formula
        // treats byte 0 as a real value (-270deg/-135deg, nowhere near
        // centre) — correct when a console genuinely sent that. But a
        // universe nothing has EVER been received on isn't "byte 0", it's
        // "no signal at all", and was resolving to that same skewed
        // pan/tilt purely from having never heard from the console yet —
        // caught via a moving head visibly deforming in a --snapshot with
        // no DMX source running.
        let universes = DmxUniverses::new(); // universe 1 never written at all
        let m = map(vec![(0, Attribute::Pan), (1, Attribute::Tilt)]);
        let resolved = universes.resolve(
            &DmxAddress {
                universe: 1,
                start_channel: 1,
            },
            &m,
        );
        assert_eq!(resolved.pan_deg, 0.0);
        assert_eq!(resolved.tilt_deg, 0.0);
    }

    #[test]
    fn bare_rgb_par_with_no_dimmer_channel_is_governed_by_its_colour() {
        // A 3ch RGB-only par (Rockville Rockstrip 3ch — see
        // channel_map.rs) has no separate Dimmer byte at all; its colour
        // bytes ARE its brightness.
        let universes = DmxUniverses::new();
        let m = map(vec![
            (
                0,
                Attribute::ColorAdd {
                    channel: ColorChannel::Red,
                },
            ),
            (
                1,
                Attribute::ColorAdd {
                    channel: ColorChannel::Green,
                },
            ),
            (
                2,
                Attribute::ColorAdd {
                    channel: ColorChannel::Blue,
                },
            ),
        ]);
        // All-zero RGB (nothing sent) -> still off, not full-on.
        let off = universes.resolve(
            &DmxAddress {
                universe: 1,
                start_channel: 1,
            },
            &m,
        );
        assert_eq!(off.dimmer, 0.0);

        // Non-zero RGB -> on, governed by colour alone (no dimmer channel
        // to read, so this fixture treats itself as full brightness and
        // lets the colour bytes carry the actual intensity).
        universes.write_universe(1, &{
            let mut u = [0u8; 512];
            u[0] = 255;
            u
        });
        let on = universes.resolve(
            &DmxAddress {
                universe: 1,
                start_channel: 1,
            },
            &m,
        );
        assert_eq!(on.dimmer, 1.0);
        assert!((on.color[0] - 1.0).abs() < 0.01);
    }
}
