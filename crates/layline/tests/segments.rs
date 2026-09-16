//! The `SEGMENTS` table of a `#[derive(Message)]`, every offset computed by hand from the widths.

#![cfg(feature = "derive")]

#[path = "support/text.rs"]
mod codecs;

use layline::table::{By, Discriminant, Presence, Span, Start};
#[path = "support/fixtures.rs"]
mod fixtures;

use fixtures::{Body, Ping, Pong, Switched};
use layline::{Choice, Layout, Message, table::fixed_bits};

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Solved {
    magic: u32,
    count: u16,
    spare: u8,
}

#[test]
fn a_wholly_fixed_message_is_one_solved_row_and_solves_to_its_own_width() {
    let table = <Solved as Message>::SEGMENTS;
    assert_eq!(table.len(), 1, "one run of fixed fields is one block");

    let row = table[0];
    assert_eq!(row.name, "__SolvedBlock0");
    assert_eq!(row.start, Start::At(0));
    assert_eq!(row.span, Span::Fixed(56), "4 + 2 + 1 bytes, by hand");
    assert_eq!(row.decoded_by, None);
    assert!(row.is_fixed());

    let names: Vec<_> = row.fields.iter().map(|f| f.name).collect();
    assert_eq!(names, ["magic", "count", "spare"]);
    let extents: Vec<_> = row.fields.iter().map(|f| (f.extent.start(), f.extent.end())).collect();
    assert_eq!(extents, [(0, 32), (32, 48), (48, 56)]);

    assert_eq!(fixed_bits(table), 56);
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4, endian = be)]
struct Entry {
    value: u32,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Counted {
    magic: u32,
    count: u16,
    #[count(count)]
    #[bytes(4)]
    entries: Vec<Entry>,
    crc: u16,
}

#[test]
fn a_counted_run_is_a_joint_and_nothing_after_it_has_a_position() {
    let table = <Counted as Message>::SEGMENTS;
    assert_eq!(table.len(), 3);

    assert_eq!(table[0].start, Start::At(0));
    assert_eq!(table[0].span, Span::Fixed(48), "u32 then u16");

    assert_eq!(table[1].name, "entries");
    assert_eq!(table[1].start, Start::At(48));
    assert_eq!(
        table[1].span,
        Span::Counted { by: By::field("count"), each: Some(32) },
        "32 bits per element, the stride of the run"
    );
    assert_eq!(table[1].decoded_by, Some("Entry"));
    assert!(!table[1].is_fixed());

    assert_eq!(table[2].name, "__CountedBlock1");
    assert_eq!(table[2].start, Start::After { segment: "entries", bits: 0 });
    assert_eq!(table[2].span, Span::Fixed(16));
    assert_eq!(table[2].start.bit(), None);
    assert!(!table[2].is_fixed());

    assert_eq!(fixed_bits(table), 48);
}

#[test]
fn the_joint_is_where_the_wire_stops_agreeing_with_the_grammar() {
    let one =
        Counted { magic: 0x0102_0304, count: 0, entries: vec![Entry { value: 7 }], crc: 0xBEEF };
    let two = Counted { entries: vec![Entry { value: 7 }, Entry { value: 8 }], ..one.clone() };

    assert_eq!((one.encode().len(), two.encode().len()), (12, 16));

    let solved_bytes = (fixed_bits(<Counted as Message>::SEGMENTS) / 8) as usize;
    assert_eq!(solved_bytes, 6);
    assert_eq!(&one.encode()[..4], &[0x01, 0x02, 0x03, 0x04]);
    assert_eq!(&two.encode()[..4], &[0x01, 0x02, 0x03, 0x04]);
    assert_eq!(&one.encode()[4..6], &[0, 1]);
    assert_eq!(&two.encode()[4..6], &[0, 2]);

    assert_eq!(&one.encode()[10..12], &[0xBE, 0xEF]);
    assert_eq!(&two.encode()[14..16], &[0xBE, 0xEF]);
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Flagged {
    flags: u16,
    #[when(flags & 0x0001)]
    timestamp: Option<u32>,
    #[when(flags & 0x0004)]
    #[text]
    #[until(0)]
    label: Option<String>,
    payload: u32,
}

#[test]
fn a_field_behind_a_flag_says_which_bit_and_how_far_it_runs_when_it_is_there() {
    let table = <Flagged as Message>::SEGMENTS;
    assert_eq!(table.len(), 4);

    assert_eq!(table[0].span, Span::Fixed(16));

    assert_eq!(table[1].name, "timestamp");
    assert_eq!(
        table[1].start,
        Start::At(16),
        "an optional field after a fixed block starts at a computed bit"
    );
    assert_eq!(
        (table[1].span, table[1].when),
        (Span::Fixed(32), Some(Presence::Flag { field: "flags", mask: 0x0001 }))
    );
    assert_eq!(table[1].fields.len(), 1);
    assert_eq!(table[1].fields[0].name, "timestamp");
    assert_eq!((table[1].fields[0].extent.start(), table[1].fields[0].extent.end()), (0, 32));

    assert_eq!(table[2].name, "label");
    assert_eq!(table[2].start, Start::After { segment: "timestamp", bits: 0 });
    assert_eq!(
        table[2].span,
        Span::Until { terminator: 0 },
        "a flagged span wraps the span the field has when present"
    );
    assert!(table[2].fields.is_empty(), "a segment of variable length has no field table");

    assert_eq!(table[3].start, Start::After { segment: "label", bits: 0 });
    assert_eq!(table[3].span, Span::Fixed(32));

    assert_eq!(fixed_bits(table), 16);
}

#[test]
fn a_switch_names_the_catalogue_and_the_field_that_chooses() {
    let table = <Switched as Message>::SEGMENTS;
    assert_eq!(table.len(), 3);

    assert_eq!(table[0].span, Span::Fixed(8));
    assert_eq!(table[1].name, "body");
    assert_eq!(table[1].start, Start::At(8), "the switch itself begins at a known bit");
    assert_eq!(table[1].span, Span::Chosen { on: Discriminant::Field("kind") });
    assert_eq!(table[1].decoded_by, Some("Body"));
    assert_eq!(table[2].start, Start::After { segment: "body", bits: 0 });
    assert_eq!(fixed_bits(table), 8);
}

#[test]
fn each_arm_publishes_its_own_table_from_its_own_first_bit() {
    let arms = <Body as Choice>::ARMS;
    assert_eq!(arms.len(), 2, "closed: two arms");

    let ping = arms[0];
    assert_eq!((ping.name, ping.value), ("Ping", Some(0)));
    assert_eq!(ping.segments.len(), 1);
    assert_eq!(ping.segments[0].start, Start::At(0), "an arm's numbering starts at its own bit 0");
    assert_eq!(ping.segments[0].span, Span::Fixed(16));
    assert_eq!(fixed_bits(ping.segments), 16);

    let pong = arms[1];
    assert_eq!((pong.name, pong.value), ("Pong", Some(1)));
    assert_eq!(pong.segments[0].span, Span::Fixed(32));
    assert_eq!(fixed_bits(pong.segments), 32);
}

#[test]
fn the_arm_chosen_moves_everything_behind_the_switch() {
    let ping = Switched { kind: 9, body: Body::Ping(Ping { seq: 0x0102 }), crc: 0xBEEF };
    assert_eq!(ping.encode(), vec![0x00, 0x01, 0x02, 0xBE, 0xEF]);
    let pong = Switched { kind: 9, body: Body::Pong(Pong { seq: 1 }), crc: 0xBEEF };
    assert_eq!(pong.encode(), vec![0x01, 0x00, 0x00, 0x00, 0x01, 0xBE, 0xEF]);

    assert_eq!(Switched::decode(&ping.encode()), Ok((Switched { kind: 0, ..ping }, 5)));
    assert_eq!(Switched::decode(&pong.encode()), Ok((Switched { kind: 1, ..pong }, 7)));
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
enum Open {
    #[value(0)]
    #[bytes(2)]
    Known(Ping),
    #[other]
    Unrecognised(Vec<u8>),
}

#[test]
fn the_open_arm_has_no_discriminant_and_keeps_everything() {
    let arms = <Open as Choice>::ARMS;
    assert_eq!(arms.len(), 2);
    let other = arms[1];
    assert_eq!(other.name, "Unrecognised");
    assert_eq!(other.value, None, "the `#[other]` arm has no value");
    assert_eq!(other.segments.len(), 1);
    assert_eq!(other.segments[0].span, Span::Fill { cap: None });
    assert_eq!(fixed_bits(other.segments), 0);

    assert_eq!(Open::Known(Ping { seq: 0x0102 }).encode(), vec![0x01, 0x02]);
    let kept = Open::Unrecognised(vec![0xDE, 0xAD, 0xBE]);
    assert_eq!(kept.encode(), vec![0xDE, 0xAD, 0xBE]);
    assert_eq!(Open::decode_with(7, &kept.encode()), Ok((kept, 3)));
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Tailed {
    magic: u16,
    #[fill]
    trailing: Vec<u8>,
}

#[test]
fn a_tail_fills_the_body_from_a_position_the_grammar_knows() {
    let table = <Tailed as Message>::SEGMENTS;
    assert_eq!(table.len(), 2);
    assert_eq!(table[1].name, "trailing");
    assert_eq!(
        table[1].start,
        Start::At(16),
        "the start is a computed bit, and the end is the end of the body"
    );
    assert_eq!(table[1].span, Span::Fill { cap: None });
    assert_eq!(fixed_bits(table), 16);
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Inner {
    len: u8,
    #[count(len)]
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Outer {
    kind: u8,
    #[message]
    inner: Inner,
    crc: u16,
}

#[test]
fn a_nested_message_delimits_itself_and_names_its_own_type() {
    let table = <Outer as Message>::SEGMENTS;
    assert_eq!(table.len(), 3);

    assert_eq!(table[1].name, "inner");
    assert_eq!(table[1].start, Start::At(8));
    assert_eq!(table[1].span, Span::SelfDelimiting, "a nested message sets its own length");
    assert_eq!(table[1].decoded_by, Some("Inner"), "`decoded_by` is the child's type name");
    assert_eq!(table[2].start, Start::After { segment: "inner", bits: 0 });
    assert_eq!(fixed_bits(table), 8);

    let inner = <Inner as Message>::SEGMENTS;
    assert_eq!(inner[0].span, Span::Fixed(8));
    assert_eq!(inner[1].name, "data");
    assert_eq!(inner[1].span, Span::Counted { by: By::field("len"), each: Some(8) });
    assert_eq!(fixed_bits(inner), 8);
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Padded {
    kind: u8,
    #[text(codecs::Ascii)]
    #[bytes(8)]
    name: String,
    crc: u16,
}

#[test]
fn a_fixed_run_of_text_is_solved_and_the_field_behind_it_has_a_position() {
    let table = <Padded as Message>::SEGMENTS;
    assert_eq!(table.len(), 3);
    assert_eq!(table[1].name, "name");
    assert_eq!(table[1].start, Start::At(8));
    assert_eq!(table[1].span, Span::Fixed(64), "eight bytes, whatever is written in them");
    assert!(table[1].is_fixed(), "the text length in bytes is fixed");
    assert_eq!(table[2].start, Start::At(72), "bit 72 is byte 9");
    assert_eq!(fixed_bits(table), 88, "1 + 8 + 2 bytes, all of it fixed");

    let short = Padded { kind: 1, name: "ab".into(), crc: 0xBEEF };
    assert_eq!(short.encode().len(), 11);
    assert_eq!(&short.encode()[9..], &[0xBE, 0xEF]);
    let full = Padded { kind: 1, name: "abcdefgh".into(), crc: 0xBEEF };
    assert_eq!(&full.encode()[9..], &[0xBE, 0xEF]);
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Directory {
    table_offset: u8,
    table_len: u8,
    #[count(table_len)]
    #[seek(table_offset)]
    #[bytes(4)]
    entries: Vec<Entry>,
}

#[test]
fn a_run_the_wire_places_has_no_position_only_a_field_that_carries_one() {
    let table = <Directory as Message>::SEGMENTS;
    assert_eq!(table.len(), 2);
    assert_eq!(table[0].span, Span::Fixed(16), "two offset/length octets");

    assert_eq!(table[1].name, "entries");
    assert_eq!(
        table[1].start,
        Start::Seek { by: "table_offset" },
        "the start is the value of `table_offset`"
    );
    assert_eq!(table[1].start.bit(), None);
    assert_eq!(table[1].span, Span::Counted { by: By::field("table_len"), each: Some(32) });

    assert_eq!(fixed_bits(table), 16);
}

#[test]
fn a_block_row_carries_the_layout_table_the_walk_reads_it_through() {
    let row = <Counted as Message>::SEGMENTS[0];
    // The row's `fields` is the same slice as the generated block's `FIELDS`.
    assert!(core::ptr::eq(row.fields, <__CountedBlock0 as Layout>::FIELDS));
    assert_eq!(row.fields.len(), 2);
    assert_eq!(layline::table::check_layout(row.fields, 48), Ok(()));
}

#[derive(Message, Debug, PartialEq)]
struct StatesItsPositions {
    #[at(byte = 0)]
    kind: u8,
    #[at(byte = 1)]
    len: u16,
    #[at(byte = 3)]
    flags: u8,
}

#[test]
fn a_message_field_may_state_its_position() {
    let m = StatesItsPositions { kind: 7, len: 0x0102, flags: 0xFF };
    let (back, used) = StatesItsPositions::decode(&m.encode()).expect("round trip");
    assert_eq!(back, m, "the values round-trip");
    assert_eq!(used, 4);
}
