//! `[[T; C]; R]`: the same elements, offsets and rows as `[T; R * C]`, row-major.

#![cfg(feature = "derive")]

use layline::Layout;

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 18, endian = be)]
struct Matrix3 {
    m: [[u16; 3]; 3],
}

const ASCENDING: [u8; 18] = [
    0, 1, 0, 2, 0, 3, //
    0, 4, 0, 5, 0, 6, //
    0, 7, 0, 8, 0, 9,
];

#[test]
fn a_matrix_is_row_major_and_the_outer_dimension_is_the_outer_bracket() {
    let got = Matrix3::decode(&ASCENDING);
    assert_eq!(got.m, [[1, 2, 3], [4, 5, 6], [7, 8, 9]]);
    assert_eq!(got.encode(), ASCENDING);
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 18, endian = be)]
struct Flat {
    m: [u16; 9],
}

#[test]
fn the_nesting_is_invisible_to_the_wire_and_visible_in_the_table() {
    assert_eq!(Matrix3::WIRE_BYTES, Flat::WIRE_BYTES);

    let extents = |f: &[layline::table::FieldDef<'static>]| -> Vec<(u64, u32)> {
        f.iter().map(|r| (r.extent.start(), r.extent.width())).collect()
    };
    assert_eq!(extents(Matrix3::FIELDS), extents(Flat::FIELDS), "the same nine rows");

    let names: Vec<&str> = Matrix3::FIELDS.iter().map(|r| r.name).collect();
    assert_eq!(names[0], "m[0][0]");
    assert_eq!(names[5], "m[1][2]", "row-major, with the index in the row name");
    assert_eq!(names[8], "m[2][2]");

    assert_eq!(Flat::decode(&ASCENDING).encode(), Matrix3::decode(&ASCENDING).encode());
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 8)]
struct Cube {
    c: [[[u8; 2]; 2]; 2],
}

#[test]
fn rank_three_carries_the_same_rule() {
    let wire = [1u8, 2, 3, 4, 5, 6, 7, 8];
    let got = Cube::decode(&wire);
    assert_eq!(got.c, [[[1, 2], [3, 4]], [[5, 6], [7, 8]]]);
    assert_eq!(got.encode(), wire);
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 11, endian = be)]
struct Reading {
    id: u8,
    cov: [[u16; 2]; 2],
    flags: u16,
}

#[test]
fn a_matrix_tiles_beside_its_neighbours() {
    let wire = [0xAA, 0, 1, 0, 2, 0, 3, 0, 4, 0xBE, 0xEF];
    let got = Reading::decode(&wire);
    assert_eq!((got.id, got.cov, got.flags), (0xAA, [[1, 2], [3, 4]], 0xBEEF));
    assert_eq!(got.encode(), wire);

    let run: Vec<&layline::table::FieldDef<'static>> =
        Reading::FIELDS.iter().filter(|f| f.name.starts_with("cov")).collect();
    assert_eq!(run.len(), 4, "four elements, four rows");
    assert_eq!((run[0].name, run[0].extent.start()), ("cov[0][0]", 8));
    assert_eq!(
        run[3].extent.start() + u64::from(run[3].extent.width()),
        72,
        "the run ends where `flags` starts"
    );
}
