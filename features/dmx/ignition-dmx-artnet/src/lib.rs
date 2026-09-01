//! Art-Net 4 packets: `ArtDmx` out, `ArtPoll` in, `ArtPollReply` back.
//!
//! Hand-rolled for the same reason as `sacn.rs`: the reply must name the
//! source and carry a port-address per universe, and the sequence must
//! be ours to drive. Every multi-byte field is little-endian except the
//! DMX length, which the Art-Net spec puts high byte first.

use std::net::{Ipv4Addr, SocketAddr};

/// Every Art-Net node and controller lives on this port.
pub const ARTNET_PORT: u16 = 6454;
/// `OpDmx` — one universe of levels.
pub const OP_DMX: u16 = 0x5000;
/// `OpPoll` — "who is out there".
pub const OP_POLL: u16 = 0x2000;
/// `OpPollReply` — "I am".
pub const OP_POLL_REPLY: u16 = 0x2100;
/// The protocol revision every Art-Net 4 packet carries.
pub const PROTOCOL_VERSION: u16 = 14;
/// `StController` — a desk, not a node.
pub const STYLE_CONTROLLER: u8 = 0x01;
/// Art-Net's fixed length: ID (8) + OpCode (2) + ProtVer (2) + Seq +
/// Phys + SubUni + Net + Length (2), then 512 slots.
pub const ART_DMX_LEN: usize = 18 + 512;
/// The reply is a fixed-size record.
pub const ART_POLL_REPLY_LEN: usize = 239;
const ID: [u8; 8] = *b"Art-Net\0";

/// A 15-bit Art-Net port-address: net (7 bits), sub-net (4), universe (4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PortAddress {
    pub net: u8,
    pub subnet: u8,
    pub universe: u8,
}

impl PortAddress {
    pub fn new(net: u8, subnet: u8, universe: u8) -> Self {
        Self {
            net: net & 0x7f,
            subnet: subnet & 0x0f,
            universe: universe & 0x0f,
        }
    }

    /// The folded 15-bit value the wire carries and receivers key on.
    pub fn as_u16(self) -> u16 {
        ((self.net as u16) << 8) | ((self.subnet as u16) << 4) | self.universe as u16
    }

    pub fn from_u16(v: u16) -> Self {
        Self::new((v >> 8) as u8, ((v >> 4) & 0x0f) as u8, (v & 0x0f) as u8)
    }
}

/// A configured universe's port address.
///
/// The conversion lives here rather than as a method on `ArtnetOutput`
/// because `PortAddress` is an Art-Net wire concept and
/// `ignition-dmx-proto` is a leaf: the contract crate must not have to
/// know what a subnet is to describe a venue's config.
impl From<&ignition_dmx_proto::ArtnetOutput> for PortAddress {
    fn from(cfg: &ignition_dmx_proto::ArtnetOutput) -> Self {
        Self::new(cfg.net, cfg.subnet, cfg.universe)
    }
}

/// One universe of levels.
// r[impl dmx.artnet.addressing]
// r[impl dmx.sequence]
pub fn art_dmx(sequence: u8, port_address: PortAddress, data: &[u8; 512]) -> Vec<u8> {
    let mut p = Vec::with_capacity(ART_DMX_LEN);
    p.extend_from_slice(&ID);
    p.extend_from_slice(&OP_DMX.to_le_bytes());
    p.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    p.push(sequence);
    p.push(0); // physical input port — none, we are a desk
    p.push((port_address.subnet << 4) | port_address.universe);
    p.push(port_address.net);
    p.extend_from_slice(&512u16.to_be_bytes());
    p.extend_from_slice(data);
    p
}

/// What an `ArtPollReply` says about us. One reply covers up to four
/// universes that share a net and sub-net; `bind_index` distinguishes
/// pages when there are more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollReply {
    /// The address we send from, as nodes should see it.
    pub ip: Ipv4Addr,
    pub short_name: String,
    pub long_name: String,
    pub node_report: String,
    pub net: u8,
    pub subnet: u8,
    /// Up to four universe nibbles on this net/sub-net.
    pub universes: Vec<u8>,
    /// 1-based page index when a source spans more than four universes.
    pub bind_index: u8,
}

fn fixed(dst: &mut [u8], s: &str) {
    let max = dst.len() - 1;
    let mut n = s.len().min(max);
    while n > 0 && !s.is_char_boundary(n) {
        n -= 1;
    }
    dst[..n].copy_from_slice(&s.as_bytes()[..n]);
}

