//! Word-sized `bits = N` layouts (N <= 64): one native register, `from_bits`/`to_bits`, `truncate`,
//! and `FieldCodec` bit groups. Every expected word is assembled by hand from the stated bit positions.

#![cfg(feature = "derive")]

use layline::{FieldCodec, Layout, ParseError};

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(1)]
pub enum MessageKind {
    #[value(0)]
    Short,
    #[value(1)]
    Long,
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(5)]
pub struct ChannelId(pub u8);

/// `[words: 13][buffer_index: 8]`, LSB first: a 21-bit group, narrower than any layout.
#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(21)]
pub struct LongDescriptor {
    #[bits(13)]
    pub words: u16,
    #[bits(8)]
    pub buffer_index: u8,
}

/// | 31..27 channel | 26 kind | 25 wants_reply | 24 is_reply | 23..21 seq | 20..0 payload |
#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 32, endian = le, order = lsb, view)]
pub struct Header {
    #[bits(21)]
    #[overlay(long_descriptor: LongDescriptor)]
    pub payload: u32,
    #[bits(3)]
    pub seq: u8,
    #[bits(1)]
    pub is_reply: bool,
    #[bits(1)]
    pub wants_reply: bool,
    #[bits(1)]
    pub kind: MessageKind,
    #[bits(5)]
    pub channel: ChannelId,
}

fn short() -> Header {
    Header {
        payload: 0x03_3456,
        seq: 6,
        is_reply: false,
        wants_reply: true,
        kind: MessageKind::Short,
        channel: ChannelId(5),
    }
}

#[test]
fn a_word_is_the_fields_shifted_to_their_stated_bits() {
    // 5 << 27 | 1 << 25 | 6 << 21 | 0x03_3456
    let word = (5 << 27) | (1 << 25) | (6 << 21) | 0x03_3456;
    assert_eq!(word, 0x2AC3_3456);
    assert_eq!(short().to_bits(), word);
    assert_eq!(Header::from_bits(word), short());
    assert_eq!(short().encode(), [0x56, 0x34, 0xC3, 0x2A]);
    assert_eq!(Header::decode(&[0x56, 0x34, 0xC3, 0x2A]), short());
}

#[test]
fn a_bit_group_is_an_overlay() {
    let mut long = Header {
        payload: 0,
        seq: 5,
        is_reply: true,
        wants_reply: false,
        kind: MessageKind::Long,
        channel: ChannelId(31),
    };
    long.set_long_descriptor(LongDescriptor { words: 1024, buffer_index: 42 });
    // 42 << 13 | 1024
    assert_eq!(long.payload, (42 << 13) | 1024);
    // 31 << 27 | 1 << 26 | 1 << 24 | 5 << 21 | 42 << 13 | 1024
    assert_eq!(long.to_bits(), 0xFDA5_4400);
    assert_eq!(
        Header::from_bits(0xFDA5_4400).long_descriptor(),
        LongDescriptor { words: 1024, buffer_index: 42 }
    );
}

#[test]
fn a_view_reads_and_writes_one_field_of_the_word() {
    let mut view = HeaderView::new(0x2AC3_3456u32.to_le_bytes());
    assert_eq!(view.channel(), ChannelId(5));
    assert!(view.wants_reply());
    assert!(!view.is_reply());
    assert_eq!(view.seq(), 6);
    view.set_seq(3);
    assert_eq!(u32::from_le_bytes(*view.as_wire()), (0x2AC3_3456 & !(7 << 21)) | (3 << 21));
    view.set_is_reply(true);
    assert_eq!(u32::from_le_bytes(*view.as_wire()), 0x2B63_3456);
    view.set_payload(0);
    assert_eq!(u32::from_le_bytes(*view.as_wire()), 0x2B60_0000);
}

#[test]
#[should_panic(expected = "field `seq`")]
fn without_truncate_a_value_past_its_width_panics() {
    let _ = Header { seq: 8, ..short() }.to_bits();
}

#[test]
#[should_panic(expected = "field `channel`")]
fn without_truncate_a_codec_value_past_its_width_panics() {
    let _ = Header { channel: ChannelId(32), ..short() }.encode();
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 16, truncate)]
pub struct Register {
    #[bits(3)]
    pub mode: u8,
    #[bits(4)]
    pub level: i8,
    #[bits(5)]
    pub channel: ChannelId,
    #[bits(4)]
    pub rest: u16,
}

