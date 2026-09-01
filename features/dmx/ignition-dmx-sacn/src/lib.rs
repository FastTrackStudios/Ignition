//! sACN (ANSI E1.31) packets, built and parsed byte for byte.
//!
//! Hand-rolled rather than through the `sacn` crate because the spec
//! wants exact control of the fields a shared universe is negotiated on —
//! priority, CID, sequence, and the stream-terminated option — and the
//! crate's source object owns all four. The layout is the three-layer
//! ACN PDU stack: root (CID), framing (name, priority, sequence, universe),
//! DMP (start code + 512 slots).

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

/// The port every sACN receiver listens on.
pub const SACN_PORT: u16 = 5568;
/// Root-layer vector: `VECTOR_ROOT_E131_DATA`.
pub const VECTOR_ROOT_DATA: u32 = 0x0000_0004;
/// Framing-layer vector: `VECTOR_E131_DATA_PACKET`.
pub const VECTOR_FRAMING_DATA: u32 = 0x0000_0002;
/// DMP-layer vector: `VECTOR_DMP_SET_PROPERTY`.
pub const VECTOR_DMP_SET_PROPERTY: u8 = 0x02;
/// A full data packet: 38 root + 77 framing + 523 DMP.
pub const DATA_PACKET_LEN: usize = 638;
/// Framing-layer options bit: the source is leaving this universe.
pub const OPTION_STREAM_TERMINATED: u8 = 0x40;
/// Framing-layer options bit: preview data, not for live output.
pub const OPTION_PREVIEW: u8 = 0x80;
/// The longest UTF-8 source name, null padded to this.
pub const SOURCE_NAME_LEN: usize = 64;

const ACN_PACKET_ID: [u8; 12] = *b"ASC-E1.17\0\0\0";

/// The multicast group a universe is sent to: `239.255.hi.lo:5568`.
// r[impl dmx.sacn.addressing]
pub fn multicast_addr(universe: u16) -> SocketAddr {
    let [hi, lo] = universe.to_be_bytes();
    SocketAddr::V4(SocketAddrV4::new(
        Ipv4Addr::new(239, 255, hi, lo),
        SACN_PORT,
    ))
}

/// A live data frame for one universe.
// r[impl dmx.sacn.priority]
// r[impl dmx.sequence]
pub fn data_packet(
    cid: &[u8; 16],
    source_name: &str,
    priority: u8,
    sequence: u8,
    universe: u16,
    data: &[u8; 512],
) -> Vec<u8> {
    build(cid, source_name, priority, sequence, universe, 0, data)
}

/// The frame a source sends (three times) when it leaves a universe,
/// so nodes release it at once instead of waiting out their timeout.
// r[impl dmx.sacn.addressing]
pub fn terminate_packet(
    cid: &[u8; 16],
    source_name: &str,
    priority: u8,
    sequence: u8,
    universe: u16,
) -> Vec<u8> {
    build(
        cid,
        source_name,
        priority,
        sequence,
        universe,
        OPTION_STREAM_TERMINATED,
        &[0u8; 512],
    )
}

fn flags_length(len: usize) -> [u8; 2] {
    // High nibble 0x7 (the ACN "flags"), low 12 bits the PDU length.
    (0x7000 | (len as u16 & 0x0fff)).to_be_bytes()
}

