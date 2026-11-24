//! One language for values: counts, lengths, strides, conditions, selectors and offsets are
//! expressions. Encode writes the one field an invertible expression reads, and checks any other,
//! and where several would write one field, the first writes it and encode checks the rest.

#![cfg(feature = "derive")]

use layline::{EncodeError, Layout, Message, ParseError, Stated};

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

/// Kaitai's `rfextgeo_ext`: one length sizes the markings and, with the holder's extension
/// length, the padding after them. The text writes it, and encode checks the padding.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be, needs(extension_len: u16))]
pub struct RfextgeoExt {
    pub timestamp_seconds: u64,
    pub timestamp_nanos: u32,
    pub markings_len: Stated<u8>,
    #[len(markings_len)]
    #[text]
    pub markings: String,
    #[len(extension_len * 4 - (13 + markings_len))]
    pub padding: Vec<u8>,
}

/// One count for a collection and a string: the collection, first, writes it.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Twice {
    pub n: Stated<u8>,
    #[count(n)]
    pub items: Vec<u8>,
    #[len(n)]
    #[text]
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
pub enum Level {
    #[value(0)]
    Bytes(Vec<u8>),
}

/// A count that also sizes a switch: the count writes it, and the arm is checked.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Sized {
    pub kind: u8,
    pub n: u8,
    #[count(n)]
    pub items: Vec<u8>,
    #[switch(kind)]
    #[len(n)]
    pub body: Level,
}

/// One flag for two fields, present together: the first writes it.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Either {
    pub on: Stated<bool>,
    #[when(on)]
    pub yes: Option<u8>,
    #[when(!on)]
    pub no: Option<u16>,
}

/// A flags word that also counts: its presence bit writes it, and encode checks the count.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Counted {
    pub flags: u8,
    #[when(flags & 0x01)]
    pub extra: Option<u8>,
    #[count(flags)]
    pub items: Vec<u8>,
}

fn rfextgeo(markings: &str, padding: usize) -> RfextgeoExt {
    RfextgeoExt {
        timestamp_seconds: 0x0102_0304_0506_0708,
        timestamp_nanos: 999_999_999,
        markings_len: Stated::default(),
        markings: String::from(markings),
        padding: vec![0; padding],
    }
}

#[test]
fn one_length_for_two_values_is_written_by_the_first_and_checked_by_the_second() {
    // Four words: 13 fixed bytes, 1 of markings and 2 of padding.
    let ctx = RfextgeoExtCtx { extension_len: 4 };
    let ext = rfextgeo("U", 2);
    let wire = ext.encode_with(ctx).unwrap();
    #[rustfmt::skip]
    assert_eq!(wire, [
        1, 2, 3, 4, 5, 6, 7, 8,     // seconds
        0x3B, 0x9A, 0xC9, 0xFF,     // nanoseconds
        1, b'U',                    // the markings, which state their length
        0, 0,                       // the padding the extension's length leaves
    ]);
    let (back, used) = RfextgeoExt::decode_with(&wire, ctx).unwrap();
    assert_eq!((back.markings_len.get(), used), (1, 16));
    assert_eq!(back, ext);

    assert_eq!(
        rfextgeo("U", 3).encode_with(ctx),
        Err(EncodeError::Mismatch { field: "padding", value: 3, stated: 2 })
    );
    assert_eq!(
        rfextgeo("UNCLASSIFIED", 2).encode_with(ctx),
        Err(EncodeError::Mismatch { field: "padding", value: 2, stated: -9 })
    );
    assert!(rfextgeo("", 3).encode_with(ctx).is_ok());

    let twice = Twice { n: Stated::default(), items: vec![7, 8], label: String::from("ab") };
    assert_eq!(twice.encode().unwrap(), [2, 7, 8, b'a', b'b']);
    assert_eq!(Twice::decode(&[2, 7, 8, b'a', b'b']).unwrap().0, twice);
    let long = Twice { label: String::from("abc"), ..twice };
    assert_eq!(long.encode(), Err(EncodeError::Mismatch { field: "label", value: 3, stated: 2 }));

    let sized = Sized { kind: 0, n: 0, items: vec![1], body: Level::Bytes(vec![2]) };
    assert_eq!(sized.encode().unwrap(), [0, 1, 1, 2]);
    assert_eq!(Sized::decode(&[0, 1, 1, 2]).unwrap().0, Sized { n: 1, ..sized.clone() });
    let short = Sized { body: Level::Bytes(vec![]), ..sized };
    assert_eq!(short.encode(), Err(EncodeError::Mismatch { field: "body", value: 0, stated: 1 }));
}

