//! An encode into a caller's array writes the bytes a vector does, or returns `Overflow`.

#![cfg(feature = "derive")]

use layline::{Buffer, Choice, Dispatch, Fixed, Layout, Message, Overflow};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4)]
struct Head {
    kind: u16,
    seq: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Packet {
    #[bytes(4)]
    head: Head,
    a: u32,
    b: i16,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
struct Bits {
    #[bits(4)]
    version: u8,
    urgent: bool,
    #[present]
    #[bits(12)]
    origin: Option<u16>,
    #[bits(3)]
    priority: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Sized {
    tag: u8,
    len: u8,
    #[switch(tag)]
    #[len(len)]
    value: Body,
}

#[derive(Debug, Clone, PartialEq, Message)]
enum Body {
    #[value(1)]
    #[bytes(4)]
    Head(Head),
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Dispatch, Debug, Clone, PartialEq)]
#[dispatch(id = u8)]
enum Frame {
    #[value(1)]
    Head(Head),
    #[other]
    Unknown { id: u8, body: Vec<u8> },
}

/// The bytes `value` writes into a caller's array, asserted equal to the bytes it writes into a vector.
fn in_place<T: Message<Ctx = ()>>(value: &T) -> Vec<u8> {
    let vector = value.encode();

    let mut room = vec![0xA5u8; vector.len() + 8];
    let mut out = Fixed::new(&mut room);
    value.encode_into(&mut out).expect("eight bytes over is room enough");
    assert_eq!(out.into_written(), vector, "a fixed buffer takes the bytes a vector takes");

    for short in 0..vector.len() {
        let mut room = vec![0u8; short];
        assert_eq!(
            value.encode_into(&mut Fixed::new(&mut room)),
            Err(Overflow),
            "{short} of {} bytes and the encode returned",
            vector.len()
        );
    }
    vector
}

#[test]
fn a_message_of_fixed_fields_encodes_into_an_array() {
    let m = Packet { head: Head { kind: 1, seq: 2 }, a: 7, b: -3 };
    let wire = in_place(&m);
    assert_eq!(wire.len(), 10);
    assert_eq!(Packet::decode(&wire).expect("decodes"), (m, 10));
}

#[test]
fn a_bit_addressed_message_encodes_into_an_array() {
    let m = Bits { version: 6, urgent: true, origin: Some(0x0ABC), priority: 5 };
    let wire = in_place(&m);
    assert_eq!(Bits::decode(&wire).expect("decodes").0, m);
}

#[test]
fn a_wire_stated_window_and_its_arm_encode_into_an_array() {
    let m = Sized { tag: 1, len: 4, value: Body::Head(Head { kind: 9, seq: 8 }) };
    let wire = in_place(&m);
    assert_eq!(Sized::decode(&wire).expect("decodes"), (m, 6));
}

#[test]
fn a_layout_a_choice_and_a_dispatch_take_the_same_buffer() {
    let mut room = [0u8; 4];
    let mut out = Fixed::new(&mut room);
    Head { kind: 1, seq: 2 }.encode_into(&mut out).expect("four bytes");
    assert_eq!(out.len(), 4);

    let mut room = [0u8; 4];
    let mut out = Fixed::new(&mut room);
    Body::Head(Head { kind: 1, seq: 2 }).encode_into(&mut out).expect("four bytes");
    assert_eq!(out.len(), 4);

    let mut room = [0u8; 4];
    let mut out = Fixed::new(&mut room);
    Frame::Head(Head { kind: 1, seq: 2 }).encode_into(&mut out).expect("four bytes");
    assert_eq!(out.len(), 4);

    let mut room = [0u8; 3];
    assert_eq!(
        Frame::Head(Head { kind: 1, seq: 2 }).encode_into(&mut Fixed::new(&mut room)),
        Err(Overflow)
    );
}

#[test]
fn a_refused_encode_leaves_the_segments_written_before_it() {
    let m = Sized { tag: 1, len: 4, value: Body::Head(Head { kind: 9, seq: 8 }) };
    let mut room = [0u8; 4];
    let mut out = Fixed::new(&mut room);
    assert_eq!(m.encode_into(&mut out), Err(Overflow));
    assert_eq!(out.written(), &[1, 4], "the header landed and the four-byte arm did not");
}

#[test]
fn a_block_lands_whole_or_not_at_all() {
    let m = Packet { head: Head { kind: 0x0102, seq: 0x0304 }, a: 0, b: 0 };
    let mut room = [0u8; 9];
    let mut out = Fixed::new(&mut room);
    assert_eq!(m.encode_into(&mut out), Err(Overflow));
    assert!(out.written().is_empty(), "the message is one block of ten bytes");
}
