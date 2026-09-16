//! `Dispatch` refusals.

use super::*;

#[test]
fn dispatch_duplicate_id() {
    refused(
        r#"use layline::{Dispatch, Layout};

#[derive(Layout)]
#[layout(bytes = 4)]
pub struct A {
    pub x: u32,
}

#[derive(Layout)]
#[layout(bytes = 8)]
pub struct B {
    pub y: u64,
}

#[derive(Dispatch)]
#[dispatch(id = u16)]
pub enum Catalog {
    #[value(7)]
    First(A),
    #[value(7)]
    Second(B),
    #[other]
    Unknown { id: u16, body: Vec<u8> },
}"#,
        "#[value(7)] is already used by another variant",
    );
}

#[test]
fn dispatch_missing_other() {
    refused(
        r#"use layline::{Dispatch, Layout};

#[derive(Layout)]
#[layout(bytes = 4)]
pub struct A {
    pub x: u32,
}

#[derive(Dispatch)]
#[dispatch(id = u16)]
pub enum Catalog {
    #[value(7)]
    First(A),
}"#,
        "`Catalog`: no variant catches unlisted ids",
    );
}

#[test]
fn dispatch_value_on_the_container() {
    refused(
        r#"use layline::{Dispatch, Layout};

#[derive(Layout)]
#[layout(bytes = 4)]
pub struct A {
    pub x: u32,
}

#[derive(Dispatch)]
#[dispatch(id = u16)]
#[value(1)]
pub enum Catalog<'a> {
    #[value(7)]
    First(A),
    #[other]
    Unknown { id: u16, body: &'a [u8] },
}"#,
        "`Catalog`: `#[value]` belongs on a variant",
    );
}

#[test]
fn dispatch_other_and_value() {
    refused(
        r#"use layline::{Dispatch, Layout};

#[derive(Layout)]
#[layout(bytes = 4)]
pub struct A {
    pub x: u32,
}

#[derive(Dispatch)]
#[dispatch(id = u16)]
pub enum Catalog<'a> {
    #[value(7)]
    First(A),
    #[other]
    #[value(8)]
    Unknown { id: u16, body: &'a [u8] },
}"#,
        "variant `Unknown`: both #[other] and #[value(..)]",
    );
}

#[test]
fn dispatch_attribute_on_a_payload() {
    refused(
        r#"use layline::{Dispatch, Layout};

#[derive(Layout)]
#[layout(bytes = 4)]
pub struct A {
    pub x: u32,
}

#[derive(Dispatch)]
#[dispatch(id = u16)]
pub enum Catalog<'a> {
    #[value(7)]
    First(#[value(7)] A),
    #[other]
    Unknown { id: u16, body: &'a [u8] },
}"#,
        "variant `First`: `#[value]` is ignored on a payload",
    );
}

#[test]
fn dispatch_on_a_variant() {
    refused(
        r#"use layline::{Dispatch, Layout};

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 2)]
pub struct Sample {
    pub a: u16,
}

#[derive(Dispatch)]
#[dispatch(id = u16)]
pub enum Payload<'a> {
    #[dispatch(id = u8)]
    #[value(4)]
    Sample(Sample),
    #[other]
    Unknown { id: u16, body: &'a [u8] },
}"#,
        "variant `Sample`: `#[dispatch(..)]` belongs above the enum",
    );
}

#[test]
fn a_dispatch_id_fits_the_prefix() {
    refused(
        "#[derive(Dispatch)] #[dispatch(id = u8, prefix = 3)] \
         enum D { #[value(9)] A(W), #[other] Unknown { id: u8, body: Vec<u8> } }",
        "variant `A`: #[value(9)] does not fit #[dispatch(prefix = 3)]. The largest id is 7",
    );
}
