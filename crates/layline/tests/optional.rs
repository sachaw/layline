//! `#[when]`: an `Option` field that is present when an earlier flag bit is set.
//! Encode sets the flag bit from the field's presence.

#![cfg(feature = "derive")]

#[path = "support/text.rs"]
mod codecs;

use layline::{Layout, Message, ParseError};
use layline_core::num::Uleb128;

#[derive(Debug, Clone, Copy, PartialEq, layline::Layout)]
#[layout(bytes = 6)]
struct Origin {
    lat: i16,
    lon: i32,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Packet {
    flags: u16,
    #[when(flags & 0x01)]
    timestamp: Option<u32>,
    #[when(flags & 0x02)]
    #[bytes(6)]
    origin: Option<Origin>,
    #[when(flags & 0x04)]
    #[text]
    #[until(0)]
    label: Option<String>,
    payload: u32,
}

const ORIGIN: Origin = Origin { lat: -1000, lon: 200_000 };

#[test]
fn present_and_absent_both_round_trip() {
    let none =
        Packet { flags: 0, timestamp: None, origin: None, label: None, payload: 0xDEAD_BEEF };
    assert_eq!(none.encode(), vec![0x00, 0x00, 0xEF, 0xBE, 0xAD, 0xDE]);
    assert_eq!(Packet::decode(&none.encode()), Ok((none, 6)));

    let all = Packet {
        flags: 0x07,
        timestamp: Some(0x0102_0304),
        origin: Some(ORIGIN),
        label: Some("hi".into()),
        payload: 1,
    };
    let wire = all.encode();
    assert_eq!(
        wire,
        vec![
            0x07, 0x00, //
            0x04, 0x03, 0x02, 0x01, //
            0x18, 0xFC, 0x40, 0x0D, 0x03, 0x00, //
            b'h', b'i', 0x00, //
            0x01, 0x00, 0x00, 0x00, //
        ]
    );
    assert_eq!(Packet::decode(&wire), Ok((all, wire.len())));
}

#[test]
fn every_combination_of_bits_round_trips() {
    for bits in 0u16..8 {
        let p = Packet {
            flags: 0,
            timestamp: (bits & 1 != 0).then_some(0x1122_3344),
            origin: (bits & 2 != 0).then_some(ORIGIN),
            label: (bits & 4 != 0).then(|| String::from("x")),
            payload: 0x5566_7788,
        };
        let wire = p.encode();
        assert_eq!(
            u16::from_le_bytes([wire[0], wire[1]]),
            bits,
            "encode writes the flags word from presence, for {bits:#05b}"
        );

        let expected = 2
            + usize::from(bits & 1 != 0) * 4
            + usize::from(bits & 2 != 0) * 6
            + usize::from(bits & 4 != 0) * 2
            + 4;
        assert_eq!(wire.len(), expected, "only the present fields occupy bytes");

        let (back, used) = Packet::decode(&wire).expect("parses");
        assert_eq!(used, wire.len());
        assert_eq!(back, Packet { flags: bits, ..p }, "round trip for {bits:#05b}");
    }
}

#[test]
fn the_flag_is_back_patched_from_presence() {
    let lying = Packet { flags: 0xFFFF, timestamp: None, origin: None, label: None, payload: 0 };
    let wire = lying.encode();
    assert_eq!(
        u16::from_le_bytes([wire[0], wire[1]]),
        0xFFF8,
        "encode clears the three governed bits and keeps the other thirteen"
    );
    assert_eq!(wire.len(), 6, "encode wrote no bytes for the absent fields");

    let other = Packet {
        flags: 0,
        timestamp: Some(7),
        origin: Some(ORIGIN),
        label: Some("q".into()),
        payload: 0,
    };
    let wire = other.encode();
    assert_eq!(
        u16::from_le_bytes([wire[0], wire[1]]),
        0x0007,
        "encode sets the bits from presence"
    );

    let (back, _) = Packet::decode(&wire).expect("parses");
    assert_eq!(back, Packet { flags: 0x0007, ..other });
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Mixed {
    flags: u8,
    #[when(flags & 0x01)]
    extra: Option<u16>,
}

#[test]
fn ungoverned_bits_are_left_alone() {
    let m = Mixed { flags: 0b1010_1110, extra: Some(0xBEEF) };
    assert_eq!(m.encode(), vec![0b1010_1111, 0xEF, 0xBE]);

    let m = Mixed { flags: 0b1010_1111, extra: None };
    assert_eq!(m.encode(), vec![0b1010_1110]);
}

#[test]
fn a_truncated_optional_body_errors() {
    assert_eq!(
        Packet::decode(&[0x01, 0x00, 1, 2, 3]),
        Err(ParseError::Short { need_bytes: 6, got_bytes: 5, at: 2 })
    );
    assert_eq!(
        Packet::decode(&[0x02, 0x00, 1, 2, 3]),
        Err(ParseError::Short { need_bytes: 8, got_bytes: 5, at: 2 })
    );
    assert_eq!(
        Packet::decode(&[0x04, 0x00, b'h', b'i']),
        Err(ParseError::Malformed { field: "label", at: 2 })
    );
    assert_eq!(
        Packet::decode(&[0x01, 0x00, 1, 2, 3, 4]),
        Err(ParseError::Short { need_bytes: 10, got_bytes: 6, at: 6 })
    );
    assert_eq!(
        Packet::decode(&[0x01]),
        Err(ParseError::Short { need_bytes: 2, got_bytes: 1, at: 0 })
    );
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Discovered {
    #[var]
    flags: Uleb128,
    #[when(flags & 0x01)]
    #[var]
    delta: Option<Uleb128>,
    #[when(flags & 0x02)]
    #[text(codecs::Ascii)]
    #[bytes(4)]
    tag: Option<String>,
    trailer: u8,
}

#[test]
fn a_flags_word_and_an_optional_body_may_both_be_discovered() {
    let both = Discovered {
        flags: Uleb128(0),
        delta: Some(Uleb128(300)),
        tag: Some("ab".into()),
        trailer: 9,
    };
    let wire = both.encode();
    assert_eq!(wire, vec![0x03, 0xAC, 0x02, b'a', b'b', 0, 0, 9]);
    assert_eq!(
        Discovered::decode(&wire),
        Ok((Discovered { flags: Uleb128(3), ..both }, wire.len()))
    );

    let neither = Discovered { flags: Uleb128(0xFF), delta: None, tag: None, trailer: 9 };
    let wire = neither.encode();
    assert_eq!(wire, vec![0xFC, 0x01, 9], "0xFF with the two governed bits cleared");
    assert_eq!(
        Discovered::decode(&wire),
        Ok((Discovered { flags: Uleb128(0xFC), ..neither }, wire.len()))
    );
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Greedy {
    flags: u8,
    #[when(flags & 0x80)]
    #[text]
    #[fill]
    note: Option<String>,
}

const OPEN_ENDED: [bool; 4] = [
    <Packet as Message>::OPEN_ENDED,
    <Mixed as Message>::OPEN_ENDED,
    <Discovered as Message>::OPEN_ENDED,
    <Greedy as Message>::OPEN_ENDED,
];

#[test]
fn an_optional_fill_makes_its_message_open_ended() {
    assert_eq!(
        OPEN_ENDED,
        [false, false, false, true],
        "an optional body that fills does so whenever the bit is set, and a rule that \
         held only on the wires where it happened to be clear would not be a rule"
    );

    let g = Greedy { flags: 0, note: Some("all of it".into()) };
    assert_eq!(g.encode(), b"\x80all of it".to_vec());
    assert_eq!(
        Greedy::decode(b"\x80all of it"),
        Ok((Greedy { flags: 0x80, note: Some("all of it".into()) }, 10))
    );
    assert_eq!(Greedy::decode(b"\x00rest"), Ok((Greedy { flags: 0, note: None }, 1)));
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Reading {
    flags: u8,
    #[when(flags & 0x01)]
    sigma: Option<u16>,
    value: i16,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
enum Sample {
    #[value(0)]
    Raw(i16),
    #[value(1)]
    Filtered(Reading),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Frame {
    kind: u8,
    #[switch(kind)]
    sample: Sample,
    crc: u8,
}

#[test]
fn an_optional_field_inside_a_switch_arm() {
    let f = Frame {
        kind: 0,
        sample: Sample::Filtered(Reading { flags: 0xF0, sigma: Some(5), value: -2 }),
        crc: 0xAB,
    };
    let wire = f.encode();
    assert_eq!(wire, vec![1, 0xF1, 5, 0, 0xFE, 0xFF, 0xAB]);
    assert_eq!(
        Frame::decode(&wire),
        Ok((
            Frame {
                kind: 1,
                sample: Sample::Filtered(Reading { flags: 0xF1, sigma: Some(5), value: -2 }),
                crc: 0xAB,
            },
            wire.len()
        ))
    );

    let f = Frame {
        kind: 9,
        sample: Sample::Filtered(Reading { flags: 0x01, sigma: None, value: 7 }),
        crc: 1,
    };
    assert_eq!(f.encode(), vec![1, 0x00, 7, 0, 1], "arm and flag both recomputed");
}

/// The DS wireless-manager MP sub-packet header.
#[derive(Debug, Clone, Copy, PartialEq, layline::Layout)]
#[layout(bits = 16)]
struct WmPacketHeader {
    #[bits(10)]
    length_halfwords: u16,
    #[bits(1)]
    seq_flag: bool,
    #[bits(1)]
    dest_bitmap: bool,
    #[bits(4)]
    spare: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct WmPacket {
    #[bytes(2)]
    header: WmPacketHeader,
    #[when(header.seq_flag)]
    seq: Option<u16>,
    #[when(header.dest_bitmap)]
    dest: Option<u8>,
    #[count(header.length_halfwords, scale = 2)]
    payload: Vec<u8>,
}

fn header_bytes(length_halfwords: u16, seq: bool, dest: bool, spare: u8) -> [u8; 2] {
    let v = length_halfwords
        | (u16::from(seq) << 10)
        | (u16::from(dest) << 11)
        | (u16::from(spare) << 12);
    [(v & 0xFF) as u8, (v >> 8) as u8]
}

#[test]
fn two_bool_flags_gate_two_optional_fields() {
    let both = WmPacket {
        header: WmPacketHeader { length_halfwords: 3, seq_flag: true, dest_bitmap: true, spare: 0 },
        seq: Some(0x1234),
        dest: Some(0x5A),
        payload: vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
    };
    let wire = both.encode();
    #[rustfmt::skip]
    assert_eq!(
        wire,
        vec![
            0x03, 0x0C,
            0x34, 0x12,
            0x5A,
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
        ],
    );
    assert_eq!(header_bytes(3, true, true, 0).to_vec(), wire[..2].to_vec());
    assert_eq!(WmPacket::decode(&wire), Ok((both, 11)));

    let neither = WmPacket {
        header: WmPacketHeader {
            length_halfwords: 2,
            seq_flag: false,
            dest_bitmap: false,
            spare: 0,
        },
        seq: None,
        dest: None,
        payload: vec![0x11, 0x22, 0x33, 0x44],
    };
    let wire = neither.encode();
    assert_eq!(wire, vec![0x02, 0x00, 0x11, 0x22, 0x33, 0x44]);
    assert_eq!(WmPacket::decode(&wire), Ok((neither, 6)));

    let seq_only = WmPacket {
        header: WmPacketHeader {
            length_halfwords: 1,
            seq_flag: true,
            dest_bitmap: false,
            spare: 0,
        },
        seq: Some(0x1234),
        dest: None,
        payload: vec![0x77, 0x88],
    };
    let wire = seq_only.encode();
    assert_eq!(wire, vec![0x01, 0x04, 0x34, 0x12, 0x77, 0x88]);
    assert_eq!(WmPacket::decode(&wire), Ok((seq_only, 6)));

    let dest_only = WmPacket {
        header: WmPacketHeader {
            length_halfwords: 0,
            seq_flag: false,
            dest_bitmap: true,
            spare: 0,
        },
        seq: None,
        dest: Some(0x5A),
        payload: vec![],
    };
    let wire = dest_only.encode();
    assert_eq!(wire, vec![0x00, 0x08, 0x5A]);
    assert_eq!(WmPacket::decode(&wire), Ok((dest_only, 3)));
}

#[test]
fn the_presence_bits_are_back_patched_and_the_spare_is_not() {
    let lying = WmPacket {
        header: WmPacketHeader {
            length_halfwords: 99,
            seq_flag: false,
            dest_bitmap: true,
            spare: 0b1010,
        },
        seq: Some(0x1234),
        dest: None,
        payload: vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
    };

    let wire = lying.encode();
    #[rustfmt::skip]
    assert_eq!(
        wire,
        vec![
            0x03, 0xA4,
            0x34, 0x12,
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF,
        ],
    );
    assert_eq!(header_bytes(3, true, false, 0b1010).to_vec(), wire[..2].to_vec());

    assert_eq!(
        WmPacket::decode(&wire),
        Ok((
            WmPacket {
                header: WmPacketHeader {
                    length_halfwords: 3,
                    seq_flag: true,
                    dest_bitmap: false,
                    spare: 0b1010,
                },
                ..lying
            },
            10
        ))
    );
}

#[test]
fn a_truncated_body_behind_a_bool_errors() {
    assert_eq!(
        WmPacket::decode(&[0x00, 0x04, 0x34]),
        Err(ParseError::Short { need_bytes: 4, got_bytes: 3, at: 2 })
    );
    assert_eq!(
        WmPacket::decode(&[0x00, 0x08]),
        Err(ParseError::Short { need_bytes: 3, got_bytes: 2, at: 2 })
    );
    assert_eq!(
        WmPacket::decode(&[0x04, 0x00]),
        Err(ParseError::Short { need_bytes: 3, got_bytes: 2, at: 2 })
    );
    assert_eq!(
        WmPacket::decode(&[0x00]),
        Err(ParseError::Short { need_bytes: 2, got_bytes: 1, at: 0 })
    );
}

#[test]
fn a_bool_flag_publishes_the_bit_it_is() {
    use layline::table::{By, Presence, Span};

    let rows = <WmPacket as layline::Message>::SEGMENTS;
    let flagged: Vec<(&str, Span, Presence)> = rows
        .iter()
        .filter_map(|r| match r.when {
            Some(when @ Presence::Flag { .. }) => Some((r.name, r.span, when)),
            _ => None,
        })
        .collect();
    assert_eq!(flagged.len(), 2, "two optional fields, two flagged rows");
    for (name, span, when) in flagged {
        let Presence::Flag { field, mask } = when else { unreachable!("just filtered") };
        let expected = if name == "seq" { "header.seq_flag" } else { "header.dest_bitmap" };
        assert_eq!(field, expected);
        assert_eq!(mask, 1, "a `bool` flag has a one-bit mask");
        assert!(matches!(span, Span::Fixed(_)), "the width is the row's own, not the flag's");
    }

    let types = <WmPacketHeader as layline::Layout>::TYPES;
    assert_eq!(
        types.iter().find(|c| c.field == "seq_flag").map(|c| c.ty),
        Some("bool"),
        "the segment row contains the flag bit, and TYPES contains the flag's type"
    );
    assert!(layline::table::type_is(types, "dest_bitmap", "bool"));
    assert!(!layline::table::type_is(types, "length_halfwords", "bool"));

    assert!(
        rows.iter().any(|r| matches!(
            r.span,
            Span::Counted { by: By { field: "header.length_halfwords", scale: 2, .. }, .. }
        )),
        "the payload is counted in halfwords by the same header"
    );
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 6)]
pub struct AssertingTag {
    #[magic(b"TAG")]
    pub sig: [u8; 3],
    pub a: u8,
    pub b: u16,
}

#[derive(Message, Debug, PartialEq)]
pub struct HoldsAnAssertion {
    pub flags: u8,
    #[bytes(6)]
    pub always: AssertingTag,
    #[when(flags & 1)]
    #[bytes(6)]
    pub sometimes: Option<AssertingTag>,
}

#[test]
fn an_optional_field_may_assert_about_its_own_bytes() {
    let tag = AssertingTag { sig: *b"TAG", a: 7, b: 0x0102 };

    for sometimes in [None, Some(tag.clone())] {
        let expected = u8::from(sometimes.is_some());
        let m = HoldsAnAssertion { flags: 0, always: tag.clone(), sometimes };
        let wire = m.encode();
        let (back, used) = HoldsAnAssertion::decode(&wire).expect("it round-trips");
        assert_eq!(back.flags, expected, "encode writes the flag from presence");
        assert_eq!(back.always, m.always, "the mandatory record round-trips");
        assert_eq!(back.sometimes, m.sometimes, "the optional record round-trips");
        assert_eq!(used, wire.len(), "decode consumed every byte encode wrote");
    }
}

#[test]
fn a_present_optional_assertion_reports_its_own_field() {
    let tag = AssertingTag { sig: *b"TAG", a: 7, b: 0x0102 };
    let m = HoldsAnAssertion { flags: 0, always: tag.clone(), sometimes: Some(tag) };
    let mut wire = m.encode();

    let at = wire.len() - 6;
    wire[at] = b'X';
    assert!(
        HoldsAnAssertion::decode(&wire).is_err(),
        "decode checks the nested magic inside the optional field"
    );
}

#[test]
fn a_stated_position_on_an_optional_is_checked_and_kept() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Stated {
        flags: u8,
        #[at(byte = 1)]
        #[when(flags & 0x01)]
        stamp: Option<u16>,
        #[fill]
        extra: Option<u8>,
    }
    let m = Stated { flags: 0, stamp: Some(0xBEEF), extra: None };
    let wire = m.encode();
    assert_eq!(wire, vec![0x01, 0xEF, 0xBE], "encode writes the flag, and the field is at byte 1");
    let (back, used) = Stated::decode(&wire).expect("round-trips");
    assert_eq!((back.stamp, back.extra, used), (Some(0xBEEF), None, 3));

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Trailing {
        n: u16,
        #[at(byte = 2)]
        #[fill]
        extra: Option<u8>,
    }
    let (t, used) = Trailing::decode(&[1, 0, 9]).expect("round-trips");
    assert_eq!((t.n, t.extra, used), (1, Some(9), 3));
}
