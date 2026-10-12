//! A byte-mode `bool` is one strict byte; `Nonzero` is the flag any nonzero value sets.

#![cfg(all(feature = "derive", feature = "alloc"))]

use layline::{FieldCodec, Layout, Message, Nonzero, ParseError};

/// `BootInfo` from the Pocket Debug cart: a reset reason, a panic flag, a count.
#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 4, endian = le)]
pub struct BootInfo {
    pub reset_reason: u8,
    pub panicked: bool,
    pub boots: u16,
}

#[test]
fn a_byte_bool_is_zero_or_one() {
    assert_eq!(
        BootInfo::decode(&[0x07, 0x01, 0x34, 0x12]),
        Ok(BootInfo { reset_reason: 7, panicked: true, boots: 0x1234 })
    );
    assert_eq!(
        BootInfo::decode(&[0x07, 0x00, 0x34, 0x12]),
        Ok(BootInfo { reset_reason: 7, panicked: false, boots: 0x1234 })
    );
    assert_eq!(
        BootInfo { reset_reason: 7, panicked: true, boots: 0x1234 }.encode(),
        [0x07, 0x01, 0x34, 0x12]
    );
    assert_eq!(
        BootInfo { reset_reason: 7, panicked: false, boots: 0x1234 }.encode(),
        [0x07, 0x00, 0x34, 0x12]
    );
}

#[test]
fn any_other_byte_is_malformed() {
    for byte in [0x02, 0x80, 0xFF] {
        assert_eq!(
            BootInfo::decode(&[0x07, byte, 0x34, 0x12]),
            Err(ParseError::Malformed { field: "panicked", at: 1 })
        );
    }
}

/// IS-TWL `DebugStatus`: flag bytes the host tests with `!= 0`.
#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 4)]
pub struct DebugStatus {
    pub state: u8,
    #[codec(8)]
    pub halted: Nonzero,
    #[codec(16)]
    pub stepping: Nonzero<16>,
}

#[test]
fn a_nonzero_flag_is_set_by_any_nonzero_value() {
    let got = DebugStatus::decode(&[0x05, 0x02, 0x00, 0x80]);
    assert_eq!(got, DebugStatus { state: 5, halted: Nonzero(true), stepping: Nonzero(true) });
    assert_eq!(got.encode(), [0x05, 0x01, 0x01, 0x00]);
    assert_eq!(
        DebugStatus::decode(&[0x05, 0x00, 0x00, 0x00]),
        DebugStatus { state: 5, halted: Nonzero(false), stepping: Nonzero(false) }
    );
    assert!(bool::from(Nonzero::<8>::from_raw(0xFF)));
    assert!(!Nonzero::<8>::from_raw(0).get());
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bits = 8)]
pub struct Packed {
    #[bits(3)]
    pub any: Nonzero<3>,
    #[bits(5)]
    pub rest: u8,
}

#[test]
fn a_nonzero_flag_can_be_a_bit_field() {
    assert_eq!(Packed::decode(&[0b0000_0100]), Packed { any: Nonzero(true), rest: 0 });
    assert_eq!(Packed { any: Nonzero(true), rest: 1 }.encode(), [0b0000_1001]);
}

/// A byte bool in a message sits in the block of fixed fields and can mark presence.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Reply {
    pub code: u8,
    pub has_stamp: bool,
    #[when(has_stamp)]
    pub stamp: Option<u16>,
    pub len: u8,
    #[count(len)]
    pub data: Vec<u8>,
}

#[test]
fn a_message_byte_bool_marks_presence_and_is_strict() {
    let with = [0x09, 0x01, 0x34, 0x12, 0x02, 0xAA, 0xBB];
    let (got, used) = Reply::decode(&with).expect("parses");
    assert_eq!(used, 7);
    assert_eq!(got.stamp, Some(0x1234));
    assert!(got.has_stamp);
    assert_eq!(got.encode(), with);

    let without = Reply { code: 9, has_stamp: true, stamp: None, len: 0, data: vec![0xAA] };
    assert_eq!(without.encode(), [0x09, 0x00, 0x01, 0xAA]);

    assert_eq!(
        Reply::decode(&[0x09, 0x02, 0x34, 0x12, 0x00]),
        Err(ParseError::Malformed { field: "has_stamp", at: 1 })
    );
}
