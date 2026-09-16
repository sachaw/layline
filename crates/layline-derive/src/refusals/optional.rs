//! Optional field refusals: `#[when]` and trailing `#[fill]`.

use super::*;

#[test]
fn message_after_optional_fill() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u8,
    #[when(flags & 0x80)]
    #[text]
    #[fill]
    pub note: Option<String>,
    pub trailer: u8,
}"#,
        "field `trailer`: follows `note`",
    );
}

#[test]
fn message_at_and_when() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Table {
    pub first: u16,
}

#[derive(Message)]
pub struct Header {
    pub flags: u8,
    pub table_at: u16,
    #[when(flags & 0x01)]
    #[seek(table_at)]
    #[bytes(2)]
    pub table: Option<Table>,
}"#,
        "field `table`: `#[when(flags & 0x1)]` and `#[seek(table_at)]` both decide whether the record is present",
    );
}

#[test]
fn message_at_on_an_optional_collection() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Entry {
    pub tag: u16,
}

#[derive(Message)]
pub struct Directory {
    pub table_at: u16,
    pub table_len: u16,
    #[seek(table_at)]
    #[count(table_len)]
    #[bytes(2)]
    pub entries: Option<Vec<Entry>>,
}"#,
        "field `entries`: `#[seek(table_at)]` on an `Option` needs a record",
    );
}

#[test]
fn message_option_without_when() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u16,
    pub timestamp: Option<u32>,
}"#,
        "field `timestamp`: nothing says whether this `Option` is present",
    );
}

#[test]
fn message_when_bare_names_a_word() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u16,
    #[when(flags)]
    pub timestamp: Option<u32>,
}"#,
        "#[when(flags)] needs a `bool`, and `flags` is an integer",
    );
}

#[test]
fn message_when_compares_a_constant() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct InfoBlock {
    pub content_len: u32,
    #[when(content_len >= 0x30)]
    pub emulation_memory_bytes: Option<u64>,
}"#,
        "field `emulation_memory_bytes`: `#[when(content_len>=0x30)]` is not supported",
    );
}

#[test]
fn message_when_equality() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u16,
    #[when(flags == 3)]
    pub timestamp: Option<u32>,
}"#,
        "field `timestamp`: `#[when(flags==3)]` is not supported",
    );
}

#[test]
fn message_when_flag_is_also_a_count() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u8,
    #[when(flags & 0x01)]
    pub extra: Option<u16>,
    #[count(flags)]
    pub items: Vec<u8>,
}"#,
        "`flags` is already the count of `items`",
    );
}

#[test]
fn message_when_later_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    #[when(flags & 0x01)]
    pub timestamp: Option<u32>,
    pub flags: u16,
}"#,
        "field `timestamp`: #[when] names `flags`, which comes after this field",
    );
}

#[test]
fn message_when_mask_too_wide() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u8,
    #[when(flags & 0x100)]
    pub timestamp: Option<u32>,
}"#,
        "the mask tests bit 8, but `flags` is only 8 bits wide",
    );
}

#[test]
fn message_when_not_an_integer() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    #[text]
    #[until(0)]
    pub flags: String,
    #[when(flags & 0x01)]
    pub timestamp: Option<u32>,
}"#,
        "#[when] needs an integer, and `flags` is text",
    );
}

#[test]
fn message_when_not_an_option() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u16,
    #[when(flags & 0x01)]
    pub timestamp: u32,
}"#,
        "#[when(..)] needs an `Option` field",
    );
}

#[test]
fn message_when_on_a_collection() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u8,
    pub n: u8,
    #[when(flags & 0x01)]
    #[count(n)]
    pub items: Option<Vec<u8>>,
}"#,
        "field `items`: #[when(..)] cannot make a collection optional",
    );
}

#[test]
fn message_when_positive_names_the_offset() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Table {
    pub first: u16,
}

