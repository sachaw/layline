//! Attribute refusals shared by every derive.

use super::*;

#[test]
fn an_unknown_layout_key_is_named() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 16, colour = 9)]
pub struct W {
    pub a: u16,
}"#,
        "`W`: expected `bits`, `bytes`, `words`, `endian`, `order`, `prefix`, `prefix_value`, `view`, `internal`, or `crate`",
    );
}

#[test]
fn an_unknown_message_key_is_named() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
#[message(colour = 9)]
pub struct M {
    pub a: u16,
}"#,
        "`M`: expected `endian`, `bits`, `order`, `needs`, `closed`, or `crate`",
    );
}

#[test]
fn an_unknown_dispatch_key_is_named() {
    refused(
        r#"use layline::Dispatch;

#[derive(Dispatch)]
#[dispatch(id = u8, colour = 9)]
pub enum Frame {
    #[other]
    Unknown { id: u8, body: Vec<u8> },
}"#,
        "`Frame`: expected `id = <int type>`, `prefix = K`, or `crate = <path>`",
    );
}

#[test]
fn an_unknown_codec_key_is_named() {
    refused(
        r#"use layline::FieldCodec;

#[derive(FieldCodec)]
#[bits(8, colour = 9)]
pub enum Kind {
    #[value(1)]
    A,
    #[other]
    Other(u8),
}"#,
        "expected `#[bits(N)]` or `#[bits(N, crate = <path>)]`",
    );
}

#[test]
fn a_width_is_stated_once() {
    let cases = [
        (
            "#[derive(Layout)] #[layout(bytes = 2)] struct V { #[codec(8)] #[codec(16)] a: Nib }",
            "field `a`: duplicate #[codec] attribute",
        ),
        (
            "#[derive(Layout)] #[layout(bytes = 2)] struct V { #[bytes(2)] #[bytes(2)] a: Head }",
            "field `a`: duplicate #[bytes] attribute",
        ),
        (
            "#[derive(Layout)] #[layout(words = 1)] struct V { #[bits(8)] #[bits(4)] a: u8 }",
            "field `a`: duplicate #[bits] attribute",
        ),
        (
            "#[derive(Layout)] #[layout(words = 1)] struct V { #[bytes(2)] #[bytes(2)] a: Head }",
            "field `a`: duplicate #[bytes] attribute",
        ),
        (
            "#[derive(Message)] struct M { #[codec(8)] #[codec(16)] a: Nib }",
            "field `a`: duplicate #[codec] attribute",
        ),
        (
            "#[derive(Message)] struct M { #[bytes(2)] #[bytes(4)] a: Head }",
            "field `a`: duplicate #[bytes] attribute",
        ),
        (
            "#[derive(Message)] struct M { #[text] #[until(0)] #[until(1)] s: String }",
            "field `s`: duplicate #[until] attribute",
        ),
    ];
    for (src, reason) in cases {
        refused(src, reason);
    }
}

#[test]
fn a_container_is_described_once() {
    let cases = [
        (
            "#[derive(Layout)] #[layout(bytes = 1)] #[layout(bytes = 2)] struct V { a: u8 }",
            "`V`: duplicate #[layout] attribute",
        ),
        (
            "#[derive(Message)] #[message(endian = be)] #[message(endian = le)] struct M { a: u8 }",
            "`M`: duplicate #[message] attribute",
        ),
        (
            "#[derive(FieldCodec)] #[bits(3)] #[bits(5)] struct T(u8);",
            "`T`: duplicate #[bits] attribute",
        ),
        (
            "#[derive(Dispatch)] #[dispatch(id = u8)] #[dispatch(id = u8)] \
             enum D { #[value(1)] A(W), #[other] Unknown { id: u8, body: Vec<u8> } }",
            "`D`: duplicate #[dispatch] attribute",
        ),
        (
            "#[derive(Layout)] #[layout(bytes = 1, bytes = 2)] struct V { a: u8 }",
            "`V`: duplicate `bytes` in #[layout]",
        ),
        (
            "#[derive(Layout)] #[layout(bytes = 1, endian = le, endian = be)] struct V { a: u8 }",
            "`V`: duplicate `endian` in #[layout]",
        ),
        (
            "#[derive(Layout)] #[layout(bits = 8, order = msb, order = lsb)] struct V { #[bits(8)] a: u8 }",
            "`V`: duplicate `order` in #[layout]",
        ),
        (
            "#[derive(Layout)] #[layout(bits = 16, prefix = 3, prefix = 4)] struct V { #[bits(12)] a: u16 }",
            "`V`: duplicate `prefix` in #[layout]",
        ),
        (
            "#[derive(Layout)] #[layout(bytes = 1, view, view)] struct V { a: u8 }",
            "`V`: duplicate `view` in #[layout]",
        ),
        (
            "#[derive(Message)] #[message(endian = be, endian = be)] struct M { a: u8 }",
            "`M`: duplicate `endian` in #[message]",
        ),
        (
            "#[derive(Message)] #[message(bits, bits)] struct M { #[bits(8)] a: u8 }",
            "`M`: duplicate `bits` in #[message]",
        ),
        (
            "#[derive(Dispatch)] #[dispatch(id = u8, id = u16)] \
             enum D { #[value(1)] A(W), #[other] Unknown { id: u8, body: Vec<u8> } }",
            "`D`: duplicate `id` in #[dispatch]",
        ),
    ];
    for (src, reason) in cases {
        refused(src, reason);
    }
}

#[test]
fn an_arm_is_keyed_once() {
    let cases = [
        (
            "#[derive(Dispatch)] #[dispatch(id = u8)] \
             enum D { #[value(1)] #[value(2)] A(W), #[other] Unknown { id: u8, body: Vec<u8> } }",
            "variant `A`: duplicate #[value] attribute",
        ),
        (
            "#[derive(FieldCodec)] #[bits(1)] enum F { #[value(0)] #[value(1)] A, #[other] B(u8) }",
            "variant `A`: duplicate #[value] attribute",
        ),
        (
            "#[derive(Message)] enum C { #[value(1)] #[value(2)] A(u8), #[other] O(Vec<u8>) }",
            "variant `A`: duplicate #[value] attribute",
        ),
        (
            "#[derive(Message)] enum C { #[value(1)] #[bytes(2)] #[bytes(2)] A(Head), #[other] O(Vec<u8>) }",
            "variant `A`: duplicate #[bytes] attribute",
        ),
    ];
    for (src, reason) in cases {
        refused(src, reason);
    }
}

#[test]
fn a_carried_width_is_named_by_its_type() {
    let cases = [
        (
            "#[derive(Layout)] #[layout(bytes = 2)] struct V { a: U<12> }",
            "field `a`: `U<12>`, of 12 bits, is not 8, 16, 32, or 64. A codec field must fill a whole scalar",
        ),
        (
            "#[derive(Message)] struct M { a: U<12> }",
            "field `a`: `U<12>`, of 12 bits, is not 8, 16, 32, or 64",
        ),
        (
            "#[derive(Layout)] #[layout(bytes = 2)] struct V { #[codec(8)] a: U<16> }",
            "field `a`: #[codec(8)] disagrees with `U<16>`",
        ),
    ];
    for (src, reason) in cases {
        refused(src, reason);
    }
}

#[test]
fn a_refusal_stub_keeps_the_root_a_bare_width_precedes() {
    let input: DeriveInput =
        syn::parse_str("#[bits(3, crate = ::wire)] struct F(f32);").expect("a declaration");
    let root = crate::root::of(&input);
    assert_eq!(tokens(&quote::quote!(#root)), "::wire");
}
