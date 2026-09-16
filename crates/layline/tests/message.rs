//! `#[derive(Message)]`: every attribute the derive accepts, on the wire, against bytes written out by hand.

#![cfg(feature = "derive")]

use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4)]
pub struct Item {
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Entry {
    pub tag: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Packet {
    pub magic: u32,
    pub count: u16,
    pub flags: u16,
    #[count(count)]
    #[bytes(4)]
    pub items: Vec<Item>,
    #[fill]
    pub trailing: Vec<u8>,
}

#[test]
fn block_repeat_tail_round_trips() {
    let p = Packet {
        magic: 0xDEAD_BEEF,
        count: 2,
        flags: 0x0102,
        items: vec![Item { value: 1 }, Item { value: 0x0A0B_0C0D }],
        trailing: vec![0xAA, 0xBB, 0xCC],
    };

    let bytes = p.encode();
    #[rustfmt::skip]
    assert_eq!(bytes, vec![
        0xEF, 0xBE, 0xAD, 0xDE,
        0x02, 0x00,
        0x02, 0x01,
        0x01, 0x00, 0x00, 0x00,
        0x0D, 0x0C, 0x0B, 0x0A,
        0xAA, 0xBB, 0xCC,
    ]);

    let (back, used) = Packet::decode(&bytes).expect("parses");
    assert_eq!(used, bytes.len());
    assert_eq!(back, p);

    let (empty, used) = Packet::decode(&bytes[..16]).expect("parses");
    assert!(empty.trailing.is_empty());
    assert_eq!(used, 16);
}

#[test]
fn parse_reports_the_bytes_it_consumed() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Counted {
        n: u8,
        #[count(n)]
        #[bytes(4)]
        items: Vec<Item>,
    }

    let mut buf = Counted { n: 0, items: vec![Item { value: 7 }] }.encode();
    buf.extend_from_slice(b"the next message");
    let (back, used) = Counted::decode(&buf).expect("parses");
    assert_eq!(used, 5);
    assert_eq!(back.items, vec![Item { value: 7 }]);
}

#[test]
fn a_wrong_count_field_does_not_reach_the_wire() {
    let lying = Packet {
        magic: 0,
        count: 999,
        flags: 0,
        items: vec![Item { value: 1 }, Item { value: 2 }, Item { value: 3 }],
        trailing: vec![],
    };

    let bytes = lying.encode();
    assert_eq!(&bytes[4..6], &[3, 0], "the wire contains the collection's length");

    let (back, _) = Packet::decode(&bytes).expect("parses");
    assert_eq!(back.count, 2 + 1, "decode reads the recomputed count");
    assert_eq!(back.items.len(), 3);
}

#[test]
fn a_scaled_count_is_back_patched_through_its_affine_form() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Scaled {
        pairs: u16,
        #[count(pairs, scale = 2, offset = -1)]
        #[bytes(4)]
        items: Vec<Item>,
    }

    let m = Scaled { pairs: 7, items: (0..3).map(|value| Item { value }).collect() };
    let bytes = m.encode();
    assert_eq!(&bytes[..2], &[2, 0], "(3 - -1) / 2 == 2");

    let (back, used) = Scaled::decode(&bytes).expect("parses");
    assert_eq!(used, bytes.len());
    assert_eq!(back.pairs, 2);
    assert_eq!(back.items.len(), 3, "2 * 2 - 1");
}

