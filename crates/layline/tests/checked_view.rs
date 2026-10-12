//! A view of a checked layout reads any bytes, and verifies, checks and seals them on request.

#![cfg(feature = "derive")]

use layline::checksum::Crc;
use layline::{Checksum, Layout, ParseError, View};

/// The wrapping sum of the covered bytes.
pub struct Sum8;

impl Checksum for Sum8 {
    type Output = u8;
    type State = u8;

    fn init() -> u8 {
        0
    }

    fn update(state: u8, bytes: &[u8]) -> u8 {
        bytes.iter().fold(state, |sum, &b| sum.wrapping_add(b))
    }

    fn finish(state: u8) -> u8 {
        state
    }
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 6, view)]
pub struct Record {
    #[magic(b"RC")]
    pub tag: [u8; 2],
    pub counter: u16,
    pub flags: u8,
    #[checksum(Sum8, over = ..)]
    pub sum: u8,
}

/// "RC", counter 0x0102, flags 0x10, and 0x52 + 0x43 + 0x02 + 0x01 + 0x10 = 0xA8.
const GOOD: [u8; 6] = [0x52, 0x43, 0x02, 0x01, 0x10, 0xA8];

#[test]
fn a_checked_view_passes_what_decode_passes() {
    let view = RecordView::checked(&GOOD).expect("the sum matches");
    assert_eq!(view.counter(), 0x0102);
    assert_eq!(view.verify(), Ok(()));
    assert_eq!(Record::decode(&GOOD), Ok(view.decode()));
}

#[test]
fn an_unchecked_view_reads_a_corrupt_record_and_says_why() {
    let mut torn = GOOD;
    torn[2] = 0x03;
    let view = RecordView::from_wire(&torn);
    assert_eq!(view.counter(), 0x0103);
    assert_eq!(view.decode(), Record { tag: *b"RC", counter: 0x0103, flags: 0x10, sum: 0xA8 });
    let wrong = ParseError::Checksum { field: "sum", expected: 0xA9, actual: 0xA8 };
    assert_eq!(view.verify(), Err(wrong));
    assert_eq!(RecordView::checked(&torn), Err(wrong));
    assert_eq!(Record::decode(&torn), Err(wrong));

    let mut foreign = GOOD;
    foreign[0] = b'X';
    assert_eq!(
        RecordView::from_wire(&foreign).verify(),
        Err(ParseError::Magic { field: "tag", expected: b"RC", at: 0 })
    );
}

#[test]
fn seal_restamps_after_the_setters() {
    let mut wire = GOOD;
    let view = RecordView::checked_mut(&mut wire).expect("the sum matches");
    view.set_counter(0x0203);
    assert!(view.verify().is_err(), "the setter leaves the sum stale");
    view.seal();
    assert_eq!(view.verify(), Ok(()));
    // 0x52 + 0x43 + 0x03 + 0x02 + 0x10 = 0xAA.
    assert_eq!(wire, [0x52, 0x43, 0x03, 0x02, 0x10, 0xAA]);

    let mut blank = [0u8; 6];
    let view = RecordView::from_wire_mut(&mut blank);
    view.seal();
    // The magic is written too: 0x52 + 0x43 = 0x95.
    assert_eq!(blank, [0x52, 0x43, 0x00, 0x00, 0x00, 0x95]);
}

#[test]
fn the_trait_checks_the_length_first() {
    assert_eq!(
        <RecordView as View>::checked(&GOOD[..5]),
        Err(ParseError::Short { need_bytes: 6, got_bytes: 5, at: 0 })
    );
    assert!(<RecordView as View>::checked(&GOOD).is_ok());
}

type Crc16Modbus = Crc<u16, 0xA001, 0xFFFF, 0x0000, true>;

/// CRC-16/MODBUS of "123456789" is 0x4B37, the catalogue's check value.
#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 11, view)]
pub struct Text {
    pub text: [u8; 9],
    #[checksum(Crc16Modbus, over = ..)]
    pub crc: u16,
}

#[test]
fn a_crc_checks_in_place() {
    let mut wire = [0u8; 11];
    wire[..9].copy_from_slice(b"123456789");
    wire[9..].copy_from_slice(&[0x37, 0x4B]);
    assert_eq!(TextView::checked(&wire).map(TextView::crc), Ok(0x4B37));

    let mut zeroed = wire;
    zeroed[9..].fill(0);
    TextView::from_wire_mut(&mut zeroed).seal();
    assert_eq!(zeroed, wire);
}

/// A prefixed word whose `decode` checks the prefix.
#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bits = 16, prefix = 2, prefix_value = 1, view)]
pub struct Tagged {
    #[bits(14)]
    pub value: u16,
}

#[test]
fn a_view_verifies_a_prefix() {
    // Prefix 0b01 in the low bits, value 1 above it.
    let wire = [0b0000_0101, 0x00];
    let view = TaggedView::checked(&wire).expect("the prefix matches");
    assert_eq!(view.decode(), Tagged { value: 1 });

    let other = [0b0000_0110, 0x00];
    assert_eq!(
        TaggedView::from_wire(&other).verify(),
        Err(ParseError::Prefix { expected: 1, got: 2 })
    );
    assert_eq!(TaggedView::from_wire(&other).value(), 1);

    let mut blank = [0u8; 2];
    let view = TaggedView::from_wire_mut(&mut blank);
    view.set_value(1);
    view.seal();
    assert_eq!(blank, wire, "seal writes the prefix and keeps the value");
}
