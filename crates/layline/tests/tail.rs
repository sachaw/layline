//! Fields anchored to the end of a message: `#[at(end)]`. Decode reads them first, from the end,
//! so a `#[fill]` before them stops where they start, as does a switch arm, listed or not. RTP's
//! padding (RFC 3550 §5.1), a CRC trailer over everything before it, and JEL's DW packet, whose
//! conditions on fixed fields take no bytes, with every byte worked out by hand.

#![cfg(feature = "derive")]

use layline::checksum::Crc;
use layline::{EncodeError, Layout, Message, ParseError, Stated};

/// CRC-32/ISO-HDLC, as Ethernet and zlib use.
type Crc32IsoHdlc = Crc<u32, 0xEDB8_8320, 0xFFFF_FFFF, 0xFFFF_FFFF, true>;

#[path = "support/rng.rs"]
mod rng;

/// The RTP fixed header, as far as padding needs it.
#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 32, endian = be, order = msb)]
pub struct RtpHeader {
    #[bits(2)]
    pub version: u8,
    /// Whether the last byte of the packet counts padding bytes at its end.
    #[bits(1)]
    pub padding: bool,
    #[bits(1)]
    pub extension: bool,
    #[bits(4)]
    pub csrc_count: u8,
    #[bits(8)]
    pub payload_type: u8,
    #[bits(16)]
    pub sequence_number: u16,
}

/// An RTP packet whose payload runs up to its padding. The last byte counts the padding,
/// itself included, when the header's padding bit is set.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Rtp {
    #[bytes(4)]
    pub header: RtpHeader,
    #[count(header.csrc_count)]
    pub csrc: Vec<u32>,
    #[fill]
    pub payload: Vec<u8>,
    #[at(end)]
    #[when(header.padding)]
    #[count(pad_count - 1)]
    pub padding: Option<Vec<u8>>,
    #[at(end)]
    #[when(header.padding)]
    pub pad_count: Stated<u8>,
}

/// A frame with a CRC-32 trailer over everything before it.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = le)]
pub struct Sealed {
    pub kind: u8,
    #[fill]
    pub data: Vec<u8>,
    #[at(end)]
    #[checksum(Crc32IsoHdlc, over = kind..=data)]
    pub crc: u32,
}

/// A trailer whose length its last two bytes state: Kaitai's `_io.size`-relative sizes.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Trailed {
    #[fill]
    pub body: Vec<u8>,
    #[at(end)]
    #[len(trailer_len)]
    pub trailer: Vec<u8>,
    #[at(end)]
    pub trailer_len: Stated<u16>,
}

/// A tail after a body of fixed size: the body must end where the tail starts.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Footed {
    pub head: u16,
    #[at(end)]
    pub foot: u8,
}

fn rtp(padding: Option<Vec<u8>>) -> Rtp {
    Rtp {
        header: RtpHeader {
            version: 2,
            padding: padding.is_some(),
            payload_type: 96,
            sequence_number: 7,
            ..RtpHeader::default()
        },
        csrc: Vec::new(),
        payload: vec![0xDE, 0xAD, 0xBE, 0xEF],
        padding,
        pad_count: Stated::default(),
    }
}

