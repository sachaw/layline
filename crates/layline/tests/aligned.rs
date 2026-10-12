//! `#[align(N)]`: padding after a field that its stated length does not count.

#![cfg(all(feature = "derive", feature = "alloc"))]

use layline::table::{Presence, Span, Start};
use layline::{Layout, Message, ParseError};

/// A RIFF chunk: an odd body is followed by one pad byte that `size` does not count.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Chunk {
    pub id: [u8; 4],
    pub size: u32,
    #[len(size)]
    #[align(2)]
    pub body: Vec<u8>,
}

/// `size` counts the form type and the chunks, padding included.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Riff {
    #[magic(b"RIFF")]
    pub magic: [u8; 4],
    pub size: u32,
    pub form: [u8; 4],
    #[len(size, offset = -4)]
    #[message]
    pub chunks: Vec<Chunk>,
}

fn chunk(id: &[u8; 4], body: &[u8]) -> Chunk {
    Chunk { id: *id, size: 0, body: body.to_vec() }
}

#[rustfmt::skip]
const WAVE: [u8; 34] = [
    b'R', b'I', b'F', b'F', 26, 0, 0, 0, b'W', b'A', b'V', b'E',
    // "fmt " with a 3-byte body and its pad byte.
    b'f', b'm', b't', b' ', 3, 0, 0, 0, 1, 2, 3, 0,
    // "data" with a 2-byte body and no pad.
    b'd', b'a', b't', b'a', 2, 0, 0, 0, 9, 9,
];

#[test]
fn an_odd_riff_chunk_is_followed_by_an_uncounted_pad_byte() {
    let riff = Riff {
        magic: *b"RIFF",
        size: 0,
        form: *b"WAVE",
        chunks: vec![chunk(b"fmt ", &[1, 2, 3]), chunk(b"data", &[9, 9])],
    };
    assert_eq!(riff.encode(), WAVE);

    let (back, used) = Riff::decode(&WAVE).expect("parses");
    assert_eq!(used, 34);
    assert_eq!((back.chunks[0].size, back.chunks[1].size), (3, 2));
    assert_eq!(back.chunks[0].body, [1, 2, 3]);
    assert_eq!(back.chunks[1].body, [9, 9]);
}

#[test]
fn decode_skips_the_pad_without_reading_it() {
    let mut odd = [b'f', b'm', b't', b' ', 1, 0, 0, 0, 7, 0xFF];
    let (back, used) = Chunk::decode(&odd).expect("parses");
    assert_eq!((back.body.as_slice(), used), (&[7][..], 10));
    assert_eq!(back.encode(), [b'f', b'm', b't', b' ', 1, 0, 0, 0, 7, 0], "encode writes zeros");

    odd[4] = 2;
    assert_eq!(Chunk::decode(&odd).map(|(c, _)| c.body), Ok(vec![7, 0xFF]), "an even body");
}

#[test]
fn a_missing_pad_byte_is_short() {
    let cut = [b'f', b'm', b't', b' ', 1, 0, 0, 0, 7];
    assert_eq!(Chunk::decode(&cut), Err(ParseError::Short { need_bytes: 10, got_bytes: 9, at: 9 }));
}

#[test]
fn the_table_gives_the_padding_a_row() {
    let rows = <Chunk as Message>::SEGMENTS;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1].name, "body");
    assert_eq!(rows[2].name, "body.pad");
    assert_eq!(rows[2].start, Start::After { segment: "body", bits: 0 });
    assert_eq!(rows[2].span, Span::Pad { align: 2 });
}

/// An IS-NITRO command frame on the 16-bit ring: an odd payload is padded to even.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct CommandFrame {
    #[magic(0x9966u16)]
    pub magic: u16,
    pub channel: u16,
    pub len: u16,
    #[len(len)]
    #[align(2)]
    pub payload: Vec<u8>,
}

#[test]
fn a_ring_frame_pads_to_even() {
    let frame = CommandFrame { magic: 0, channel: 0x0102, len: 0, payload: vec![0xAA, 0xBB, 0xCC] };
    let wire = [0x66, 0x99, 0x02, 0x01, 0x03, 0x00, 0xAA, 0xBB, 0xCC, 0x00];
    assert_eq!(frame.encode(), wire);
    assert_eq!(CommandFrame::decode(&wire).expect("parses").1, 10);

    let even = CommandFrame { payload: vec![0xAA, 0xBB], ..frame };
    assert_eq!(even.encode(), [0x66, 0x99, 0x02, 0x01, 0x02, 0x00, 0xAA, 0xBB]);
}

/// A name padded to four bytes, then a field after it.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Named {
    pub n: u8,
    #[len(n)]
    #[text]
    #[align(4)]
    pub name: String,
    pub tail: u8,
}

