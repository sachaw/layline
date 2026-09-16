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
fn with_reaches_through_a_nested_layout() {
    refused(
        r#"#[derive(Message)]
pub struct M {
    #[bytes(2)]
    pub head: Head,
    #[with(head.n)]
    #[message]
    pub child: Block,
}"#,
        "`#[with(..)]` takes one earlier field name per parameter",
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
        "`M`: parameter `scale` must be an integer, not `f32`",
    );
}