#[test]
fn padding_is_the_last_byte_and_the_payload_stops_before_it() {
    let padded = rtp(Some(vec![0, 0, 0]));
    let wire = padded.encode().unwrap();
    #[rustfmt::skip]
    assert_eq!(wire, [
        0xA0, 96, 0, 7,             // V=2, P=1
        0xDE, 0xAD, 0xBE, 0xEF,     // payload
        0, 0, 0, 4,                 // three bytes of padding, then the count, itself included
    ]);
    let (back, used) = Rtp::decode(&wire).unwrap();
    assert_eq!((used, back.pad_count.get()), (wire.len(), 4));
    assert_eq!(back, padded);
    assert_eq!(back.payload, [0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn a_count_of_one_is_the_count_byte_alone() {
    let wire = rtp(Some(Vec::new())).encode().unwrap();
    assert_eq!(&wire[8..], [1]);
    assert_eq!(Rtp::decode(&wire).unwrap().0.padding, Some(Vec::new()));
}

#[test]
fn without_the_padding_bit_the_payload_runs_to_the_end() {
    let plain = rtp(None);
    let wire = plain.encode().unwrap();
    assert_eq!(wire, [0x80, 96, 0, 7, 0xDE, 0xAD, 0xBE, 0xEF]);
    let (back, used) = Rtp::decode(&wire).unwrap();
    assert_eq!((back, used), (plain, 8));
    // The last byte is payload, whatever it holds.
    let (back, _) = Rtp::decode(&[0x80, 96, 0, 7, 1, 2, 3, 4]).unwrap();
    assert_eq!((back.payload.len(), back.padding), (4, None));
}

#[test]
fn csrcs_come_before_the_payload_and_padding_after() {
    let mut packet = rtp(Some(vec![9]));
    packet.csrc = vec![0x0102_0304];
    packet.header.csrc_count = 1;
    let wire = packet.encode().unwrap();
    assert_eq!(wire, [0xA1, 96, 0, 7, 1, 2, 3, 4, 0xDE, 0xAD, 0xBE, 0xEF, 9, 2]);
    assert_eq!(Rtp::decode(&wire).unwrap(), (packet, wire.len()));
}

#[test]
fn a_hostile_padding_count_is_refused() {
    // A count of zero counts not even itself.
    assert!(matches!(
        Rtp::decode(&[0xA0, 96, 0, 7, 1, 2, 0]),
        Err(ParseError::Malformed { field: "pad_count", .. })
    ));
    // A count past the payload runs into the header.
    assert!(matches!(Rtp::decode(&[0xA0, 96, 0, 7, 1, 2, 9]), Err(ParseError::Short { .. })));
    // Exactly the bytes after the header is fine: an empty payload.
    let (back, _) = Rtp::decode(&[0xA0, 96, 0, 7, 0, 0, 3]).unwrap();
    assert_eq!((back.payload.len(), back.padding), (0, Some(vec![0, 0])));
    // A padded packet with no room for the count.
    assert!(matches!(Rtp::decode(&[0xA0, 96, 0, 7]), Err(ParseError::Short { .. })));
}

#[test]
fn encode_refuses_padding_the_count_cannot_hold() {
    assert!(matches!(
        rtp(Some(vec![0; 255])).encode(),
        Err(EncodeError::Unfit { field: "pad_count", .. })
    ));
    assert!(rtp(Some(vec![0; 254])).encode().is_ok());
}

#[test]
fn a_crc_trailer_covers_everything_before_it() {
    let sealed = Sealed { kind: 1, data: b"123456789".to_vec(), crc: 0 };
    let wire = sealed.encode();
    let crc = <Crc32IsoHdlc as layline::Checksum>::compute(&wire[..10]);
    assert_eq!(&wire[10..], crc.to_le_bytes());
    let (back, used) = Sealed::decode(&wire).unwrap();
    assert_eq!((back.kind, &back.data[..], back.crc, used), (1, &b"123456789"[..], crc, 14));

    let mut bent = wire.clone();
    bent[3] ^= 1;
    assert!(matches!(Sealed::decode(&bent), Err(ParseError::Checksum { field: "crc", .. })));
    assert!(matches!(Sealed::decode(&wire[..3]), Err(ParseError::Short { .. })));
}

#[test]
fn a_trailer_states_its_own_length_from_the_end() {
    let t = Trailed { body: vec![1, 2, 3], trailer: vec![8, 9], trailer_len: Stated::default() };
    let wire = t.encode().unwrap();
    assert_eq!(wire, [1, 2, 3, 8, 9, 0, 2]);
    assert_eq!(Trailed::decode(&wire).unwrap(), (t, 7));
    assert!(matches!(Trailed::decode(&[1, 0, 9]), Err(ParseError::Short { .. })));
}

#[test]
fn a_byte_between_the_body_and_the_tail_is_refused() {
    let footed = Footed { head: 0x0201, foot: 3 };
    assert_eq!(footed.encode(), [1, 2, 3]);
    assert_eq!(Footed::decode(&[1, 2, 3]), Ok((footed, 3)));
    assert_eq!(Footed::decode(&[1, 2, 9, 3]), Err(ParseError::Malformed { field: "foot", at: 2 }));
    assert!(matches!(Footed::decode(&[1, 2]), Err(ParseError::Short { .. })));
}

#[test]
fn the_table_counts_the_tail_from_the_end() {
    use layline::table::{Span, Start};
    let rows = <Rtp as Message>::SEGMENTS;
    let last = &rows[rows.len() - 1];
    assert_eq!(last.name, "pad_count");
    assert_eq!(last.start, Start::Tail { before: None, bits: 0 });
    assert_eq!(last.span, Span::Fixed(8));
    assert!(last.when.is_some(), "on the wire when the padding bit is set");
    // The count may be absent, so the padding ends where it starts.
    let padding = &rows[rows.len() - 2];
    assert_eq!(padding.name, "padding");
    assert_eq!(padding.start, Start::Tail { before: Some("pad_count"), bits: 0 });
    let crc = <Sealed as Message>::SEGMENTS.last().unwrap();
    assert_eq!((crc.name, crc.start), ("crc", Start::Tail { before: None, bits: 0 }));
    const _: () = assert!(<Rtp as Message>::OPEN_ENDED);
}

#[test]
fn random_bytes_decode_to_what_encodes_back() {
    let mut r = rng::Rng(0x7A11_0000_0000_0001);
    let mut padded = 0;
    for _ in 0..20_000 {
        let mut buf = vec![0u8; 4 + (r.next() % 24) as usize];
        r.fill(&mut buf);
        buf[0] = 0x80 | (buf[0] & 0x20);
        if let Some(last) = buf.last_mut() {
            *last %= 8;
        }
        if let Ok((packet, used)) = Rtp::decode(&buf) {
            assert_eq!(used, buf.len());
            assert_eq!(packet.encode().unwrap(), buf);
            padded += usize::from(packet.padding.is_some());
        }
        if let Ok((sealed, used)) = Sealed::decode(&buf) {
            assert_eq!(sealed.encode(), &buf[..used]);
        }
    }
    assert!(padded > 1000, "the corpus reaches padded packets: {padded}");
}

/// RFC 3550's header extension profile as JEL's DW stream sends it.
const DW_PROFILE: u16 = 16;

/// The extension's length in 32-bit words.
const EXTENSION_WORDS: u16 = 2;

/// The arm a payload type other than 1 selects.
const DESCRIBED: u8 = 128;

/// Elements of one 32-bit word each, up to the tail.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Words {
    #[fill]
    pub words: Vec<u32>,
}

/// A DW payload: both arms are listed, and both read to where the holder's tail starts.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(closed)]
pub enum DwPayload {
    #[value(1)]
    Words(Words),
    /// The bytes a schema read at run time describes.
    #[value(128)]
    Described(Vec<u8>),
}

/// JEL's DW packet: conditions on its fixed fields, a listed arm that reads to the end, and RTP
/// padding in the tail.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Dw {
    #[bytes(4)]
    #[valid(header.extension)]
    pub header: RtpHeader,
    #[valid(profile == DW_PROFILE)]
    pub profile: u16,
    #[valid(extension_words == EXTENSION_WORDS)]
    pub extension_words: u16,
    #[switch(if header.payload_type == 1 { 1 } else { DESCRIBED })]
    pub payload: DwPayload,
    #[at(end)]
    #[when(header.padding)]
    #[count(pad_count - 1)]
    pub padding: Option<Vec<u8>>,
    #[at(end)]
    #[when(header.padding)]
    pub pad_count: Stated<u8>,
}

/// A condition before a switch that reads to the end, with no tail: only the words arm holds.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Tagged {
    #[valid(kind == 1)]
    pub kind: u8,
    #[switch(kind)]
    pub payload: DwPayload,
}

