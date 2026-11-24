//! A layout nesting a layout with `#[range]`: the holder's `decode` runs the nested checks, and
//! is fallible exactly when something it nests is.

#![cfg(feature = "derive")]

use layline::{Layout, Message, ParseError};

#[path = "support/rng.rs"]
mod rng;

/// A Kaitai-style flag byte whose spare bits must be zero.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8, endian = be, order = msb)]
pub struct Flags {
    #[bits(1)]
    pub has_scaling: bool,
    #[bits(1)]
    pub units: bool,
    #[bits(1)]
    pub has_origin: bool,
    #[bits(5)]
    #[range(0..=0)]
    pub reserved: u8,
}

/// `psd_ext`: a holder with no checks of its own.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 12, endian = be)]
pub struct PsdExt {
    pub duration: f64,
    pub num_bin: u16,
    #[bytes(1)]
    pub bits: Flags,
    pub num_blocks: u8,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2, endian = be)]
pub struct Point {
    pub x: u8,
    pub y: u8,
}

/// Nests only layouts that decode every wire.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 5, endian = be)]
pub struct Segment {
    pub tag: u8,
    #[bytes(2)]
    pub from: Point,
    #[bytes(2)]
    pub to: Point,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 13, endian = be)]
pub struct Outer {
    pub kind: u8,
    #[bytes(12)]
    pub ext: PsdExt,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 3, endian = be)]
pub struct Run {
    pub n: u8,
    #[bytes(1)]
    pub flags: [Flags; 2],
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 3, endian = be)]
pub struct Sealed {
    #[magic(0xA5)]
    pub sync: u8,
    #[range(..=9)]
    pub level: u8,
    #[bytes(1)]
    pub flags: Flags,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Packet {
    pub len: u8,
    #[bytes(12)]
    pub ext: PsdExt,
    #[count(len)]
    pub tail: Vec<u8>,
}

fn flags(spare: u8) -> u8 {
    0b1010_0000 | spare
}

fn psd(spare: u8) -> [u8; 12] {
    let mut wire = [0u8; 12];
    wire[..8].copy_from_slice(&1.5f64.to_be_bytes());
    wire[8..10].copy_from_slice(&7u16.to_be_bytes());
    wire[10] = flags(spare);
    wire[11] = 3;
    wire
}

#[test]
fn a_holder_decodes_what_its_nested_layout_admits() {
    let got = PsdExt::decode(&psd(0)).expect("the spare bits are zero");
    assert_eq!((got.duration, got.num_bin, got.num_blocks), (1.5, 7, 3));
    assert!(got.bits.has_scaling && !got.bits.units && got.bits.has_origin);
    assert_eq!(got.encode().unwrap(), psd(0));
}

#[test]
fn a_holder_refuses_what_its_nested_layout_refuses() {
    assert_eq!(
        PsdExt::decode(&psd(0b0_0100)),
        Err(ParseError::OutOfRange { field: "reserved", value: 4, lo: 0, hi: 0 }),
    );
}

#[test]
fn the_trait_decode_refuses_it_too() {
    assert!(matches!(
        <PsdExt as Layout>::decode_slice(&psd(1)),
        Err(ParseError::OutOfRange { field: "reserved", .. })
    ));
    assert!(matches!(
        <PsdExt as Layout>::decode_slice(&psd(1)[..11]),
        Err(ParseError::Short { need_bytes: 12, got_bytes: 11, .. })
    ));
}

#[test]
fn a_holder_is_fallible_exactly_when_what_it_nests_is() {
    const {
        assert!(<Flags as Layout>::DECODE_FALLIBLE);
        assert!(<PsdExt as Layout>::DECODE_FALLIBLE);
        assert!(<Outer as Layout>::DECODE_FALLIBLE);
        assert!(<Run as Layout>::DECODE_FALLIBLE);
        assert!(!<Point as Layout>::DECODE_FALLIBLE);
        assert!(!<Segment as Layout>::DECODE_FALLIBLE);
    }
    let segment: Segment = Segment::decode(&[1, 2, 3, 4, 5]);
    assert_eq!((segment.from, segment.to), (Point { x: 2, y: 3 }, Point { x: 4, y: 5 }));
    let checked: Result<PsdExt, ParseError> = PsdExt::decode(&psd(0));
    assert!(checked.is_ok());
}

#[test]
fn a_check_two_layouts_down_still_runs() {
    let mut wire = [0u8; 13];
    wire[0] = 9;
    wire[1..].copy_from_slice(&psd(0));
    assert_eq!(Outer::decode(&wire).expect("admitted").ext.num_blocks, 3);
    wire[1..].copy_from_slice(&psd(0b1_0000));
    assert!(matches!(Outer::decode(&wire), Err(ParseError::OutOfRange { value: 16, .. })));
}

#[test]
fn every_element_of_a_nested_array_is_checked() {
    assert_eq!(Run::decode(&[2, flags(0), flags(0)]).expect("admitted").n, 2);
    assert!(matches!(
        Run::decode(&[2, flags(0), flags(2)]),
        Err(ParseError::OutOfRange { field: "reserved", value: 2, .. })
    ));
}

#[test]
fn a_holder_with_checks_of_its_own_runs_both() {
    assert_eq!(Sealed::decode(&[0xA5, 9, flags(0)]).expect("admitted").level, 9);
    assert!(matches!(Sealed::decode(&[0x00, 9, flags(0)]), Err(ParseError::Magic { .. })));
    assert!(matches!(
        Sealed::decode(&[0xA5, 10, flags(0)]),
        Err(ParseError::OutOfRange { field: "level", .. })
    ));
    assert!(matches!(
        Sealed::decode(&[0xA5, 9, flags(1)]),
        Err(ParseError::OutOfRange { field: "reserved", .. })
    ));
}

#[test]
fn a_message_holding_the_holder_checks_it() {
    let mut wire = vec![1];
    wire.extend_from_slice(&psd(0));
    wire.push(0xEE);
    let (packet, used) = Packet::decode(&wire).expect("admitted");
    assert_eq!((packet.ext.num_bin, packet.tail.as_slice(), used), (7, &[0xEE][..], 14));

    wire[11] = flags(3);
    assert!(matches!(
        Packet::decode(&wire),
        Err(ParseError::OutOfRange { field: "reserved", value: 3, .. })
    ));
}

#[test]
fn random_wires_decode_exactly_when_the_spare_bits_are_zero() {
    let mut r = rng::Rng(0x05D_E47);
    let mut admitted = 0;
    for i in 0..4096 {
        let mut wire = [0u8; 12];
        r.fill(&mut wire);
        if i % 2 == 0 {
            wire[10] &= 0b1110_0000;
        }
        match PsdExt::decode(&wire) {
            Ok(got) => {
                admitted += 1;
                assert_eq!(wire[10] & 0b1_1111, 0, "wire {i}: admitted nonzero spare bits");
                assert_eq!(got.encode().unwrap(), wire, "wire {i}: a round trip changed the bytes");
            }
            Err(ParseError::OutOfRange { field: "reserved", value, .. }) => {
                assert_eq!(i64::from(wire[10] & 0b1_1111), value, "wire {i}");
            }
            Err(e) => panic!("wire {i}: {e:?}"),
        }
    }
    assert!(admitted > 0, "no random wire decoded, so nothing was proven");
}
