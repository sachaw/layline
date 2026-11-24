//! One language for values: counts, lengths, strides, conditions, selectors and offsets are
//! expressions. Encode writes the one field an invertible expression reads, and checks any other.

#![cfg(feature = "derive")]

use layline::{EncodeError, Layout, Message, ParseError};

#[path = "support/rng.rs"]
mod rng;

const FLAG_EXT: u8 = 0x04;
const PDW: u8 = 1;

/// Kaitai's `has_extension and payload_type != 3`: a condition over two fields, which encode
/// checks.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
pub struct Compound {
    pub has_ext: bool,
    pub payload_type: u8,
    #[when(has_ext && payload_type != 3)]
    pub ext: Option<u16>,
}

#[test]
fn a_compound_condition_decides_presence_and_encode_checks_it() {
    let both = Compound { has_ext: true, payload_type: 1, ext: Some(0xBEEF) };
    assert_eq!(both.encode().unwrap(), [1, 1, 0xBE, 0xEF]);
    assert_eq!(Compound::decode(&[1, 1, 0xBE, 0xEF]).unwrap().0, both);
    assert_eq!(Compound::decode(&[1, 3, 0xBE, 0xEF]).unwrap().0.ext, None);

    let absent = Compound { has_ext: true, payload_type: 3, ext: None };
    assert_eq!(Compound::decode(&absent.encode().unwrap()).unwrap().0, absent);

    let lying = Compound { has_ext: true, payload_type: 3, ext: Some(1) };
    assert_eq!(lying.encode(), Err(EncodeError::Mismatch { field: "ext", value: 1, stated: 0 }));
}

/// Kaitai's `extension_len*4 - (13+markings_len)`: a length over two fields, one of them
/// written by the length it states itself.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Extension {
    pub ext_len: u8,
    pub markings_len: u8,
    #[len(markings_len)]
    pub markings: Vec<u8>,
    #[len(ext_len*4 - (13+markings_len))]
    pub rest: Vec<u8>,
}

#[test]
fn a_length_from_two_fields_is_read_and_checked() {
    // ext_len 4: 16 - (13 + 1) = 2 bytes of rest.
    let wire = [4, 1, 0xAA, 0x01, 0x02];
    let (e, used) = Extension::decode(&wire).unwrap();
    assert_eq!(used, 5);
    assert_eq!(
        e,
        Extension { ext_len: 4, markings_len: 1, markings: vec![0xAA], rest: vec![1, 2] }
    );
    // `markings_len` is the markings' length, so encode writes it, and the check reads that.
    let stale = Extension { markings_len: 9, ..e.clone() };
    assert_eq!(stale.encode().unwrap(), wire);

    let short = Extension { rest: vec![1], ..e };
    assert_eq!(short.encode(), Err(EncodeError::Mismatch { field: "rest", value: 1, stated: 2 }));
    // 4 * 0 - 14 is negative: no length, so decode refuses it naming the first field read.
    assert!(matches!(
        Extension::decode(&[0, 1, 0xAA]),
        Err(ParseError::Malformed { field: "ext_len", .. })
    ));
}

/// Invertible in one field: encode works the count back to it.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Affine {
    pub pairs: u8,
    #[count((pairs << 1) + 1)]
    pub items: Vec<u8>,
    pub order: u8,
    #[count(order * order)]
    pub grid: Vec<u8>,
    pub words: u8,
    #[len(20 - words * 2)]
    pub tail: Vec<u8>,
}

#[test]
fn an_invertible_expression_writes_its_field() {
    let a = Affine {
        pairs: 0,
        items: vec![1, 2, 3, 4, 5],
        order: 0,
        grid: vec![0; 9],
        words: 0,
        tail: vec![7; 16],
    };
    let wire = a.encode().unwrap();
    let (back, _) = Affine::decode(&wire).unwrap();
    assert_eq!((back.pairs, back.order, back.words), (2, 3, 2));
    assert_eq!(back.items, a.items);
    assert_eq!(back.tail, a.tail);

    let even = Affine { items: vec![1, 2], ..a.clone() };
    assert_eq!(even.encode(), Err(EncodeError::Inexact { field: "pairs", value: 2 }));
    let ragged = Affine { grid: vec![0; 8], ..a.clone() };
    assert_eq!(ragged.encode(), Err(EncodeError::Inexact { field: "order", value: 8 }));
    let odd = Affine { tail: vec![0; 15], ..a.clone() };
    assert_eq!(odd.encode(), Err(EncodeError::Inexact { field: "words", value: 15 }));
    let wide = Affine { tail: vec![0; 22], ..a };
    assert_eq!(wide.encode(), Err(EncodeError::Unfit { field: "words", value: -1, bits: 0 }));
}

/// Two fields in one count: nothing to work back to, so encode checks it.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Checked {
    pub rows: u8,
    pub cols: u8,
    #[count(rows * cols, cap = 64)]
    pub cells: Vec<u8>,
}

