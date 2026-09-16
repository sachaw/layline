//! `#[magic]` and `#[checksum]` in a fixed `Layout`.
//! An external tool computed every check value here.

#![cfg(feature = "derive")]

#[path = "support/checksum.rs"]
mod checksum;
#[path = "support/fixtures.rs"]
mod fixtures;

use checksum::RotateAddSum;
use fixtures::Sealed;
use layline::{Layout, Message, ParseError, checksum::Crc};

type Crc16Modbus = Crc<u16, 0xA001, 0xFFFF, 0x0000, true>;
type Crc16Ccitt = Crc<u16, 0x1021, 0xFFFF, 0x0000, false>;

#[test]
fn a_byte_string_magic_decodes_and_the_field_carries_it() {
    let wire = [0x53, 0x45, 0x41, 0x4C, 0x10, 0x01, 0x00, 0x00];
    let sealed = Sealed::decode(&wire).expect("the signature is SEAL");
    assert_eq!(sealed.signature, *b"SEAL");
    assert_eq!(sealed.file_size, 0x0000_0110);
}

#[test]
fn a_wrong_magic_is_an_error_naming_the_field() {
    let wire = [0x53, 0x45, 0x41, 0x4D, 0x10, 0x01, 0x00, 0x00];
    let err = Sealed::decode(&wire).expect_err("SEAM is not SEAL");
    assert_eq!(err, ParseError::Magic { field: "signature", expected: b"SEAL", at: 0 });
}

#[test]
fn encode_writes_the_constant_over_whatever_the_writer_supplied() {
    let lying = Sealed { signature: *b"XXXX", file_size: 0x0000_0110 };
    assert_eq!(lying.encode(), [0x53, 0x45, 0x41, 0x4C, 0x10, 0x01, 0x00, 0x00]);
    assert_eq!(
        Sealed::decode(&lying.encode()).expect("encode wrote the magic"),
        Sealed { signature: *b"SEAL", file_size: 0x0000_0110 }
    );
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4, endian = le)]
struct BootTail {
    tail: u16,
    #[magic(0xAA55)]
    signature: u16,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4, endian = be)]
struct BootTailBe {
    tail: u16,
    #[magic(0xAA55)]
    signature: u16,
}

#[test]
fn an_integer_magic_is_the_containers_byte_order() {
    assert_eq!(
        BootTail::decode(&[0x00, 0x00, 0x55, 0xAA]).expect("le"),
        BootTail { tail: 0, signature: 0xAA55 }
    );
    assert_eq!(
        BootTailBe::decode(&[0x00, 0x00, 0xAA, 0x55]).expect("be"),
        BootTailBe { tail: 0, signature: 0xAA55 }
    );
    assert_eq!(
        BootTail::decode(&[0x00, 0x00, 0xAA, 0x55]),
        Err(ParseError::Magic { field: "signature", expected: &[0x55, 0xAA], at: 2 }),
    );
    assert_eq!(
        BootTailBe::decode(&[0x00, 0x00, 0x55, 0xAA]),
        Err(ParseError::Magic { field: "signature", expected: &[0xAA, 0x55], at: 2 }),
    );
}

/// A checksum that covers a checksum: `logo_crc` at `0x15C` over `0x0C0..0x15C`, `header_crc` at
/// `0x15E` over `0x000..0x15E`.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 352, endian = le)]
struct Header {
    body: [u8; 192],
    #[at(byte = 192)]
    logo: [u8; 156],
    #[at(byte = 348)]
    #[checksum(Crc16Modbus, over = logo..)]
    logo_crc: u16,
    #[at(byte = 350)]
    #[checksum(Crc16Modbus, over = ..)]
    header_crc: u16,
}

/// An external tool computed both CRCs over the `i % 256` bytes.
fn header_wire() -> [u8; 352] {
    let mut wire = [0u8; 352];
    for (i, byte) in wire.iter_mut().enumerate().take(348) {
        *byte = i as u8;
    }
    wire[0x15C..0x15E].copy_from_slice(&[0x18, 0xA9]);
    wire[0x15E..0x160].copy_from_slice(&[0x34, 0xB6]);
    wire
}

#[test]
fn a_layout_verifies_both_of_its_checksums() {
    let header = Header::decode(&header_wire()).expect("both CRCs are right");
    assert_eq!(header.logo_crc, 0xA918);
    assert_eq!(header.header_crc, 0xB634);
    assert_eq!(header.body[0], 0);
    assert_eq!(header.logo[0], 0xC0);
}

#[test]
fn a_layout_checksum_covers_the_bytes_it_says_and_no_others() {
    let mut wire = header_wire();
    wire[0x0C0] ^= 0xFF;
    assert_eq!(
        Header::decode(&wire),
        Err(ParseError::Checksum { field: "logo_crc", expected: 0xEDBD, actual: 0xA918 }),
    );

    let mut wire = header_wire();
    wire[0x000] ^= 0xFF;
    assert_eq!(
        Header::decode(&wire),
        Err(ParseError::Checksum { field: "header_crc", expected: 0x538B, actual: 0xB634 }),
    );
}

