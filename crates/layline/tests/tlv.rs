//! Tag / length / value: `#[switch(tag)] #[len(len)]` decodes the arm from exactly `len` bytes.
//! The `#[other]` arm keeps the bytes, and decode advances by `len` for every arm.

#![cfg(feature = "derive")]

use layline::table::{By, Discriminant, Span};
use layline::{Layout, Message, ParseError};
use layline_core::num::Uleb128;

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4)]
struct Point {
    x: u16,
    y: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Label {
    n: u8,
    #[text]
    #[len(n)]
    text: String,
}

#[derive(Debug, Clone, PartialEq, Message)]
enum Value {
    #[value(1)]
    #[bytes(4)]
    Point(Point),
    #[value(2)]
    Label(Label),
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Entry {
    #[var]
    tag: Uleb128,
    #[var]
    len: Uleb128,
    #[switch(tag)]
    #[len(len)]
    value: Value,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct List {
    version: u8,
    #[fill]
    #[message]
    entries: Vec<Entry>,
}

fn entry(tag: u64, value: Value) -> Entry {
    Entry { tag: Uleb128(tag), len: Uleb128(999), value }
}

fn label(s: &str) -> Value {
    Value::Label(Label { n: s.len() as u8, text: String::from(s) })
}

#[test]
fn every_arm_round_trips() {
    let cases: Vec<(Entry, Vec<u8>)> = vec![
        (
            entry(1, Value::Point(Point { x: 0x0102, y: 0x0304 })),
            vec![0x01, 0x04, 0x02, 0x01, 0x04, 0x03],
        ),
        (entry(2, label("hi")), vec![0x02, 0x03, 0x02, b'h', b'i']),
        (entry(9, Value::Unknown(vec![0xAA, 0xBB])), vec![0x09, 0x02, 0xAA, 0xBB]),
        (entry(7, Value::Unknown(Vec::new())), vec![0x07, 0x00]),
    ];
    for (e, wire) in cases {
        assert_eq!(e.encode(), wire, "{e:?} encodes");
        let (back, used) = Entry::decode(&wire).expect("parses");
        assert_eq!(used, wire.len(), "`used` is the window plus the header");
        assert_eq!(back.tag, e.tag);
        assert_eq!(back.value, e.value);
        assert_eq!(back.len, Uleb128((wire.len() - 2) as u64), "the length is what was written");
        assert_eq!(back.encode(), wire, "and a second trip is a fixed point");
    }
}

#[test]
fn an_unknown_entry_in_the_middle_survives_and_swallows_nothing() {
    let m = List {
        version: 3,
        entries: vec![
            entry(1, Value::Point(Point { x: 1, y: 2 })),
            entry(300, Value::Unknown(vec![0x5A; 200])),
            entry(2, label("after")),
        ],
    };
    let bytes = m.encode();

    #[rustfmt::skip]
    let head = vec![
        3,
        0x01, 0x04, 1, 0, 2, 0,
        0xAC, 0x02,
        0xC8, 0x01,
    ];
    assert_eq!(&bytes[..head.len()], &head[..]);
    let tail = [0x02, 0x06, 0x05, b'a', b'f', b't', b'e', b'r'];
    assert_eq!(&bytes[head.len() + 200..], &tail[..]);

    let (back, used) = List::decode(&bytes).expect("parses");
    assert_eq!(used, bytes.len());
    assert_eq!(back.entries.len(), 3, "the entry after the unknown one survives");
    assert_eq!(back.entries[1].value, Value::Unknown(vec![0x5A; 200]));
    assert_eq!(back.entries[2].value, label("after"));
    assert_eq!(back.encode(), bytes, "byte for byte, the unknown entry included");

    assert!(List::decode(&[3]).expect("empty list").0.entries.is_empty());
}

#[test]
fn the_length_is_back_patched_from_the_arm() {
    let lying = Entry { tag: Uleb128(2), len: Uleb128(999), value: label("abc") };
    assert_eq!(lying.encode(), vec![0x02, 0x04, 0x03, b'a', b'b', b'c'], "four");

    assert_eq!(lying.encode().len(), 6);
    assert_eq!(Entry::decode(&lying.encode()).unwrap().0.len, Uleb128(4));

    let wrong_tag = Entry { tag: Uleb128(77), len: Uleb128(0), value: label("") };
    assert_eq!(wrong_tag.encode(), vec![0x02, 0x01, 0x00]);

    let list = List { version: 0, entries: vec![lying.clone(), lying] };
    assert_eq!(
        list.encode(),
        vec![0, 0x02, 0x04, 0x03, b'a', b'b', b'c', 0x02, 0x04, 0x03, b'a', b'b', b'c']
    );
    assert_eq!(List::decode(&list.encode()).unwrap().1, 13);
}

#[test]
fn a_length_that_overruns_the_buffer_is_short() {
    assert_eq!(
        Entry::decode(&[0x01, 0x08, 0x02, 0x01]),
        Err(ParseError::Short { need_bytes: 10, got_bytes: 4, at: 2 })
    );

    assert_eq!(
        Entry::decode(&[0x09, 0x08, 0x02, 0x01]),
        Err(ParseError::Short { need_bytes: 10, got_bytes: 4, at: 2 })
    );

    assert_eq!(
        Entry::decode(&[0x09, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F]),
        Err(ParseError::Short { need_bytes: 0xFFFF_FFFF + 6, got_bytes: 6, at: 6 }),
    );

    let mut bytes =
        List { version: 1, entries: vec![entry(2, label("hi")), entry(2, label("there"))] }
            .encode();
    bytes.truncate(bytes.len() - 2);
    let from_list = List::decode(&bytes).unwrap_err();
    let from_entry = Entry::decode(&bytes[6..]).unwrap_err();
    assert_eq!(from_list, ParseError::Short { need_bytes: 8, got_bytes: 6, at: 8 });
    assert_eq!(from_entry, ParseError::Short { need_bytes: 8, got_bytes: 6, at: 2 });
    assert_eq!(
        from_list,
        from_entry.rebased(6),
        "the list's error is the entry's error, with `at` offset by the entry's position"
    );
}

#[test]
fn a_message_arm_that_leaves_a_remainder_is_malformed() {
    assert_eq!(
        Entry::decode(&[0x02, 0x04, 0x02, b'h', b'i', 0x00]),
        Err(ParseError::Malformed { field: "value", at: 2 }),
    );
    assert_eq!(Entry::decode(&[0x02, 0x03, 0x02, b'h', b'i']).unwrap().0.value, label("hi"));
}

#[test]
fn a_message_arm_that_reads_past_the_window_is_malformed() {
    assert_eq!(
        Entry::decode(&[0x02, 0x02, 0x05, b'h', b'e', b'l', b'l', b'o']),
        Err(ParseError::Malformed { field: "value", at: 2 }),
    );
}

#[test]
fn a_fixed_arm_of_the_wrong_size_is_malformed() {
    assert_eq!(
        Entry::decode(&[0x01, 0x02, 0x02, 0x01]),
        Err(ParseError::Malformed { field: "value", at: 2 })
    );
    assert_eq!(
        Entry::decode(&[0x01, 0x06, 0x02, 0x01, 0x04, 0x03, 0, 0]),
        Err(ParseError::Malformed { field: "value", at: 2 }),
    );
    assert_eq!(
        Entry::decode(&[0x01, 0x04, 0x02, 0x01, 0x04, 0x03]).unwrap().0.value,
        Value::Point(Point { x: 0x0102, y: 0x0304 }),
    );
}

const OPEN_ENDED: [bool; 3] = [
    <Value as layline::Choice>::OPEN_ENDED,
    <Entry as Message>::OPEN_ENDED,
    <List as Message>::OPEN_ENDED,
];

#[test]
fn a_windowed_switch_is_never_open_ended() {
    assert_eq!(
        OPEN_ENDED,
        [true, false, true],
        "`Value` keeps everything through `Unknown`; `Entry` bounds it to the window; `List` \
         fills"
    );
}

#[test]
fn the_segment_table_says_the_window_is_the_length_fields() {
    let table = <Entry as Message>::SEGMENTS;
    assert_eq!(table.len(), 3, "tag, len, value");
    assert_eq!(table[2].name, "value");
    assert_eq!(
        table[2].span,
        Span::ChosenWithin { on: Discriminant::Field("tag"), by: By::field("len") }
    );
    assert_eq!(table[2].decoded_by, Some("Value"));
    assert_eq!(table[2].span.bits(), None, "the span has no fixed width in bits");
}

#[test]
fn the_audit_artifact_names_the_discriminant_and_the_window() {
    let text = <Entry as layline::Message>::audit().to_string();
    assert!(text.contains(" value after,len,0 chosenwithin,field,tag,len,1,0 - Value\n"), "{text}");
}

#[test]
fn scale_offset_and_cap_apply_to_the_window() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Words {
        #[var]
        tag: Uleb128,
        #[var]
        words: Uleb128,
        #[switch(tag)]
        #[len(words, scale = 2, offset = 2, cap = 8)]
        value: Value,
    }

    let m = Words { tag: Uleb128(0), words: Uleb128(99), value: Value::Unknown(vec![1, 2, 3, 4]) };
    assert_eq!(m.encode(), vec![0x00, 0x01, 1, 2, 3, 4], "(4 - 2) / 2 == 1");

    let (back, used) = Words::decode(&m.encode()).expect("parses");
    assert_eq!(used, 6);
    assert_eq!(back.words, Uleb128(1));
    assert_eq!(back.value, m.value);

    assert_eq!(
        Words::decode(&[0x00, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
        Err(ParseError::Malformed { field: "words", at: 2 }),
    );
}