/// The discovery answer: short name "Ignition", long name naming the
/// source, style `StController`, one port per universe.
// r[impl dmx.artnet.addressing]
pub fn art_poll_reply(reply: &PollReply) -> Vec<u8> {
    let mut p = vec![0u8; ART_POLL_REPLY_LEN];
    p[0..8].copy_from_slice(&ID);
    p[8..10].copy_from_slice(&OP_POLL_REPLY.to_le_bytes());
    p[10..14].copy_from_slice(&reply.ip.octets());
    p[14..16].copy_from_slice(&ARTNET_PORT.to_le_bytes());
    p[16] = 0; // VersInfoH
    p[17] = 1; // VersInfoL
    p[18] = reply.net & 0x7f;
    p[19] = reply.subnet & 0x0f;
    // OemHi/OemLo: 0xffff is the "unknown / development" OEM code.
    p[20] = 0xff;
    p[21] = 0xff;
    p[22] = 0; // UbeaVersion
    p[23] = 0b1110_0000; // Status1: indicators normal, port-address by network
    // EstaManLo/Hi: 0x0000 is reserved for unregistered manufacturers.
    p[24] = 0;
    p[25] = 0;
    fixed(&mut p[26..44], &reply.short_name); // 18
    fixed(&mut p[44..108], &reply.long_name); // 64
    fixed(&mut p[108..172], &reply.node_report); // 64
    let ports = reply.universes.len().min(4);
    p[172] = 0; // NumPortsHi
    p[173] = ports as u8;
    for (i, u) in reply.universes.iter().take(4).enumerate() {
        p[174 + i] = 0x40; // PortTypes: this port inputs to the Art-Net network (DMX -> Art-Net)
        p[178 + i] = 0x80; // GoodInput: data received on this port
        p[182 + i] = 0x00; // GoodOutput
        p[186 + i] = u & 0x0f; // SwIn
        p[190 + i] = 0x00; // SwOut
    }
    p[194] = 0; // AcnPriority / SwVideo
    p[195] = 0; // SwMacro
    p[196] = 0; // SwRemote
    // 197..200 spare
    p[200] = STYLE_CONTROLLER;
    // 201..207 MAC: all zero means "not known", which is what the spec says to send.
    p[207..211].copy_from_slice(&reply.ip.octets()); // BindIp
    p[211] = reply.bind_index;
    p[212] = 0b0000_1110; // Status2: DHCP capable, 15-bit port-address, sACN capable
    // 213..217 GoodOutputB, 217 Status3, 218..224 DefaultRespUID, rest filler.
    p
}

/// An `ArtPoll` we have been asked to answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Poll {
    pub flags: u8,
    pub diag_priority: u8,
}

/// Anything on the Art-Net port we care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    Dmx {
        sequence: u8,
        port_address: PortAddress,
        data: Vec<u8>,
    },
    Poll(Poll),
    PollReply {
        short_name: String,
        long_name: String,
        style: u8,
        net: u8,
        subnet: u8,
        universes: Vec<u8>,
    },
}

/// Decode a packet, or `None` for anything that is not Art-Net or is an
/// op we do not handle.
pub fn parse(buf: &[u8]) -> Option<Packet> {
    if buf.len() < 12 || buf[0..8] != ID {
        return None;
    }
    let op = u16::from_le_bytes([buf[8], buf[9]]);
    match op {
        OP_DMX => {
            if buf.len() < 18 {
                return None;
            }
            let len = u16::from_be_bytes([buf[16], buf[17]]) as usize;
            let data = buf.get(18..18 + len)?.to_vec();
            Some(Packet::Dmx {
                sequence: buf[12],
                port_address: PortAddress::new(buf[15], buf[14] >> 4, buf[14] & 0x0f),
                data,
            })
        }
        OP_POLL => Some(Packet::Poll(Poll {
            flags: *buf.get(12).unwrap_or(&0),
            diag_priority: *buf.get(13).unwrap_or(&0),
        })),
        OP_POLL_REPLY => {
            if buf.len() < 213 {
                return None;
            }
            let text = |r: std::ops::Range<usize>| {
                let s = &buf[r];
                let n = s.iter().position(|b| *b == 0).unwrap_or(s.len());
                String::from_utf8_lossy(&s[..n]).into_owned()
            };
            let ports = buf[173].min(4) as usize;
            Some(Packet::PollReply {
                short_name: text(26..44),
                long_name: text(44..108),
                style: buf[200],
                net: buf[18],
                subnet: buf[19],
                universes: buf[186..186 + ports].to_vec(),
            })
        }
        _ => None,
    }
}

