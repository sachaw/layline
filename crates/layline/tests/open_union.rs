//! A union has the same size in bytes for a listed tag and for an unlisted tag.

#![cfg(feature = "derive")]

use layline::{Layout, Message, ParseError};

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 8, endian = be)]
pub struct Pulse {
    pub pri: u32,
    pub width: u32,
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 8, endian = be)]
pub struct Cw {
    pub freq: u64,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub enum Details {
    #[value(0)]
    #[bytes(8)]
    Pulse(Pulse),
    #[value(1)]
    #[bytes(8)]
    Cw(Cw),
    #[other]
    Unknown([u8; 8]),
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Packet {
    pub kind: u32,
    #[switch(kind)]
    #[bytes(8)]
    pub details: Details,
    pub trailer: u16,
}

fn wire(kind: u32, body: [u8; 8], trailer: u16) -> Vec<u8> {
    let mut v = kind.to_be_bytes().to_vec();
    v.extend_from_slice(&body);
    v.extend_from_slice(&trailer.to_be_bytes());
    v
}

#[test]
fn a_listed_discriminant_reads_its_arm() {
    let bytes = wire(0, [0, 0, 0, 9, 0, 0, 0, 4], 0xBEEF);
    let (p, used) = Packet::decode(&bytes).unwrap();
    assert_eq!(p.details, Details::Pulse(Pulse { pri: 9, width: 4 }));
    assert_eq!(p.trailer, 0xBEEF);
    assert_eq!(used, bytes.len());
}

#[test]
fn an_unlisted_discriminant_leaves_the_field_behind_it_readable() {
    let body = [1, 2, 3, 4, 5, 6, 7, 8];
    let bytes = wire(7, body, 0xBEEF);
    let (p, used) = Packet::decode(&bytes).unwrap();
    assert_eq!(p.details, Details::Unknown(body));
    assert_eq!(p.trailer, 0xBEEF, "the union's fixed size keeps `trailer` at its offset");
    assert_eq!(used, bytes.len());
}

#[test]
fn an_unlisted_discriminant_re_encodes_byte_for_byte() {
    for kind in [0u32, 1, 7, u32::MAX] {
        let bytes = wire(kind, [9, 8, 7, 6, 5, 4, 3, 2], 0x1234);
        let (p, _) = Packet::decode(&bytes).unwrap();
        assert_eq!(p.encode(), bytes, "kind {kind}");
    }
}

#[test]
fn a_body_too_short_for_the_footprint_is_refused() {
    let mut bytes = wire(7, [0; 8], 0);
    bytes.truncate(9);
    assert!(matches!(Packet::decode(&bytes), Err(ParseError::Short { .. })));
}
