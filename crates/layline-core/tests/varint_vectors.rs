//! The varints, against the vectors their specifications publish.
//!
//! - DWARF Debugging Information Format v5, §7.6: Table 7.7 (unsigned LEB128) and Table 7.8 (signed LEB128).
//! - Wikipedia, "LEB128": the two worked examples, 624485 unsigned and -123456 signed.
//! - Protocol Buffers encoding guide (protobuf.dev): the base-128 varint examples and the zigzag mapping.

#![cfg(feature = "num")]

use layline_core::num::{Sleb128, Uleb128, unzigzag, zigzag};
use layline_core::{Fixed, VarCodec};

fn enc<T: VarCodec>(v: T) -> Vec<u8> {
    let mut room = [0u8; 16];
    let mut out = Fixed::new(&mut room);
    v.encode(&mut out).expect("sixteen bytes hold any varint");
    out.into_written().to_vec()
}

/// DWARF 5, Table 7.7.
#[test]
fn unsigned_leb128_matches_dwarf_table_7_7() {
    let vectors: &[(u64, &[u8])] = &[
        (2, &[0x02]),
        (127, &[0x7F]),
        (128, &[0x80, 0x01]),
        (129, &[0x81, 0x01]),
        (12857, &[0xB9, 0x64]),
    ];
    for (value, bytes) in vectors {
        assert_eq!(enc(Uleb128(*value)), *bytes, "encoding {value}");
        assert_eq!(
            Uleb128::decode(bytes).ok(),
            Some((Uleb128(*value), bytes.len())),
            "decoding {value}",
        );
    }
}

/// DWARF 5, Table 7.8.
#[test]
fn signed_leb128_matches_dwarf_table_7_8() {
    let vectors: &[(i64, &[u8])] = &[
        (2, &[0x02]),
        (-2, &[0x7E]),
        (127, &[0xFF, 0x00]),
        (-127, &[0x81, 0x7F]),
        (128, &[0x80, 0x01]),
        (-128, &[0x80, 0x7F]),
        (129, &[0x81, 0x01]),
        (-129, &[0xFF, 0x7E]),
    ];
    for (value, bytes) in vectors {
        assert_eq!(enc(Sleb128(*value)), *bytes, "encoding {value}");
        assert_eq!(
            Sleb128::decode(bytes).ok(),
            Some((Sleb128(*value), bytes.len())),
            "decoding {value}",
        );
    }
}

/// The Wikipedia worked examples, and protobuf.dev's two base-128 examples.
#[test]
fn the_published_worked_examples_come_out_byte_for_byte() {
    assert_eq!(enc(Uleb128(624_485)), [0xE5, 0x8E, 0x26]);
    assert_eq!(enc(Sleb128(-123_456)), [0xC0, 0xBB, 0x78]);

    assert_eq!(enc(Uleb128(1)), [0x01]);
    assert_eq!(enc(Uleb128(150)), [0x96, 0x01]);
}

/// protobuf.dev, "Signed integers": the zigzag table and its two formulas.
#[test]
fn zigzag_matches_the_protobuf_table() {
    let vectors: &[(i64, u64)] = &[
        (0, 0),
        (-1, 1),
        (1, 2),
        (-2, 3),
        (0x3FFF_FFFF, 0x7FFF_FFFE),
        (-0x4000_0000, 0x7FFF_FFFF),
        (0x7FFF_FFFF, 0xFFFF_FFFE),
        (-0x8000_0000, 0xFFFF_FFFF),
        (i64::MAX, u64::MAX - 1),
        (i64::MIN, u64::MAX),
    ];
    for (signed, unsigned) in vectors {
        assert_eq!(zigzag(*signed), *unsigned, "zigzag {signed}");
        assert_eq!(unzigzag(*unsigned), *signed);
    }

    for n in [0i32, -1, 1, -2, i32::MAX, i32::MIN] {
        let expected = u64::from(((n << 1) ^ (n >> 31)) as u32);
        assert_eq!(zigzag(i64::from(n)), expected, "32-bit {n}");
    }
}

#[test]
fn every_boundary_width_round_trips() {
    fn closes<T: VarCodec + Copy + core::fmt::Debug + PartialEq>(v: T) {
        let bytes = enc(v);
        assert_eq!(T::decode(&bytes).ok(), Some((v, bytes.len())), "{bytes:02X?}");
    }

    let mut ones = 0u64;
    for shift in 0..64 {
        ones = (ones << 1) | 1;
        for probe in [ones, ones.wrapping_add(1), 1u64 << shift] {
            closes(Uleb128(probe));
            closes(Sleb128(probe as i64));
        }
    }
    assert_eq!(enc(Uleb128(u64::MAX)).len(), 10, "ten groups is the whole of a u64");
    assert_eq!(enc(Sleb128(i64::MIN)).len(), 10);
    assert_eq!(enc(Sleb128(-1)).len(), 1, "and sign extension costs nothing");
}

#[test]
fn malformed_input_is_refused() {
    assert!(Uleb128::decode(&[]).is_err());
    assert!(Uleb128::decode(&[0x80]).is_err());
    assert!(Uleb128::decode(&[0x80; 9]).is_err());
    assert!(Sleb128::decode(&[0x80; 9]).is_err());

    assert!(Uleb128::decode(&[0x80; 11]).is_err());
    assert!(Sleb128::decode(&[0x80; 11]).is_err());

    let mut top = [0x80u8; 10];
    top[9] = 0x01;
    assert_eq!(Uleb128::decode(&top).ok(), Some((Uleb128(1 << 63), 10)));
    top[9] = 0x02;
    assert!(Uleb128::decode(&top).is_err());

    let mut sover = [0x80u8; 10];
    sover[9] = 0x40;
    assert!(Sleb128::decode(&sover).is_err());

    assert_eq!(Uleb128::decode(&[0x01, 0xFF, 0xFF]).ok(), Some((Uleb128(1), 1)));
}

#[test]
fn a_successful_decode_always_advances() {
    for b in 0u16..=255 {
        let buf = [b as u8, 0x00];
        for used in
            [Uleb128::decode(&buf).ok().map(|(_, n)| n), Sleb128::decode(&buf).ok().map(|(_, n)| n)]
                .into_iter()
                .flatten()
        {
            assert!(used >= 1, "byte {b:#04x} decoded without advancing");
        }
    }
}