/// A condition after a field that reads to the end leaves the message open-ended.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Rest {
    #[valid(lead != 0)]
    pub lead: u8,
    #[fill]
    pub rest: Vec<u8>,
}

fn dw(payload_type: u8, payload: DwPayload, padding: Option<Vec<u8>>) -> Dw {
    Dw {
        header: RtpHeader {
            version: 2,
            padding: padding.is_some(),
            extension: true,
            payload_type,
            sequence_number: 7,
            ..RtpHeader::default()
        },
        profile: DW_PROFILE,
        extension_words: EXTENSION_WORDS,
        payload,
        padding,
        pad_count: Stated::default(),
    }
}

#[test]
fn a_listed_arm_reads_to_the_tail() {
    let described = dw(0, DwPayload::Described(vec![0xAA, 0xBB, 0xCC]), Some(vec![0]));
    let wire = described.encode().unwrap();
    #[rustfmt::skip]
    assert_eq!(wire, [
        0xB0, 0, 0, 7,      // V=2, P=1, X=1, payload type 0
        0, 16, 0, 2,        // the profile and the extension's length
        0xAA, 0xBB, 0xCC,   // the described bytes, up to the padding
        0, 2,               // one byte of padding, then the count
    ]);
    assert_eq!(Dw::decode(&wire), Ok((described, wire.len())));

    let words = dw(1, DwPayload::Words(Words { words: vec![0x0102_0304] }), None);
    let wire = words.encode().unwrap();
    assert_eq!(wire, [0x90, 1, 0, 7, 0, 16, 0, 2, 1, 2, 3, 4]);
    assert_eq!(Dw::decode(&wire), Ok((words, wire.len())));
    // Words stop at the padding too, and a word cut short by it is refused.
    let (padded, _) = Dw::decode(&[0xB0, 1, 0, 7, 0, 16, 0, 2, 1, 2, 3, 4, 9, 9, 3]).unwrap();
    assert_eq!(padded.payload, DwPayload::Words(Words { words: vec![0x0102_0304] }));
    assert!(Dw::decode(&[0xB0, 1, 0, 7, 0, 16, 0, 2, 1, 2, 3, 4, 9, 9, 2]).is_err());
}

