//! Encode panics, naming the field, rather than write bytes decode would refuse or read back differently.

#![cfg(feature = "derive")]

use layline::{FieldCodec, Fixed, Layout, Message, Overflow};

#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits)]
struct Nibbles {
    #[bits(4)]
    v: u8,
    #[bits(4)]
    w: u8,
}

#[test]
#[should_panic(expected = "field `v`")]
fn a_bit_field_refuses_an_unsigned_value_past_its_width() {
    let _ = Nibbles { v: 0x1F, w: 0 }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits)]
struct Signed9 {
    #[bits(9)]
    v: i16,
    #[bits(7)]
    rest: u8,
}

#[test]
#[should_panic(expected = "field `v`")]
fn a_bit_field_refuses_a_signed_value_past_its_width() {
    let _ = Signed9 { v: 300, rest: 0 }.encode();
}

#[test]
fn a_bit_field_takes_a_negative_value_that_fits() {
    let m = Signed9 { v: -256, rest: 1 };
    assert_eq!(Signed9::decode(&m.encode()).expect("reads back").0, m);
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(6)]
struct Tag(u8);

#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits)]
struct Tagged {
    #[bits(6)]
    t: Tag,
    #[bits(2)]
    rest: u8,
}

#[test]
#[should_panic(expected = "field `t`")]
fn a_bit_field_refuses_a_codec_value_past_its_width() {
    let _ = Tagged { t: Tag(0xFF), rest: 0 }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Terminated {
    #[text]
    #[until(0)]
    name: String,
}

#[test]
#[should_panic(expected = "field `name`")]
fn terminated_text_refuses_its_terminator() {
    let _ = Terminated { name: String::from("a\0b") }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Padded {
    #[text]
    #[bytes(4)]
    name: String,
}

#[test]
#[should_panic(expected = "field `name`")]
fn fixed_text_refuses_text_longer_than_its_bytes() {
    let _ = Padded { name: String::from("hello") }.encode();
}

#[test]
#[should_panic(expected = "field `name`")]
fn fixed_text_refuses_a_trailing_nul_decode_would_strip() {
    let _ = Padded { name: String::from("ab\0") }.encode();
}

#[test]
fn fixed_text_keeps_an_interior_nul() {
    let m = Padded { name: String::from("a\0b") };
    assert_eq!(Padded::decode(&m.encode()).expect("reads back").0, m);
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Raw {
    #[until(0xFF)]
    raw: Vec<u8>,
    tail: u8,
}

#[test]
#[should_panic(expected = "`raw`")]
fn a_run_until_a_byte_refuses_an_element_that_starts_with_it() {
    let _ = Raw { raw: vec![1, 0xFF, 2], tail: 3 }.encode();
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 1)]
struct Octet {
    b: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Masked {
    #[until(mask = 0x80)]
    #[bytes(1)]
    items: Vec<Octet>,
}

#[test]
#[should_panic(expected = "`items`")]
fn a_masked_run_refuses_no_elements() {
    let _ = Masked { items: vec![] }.encode();
}

#[test]
#[should_panic(expected = "`items`")]
fn a_masked_run_refuses_an_element_that_already_ends_it() {
    let _ = Masked { items: vec![Octet { b: 0x81 }, Octet { b: 0x02 }] }.encode();
}

#[test]
fn a_masked_run_re_encodes_what_it_decoded() {
    let m = Masked { items: vec![Octet { b: 0x01 }, Octet { b: 0x02 }] };
    assert_eq!(m.encode(), [0x01, 0x82]);
    let (back, _) = Masked::decode(&[0x01, 0x82]).expect("reads back");
    assert_eq!(back.items, [Octet { b: 0x01 }, Octet { b: 0x82 }]);
    assert_eq!(back.encode(), [0x01, 0x82]);
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(needs(stride: u8))]
struct Sized0 {
    #[len(stride)]
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Unbounded {
    stride: u8,
    #[fill]
    #[with(stride)]
    #[message]
    items: Vec<Sized0>,
}

#[test]
#[should_panic(expected = "`items`")]
fn an_unbounded_run_refuses_an_element_of_no_bytes() {
    let _ = Unbounded { stride: 0, items: vec![Sized0 { data: vec![] }] }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
enum Body {
    #[value(1)]
    Level(Level),
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Level {
    v: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Keyed {
    kind: u8,
    #[switch(kind)]
    body: Body,
}

#[test]
#[should_panic(expected = "`kind`")]
fn an_open_arm_refuses_a_discriminant_the_catalogue_defines() {
    let _ = Keyed { kind: 1, body: Body::Unknown(vec![5, 6]) }.encode();
}

#[test]
fn an_open_arm_keeps_a_discriminant_the_catalogue_does_not_define() {
    let m = Keyed { kind: 9, body: Body::Unknown(vec![5, 6]) };
    assert_eq!(Keyed::decode(&m.encode()).expect("reads back").0, m);
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Lengthed {
    seq: u8,
    #[switch(..)]
    body: Body,
}

#[test]
#[should_panic(expected = "`body`")]
fn an_open_arm_refuses_a_length_the_catalogue_defines() {
    let _ = Lengthed { seq: 0, body: Body::Unknown(vec![7]) }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Counted {
    n: u8,
    #[count(n)]
    rest: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
enum Arm {
    #[value(2)]
    Counted(Counted),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Windowed {
    tag: u8,
    len: u8,
    #[switch(tag)]
    #[len(len)]
    value: Arm,
}

#[test]
fn a_measured_arm_that_overflows_leaves_nothing_where_the_header_goes() {
    let m = Windowed { tag: 0, len: 0, value: Arm::Counted(Counted { n: 0, rest: vec![1, 2, 3] }) };
    let mut room = [0u8; 2];
    let mut out = Fixed::new(&mut room);
    assert_eq!(m.encode_into(&mut out), Err(Overflow));
    assert_eq!(out.into_written(), [0u8; 0]);
}
