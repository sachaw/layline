//! The ELF section header table, read from a real binary on this machine.
//! It is a counted run at an offset, with the element stride read from the header.

#![cfg(feature = "derive")]

use layline::{Layout, Message};

/// The ELF64 header, in the order gABI §4.1 lists it.
#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 64)]
struct Ehdr {
    #[magic(b"\x7fELF")]
    magic: [u8; 4],
    #[range(1..=2)]
    class: u8,
    #[range(1..=2)]
    data: u8,
    version: u8,
    osabi: u8,
    abiversion: u8,
    ident_pad: [u8; 7],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

/// The first four fields of a section header.
/// `e_shentsize` (64 in every ELF64 file) is the stride, and decode skips the rest of each header.
#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 24)]
struct Shdr {
    sh_name: u32,
    sh_type: u32,
    sh_flags: u64,
    sh_addr: u64,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Elf {
    #[bytes(64)]
    head: Ehdr,
    #[seek(head.e_shoff)]
    #[count(head.e_shnum)]
    #[stride(head.e_shentsize)]
    #[bytes(24)]
    sections: Vec<Shdr>,
}

fn an_elf() -> Option<Vec<u8>> {
    for path in ["/bin/true", "/bin/sh", "/usr/bin/env", "/bin/cat"] {
        let Ok(bytes) = std::fs::read(path) else { continue };
        // `EI_CLASS` 2 is ELF64 and `EI_DATA` 1 is little-endian.
        if bytes.len() > 64 && bytes[..4] == *b"\x7fELF" && bytes[4] == 2 && bytes[5] == 1 {
            return Some(bytes);
        }
    }
    None
}

/// The three numbers at the offsets gABI §4.1 lists, read with plain slicing.
fn by_hand(b: &[u8]) -> (u64, u16, u16) {
    let at = |i: usize, n: usize| -> u64 {
        let mut v = 0u64;
        for k in (0..n).rev() {
            v = (v << 8) | u64::from(b[i + k]);
        }
        v
    };
    (at(0x28, 8), at(0x3A, 2) as u16, at(0x3C, 2) as u16)
}

#[test]
fn the_section_table_is_where_the_header_says_and_as_long_as_it_says() {
    let Some(bytes) = an_elf() else {
        eprintln!("no ELF64 little-endian file on this machine; skipped");
        return;
    };
    let (shoff, shentsize, shnum) = by_hand(&bytes);

    let (elf, _) = Elf::decode(&bytes).expect("parses");
    assert_eq!(elf.head.e_shoff, shoff, "`e_shoff` from plain slicing");
    assert_eq!(elf.head.e_shentsize, shentsize, "`e_shentsize` from plain slicing");
    assert_eq!(elf.head.e_shnum, shnum, "`e_shnum` from plain slicing");
    assert_eq!(elf.sections.len(), usize::from(shnum), "one record per entry");
}

#[test]
fn each_record_is_read_at_the_stride_the_wire_states() {
    let Some(bytes) = an_elf() else { return };
    let (shoff, shentsize, shnum) = by_hand(&bytes);
    let (elf, _) = Elf::decode(&bytes).expect("parses");

    for (i, got) in elf.sections.iter().enumerate() {
        let at = shoff as usize + i * usize::from(shentsize);
        let want = Shdr::decode(bytes[at..at + 24].try_into().expect("24 bytes"));
        assert_eq!(*got, want, "section header {i} at byte {at}");
    }
    assert!(shnum > 1, "more than one entry tests the stride");
}

#[test]
fn the_null_section_header_is_zero() {
    let Some(bytes) = an_elf() else { return };
    let (elf, _) = Elf::decode(&bytes).expect("parses");
    let first = elf.sections.first().expect("at least one");
    assert_eq!(*first, Shdr { sh_name: 0, sh_type: 0, sh_flags: 0, sh_addr: 0 });
}

#[test]
fn the_assertions_hold_against_a_real_header() {
    let Some(bytes) = an_elf() else { return };
    let head: &[u8; 64] = bytes[..64].try_into().expect("sixty-four bytes");
    let checked =
        Ehdr::decode(head).expect("a real ELF64 header passes the magic and range checks");
    assert_eq!((checked.class, checked.data), (2, 1), "ELF64, little-endian");

    let mut wrong = *head;
    wrong[0] = b'M';
    assert!(matches!(Ehdr::decode(&wrong), Err(layline::ParseError::Magic { .. })));

    let mut wrong = *head;
    wrong[4] = 3;
    assert_eq!(
        Ehdr::decode(&wrong),
        Err(layline::ParseError::OutOfRange { field: "class", value: 3, lo: 1, hi: 2 }),
    );
}

#[test]
fn the_tables_describe_the_standard() {
    assert_eq!(Ehdr::WIRE_BYTES, 64, "the gABI's own number");
    let rows: Vec<(&str, u64)> =
        Ehdr::FIELDS.iter().map(|f| (f.name, f.extent.start() / 8)).collect();
    assert!(rows.contains(&("e_shoff", 0x28)));
    assert!(rows.contains(&("e_shentsize", 0x3A)));
    assert!(rows.contains(&("e_shnum", 0x3C)));

    let row = Elf::SEGMENTS.iter().find(|s| s.name == "sections").expect("a row");
    assert!(
        matches!(row.span, layline::table::Span::Strided { .. }),
        "the stride is a field: {row:?}"
    );
}

#[test]
fn a_refusal_inside_the_header_names_the_field_that_refused() {
    let Some(bytes) = an_elf() else { return };

    let mut wrong = bytes.clone();
    wrong[0] = b'M';
    assert!(
        matches!(Elf::decode(&wrong), Err(layline::ParseError::Magic { field: "magic", .. })),
        "the error contains the magic field's name",
    );

    let mut wrong = bytes.clone();
    wrong[4] = 3;
    assert_eq!(
        Elf::decode(&wrong),
        Err(layline::ParseError::OutOfRange { field: "class", value: 3, lo: 1, hi: 2 }),
    );
}