#[test]
fn an_offset_directory_stamps_and_seeks() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Directory {
        table_offset: u32,
        table_len: u32,
        #[count(table_len)]
        #[seek(table_offset)]
        #[bytes(2)]
        entries: Vec<Entry>,
    }

    let d = Directory {
        table_offset: 0xFFFF,
        table_len: 0,
        entries: vec![Entry { tag: 0x1122 }, Entry { tag: 0x3344 }],
    };

    let bytes = d.encode();
    assert_eq!(&bytes[0..4], &[8, 0, 0, 0], "the table starts at byte 8, right after the header");
    assert_eq!(&bytes[4..8], &[2, 0, 0, 0], "the count is 2");

    let (back, used) = Directory::decode(&bytes).expect("parses");
    assert_eq!(used, 12);
    assert_eq!(back.table_offset, 8);
    assert_eq!(back.entries, d.entries);

    let mut bad = bytes.clone();
    bad[0] = 0xF0;
    assert!(matches!(Directory::decode(&bad), Err(layline::ParseError::Short { .. })));

    #[derive(Debug, Clone, PartialEq, Message)]
    #[message(endian = be)]
    struct BigEndian {
        table_offset: u32,
        table_len: u16,
        #[count(table_len)]
        #[seek(table_offset)]
        #[bytes(2)]
        entries: Vec<Entry>,
    }

    let b = BigEndian { table_offset: 0, table_len: 0, entries: vec![Entry { tag: 0x1122 }] };
    let bytes = b.encode();
    // `Entry` has its own byte order; only the header is big-endian.
    assert_eq!(bytes, vec![0, 0, 0, 6, 0, 1, 0x22, 0x11]);
    assert_eq!(BigEndian::decode(&bytes).unwrap().0.table_offset, 6);
}

#[test]
fn an_offset_is_measured_not_predicted() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Directory {
        table_offset: u32,
        table_len: u32,
        #[text]
        #[until(0)]
        name: String,
        #[count(table_len)]
        #[seek(table_offset)]
        #[bytes(2)]
        entries: Vec<Entry>,
    }

    let d = Directory {
        table_offset: 0xFFFF,
        table_len: 0,
        name: String::from("cmap"),
        entries: vec![Entry { tag: 0x1122 }, Entry { tag: 0x3344 }],
    };

    let bytes = d.encode();
    assert_eq!(&bytes[8..13], b"cmap\0", "the string is between the header and the table");
    assert_eq!(&bytes[0..4], &[13, 0, 0, 0], "the table offset is 13, right after the string");
    assert_eq!(&bytes[4..8], &[2, 0, 0, 0]);
    assert_eq!(bytes.len(), 17);

    let (back, used) = Directory::decode(&bytes).expect("parses");
    assert_eq!(used, 17);
    assert_eq!(back.table_offset, 13);
    assert_eq!(back.name, "cmap");
    assert_eq!(back.entries, d.entries);

    let longer = Directory { name: String::from("a much longer name"), ..d };
    let bytes = longer.encode();
    assert_eq!(&bytes[0..4], &[27, 0, 0, 0]);
    assert_eq!(Directory::decode(&bytes).unwrap().0.entries, longer.entries);
}

#[test]
fn count_by_field_caps_a_hostile_length() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Capped {
        n: u16,
        #[count(n, cap = 4)]
        #[bytes(4)]
        items: Vec<Item>,
    }

    let ok = Capped { n: 0, items: vec![Item { value: 1 }] }.encode();
    assert_eq!(Capped::decode(&ok).unwrap().0.items.len(), 1);

    let hostile = vec![5, 0];
    assert_eq!(Capped::decode(&hostile), Err(layline::ParseError::Malformed { field: "n", at: 2 }),);
}

#[test]
fn fill_takes_as_many_whole_elements_as_the_body_holds() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Filled {
        kind: u8,
        #[fill]
        #[bytes(2)]
        entries: Vec<Entry>,
    }

    let m = Filled { kind: 9, entries: vec![Entry { tag: 1 }, Entry { tag: 2 }, Entry { tag: 3 }] };
    let bytes = m.encode();
    assert_eq!(bytes.len(), 1 + 3 * 2);
    assert_eq!(Filled::decode(&bytes).unwrap().0, m);

    let mut ragged = bytes.clone();
    ragged.push(0);
    assert_eq!(
        Filled::decode(&ragged),
        Err(layline::ParseError::Malformed { field: "entries", at: 1 }),
    );
}