fn build(
    cid: &[u8; 16],
    source_name: &str,
    priority: u8,
    sequence: u8,
    universe: u16,
    options: u8,
    data: &[u8; 512],
) -> Vec<u8> {
    let mut p = Vec::with_capacity(DATA_PACKET_LEN);
    // --- Root layer (38 bytes) ---
    p.extend_from_slice(&0x0010u16.to_be_bytes()); // preamble size
    p.extend_from_slice(&0x0000u16.to_be_bytes()); // post-amble size
    p.extend_from_slice(&ACN_PACKET_ID);
    p.extend_from_slice(&flags_length(DATA_PACKET_LEN - 16)); // 622
    p.extend_from_slice(&VECTOR_ROOT_DATA.to_be_bytes());
    p.extend_from_slice(cid);
    // --- Framing layer (77 bytes) ---
    p.extend_from_slice(&flags_length(DATA_PACKET_LEN - 38)); // 600
    p.extend_from_slice(&VECTOR_FRAMING_DATA.to_be_bytes());
    let mut name = [0u8; SOURCE_NAME_LEN];
    let bytes = source_name.as_bytes();
    // Leave the last byte null so the name is always terminated, and
    // never cut a multi-byte character in half.
    let mut n = bytes.len().min(SOURCE_NAME_LEN - 1);
    while n > 0 && !source_name.is_char_boundary(n) {
        n -= 1;
    }
    name[..n].copy_from_slice(&bytes[..n]);
    p.extend_from_slice(&name);
    p.push(priority.min(200));
    p.extend_from_slice(&0u16.to_be_bytes()); // synchronization address
    p.push(sequence);
    p.push(options);
    p.extend_from_slice(&universe.to_be_bytes());
    // --- DMP layer (523 bytes) ---
    p.extend_from_slice(&flags_length(523));
    p.push(VECTOR_DMP_SET_PROPERTY);
    p.push(0xa1); // address type & data type
    p.extend_from_slice(&0u16.to_be_bytes()); // first property address
    p.extend_from_slice(&1u16.to_be_bytes()); // address increment
    p.extend_from_slice(&513u16.to_be_bytes()); // property value count
    p.push(0); // DMX start code
    p.extend_from_slice(data);
    debug_assert_eq!(p.len(), DATA_PACKET_LEN);
    p
}

/// A decoded data packet — what a receiver (or the loopback test) sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataPacket {
    pub cid: [u8; 16],
    pub source_name: String,
    pub priority: u8,
    pub sequence: u8,
    pub options: u8,
    pub universe: u16,
    pub start_code: u8,
    pub data: Vec<u8>,
}

impl DataPacket {
    pub fn is_terminated(&self) -> bool {
        self.options & OPTION_STREAM_TERMINATED != 0
    }
}

