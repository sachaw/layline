//! `Layout` refusals.

use super::*;

#[test]
fn at_mismatch() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 16)]
pub struct At {
    #[bits(2)]
    pub a: u8,
    #[bits(14)]
    #[at(bit = 3)]
    pub b: u16,
}"#,
        "#[at(bit = 3)] but the solver placed it at bit 2",
    );
}

#[test]
fn at_mismatch_under_msb_names_both_numberings() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 32, endian = be, order = msb)]
pub struct Can {
    #[bits(29)]
    #[at(bit = 0)]
    pub id: u32,
    #[bits(1)]
    #[at(bit = 2)]
    pub ide: bool,
    #[bits(1)]
    pub rtr: bool,
    #[bits(1)]
    pub err: bool,
}"#,
        "field `ide`: #[at(bit = 2)] (declared numbering, msb) is physical bit 29, but the \
         solver placed it at physical bit 2 — declared bit 29",
    );
}

#[test]
fn bits_in_byte_mode() {
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

#[derive(layline::Layout)]
#[layout(bytes = 1)]
struct Byte {
    #[bits(8)]
    level: Level,
}

#[derive(layline::Message)]
struct Msg {
    #[bits(8)]
    level: Level,
}"#,
        "field `level`: byte mode has no bit fields. \
         Write `#[codec(N)]` for a field narrower than its type",
    );
}

#[test]
fn bool_width() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 8)]
pub struct B {
    #[bits(2)]
    pub flag: bool,
    #[bits(6)]
    pub rest: u8,
}"#,
        "field `flag`: a `bool` is one bit, not #[bits(2)]",
    );
}

#[test]
fn bytes_tiling_short() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bytes = 16)]
pub struct Short {
    pub a: u32,
    pub b: f32,
}"#,
        "`Short`: declares 16 bytes but its fields end at byte 8. \
         Add 8 bytes of reserved `[u8; N]` fields after `b`",
    );
}

#[test]
fn checksum_cycle() {
    refused(
        r#"#[derive(layline::Message)]
struct Cycle {
    body: u16,
    #[checksum(wire::Crc16Ccitt, over = ..=b)]
    a: u16,
    #[checksum(wire::Crc16Ccitt, over = ..=a)]
    b: u16,
}"#,
        "checksums `a` and `b` cover each other. \
         Narrow one range so it does not reach the other",
    );
}

#[test]
fn layout_checksum_covers_a_later_checksum() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bytes = 6, endian = le)]
pub struct Backwards {
    pub body: u16,
    #[checksum(wire::Crc16Ccitt, over = ..=second)]
    pub first: u16,
    #[checksum(wire::Crc16Ccitt, over = body..)]
    pub second: u16,
}"#,
        "field `first`: the checksum range covers `second`, a later checksum",
    );
}

#[test]
fn layout_magic_in_bit_mode() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 16, order = msb)]
pub struct Word {
    #[bits(8)]
    #[magic(0xAA)]
    pub sig: u8,
    #[bits(8)]
    pub rest: u8,
}"#,
        "field `sig`: #[magic] needs whole bytes, and a bit-addressed layout has none. \
         Use `#[layout(bytes = N)]`",
    );
}

#[test]
fn layout_magic_not_a_literal() {
    refused(
        r#"use layline::Layout;

const SIGNATURE: u16 = 0xAA55;

#[derive(Layout)]
#[layout(bytes = 2, endian = le)]
pub struct BootTail {
    #[magic(SIGNATURE)]
    pub signature: u16,
}"#,
        "field `signature`: #[magic(..)] takes one literal. \
         Write a byte string such as `b\"SEAL\"`, or an integer such as `0xAA55`",
    );
}

#[test]
fn layout_magic_on_a_codec_field() {
    refused(
        r#"use layline::{FieldCodec, Layout};

#[derive(FieldCodec)]
#[bits(8)]
pub struct Mode(u8);

#[derive(Layout)]
#[layout(bytes = 1)]
pub struct Header {
    #[codec(8)]
    #[magic(0x01)]
    pub mode: Mode,
}"#,
        "field `mode`: #[magic] needs an integer scalar (`u8`..`u64`, `i8`..`i64`) or a `[u8; N]`",
    );
}

#[test]
fn layout_magic_too_wide() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bytes = 1)]
pub struct Tag {
    #[magic(0x1FF)]
    pub tag: u8,
}"#,
        "#[magic(511)] does not fit 1 byte",
    );
}

#[test]
fn layout_magic_with_view() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bytes = 4, endian = le, view)]
pub struct Header {
    #[magic(b"AB")]
    pub sync: [u8; 2],
    pub rest: u16,
}"#,
        "`Header`: `view` cannot check the #[magic] on `sync`. Remove `view` or the #[magic]",
    );
}

#[test]
fn narrow_bits_disagree() {
    refused(
        r#"use layline::{Layout, U};

#[derive(Layout)]
#[layout(bits = 8)]
pub struct Header {
    #[bits(5)]
    pub version: U<4>,
    #[bits(3)]
    pub rest: u8,
}"#,
        "#[bits(5)] disagrees with `U<4>`, which is 4 bits",
    );
}

#[test]
fn stated_run_disagrees() {
    refused(
        r#"#[derive(layline::Message)]
struct Wrong {
    n: u8,
    #[at(byte = 4)]
    #[count(n)]
    items: Vec<u16>,
}"#,
        "field `items`: #[at(byte = 4)] but the solver placed it at byte 1",
    );
}

#[test]
fn tiling_overflow() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 16)]
pub struct Overflow {
    #[bits(10)]
    pub a: u16,
    #[bits(10)]
    pub b: u16,
}"#,
        "field `b`: bits 10..20 overflow the 16-bit layout",
    );
}

