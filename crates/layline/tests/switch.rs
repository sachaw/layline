//! `#[switch]`: a field whose arm an earlier value selects.
//! The parent names the discriminant field, and the enum declares the arm values.

#![cfg(feature = "derive")]

use layline::{Message, ParseError};
use layline_core::num::Uleb128;

#[derive(Debug, Clone, PartialEq, Message)]
struct PingBody {
    seq: u16,
}

#[derive(Debug, Clone, PartialEq, layline::Layout)]
#[layout(bytes = 3)]
struct DataHeader {
    channel: u8,
    scale: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
enum Body {
    #[value(0)]
    Ping(PingBody),
    #[value(1)]
    #[bytes(3)]
    Data(DataHeader),
    #[value(2)]
    Level(u8),
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Packet {
    kind: u8,
    #[switch(kind)]
    body: Body,
}

#[test]
fn every_arm_round_trips() {
    let cases: Vec<(Packet, Vec<u8>)> = vec![
        (Packet { kind: 0, body: Body::Ping(PingBody { seq: 0x0102 }) }, vec![0, 0x02, 0x01]),
        (
            Packet { kind: 1, body: Body::Data(DataHeader { channel: 7, scale: 0x0304 }) },
            vec![1, 7, 0x04, 0x03],
        ),
        (Packet { kind: 2, body: Body::Level(200) }, vec![2, 200]),
    ];

    for (packet, wire) in cases {
        assert_eq!(packet.encode(), wire, "{packet:?} encodes");
        let (back, used) = Packet::decode(&wire).expect("parses");
        assert_eq!(back, packet);
        assert_eq!(used, wire.len(), "`used` is the arm's length plus the header");
    }
}

#[test]
fn an_unrecognised_discriminant_keeps_its_bytes() {
    let wire = vec![0x9E, 0xDE, 0xAD, 0xBE, 0xEF];
    let (packet, used) = Packet::decode(&wire).expect("parses");
    assert_eq!(packet, Packet { kind: 0x9E, body: Body::Unknown(vec![0xDE, 0xAD, 0xBE, 0xEF]) });
    assert_eq!(used, wire.len());
    assert_eq!(packet.encode(), wire, "byte-identical, discriminant and all");
}

#[test]
fn the_discriminant_is_recomputed_from_the_arm() {
    let lying = Packet { kind: 99, body: Body::Level(7) };
    assert_eq!(lying.encode(), vec![2, 7], "encode writes the arm's value over the discriminant");

    let unknown = Packet { kind: 99, body: Body::Unknown(vec![7]) };
    assert_eq!(
        unknown.encode(),
        vec![99, 7],
        "the `#[other]` arm keeps the discriminant in the struct"
    );
}

#[test]
fn an_arm_that_overruns_the_buffer_errors() {
    assert_eq!(
        Packet::decode(&[0, 0x01]),
        Err(ParseError::Short { need_bytes: 2, got_bytes: 1, at: 1 })
    );
    assert_eq!(Packet::decode(&[1]), Err(ParseError::Short { need_bytes: 3, got_bytes: 0, at: 1 }));
    assert_eq!(Packet::decode(&[]), Err(ParseError::Short { need_bytes: 1, got_bytes: 0, at: 0 }));
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
enum Reading {
    #[value(0)]
    Celsius(i16),
    #[value(1)]
    Fahrenheit(i16),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Sample {
    unit: u8,
    #[switch(unit)]
    reading: Reading,
    checksum: u16,
}

#[test]
fn a_bounded_switch_may_be_followed_by_more_fields() {
    let s = Sample { unit: 1, reading: Reading::Fahrenheit(-40), checksum: 0xBEEF };
    let wire = s.encode();
    assert_eq!(wire, vec![1, 0xD8, 0xFF, 0xEF, 0xBE]);
    assert_eq!(Sample::decode(&wire), Ok((s, 5)));
}

#[test]
fn a_closed_catalogue_refuses_what_it_does_not_list() {
    assert_eq!(
        Sample::decode(&[7, 0, 0, 0, 0]),
        Err(ParseError::Malformed { field: "Reading", at: 1 })
    );
}

const OPEN_ENDED: [bool; 5] = [
    <Body as layline::Choice>::OPEN_ENDED,
    <Packet as Message>::OPEN_ENDED,
    <Reading as layline::Choice>::OPEN_ENDED,
    <Sample as Message>::OPEN_ENDED,
    <PingBody as Message>::OPEN_ENDED,
];

#[test]
fn openness_propagates_to_the_message_that_holds_it() {
    assert_eq!(
        OPEN_ENDED,
        [true, true, false, false, false],
        "the unknown arm keeps everything, so `Body` does and so does the `Packet` that \
         ends with it; `Reading` is closed with every arm bounded, so `Sample` may put a \
         checksum after it"
    );
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Tagged {
    #[var]
    tag: Uleb128,
    #[switch(tag)]
    body: Body,
}

#[test]
fn a_discriminant_may_be_a_discovered_varint() {
    let ping = Tagged { tag: Uleb128(0), body: Body::Ping(PingBody { seq: 9 }) };
    assert_eq!(ping.encode(), vec![0, 9, 0]);
    assert_eq!(Tagged::decode(&[0, 9, 0]), Ok((ping, 3)));

    let wire = vec![0xAC, 0x02, 1, 2, 3];
    let (t, used) = Tagged::decode(&wire).expect("parses");
    assert_eq!(t.tag, Uleb128(300));
    assert_eq!(t.body, Body::Unknown(vec![1, 2, 3]));
    assert_eq!(used, wire.len());
    assert_eq!(t.encode(), wire, "the varint tag survives the round trip");
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Coarse {
    x: u16,
    y: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Fine {
    x: u32,
    y: u32,
}

#[derive(Debug, Clone, PartialEq, Message)]
enum Reported {
    #[value(4)]
    Coarse(Coarse),
    #[value(8)]
    Fine(Fine),
    #[other]
    Unrecognised(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Measurement {
    seq: u8,
    #[switch(..)]
    reported: Reported,
}

#[test]
fn a_switch_may_discriminate_on_the_bytes_that_are_left() {
    let coarse =
        Measurement { seq: 7, reported: Reported::Coarse(Coarse { x: 0x1122, y: 0x3344 }) };
    let fine =
        Measurement { seq: 8, reported: Reported::Fine(Fine { x: 0xAABB_CCDD, y: 0x1122_3344 }) };

    assert_eq!(coarse.encode(), vec![7, 0x22, 0x11, 0x44, 0x33]);
    assert_eq!(fine.encode(), vec![8, 0xDD, 0xCC, 0xBB, 0xAA, 0x44, 0x33, 0x22, 0x11]);

    assert_eq!(coarse.encode().len(), 5);
    assert_eq!(fine.encode().len(), 9);

    for s in [&coarse, &fine] {
        let w = s.encode();
        assert_eq!(Measurement::decode(&w), Ok((s.clone(), w.len())), "{s:?}");
    }
}

#[test]
fn a_length_no_arm_claims_is_still_a_message() {
    let wire = vec![9, 1, 2, 3];
    let (s, used) = Measurement::decode(&wire).expect("parses");
    assert_eq!(s.reported, Reported::Unrecognised(vec![1, 2, 3]));
    assert_eq!(used, wire.len());
    assert_eq!(s.encode(), wire, "an unclaimed length round-trips losslessly");
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Frame {
    kind: u8,
    len: u8,
    #[switch(kind)]
    #[len(len)]
    body: Body,
    crc: u16,
}

#[test]
fn a_u8_length_windows_the_switch_and_a_field_follows_it() {
    let cases: Vec<(Frame, Vec<u8>)> = vec![
        (
            Frame { kind: 0, len: 99, body: Body::Ping(PingBody { seq: 0x0102 }), crc: 0xBEEF },
            vec![0, 2, 0x02, 0x01, 0xEF, 0xBE],
        ),
        (
            Frame {
                kind: 1,
                len: 99,
                body: Body::Data(DataHeader { channel: 7, scale: 0x0304 }),
                crc: 0xBEEF,
            },
            vec![1, 3, 7, 0x04, 0x03, 0xEF, 0xBE],
        ),
        (Frame { kind: 2, len: 99, body: Body::Level(200), crc: 1 }, vec![2, 1, 200, 1, 0]),
        (
            Frame { kind: 0x9E, len: 99, body: Body::Unknown(vec![0xDE, 0xAD]), crc: 1 },
            vec![0x9E, 2, 0xDE, 0xAD, 1, 0],
        ),
    ];
    for (frame, wire) in cases {
        assert_eq!(frame.encode(), wire, "{frame:?} encodes with the recomputed length");
        let (back, used) = Frame::decode(&wire).expect("parses");
        assert_eq!(used, wire.len());
        assert_eq!(back.body, frame.body);
        assert_eq!(back.crc, frame.crc, "decode reads `crc` after the window");
        assert_eq!(back.len, wire[1], "the length is what was written");
    }
}

#[test]
#[should_panic(expected = "too narrow for the byte length of a switch")]
fn an_arm_too_long_for_its_length_field_is_refused_on_encode() {
    let f = Frame { kind: 9, len: 0, body: Body::Unknown(vec![0; 256]), crc: 0 };
    let _ = f.encode();
}

#[test]
fn the_window_is_checked_against_the_arm() {
    assert_eq!(
        Frame::decode(&[0, 1, 0x02, 0, 0]),
        Err(ParseError::Malformed { field: "body", at: 2 })
    );
    assert_eq!(
        Frame::decode(&[0, 3, 0x02, 0x01, 0, 0, 0]),
        Err(ParseError::Malformed { field: "body", at: 2 })
    );
    assert_eq!(
        Frame::decode(&[0, 9, 0x02]),
        Err(ParseError::Short { need_bytes: 11, got_bytes: 3, at: 2 })
    );
}
