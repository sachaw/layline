//! `FieldCodec` refusals.

use super::*;

#[test]
fn codec_duplicate_value() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(2)]
pub enum Quadrant {
    #[value(0)]
    N,
    #[value(1)]
    E,
    #[value(1)]
    S,
    #[other]
    Other(u8),
}"#,
        "#[value(1)] is already used by another variant",
    );
}

#[test]
fn codec_not_total() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(3)]
pub enum Identity {
    #[value(0)]
    Pending,
    #[value(1)]
    Friend,
    #[value(2)]
    Hostile,
}"#,
        "`Identity`: 5 of the 8 values #[bits(3)] can hold have no variant. \
         Add `#[other] Undefined(u8)`, or list all 8",
    );
}

#[test]
fn codec_newtype_over_a_float() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(32)]
pub struct Ratio(f32);"#,
        "`f32` is not an integer primitive",
    );
}

#[test]
fn codec_value_too_wide() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(3)]
pub enum Identity {
    #[value(0)]
    Pending,
    #[value(9)]
    Friend,
    #[other]
    Undefined(u8),
}"#,
        "#[value(9)] does not fit #[bits(3)]",
    );
}

#[test]
fn codec_value_on_the_container() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(3)]
#[value(1)]
pub struct Track(u8);"#,
        "`Track`: `#[value]` belongs on an enum variant, not a newtype",
    );
}

#[test]
fn codec_bits_on_the_newtype_field() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(3)]
pub struct Track(#[bits(3)] u8);"#,
        "`Track`'s field: `#[bits(N)]` sets the width of the whole type",
    );
}

#[test]
fn codec_attribute_on_a_variant_payload() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(3)]
pub enum Kind {
    #[value(0)]
    A,
    #[other]
    Other(#[other] u8),
}"#,
        "variant `Other`: `#[other]` belongs on a variant, not a field",
    );
}

#[test]
fn field_codec_bits_on_a_variant() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(2)]
pub enum Quadrant {
    #[bits(2)]
    #[value(0)]
    North,
    #[value(1)]
    E,
    #[other]
    Other(u8),
}"#,
        "variant `North`: `#[bits(N)]` sets the enum's width. Write it above the enum",
    );
}