#[test]
fn tiling_short() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 16)]
pub struct Short {
    #[bits(2)]
    pub a: u8,
    #[bits(11)]
    pub b: u16,
}"#,
        "`Short`: declares 16 bits but its fields end at bit 13. \
         Add 3 bits of named spare fields after `b`",
    );
}

#[test]
fn word_bits_not_bytes() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 70)]
pub struct Ragged {
    #[bits(35)]
    pub a: u64,
    #[bits(35)]
    pub b: u64,
}"#,
        "`bits = 70` is not a whole number of bytes",
    );
}

#[test]
fn word_field_wider_than_accessor() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 512)]
pub struct Opaque {
    #[bits(200)]
    pub blob: u128,
    #[bits(312)]
    pub rest: u128,
}"#,
        "#[bits(200)] is wider than 128 bits",
    );
}

#[test]
fn layout_field_attribute_on_the_container() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bytes = 2)]
#[range(0..=9)]
pub struct L {
    pub a: u16,
}"#,
        "`L`: `#[range]` belongs on a field",
    );
}

#[test]
fn layout_container_attribute_on_a_field() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bytes = 2)]
pub struct L {
    #[layout(bytes = 2)]
    pub a: u16,
}"#,
        "field `a`: `#[layout]` belongs above the struct",
    );
}

#[test]
fn words_mode_codec_refused() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(words = 1)]
pub struct W {
    #[codec(16)]
    #[bits(16)]
    pub a: u16,
}"#,
        "field `a`: `#[codec(N)]` does not apply in a `words = N` layout",
    );
}

#[test]
fn layout_refuses_the_walk_vocabulary_by_name() {
    for word in [
        "count(n)",
        "len(n)",
        "seek(n)",
        "fill",
        "text",
        "var",
        "message",
        "when(n)",
        "switch(n)",
        "until(0)",
        "stride(n)",
    ] {
        refused(
            &format!(
                r#"use layline::Layout;

#[derive(Layout)]
#[layout(bytes = 2)]
pub struct W {{
    pub n: u8,
    #[{word}]
    pub items: u8,
}}"#
            ),
            "is not supported in a `bytes = N` layout",
        );
    }
}

#[test]
fn layout_refuses_the_walk_vocabulary_on_the_container() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bytes = 1)]
#[message]
pub struct W {
    pub n: u8,
}"#,
        "`W`: `#[message]` is not supported on a layout",
    );
}

#[test]
fn prefix_value_needs_a_prefix() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 16, prefix_value = 0b10)]
pub struct W {
    #[bits(16)]
    pub a: u16,
}"#,
        "`W`: `prefix_value` needs a prefix. Add `prefix = K`",
    );
}

#[test]
fn prefix_value_must_fit_the_prefix() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bits = 16, prefix = 2, prefix_value = 0b100)]
pub struct W {
    #[bits(14)]
    pub a: u16,
}"#,
        "`W`: `prefix_value = 0b100` does not fit the 2-bit prefix",
    );
}

#[test]
fn prefix_is_word_mode_only() {
    refused(
        r#"use layline::Layout;

#[derive(Layout)]
#[layout(bytes = 2, prefix = 3)]
pub struct W {
    pub a: u16,
}"#,
        "`W`: `prefix` only applies to a `bits = N` layout",
    );
}

#[test]
fn a_view_cannot_check_a_range() {
    for (layout, field) in [
        ("bytes = 1", "a: u8"),
        ("bits = 8", "#[bits(8)] a: u8"),
        ("words = 1", "#[bits(8)] a: u8"),
    ] {
        refused(
            &format!(
                "#[derive(Layout)] #[layout({layout}, view)] struct V {{ #[range(0..=5)] {field} }}"
            ),
            "`V`: `view` cannot check the #[range] on `a`",
        );
    }
}

#[test]
fn an_overlay_names_a_method_of_its_own() {
    let word = |a: &str, b: &str| {
        format!(
            "#[derive(Layout)] #[layout(bits = 8)] struct W {{ \
             #[bits(4)] {a} a: u8, #[bits(4)] {b} b: u8 }}"
        )
    };
    let cases = [
        (word("#[overlay(decode: Nib)]", ""), "overlay `decode`: writes `fn decode`"),
        (word("#[overlay(encode: Nib)]", ""), "overlay `encode`: writes `fn encode`"),
        (word("#[overlay(x: Nib)] #[overlay(x: Nib)]", ""), "overlay `x`: writes `fn set_x`"),
        (word("#[overlay(x: Nib)]", "#[overlay(x: Nib)]"), "overlay `x`: writes `fn set_x`"),
        (
            word("#[overlay(x: Nib)]", "#[overlay(set_x: Nib)]"),
            "overlay `set_x`: writes `fn set_x`",
        ),
        (
            String::from(
                "#[derive(Layout)] #[layout(words = 1)] struct W { \
                 #[bits(8)] #[overlay(decode: Nib)] a: u8, #[bits(8)] b: u8 }",
            ),
            "overlay `decode`: writes `fn decode`, which `W` already has",
        ),
    ];
    for (src, reason) in cases {
        refused(&src, reason);
    }
}

#[test]
fn prefix_value_is_word_mode_only() {
    for layout in ["bytes = 1", "words = 1"] {
        refused(
            &format!(
                "#[derive(Layout)] #[layout({layout}, prefix_value = 5)] struct W {{ a: u8 }}"
            ),
            "`W`: `prefix_value` only applies to a `bits = N` layout",
        );
    }
}
