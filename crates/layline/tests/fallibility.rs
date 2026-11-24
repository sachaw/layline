//! Encode returns a `Result` exactly when a type declares something a value can break, or holds a
//! type that does. The annotated types fail to compile if a signature changes.

#![cfg(feature = "derive")]

#[path = "support/rng.rs"]
mod rng;

use layline::{EncodeError, FieldCodec, Layout, Message, Overflow, U};
use layline_core::num::{Uleb128, Vlq};

/// Whole scalars: nothing a value can break.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 3, endian = be)]
pub struct Plain {
    pub a: u8,
    pub b: u16,
}

/// A field narrower than its type.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
pub struct Narrow {
    #[bits(3)]
    pub a: u8,
    #[bits(5)]
    pub b: u8,
}

/// The same field, keeping its low bits.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8, truncate)]
pub struct Kept {
    #[bits(3)]
    pub a: u8,
    #[bits(5)]
    pub b: u8,
}

/// A range on a whole field.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 1)]
pub struct Hour {
    #[range(0..=23)]
    pub h: u8,
}

/// Holds an infallible layout: as infallible as it.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4, endian = be)]
pub struct HoldsPlain {
    #[bytes(3)]
    pub inner: Plain,
    pub tail: u8,
}

/// Holds a ranged layout that is private to this module: as fallible as it.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct HoldsHour {
    #[bytes(1)]
    hour: Hour,
    pub tail: u8,
}

/// An enum whose `#[other]` arm holds more bits than the field.
#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(4)]
pub enum Kind {
    #[value(1)]
    One,
    #[other]
    Unknown(u8),
}

