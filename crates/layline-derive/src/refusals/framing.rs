//! Framing refusals: a prefix is the number itself, and a `Stated<T>` is written by an attribute.

use super::*;

fn message(fields: &str) -> String {
    format!(
        "use layline::{{Message, Stated}};\n\n#[derive(Message)]\npub struct M {{\n{fields}\n}}"
    )
}

#[test]
fn a_stated_field_nothing_writes() {
    refused(
        &message("pub n: Stated<u8>,\npub tag: u16,"),
        "field `n`: is `Stated`, and no attribute writes it",
    );
    refused(
        &message("pub n: Stated<u8>,\npub m: u8,\n#[count(n * m)]\npub items: Vec<u8>,"),
        "field `n`: is `Stated`, and no attribute writes it",
    );
}

#[test]
fn a_stated_field_is_one_number_or_flag() {
    refused(
        &message("pub n: Stated<[u8; 2]>,\n#[count(n)]\npub items: Vec<u8>,"),
        "`Stated<T>` is a number or flag encode writes",
    );
    refused(
        &message("#[count(prefix = u8)]\npub n: Stated<Vec<u8>>,"),
        "#[count] does not apply to `Stated<T>`",
    );
    refused(
        &message("pub f: u8,\n#[when(f & 1)]\npub n: Stated<Option<u8>>,"),
        "field `n`: `Option<u8>` has no known size",
    );
    refused(
        &message("#[present]\npub n: Stated<u8>,\n#[count(n)]\npub items: Vec<u8>,"),
        "it has no flag of its own: `#[when(c)]` puts it on the wire when `c` holds",
    );
}

#[test]
fn a_stated_field_in_a_bit_message() {
    refused(
        "use layline::{Message, Stated};\n\n#[derive(Message)]\n#[message(bits)]\npub struct M {\n#[bits(8)]\npub n: Stated<u8>,\n}",
        "field `n`",
    );
}

#[test]
fn a_prefix_and_an_expression() {
    refused(
        &message("pub n: u8,\n#[len(n, prefix = u8)]\npub data: Vec<u8>,"),
        "#[len] takes an expression or a `prefix`, not both",
    );
}

#[test]
fn a_prefix_that_is_not_a_whole_byte_integer() {
    for ty in ["f32", "bool", "U<4>", "u128"] {
        refused(
            &message(&format!("#[count(prefix = {ty})]\npub items: Vec<u8>,")),
            "`prefix` is the integer type just before the value",
        );
    }
}

#[test]
fn a_count_prefix_on_a_string() {
    refused(
        &message("#[count(prefix = u8)]\n#[text]\npub name: String,"),
        "#[count] does not apply to a string. Write `#[len(prefix = ..)]`",
    );
}

#[test]
fn an_optional_collection_sized_by_another_field() {
    refused(
        &message("pub n: u8,\n#[present]\n#[count(n)]\npub items: Option<Vec<u8>>,"),
        "an optional collection states its own size in place",
    );
}

#[test]
fn present_and_when_together() {
    refused(
        &message("pub f: u8,\n#[present]\n#[when(f & 1)]\npub x: Option<u8>,"),
        "`#[present]` and `#[when]` both gate this field",
    );
}

#[test]
fn a_prefix_is_a_row_of_its_own_and_no_field() {
    let got = expansion(&message("#[len(prefix = u16)]\n#[text]\npub name: String,"));
    assert!(got.contains(r#"SegmentDef::new("name.len",::layline::table::Start::At(0),::layline::table::Span::Fixed(16)"#), "{got}");
    assert!(got.contains(r#"By::field("name.len")"#), "{got}");
    assert!(got.contains("Self{name}"), "{got}");
}

#[test]
fn a_field_after_the_tail() {
    refused(
        &message("#[fill]\npub a: Vec<u8>,\n#[at(end)]\npub b: u8,\npub c: u8,"),
        "field `c`: it follows `b`, which is anchored to the end with `#[at(end)]`",
    );
}

#[test]
fn a_tail_field_that_reads_its_own_size() {
    refused(
        &message("#[fill]\npub a: Vec<u8>,\n#[at(end)]\n#[len(prefix = u8)]\npub b: Vec<u8>,"),
        "a collection in the tail is counted or sized",
    );
}