#[test]
fn encode_stamps_both_in_wire_order_so_the_later_covers_the_earlier() {
    let mut header = Header::decode(&header_wire()).expect("valid");
    header.logo_crc = 0xDEAD;
    header.header_crc = 0xBEEF;
    assert_eq!(header.encode(), header_wire());
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 8, endian = le)]
struct Excised {
    kind: u8,
    count: u8,
    #[checksum(RotateAddSum, over = ..=body)]
    sum: u16,
    body: [u8; 4],
}

#[test]
fn a_layout_checksum_may_cover_what_follows_it_less_its_own_bytes() {
    let wire = [0x85, 0x02, 0x6C, 0x68, 0x11, 0x22, 0x33, 0x44];
    let set = Excised::decode(&wire).expect("0x686C over the two runs");
    assert_eq!(set.sum, 0x686C);

    let mut wrong = wire;
    wrong[2] = 0x00;
    assert_eq!(
        Excised::decode(&wrong),
        Err(ParseError::Checksum { field: "sum", expected: 0x686C, actual: 0x6800 }),
    );
    assert_eq!(Excised { sum: 0, ..set }.encode(), wire);
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 10, endian = le)]
struct Frame {
    #[magic(b"LN")]
    sync: [u8; 2],
    length: u16,
    payload: [u8; 4],
    #[checksum(Crc16Ccitt, over = length..)]
    crc: u16,
}

#[test]
fn a_layout_may_have_both_a_magic_and_a_checksum() {
    let wire = [0x4C, 0x4E, 0x04, 0x00, 0xDE, 0xAD, 0xBE, 0xEF, 0xE6, 0xCC];
    let frame = Frame::decode(&wire).expect("the magic and the CRC both check");
    assert_eq!((frame.length, frame.crc), (4, 0xCCE6));

    let mut both_wrong = wire;
    both_wrong[0] = 0x4D;
    both_wrong[8] = 0x00;
    assert_eq!(
        Frame::decode(&both_wrong),
        Err(ParseError::Magic { field: "sync", expected: b"LN", at: 0 }),
    );

    let mut corrupt = wire;
    corrupt[4] = 0x00;
    let err = Frame::decode(&corrupt).expect_err("the CRC covers the payload");
    assert!(matches!(err, ParseError::Checksum { field: "crc", .. }), "{err:?}");

    let lying = Frame { sync: *b"??", crc: 0, ..frame };
    assert_eq!(lying.encode(), wire);
}

#[test]
fn the_artifact_publishes_the_constant_and_the_coverage() {
    assert_eq!(
        <Sealed as layline::Layout>::audit().to_string(),
        "L Sealed 64\n\
         F Sealed 0 8 signature[0]\n\
         F Sealed 8 8 signature[1]\n\
         F Sealed 16 8 signature[2]\n\
         F Sealed 24 8 signature[3]\n\
         F Sealed 32 32 file_size\n\
         V Sealed ok\n\
         T Sealed signature [u8;4]\n\
         T Sealed file_size u32\n\
         X Sealed signature 5345414c\n",
    );

    let tail = <BootTail as layline::Layout>::audit().to_string();
    assert!(tail.ends_with("X BootTail signature 55aa\n"), "{tail}");
    let tail_be = <BootTailBe as layline::Layout>::audit().to_string();
    assert!(tail_be.ends_with("X BootTailBe signature aa55\n"), "{tail_be}");

    let header = <Header as layline::Layout>::audit().to_string();
    assert!(header.contains("K Header logo_crc before,logo before,logo_crc -\n"), "{header}");
    assert!(header.contains("K Header header_crc start before,header_crc -\n"), "{header}");
    assert!(header.contains("F Header 1536 8 logo[0]\n"), "{header}");
    assert!(header.contains("F Header 2784 16 logo_crc\n"), "{header}");
    assert!(header.contains("F Header 2800 16 header_crc\n"), "{header}");
    assert!(header.contains("S Header logo_crc byte 348\n"), "{header}");

    let set = <Excised as layline::Layout>::audit().to_string();
    assert!(set.contains("K Excised sum start after,body excludes_self\n"), "{set}");

    assert_eq!(
        <Frame as layline::Layout>::audit()
            .to_string()
            .lines()
            .filter(|l| { l.starts_with('X') || l.starts_with('K') })
            .collect::<Vec<_>>(),
        ["X Frame sync 4c4e", "K Frame crc before,length before,crc -"],
    );
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4, endian = be)]
struct Plain {
    a: u16,
    b: u16,
}

