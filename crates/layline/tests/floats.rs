//! A float field keeps every bit pattern, including NaN payloads.

#![cfg(feature = "derive")]

use layline::Layout;

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 16, endian = be)]
struct Fix {
    latitude: f64,
    longitude: f64,
}

const ONE_STATED: [u8; 16] = [
    0x40, 0x49, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x7F, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];

#[test]
fn a_nan_with_a_payload_decodes_as_a_nan() {
    let fix = Fix::decode(&ONE_STATED);
    assert_eq!(fix.latitude, 51.5);
    assert!(fix.longitude.is_nan(), "a NaN with a payload is still a NaN");
}

#[test]
fn a_negative_nan_decodes_as_a_nan() {
    let mut wire = ONE_STATED;
    wire[8..].copy_from_slice(&[0xFF, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    assert!(Fix::decode(&wire).longitude.is_nan(), "a NaN with the sign bit set is a NaN");
}

#[test]
fn every_bit_pattern_round_trips_nan_payloads_included() {
    assert_eq!(Fix::decode(&ONE_STATED).encode(), ONE_STATED);

    let mut wire = ONE_STATED;
    wire[8..].copy_from_slice(&[0xFF, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2A]);
    assert_eq!(Fix::decode(&wire).encode(), wire);
}

#[test]
fn negative_zero_is_a_number() {
    let mut wire = ONE_STATED;
    wire[..8].copy_from_slice(&[0x80, 0, 0, 0, 0, 0, 0, 0]);
    let got = Fix::decode(&wire).latitude;
    assert!(got == 0.0 && got.is_sign_negative());
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 4, endian = be)]
struct Channel {
    value: f32,
}

#[test]
fn binary32_carries_the_same_rule() {
    let wire = [0x7F, 0xC0, 0x00, 0x07];
    assert!(Channel::decode(&wire).value.is_nan());
    assert_eq!(Channel::decode(&wire).encode(), wire, "the payload survives at binary32 too");
    assert_eq!(Channel::decode(&[0x42, 0x4E, 0x00, 0x00]).value, 51.5);
}

#[test]
fn the_field_table_says_nothing_about_absence() {
    let rows: Vec<(&str, u64, u32)> =
        Fix::FIELDS.iter().map(|f| (f.name, f.extent.start(), f.extent.width())).collect();
    assert_eq!(rows, vec![("latitude", 0, 64), ("longitude", 64, 64)]);
}
