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

#[test]
fn group_fields_short_of_the_width() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(21)]
pub struct Descriptor {
    #[bits(13)]
    pub words: u16,
    #[bits(7)]
    pub index: u8,
}"#,
        "`Descriptor`: the fields cover 20 bits, but #[bits(21)] states 21. \
         Declare the other 1 bits as a field",
    );
}

#[test]
fn group_fields_past_the_width() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(8)]
pub struct Pair {
    #[bits(5)]
    pub a: u8,
    #[bits(5)]
    pub b: u8,
}"#,
        "`Pair`: the fields cover 10 bits, but #[bits(8)] states 8. \
         Narrow the fields by 2 bits, or widen the group",
    );
}

#[test]
fn group_field_without_a_width() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(8)]
pub struct Pair {
    #[bits(5)]
    pub a: u8,
    pub b: u8,
}"#,
        "field `b`: no width",
    );
}

#[test]
fn group_field_with_an_overlay() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(8)]
pub struct Pair {
    #[bits(4)]
    #[overlay(other: Nibble)]
    pub a: u8,
    #[bits(4)]
    pub b: u8,
}"#,
        "field `a`: #[overlay] does not apply in a `FieldCodec` group",
    );
}

#[test]
fn truncate_on_an_enum() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(1, truncate)]
pub enum Kind {
    #[value(0)]
    A,
    #[value(1)]
    B,
}"#,
        "`Kind`: `truncate` and `order` apply to a struct of `#[bits(N)]` fields",
    );
}

#[test]
fn a_group_key_given_twice() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(8, truncate, truncate)]
pub struct Pair {
    #[bits(4)]
    pub a: u8,
    #[bits(4)]
    pub b: u8,
}"#,
        "`truncate` is given twice",
    );
}
