//! `needs` and `#[with]` refusals.

use super::*;

#[test]
fn message_until_without_text() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[until(0)]
    pub name: String,
}"#,
        "field `name`: #[until] needs a string or a `Vec` of one-byte elements, not `String`",
    );
}

#[test]
fn until_byte_needs_a_one_byte_element() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct M {
    #[until(0)]
    pub items: Vec<u16>,
}"#,
        "field `items`: #[until(b)] needs one-byte elements, and `u16` is 2 bytes wide",
    );
}

#[test]
fn message_recursion_without_a_box() {
    refused(
        r#"use layline::Message;

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Link {
    pub flags: u8,
    #[when(flags & 0x80)]
    #[message]
    pub next: Option<Link>,
}"#,
        "`Link`: `next` contains a `Link` directly, so the type has infinite size. \
         Use `Box<Link>`, or `Vec<Link>` for a sequence",
    );
}

#[test]
fn order_without_bits() {
    refused(
        r#"#[derive(Message)]
#[message(order = msb)]
pub struct Header {
    pub version: u8,
}"#,
        "`order` needs `bits`",
    );
}

#[test]
fn with_on_a_field_that_holds_no_message() {
    refused(
        r#"#[derive(Message)]
pub struct M {
    pub n: u8,
    #[count(n)]
    #[with(n)]
    pub items: Vec<u8>,
}"#,
        "field `items`: `#[with(..)]` needs a `#[message]` field",
    );
}

#[test]
fn with_names_a_field_this_message_does_not_have() {
    refused(
        r#"#[derive(Message)]
pub struct M {
    pub n: u8,
    #[with(missing)]
    #[message]
    pub child: Block,
}"#,
        "field `child`: #[with] names `missing`, which is not an integer field of this message",
    );
}

#[test]
fn a_parameter_that_is_also_a_field() {
    refused(
        r#"#[derive(Message)]
#[message(needs(n: u8))]
pub struct M {
    pub n: u8,
    #[count(n)]
    pub items: Vec<u8>,
}"#,
        "message `M`: `n` is both a parameter and a field. Rename one",
    );
}

#[test]
fn a_parameter_a_seek_would_write_back() {
    refused(
        r#"#[derive(Message)]
#[message(needs(off: u8))]
pub struct M {
    pub a: u8,
    #[seek(off)]
    #[bytes(2)]
    pub head: Head,
}"#,
        "`off` is a parameter, and encode cannot write a record offset into it. \
         Name a field of this message",
    );
}

#[test]
fn a_parameter_that_is_not_an_integer() {
    refused(
        r#"#[derive(Message)]
#[message(needs(scale: f32))]
pub struct M {
    pub a: u8,
}"#,
        "`M`: parameter `scale` must be an integer or a `bool`, not `f32`",
    );
}

#[test]
fn a_byte_order_from_an_integer_parameter() {
    refused(
        r#"#[derive(Message)]
#[message(needs(order: u8), endian = needs(order))]
pub struct M {
    pub a: u16,
}"#,
        "`M`: the byte order reads `order`, which is `u8`. Declare it `order: bool`",
    );
}

#[test]
fn a_byte_order_from_no_parameter() {
    refused(
        r#"#[derive(Message)]
#[message(endian = needs(big))]
pub struct M {
    pub a: u16,
}"#,
        "`M`: the byte order reads `big`, which is not a parameter. Declare it: `needs(big: bool)`",
    );
}

#[test]
fn a_byte_order_from_a_parameter_on_an_enum() {
    refused(
        r#"#[derive(Message)]
#[message(endian = needs(big))]
pub enum Body {
    #[value(1)] A(u16),
}"#,
        "`Body`: an enum takes no parameters to read its byte order from",
    );
}

#[test]
fn a_byte_order_that_is_not_a_word() {
    refused(
        r#"#[derive(Message)]
#[message(needs(big: bool), endian = big)]
pub struct M {
    pub a: u16,
}"#,
        "`M`: expected `endian = le`, `endian = be`, or `endian = needs(p)`",
    );
}

#[test]
fn an_integer_magic_under_a_held_byte_order() {
    refused(
        r#"#[derive(Message)]
#[message(needs(big: bool), endian = needs(big))]
pub struct M {
    #[magic(0xAA55)]
    pub sync: u16,
}"#,
        "field `sync`: an integer #[magic] has no byte order until `big` picks one",
    );
}