#[test]
fn a_flag_one_field_writes_is_checked_for_every_other() {
    let yes = Either { on: Stated::default(), yes: Some(1), no: None };
    assert_eq!(yes.encode().unwrap(), [1, 1]);
    assert_eq!(Either::decode(&[1, 1]).unwrap().0, yes);
    let no = Either { on: Stated::default(), yes: None, no: Some(0x0302) };
    assert_eq!(no.encode().unwrap(), [0, 2, 3]);
    assert_eq!(Either::decode(&[0, 2, 3]).unwrap().0, no);
    let both = Either { on: Stated::default(), yes: Some(1), no: Some(2) };
    assert_eq!(both.encode(), Err(EncodeError::Mismatch { field: "no", value: 1, stated: 0 }));
    let neither = Either { on: Stated::default(), yes: None, no: None };
    assert_eq!(neither.encode(), Err(EncodeError::Mismatch { field: "no", value: 0, stated: 1 }));

    let counted = Counted { flags: 2, extra: Some(9), items: vec![1, 2, 3] };
    assert_eq!(counted.encode().unwrap(), [3, 9, 1, 2, 3]);
    assert_eq!(Counted::decode(&[3, 9, 1, 2, 3]).unwrap().0, Counted { flags: 3, ..counted });
    let short = Counted { flags: 2, extra: Some(9), items: vec![1, 2] };
    assert_eq!(short.encode(), Err(EncodeError::Mismatch { field: "items", value: 2, stated: 3 }));
}

#[test]
fn hostile_lengths_for_two_values_are_refused() {
    let ctx = RfextgeoExtCtx { extension_len: 4 };
    let mut wire = rfextgeo("U", 2).encode_with(ctx).unwrap();
    // Markings past the extension leave a negative padding.
    wire[12] = 4;
    assert!(RfextgeoExt::decode_with(&wire, ctx).is_err());
    // An extension too short for its fixed bytes.
    assert!(RfextgeoExt::decode_with(&wire, RfextgeoExtCtx { extension_len: 3 }).is_err());
    assert!(RfextgeoExt::decode_with(&wire, RfextgeoExtCtx { extension_len: u16::MAX }).is_err());
    assert!(Twice::decode(&[3, 1, 2, 3, b'a', b'b']).is_err());
}

#[test]
fn random_bytes_with_one_length_for_two_values_decode_to_what_encodes_back() {
    let mut r = rng::Rng(0x0e1e_c7ed_0000_0c0c);
    let mut read = 0;
    for _ in 0..20_000 {
        let mut buf = vec![0u8; 13 + (r.next() % 20) as usize];
        r.fill(&mut buf);
        buf[12] %= 8;
        for b in &mut buf[13..] {
            *b &= 0x7F;
        }
        let ctx = RfextgeoExtCtx { extension_len: (r.next() % 9) as u16 };
        if let Ok((ext, used)) = RfextgeoExt::decode_with(&buf, ctx) {
            assert_eq!(ext.encode_with(ctx).unwrap(), &buf[..used]);
            read += 1;
        }
        if let Ok((twice, used)) = Twice::decode(&buf[12..]) {
            assert_eq!(twice.encode().unwrap(), &buf[12..12 + used]);
        }
        if let Ok((either, used)) = Either::decode(&buf[..3]) {
            assert_eq!(either.encode().unwrap(), &buf[..used]);
        }
    }
    assert!(read > 1000, "the corpus reaches whole extensions: {read}");
}
