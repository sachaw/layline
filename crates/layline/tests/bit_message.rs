//! `#[message(bits)]`: a bit-addressed message, its inline presence bits, and its pad.
//! Every wire byte here is written out from the declared widths.

#![cfg(feature = "derive")]

use layline::{FieldCodec, Message, ParseError, U};
use layline_core::num::{ExpGolomb, unzigzag, zigzag};
use layline_core::{BitCodec, BitWriter, Buffer, Overflow};

/// H.264 `se(v)`: `ue(v)` through zigzag, with the sign flipped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Se(pub i64);

impl BitCodec<true> for Se {
    fn decode(bytes: &[u8], at_bit: usize) -> Result<(Self, usize), ParseError> {
        let (code, used) = ExpGolomb::decode(bytes, at_bit)?;
        Ok((Self(-unzigzag(code.0)), used))
    }

    fn encode<B: Buffer>(&self, out: &mut BitWriter<'_, B, true>) -> Result<(), Overflow> {
        ExpGolomb(zigzag(-self.0)).encode(out)
    }
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(6)]
pub struct Tag(pub u8);

#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
pub struct Header {
    #[bits(4)]
    pub version: u8,
    pub urgent: bool,
    #[present]
    #[bits(12)]
    pub origin: Option<u16>,
    #[present]
    #[bits(6)]
    pub tag: Option<Tag>,
    #[bits(3)]
    pub priority: u8,
}

/// The same fields, filling each byte from the low bit.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits)]
pub struct LowFirst {
    #[bits(4)]
    pub version: u8,
    pub urgent: bool,
    #[present]
    #[bits(12)]
    pub origin: Option<u16>,
    #[present]
    #[bits(6)]
    pub tag: Option<Tag>,
    #[bits(3)]
    pub priority: u8,
}

fn header(origin: Option<u16>, tag: Option<Tag>) -> Header {
    Header { version: 9, urgent: true, origin, tag, priority: 5 }
}

#[test]
fn each_combination_of_the_two_optionals_is_the_bytes_the_widths_give() {
    // 4 + 1 + (1 + 12) + (1 + 6) + 3 = 28 bits, padded to 4 bytes.
    let cases: Vec<(Header, Vec<u8>)> = vec![
        (
            header(Some(0xABC), Some(Tag(0b10_1010))),
            vec![0b1001_1110, 0b1010_1111, 0b0011_0101, 0b0101_0000],
        ),
        // 4 + 1 + 1 + 1 + 3 = 10 bits, padded to 2 bytes.
        (header(None, None), vec![0b1001_1001, 0b0100_0000]),
        // 4 + 1 + (1 + 12) + 1 + 3 = 22 bits, padded to 3 bytes.
        (header(Some(0xABC), None), vec![0b1001_1110, 0b1010_1111, 0b0001_0100]),
        // 4 + 1 + 1 + (1 + 6) + 3 = 16 bits, and the pad is empty.
        (header(None, Some(Tag(0b10_1010))), vec![0b1001_1011, 0b0101_0101]),
    ];

    for (value, wire) in cases {
        assert_eq!(value.encode(), wire, "{value:?} encodes");
        let (back, used) = Header::decode(&wire).expect("parses");
        assert_eq!(back, value);
        assert_eq!(used, wire.len(), "the message occupies whole bytes");
    }
}

#[test]
fn the_presence_bit_comes_from_the_option_and_from_nothing_else() {
    let absent = Header { version: 0, urgent: false, origin: None, tag: None, priority: 0 };
    let present = Header { origin: Some(0), ..absent.clone() };

    // Bit 5 of the message is the presence bit for `origin`: bit 2 from the top of byte 0.
    assert_eq!(absent.encode()[0] & 0b0000_0100, 0);
    assert_eq!(present.encode()[0] & 0b0000_0100, 0b0000_0100);

    let loud = Header { version: 0xF, urgent: true, priority: 7, ..absent.clone() };
    assert_eq!(loud.encode()[0] & 0b0000_0100, 0, "the other fields do not set it");
}