#[test]
fn an_expression_over_two_fields_is_checked() {
    let c = Checked { rows: 2, cols: 3, cells: vec![0; 6] };
    assert_eq!(c.encode().unwrap(), [2, 3, 0, 0, 0, 0, 0, 0]);
    assert_eq!(Checked::decode(&c.encode().unwrap()).unwrap().0, c);
    let wrong = Checked { cells: vec![0; 5], ..c };
    assert_eq!(wrong.encode(), Err(EncodeError::Mismatch { field: "cells", value: 5, stated: 6 }));
    assert!(matches!(Checked::decode(&[9, 9]), Err(ParseError::Malformed { field: "rows", .. })));
}

/// A constant mask is a test of one field, which encode sets.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Flagged {
    pub flags: u8,
    #[when(flags & FLAG_EXT)]
    pub ext: Option<u8>,
    #[when(flags & 0x01 == 0)]
    pub default: Option<u8>,
}

#[test]
fn a_constant_mask_is_set_by_encode() {
    let f = Flagged { flags: 0x80, ext: Some(7), default: None };
    assert_eq!(f.encode().unwrap(), [0x85, 7]);
    assert_eq!(Flagged::decode(&[0x85, 7]).unwrap().0, Flagged { flags: 0x85, ..f });
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2, endian = be)]
pub struct RtpHeader {
    pub payload_type: u8,
    pub seq: u8,
}

/// A condition on the whole value, with no field to write: `#[valid]`.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct PdwPacket {
    #[bytes(2)]
    #[valid(rtp.payload_type == PDW)]
    pub rtp: RtpHeader,
    pub word: u8,
}

#[test]
fn a_condition_with_no_field_to_write_is_checked_both_ways() {
    let p = PdwPacket { rtp: RtpHeader { payload_type: 1, seq: 9 }, word: 3 };
    assert_eq!(p.encode().unwrap(), [1, 9, 3]);
    assert_eq!(PdwPacket::decode(&[1, 9, 3]).unwrap().0, p);
    assert!(matches!(
        PdwPacket::decode(&[2, 9, 3]),
        Err(ParseError::Malformed { field: "rtp", .. })
    ));
    let other = PdwPacket { rtp: RtpHeader { payload_type: 2, seq: 9 }, word: 3 };
    assert_eq!(other.encode(), Err(EncodeError::Mismatch { field: "rtp", value: 0, stated: 1 }));
}

/// An offset and a selector from expressions.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Placed {
    pub off_words: u8,
    pub kind: u8,
    #[seek(off_words * 2)]
    #[count(kind & 0x0f)]
    pub items: Vec<u8>,
}

#[test]
fn an_offset_is_worked_back_and_a_masked_count_checked() {
    let p = Placed { off_words: 0, kind: 0x32, items: vec![5, 6] };
    let wire = p.encode().unwrap();
    assert_eq!(wire, [1, 0x32, 5, 6]);
    assert_eq!(Placed::decode(&wire).unwrap().0, Placed { off_words: 1, ..p.clone() });
    let short = Placed { items: vec![5], ..p };
    assert_eq!(short.encode(), Err(EncodeError::Mismatch { field: "items", value: 1, stated: 2 }));
}

/// Shifts and products that overflow `i64` give no number, and decode refuses them.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Overflowing {
    pub n: u64,
    #[count(n << 62)]
    pub items: Vec<u8>,
}

#[test]
fn evaluation_refuses_overflow_rather_than_wrapping() {
    assert!(matches!(
        Overflowing::decode(&4u64.to_le_bytes()),
        Err(ParseError::Malformed { field: "n", .. })
    ));
    assert_eq!(Overflowing::decode(&0u64.to_le_bytes()).unwrap().0.items, Vec::<u8>::new());
}

/// Random values through every form above: each encodes and reads back, or encode refuses.
#[test]
fn random_values_round_trip_or_are_refused() {
    let mut r = rng::Rng(0x0e1e_c7ed_0000_0b0b);
    let mut wrote = 0;
    for _ in 0..4096 {
        let n = |r: &mut rng::Rng, m: u64| (r.next() % m) as usize;
        let ext = (r.next().is_multiple_of(2)).then(|| r.next() as u16);
        let c =
            Compound { has_ext: r.next().is_multiple_of(2), payload_type: n(&mut r, 5) as u8, ext };
        let k = Checked {
            rows: n(&mut r, 4) as u8,
            cols: n(&mut r, 4) as u8,
            cells: vec![0; n(&mut r, 10)],
        };
        let a = Affine {
            pairs: r.next() as u8,
            items: vec![1; n(&mut r, 8)],
            order: r.next() as u8,
            grid: vec![2; n(&mut r, 17)],
            words: r.next() as u8,
            tail: vec![3; n(&mut r, 24)],
        };
        if let Ok(wire) = c.encode() {
            assert_eq!(Compound::decode(&wire).unwrap().0, c);
            wrote += 1;
        }
        if let Ok(wire) = k.encode() {
            assert_eq!(Checked::decode(&wire).unwrap().0, k);
            wrote += 1;
        }
        if let Ok(wire) = a.encode() {
            let back = Affine::decode(&wire).unwrap().0;
            assert_eq!((back.items, back.grid, back.tail), (a.items, a.grid, a.tail));
            wrote += 1;
        }
    }
    assert!(wrote > 1000, "too few values encoded, so little was proven: {wrote}");
}
