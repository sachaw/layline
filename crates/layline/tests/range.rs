//! `#[range(lo..=hi)]`: decode rejects a value outside the range.

#![cfg(feature = "derive")]

use layline::{Layout, Message, ParseError};

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 4, endian = be)]
struct Reading {
    #[range(..=255)]
    count: u16,
    value: u16,
}

#[test]
fn a_value_inside_the_range_decodes() {
    let got = Reading::decode(&[0x00, 0xFF, 0xBE, 0xEF]).expect("255 is admitted");
    assert_eq!((got.count, got.value), (255, 0xBEEF));
    assert_eq!(got.encode(), [0x00, 0xFF, 0xBE, 0xEF]);
}

#[test]
fn a_value_outside_it_is_refused_and_the_error_names_both_ends() {
    assert_eq!(
        Reading::decode(&[0x01, 0x2C, 0xBE, 0xEF]),
        Err(ParseError::OutOfRange { field: "count", value: 300, lo: 0, hi: 255 }),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 2, endian = be)]
struct Offset {
    #[range(-100..=100)]
    tenths: i16,
}

#[test]
fn a_signed_range_is_checked_at_both_ends() {
    assert_eq!(Offset::decode(&[0xFF, 0x9C]).expect("-100 is admitted").tenths, -100);
    assert_eq!(
        Offset::decode(&[0xFF, 0x9B]),
        Err(ParseError::OutOfRange { field: "tenths", value: -101, lo: -100, hi: 100 }),
    );
}

#[test]
#[should_panic(expected = "outside `#[range(0..=255)]`")]
fn encode_refuses_what_decode_would() {
    let _ = Reading { count: 300, value: 0 }.encode();
}

/// The annotated `Result` fails to compile if `decode` becomes infallible.
#[test]
fn decode_is_fallible_because_the_layout_asserts() {
    let out: Result<Reading, ParseError> = Reading::decode(&[0, 1, 0, 0]);
    assert!(out.is_ok());
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 2, endian = be)]
struct Unbounded {
    anything: u16,
}

#[test]
fn a_layout_without_one_keeps_its_infallible_decode() {
    let got: Unbounded = Unbounded::decode(&[0xFF, 0xFF]);
    assert_eq!(got.anything, 0xFFFF);
}

#[test]
fn the_artifact_publishes_the_bound() {
    let text = format!("{}", <Reading as layline::Layout>::audit());
    assert!(text.contains("R Reading count 0 255\n"), "no R record in:\n{text}");
    assert!(text.contains("F Reading 0 16 count\n"), "the F row contains the width");
}

#[test]
fn a_layout_without_one_publishes_nothing() {
    let text = format!("{}", <Unbounded as layline::Layout>::audit());
    assert!(!text.contains("R "), "a layout without a range has no R row:\n{text}");
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
#[message(endian = be)]
struct Widened {
    #[range(..=255)]
    count: u16,
    flags: u16,
    #[when(flags & 0x01)]
    #[range(..=255)]
    extra: Option<u16>,
    tail: u16,
}

#[test]
fn a_bound_on_a_mandatory_message_field_names_the_field() {
    let wire = [0x01, 0x2C, 0x00, 0x00, 0xBE, 0xEF];
    assert_eq!(
        Widened::decode(&wire),
        Err(ParseError::OutOfRange { field: "count", value: 300, lo: 0, hi: 255 }),
    );
}

#[test]
fn a_bound_on_an_optional_message_field_names_the_field() {
    let wire = [0x00, 0x01, 0x00, 0x01, 0x01, 0x2C, 0xBE, 0xEF];
    assert_eq!(
        Widened::decode(&wire),
        Err(ParseError::OutOfRange { field: "extra", value: 300, lo: 0, hi: 255 }),
    );
}

#[test]
fn an_absent_optional_field_is_not_bounded() {
    let wire = [0x00, 0x01, 0x00, 0x00, 0xBE, 0xEF];
    let (m, used) = Widened::decode(&wire).expect("the field is absent");
    assert_eq!((m.count, m.extra, m.tail, used), (1, None, 0xBEEF, 6));
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bits = 16)]
struct Slotted {
    #[bits(4)]
    kind: u8,
    #[bits(1)]
    #[range(0..=0)]
    spare: u8,
    #[bits(11)]
    payload: u16,
}

#[test]
fn a_word_mode_range_is_enforced() {
    use layline::Layout;
    let ok = Slotted::decode_slice(&[0b0000_0101, 0x7F]).expect("spare clear");
    assert_eq!((ok.kind, ok.payload), (5, 1016));
    let err = Slotted::decode_slice(&[0b0001_0101, 0x7F]);
    assert_eq!(err, Err(ParseError::OutOfRange { field: "spare", value: 1, lo: 0, hi: 0 }),);
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(words = 2)]
struct Sequenced {
    #[bits(4)]
    kind: u8,
    #[bits(1)]
    #[range(0..=0)]
    spare: u8,
    #[bits(11)]
    payload: u16,
    #[bits(16)]
    trailer: u16,
}

#[test]
fn a_words_mode_range_is_enforced() {
    use layline::Layout;
    let ok = Sequenced::decode_slice(&[0b0000_0101, 0x7F, 0, 0]).expect("spare clear");
    assert_eq!(ok.kind, 5);
    let err = Sequenced::decode_slice(&[0b0001_0101, 0x7F, 0, 0]);
    assert_eq!(err, Err(ParseError::OutOfRange { field: "spare", value: 1, lo: 0, hi: 0 }),);
}
