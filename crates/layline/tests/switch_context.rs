//! A switch passes its holder's fields to the arm it selects, as `#[with]` passes them to a
//! nested message: JEL's DW packet, whose payload type picks PDW or SDW elements that each need
//! the header's mask, count and byte order, and Kaitai's `rfextgeo_ext(len)`, an arm sized by
//! its holder's length.

#![cfg(feature = "derive")]

use layline::{Choice, EncodeError, Layout, Message, ParseError, Stated};

#[path = "support/rng.rs"]
mod rng;

/// The flags byte: the payload's byte order in its top bit.
#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 8, order = msb)]
pub struct Flags {
    #[bits(1)]
    pub big_endian: bool,
    #[bits(7)]
    pub reserved: u8,
}

/// A DW-shaped packet: one type, whatever the payload holds. The payload type is a plain
/// field: encode writes a listed arm's, and keeps an unlisted one's.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Packet {
    pub payload_type: u8,
    pub mask: u32,
    #[bytes(1)]
    pub flags: Flags,
    pub num_elements: u16,
    #[switch(payload_type)]
    #[with(mask, num_elements, flags.big_endian)]
    pub payload: Payload,
}

/// The payload, by payload type. Each element run takes the header's numbers.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(mask: u32, count: u16, big_endian: bool))]
pub enum Payload {
    #[value(1)]
    #[with(mask, count, big_endian)]
    Pdw(Pdws),
    #[value(2)]
    #[with(mask, count, big_endian)]
    Sdw(Sdws),
    #[other]
    Described(Vec<u8>),
}

#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(needs(mask: u32, count: u16, big_endian: bool), endian = needs(big_endian))]
pub struct Pdws {
    #[count(count)]
    #[with(mask, big_endian)]
    #[message]
    pub elements: Vec<Pdw>,
}

#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(needs(mask: u32, count: u16, big_endian: bool), endian = needs(big_endian))]
pub struct Sdws {
    #[count(count)]
    #[with(mask, big_endian)]
    #[message]
    pub elements: Vec<Sdw>,
}

/// A pulse: each field present when the mask has its flag.
#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(needs(mask: u32, big_endian: bool), endian = needs(big_endian))]
pub struct Pdw {
    #[when(mask & 0x01)]
    pub toa: Option<i64>,
    #[when(mask & 0x02)]
    pub pf: Option<f64>,
    #[when(mask & 0x04)]
    pub pw: Option<f32>,
}

#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(needs(mask: u32, big_endian: bool), endian = needs(big_endian))]
pub struct Sdw {
    #[when(mask & 0x01)]
    pub first_toa: Option<i64>,
    #[when(mask & 0x02)]
    pub fc: Option<f64>,
}

/// A Kaitai extension whose arm is sized by its holder's length in 32-bit words, less the
/// header word: `rfextgeo_ext(len)`.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Extension {
    pub kind: Stated<u8>,
    pub words: u16,
    #[switch(kind)]
    #[with(words)]
    pub body: ExtBody,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be, needs(words: u16), closed)]
pub enum ExtBody {
    #[value(1)]
    #[with(words)]
    RfExtGeo(RfExtGeo),
    #[value(2)]
    Empty,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be, needs(words: u16))]
pub struct RfExtGeo {
    pub azimuth: i16,
    #[len(words * 4 - 2)]
    pub data: Vec<u8>,
}

