//! Expression refusals: every invertibility and type decision is made when the type is built.

use super::*;

fn message(fields: &str) -> String {
    format!(
        "use layline::{{Layout, Message}};\n\n#[derive(Message)]\npub struct M {{\n{fields}\n}}"
    )
}

#[test]
fn a_check_cannot_read_a_number_encode_writes_later() {
    refused(
        r#"use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Rec {
    pub tag: u16,
}

#[derive(Message)]
pub struct M {
    pub at: u8,
    pub len: u8,
    pub n: u8,
    #[count(n * len)]
    pub items: Vec<u8>,
    #[seek(at)]
    #[when(len > 0)]
    #[bytes(2)]
    pub rec: Option<Rec>,
}"#,
        "an expression reads `len`, which encode fills in only after writing `rec`",
    );
}

#[test]
fn a_count_reads_integers() {
    refused(
        &message("pub on: bool,\n#[count(on * 2)]\npub items: Vec<u8>,"),
        "needs an integer, and `on` is a bool",
    );
}

#[test]
fn a_number_reads_a_field() {
    refused(
        &message("#[count(4)]\npub items: Vec<u8>,"),
        "field `items`: #[count] reads no field or parameter, so `4` is a fixed number",
    );
}

#[test]
fn scale_and_offset_moved_into_the_expression() {
    refused(
        &message("pub n: u8,\n#[count(n, scale = 2)]\npub items: Vec<u8>,"),
        "field `items`: `scale` is part of the expression now. Write `#[count(n * 2)]`",
    );
    refused(
        &message("pub n: u8,\n#[len(n, offset = -4)]\npub items: Vec<u8>,"),
        "field `items`: `offset` is part of the expression now. Write `#[len(n - 4)]`",
    );
}

#[test]
fn a_condition_is_a_bool_or_a_bit_test() {
    refused(
        &message("pub on: bool,\npub n: u8,\n#[when(on && n + 1)]\npub x: Option<u8>,"),
        "`n + 1` is an integer, and `on && n + 1` needs a condition there",
    );
    refused(&message("pub n: u8,\n#[valid(n + 1)]\npub m: u8,"), "`n + 1` is an integer");
}

#[test]
fn an_expression_must_parse() {
    refused(
        &message("pub n: u8,\n#[count(n +)]\npub items: Vec<u8>,"),
        "field `items`: #[count] takes an expression",
    );
    refused(&message("pub n: u8,\n#[count(n < 2 < 3)]\npub items: Vec<u8>,"), "chains comparisons");
}

#[test]
fn an_expression_reads_earlier_fields() {
    refused(
        &message("pub a: u8,\n#[count(a * b)]\npub items: Vec<u8>,\npub b: u8,"),
        "field `items`: #[count] names `b`, which comes after this field",
    );
}

#[test]
fn a_comparison_is_like_with_like() {
    refused(
        &message("pub on: bool,\npub n: u8,\n#[when(on == n)]\npub x: Option<u8>,"),
        "compares a `bool` with an integer",
    );
}

#[test]
fn a_conditional_gives_one_kind_and_tests_a_condition() {
    refused(
        &message(
            "pub on: bool,\npub n: u8,\n#[count(if on { n } else { on })]\npub items: Vec<u8>,",
        ),
        "gives an integer one way and a `bool` the other",
    );
    refused(
        &message("pub n: u8,\n#[count(if n { n } else { 1 })]\npub items: Vec<u8>,"),
        "`n` is an integer, and `if n { n } else { 1 }` needs a condition there",
    );
    refused(
        &message("pub on: bool,\npub n: u8,\n#[count(if on { n })]\npub items: Vec<u8>,"),
        "`if on { n }` has no `else`",
    );
}

#[test]
fn a_real_reaches_a_number_only_through_as() {
    refused(
        &message("pub size: f64,\n#[count(size)]\npub items: Vec<u8>,"),
        "needs an integer, and `size` is a float. Write `size as i64`, or name an earlier integer",
    );
    refused(
        &message("pub size: f64,\n#[count(size * 2.0)]\npub items: Vec<u8>,"),
        "`size * 2.0` is a real, and an integer is needed here. Write `(size * 2.0) as i64`",
    );
    refused(
        &message(
            "pub size: f64,\npub n: u8,\n#[count(n + size as i64 + 1.5)]\npub items: Vec<u8>,",
        ),
        "mixes an integer and a real. Convert one",
    );
    refused(
        &message("pub size: f64,\n#[valid(size)]\npub m: u8,"),
        "`size` is a real, and a condition is needed here",
    );
    refused(
        &message("pub on: bool,\n#[count(on as i64)]\npub items: Vec<u8>,"),
        "converts a `bool`, which has no number",
    );
    refused(
        &message("pub n: u8,\n#[count(n as u16)]\npub items: Vec<u8>,"),
        "`as` makes an integer",
    );
}
