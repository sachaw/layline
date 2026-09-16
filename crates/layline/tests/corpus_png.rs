//! PNG chunks and `IHDR` (RFC 2083, ISO/IEC 15948); every CRC vector here came from `zlib.crc32`.

#![cfg(feature = "derive")]

use layline::{Checksum, Layout, Message, ParseError};

/// CRC-32/ISO-HDLC, which PNG §5.5 specifies for every chunk.
type Crc32 = layline::checksum::Crc<u32, 0xEDB8_8320, 0xFFFF_FFFF, 0xFFFF_FFFF, true>;

/// PNG §5.3: the CRC covers the type and the data.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Chunk {
    length: u32,
    ctype: [u8; 4],
    #[count(length)]
    data: Vec<u8>,
    #[checksum(Crc32, over = ctype..=data)]
    crc: u32,
}

/// `IHDR` in the order §11.2.2 lists it; depth and colour type are plain `u8` fields.
#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 13, endian = be)]
struct Ihdr {
    width: u32,
    height: u32,
    bit_depth: u8,
    colour_type: u8,
    compression_method: u8,
    filter_method: u8,
    #[range(..=1)]
    interlace_method: u8,
}

const GREY_1X1: [u8; 25] = [
    0x00, 0x00, 0x00, 0x0D, //
    0x49, 0x48, 0x44, 0x52, //
    0x00, 0x00, 0x00, 0x01, //
    0x00, 0x00, 0x00, 0x01, //
    0x08, 0x00, 0x00, 0x00, 0x00, //
    0x3A, 0x7E, 0x9B, 0x55, //
];

const RGBA_640X480: [u8; 25] = [
    0x00, 0x00, 0x00, 0x0D, //
    0x49, 0x48, 0x44, 0x52, //
    0x00, 0x00, 0x02, 0x80, //
    0x00, 0x00, 0x01, 0xE0, //
    0x08, 0x06, 0x00, 0x00, 0x01, //
    0x42, 0xD6, 0xEC, 0x72, //
];

const IEND: [u8; 12] = [0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82];

#[test]
fn a_chunk_decodes_and_re_encodes_byte_for_byte() {
    let (c, used) = Chunk::decode(&GREY_1X1).expect("parses");
    assert_eq!(used, GREY_1X1.len());
    assert_eq!(&c.ctype, b"IHDR");
    assert_eq!(c.length, 13);
    assert_eq!(c.data.len(), 13);
    assert_eq!(c.encode(), GREY_1X1);
}

#[test]
fn a_chunk_with_no_data_round_trips() {
    let (c, used) = Chunk::decode(&IEND).expect("parses");
    assert_eq!(used, IEND.len());
    assert_eq!((&c.ctype, c.length, c.data.len()), (b"IEND", 0, 0));
    assert_eq!(c.encode(), IEND);
}

#[test]
fn the_crc_covers_the_type_and_the_data_and_not_the_length() {
    let (c, _) = Chunk::decode(&GREY_1X1).expect("parses");
    assert_eq!(c.crc, 0x3A7E_9B55);
    assert_eq!(Crc32::compute(&GREY_1X1[4..21]), 0x3A7E_9B55, "type and data");
    assert_ne!(Crc32::compute(&GREY_1X1[..21]), 0x3A7E_9B55, "the length is outside the range");
}

#[test]
fn a_flipped_byte_fails_the_crc() {
    let mut bad = GREY_1X1;
    bad[16] ^= 0x01;
    assert!(matches!(Chunk::decode(&bad), Err(ParseError::Checksum { field: "crc", .. })));
}

#[test]
fn the_header_fields_are_where_the_standard_puts_them() {
    let (c, _) = Chunk::decode(&RGBA_640X480).expect("parses");
    let h = Ihdr::decode(c.data[..].try_into().expect("thirteen bytes")).expect("in range");
    assert_eq!((h.width, h.height), (640, 480));
    assert_eq!((h.bit_depth, h.colour_type), (8, 6));
    assert_eq!(h.interlace_method, 1, "Adam7");
    assert_eq!(h.encode()[..], c.data[..]);
}

#[test]
fn an_undefined_interlace_method_is_refused() {
    let mut data = [0u8; 13];
    data[..12].copy_from_slice(&GREY_1X1[8..20]);
    data[12] = 2;
    assert_eq!(
        Ihdr::decode(&data),
        Err(ParseError::OutOfRange { field: "interlace_method", value: 2, lo: 0, hi: 1 }),
    );
}

#[test]
fn the_published_tables_describe_the_standard() {
    let rows: Vec<(&str, u64, u32)> =
        Ihdr::FIELDS.iter().map(|f| (f.name, f.extent.start(), f.extent.width())).collect();
    assert_eq!(rows[0], ("width", 0, 32));
    assert_eq!(rows[1], ("height", 32, 32));
    assert_eq!(rows[6], ("interlace_method", 96, 8));
    assert_eq!(Ihdr::WIRE_BYTES, 13);

    let bound = Ihdr::RANGES.iter().find(|r| r.field == "interlace_method").expect("a row");
    assert_eq!((bound.lo, bound.hi), (0, 1));
}