#[test]
fn conditions_on_fixed_fields_hold_before_a_switch_and_a_tail() {
    let good = [0x90, 9, 0, 7, 0, 16, 0, 2, 5];
    assert_eq!(Dw::decode(&good).unwrap().0.payload, DwPayload::Described(vec![5]));
    let refused = |wire: &[u8]| match Dw::decode(wire) {
        Err(ParseError::Malformed { field, .. }) => field,
        other => panic!("{other:?}"),
    };
    assert_eq!(refused(&[0x80, 9, 0, 7, 0, 16, 0, 2, 5]), "header");
    assert_eq!(refused(&[0x90, 9, 0, 7, 0, 17, 0, 2, 5]), "profile");
    assert_eq!(refused(&[0x90, 9, 0, 7, 0, 16, 0, 3, 5]), "extension_words");

    let mut plain = dw(0, DwPayload::Described(vec![]), None);
    plain.header.extension = false;
    assert_eq!(plain.encode(), Err(EncodeError::Mismatch { field: "header", value: 0, stated: 1 }));
    let mut other = dw(0, DwPayload::Described(vec![]), None);
    other.profile = 0xBEDE;
    assert_eq!(
        other.encode(),
        Err(EncodeError::Mismatch { field: "profile", value: 0, stated: 1 })
    );
    // The selector reads one field through a condition, so encode checks the arm against it.
    assert_eq!(
        dw(1, DwPayload::Described(vec![]), None).encode(),
        Err(EncodeError::Mismatch { field: "payload", value: 128, stated: 1 })
    );
}

#[test]
fn a_condition_takes_no_bytes_after_what_reads_to_the_end() {
    let tagged = Tagged { kind: 1, payload: DwPayload::Words(Words { words: vec![7] }) };
    assert_eq!(tagged.encode().unwrap(), [1, 0, 0, 0, 7]);
    assert_eq!(Tagged::decode(&[1, 0, 0, 0, 7]), Ok((tagged, 5)));
    // The arm reads first, and the condition holds for the whole value once it has.
    assert!(matches!(Tagged::decode(&[128, 1]), Err(ParseError::Malformed { field: "kind", .. })));
    let described = Tagged { kind: 128, payload: DwPayload::Described(vec![1]) };
    assert_eq!(
        described.encode(),
        Err(EncodeError::Mismatch { field: "kind", value: 0, stated: 1 })
    );

    assert_eq!(Rest::decode(&[1, 2, 3]), Ok((Rest { lead: 1, rest: vec![2, 3] }, 3)));
    assert!(matches!(Rest::decode(&[0, 2]), Err(ParseError::Malformed { field: "lead", .. })));
    const _: () = assert!(<Tagged as Message>::OPEN_ENDED && <Rest as Message>::OPEN_ENDED);
    const _: () = assert!(<DwPayload as layline::Choice>::OPEN_ENDED);
}

#[test]
fn hostile_dw_packets_are_refused_or_encode_back() {
    let wire = dw(1, DwPayload::Words(Words { words: vec![1, 2] }), Some(vec![0, 0, 0])).encode();
    let wire = wire.unwrap();
    for cut in 0..wire.len() {
        if let Ok((packet, used)) = Dw::decode(&wire[..cut]) {
            assert_eq!(packet.encode().unwrap(), &wire[..used]);
        }
    }
    let mut r = rng::Rng(0x7A11_0000_0000_0D30);
    let mut read = [0usize; 2];
    for _ in 0..20_000 {
        let mut buf = vec![0u8; 8 + (r.next() % 16) as usize];
        r.fill(&mut buf);
        buf[0] = 0x90 | (buf[0] & 0x2F);
        buf[1] %= 3;
        if !r.next().is_multiple_of(8) {
            buf[4..8].copy_from_slice(&[0, 16, 0, 2]);
        }
        if let Some(last) = buf.last_mut() {
            *last %= 8;
        }
        if let Ok((packet, used)) = Dw::decode(&buf) {
            assert_eq!(used, buf.len());
            assert_eq!(packet.encode().unwrap(), buf, "{packet:?}");
            read[usize::from(matches!(packet.payload, DwPayload::Described(_)))] += 1;
        }
    }
    assert!(read.iter().all(|&n| n > 1000), "the corpus reaches both arms: {read:?}");
}
