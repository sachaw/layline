//! Reals in expressions: an `f32` or `f64` field or parameter is a real, a real reaches a count,
//! length or offset only through `as i64`, which has no value for NaN, an infinity or a real
//! outside `i64`, and an integer and a real do not mix. As Midas BLUE's header states its data
//! section's offset and size in `f64`s.

#![cfg(feature = "derive")]

use layline::{EncodeError, Message, ParseError};

/// A size in a real, and a check on another.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
pub struct Sized {
    pub size: f64,
    pub rate: f32,
    #[count(size as i64)]
    #[valid(rate > 0.0 && size >= 0.0)]
    pub data: Vec<u8>,
}

fn sized(size: f64, rate: f32, data: Vec<u8>) -> Sized {
    Sized { size, rate, data }
}

fn wire(size: f64, rate: f32, data: &[u8]) -> Vec<u8> {
    let mut w = size.to_be_bytes().to_vec();
    w.extend(rate.to_be_bytes());
    w.extend(data);
    w
}

#[test]
fn a_count_from_a_real_reads_and_encode_checks_it() {
    let s = sized(3.0, 0.5, vec![7, 8, 9]);
    let bytes = s.encode().unwrap();
    assert_eq!(bytes, wire(3.0, 0.5, &[7, 8, 9]));
    assert_eq!(Sized::decode(&bytes).unwrap(), (s, bytes.len()));
    // `as i64` rounds toward zero, as Rust's does.
    let (back, _) = Sized::decode(&wire(2.9, 0.5, &[1, 2])).unwrap();
    assert_eq!(back.data, [1, 2]);
    // A real does not work back: encode checks the size it is given.
    assert!(matches!(sized(2.0, 0.5, vec![1, 2, 3]).encode(), Err(EncodeError::Mismatch { .. })));
}

#[test]
fn a_real_with_no_integer_is_refused() {
    for size in [f64::NAN, f64::INFINITY, 1e300] {
        assert!(
            matches!(Sized::decode(&wire(size, 0.5, &[])), Err(ParseError::Malformed { .. })),
            "{size}"
        );
    }
    // A negative count is no count.
    assert!(matches!(Sized::decode(&wire(-1.0, 0.5, &[])), Err(ParseError::Malformed { .. })));
}

#[test]
fn a_condition_compares_reals_and_nan_breaks_it() {
    assert!(matches!(Sized::decode(&wire(0.0, 0.0, &[])), Err(ParseError::Malformed { .. })));
    assert!(matches!(Sized::decode(&wire(0.0, f32::NAN, &[])), Err(ParseError::Malformed { .. })));
    assert!(matches!(sized(0.0, -1.0, vec![]).encode(), Err(EncodeError::Mismatch { .. })));
    assert_eq!(Sized::decode(&wire(0.0, 1e-6, &[])).unwrap().0.data, []);
}

/// Kaitai's BLUE gap: `data_start < ext_start * 512 ? data_start - 276 : ext_start * 512 - 276`,
/// where `data_start` is an `f8` and `ext_start` an `s4`. Rust's spelling converts each side.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = le)]
pub struct Gap {
    pub ext_start: i32,
    pub data_start: f64,
    #[count(
        (if data_start < (ext_start * 512) as f64 { data_start - 12.0 } else {
            (ext_start * 512 - 12) as f64
        }) as i64
    )]
    pub gap: Vec<u8>,
}

#[test]
fn integers_and_reals_meet_through_as() {
    let near = Gap { ext_start: 1, data_start: 16.0, gap: vec![0; 4] };
    let bytes = near.encode().unwrap();
    assert_eq!(bytes.len(), 12 + 4);
    assert_eq!(Gap::decode(&bytes).unwrap().0, near);
    let far = Gap { ext_start: 0, data_start: 600.0, gap: vec![] };
    assert!(matches!(Gap::decode(&far_wire(&far)), Err(ParseError::Malformed { .. })));
    let ext_first = Gap { ext_start: 1, data_start: 2000.0, gap: vec![0; 500] };
    assert_eq!(Gap::decode(&ext_first.encode().unwrap()).unwrap().0, ext_first);
}

fn far_wire(g: &Gap) -> Vec<u8> {
    let mut w = g.ext_start.to_le_bytes().to_vec();
    w.extend(g.data_start.to_le_bytes());
    w
}

/// A real parameter: a holder passes a real field, bare or in an expression, to `f32` and `f64`.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(needs(scale: f64, half: f32), endian = be)]
pub struct Scaled {
    #[count((scale * 2.0) as i64)]
    #[valid(half as f64 * 2.0 == scale)]
    pub items: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
pub struct Holder {
    pub scale: f64,
    #[with(scale, scale / 2.0)]
    #[message]
    pub body: Scaled,
}

#[test]
fn a_real_parameter_takes_a_real() {
    let h = Holder { scale: 1.5, body: Scaled { items: vec![4, 5, 6] } };
    let bytes = h.encode().unwrap();
    assert_eq!(bytes[8..], [4, 5, 6]);
    assert_eq!(Holder::decode(&bytes).unwrap().0, h);
    let ctx = ScaledCtx::new(1.0, 0.5);
    assert_eq!(Scaled::decode_with(&[1, 2], ctx).unwrap().0.items, [1, 2]);
    assert!(matches!(
        Scaled::decode_with(&[1, 2], ScaledCtx::new(1.0, 0.25)),
        Err(ParseError::Malformed { .. })
    ));
}

#[test]
fn a_real_too_large_for_an_f32_parameter_is_not_passed() {
    let h = Holder { scale: 1e300, body: Scaled { items: vec![] } };
    assert!(matches!(h.encode(), Err(EncodeError::Inexact { .. })));
    let mut w = 1e300f64.to_be_bytes().to_vec();
    w.push(0);
    assert!(matches!(Holder::decode(&w), Err(ParseError::Malformed { .. })));
}
