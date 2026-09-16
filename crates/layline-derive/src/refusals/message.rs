//! `Message` refusals.

use super::*;

#[test]
fn message_after_open_end() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u32,
    #[fill]
    pub rest_of_them: Vec<u16>,
    pub trailer: u32,
}"#,
        "field `trailer`: follows `rest_of_them`",
    );
}

#[test]
fn message_at_mismatch() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
struct Wrong {
    kind: u8,
    #[at(byte = 3)]
    len: u16,
}"#,
        "field `len`: #[at(byte = 3)] but the solver placed it at byte 1",
    );
}

#[test]
fn message_seek_on_a_scalar() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Header {
    pub value_at: u16,
    #[seek(value_at)]
    pub value: u32,
}"#,
        "field `value`: #[seek] needs a record, not `u32`",
    );
}

#[test]
fn message_count_later_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u32,
    #[count(count)]
    pub items: Vec<u8>,
    pub count: u16,
}"#,
        "field `items`: #[count] names `count`, which comes after this field",
    );
}

#[test]
fn message_count_not_an_integer() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Pair {
    pub lo: u8,
    pub hi: u8,
}

#[derive(Message)]
pub struct FromText {
    pub kind: u8,
    #[text]
    #[until(0)]
    pub label: String,
    #[count(label)]
    pub items: Vec<u8>,
}

#[derive(Message)]
pub struct FromNested {
    #[bytes(2)]
    pub pair: Pair,
    #[count(pair)]
    pub items: Vec<u8>,
}"#,
        "#[count] needs an integer, and `label` is text",
    );
}

#[test]
fn message_count_unknown_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u32,
    pub count: u16,
    #[count(len)]
    pub items: Vec<u8>,
}"#,
        "#[count] names `len`, which is not an integer field of this message",
    );
}

#[test]
fn message_magic_codec() {
    refused(
        r#"#[derive(Debug, Clone, PartialEq)]
struct Level(u8);

impl layline::FieldCodec for Level {
    const BITS: u32 = 8;
    fn from_raw(raw: u64) -> Self {
        Self(raw as u8)
    }
    fn to_raw(&self) -> u64 {
        self.0 as u64
    }
}

#[derive(layline::Message)]
struct Bad {
    #[codec(8)]
    #[magic(0x5)]
    tag: Level,
    #[codec(8)]
    rest: Level,
}"#,
        "field `tag`: #[magic] needs an integer scalar (`u8`..`u64`, `i8`..`i64`) or a `[u8; N]`",
    );
}

#[test]
fn message_magic_walked() {
    refused(
        r#"#[derive(layline::Message)]
struct Bad {
    n: u8,
    #[magic(b"AB")]
    #[count(n)]
    items: Vec<u16>,
}"#,
        "#[magic] cannot be combined with #[count]",
    );
}

#[test]
fn message_nested_element_no_size() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Layout)]
#[layout(bytes = 4)]
pub struct Item {
    pub value: u32,
}

#[derive(Message)]
pub struct Packet {
    pub count: u16,
    #[count(count)]
    pub items: Vec<Item>,
}"#,
        "`Item` elements need a size",
    );
}

#[test]
fn message_nested_ref_into_a_scalar() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub n: u8,
    #[count(n.entries)]
    pub items: Vec<u8>,
}"#,
        "#[count] names `n.entries`",
    );
}

#[test]
fn message_nested_ref_too_deep() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
pub struct Inner {
    #[bits(4)]
    pub n: u8,
    #[bits(4)]
    pub spare: u8,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 1)]
pub struct Outer {
    #[bytes(1)]
    pub inner: Inner,
}

#[derive(Message)]
pub struct Packet {
    #[bytes(1)]
    pub outer: Outer,
    #[count(outer.inner.n)]
    pub items: Vec<u8>,
}"#,
        "`outer.inner.…` reaches more than one level deep",
    );
}

#[test]
fn message_offset_at_a_discovered_field() {
    refused(
        r#"use layline::{Layout, Message};
use layline_core::num::Uleb128;

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Entry {
    pub tag: u16,
}

#[derive(Message)]
pub struct Directory {
    #[var]
    pub table_offset: Uleb128,
    pub table_len: u32,
    #[count(table_len)]
    #[seek(table_offset)]
    #[bytes(2)]
    pub entries: Vec<Entry>,
}"#,
        "Directory: `entries` is placed at offset field `table_offset`, which has no fixed width",
    );
}

#[test]
fn message_recursion_unbounded() {
    refused(
        r#"use layline::Message;

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Elem {
    pub tag: u8,
    #[message]
    pub child: Box<Elem>,
}"#,
        "`Elem`: `child` is a nested `Elem`",
    );
}

#[test]
fn message_stride_is_the_count() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Pair {
    pub a: u8,
    pub b: u8,
}

#[derive(Message)]
pub struct Epoch {
    pub n: u8,
    #[count(n)]
    #[stride(n)]
    #[bytes(2)]
    pub sub_blocks: Vec<Pair>,
}"#,
        "field `sub_blocks`: `n` cannot be both the count and the stride",
    );
}

#[test]
fn message_stride_on_a_single_value() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Epoch {
    pub sb_length: u8,
    #[stride(sb_length)]
    pub reading: u16,
}"#,
        "field `reading`: #[stride] needs a `Vec`, not `u16`",
    );
}

#[test]
fn message_fill_and_stride_together() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Pair {
    pub a: u8,
    pub b: u8,
}