#[test]
fn truncate_keeps_the_low_bits_like_a_hardware_register() {
    let r = Register { mode: 0xFF, level: -1, channel: ChannelId(0xFF), rest: 0x1234 };
    // mode 0b111, level -1 as 4 bits 0b1111 << 3, channel 31 << 7, rest 0x4 << 12
    let word = 0b111 | (0b1111 << 3) | (31 << 7) | (0x4 << 12);
    assert_eq!(word, 0x4FFF);
    assert_eq!(r.to_bits(), word);
    assert_eq!(r.encode(), [0xFF, 0x4F]);
    // Only the bits each field holds come back.
    assert_eq!(
        Register::from_bits(word),
        Register { mode: 7, level: -1, channel: ChannelId(31), rest: 4 }
    );
}

#[test]
fn truncate_keeps_a_value_that_fits_as_it_is() {
    let r = Register { mode: 5, level: -8, channel: ChannelId(17), rest: 9 };
    // 5 | (-8 as 4 bits = 0b1000) << 3 | 17 << 7 | 9 << 12
    assert_eq!(r.to_bits(), 5 | (0b1000 << 3) | (17 << 7) | (9 << 12));
    assert_eq!(Register::from_bits(r.to_bits()), r);
}

/// 24 bits: a register wider than the wire. No `from_bits`, since a `u32` has bits the wire lacks.
#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 24, endian = be, order = msb)]
pub struct Be24 {
    #[bits(4)]
    pub a: u8,
    #[bits(12)]
    pub b: u16,
    #[bits(8)]
    pub c: u8,
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 24)]
pub struct Le24 {
    #[bits(4)]
    pub a: u8,
    #[bits(12)]
    pub b: u16,
    #[bits(8)]
    pub c: u8,
}

#[test]
fn a_three_byte_layout_keeps_its_byte_and_bit_order() {
    // msb: a takes bits 23..20, b 19..8, c 7..0. Container 0xA1_2345; big-endian on the wire.
    let be = Be24 { a: 0xA, b: 0x123, c: 0x45 };
    assert_eq!(be.encode(), [0xA1, 0x23, 0x45]);
    assert_eq!(Be24::decode(&[0xA1, 0x23, 0x45]), be);

    // lsb: a takes bits 3..0, b 15..4, c 23..16. Container 0x45_123A; little-endian on the wire.
    let le = Le24 { a: 0xA, b: 0x123, c: 0x45 };
    assert_eq!(le.encode(), [0x3A, 0x12, 0x45]);
    assert_eq!(Le24::decode(&[0x3A, 0x12, 0x45]), le);
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 40)]
pub struct Le40 {
    #[bits(3)]
    pub k: u8,
    #[bits(37)]
    pub v: u64,
}

#[test]
fn a_five_byte_layout_works_in_a_u64() {
    // 5 | 0x1F_2345_6789 << 3 = 0xF9_1A2B_3C4D
    let x = Le40 { k: 5, v: 0x1F_2345_6789 };
    assert_eq!(x.encode(), [0x4D, 0x3C, 0x2B, 0x1A, 0xF9]);
    assert_eq!(Le40::decode(&[0x4D, 0x3C, 0x2B, 0x1A, 0xF9]), x);
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 64, endian = be, order = msb)]
pub struct Be64 {
    #[bits(4)]
    pub a: i8,
    #[bits(28)]
    pub b: u32,
    #[bits(1)]
    pub c: bool,
    #[bits(31)]
    pub d: u64,
}

#[test]
fn a_full_u64_register_signs_and_orders_its_fields() {
    // -3 as 4 bits = 0xD at 63..60, b at 59..32, c at 31, d at 30..0.
    let x = Be64 { a: -3, b: 0xABC_DEF1, c: true, d: 0x765_4321 };
    let word = (0xDu64 << 60) | (0xABC_DEF1 << 32) | (1 << 31) | 0x765_4321;
    assert_eq!(word, 0xDABC_DEF1_8765_4321);
    assert_eq!(x.to_bits(), word);
    assert_eq!(x.encode(), [0xDA, 0xBC, 0xDE, 0xF1, 0x87, 0x65, 0x43, 0x21]);
    assert_eq!(Be64::from_bits(word), x);
    assert_eq!(Be64::from_bits(u64::from_be_bytes(x.encode())), x);
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 8, order = msb)]
pub struct Flags {
    #[bits(1)]
    pub on: bool,
    #[bits(7)]
    pub n: u8,
}

