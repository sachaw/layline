//! Scalar arrays read and write as runs: one copy for bytes, a loop for wider scalars.
//! Expected bytes are placed by hand at each field's offset.

#![cfg(feature = "derive")]

use layline::Layout;

#[derive(Layout, Debug, Clone, PartialEq, Eq)]
#[layout(bytes = 512, view)]
pub struct FsInfo {
    pub lead_sig: u32,
    pub reserved1: [u8; 464],
    pub table: [u16; 8],
    pub struct_sig: u32,
    pub free_count: u32,
    pub next_free: u32,
    pub reserved2: [u8; 12],
    pub trail_sig: u32,
}

fn sector() -> [u8; 512] {
    let mut s = [0u8; 512];
    s[0..4].copy_from_slice(&[0x52, 0x52, 0x61, 0x41]);
    for (i, b) in s[4..468].iter_mut().enumerate() {
        *b = (i * 7 + 3) as u8;
    }
    for i in 0..8 {
        // table[i] = 0x1100 * i + 0x22, little-endian at 468 + 2i
        let v = 0x1100u16 * i as u16 + 0x22;
        s[468 + 2 * i] = v as u8;
        s[469 + 2 * i] = (v >> 8) as u8;
    }
    s[484..488].copy_from_slice(&[0x72, 0x72, 0x41, 0x61]);
    s[488..492].copy_from_slice(&[0x10, 0x20, 0x30, 0x40]);
    s[492..496].copy_from_slice(&[0x01, 0x00, 0x00, 0x00]);
    s[496..508].copy_from_slice(&[9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0xFF, 0xFE]);
    s[508..512].copy_from_slice(&[0x00, 0x00, 0x55, 0xAA]);
    s
}

#[test]
fn a_sector_with_long_byte_runs_reads_each_run_at_its_offset() {
    let s = sector();
    let info = FsInfo::decode(&s);
    assert_eq!(info.lead_sig, 0x4161_5252);
    assert_eq!(info.reserved1[0], 3);
    assert_eq!(info.reserved1[463], (463 * 7 + 3) as u8);
    assert_eq!(info.table, [0x22, 0x1122, 0x2222, 0x3322, 0x4422, 0x5522, 0x6622, 0x7722]);
    assert_eq!(info.struct_sig, 0x6141_7272);
    assert_eq!(info.free_count, 0x4030_2010);
    assert_eq!(info.next_free, 1);
    assert_eq!(info.reserved2, [9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0xFF, 0xFE]);
    assert_eq!(info.trail_sig, 0xAA55_0000);
    assert_eq!(info.encode(), s);
}

#[test]
fn a_view_reads_and_writes_a_run_in_place() {
    let mut s = sector();
    let view = FsInfoView::from_wire_mut(&mut s);
    assert_eq!(view.table()[3], 0x3322);
    view.set_table([0xBEEF; 8]);
    view.set_reserved2([0xAB; 12]);
    assert_eq!(&s[468..470], &[0xEF, 0xBE]);
    assert_eq!(&s[482..484], &[0xEF, 0xBE]);
    assert_eq!(&s[496..508], &[0xAB; 12]);
    assert_eq!(&s[484..488], &[0x72, 0x72, 0x41, 0x61], "the next field is untouched");
}

#[derive(Layout, Debug, Clone, PartialEq, Eq)]
#[layout(bytes = 23, endian = be)]
pub struct Grid {
    pub head: u8,
    pub cells: [[u8; 2]; 3],
    pub signed: [i8; 3],
    pub wide: [i16; 2],
    pub rows: [[u16; 2]; 2],
    pub tail: u8,
}

#[test]
fn nested_and_signed_runs_keep_their_order_and_byte_order() {
    let g = Grid {
        head: 0xAA,
        cells: [[1, 2], [3, 4], [5, 6]],
        signed: [-1, 2, -128],
        wide: [-2, 0x1234],
        rows: [[0x0102, 0x0304], [0x0506, 0x0708]],
        tail: 0xBB,
    };
    let wire = [
        0xAA, // head
        1, 2, 3, 4, 5, 6, // cells
        0xFF, 2, 0x80, // signed
        0xFF, 0xFE, 0x12, 0x34, // wide, big-endian
        1, 2, 3, 4, 5, 6, 7, 8,    // rows, big-endian
        0xBB, // tail
    ];
    assert_eq!(g.encode(), wire);
    assert_eq!(Grid::decode(&wire), g);
}

#[derive(Layout, Debug, Clone, PartialEq, Eq)]
#[layout(words = 4, view)]
pub struct Words {
    #[bits(16)]
    pub id: u16,
    pub name: [u8; 4],
    pub more: [u16; 1],
}

#[test]
fn a_words_layout_copies_its_runs() {
    let w = Words { id: 0x1234, name: *b"LAYL", more: [0xA0B0] };
    let wire = [0x34, 0x12, b'L', b'A', b'Y', b'L', 0xB0, 0xA0];
    assert_eq!(w.encode(), wire);
    assert_eq!(Words::decode(&wire), w);
    assert_eq!(WordsView::new(wire).name(), *b"LAYL");
}
