//! Random bytes into `decode`, over every segment shape.
//! A wire nobody wrote must give an error, and a wire that decodes must survive a round trip.

#![cfg(feature = "derive")]

use layline::checksum::Crc;
use layline::{Fixed, Layout, Message, Overflow};

type Crc16Ccitt = Crc<u16, 0x1021, 0xFFFF, 0x0000, false>;

#[path = "support/rng.rs"]
mod rng;

/// Random wires per type, and the longest wire.
const WIRES: usize = 4096;
const LONGEST: usize = 64;

/// Decode random wires, and check that each decoded value round-trips.
///
/// The value must also encode into an exact-size array, and fail one byte short.
/// Returns how many wires decoded.
fn survives<T: Message<Ctx = ()> + PartialEq + core::fmt::Debug>(seed: u64) -> usize {
    let mut r = rng::Rng(seed);
    let mut decoded = 0usize;
    for i in 0..WIRES {
        let len = (r.next() as usize) % (LONGEST + 1);
        let mut wire = vec![0u8; len];
        r.fill(&mut wire);
        let Ok((value, used)) = T::decode(&wire) else { continue };
        decoded += 1;
        assert!(used <= wire.len(), "wire {i}: consumed {used} of {}", wire.len());

        let out = value.encode();
        let mut room = vec![0u8; out.len()];
        let mut fixed = Fixed::new(&mut room);
        assert_eq!(value.encode_into(&mut fixed), Ok(()), "wire {i}: the exact length overflowed");
        assert_eq!(fixed.into_written(), out, "wire {i}: a fixed buffer took other bytes");
        if !out.is_empty() {
            let mut room = vec![0u8; out.len() - 1];
            assert_eq!(
                value.encode_into(&mut Fixed::new(&mut room)),
                Err(Overflow),
                "wire {i}: one byte short and the encode returned"
            );
        }

        let (again, used_again) = T::decode(&out)
            .unwrap_or_else(|e| panic!("wire {i}: re-encoded to bytes it refuses: {e:?}"));
        assert_eq!(value, again, "wire {i}: a round trip changed the value");
        assert_eq!(used_again, out.len(), "wire {i}: re-encode left bytes over");
    }
    decoded
}

/// [`survives`], and fail if no wire decoded.
fn survives_and_decodes<T: Message<Ctx = ()> + PartialEq + core::fmt::Debug>(seed: u64) {
    assert!(survives::<T>(seed) > 0, "no random wire decoded, so nothing was proven");
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4)]
struct Point {
    x: u16,
    y: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
enum Body {
    #[value(1)]
    #[bytes(4)]
    Point(Point),
    #[value(2)]
    Text(Label),
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Label {
    n: u8,
    #[text]
    #[len(n)]
    text: String,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Tlv {
    tag: u8,
    len: u8,
    #[switch(tag)]
    #[len(len)]
    value: Body,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Counted {
    n: u8,
    #[count(n)]
    #[bytes(4)]
    items: Vec<Point>,
    #[fill]
    tail: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Checked {
    len: u8,
    #[count(len)]
    body: Vec<u8>,
    #[checksum(Crc16Ccitt, over = len..)]
    crc: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Optional {
    flags: u8,
    #[when(flags & 0x01)]
    a: Option<u32>,
    #[when(flags & 0x02)]
    b: Option<u16>,
    #[fill]
    rest: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Nest {
    depth: u8,
    #[len(depth)]
    #[message]
    kids: Vec<Leaf>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Leaf {
    tag: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
struct BitHeader {
    #[bits(4)]
    version: u8,
    urgent: bool,
    #[present]
    #[bits(12)]
    origin: Option<u16>,
    #[bits(3)]
    priority: u8,
}

/// A coding whose width is read from the wire, between two fixed-width fields.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
struct BitRun {
    #[bits(3)]
    channel: u8,
    #[var]
    length: layline_core::num::ExpGolomb,
    #[bits(4)]
    tail: u8,
}

/// One bit field enabled by a mask of an earlier field, and one by an inline presence bit.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
struct BitGated {
    #[bits(4)]
    flags: u8,
    #[when(flags & 0x1)]
    #[bits(12)]
    origin: Option<u16>,
    #[present]
    #[bits(5)]
    level: Option<u8>,
    #[bits(3)]
    priority: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Framed {
    magic: u16,
    #[message]
    header: BitHeader,
    #[fill]
    body: Vec<u8>,
}

#[test]
fn a_switch_in_a_wire_stated_window() {
    survives_and_decodes::<Tlv>(0x5EED_0001);
}

#[test]
fn a_counted_run_and_a_fill() {
    survives_and_decodes::<Counted>(0x5EED_0002);
}

/// A random wire rarely passes a 16-bit checksum, so this only checks that `decode` returns.
#[test]
fn a_checksum_over_an_earlier_field() {
    survives::<Checked>(0x5EED_0003);
}

#[test]
fn a_flipped_byte_under_a_checksum_is_refused() {
    let good = Checked { len: 3, body: vec![1, 2, 3], crc: 0 }.encode();
    assert_eq!(Checked::decode(&good).expect("encode writes the check value").1, good.len());
    for i in 0..good.len() {
        let mut bad = good.clone();
        bad[i] ^= 0x80;
        assert!(Checked::decode(&bad).is_err(), "byte {i} flipped and the wire still decoded");
    }
}

#[test]
fn optional_fields_behind_a_flags_word() {
    survives_and_decodes::<Optional>(0x5EED_0004);
}

#[test]
fn a_length_gated_run_of_sub_messages() {
    survives_and_decodes::<Nest>(0x5EED_0005);
}

#[test]
fn a_bit_addressed_message() {
    survives_and_decodes::<BitHeader>(0x5EED_0006);
}

#[test]
fn a_bit_addressed_message_inside_a_byte_addressed_one() {
    survives_and_decodes::<Framed>(0x5EED_0007);
}

#[test]
fn a_bit_addressed_message_with_a_coding_of_its_own_width() {
    survives_and_decodes::<BitRun>(0x5EED_0009);
}

#[test]
fn a_bit_addressed_message_whose_fields_are_gated_on_an_earlier_one() {
    survives_and_decodes::<BitGated>(0x5EED_000A);
}

#[test]
fn a_length_prefixed_string() {
    survives_and_decodes::<Label>(0x5EED_0008);
}