/// Parse a data packet. Returns `None` for anything that is not an
/// E1.31 data packet with a level (start code 0) or other payload.
pub fn parse(buf: &[u8]) -> Option<DataPacket> {
    if buf.len() < 126 || buf[4..16] != ACN_PACKET_ID {
        return None;
    }
    let be32 = |i: usize| Some(u32::from_be_bytes(buf[i..i + 4].try_into().ok()?));
    let be16 = |i: usize| u16::from_be_bytes([buf[i], buf[i + 1]]);
    if be32(18)? != VECTOR_ROOT_DATA || be32(40)? != VECTOR_FRAMING_DATA {
        return None;
    }
    let mut cid = [0u8; 16];
    cid.copy_from_slice(&buf[22..38]);
    let name_end = buf[44..108].iter().position(|b| *b == 0).unwrap_or(64);
    let source_name = String::from_utf8_lossy(&buf[44..44 + name_end]).into_owned();
    let priority = buf[108];
    let sequence = buf[111];
    let options = buf[112];
    let universe = be16(113);
    if buf[117] != VECTOR_DMP_SET_PROPERTY || buf[118] != 0xa1 {
        return None;
    }
    let count = be16(123) as usize;
    if count == 0 || buf.len() < 125 + count {
        return None;
    }
    Some(DataPacket {
        cid,
        source_name,
        priority,
        sequence,
        options,
        universe,
        start_code: buf[125],
        data: buf[126..125 + count].to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CID: [u8; 16] = [
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f,
    ];

    fn ramp() -> [u8; 512] {
        let mut d = [0u8; 512];
        for (i, b) in d.iter_mut().enumerate() {
            *b = (i % 256) as u8;
        }
        d
    }

    /// r[verify dmx.sacn.priority]
    /// r[verify dmx.sequence]
    #[test]
    fn data_packet_is_byte_exact_against_the_spec_layout() {
        let p = data_packet(&CID, "Ignition", 100, 7, 1, &ramp());
        assert_eq!(p.len(), DATA_PACKET_LEN);
        // Root layer.
        assert_eq!(&p[0..2], &[0x00, 0x10], "preamble size");
        assert_eq!(&p[2..4], &[0x00, 0x00], "post-amble size");
        assert_eq!(&p[4..16], b"ASC-E1.17\0\0\0");
        assert_eq!(&p[16..18], &[0x72, 0x6e], "root flags/length 0x7000|622");
        assert_eq!(&p[18..22], &[0, 0, 0, 4], "root vector");
        assert_eq!(&p[22..38], &CID);
        // Framing layer.
        assert_eq!(&p[38..40], &[0x72, 0x58], "framing flags/length 0x7000|600");
        assert_eq!(&p[40..44], &[0, 0, 0, 2], "framing vector");
        assert_eq!(&p[44..52], b"Ignition");
        assert!(p[52..108].iter().all(|b| *b == 0), "name null padded");
        assert_eq!(p[108], 100, "priority");
        assert_eq!(&p[109..111], &[0, 0], "sync address");
        assert_eq!(p[111], 7, "sequence");
        assert_eq!(p[112], 0, "options");
        assert_eq!(&p[113..115], &[0, 1], "universe");
        // DMP layer.
        assert_eq!(&p[115..117], &[0x72, 0x0b], "dmp flags/length 0x7000|523");
        assert_eq!(p[117], 0x02, "dmp vector");
        assert_eq!(p[118], 0xa1, "address & data type");
        assert_eq!(&p[119..121], &[0, 0], "first property address");
        assert_eq!(&p[121..123], &[0, 1], "address increment");
        assert_eq!(&p[123..125], &[0x02, 0x01], "property value count 513");
        assert_eq!(p[125], 0, "start code");
        assert_eq!(&p[126..], &ramp());
    }

    /// r[verify dmx.sacn.addressing]
    #[test]
    fn terminate_packet_sets_the_stream_terminated_bit() {
        let p = terminate_packet(&CID, "Ignition", 100, 3, 9);
        assert_eq!(p[112], OPTION_STREAM_TERMINATED);
        assert_eq!(&p[113..115], &[0, 9]);
        let parsed = parse(&p).unwrap();
        assert!(parsed.is_terminated());
        assert_eq!(parsed.sequence, 3);
    }

    /// r[verify dmx.sacn.addressing]
    #[test]
    fn multicast_group_is_239_255_hi_lo() {
        assert_eq!(multicast_addr(1).to_string(), "239.255.0.1:5568");
        assert_eq!(multicast_addr(0x1234).to_string(), "239.255.18.52:5568");
    }

    #[test]
    fn priority_is_clamped_to_200_and_name_is_terminated() {
        let long = "x".repeat(100);
        let p = data_packet(&CID, &long, 255, 0, 1, &[0; 512]);
        assert_eq!(p[108], 200);
        assert_eq!(p[107], 0, "last name byte is always null");
        assert_eq!(parse(&p).unwrap().source_name.len(), 63);
    }

    #[test]
    fn parse_round_trips_the_builder() {
        let p = data_packet(&CID, "Ignition @ Norco", 150, 200, 300, &ramp());
        let d = parse(&p).unwrap();
        assert_eq!(d.cid, CID);
        assert_eq!(d.source_name, "Ignition @ Norco");
        assert_eq!(d.priority, 150);
        assert_eq!(d.sequence, 200);
        assert_eq!(d.universe, 300);
        assert_eq!(d.start_code, 0);
        assert_eq!(d.data, ramp().to_vec());
        assert!(parse(b"nope").is_none());
    }
}