/// A catalogue whose scalar arms take their byte order from the holder.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(big: bool), endian = needs(big), closed)]
pub enum Word {
    #[value(0)]
    Short(u16),
    #[value(1)]
    Long(u32),
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Sample {
    pub big: bool,
    pub kind: Stated<u8>,
    #[switch(kind)]
    #[with(big)]
    pub word: Word,
}

fn pdw_packet(big_endian: bool, elements: Vec<Pdw>) -> Packet {
    Packet {
        payload_type: 1,
        mask: 0x03,
        flags: Flags { big_endian, reserved: 0 },
        num_elements: elements.len() as u16,
        payload: Payload::Pdw(Pdws { elements }),
    }
}

#[test]
fn one_packet_type_reads_pdws_in_either_byte_order() {
    let pulses = vec![
        Pdw { toa: Some(1_000), pf: Some(9.5e9), pw: None },
        Pdw { toa: Some(2_000), pf: Some(1.0), pw: None },
    ];
    for big in [false, true] {
        let packet = pdw_packet(big, pulses.clone());
        let wire = packet.encode().unwrap();
        assert_eq!(&wire[..8], [1, 0, 0, 0, 3, u8::from(big) << 7, 0, 2], "the big-endian header");
        let toa = if big { 1_000i64.to_be_bytes() } else { 1_000i64.to_le_bytes() };
        assert_eq!(&wire[8..16], toa, "the first time of arrival, in the flags' order");
        assert_eq!(wire.len(), 8 + 2 * 16);
        assert_eq!(Packet::decode(&wire).unwrap(), (packet, wire.len()));
    }
}

#[test]
fn the_payload_type_picks_the_arm_and_an_unknown_one_keeps_its_bytes() {
    let sdw = Packet {
        payload_type: 2,
        mask: 0x02,
        flags: Flags::default(),
        num_elements: 1,
        payload: Payload::Sdw(Sdws { elements: vec![Sdw { first_toa: None, fc: Some(2.5) }] }),
    };
    let wire = sdw.encode().unwrap();
    assert_eq!(wire[0], 2);
    assert_eq!(&wire[8..], 2.5f64.to_le_bytes());
    assert_eq!(Packet::decode(&wire).unwrap(), (sdw, wire.len()));

    let wire = [9, 0, 0, 0, 0, 0, 0, 0, 0xAA, 0xBB];
    let (other, used) = Packet::decode(&wire).unwrap();
    assert_eq!((other.payload, used), (Payload::Described(vec![0xAA, 0xBB]), 10));
}

#[test]
fn a_count_the_holder_states_is_checked_against_the_elements() {
    let mut packet = pdw_packet(true, vec![Pdw::default()]);
    packet.mask = 0;
    packet.num_elements = 2;
    assert_eq!(
        packet.encode(),
        Err(EncodeError::Mismatch { field: "elements", value: 1, stated: 2 }),
    );
}

#[test]
fn an_arm_takes_its_holders_length() {
    let ext = Extension {
        kind: Stated::default(),
        words: 2,
        body: ExtBody::RfExtGeo(RfExtGeo { azimuth: -2, data: vec![1, 2, 3, 4, 5, 6] }),
    };
    let wire = ext.encode().unwrap();
    assert_eq!(wire, [1, 0, 2, 0xFF, 0xFE, 1, 2, 3, 4, 5, 6]);
    assert_eq!(Extension::decode(&wire).unwrap(), (ext, wire.len()));
    assert!(matches!(
        Extension::decode(&[1, 0, 3, 0xFF, 0xFE, 1, 2, 3, 4, 5, 6]),
        Err(ParseError::Short { .. })
    ));
    let wrong = Extension { words: 1, ..Extension::decode(&wire).unwrap().0 };
    assert!(matches!(wrong.encode(), Err(EncodeError::Mismatch { field: "data", .. })));
}

#[test]
fn a_catalogue_takes_its_byte_order_from_the_holder() {
    for (big, wire) in [(false, [0u8, 1, 0x34, 0x12, 0, 0]), (true, [1, 1, 0, 0, 0x12, 0x34])] {
        let sample = Sample { big, kind: Stated::default(), word: Word::Long(0x1234) };
        assert_eq!(sample.encode().unwrap(), wire);
        assert_eq!(Sample::decode(&wire).unwrap(), (sample, 6));
    }
    assert_eq!(
        Word::decode_with(0, &[0x12, 0x34], WordCtx::new(true)),
        Ok((Word::Short(0x1234), 2))
    );
    assert_eq!(<Word as Choice>::PARAMS.len(), 1);
}

#[test]
fn random_bytes_decode_to_what_encodes_back() {
    let mut r = rng::Rng(0xD3_0000_0000_0001);
    let mut whole = 0;
    for _ in 0..20_000 {
        let mut buf = vec![0u8; (r.next() % 48) as usize];
        r.fill(&mut buf);
        if let Some(b) = buf.first_mut() {
            *b = (r.next() % 3) as u8;
        }
        if buf.len() > 7 {
            buf[1..4].fill(0);
            buf[4] &= 0x07;
            buf[6] = 0;
            buf[7] %= 3;
        }
        if let Ok((packet, used)) = Packet::decode(&buf) {
            assert_eq!(packet.encode().unwrap(), &buf[..used]);
            whole += 1;
        }
    }
    assert!(whole > 1000, "the corpus reaches valid packets: {whole}");
}
