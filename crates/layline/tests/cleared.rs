//! `#[when(!flag)]` and `#[when(flags & M == 0)]`: a field present while its bits are clear.

#![cfg(all(feature = "derive", feature = "alloc"))]

use layline::table::Presence;
use layline::{Message, ParseError};

/// After the G3D bone record: each part is on the wire unless its "is default" bit is set.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct NodeData {
    /// TRANS_ZERO = 0x1, SCALE_ONE = 0x4; 0x10 belongs to no field.
    pub flag: u16,
    #[when(flag & 0x0001 == 0)]
    pub translation: Option<u32>,
    #[when(flag & 0x0004 == 0)]
    pub scale: Option<u16>,
}

#[test]
fn a_part_is_present_while_its_default_bit_is_clear() {
    // flag 0x0010: both default bits clear, so both parts follow.
    let wire = [0x10, 0x00, 0x44, 0x33, 0x22, 0x11, 0xCD, 0xAB];
    let (got, used) = NodeData::decode(&wire).expect("parses");
    assert_eq!(used, 8);
    assert_eq!(got, NodeData { flag: 0x0010, translation: Some(0x1122_3344), scale: Some(0xABCD) });
    assert_eq!(got.encode(), wire);

    // flag 0x0015: both default bits set, so neither part follows.
    let (got, used) = NodeData::decode(&[0x15, 0x00]).expect("parses");
    assert_eq!((got.translation, got.scale, used), (None, None, 2));
}

#[test]
fn encode_sets_the_bit_for_none_and_clears_it_for_some() {
    // The caller's bits disagree with the fields: bit 0 set though translation is Some, bit 2
    // clear though scale is None. Encode keeps 0x10 and writes 0x1 clear and 0x4 set.
    let node = NodeData { flag: 0x0011, translation: Some(0x1122_3344), scale: None };
    assert_eq!(node.encode(), [0x14, 0x00, 0x44, 0x33, 0x22, 0x11]);
}

#[test]
fn the_table_says_the_bits_mark_absence() {
    let rows = <NodeData as Message>::SEGMENTS;
    assert_eq!(rows[1].when, Some(Presence::Clear { field: "flag", mask: 0x1 }));
    assert_eq!(rows[2].when, Some(Presence::Clear { field: "flag", mask: 0x4 }));
}

/// After the IS-TWL version block: a byte flag that says the default applies.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Version {
    pub is_default: bool,
    #[when(!is_default)]
    pub version: Option<u16>,
}

#[test]
fn a_negated_bool_is_written_true_for_none() {
    let absent = Version { is_default: false, version: None };
    assert_eq!(absent.encode(), [0x01]);
    let present = Version { is_default: true, version: Some(0x0203) };
    assert_eq!(present.encode(), [0x00, 0x03, 0x02]);

    assert_eq!(
        Version::decode(&[0x00, 0x03, 0x02]).expect("parses"),
        (Version { is_default: false, version: Some(0x0203) }, 3)
    );
    assert_eq!(
        Version::decode(&[0x01]).expect("parses"),
        (Version { is_default: true, version: None }, 1)
    );
    assert_eq!(Version::decode(&[0x02]), Err(ParseError::Malformed { field: "is_default", at: 0 }));
    assert_eq!(
        <Version as Message>::SEGMENTS[1].when,
        Some(Presence::Clear { field: "is_default", mask: 1 })
    );
}

/// The same tests in a bit-addressed message, least significant bit first.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(bits)]
pub struct Packed {
    pub identity: bool,
    #[bits(3)]
    pub flags: u8,
    #[when(!identity)]
    #[bits(4)]
    pub value: Option<u8>,
    #[when(flags & 0x2 == 0)]
    #[bits(8)]
    pub extra: Option<u8>,
}

#[test]
fn a_bit_message_tests_cleared_bits() {
    // identity 0 | flags 101 | value 1010 | extra 11111111: 16 bits.
    let both = Packed { identity: true, flags: 0b111, value: Some(0xA), extra: Some(0xFF) };
    assert_eq!(both.encode(), [0b1010_1010, 0xFF]);
    let (back, used) = Packed::decode(&both.encode()).expect("parses");
    assert_eq!((back.identity, back.flags, used), (false, 0b101, 2));
    assert_eq!((back.value, back.extra), (Some(0xA), Some(0xFF)));

    // identity 1 | flags 010, padded to 8 bits.
    let neither = Packed { identity: false, flags: 0, value: None, extra: None };
    assert_eq!(neither.encode(), [0b0000_0101]);
    assert_eq!(
        Packed::decode(&[0b0000_0101]).expect("parses"),
        (Packed { identity: true, flags: 0b010, value: None, extra: None }, 1)
    );
}
