//! Seeks counted from the stream: Kaitai's `pos` in a type that shares its holder's stream, as
//! Midas BLUE places its data and its extended header from the file's first byte while its header
//! starts twelve bytes in. A record, a collection or a switch may seek so; a window a holder
//! sizes is a stream of its own.

#![cfg(feature = "derive")]

use layline::{Message, ParseError, Stated};

/// What an extended header holds: a run to the end of its window, and a byte it places from the
/// window's start, which is a stream of its own.
#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(endian = be)]
pub struct Keywords {
    pub at: Stated<u8>,
    pub tag: u8,
    #[seek(at, from = stream)]
    #[fill]
    pub rest: Vec<u8>,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Words {
    #[fill]
    pub words: Vec<u16>,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be, closed)]
pub enum Data {
    #[value(1)]
    Bytes(Vec<u8>),
    #[value(2)]
    Words(Words),
}

/// A header nested in its file: its offsets count from the file's first byte.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Header {
    pub ext_start: Stated<u8>,
    pub ext_size: Stated<u8>,
    pub data_start: Stated<u16>,
    pub data_size: Stated<u16>,
    pub format: Stated<u8>,
    pub gap_len: Stated<u8>,
    #[seek(data_start, from = stream)]
    #[len(data_size)]
    #[switch(format)]
    pub data: Data,
    #[count(gap_len)]
    pub gap: Vec<u8>,
    #[seek(ext_start * 16, from = stream)]
    #[len(ext_size)]
    #[message]
    pub ext: Keywords,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct File {
    pub tag: [u8; 4],
    #[message]
    pub header: Header,
}

fn file(data: Data, gap: usize) -> File {
    File {
        tag: *b"BLU!",
        header: Header {
            ext_start: Stated::default(),
            ext_size: Stated::default(),
            data_start: Stated::default(),
            data_size: Stated::default(),
            format: Stated::default(),
            gap_len: Stated::default(),
            data,
            gap: vec![0; gap],
            ext: Keywords { tag: 7, rest: vec![9, 9, 9], ..Keywords::default() },
        },
    }
}

#[test]
fn a_nested_header_places_its_sections_from_the_files_first_byte() {
    let f = file(Data::Bytes(vec![1, 2, 3, 4]), 0);
    let wire = f.encode().unwrap();
    // The header's fields are bytes 4 to 12 of the file; its data starts at the file's byte 12,
    // and its extended header at block 1, byte 16.
    assert_eq!(wire[..12], [b'B', b'L', b'U', b'!', 1, 5, 0, 12, 0, 4, 1, 0]);
    assert_eq!(wire[12..16], [1, 2, 3, 4]);
    // The window is a stream of its own: `at` counts from the extended header's first byte.
    assert_eq!(wire[16..], [2, 7, 9, 9, 9]);
    assert_eq!(File::decode(&wire).unwrap(), (f, wire.len()));
}

#[test]
fn a_switch_and_a_record_seek_past_a_gap() {
    let f = file(Data::Words(Words { words: vec![0x0102, 0x0304, 0x0506] }), 14);
    let wire = f.encode().unwrap();
    assert_eq!(wire[4..12], [2, 5, 0, 12, 0, 6, 2, 14]);
    assert_eq!(wire[32..], [2, 7, 9, 9, 9]);
    assert_eq!(File::decode(&wire).unwrap().0, f);
    // A gap that leaves the extended header off its block cannot be stated.
    assert!(file(Data::Bytes(vec![1]), 1).encode().is_err());
}

#[test]
fn an_offset_before_the_headers_first_byte_is_refused() {
    let mut wire = file(Data::Bytes(vec![1, 2, 3, 4]), 0).encode().unwrap();
    wire[7] = 2;
    assert!(matches!(File::decode(&wire), Err(ParseError::Malformed { .. })));
}

#[test]
fn decoded_alone_a_header_is_its_own_stream() {
    let f = file(Data::Bytes(vec![1, 2, 3, 4]), 0);
    // Its data starts at its own byte 8, and its extended header at block 1, byte 16, after a
    // gap of 4: encode writes no padding, so the gap is the caller's.
    assert!(f.header.encode().is_err());
    let h = Header { gap: vec![0; 4], ..f.header };
    let alone = h.encode().unwrap();
    assert_eq!(alone[..8], [1, 5, 0, 8, 0, 4, 1, 4]);
    assert_eq!(Header::decode(&alone).unwrap().0, h);
}

#[derive(layline::Layout, Debug, Clone, Copy, PartialEq)]
#[layout(bytes = 2, endian = be)]
pub struct Rec {
    pub v: u16,
}

/// BLUE's shape: the gap after the header is sized from both sections' offsets, before either
/// section is written, so the offsets are the caller's and encode checks each section's place.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Sections {
    pub data_start: u8,
    pub ext_start: u8,
    #[count(if data_start < ext_start { data_start - 2 } else { ext_start - 2 })]
    pub gap: Vec<u8>,
    #[seek(data_start, from = stream)]
    #[bytes(2)]
    pub data: Rec,
    #[seek(ext_start, from = stream)]
    #[bytes(2)]
    #[valid(ext_start > data_start)]
    pub ext: Rec,
}

#[test]
fn an_offset_read_before_its_section_is_the_callers_and_encode_checks_it() {
    let s = Sections {
        data_start: 4,
        ext_start: 6,
        gap: vec![0, 0],
        data: Rec { v: 0x0102 },
        ext: Rec { v: 0x0304 },
    };
    let wire = s.encode().unwrap();
    assert_eq!(wire, [4, 6, 0, 0, 1, 2, 3, 4]);
    assert_eq!(Sections::decode(&wire).unwrap(), (s.clone(), wire.len()));
    // An offset that is not where encode puts the section is refused, not rewritten.
    let off = Sections { ext_start: 7, ..s.clone() };
    assert!(matches!(off.encode(), Err(layline::EncodeError::Mismatch { .. })));
    let early = Sections { data_start: 5, gap: vec![0; 3], ..s };
    assert!(early.encode().is_err());
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Rest {
    #[fill]
    pub bytes: Vec<u8>,
}

/// A message that reads to the end of what it is given, in a window of its own, before a field.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Windowed {
    pub at: Stated<u8>,
    pub len: Stated<u8>,
    #[seek(at)]
    #[len(len)]
    #[message]
    pub rest: Rest,
    pub after: u8,
}

#[test]
fn a_record_in_a_window_ends_where_the_window_does() {
    let w = Windowed {
        at: Stated::default(),
        len: Stated::default(),
        rest: Rest { bytes: vec![1, 2, 3] },
        after: 9,
    };
    let wire = w.encode().unwrap();
    assert_eq!(wire, [2, 3, 1, 2, 3, 9]);
    assert_eq!(Windowed::decode(&wire).unwrap(), (w, wire.len()));
}
