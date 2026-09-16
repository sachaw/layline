//! `#[seek(field)]` on one record: an offset directory whose target is a struct.
//! Every wire here is written out by hand.

#![cfg(feature = "derive")]

use layline::table::{Presence, Span, Start};
use layline::{Layout, Message};

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 4)]
struct Table {
    first: u16,
    count: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Symb {
    n: u8,
    #[count(n)]
    names: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Container {
    magic: u32,
    table_off: u16,
    symb_off: u16,
    #[seek(table_off)]
    #[bytes(4)]
    table: Table,
    #[seek(symb_off)]
    #[message]
    symb: Symb,
}

fn container() -> Container {
    Container {
        magic: 0x4B43_4150,
        table_off: 0xDEAD,
        symb_off: 0xBEEF,
        table: Table { first: 0x0201, count: 3 },
        symb: Symb { n: 2, names: vec![0x41, 0x42] },
    }
}

const CONTAINER_WIRE: &[u8] =
    &[0x50, 0x41, 0x43, 0x4B, 8, 0, 12, 0, 0x01, 0x02, 0x03, 0x00, 0x02, 0x41, 0x42];

#[test]
fn a_record_lands_where_the_walk_packed_it_and_the_offset_is_stamped_from_that() {
    assert_eq!(container().encode(), CONTAINER_WIRE.to_vec());

    let (back, used) = Container::decode(CONTAINER_WIRE).expect("parses");
    assert_eq!(used, CONTAINER_WIRE.len());
    assert_eq!((back.table_off, back.symb_off), (8, 12));
    assert_eq!(back.table, Table { first: 0x0201, count: 3 });
    assert_eq!(back.symb, Symb { n: 2, names: vec![0x41, 0x42] });

    // The field type is one record.
    let _: Table = back.table;
    let _: Symb = back.symb;
}

#[test]
fn a_reader_honours_an_offset_that_points_backwards() {
    let out_of_order: &[u8] =
        &[0x50, 0x41, 0x43, 0x4B, 11, 0, 8, 0, 0x02, 0x41, 0x42, 0x01, 0x02, 0x03, 0x00];
    let (back, used) = Container::decode(out_of_order).expect("parses");
    assert_eq!(back.table, Table { first: 0x0201, count: 3 });
    assert_eq!(back.symb, Symb { n: 2, names: vec![0x41, 0x42] });
    assert_eq!(used, 15);

    assert_eq!(back.encode(), CONTAINER_WIRE.to_vec());
}

#[test]
fn an_offset_past_the_end_is_refused() {
    let mut bad = CONTAINER_WIRE.to_vec();
    bad[4] = 0xF0;
    assert!(matches!(Container::decode(&bad), Err(layline::ParseError::Short { .. })));

    let mut short = CONTAINER_WIRE.to_vec();
    short[4] = 13;
    assert!(matches!(Container::decode(&short), Err(layline::ParseError::Short { .. })));
}

#[test]
fn the_row_says_one_record_at_a_stated_offset() {
    let table = <Container as Message>::SEGMENTS;
    assert_eq!(table.len(), 3, "the header block, then a row per placed record");

    assert_eq!(table[0].span, Span::Fixed(64), "magic + two offsets");

    assert_eq!(table[1].name, "table");
    assert_eq!(table[1].start, Start::Seek { by: "table_off" });
    assert_eq!(table[1].start.bit(), None, "a seek start has no bit number");
    assert_eq!(table[1].span, Span::Fixed(32), "the record's own width");
    assert_eq!(table[1].decoded_by, Some("Table"));

    assert_eq!(table[2].name, "symb");
    assert_eq!(table[2].start, Start::Seek { by: "symb_off" });
    assert_eq!(table[2].span, Span::SelfDelimiting, "a nested message sets its own length");
    assert_eq!(table[2].decoded_by, Some("Symb"));

    assert_eq!(layline::table::fixed_bits(table), 64);
    assert!(!table[1].is_fixed());
    assert!(!table[2].is_fixed());
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Optional {
    table_off: u8,
    #[seek(table_off)]
    #[bytes(4)]
    table: Option<Table>,
    trailer: u8,
}

#[test]
fn an_absent_record_is_an_offset_of_zero() {
    let present =
        Optional { table_off: 0xFF, table: Some(Table { first: 0x0201, count: 3 }), trailer: 0x7F };
    assert_eq!(present.encode(), vec![1, 0x01, 0x02, 0x03, 0x00, 0x7F]);

    let absent = Optional { table_off: 0xFF, table: None, trailer: 0x7F };
    assert_eq!(absent.encode(), vec![0, 0x7F]);

    for m in [present.clone(), absent.clone()] {
        let bytes = m.encode();
        let (back, used) = Optional::decode(&bytes).expect("parses");
        assert_eq!(used, bytes.len());
        assert_eq!((back.table, back.trailer), (m.table, m.trailer));
        assert_eq!(back.table_off, if m.table.is_some() { 1 } else { 0 });
    }
}

#[test]
fn an_optional_record_publishes_the_condition_on_its_span() {
    let table = <Optional as Message>::SEGMENTS;
    assert_eq!(table[1].name, "table");
    assert_eq!(table[1].start, Start::Seek { by: "table_off" });
    assert_eq!((table[1].span, table[1].when), (Span::Fixed(32), Some(Presence::Offset)));

    assert_eq!(
        <Optional as layline::Message>::audit().to_string(),
        "\
M Optional
G Optional __OptionalBlock0 at,0 fixed,8 - -
F __OptionalBlock0 0 8 table_off
V __OptionalBlock0 ok
G Optional table seek,table_off fixed,32 offset Table
G Optional __OptionalBlock1 after,table,0 fixed,8 - -
F __OptionalBlock1 0 8 trailer
V __OptionalBlock1 ok
E Optional 8
"
    );
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Block {
    magic: u32,
    n: u8,
    #[count(n)]
    body: Vec<u8>,
}

/// `file_size` is a plain field, so encode writes the struct's value unchanged.
#[derive(Debug, Clone, PartialEq, Message)]
struct Container2 {
    magic: u32,
    byte_order: u16,
    version: u16,
    file_size: u32,
    header_size: u16,
    block_count: u16,
    symb_offset: u32,
    symb_size: u32,
    info_offset: u32,
    info_size: u32,
    table_offset: u32,
    table_size: u32,
    #[seek(symb_offset)]
    #[when(symb_size > 0)]
    #[message]
    symb: Option<Block>,
    #[seek(info_offset)]
    #[when(info_size > 0)]
    #[message]
    info: Option<Block>,
    #[seek(table_offset)]
    #[when(table_size > 0)]
    #[message]
    table: Option<Block>,
}

const SYMB: u32 = 0x424D_5953;
const INFO: u32 = 0x4F46_4E49;
const TABL: u32 = 0x4C42_4154;

fn symb() -> Block {
    Block { magic: SYMB, n: 2, body: vec![0x41, 0x42] }
}
fn info() -> Block {
    Block { magic: INFO, n: 3, body: vec![0x61, 0x62, 0x63] }
}
fn table_block() -> Block {
    Block { magic: TABL, n: 4, body: vec![1, 2, 3, 4] }
}

fn container2(
    symb: Option<Block>,
    info: Option<Block>,
    table: Option<Block>,
    file_size: u32,
) -> Container2 {
    Container2 {
        magic: 0x4B43_4150,
        byte_order: 0xFEFF,
        version: 0x0100,
        file_size,
        header_size: 40,
        block_count: 3,
        symb_offset: 0xDEAD,
        symb_size: 0xDEAD,
        info_offset: 0xBEEF,
        info_size: 0xBEEF,
        table_offset: 0xF00D,
        table_size: 0xF00D,
        symb,
        info,
        table,
    }
}

const CONTAINER2_ALL: &[u8] = &[
    0x50, 0x41, 0x43, 0x4B, 0xFF, 0xFE, 0x00, 0x01, 0x40, 0x00, 0x00, 0x00, 0x28, 0x00, 0x03, 0x00,
    0x28, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x2F, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
    0x37, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x53, 0x59, 0x4D, 0x42, 0x02, 0x41, 0x42, 0x49,
    0x4E, 0x46, 0x4F, 0x03, 0x61, 0x62, 0x63, 0x54, 0x41, 0x42, 0x4C, 0x04, 0x01, 0x02, 0x03, 0x04,
];

const CONTAINER2_NONE: &[u8] = &[
    0x50, 0x41, 0x43, 0x4B, 0xFF, 0xFE, 0x00, 0x01, 0x28, 0x00, 0x00, 0x00, 0x28, 0x00, 0x03, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

const CONTAINER2_MIXED: &[u8] = &[
    0x50, 0x41, 0x43, 0x4B, 0xFF, 0xFE, 0x00, 0x01, 0x39, 0x00, 0x00, 0x00, 0x28, 0x00, 0x03, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x28, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
    0x30, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x49, 0x4E, 0x46, 0x4F, 0x03, 0x61, 0x62, 0x63,
    0x54, 0x41, 0x42, 0x4C, 0x04, 0x01, 0x02, 0x03, 0x04,
];

#[test]
fn a_section_placed_by_one_derived_field_and_gated_by_another() {
    let all = container2(Some(symb()), Some(info()), Some(table_block()), 64);
    assert_eq!(all.encode(), CONTAINER2_ALL.to_vec(), "the hand-computed wire");

    let none = container2(None, None, None, 40);
    assert_eq!(none.encode(), CONTAINER2_NONE.to_vec());

    let mixed = container2(None, Some(info()), Some(table_block()), 57);
    assert_eq!(mixed.encode(), CONTAINER2_MIXED.to_vec());

    for (wire, m) in [(CONTAINER2_ALL, &all), (CONTAINER2_NONE, &none), (CONTAINER2_MIXED, &mixed)]
    {
        let (back, used) = Container2::decode(wire).expect("parses");
        assert_eq!(used, wire.len(), "the cursor ends past the last section read");
        assert_eq!((&back.symb, &back.info, &back.table), (&m.symb, &m.info, &m.table));
        assert_eq!(
            (back.symb_offset, back.symb_size),
            if m.symb.is_some() { (40, 7) } else { (0, 0) }
        );
        assert_eq!(
            (back.info_offset, back.info_size),
            if m.info.is_some() { (if m.symb.is_some() { 47 } else { 40 }, 8) } else { (0, 0) }
        );
        assert_eq!(
            (back.table_offset, back.table_size),
            if m.table.is_some() { (if m.symb.is_some() { 55 } else { 48 }, 9) } else { (0, 0) }
        );
        assert_eq!((back.magic, back.byte_order, back.version), (0x4B43_4150, 0xFEFF, 0x0100));
        assert_eq!((back.header_size, back.block_count), (40, 3));
        assert_eq!(back.encode(), wire.to_vec(), "encode returns the same bytes");
    }
}

#[test]
fn a_stale_offset_beside_a_zero_size_reads_as_absent() {
    let mut stale = CONTAINER2_NONE.to_vec();
    stale[32] = 40;
    stale.extend_from_slice(&[0xFF; 8]);

    let (back, used) = Container2::decode(&stale).expect("parses");
    assert_eq!(back.table, None, "a zero size gates the record as absent");
    assert_eq!(used, 40, "nothing was read at the stale offset");
    assert_eq!(back.encode(), CONTAINER2_NONE.to_vec());
}

#[test]
fn a_writers_size_is_replaced_by_the_measurement_whichever_way_it_is_wrong() {
    let lying = Container2 { table_size: 9, table: None, ..container2(None, None, None, 40) };
    assert_eq!(lying.encode(), CONTAINER2_NONE.to_vec());
    assert_eq!(
        &lying.encode()[36..40],
        &[0, 0, 0, 0],
        "encode writes 0 to `table_size` for an absent record"
    );

    let modest = Container2 {
        table_size: 0,
        symb_size: 0,
        info_size: 0,
        ..container2(Some(symb()), Some(info()), Some(table_block()), 64)
    };
    assert_eq!(modest.encode(), CONTAINER2_ALL.to_vec());
    assert_eq!(
        &modest.encode()[36..40],
        &[9, 0, 0, 0],
        "encode writes the measured 9 to `table_size`"
    );
}

#[test]
fn the_two_gated_rows_differ_by_the_field_the_gate_is_in() {
    let table = <Container2 as Message>::SEGMENTS;
    assert_eq!(table.len(), 4, "the header block, then a row per section");

    assert_eq!(table[3].name, "table");
    assert_eq!(table[3].start, Start::Seek { by: "table_offset" });
    assert_eq!(table[3].span, Span::SelfDelimiting, "the row names the length field");

    assert_eq!(
        (<Optional as Message>::SEGMENTS[1].span, <Optional as Message>::SEGMENTS[1].when),
        (Span::Fixed(32), Some(Presence::Offset)),
    );
}

#[test]
fn the_artifact_says_which_field_decides_presence() {
    assert_eq!(
        <Container2 as layline::Message>::audit().to_string(),
        "\
M Container2
G Container2 __Container2Block0 at,0 fixed,320 - -
F __Container2Block0 0 32 magic
F __Container2Block0 32 16 byte_order
F __Container2Block0 48 16 version
F __Container2Block0 64 32 file_size
F __Container2Block0 96 16 header_size
F __Container2Block0 112 16 block_count
F __Container2Block0 128 32 symb_offset
F __Container2Block0 160 32 symb_size
F __Container2Block0 192 32 info_offset
F __Container2Block0 224 32 info_size
F __Container2Block0 256 32 table_offset
F __Container2Block0 288 32 table_size
V __Container2Block0 ok
G Container2 symb seek,symb_offset selfdelimiting length,symb_size Block
G Container2 info seek,info_offset selfdelimiting length,info_size Block
G Container2 table seek,table_offset selfdelimiting length,table_size Block
E Container2 320
"
    );

    let offset_gated = <Optional as layline::Message>::audit().to_string();
    assert!(offset_gated.contains("table seek,table_off fixed,32 offset Table"));
    assert!(!offset_gated.contains("length,"), "presence comes from the offset, not a length");
}

#[derive(Debug, Clone, PartialEq, Message)]
struct NarrowSize {
    body_offset: u16,
    body_size: u8,
    #[seek(body_offset)]
    #[when(body_size > 0)]
    #[message]
    body: Option<Block>,
}

#[test]
#[should_panic(
    expected = "field `body_size` is too narrow for the byte length of a record the wire derives it from"
)]
fn a_size_field_too_narrow_for_the_length_is_caught_where_it_is_stamped() {
    let ok = NarrowSize {
        body_offset: 0,
        body_size: 0,
        body: Some(Block { magic: TABL, n: 250, body: vec![0; 250] }),
    };
    let wire = ok.encode();
    assert_eq!(wire[2], 255, "the measured length, in the field that gates the section");
    assert_eq!(NarrowSize::decode(&wire).expect("parses").0.body, ok.body);

    let _ =
        NarrowSize { body: Some(Block { magic: TABL, n: 251, body: vec![0; 251] }), ..ok }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Weightless {
    #[fill]
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct MayWeighNothing {
    body_offset: u16,
    body_size: u16,
    #[seek(body_offset)]
    #[when(body_size > 0)]
    #[message]
    body: Option<Weightless>,
}

#[test]
#[should_panic(
    expected = "record `body` is present and measured zero bytes, and length field `body_size`"
)]
fn a_present_record_that_measures_nothing_is_caught_where_it_is_stamped() {
    let ok = MayWeighNothing {
        body_offset: 0,
        body_size: 0,
        body: Some(Weightless { data: vec![7, 8] }),
    };
    assert_eq!(ok.encode(), vec![4, 0, 2, 0, 7, 8], "offset 4, size 2, both measured");

    let _ = MayWeighNothing { body: Some(Weightless { data: vec![] }), ..ok }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
struct TooNarrow {
    table_off: u8,
    pad_len: u8,
    #[count(pad_len)]
    pad: Vec<u8>,
    #[seek(table_off)]
    #[bytes(4)]
    table: Table,
}

#[test]
#[should_panic(
    expected = "field `table_off` is too narrow for the position of a record the wire derives it from"
)]
fn an_offset_field_too_narrow_for_the_position_is_caught_where_it_is_stamped() {
    let ok = TooNarrow {
        table_off: 0,
        pad_len: 0,
        pad: vec![0; 253],
        table: Table { first: 1, count: 2 },
    };
    assert_eq!(ok.encode()[0], 255);
    assert_eq!(TooNarrow::decode(&ok.encode()).expect("parses").0.table, ok.table);

    let _ = TooNarrow { pad: vec![0; 254], ..ok }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Inner {
    off: u8,
    #[seek(off)]
    #[bytes(4)]
    table: Table,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Outer {
    lead: u16,
    #[message]
    inner: Inner,
}

#[test]
fn an_offset_inside_a_nested_message_counts_from_that_messages_own_body() {
    let m = Outer {
        lead: 0xABCD,
        inner: Inner { off: 0xFF, table: Table { first: 0x0201, count: 3 } },
    };
    assert_eq!(m.encode(), vec![0xCD, 0xAB, 1, 0x01, 0x02, 0x03, 0x00]);
    let (back, used) = Outer::decode(&m.encode()).expect("parses");
    assert_eq!(used, 7);
    assert_eq!(back.inner.off, 1);
    assert_eq!(back.inner.table, m.inner.table);
}