/// Where a poll reply goes: back to whoever asked (Art-Net 4), with the
/// standard port in case the poller sent from an ephemeral one.
pub fn reply_target(poller: SocketAddr) -> SocketAddr {
    SocketAddr::new(poller.ip(), poller.port())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r[verify dmx.artnet.addressing]
    /// r[verify dmx.sequence]
    #[test]
    fn art_dmx_is_byte_exact_against_the_spec_layout() {
        let mut data = [0u8; 512];
        data[0] = 255;
        data[511] = 1;
        let p = art_dmx(42, PortAddress::new(1, 2, 3), &data);
        assert_eq!(p.len(), ART_DMX_LEN);
        assert_eq!(&p[0..8], b"Art-Net\0");
        assert_eq!(&p[8..10], &[0x00, 0x50], "OpDmx little-endian");
        assert_eq!(&p[10..12], &[0x00, 0x0e], "ProtVer 14");
        assert_eq!(p[12], 42, "sequence");
        assert_eq!(p[13], 0, "physical");
        assert_eq!(p[14], 0x23, "sub-net 2 | universe 3");
        assert_eq!(p[15], 1, "net");
        assert_eq!(&p[16..18], &[0x02, 0x00], "length 512 high byte first");
        assert_eq!(&p[18..], &data);
    }

    #[test]
    fn port_address_folds_to_fifteen_bits() {
        let pa = PortAddress::new(0x7f, 0xf, 0xf);
        assert_eq!(pa.as_u16(), 0x7fff);
        assert_eq!(PortAddress::from_u16(0x0123), PortAddress::new(1, 2, 3));
        assert_eq!(PortAddress::new(0xff, 0xff, 0xff), pa, "masked");
    }

    /// r[verify dmx.artnet.addressing]
    #[test]
    fn poll_reply_names_the_source_and_is_a_controller() {
        let reply = PollReply {
            ip: Ipv4Addr::new(10, 0, 0, 5),
            short_name: "Ignition".into(),
            long_name: "Ignition (Norco)".into(),
            node_report: "#0001 [0000] OK".into(),
            net: 0,
            subnet: 1,
            universes: vec![0, 1, 2],
            bind_index: 1,
        };
        let p = art_poll_reply(&reply);
        assert_eq!(p.len(), ART_POLL_REPLY_LEN);
        assert_eq!(&p[0..8], b"Art-Net\0");
        assert_eq!(&p[8..10], &[0x00, 0x21], "OpPollReply little-endian");
        assert_eq!(&p[10..14], &[10, 0, 0, 5]);
        assert_eq!(&p[14..16], &[0x36, 0x19], "port 6454 little-endian");
        assert_eq!(p[18], 0, "NetSwitch");
        assert_eq!(p[19], 1, "SubSwitch");
        assert_eq!(&p[26..34], b"Ignition");
        assert_eq!(p[43], 0, "short name terminated");
        assert_eq!(&p[44..60], b"Ignition (Norco)");
        assert_eq!(&p[172..174], &[0, 3], "NumPorts");
        assert_eq!(&p[186..190], &[0, 1, 2, 0], "SwIn");
        assert_eq!(p[200], STYLE_CONTROLLER);
        assert_eq!(p[211], 1, "BindIndex");
        match parse(&p).unwrap() {
            Packet::PollReply {
                short_name,
                long_name,
                style,
                universes,
                ..
            } => {
                assert_eq!(short_name, "Ignition");
                assert_eq!(long_name, "Ignition (Norco)");
                assert_eq!(style, STYLE_CONTROLLER);
                assert_eq!(universes, vec![0, 1, 2]);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parse_round_trips_dmx_and_recognises_a_poll() {
        let data = [7u8; 512];
        let p = art_dmx(9, PortAddress::new(0, 0, 4), &data);
        assert_eq!(
            parse(&p),
            Some(Packet::Dmx {
                sequence: 9,
                port_address: PortAddress::new(0, 0, 4),
                data: data.to_vec(),
            })
        );
        let mut poll = Vec::new();
        poll.extend_from_slice(b"Art-Net\0");
        poll.extend_from_slice(&OP_POLL.to_le_bytes());
        poll.extend_from_slice(&[0, 14, 0x06, 0x10]);
        assert_eq!(
            parse(&poll),
            Some(Packet::Poll(Poll {
                flags: 6,
                diag_priority: 0x10
            }))
        );
        assert!(parse(b"ASC-E1.17\0\0\0").is_none());
    }
}