#[test]
fn the_low_end_first_order_writes_the_same_fields_into_other_bits() {
    let value = LowFirst {
        version: 9,
        urgent: true,
        origin: Some(0xABC),
        tag: Some(Tag(0b10_1010)),
        priority: 5,
    };
    let wire = vec![0b0011_1001, 0b1010_1111, 0b0101_0110, 0b0000_1011];
    assert_eq!(value.encode(), wire);
    assert_eq!(LowFirst::decode(&wire).expect("parses").0, value);
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
pub struct Thirteen {
    #[bits(5)]
    pub head: u8,
    #[bits(8)]
    pub tail: u8,
}

#[test]
fn a_thirteen_bit_message_occupies_two_bytes_and_pads_the_last_three_with_zeros() {
    let bytes = Thirteen { head: 0b1_1111, tail: 0xFF }.encode();
    assert_eq!(bytes.len(), 2);
    assert_eq!(bytes, [0b1111_1111, 0b1111_1000]);
    assert_eq!(bytes[1] & 0b111, 0, "the pad is zero");

    let (back, used) = Thirteen::decode(&bytes).expect("parses");
    assert_eq!(back, Thirteen { head: 0b1_1111, tail: 0xFF });
    assert_eq!(used, 2, "the pad belongs to the message");

    // A decode ignores the pad bits.
    let noisy = [0b1111_1111, 0b1111_1111];
    assert_eq!(Thirteen::decode(&noisy).expect("parses").0.tail, 0xFF);
}

#[test]
fn a_body_that_ends_inside_a_field_is_short_at_the_byte_the_field_begins_in() {
    // `tag` begins at bit 19, in byte 2, and reading it needs a fourth byte.
    let truncated = [0b1001_1110, 0b1010_1111, 0b0011_0101];
    assert_eq!(
        Header::decode(&truncated).unwrap_err(),
        ParseError::Short { need_bytes: 4, got_bytes: 3, at: 2 }
    );

    // `origin` begins at bit 6, in byte 0.
    let shorter = [0b1001_1110, 0b1010_1111];
    assert_eq!(
        Header::decode(&shorter).unwrap_err(),
        ParseError::Short { need_bytes: 3, got_bytes: 2, at: 0 }
    );
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
pub struct Packet {
    pub magic: u16,
    #[message]
    pub header: Header,
    #[fill]
    pub body: Vec<u8>,
}

#[test]
fn a_parent_steps_over_the_whole_bytes_the_header_occupied() {
    let packet = Packet {
        magic: 0x1234,
        header: header(Some(0xABC), None),
        body: vec![0xDE, 0xAD, 0xBE, 0xEF],
    };
    let wire = vec![
        0x12,
        0x34, // magic, big-endian
        0b1001_1110,
        0b1010_1111,
        0b0001_0100, // the header's 22 bits, padded to 3 bytes
        0xDE,
        0xAD,
        0xBE,
        0xEF, // the body fills what is left
    ];
    assert_eq!(packet.encode(), wire);

    let (back, used) = Packet::decode(&wire).expect("parses");
    assert_eq!(back, packet);
    assert_eq!(used, wire.len());
}

#[test]
fn a_shorter_header_moves_the_body_up() {
    let packet =
        Packet { magic: 0x1234, header: header(None, None), body: vec![0xDE, 0xAD, 0xBE, 0xEF] };
    let wire = vec![0x12, 0x34, 0b1001_1001, 0b0100_0000, 0xDE, 0xAD, 0xBE, 0xEF];
    assert_eq!(packet.encode(), wire, "the header is 2 bytes with both optionals absent");
    assert_eq!(Packet::decode(&wire).expect("parses").0, packet);
}

#[derive(Debug, Clone, PartialEq, Message)]
pub enum Body {
    #[value(0)]
    Head(Header),
    #[value(1)]
    Level(u8),
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Framed {
    pub kind: u8,
    #[switch(kind)]
    pub body: Body,
}

#[test]
fn a_bit_addressed_message_is_an_arm_like_any_other() {
    let framed = Framed { kind: 0, body: Body::Head(header(None, Some(Tag(0b10_1010)))) };
    let wire = vec![0, 0b1001_1011, 0b0101_0101];
    assert_eq!(framed.encode(), wire);

    let (back, used) = Framed::decode(&wire).expect("parses");
    assert_eq!(back, framed);
    assert_eq!(used, wire.len());
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
pub struct Carried {
    pub flag: bool,
    pub small: U<3>,
    #[bits(9)]
    pub wide: i16,
    #[bits(3)]
    pub spare: u8,
}

#[test]
fn a_type_that_carries_its_width_needs_no_attribute_and_a_signed_field_extends_its_sign() {
    let value = Carried { flag: true, small: U::new(5).expect("3 bits"), wide: -3, spare: 0b101 };
    // 1 | 101 | 111111101 | 101, padded to 16 bits.
    let wire = vec![0b1101_1111, 0b1110_1101];
    assert_eq!(value.encode(), wire);
    assert_eq!(Carried::decode(&wire).expect("parses").0, value);

    let positive = Carried { wide: 255, ..value };
    assert_eq!(Carried::decode(&positive.encode()).expect("parses").0.wide, 255);
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
pub struct Scaled {
    #[bits(4)]
    pub channel: u8,
    pub scale: f32,
}

#[test]
fn a_float_field_keeps_the_bits_ieee_754_gives_it() {
    let value = Scaled { channel: 9, scale: -1.5 };
    let wire = value.encode();
    assert_eq!(wire.len(), 5, "4 + 32 bits, padded to 5 bytes");
    assert_eq!(Scaled::decode(&wire).expect("parses").0, value);

    // The 32 bits after the nibble are the float's own.
    let bits = u32::from(wire[0] & 0x0F) << 28
        | u32::from(wire[1]) << 20
        | u32::from(wire[2]) << 12
        | u32::from(wire[3]) << 4
        | u32::from(wire[4]) >> 4;
    assert_eq!(bits, (-1.5f32).to_bits());
}

#[test]
fn the_table_places_each_field_by_bit_and_each_optional_after_the_one_before_it() {
    use layline::table::{Presence, Span, Start};

    let rows = <Header as Message>::SEGMENTS;
    let names: Vec<_> = rows.iter().map(|r| r.name).collect();
    assert_eq!(names, ["version", "urgent", "origin", "tag", "priority"]);

    assert_eq!(rows[0].start, Start::At(0));
    assert_eq!(rows[0].span, Span::Fixed(4));
    assert_eq!(rows[1].start, Start::At(4));
    assert_eq!(rows[1].span, Span::Fixed(1));

    // The presence bit is bit 5, so the field it gates starts at bit 6.
    assert_eq!(rows[2].start, Start::At(6));
    assert_eq!((rows[2].span, rows[2].when), (Span::Fixed(12), Some(Presence::Bit)));
    assert!(!rows[2].is_fixed(), "an optional field ends where the wire says");

    assert_eq!(rows[3].start, Start::After { segment: "origin", bits: 1 });
    assert_eq!((rows[3].span, rows[3].when), (Span::Fixed(6), Some(Presence::Bit)));
    assert_eq!(rows[4].start, Start::After { segment: "tag", bits: 0 });
    assert_eq!(rows[4].span, Span::Fixed(3));

    assert_eq!(layline::table::fixed_bits(rows), 6, "fixed up to the first presence bit");
}

#[test]
fn the_audit_dump_renders_the_presence_rows() {
    assert_eq!(
        format!("{}", <Header as layline::Message>::audit()),
        "\
M Header
G Header version at,0 fixed,4 - -
G Header urgent at,4 fixed,1 - -
G Header origin at,6 fixed,12 bit -
G Header tag after,origin,1 fixed,6 bit -
G Header priority after,tag,0 fixed,3 - -
E Header 6
"
    );
}

// ---------------------------------------------------------------------------
// A value whose width is read from the wire
// ---------------------------------------------------------------------------

/// A run length in Exp-Golomb, between two fields of declared width.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
pub struct Run {
    #[bits(3)]
    pub channel: u8,
    #[var]
    pub length: ExpGolomb,
    #[bits(4)]
    pub tail: u8,
}

#[test]
fn a_var_field_takes_the_bits_its_own_coding_gives_it() {
    // 101 | 00101 | 1001, padded to 16 bits.
    let value = Run { channel: 5, length: ExpGolomb(4), tail: 9 };
    assert_eq!(value.encode(), [0b1010_0101, 0b1001_0000]);
    assert_eq!(Run::decode(&value.encode()).expect("parses"), (value, 2));

    // 101 | 1 | 1001 is 8 bits, and the pad is empty.
    let short = Run { channel: 5, length: ExpGolomb(0), tail: 9 };
    assert_eq!(short.encode(), [0b1011_1001]);
    assert_eq!(Run::decode(&short.encode()).expect("parses"), (short, 1));
}

#[test]
fn a_wider_value_moves_the_field_behind_it() {
    for n in [0u64, 1, 2, 7, 8, 300, 65_000, u32::MAX as u64] {
        let value = Run { channel: 5, length: ExpGolomb(n), tail: 9 };
        let wire = value.encode();
        let (back, used) = Run::decode(&wire).expect("parses");
        assert_eq!(back, value, "code number {n}");
        assert_eq!(used, wire.len());
        assert_eq!(
            wire.len(),
            (3 + 2 * (64 - (n + 1).leading_zeros() as usize) - 1 + 4).div_ceil(8)
        );
    }
}

#[test]
fn a_body_that_ends_inside_a_var_field_is_short() {
    // Three zero bits after the channel, and nothing to end the code word.
    assert!(matches!(Run::decode(&[0b1010_0000]), Err(ParseError::Short { .. })));
}

#[test]
fn the_table_places_the_fields_behind_a_discovered_extent_after_it() {
    use layline::table::{Span, Start};

    let rows = <Run as Message>::SEGMENTS;
    assert_eq!(rows[0].start, Start::At(0));
    assert_eq!(rows[0].span, Span::Fixed(3));

    assert_eq!(rows[1].start, Start::At(3));
    assert_eq!(rows[1].span, Span::SelfDelimiting);
    assert_eq!(rows[1].decoded_by, Some("ExpGolomb"));
    assert!(!rows[1].is_fixed(), "the coding states its own width");

    assert_eq!(rows[2].start, Start::After { segment: "length", bits: 0 });
    assert_eq!(rows[2].span, Span::Fixed(4));
    assert_eq!(layline::table::fixed_bits(rows), 3, "fixed up to the first bit of the coding");
}

// ---------------------------------------------------------------------------
// A field whose presence depends on an earlier field
// ---------------------------------------------------------------------------

/// Two optional fields, one behind a mask of an earlier field and one behind an earlier `bool`.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
pub struct Gated {
    #[bits(4)]
    pub flags: u8,
    pub urgent: bool,
    #[when(flags & 0x1)]
    #[bits(12)]
    pub origin: Option<u16>,
    #[when(urgent)]
    #[bits(6)]
    pub tag: Option<Tag>,
    #[bits(3)]
    pub priority: u8,
}

#[test]
fn a_gated_field_is_on_the_wire_when_the_earlier_field_says_so() {
    // 1011 | 1 | 101010111100 | 101010 | 101: 26 bits, padded to 4 bytes.
    let both = Gated {
        flags: 0b1011,
        urgent: true,
        origin: Some(0xABC),
        tag: Some(Tag(0b10_1010)),
        priority: 5,
    };
    assert_eq!(both.encode(), [0b1011_1101, 0b0101_1110, 0b0101_0101, 0b0100_0000]);
    assert_eq!(Gated::decode(&both.encode()).expect("parses"), (both.clone(), 4));

    // 1010 | 0 | 101, padded to 8 bits.
    let neither = Gated { flags: 0b1010, urgent: false, origin: None, tag: None, ..both };
    assert_eq!(neither.encode(), [0b1010_0101]);
    assert_eq!(Gated::decode(&neither.encode()).expect("parses"), (neither, 1));
}

#[test]
fn the_flag_bits_come_from_is_some_and_the_other_bits_stand() {
    let full = Gated { flags: 0b1111, urgent: true, origin: None, tag: None, priority: 0 };
    let (back, _) = Gated::decode(&full.encode()).expect("parses");
    assert_eq!(back.flags, 0b1110, "the governed bit is cleared for a `None`");
    assert!(!back.urgent, "a whole-field flag is the presence of its field");
    assert_eq!((back.origin, back.tag), (None, None));

    let empty = Gated { flags: 0b0000, urgent: false, origin: Some(0), tag: None, priority: 0 };
    let (back, _) = Gated::decode(&empty.encode()).expect("parses");
    assert_eq!(back.flags, 0b0001, "the governed bit is set for a `Some`");
    assert_eq!(back.origin, Some(0));
}

#[test]
fn the_table_places_a_gated_row_behind_the_flag_that_governs_it() {
    use layline::table::{Presence, Span, Start};

    let rows = <Gated as Message>::SEGMENTS;
    assert_eq!(rows[2].start, Start::At(5));
    assert_eq!(
        (rows[2].span, rows[2].when),
        (Span::Fixed(12), Some(Presence::Flag { field: "flags", mask: 0x1 }))
    );
    assert_eq!(rows[3].start, Start::After { segment: "origin", bits: 0 });
    assert_eq!(
        (rows[3].span, rows[3].when),
        (Span::Fixed(6), Some(Presence::Flag { field: "urgent", mask: 1 }))
    );
    assert_eq!(rows[4].start, Start::After { segment: "tag", bits: 0 });
}

/// One message with an inline presence bit, a mask of an earlier field, and a coding of its own width.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(bits, order = msb)]
pub struct Mixed {
    #[bits(4)]
    pub flags: u8,
    #[when(flags & 0x2)]
    #[bits(5)]
    pub level: Option<u8>,
    #[present]
    #[var]
    pub delta: Option<Se>,
    #[present]
    #[bits(3)]
    pub spare: Option<u8>,
    #[bits(2)]
    pub tail: u8,
}

#[test]
fn a_presence_bit_and_a_flag_bit_read_the_same_message() {
    let cases = [
        Mixed { flags: 0b0010, level: Some(9), delta: Some(Se(-3)), spare: Some(5), tail: 2 },
        Mixed { flags: 0b0010, level: Some(0), delta: None, spare: None, tail: 0 },
        Mixed { flags: 0b1101, level: None, delta: Some(Se(0)), spare: None, tail: 3 },
        Mixed { flags: 0b0000, level: None, delta: None, spare: Some(7), tail: 1 },
    ];
    for value in &cases {
        let wire = value.encode();
        let (back, used) = Mixed::decode(&wire).expect("parses");
        assert_eq!(&back, value);
        assert_eq!(used, wire.len());
    }

    // 0010 | 01001 | 1 | 00111 | 1 | 101 | 10, where the coding for -3 is five bits.
    // 21 bits, padded to 3 bytes.
    assert_eq!(cases[0].encode(), [0b0010_0100, 0b1100_1111, 0b1011_0000]);
}

#[test]
fn the_audit_dump_renders_a_discovered_extent_and_a_flagged_row() {
    assert_eq!(
        format!("{}", <Mixed as layline::Message>::audit()),
        "\
M Mixed
G Mixed flags at,0 fixed,4 - -
G Mixed level at,4 fixed,5 flag,flags,0x2 -
G Mixed delta after,level,1 selfdelimiting bit Se
G Mixed spare after,delta,1 fixed,3 bit -
G Mixed tail after,spare,0 fixed,2 - -
E Mixed 4
"
    );
}