#[test]
fn a_one_byte_layout_is_a_u8() {
    assert_eq!(Flags::from_bits(0x85), Flags { on: true, n: 5 });
    assert_eq!(Flags { on: false, n: 0x7F }.to_bits(), 0x7F);
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 16, prefix = 3, prefix_value = 0b101)]
pub struct Tagged {
    #[bits(13)]
    pub body: u16,
}

#[test]
fn a_word_with_a_prefix_value_checks_it_in_from_bits() {
    // body 0x123 << 3 | 0b101
    assert_eq!(Tagged { body: 0x123 }.to_bits(), 0x091D);
    assert_eq!(Tagged::from_bits(0x091D), Ok(Tagged { body: 0x123 }));
    assert_eq!(Tagged::from_bits(0x091C), Err(ParseError::Prefix { expected: 0b101, got: 0b100 }));
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 8)]
pub struct Ranged {
    #[bits(4)]
    #[range(1..=9)]
    pub digit: u8,
    #[bits(4)]
    pub rest: u8,
}

#[test]
#[should_panic(expected = "field `digit`")]
fn encode_refuses_a_bit_field_outside_its_range() {
    let _ = Ranged { digit: 12, rest: 0 }.encode();
}

#[test]
fn a_ranged_word_reads_through_from_bits() {
    assert_eq!(Ranged::from_bits(0x35), Ok(Ranged { digit: 5, rest: 3 }));
    assert!(matches!(Ranged::from_bits(0x3C), Err(ParseError::OutOfRange { value: 12, .. })));
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(12, order = msb)]
pub struct Msb12 {
    #[bits(1)]
    pub flag: bool,
    #[bits(4)]
    pub level: i8,
    #[bits(2)]
    pub kind: Two,
    #[bits(5)]
    pub n: u8,
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(2)]
pub enum Two {
    #[value(0)]
    A,
    #[value(1)]
    B,
    #[value(2)]
    C,
    #[value(3)]
    D,
}

#[test]
fn a_bit_group_can_take_its_fields_from_the_top() {
    let g = Msb12 { flag: true, level: -2, kind: Two::C, n: 19 };
    // flag at bit 11, level (-2 = 0b1110) at 10..7, kind 0b10 at 6..5, n at 4..0
    let raw = (1 << 11) | (0b1110 << 7) | (0b10 << 5) | 19;
    assert_eq!(g.to_raw(), raw);
    assert_eq!(Msb12::from_raw(raw), g);
}

#[test]
fn a_bit_group_from_raw_reads_each_field_from_its_bits() {
    // words 0x1ABC, buffer_index 0x5D: 0x5D << 13 | 0x1ABC
    let raw = (0x5D << 13) | 0x1ABC;
    assert_eq!(LongDescriptor::from_raw(raw), LongDescriptor { words: 0x1ABC, buffer_index: 0x5D });
    assert_eq!(LongDescriptor { words: 0x1ABC, buffer_index: 0x5D }.to_raw(), raw);
}

#[test]
#[should_panic(expected = "field `words`")]
fn a_bit_group_refuses_a_value_past_its_width() {
    let _ = LongDescriptor { words: 0x2000, buffer_index: 0 }.to_raw();
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(21, truncate)]
pub struct Truncated {
    #[bits(13)]
    pub words: u16,
    #[bits(8)]
    pub buffer_index: u8,
}

#[test]
fn a_truncating_bit_group_keeps_the_low_bits() {
    // words 0xFFFF keeps 0x1FFF; the index is untouched.
    let t = Truncated { words: 0xFFFF, buffer_index: 0x12 };
    assert_eq!(t.to_raw(), (0x12 << 13) | 0x1FFF);
}

/// A bit group as a plain field, between two others.
#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 32)]
pub struct Carrier {
    #[bits(3)]
    pub lead: u8,
    #[bits(21)]
    pub desc: LongDescriptor,
    #[bits(8)]
    pub tail: u8,
}

#[test]
fn a_bit_group_is_a_field() {
    let c = Carrier { lead: 6, desc: LongDescriptor { words: 7, buffer_index: 0xA5 }, tail: 0x3C };
    // 6 | (0xA5 << 13 | 7) << 3 | 0x3C << 24
    let word = 6 | (((0xA5 << 13) | 7) << 3) | (0x3C << 24);
    assert_eq!(word, 0x3CA5_003E);
    assert_eq!(c.to_bits(), word);
    assert_eq!(Carrier::from_bits(word), c);
}
