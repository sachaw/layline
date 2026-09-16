//! `#[until(mask = M)]`: a run that ends at the element with bit M set.
//! `#[until(b)]`: a run of one-byte elements that ends at byte `b`.
//! The shape of HDLC extended addressing, with the bytes written out by hand.

#![cfg(feature = "derive")]

use layline::{Layout, Message};

/// `prefix = 1` keeps the continuation bit out of the element's fields, and the run writes it.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8, endian = le, order = lsb, prefix = 1)]
pub struct Dest {
    #[bits(7)]
    pub address: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct AddressField {
    source: u8,
    #[until(mask = 0x01)]
    #[bytes(1)]
    destinations: Vec<Dest>,
}

#[test]
fn the_run_ends_at_the_element_that_carries_the_mask() {
    let wire = [0x55u8, 0x0A << 1, 0x0B << 1, (0x0C << 1) | 1];

    let (m, used) = AddressField::decode(&wire).expect("parses");
    assert_eq!(used, wire.len(), "the run consumed its terminator element");
    assert_eq!(m.source, 0x55);
    assert_eq!(
        m.destinations,
        vec![Dest { address: 0x0A }, Dest { address: 0x0B }, Dest { address: 0x0C }],
        "every element decodes, the flagged one included"
    );
}

#[test]
fn the_continuation_bit_is_derived_from_position() {
    let m = AddressField {
        source: 0x55,
        destinations: vec![Dest { address: 0x0A }, Dest { address: 0x0B }, Dest { address: 0x0C }],
    };
    assert_eq!(
        m.encode(),
        vec![0x55, 0x0A << 1, 0x0B << 1, (0x0C << 1) | 1],
        "only the last element has the mask bit set"
    );
}

#[test]
fn an_unterminated_run_refuses() {
    let wire = [0x55u8, 0x0A << 1, 0x0B << 1];
    assert!(
        matches!(AddressField::decode(&wire), Err(layline::ParseError::Short { .. })),
        "the body ended before an element with the mask bit"
    );
}

#[test]
fn the_round_trip_closes_over_every_run_length() {
    for n in 1usize..=8 {
        let mut wire = vec![0x7Fu8];
        for i in 0..n {
            let last = i == n - 1;
            wire.push(((i as u8 + 1) << 1) | u8::from(last));
        }
        let (m, used) = AddressField::decode(&wire).expect("parses");
        assert_eq!(used, wire.len(), "{n} elements consumed");
        assert_eq!(m.destinations.len(), n, "{n} elements read");
        assert_eq!(m.encode(), wire, "{n} elements re-encode identically");
    }
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
pub struct Octet {
    #[bits(7)]
    pub value: u8,
    #[bits(1)]
    pub spare: bool,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Sentinel {
    kind: u8,
    #[until(0xFF)]
    raw: Vec<u8>,
    #[until(0)]
    #[bytes(1)]
    octets: Vec<Octet>,
    tail: u8,
}

#[test]
fn a_byte_terminated_run_stops_at_the_byte_and_does_not_keep_it() {
    let wire = [0x01u8, 0x0A, 0x0B, 0xFF, 0x41, 0x42, 0x00, 0x7E];
    let (m, used) = Sentinel::decode(&wire).expect("parses");
    assert_eq!(used, wire.len(), "both terminators were consumed");
    assert_eq!(m.raw, vec![0x0A, 0x0B], "the terminator is not an element");
    assert_eq!(
        m.octets,
        vec![Octet { value: 0x41, spare: false }, Octet { value: 0x42, spare: false }]
    );
    assert_eq!(m.tail, 0x7E, "the field after the run starts after the terminator");
}

#[test]
fn encode_appends_the_terminator_the_struct_does_not_hold() {
    let m = Sentinel {
        kind: 1,
        raw: vec![0x0A, 0x0B],
        octets: vec![Octet { value: 0x41, spare: false }],
        tail: 0x7E,
    };
    assert_eq!(m.encode(), vec![0x01, 0x0A, 0x0B, 0xFF, 0x41, 0x00, 0x7E]);
}

#[test]
fn an_empty_run_is_just_its_terminator() {
    let wire = [0x00u8, 0xFF, 0x00, 0x09];
    let (m, used) = Sentinel::decode(&wire).expect("parses");
    assert_eq!((used, m.raw.len(), m.octets.len(), m.tail), (4, 0, 0, 9));
    assert_eq!(m.encode(), wire);
}

#[test]
fn a_run_whose_terminator_never_arrives_is_short() {
    assert!(
        matches!(Sentinel::decode(&[0x00, 0x0A, 0x0B]), Err(layline::ParseError::Short { .. })),
        "the body ended before the terminator"
    );
}
