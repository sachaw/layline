//! `#[count]`, `#[at]`, `#[when]` and `#[switch]` with a path into a nested layout.
//! `header.n_entries` resolves one field further in.

#![cfg(feature = "derive")]

use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 24)]
pub struct Header {
    #[bits(2)]
    pub kind: u8,
    #[bits(4)]
    pub n_entries: u8,
    #[bits(2)]
    pub flags: u8,
    #[bits(6)]
    pub table_offset: u8,
    #[bits(10)]
    pub spare: u16,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 24, endian = be)]
pub struct HeaderBe {
    #[bits(2)]
    pub kind: u8,
    #[bits(4)]
    pub n_entries: u8,
    #[bits(2)]
    pub flags: u8,
    #[bits(6)]
    pub table_offset: u8,
    #[bits(10)]
    pub spare: u16,
}

/// Little-endian: a nested layout has its own byte order, and `#[message(endian = be)]` applies
/// only to the parent's own scalars.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Entry {
    pub tag: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct PingBody {
    pub seq: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
pub enum Body {
    #[value(0)]
    Ping(PingBody),
    #[value(1)]
    Level(u8),
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Packet {
    #[bytes(3)]
    pub header: Header,
    #[when(header.flags & 0x01)]
    pub stamp: Option<u16>,
    #[switch(header.kind)]
    pub body: Body,
    #[count(header.n_entries)]
    #[seek(header.table_offset)]
    #[bytes(2)]
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
pub struct PacketBe {
    #[bytes(3)]
    pub header: HeaderBe,
    #[when(header.flags & 0x01)]
    pub stamp: Option<u16>,
    #[switch(header.kind)]
    pub body: Body,
    #[count(header.n_entries)]
    #[seek(header.table_offset)]
    #[bytes(2)]
    pub entries: Vec<Entry>,
}

#[test]
fn a_packed_header_drives_a_count_an_offset_a_flag_and_a_switch() {
    let p = Packet {
        header: Header { kind: 0, n_entries: 15, flags: 0b10, table_offset: 63, spare: 0x2AA },
        stamp: Some(0xBEEF),
        body: Body::Level(200),
        entries: vec![Entry { tag: 0x1122 }, Entry { tag: 0x3344 }],
    };

    #[rustfmt::skip]
    let wire = vec![
        0xC9, 0x86, 0xAA,
        0xEF, 0xBE,
        0xC8,
        0x22, 0x11,
        0x44, 0x33,
    ];
    assert_eq!(p.encode(), wire);

    let (back, used) = Packet::decode(&wire).expect("parses");
    assert_eq!(used, wire.len());
    assert_eq!(back.header.kind, 1, "the discriminant on the wire");
    assert_eq!(back.header.n_entries, 2, "the count on the wire");
    assert_eq!(
        back.header.flags, 0b11,
        "encode set the presence bit and kept the writer's other bit"
    );
    assert_eq!(back.header.table_offset, 6, "the byte offset of the table on the wire");
    assert_eq!(back.header.spare, 0x2AA);
    assert_eq!(back.stamp, Some(0xBEEF));
    assert_eq!(back.body, Body::Level(200));
    assert_eq!(back.entries, p.entries);
}

#[test]
fn the_same_header_selects_a_different_arm_and_no_optional_field() {
    let p = Packet {
        header: Header { kind: 3, n_entries: 0, flags: 0b01, table_offset: 0, spare: 0 },
        stamp: None,
        body: Body::Ping(PingBody { seq: 0x0102 }),
        entries: vec![Entry { tag: 0x00FF }],
    };

    #[rustfmt::skip]
    let wire = vec![
        0x04, 0x05, 0x00,
        0x02, 0x01,
        0xFF, 0x00,
    ];
    assert_eq!(p.encode(), wire);

    let (back, used) = Packet::decode(&wire).expect("parses");
    assert_eq!(used, wire.len());
    assert_eq!(back.header.flags, 0, "encode clears the bit for `None`");
    assert_eq!(back.stamp, None);
    assert_eq!(back.body, Body::Ping(PingBody { seq: 0x0102 }));
    assert_eq!(back.entries, p.entries);
}

#[test]
fn the_stamp_follows_the_child_s_byte_order_and_not_the_walk_s() {
    let p = PacketBe {
        header: HeaderBe { kind: 0, n_entries: 15, flags: 0b10, table_offset: 63, spare: 0x2AA },
        stamp: Some(0xBEEF),
        body: Body::Level(200),
        entries: vec![Entry { tag: 0x1122 }, Entry { tag: 0x3344 }],
    };

    #[rustfmt::skip]
    let wire = vec![
        0xAA, 0x86, 0xC9,
        0xBE, 0xEF,
        0xC8,
        0x22, 0x11,
        0x44, 0x33,
    ];
    assert_eq!(p.encode(), wire);

    let (back, used) = PacketBe::decode(&wire).expect("parses");
    assert_eq!(used, wire.len());
    assert_eq!(back.header.n_entries, 2);
    assert_eq!(back.header.table_offset, 6);
    assert_eq!(back.header.spare, 0x2AA);
    assert_eq!(back.stamp, Some(0xBEEF));
    assert_eq!(back.entries, p.entries);
}

#[test]
fn a_stamp_through_a_child_leaves_its_neighbours_alone() {
    let p = Packet {
        header: Header { kind: 3, n_entries: 15, flags: 0b11, table_offset: 63, spare: 0x3FF },
        stamp: Some(0xFFFF),
        body: Body::Level(0xFF),
        entries: vec![Entry { tag: 0xFFFF }, Entry { tag: 0xFFFF }],
    };

    let bytes = p.encode();
    assert_eq!(&bytes[..3], &[0xC9, 0xC6, 0xFF]);

    let (back, _) = Packet::decode(&bytes).expect("parses");
    assert_eq!(back.header.spare, 0x3FF, "the ten spare bits keep the writer's value");
    assert_eq!(back.header.flags, 0b11, "the ungoverned flag bit keeps the writer's value");
    assert_eq!(back.header.table_offset, 6);
    assert_eq!(back.header.n_entries, 2);
}

#[test]
#[should_panic(
    expected = "field `header.n_entries` is too narrow for the length of a collection the wire derives it from"
)]
fn a_count_too_large_for_a_nested_field_is_refused() {
    let ok = Packet {
        header: Header { kind: 0, n_entries: 0, flags: 0, table_offset: 0, spare: 0 },
        stamp: None,
        body: Body::Level(0),
        entries: vec![Entry { tag: 0 }; 15],
    };
    assert_eq!(ok.encode()[0] >> 2 & 0xF, 15, "15 is the largest count four bits contain");

    let _ = Packet { entries: vec![Entry { tag: 0 }; 16], ..ok }.encode();
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
pub struct TinyHeader {
    #[bits(4)]
    pub n: u8,
    #[bits(4)]
    pub at: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Narrow {
    #[bytes(1)]
    pub head: TinyHeader,
    pub pad_len: u8,
    #[count(pad_len)]
    pub pad: Vec<u8>,
    #[count(head.n)]
    #[seek(head.at)]
    pub table: Vec<u8>,
}

#[test]
#[should_panic(
    expected = "field `head.at` is too narrow for the position of a collection the wire derives it from"
)]
fn an_offset_too_large_for_a_nested_field_is_refused() {
    let ok = Narrow {
        head: TinyHeader { n: 0, at: 0 },
        pad_len: 0,
        pad: vec![0; 13],
        table: vec![1, 2],
    };
    assert_eq!(ok.encode()[0] >> 4, 15);
    assert_eq!(Narrow::decode(&ok.encode()).expect("parses").0.table, vec![1, 2]);

    let _ = Narrow { pad: vec![0; 14], ..ok }.encode();
}
