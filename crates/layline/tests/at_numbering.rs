//! `#[at(bit = N)]` counts in the declared bit numbering.
//! Under `order = msb` that is the MSB0 number a standard's table prints.

#![cfg(feature = "derive")]

use layline::{Layout, table::StatedUnit};

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 32, endian = be, order = msb)]
pub struct CanId {
    #[bits(29)]
    #[at(bit = 0)]
    pub id: u32,
    #[bits(1)]
    #[at(bit = 29)]
    pub ide: bool,
    #[bits(1)]
    #[at(bit = 30)]
    pub rtr: bool,
    #[bits(1)]
    #[at(bit = 31)]
    pub reserved: bool,
}

fn stated<T: Layout>() -> Vec<(&'static str, StatedUnit, u64)> {
    T::STATED.iter().map(|s| (s.field, s.unit, s.pos)).collect()
}

fn joins<T: Layout>() {
    for s in T::STATED {
        let f =
            T::FIELDS.iter().find(|f| f.name == s.field).expect("every `#[at]` field is in FIELDS");
        assert_eq!(
            s.bits(),
            f.extent.start(),
            "`{}`: STATED and FIELDS have one bit numbering",
            s.field
        );
    }
}

#[test]
fn a_word_mode_msb_claim_is_published_where_it_landed() {
    assert_eq!(
        stated::<CanId>(),
        [
            ("id", StatedUnit::Bit, 3),
            ("ide", StatedUnit::Bit, 2),
            ("rtr", StatedUnit::Bit, 1),
            ("reserved", StatedUnit::Bit, 0),
        ]
    );
    joins::<CanId>();
}

#[test]
fn a_can_identifier_encodes_as_the_register_the_standard_draws() {
    let id = 0x1ABC_DEF5u32;
    assert!(id < 1 << 29);
    let w = CanId { id, ide: true, rtr: false, reserved: true };
    let oracle = (id << 3) | (1 << 2) | 1;
    assert_eq!(w.encode(), oracle.to_be_bytes());
    assert_eq!(CanId::decode(&oracle.to_be_bytes()), w);
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(words = 2, endian = be, order = msb)]
pub struct Pair {
    #[bits(4)]
    #[at(bit = 0)]
    pub kind: u8,
    #[bits(12)]
    #[at(bit = 4)]
    pub value: u16,
    #[bits(8)]
    #[at(bit = 16)]
    pub hi: u8,
    #[bits(8)]
    #[at(bit = 24)]
    pub lo: u8,
}

/// Under `order = msb` in words mode, the derive mirrors a position within its 16-bit word.
#[test]
fn a_words_mode_msb_claim_is_published_where_it_landed() {
    assert_eq!(
        stated::<Pair>(),
        [
            ("kind", StatedUnit::Bit, 12),
            ("value", StatedUnit::Bit, 0),
            ("hi", StatedUnit::Bit, 24),
            ("lo", StatedUnit::Bit, 16),
        ]
    );
    joins::<Pair>();
}

#[test]
fn a_words_mode_pair_encodes_as_two_hand_built_words() {
    let p = Pair { kind: 0xA, value: 0x5C3, hi: 0x12, lo: 0x34 };
    let word0 = (u16::from(p.kind) << 12) | p.value;
    let word1 = (u16::from(p.hi) << 8) | u16::from(p.lo);
    let mut oracle = [0u8; 4];
    oracle[..2].copy_from_slice(&word0.to_be_bytes());
    oracle[2..].copy_from_slice(&word1.to_be_bytes());
    assert_eq!(p.encode(), oracle);
    assert_eq!(Pair::decode(&oracle), p);
}