/// An enum that names every 2-bit value.
#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(2)]
pub enum Lane {
    #[value(0)]
    A,
    #[value(1)]
    B,
    #[value(2)]
    C,
    #[value(3)]
    D,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
pub struct HoldsKind {
    #[bits(4)]
    pub kind: Kind,
    #[bits(2)]
    pub lane: Lane,
    #[bits(2)]
    pub spare: u8,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
pub struct HoldsLane {
    #[bits(2)]
    pub lane: Lane,
    #[bits(6)]
    pub spare: U<6>,
}

/// A newtype narrower than its integer: as fallible as a narrow field.
#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(6)]
pub struct Six(pub u8);

/// A length and its bytes: a `u8` cannot count past 255.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Counted {
    pub n: u8,
    #[count(n)]
    pub items: Vec<u8>,
}

/// Bytes to the end: nothing to break.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Rest {
    pub tag: u8,
    #[fill]
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct HoldsUleb {
    #[var]
    pub n: Uleb128,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct HoldsVlq {
    #[var]
    pub n: Vlq<2>,
}

/// A message that holds itself behind a flag encode writes, and nothing else to break.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Node {
    pub more: bool,
    pub value: u8,
    #[when(more)]
    #[message]
    pub next: Option<Box<Node>>,
}

#[test]
fn a_type_that_declares_nothing_a_value_can_break_keeps_an_infallible_encode() {
    const _: () = assert!(!<Plain as Layout>::ENCODE_FALLIBLE);
    const _: () = assert!(!<Kept as Layout>::ENCODE_FALLIBLE);
    const _: () = assert!(!<HoldsPlain as Layout>::ENCODE_FALLIBLE);
    const _: () = assert!(!<HoldsLane as Layout>::ENCODE_FALLIBLE);
    const _: () = assert!(!<Rest as Message>::ENCODE_FALLIBLE);
    const _: () = assert!(!<HoldsUleb as Message>::ENCODE_FALLIBLE);
    const _: () = assert!(!<Node as Message>::ENCODE_FALLIBLE);

    let _: [u8; 3] = Plain { a: 1, b: 2 }.encode();
    let _: [u8; 1] = Kept { a: 0xFF, b: 0xFF }.encode();
    let _: [u8; 4] = HoldsPlain { inner: Plain { a: 1, b: 2 }, tail: 3 }.encode();
    let _: [u8; 1] = HoldsLane { lane: Lane::C, spare: U::<6>::new(1).unwrap() }.encode();
    let _: Vec<u8> = Rest { tag: 1, data: vec![2; 300] }.encode();
    let _: Vec<u8> = HoldsUleb { n: Uleb128(u64::MAX) }.encode();
    let mut room = [0u8; 1];
    let full: Result<(), Overflow> =
        Rest { tag: 1, data: vec![2] }.encode_into(&mut layline::Fixed::new(&mut room));
    assert_eq!(full, Err(Overflow));

    let tail = Node { more: false, value: 2, next: None };
    let list = Node { more: false, value: 1, next: Some(Box::new(tail)) };
    let wire: Vec<u8> = list.encode();
    assert_eq!(wire, [1, 1, 0, 2]);
    assert_eq!(Node::decode(&wire).unwrap().0.next.unwrap().value, 2);
}

#[test]
fn a_type_that_declares_something_a_value_can_break_returns_a_result() {
    const _: () = assert!(<Narrow as Layout>::ENCODE_FALLIBLE);
    const _: () = assert!(<Hour as Layout>::ENCODE_FALLIBLE);
    const _: () = assert!(<HoldsHour as Layout>::ENCODE_FALLIBLE);
    const _: () = assert!(<HoldsKind as Layout>::ENCODE_FALLIBLE);
    const _: () = assert!(<Kind as FieldCodec>::ENCODE_FALLIBLE);
    const _: () = assert!(!<Lane as FieldCodec>::ENCODE_FALLIBLE);
    const _: () = assert!(<Six as FieldCodec>::ENCODE_FALLIBLE);
    assert_eq!(Six(63).try_raw(), Ok(63));
    assert_eq!(Six(64).try_raw(), Err(EncodeError::Unfit { field: "Six", value: 64, bits: 6 }));
    const _: () = assert!(<Counted as Message>::ENCODE_FALLIBLE);
    const _: () = assert!(<HoldsVlq as Message>::ENCODE_FALLIBLE);

    let narrow: Result<[u8; 1], EncodeError> = Narrow { a: 8, b: 0 }.encode();
    assert_eq!(narrow, Err(EncodeError::Unfit { field: "a", value: 8, bits: 3 }));
    let hour: Result<[u8; 1], EncodeError> = Hour { h: 24 }.encode();
    assert_eq!(hour, Err(EncodeError::OutOfRange { field: "h", value: 24, lo: 0, hi: 23 }));
    let held: Result<[u8; 2], EncodeError> = HoldsHour { hour: Hour { h: 99 }, tail: 0 }.encode();
    assert_eq!(held, Err(EncodeError::OutOfRange { field: "h", value: 99, lo: 0, hi: 23 }));
    let kind: Result<[u8; 1], EncodeError> =
        HoldsKind { kind: Kind::Unknown(16), lane: Lane::A, spare: 0 }.encode();
    assert_eq!(kind, Err(EncodeError::Unfit { field: "kind", value: 16, bits: 4 }));
    let counted: Result<Vec<u8>, EncodeError> = Counted { n: 0, items: vec![0; 256] }.encode();
    assert_eq!(counted, Err(EncodeError::Unfit { field: "n", value: 256, bits: 0 }));
    let vlq: Result<Vec<u8>, EncodeError> = HoldsVlq { n: Vlq(1 << 14) }.encode();
    assert_eq!(vlq, Err(EncodeError::Unfit { field: "n", value: 1 << 14, bits: 14 }));
}

#[test]
fn a_full_buffer_is_still_an_overflow_from_a_fallible_type() {
    let mut room = [0u8; 2];
    let m = Counted { n: 0, items: vec![1, 2, 3] };
    assert_eq!(m.encode_into(&mut layline::Fixed::new(&mut room)), Err(EncodeError::Overflow));
}

#[test]
fn a_view_of_a_refusing_layout_comes_by_try_from() {
    #[derive(Debug, Clone, PartialEq, Layout)]
    #[layout(bits = 8, view)]
    pub struct Flags {
        #[bits(3)]
        pub a: u8,
        #[bits(5)]
        pub b: u8,
    }
    assert!(FlagsView::try_from(&Flags { a: 7, b: 31 }).is_ok());
    assert_eq!(
        FlagsView::try_from(&Flags { a: 8, b: 0 }).map(|v| *v.as_wire()),
        Err(EncodeError::Unfit { field: "a", value: 8, bits: 3 })
    );
}

/// Random values: encode refuses exactly the values its declarations rule out, and decode reads
/// back every value it accepts.
#[test]
fn random_values_are_refused_exactly_when_they_break_a_declaration() {
    let mut r = rng::Rng(0x5EED_C0DE);
    for _ in 0..10_000 {
        let (a, b, h) = (r.next() as u8, r.next() as u8, r.next() as u8);
        let fits = a < 8 && b < 32;
        match (Narrow { a, b }).encode() {
            Ok(wire) => {
                assert!(fits, "{a} {b} encoded");
                assert_eq!(Narrow::decode(&wire), Narrow { a, b });
            }
            Err(EncodeError::Unfit { field, .. }) => {
                assert!(!fits);
                assert_eq!(field, if a >= 8 { "a" } else { "b" });
            }
            Err(e) => panic!("{e}"),
        }
        let kept = Kept { a, b }.encode();
        assert_eq!(Kept::decode(&kept), Kept { a: a & 7, b: b & 31 }, "truncate keeps low bits");
        match (Hour { h }).encode() {
            Ok(wire) => assert_eq!(Hour::decode(&wire), Ok(Hour { h })),
            Err(e) => {
                assert!(h > 23);
                assert_eq!(
                    e,
                    EncodeError::OutOfRange { field: "h", value: h.into(), lo: 0, hi: 23 }
                );
            }
        }
        let n = (r.next() % 300) as usize;
        match (Counted { n: 0, items: vec![7; n] }).encode() {
            Ok(wire) => {
                assert!(n <= 255);
                assert_eq!(Counted::decode(&wire).unwrap().0.items.len(), n);
            }
            Err(e) => {
                assert!(n > 255);
                assert_eq!(e, EncodeError::Unfit { field: "n", value: n as i128, bits: 0 });
            }
        }
    }
}
