//! A message with a lifetime borrows `&'a [u8]` and `&'a [u8; N]` fields from its input.

#![cfg(feature = "derive")]

use layline::table::{Span, Start};
use layline::{MessageRef, ParseError};

/// Pocket Debug's `UPLOAD_CHUNK`: an offset, then the rest of the frame.
#[derive(layline::Message, Debug, Clone, PartialEq)]
pub struct Chunk<'a> {
    pub offset: u32,
    #[fill]
    pub data: &'a [u8],
}

#[test]
fn a_tail_is_a_view_of_the_input() {
    let wire = [0x10, 0x00, 0x00, 0x00, 0xAA, 0xBB, 0xCC];
    let (chunk, used) = Chunk::decode(&wire).expect("parses");
    assert_eq!((chunk.offset, chunk.data, used), (0x10, &[0xAA, 0xBB, 0xCC][..], 7));
    assert!(core::ptr::eq(chunk.data, &wire[4..]), "borrowed, not copied");

    let mut room = [0u8; 16];
    let mut out = layline::Fixed::new(&mut room);
    chunk.encode_into(&mut out).expect("fits");
    assert_eq!(out.into_written(), wire);

    let (empty, used) = Chunk::decode(&wire[..4]).expect("an empty tail");
    assert_eq!((empty.data, used), (&[][..], 4));
}

/// A CTR RomFS directory entry: a name whose length an earlier field states.
#[derive(layline::Message, Debug, Clone, PartialEq)]
pub struct DirEntry<'a> {
    pub parent: u16,
    pub name_len: u8,
    #[len(name_len)]
    #[align(2)]
    pub name: &'a [u8],
    pub flags: u8,
}

#[test]
fn a_counted_name_borrows_and_encode_states_its_length() {
    // parent 0x0102, length 3, "abc", one pad byte, flags 0x80.
    let wire = [0x02, 0x01, 0x03, b'a', b'b', b'c', 0x00, 0x80];
    let (entry, used) = DirEntry::decode(&wire).expect("parses");
    assert_eq!((entry.parent, entry.name, entry.flags, used), (0x0102, &b"abc"[..], 0x80, 8));

    let fresh = DirEntry { parent: 0x0102, name_len: 0, name: b"abc", flags: 0x80 };
    let mut room = [0u8; 16];
    let mut out = layline::Fixed::new(&mut room);
    fresh.encode_into(&mut out).expect("fits");
    assert_eq!(out.into_written(), wire);

    assert_eq!(
        DirEntry::decode(&wire[..4]),
        Err(ParseError::Short { need_bytes: 6, got_bytes: 4, at: 3 })
    );
}

/// A banner: a fixed icon as an array view, a fixed tag as a slice, and a counted run.
#[derive(layline::Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Banner<'a> {
    pub version: u16,
    pub icon: &'a [u8; 4],
    #[bytes(2)]
    pub tag: &'a [u8],
    pub n: u8,
    #[count(n)]
    pub extra: &'a [u8],
}

#[test]
fn fixed_runs_borrow_as_arrays_and_slices() {
    let wire = [0x00, 0x03, 1, 2, 3, 4, b'O', b'K', 0x02, 0xEE, 0xFF];
    let (banner, used) = Banner::decode(&wire).expect("parses");
    assert_eq!(used, 11);
    assert_eq!(banner.version, 3);
    assert_eq!(banner.icon, &[1, 2, 3, 4]);
    assert_eq!(banner.tag, b"OK");
    assert_eq!((banner.n, banner.extra), (2, &[0xEE, 0xFF][..]));

    let rows = <Banner<'_> as MessageRef<'_>>::SEGMENTS;
    assert_eq!(rows[1].name, "icon");
    assert_eq!(rows[1].start, Start::At(16));
    assert_eq!(rows[1].span, Span::Fixed(32));
}

#[cfg(feature = "alloc")]
#[test]
fn encode_writes_the_count_from_the_run() {
    let banner = Banner { version: 3, icon: &[1, 2, 3, 4], tag: b"OK", n: 0, extra: &[0xEE, 0xFF] };
    assert_eq!(banner.encode(), [0x00, 0x03, 1, 2, 3, 4, b'O', b'K', 0x02, 0xEE, 0xFF]);
}

#[derive(layline::Message, Debug, Clone, PartialEq)]
pub struct Capped<'a> {
    #[fill(cap = 2)]
    pub data: &'a [u8],
}

#[test]
fn a_capped_tail_refuses_more() {
    assert_eq!(Capped::decode(&[1, 2]).map(|(c, _)| c.data), Ok(&[1, 2][..]));
    assert_eq!(Capped::decode(&[1, 2, 3]), Err(ParseError::Malformed { field: "data", at: 0 }));
}