#[test]
fn a_layout_that_asserts_nothing_still_decodes_infallibly() {
    // The type annotation is the test: the line does not compile if `decode` returns a `Result`.
    let plain: Plain = Plain::decode(&[0x01, 0x02, 0x03, 0x04]);
    assert_eq!(plain, Plain { a: 0x0102, b: 0x0304 });
    assert_eq!(<Plain as Layout>::CONSTANTS, &[]);
    assert_eq!(<Plain as Layout>::COVERED, &[]);
    let text = <Plain as layline::Layout>::audit().to_string();
    assert!(!text.contains("X ") && !text.contains("K "), "{text}");
}

#[test]
fn an_asserting_layout_is_still_a_wire_type() {
    use layline::Layout;
    assert!(Sealed::decode_slice(&[0x53, 0x45, 0x41, 0x4C, 0, 0, 0, 0]).ok().is_some());
    assert!(Sealed::decode_slice(&[0x53, 0x45, 0x41, 0x4D, 0, 0, 0, 0]).ok().is_none());
    assert!(Sealed::decode_slice(&[0x53, 0x45, 0x41, 0x4C]).ok().is_none());
    let mut out = [0u8; 8];
    assert_eq!(
        Sealed { signature: *b"____", file_size: 0 }
            .encode_into(&mut layline::Fixed::new(&mut out)),
        Ok(())
    );
    assert_eq!(&out[..4], b"SEAL");
}

#[derive(layline::Message, Debug, PartialEq)]
struct OneRecord {
    #[bytes(8)]
    item: Sealed,
}

#[derive(layline::Message, Debug, PartialEq)]
struct ManyRecords {
    n: u8,
    #[count(n)]
    #[bytes(8)]
    items: Vec<Sealed>,
}

#[test]
fn a_refused_element_names_what_a_refused_field_names() {
    let good = Sealed { signature: *b"SEAL", file_size: 272 };

    let mut one = OneRecord { item: good.clone() }.encode();
    one[0] = b'X';
    let from_value = OneRecord::decode(&one).expect_err("the magic is wrong");

    let mut many = ManyRecords { n: 0, items: vec![good.clone(), good] }.encode();
    many[9] = b'X';
    let from_element = ManyRecords::decode(&many).expect_err("the magic is wrong");

    assert_eq!(from_value, ParseError::Magic { field: "signature", expected: b"SEAL", at: 0 },);
    assert_eq!(
        from_element,
        ParseError::Magic { field: "signature", expected: b"SEAL", at: 9 },
        "one record and a run of records fail with the same error",
    );
}

#[test]
fn a_refusal_indexes_the_slice_it_was_given() {
    #[derive(layline::Message, Debug, PartialEq)]
    struct Framed {
        lead: u32,
        #[bytes(8)]
        body: Sealed,
    }

    let good = Framed { lead: 0, body: Sealed { signature: *b"SEAL", file_size: 272 } };
    let mut wire = good.encode();

    wire[4] = b'X';
    let err = Framed::decode(&wire).expect_err("the magic is wrong");
    assert_eq!(
        err,
        ParseError::Magic { field: "signature", expected: b"SEAL", at: 4 },
        "`at` counts from the first byte of the message",
    );

    assert_eq!(
        <Sealed as layline::Layout>::decode_slice(&wire[4..]).expect_err("the magic is wrong"),
        ParseError::Magic { field: "signature", expected: b"SEAL", at: 0 },
    );
}

#[test]
fn a_message_states_the_constant_its_format_fixes() {
    #[derive(layline::Message, Debug, PartialEq)]
    struct Packet {
        #[magic(b"SEAL")]
        tag: [u8; 4],
        n: u8,
        #[count(n)]
        items: Vec<u16>,
    }

    let good = Packet { tag: *b"SEAL", n: 2, items: vec![7, 9] };
    let wire = good.encode();
    assert_eq!(Packet::decode(&wire).expect("round trip"), (good, wire.len()));

    let mut bad = wire.clone();
    bad[0] = b'X';
    assert_eq!(
        Packet::decode(&bad).expect_err("the magic is wrong"),
        ParseError::Magic { field: "tag", expected: b"SEAL", at: 0 },
    );
}

#[test]
fn an_integer_magic_takes_the_messages_byte_order() {
    #[derive(layline::Message, Debug, PartialEq)]
    struct Little {
        #[magic(0xAA55)]
        sync: u16,
        body: u8,
    }

    #[derive(layline::Message, Debug, PartialEq)]
    #[message(endian = be)]
    struct Big {
        #[magic(0xAA55)]
        sync: u16,
        body: u8,
    }

    assert_eq!(Little { sync: 0xAA55, body: 1 }.encode(), vec![0x55, 0xAA, 1]);
    assert_eq!(Big { sync: 0xAA55, body: 1 }.encode(), vec![0xAA, 0x55, 1]);

    assert_eq!(
        Little::decode(&[0xAA, 0x55, 1]).expect_err("the bytes are in the other byte order"),
        ParseError::Magic { field: "sync", expected: &[0x55, 0xAA], at: 0 },
    );
}
