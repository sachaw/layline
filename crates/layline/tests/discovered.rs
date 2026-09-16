//! Fields whose length in bytes decode reads from the wire: `#[var]` and `#[text]`.

#![cfg(feature = "derive")]

#[path = "support/text.rs"]
mod codecs;

use layline::{Buffer, Message, Overflow, ParseError, VarCodec};
use layline_core::num::{Sleb128, Uleb128, unzigzag, zigzag};

/// Protobuf `sint64`: zigzag over a varint.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Delta(i64);

impl VarCodec for Delta {
    fn decode(bytes: &[u8]) -> Result<(Self, usize), ParseError> {
        let (raw, used) = Uleb128::decode(bytes)?;
        Ok((Self(unzigzag(raw.0)), used))
    }

    fn encode<B: Buffer>(&self, out: &mut B) -> Result<(), Overflow> {
        Uleb128(zigzag(self.0)).encode(out)
    }
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Header {
    magic: u16,
    #[var]
    id: Uleb128,
    #[var]
    delta: Delta,
    trailer: u8,
}

#[test]
fn a_varint_field_splits_the_blocks_around_it() {
    let m = Header { magic: 0xBEEF, id: Uleb128(300), delta: Delta(-2), trailer: 0x5A };
    let bytes = m.encode();
    #[rustfmt::skip]
    assert_eq!(bytes, vec![
        0xEF, 0xBE,   //
        0xAC, 0x02,   //
        0x03,         //
        0x5A,         //
    ]);

    let (back, used) = Header::decode(&bytes).expect("parses");
    assert_eq!(used, bytes.len());
    assert_eq!(back, m);
}

#[test]
fn the_same_shape_encodes_to_different_lengths() {
    let short = Header { magic: 0, id: Uleb128(1), delta: Delta(0), trailer: 0 };
    let long = Header { magic: 0, id: Uleb128(u64::MAX), delta: Delta(0), trailer: 0 };
    assert_eq!(short.encode().len(), 2 + 1 + 1 + 1);
    assert_eq!(long.encode().len(), 2 + 10 + 1 + 1);
    assert_eq!(Header::decode(&long.encode()).unwrap().0, long);
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Filled {
    kind: u8,
    #[var]
    #[fill]
    deltas: Vec<Sleb128>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Counted {
    n: u16,
    #[var]
    #[count(n)]
    values: Vec<Uleb128>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct CappedFill {
    kind: u8,
    #[var]
    #[fill(cap = 3)]
    deltas: Vec<Uleb128>,
}

#[test]
fn a_cap_bounds_a_fill_of_self_delimiting_elements() {
    let three = CappedFill { kind: 1, deltas: vec![Uleb128(1), Uleb128(2), Uleb128(3)] };
    assert_eq!(CappedFill::decode(&three.encode()).unwrap().0, three);

    // Encode refuses a fourth element, so the hostile wire is written by hand: a kind, then four.
    assert_eq!(
        CappedFill::decode(&[1, 1, 1, 1, 1]),
        Err(ParseError::Malformed { field: "deltas", at: 4 })
    );
}

#[test]
fn fill_reads_self_delimiting_elements_until_the_body_runs_out() {
    let m = Filled { kind: 7, deltas: vec![Sleb128(-2), Sleb128(127), Sleb128(0), Sleb128(-129)] };
    let bytes = m.encode();
    #[rustfmt::skip]
    assert_eq!(bytes, vec![
        7,
        0x7E,        // DWARF table 7.8
        0xFF, 0x00,  //
        0x00,        //
        0xFF, 0x7E,  //
    ]);
    let (back, used) = Filled::decode(&bytes).expect("parses");
    assert_eq!(used, bytes.len());
    assert_eq!(back, m);
    assert!(Filled::decode(&[7]).expect("empty is empty").0.deltas.is_empty());
}

#[test]
fn a_counted_repeat_of_varints_back_patches_its_count() {
    let m = Counted { n: 999, values: vec![Uleb128(0), Uleb128(128), Uleb128(12857)] };
    let bytes = m.encode();
    #[rustfmt::skip]
    assert_eq!(bytes, vec![
        3, 0,        //
        0x00,
        0x80, 0x01,  // DWARF table 7.7
        0xB9, 0x64,  //
    ]);
    let (back, used) = Counted::decode(&bytes).expect("parses");
    assert_eq!(used, bytes.len());
    assert_eq!(back.n, 3);
    assert_eq!(back.values, m.values);
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Terminated {
    kind: u8,
    #[text]
    #[until(0)]
    name: String,
    checksum: u8,
}

#[test]
fn a_nul_terminated_string_round_trips() {
    let m = Terminated { kind: 1, name: String::from("hello"), checksum: 0xAB };
    let bytes = m.encode();
    assert_eq!(bytes, b"\x01hello\0\xAB".to_vec());
    let (back, used) = Terminated::decode(&bytes).expect("parses");
    assert_eq!(used, bytes.len());
    assert_eq!(back, m);

    let empty = Terminated { kind: 0, name: String::new(), checksum: 0 };
    assert_eq!(empty.encode(), vec![0, 0, 0]);
    assert_eq!(Terminated::decode(&empty.encode()).unwrap().0, empty);
}

#[test]
#[should_panic(expected = "field `name`: the text contains its terminator byte 0x00")]
fn a_terminator_inside_the_value_is_refused() {
    let _ = Terminated { kind: 0, name: String::from("ab\0cd"), checksum: 0 }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
struct FixedRun {
    #[text]
    #[bytes(8)]
    tag: String,
    value: u16,
}

/// Decode strips only the trailing padding; an interior NUL survives the trip.
#[test]
fn a_nul_padded_fixed_run_round_trips_the_bytes() {
    for wire in [
        b"ABCDEFGH".to_vec(),
        b"AB\0\0\0\0\0\0".to_vec(),
        b"AB\0XY\0\0\0".to_vec(),
        b"\0\0\0\0\0\0\0\0".to_vec(),
    ] {
        let mut buf = wire.clone();
        buf.extend_from_slice(&[0x34, 0x12]);
        let (m, used) = FixedRun::decode(&buf).expect("parses");
        assert_eq!(used, 10);
        assert_eq!(m.value, 0x1234);
        assert_eq!(m.encode(), buf, "the bytes survive the trip: {wire:?}");
    }
}

#[test]
#[should_panic(expected = "field `tag`: the text is longer than its 8 bytes")]
fn a_text_longer_than_its_run_is_refused() {
    let _ = FixedRun { tag: String::from("ABCDEFGHIJ"), value: 0 }.encode();
}

#[test]
#[should_panic(expected = "field `tag`: the text ends in a zero byte")]
fn a_text_ending_in_the_padding_byte_is_refused() {
    let _ = FixedRun { tag: String::from("AB\0"), value: 0 }.encode();
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Prefixed {
    len: u16,
    #[text]
    #[len(len)]
    body: String,
}

#[test]
fn a_length_prefix_is_back_patched_from_the_text() {
    let m = Prefixed { len: 999, body: String::from("héllo") };
    let bytes = m.encode();
    assert_eq!(&bytes[..2], &[6, 0], "six *bytes*, five characters");
    assert_eq!(&bytes[2..], "héllo".as_bytes());

    let (back, used) = Prefixed::decode(&bytes).expect("parses");
    assert_eq!(used, bytes.len());
    assert_eq!(back, Prefixed { len: 6, ..m });
}

#[test]
fn len_on_a_string_takes_the_affine_form_and_a_cap() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Record {
        words: u8,
        #[text(codecs::Ascii)]
        #[len(words, scale = 2, cap = 4)]
        name: String,
    }

    let r = Record { words: 0, name: "ABCD".into() };
    let wire = r.encode();
    assert_eq!(wire, vec![2, b'A', b'B', b'C', b'D'], "four bytes is two words");
    assert_eq!(Record::decode(&wire).expect("parses").0.name, "ABCD");

    let over = [3, b'A', b'B', b'C', b'D', b'E', b'F'];
    assert_eq!(
        Record::decode(&over),
        Err(layline::ParseError::Malformed { field: "words", at: 1 })
    );
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Latin {
    #[text(codecs::Latin1)]
    #[bytes(4)]
    label: String,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Ascii {
    n: u8,
    #[text(codecs::Ascii)]
    #[len(n)]
    word: String,
}

#[test]
fn latin1_maps_every_byte_to_a_code_point() {
    let (m, _) = Latin::decode(&[0xC9, 0x74, 0xE9, 0x00]).expect("parses");
    assert_eq!(m.label, "Été");
    assert_eq!(m.encode(), vec![0xC9, 0x74, 0xE9, 0x00], "and back to the same bytes");

    assert!(core::str::from_utf8(&m.encode()[..3]).is_err());

    let wide = Latin { label: String::from("→") };
    assert_eq!(wide.encode(), vec![b'?', 0, 0, 0]);
}

#[test]
fn ascii_rejects_a_high_byte() {
    let m = Ascii { n: 0, word: String::from("ok") };
    assert_eq!(m.encode(), vec![2, b'o', b'k']);
    assert_eq!(Ascii::decode(&m.encode()).unwrap().0, Ascii { n: 2, ..m });
    assert_eq!(Ascii::decode(&[1, 0xE9]), Err(ParseError::Malformed { field: "word", at: 1 }));
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Table {
    n: u8,
    #[text]
    #[until(0)]
    #[count(n)]
    names: Vec<String>,
}

#[test]
fn a_repeat_of_terminated_strings_is_a_string_table() {
    let m =
        Table { n: 0, names: vec![String::from("alpha"), String::from(""), String::from("beta")] };
    let bytes = m.encode();
    assert_eq!(bytes, b"\x03alpha\0\0beta\0".to_vec());
    let (back, used) = Table::decode(&bytes).expect("parses");
    assert_eq!(used, bytes.len());
    assert_eq!(back.names, m.names);
    assert_eq!(back.n, 3);
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Trailing {
    kind: u8,
    #[text]
    #[fill]
    rest: String,
}

#[test]
fn a_fill_string_takes_the_rest_of_the_body() {
    let m = Trailing { kind: 3, rest: String::from("everything else") };
    let bytes = m.encode();
    assert_eq!(Trailing::decode(&bytes).unwrap().0, m);
    assert_eq!(Trailing::decode(&[3]).unwrap().0.rest, "");
}

#[test]
fn a_varint_that_never_terminates_is_an_error() {
    assert_eq!(
        Header::decode(&[0, 0, 0x80, 0x80, 0x80]),
        Err(ParseError::Short { need_bytes: 4, got_bytes: 3, at: 2 }),
    );
    let eleven = [0, 0, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00];
    assert_eq!(Header::decode(&eleven), Err(ParseError::Malformed { field: "id", at: 2 }));

    assert_eq!(
        Filled::decode(&[7, 0x80]),
        Err(ParseError::Short { need_bytes: 2, got_bytes: 1, at: 1 })
    );
}

#[test]
fn a_string_prefix_longer_than_the_buffer_is_an_error() {
    assert_eq!(
        Prefixed::decode(&[0xFF, 0xFF, b'a']),
        Err(ParseError::Short { need_bytes: 2 + 0xFFFF, got_bytes: 3, at: 2 }),
    );
    assert!(Prefixed::decode(&[0xFF, 0xFF]).is_err());
}

#[test]
fn a_terminator_that_never_arrives_is_an_error() {
    assert_eq!(
        Terminated::decode(b"\x01hello"),
        Err(ParseError::Malformed { field: "name", at: 1 }),
    );
}

#[test]
fn invalid_utf8_in_a_text_field_is_an_error() {
    assert_eq!(
        Prefixed::decode(&[2, 0, 0xC3, 0x28]),
        Err(ParseError::Malformed { field: "body", at: 2 }),
    );
}

#[test]
fn a_truncated_fixed_run_is_short_not_empty() {
    assert_eq!(
        FixedRun::decode(b"ABC"),
        Err(ParseError::Short { need_bytes: 8, got_bytes: 3, at: 0 }),
    );
}

#[test]
fn a_filling_run_can_be_bounded() {
    #[derive(layline::Message, Debug, PartialEq)]
    struct Bounded {
        tag: u8,
        #[fill(cap = 3)]
        items: Vec<u16>,
    }

    let three = Bounded { tag: 1, items: vec![7, 8, 9] };
    let wire = three.encode();
    assert_eq!(Bounded::decode(&wire).expect("three fit"), (three, wire.len()));

    let mut over = wire.clone();
    over.extend_from_slice(&[0, 0]);
    assert!(
        matches!(Bounded::decode(&over), Err(ParseError::Malformed { field: "items", at: 1 })),
        "a cap refuses a body with more elements: {:?}",
        Bounded::decode(&over),
    );

    let mut ragged = wire.clone();
    ragged.push(0);
    assert!(
        matches!(Bounded::decode(&ragged), Err(ParseError::Malformed { field: "items", at: 1 })),
        "a ragged remainder is still an error",
    );
}

#[test]
fn a_rest_can_refuse_a_longer_body() {
    #[derive(layline::Message, Debug, PartialEq)]
    struct Capped {
        tag: u8,
        #[fill(cap = 3)]
        tail: Vec<u8>,
    }

    let three = Capped { tag: 1, tail: vec![7, 8, 9] };
    let wire = three.encode();
    assert_eq!(Capped::decode(&wire).expect("three fit"), (three, wire.len()));
    assert!(
        matches!(
            Capped::decode(&[1, 7, 8, 9, 10]),
            Err(ParseError::Malformed { field: "tail", at: 1 })
        ),
        "a cap refuses a body with more elements: {:?}",
        Capped::decode(&[1, 7, 8, 9, 10]),
    );
}

#[test]
fn a_hostile_varint_count_saturates_instead_of_wrapping() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Hostile {
        #[var]
        n: Uleb128,
        #[count(n, offset = 2)]
        items: Vec<u8>,
    }

    let mut wire = vec![0xFF; 9];
    wire.extend([0x01, 0xAA, 0xBB]);
    assert!(Hostile::decode(&wire).is_err(), "{:?}", Hostile::decode(&wire));
}