#[derive(Message)]
pub struct Epoch {
    pub sb_length: u8,
    #[fill]
    #[stride(sb_length)]
    #[bytes(2)]
    pub sub_blocks: Vec<Pair>,
}"#,
        "field `sub_blocks`: #[stride] needs #[count], not #[fill]",
    );
}

#[test]
fn message_string_no_attribute() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    pub name: String,
}"#,
        "field `name`: a `String` needs `#[text]`",
    );
}

#[test]
fn message_text_after_fill() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[text]
    #[fill]
    pub rest: String,
    pub checksum: u8,
}"#,
        "field `checksum`: follows `rest`",
    );
}

#[test]
fn message_text_no_length() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[text]
    pub name: String,
}"#,
        "field `name`: #[text] needs a size",
    );
}

#[test]
fn message_text_not_a_string() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[text]
    #[bytes(4)]
    pub name: u32,
}"#,
        "field `name`: #[text] needs a `String`, not `u32`",
    );
}

#[test]
fn message_text_two_lengths() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[text]
    #[until(0)]
    #[bytes(8)]
    pub name: String,
}"#,
        "field `name`: the length is set 2 times (#[until], #[bytes])",
    );
}

#[test]
fn message_var_on_a_scalar() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[var]
    pub id: u32,
}"#,
        "field `id`: `u32` has a fixed size, so #[var] does not apply",
    );
}

#[test]
fn message_variable_element() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub count: u16,
    #[count(count)]
    pub names: Vec<String>,
}"#,
        "field `names`: `String` elements need `#[text]` and a size",
    );
}

#[test]
fn message_vec_no_attribute() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u32,
    pub items: Vec<u8>,
}"#,
        "field `items`: a `Vec` needs a count",
    );
}

#[test]
fn two_claims_one_field() {
    refused(
        r#"use layline::Message;

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Two {
    n: u8,
    #[count(n)]
    items: Vec<u8>,
    #[text]
    #[len(n)]
    label: String,
}"#,
        "Two: `n` is the length of a collection and also the encoded length of a string",
    );
}

#[test]
fn two_kind_words() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
struct TextAndMessage {
    #[text]
    #[bytes(8)]
    #[message]
    name: String,
}"#,
        "field `name`: #[text] and #[message] conflict",
    );
}

#[test]
fn message_field_attribute_on_the_container() {
    refused(
        r#"#[derive(layline::Message)]
#[count(n)]
struct M {
    n: u8,
}"#,
        "`M`: `#[count]` belongs on a field, not the struct",
    );
}

#[test]
fn message_other_on_a_field() {
    refused(
        r#"#[derive(layline::Message)]
struct M {
    #[other]
    n: u8,
}"#,
        "field `n`: `#[other]` belongs on an enum variant",
    );
}

#[test]
fn message_seek_beside_a_stated_position() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Table {
    pub first: u16,
}

#[derive(Message)]
pub struct Dir {
    pub table_off: u16,
    #[at(byte = 2)]
    #[seek(table_off)]
    #[bytes(2)]
    pub table: Table,
}"#,
        "field `table`: `#[at(byte = 2)]` conflicts with `#[seek(table_off)]`",
    );
}

#[test]
fn message_seek_takes_a_field_not_a_number() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Table {
    pub first: u16,
}

#[derive(Message)]
pub struct Dir {
    pub table_off: u16,
    #[seek(4)]
    #[bytes(2)]
    pub table: Table,
}"#,
        "field `table`: `#[seek(..)]` takes a field name",
    );
}

#[test]
fn text_count_is_a_len() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Record {
    pub name_len: u16,
    #[text]
    #[count(name_len)]
    pub name: String,
}"#,
        "field `name`: #[count] does not apply to a string",
    );
}

#[test]
fn until_refuses_a_self_delimiting_element() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct M {
    #[var] #[until(0)]
    pub items: Vec<layline::num::Uleb128>,
}"#,
        "field `items`: #[until] needs one-byte elements, and `layline::num::Uleb128` is self-delimiting",
    );
}

#[test]
fn until_mask_is_not_a_string_extent() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct M {
    #[text] #[until(mask = 0x80)]
    pub name: String,
}"#,
        "field `name`: `#[until(mask = M)]` does not apply to a string",
    );
}

#[test]
fn until_beside_a_count_is_a_second_length() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct M {
    pub n: u8,
    #[count(n)] #[until(0)]
    pub items: Vec<u8>,
}"#,
        "field `items`: #[count] conflicts with another count",
    );
}

#[test]
fn until_mask_zero_is_refused() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct M {
    #[until(mask = 0)]
    pub items: Vec<u8>,
}"#,
        "field `items`: `#[until(mask = 0)]` masks no bits. Name the bits the last element sets",
    );
}

#[test]
fn until_on_a_scalar_has_nothing_to_terminate() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct M {
    #[until(0)]
    pub n: u32,
}"#,
        "field `n`: #[until] needs a string or a `Vec` of one-byte elements, not `u32`",
    );
}

#[test]
fn message_value_on_a_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct M {
    #[value(1)]
    pub kind: u8,
}"#,
        "field `kind`: `#[value]` belongs on an enum variant",
    );
}

#[test]
fn text_fill_takes_no_cap() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct M {
    pub tag: u8,
    #[text]
    #[fill(cap = 4)]
    pub name: String,
}"#,
        "field `name`: a string has no elements to cap",
    );
}