#[test]
fn padded_text_is_followed_by_the_next_field() {
    let named = Named { n: 0, name: String::from("abcde"), tail: 0x77 };
    // n, five bytes of name, three of padding: eight bytes from the name's start.
    let wire = [5, b'a', b'b', b'c', b'd', b'e', 0, 0, 0, 0x77];
    assert_eq!(named.encode(), wire);
    assert_eq!(Named::decode(&wire), Ok((Named { n: 5, ..named }, 10)));
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 1)]
pub struct Byte {
    pub v: u8,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub enum Body {
    #[value(1)]
    #[bytes(1)]
    One(Byte),
    #[other]
    Unknown(Vec<u8>),
}

/// A tag-length-value record whose value is padded to four bytes.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Tlv {
    pub tag: u8,
    pub len: u8,
    #[switch(tag)]
    #[len(len)]
    #[align(4)]
    pub body: Body,
    pub tail: u8,
}

#[test]
fn a_padded_switch_window_is_followed_by_the_next_field() {
    let tlv = Tlv { tag: 0, len: 0, body: Body::One(Byte { v: 0x42 }), tail: 0x77 };
    let wire = [1, 1, 0x42, 0, 0, 0, 0x77];
    assert_eq!(tlv.encode(), wire);
    assert_eq!(Tlv::decode(&wire), Ok((Tlv { tag: 1, len: 1, ..tlv }, 7)));
}

/// A block that states its own size, which counts its padding, as an SDAT SYMB block does.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Symb {
    #[magic(b"SYMB")]
    pub magic: [u8; 4],
    pub block_size: u32,
    #[len(block_size, offset = -8)]
    pub body: Vec<u8>,
}

/// The SDAT header, whose size for the block does not count the padding.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Sdat {
    pub symb_offset: u32,
    pub symb_size: u32,
    #[seek(symb_offset)]
    #[when(symb_size > 0)]
    #[align(4)]
    #[message]
    pub symb: Option<Symb>,
}

/// As the SDK writes it: `symb_size` 13, the block's own size 16, three bytes of padding.
#[rustfmt::skip]
const SDK: [u8; 24] = [
    8, 0, 0, 0, 13, 0, 0, 0,
    b'S', b'Y', b'M', b'B', 16, 0, 0, 0, b'a', 0, b'b', b'c', 0, 0, 0, 0,
];

#[test]
fn a_record_may_count_its_own_padding() {
    let (sdat, used) = Sdat::decode(&SDK).expect("an SDK file parses");
    assert_eq!(used, 24);
    assert_eq!((sdat.symb_offset, sdat.symb_size), (8, 13));
    let symb = sdat.symb.clone().expect("present");
    assert_eq!(
        (symb.block_size, symb.body.as_slice()),
        (16, &[b'a', 0, b'b', b'c', 0, 0, 0, 0][..])
    );

    // Encode writes the length the record measures, which includes its own padding.
    let mut rewritten = SDK;
    rewritten[4] = 16;
    assert_eq!(sdat.encode(), rewritten);
}

#[test]
fn encode_pads_a_record_after_its_true_length() {
    let symb = Symb { magic: *b"SYMB", block_size: 0, body: vec![b'a', 0, b'b', b'c', 0] };
    let sdat = Sdat { symb_offset: 0, symb_size: 0, symb: Some(symb) };
    #[rustfmt::skip]
    let wire = [
        8, 0, 0, 0, 13, 0, 0, 0,
        b'S', b'Y', b'M', b'B', 13, 0, 0, 0, b'a', 0, b'b', b'c', 0, 0, 0, 0,
    ];
    assert_eq!(sdat.encode(), wire);
    assert_eq!(Sdat::decode(&wire).map(|(s, used)| (s.symb_size, used)), Ok((13, 24)));
}

#[test]
fn a_record_shorter_than_its_stated_length_is_malformed() {
    // `symb_size` 20 rounds to 20; the block ends at 16.
    let mut long = SDK.to_vec();
    long[4] = 20;
    long.extend_from_slice(&[0; 4]);
    assert_eq!(Sdat::decode(&long), Err(ParseError::Malformed { field: "symb_size", at: 8 }));

    let rows = <Sdat as Message>::SEGMENTS;
    assert_eq!(rows[2].name, "symb.pad");
    assert_eq!(rows[2].when, Some(Presence::Length { by: "symb_size" }));
}

/// A wide alignment, past the constant the padding is pushed from.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Sector {
    pub n: u8,
    #[len(n)]
    #[align(80)]
    pub data: Vec<u8>,
}

#[test]
fn a_wide_alignment_pads_byte_by_byte() {
    let wire = Sector { n: 0, data: vec![7] }.encode();
    assert_eq!(wire.len(), 81);
    assert_eq!((wire[0], wire[1]), (1, 7));
    assert!(wire[2..].iter().all(|&b| b == 0));
    assert_eq!(Sector::decode(&wire).map(|(s, used)| (s.data, used)), Ok((vec![7], 81)));
}