#[test]
fn an_uncapped_rest_takes_everything() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct All {
        kind: u8,
        #[fill]
        body: Vec<u8>,
    }
    let (m, used) = All::decode(&[7, 1, 2, 3]).expect("parses");
    assert_eq!(m.body, vec![1, 2, 3]);
    assert_eq!(used, 4);
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2, endian = be)]
pub struct BeEntry {
    pub tag: u16,
}

#[test]
fn endian_be_applies_to_blocks_and_to_elements() {
    #[derive(Debug, Clone, PartialEq, Message)]
    #[message(endian = be)]
    struct BigEndian {
        magic: u32,
        n: u16,
        #[count(n)]
        #[bytes(2)]
        entries: Vec<BeEntry>,
        #[count(n)]
        raw: Vec<u16>,
    }

    let m = BigEndian {
        magic: 0x0102_0304,
        n: 0,
        entries: vec![BeEntry { tag: 0xAABB }],
        raw: vec![0xCCDD],
    };
    let bytes = m.encode();
    #[rustfmt::skip]
    assert_eq!(bytes, vec![
        0x01, 0x02, 0x03, 0x04,
        0x00, 0x01,
        0xAA, 0xBB,
        0xCC, 0xDD,
    ]);
    assert_eq!(BigEndian::decode(&bytes).unwrap().0, BigEndian { n: 1, ..m });
}

#[test]
fn a_block_carries_codecs_arrays_and_nested_layouts() {
    #[derive(Debug, Clone, Copy, PartialEq, layline::FieldCodec)]
    #[bits(8)]
    pub enum Kind {
        #[value(0)]
        Idle,
        #[value(1)]
        Active,
        #[other]
        Other(u8),
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Rich {
        #[codec(8)]
        kind: Kind,
        n: u8,
        reserved: [u8; 2],
        #[bytes(4)]
        stamp: Item,
        #[count(n)]
        values: Vec<u8>,
    }

    let m = Rich {
        kind: Kind::Other(9),
        n: 0,
        reserved: [0xEE, 0xFF],
        stamp: Item { value: 0x1234_5678 },
        values: vec![1, 2],
    };
    let bytes = m.encode();
    #[rustfmt::skip]
    assert_eq!(bytes, vec![
        9,
        2,
        0xEE, 0xFF,
        0x78, 0x56, 0x34, 0x12,
        1, 2,
    ]);
    assert_eq!(Rich::decode(&bytes).unwrap().0, Rich { n: 2, ..m });
}

#[test]
fn a_collection_splits_the_blocks_around_it() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct TwoBlocks {
        a: u8,
        #[count(a)]
        first: Vec<u8>,
        b: u16,
        #[count(b)]
        #[bytes(2)]
        second: Vec<Entry>,
    }

    let m = TwoBlocks { a: 0, first: vec![1, 2, 3], b: 0, second: vec![Entry { tag: 0x0A0B }] };
    let bytes = m.encode();
    assert_eq!(bytes, vec![3, 1, 2, 3, 1, 0, 0x0B, 0x0A]);
    assert_eq!(TwoBlocks::decode(&bytes).unwrap().0, TwoBlocks { a: 3, b: 1, ..m });
}

#[test]
fn a_truncated_body_is_an_error_naming_what_was_needed() {
    assert_eq!(
        Packet::decode(&[0, 0, 0]),
        Err(layline::ParseError::Short { need_bytes: 8, got_bytes: 3, at: 0 }),
    );
    let mut short = Packet {
        magic: 0,
        count: 0,
        flags: 0,
        items: vec![Item { value: 1 }, Item { value: 2 }],
        trailing: vec![],
    }
    .encode();
    short.truncate(12);
    assert_eq!(
        Packet::decode(&short),
        Err(layline::ParseError::Short { need_bytes: 16, got_bytes: 12, at: 12 }),
    );
}
