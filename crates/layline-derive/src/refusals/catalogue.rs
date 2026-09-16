//! `Message` refusals on enums used by `#[switch]`.

use super::*;

#[test]
fn message_recursion_no_terminating_arm() {
    refused(
        r#"use layline::Message;

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Body {
    pub v: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
pub enum Chooser {
    #[value(0)]
    Left(Box<Chooser>),
    #[value(1)]
    Right(Box<Chooser>),
}"#,
        "`Chooser`: every arm always contains a `Chooser`, so the wire never ends",
    );
}

#[test]
fn switch_arm_without_value() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub enum Body {
    #[value(0)]
    Ping(u16),
    Level(u8),
    #[other]
    Unknown(Vec<u8>),
}"#,
        "variant `Level`: needs #[value(N)]",
    );
}

#[test]
fn switch_duplicate_other() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub enum Body {
    #[value(0)]
    Ping(u16),
    #[other]
    Unknown(Vec<u8>),
    #[other]
    AlsoUnknown(Vec<u8>),
}"#,
        "variant `AlsoUnknown`: a second #[other]",
    );
}

#[test]
fn switch_duplicate_value() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub enum Body {
    #[value(1)]
    First(u8),
    #[value(1)]
    Second(u16),
    #[other]
    Unknown(Vec<u8>),
}"#,
        "#[value(1)] is already used by another arm",
    );
}

#[test]
fn switch_not_total() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Ping {
    pub seq: u16,
}

#[derive(Message)]
pub enum Body {
    #[value(0)]
    Ping(Ping),
    #[value(1)]
    Level(u8),
}"#,
        "`Body`: unlisted discriminants have no arm",
    );
}

#[test]
fn catalogue_field_attribute_on_the_container() {
    refused(
        r#"#[derive(layline::Message)]
#[when(flags & 1)]
enum Arms {
    #[value(0)]
    A(u8),
    #[other]
    Unknown(Vec<u8>),
}"#,
        "`Arms`: `#[when]` belongs on a field, not the enum",
    );
}

#[test]
fn catalogue_value_on_the_container() {
    refused(
        r#"#[derive(layline::Message)]
#[value(0)]
enum Arms {
    #[value(0)]
    A(u8),
    #[other]
    Unknown(Vec<u8>),
}"#,
        "`Arms`: `#[value]` belongs on a variant, not the enum",
    );
}

#[test]
fn catalogue_bytes_on_the_other_arm() {
    refused(
        r#"#[derive(layline::Message)]
enum Arms {
    #[value(0)]
    A(u8),
    #[other]
    #[bytes(4)]
    Unknown(Vec<u8>),
}"#,
        "variant `Unknown`: the #[other] arm takes its size from the switch",
    );
}

#[test]
fn catalogue_attribute_on_a_payload() {
    refused(
        r#"#[derive(layline::Message)]
enum Arms {
    #[value(0)]
    A(#[bytes(1)] u8),
    #[other]
    Unknown(Vec<u8>),
}"#,
        "variant `A`: `#[bytes]` on a variant's payload has no effect",
    );
}

#[test]
fn catalogue_arm_count_on_a_variant() {
    refused(
        r#"use layline::Message;

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Body {
    pub n: u8,
}

#[derive(Message)]
pub enum Arms {
    #[value(0)]
    #[count(n)]
    A(Body),
    #[other]
    Unknown(Vec<u8>),
}"#,
        "variant `A`: `#[count]` belongs on a field, not a variant",
    );
}

#[test]
fn needs_on_a_catalogue_of_arms() {
    refused(
        r#"#[derive(Message)]
#[message(needs(stride: u8), closed)]
pub enum Body {
    #[value(1)]
    A(u8),
}"#,
        "`Body`: `needs` does not apply to an enum",
    );
}

#[test]
fn a_catalogue_discriminant_fits_i64() {
    refused(
        "#[derive(Message)] enum C { #[value(9223372036854775808)] A(u8), #[other] O(Vec<u8>) }",
        "variant `A`: #[value(9223372036854775808)] is past `i64::MAX`",
    );
}