#[derive(Message)]
pub struct Header {
    pub table_at: u16,
    #[seek(table_at)]
    #[when(table_at > 0)]
    #[bytes(2)]
    pub table: Option<Table>,
}"#,
        "`#[when(table_at > 0)]` repeats what `#[seek(table_at)]` already implies",
    );
}

#[test]
fn message_when_positive_on_a_data_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Entry {
    pub tenths: u8,
    pub time: u16,
    pub date: u16,
    #[when(date > 0)]
    pub created: Option<u32>,
}"#,
        "field `created`: `#[when(date > 0)]` works only with `#[seek]`",
    );
}

#[test]
fn message_when_positive_spelled_as_not_equal() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Table {
    pub first: u16,
}

#[derive(Message)]
pub struct Header {
    pub table_at: u16,
    pub table_size: u16,
    #[seek(table_at)]
    #[when(table_size != 0)]
    #[bytes(2)]
    pub table: Option<Table>,
}"#,
        "field `table`: `#[when(table_size!=0)]` is not supported",
    );
}

#[test]
fn message_when_two_fields_share_a_bool() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
pub struct PackedHeader {
    #[bits(1)]
    pub on: bool,
    #[bits(7)]
    pub spare: u8,
}

#[derive(Debug, Clone, Message)]
pub struct Packet {
    #[bytes(1)]
    pub header: PackedHeader,
    #[when(header.on)]
    pub a: Option<u16>,
    #[when(header.on)]
    pub b: Option<u16>,
}"#,
        "`header.on` is a `bool`, and `b` also uses it",
    );
}

#[test]
fn message_when_unknown_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u16,
    #[when(present & 0x01)]
    pub timestamp: Option<u32>,
}"#,
        "#[when] names `present`, which is not an integer field of this message",
    );
}

#[test]
fn message_when_zero_mask() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u16,
    #[when(flags & 0)]
    pub timestamp: Option<u32>,
}"#,
        "field `timestamp`: `#[when(flags & 0)]` masks no bits. \
         Name the bit that makes the field present",
    );
}

#[test]
fn message_at_on_a_trailing_optional_mismatch() {
    refused(
        r#"#[derive(layline::Message)]
struct M {
    n: u8,
    #[at(byte = 2)]
    #[fill]
    extra: Option<u16>,
}"#,
        "field `extra`: #[at(byte = 2)] but the solver placed it at byte 1",
    );
}

#[test]
fn message_at_on_an_optional_mismatch() {
    refused(
        r#"#[derive(layline::Message)]
struct M {
    flags: u8,
    #[at(byte = 2)]
    #[when(flags & 0x01)]
    stamp: Option<u16>,
}"#,
        "field `stamp`: #[at(byte = 2)] but the solver placed it at byte 1",
    );
}

#[test]
fn message_at_on_an_optional_after_a_discovered_extent() {
    refused(
        r#"#[derive(layline::Message)]
struct M {
    flags: u8,
    #[text] #[until(0)]
    name: String,
    #[at(byte = 4)]
    #[when(flags & 0x01)]
    stamp: Option<u16>,
}"#,
        "field `stamp`: `#[at]` after a variable-length segment, whose offset is only known at run time. \
         Put `#[at]` on an earlier field",
    );
}

#[test]
fn message_fill_cap_on_a_trailing_optional() {
    refused(
        r#"#[derive(layline::Message)]
struct M {
    n: u8,
    #[fill(cap = 1)]
    extra: Option<u16>,
}"#,
        "field `extra`: an `Option` has no count to cap",
    );
}

#[test]
fn a_parameter_a_when_would_write_back() {
    refused(
        r#"#[derive(Message)]
#[message(needs(flags: u8))]
pub struct M {
    pub a: u8,
    #[when(flags & 0x01)]
    pub stamp: Option<u16>,
}"#,
        "`flags` is a parameter, and encode cannot write presence bits into it. \
         Name a field of this message",
    );
}
