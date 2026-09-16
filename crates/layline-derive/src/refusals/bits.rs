//! `#[message(bits)]` refusals.

use super::*;

/// A `#[message(bits)]` struct with `field` appended.
fn bits_message(field: &str) -> String {
    format!(
        r#"#[derive(Message)]
#[message(bits, order = msb)]
pub struct Header {{
    #[bits(4)]
    pub version: u8,
{field}
}}"#
    )
}

#[test]
fn the_byte_addressed_vocabulary_is_refused_on_a_bit_field() {
    let cases = [
        (
            "#[count(version)] pub items: Vec<u8>,",
            "`#[count]` does not apply in a bit-addressed message",
        ),
        (
            "#[len(version)] pub items: Vec<u8>,",
            "`#[len]` does not apply in a bit-addressed message",
        ),
        ("#[fill] pub items: Vec<u8>,", "`#[fill]` does not apply in a bit-addressed message"),
        ("#[until(0)] pub items: Vec<u8>,", "`#[until]` does not apply in a bit-addressed message"),
        (
            "#[stride(version)] pub items: Vec<u8>,",
            "`#[stride]` does not apply in a bit-addressed message",
        ),
        ("#[seek(version)] pub at: u8,", "`#[seek]` does not apply in a bit-addressed message"),
        (
            "#[switch(version)] pub body: Body,",
            "`#[switch]` does not apply in a bit-addressed message",
        ),
        ("#[text] pub label: String,", "`#[text]` does not apply in a bit-addressed message"),
        ("#[message] pub inner: Inner,", "`#[message]` does not apply in a bit-addressed message"),
        (
            "#[checksum(Crc16Ccitt, over = ..)] pub crc: u16,",
            "`#[checksum]` does not apply in a bit-addressed message",
        ),
        (
            "#[magic(0xFE)] #[bits(8)] pub sync: u8,",
            "`#[magic]` does not apply in a bit-addressed message",
        ),
        (
            "#[at(byte = 1)] #[bits(8)] pub pos: u8,",
            "`#[at]` does not apply in a bit-addressed message",
        ),
        ("#[bytes(2)] pub nested: Inner,", "`#[bytes]` does not apply in a bit-addressed message"),
        ("#[codec(8)] pub tag: Tag,", "`#[codec]` does not apply in a bit-addressed message"),
        (
            "#[range(0..=3)] #[bits(4)] pub level: u8,",
            "`#[range]` does not apply in a bit-addressed message",
        ),
    ];
    for (field, reason) in cases {
        refused(&bits_message(field), reason);
    }
}

#[test]
fn a_var_field_of_a_bits_message_reads_its_own_width() {
    refused(
        &bits_message("#[var] #[bits(4)] pub n: ExpGolomb,"),
        "`#[var]` and `#[bits(4)]` both set the width",
    );
    refused(
        &bits_message("#[var] pub n: Vec<u8>,"),
        "`#[var]` needs a `BitCodec` type, not `Vec<u8>`",
    );
    refused(&bits_message("#[var] #[var] pub n: ExpGolomb,"), "duplicate #[var] attribute");
}

#[test]
fn one_bit_field_is_on_the_wire_under_one_test() {
    refused(
        &bits_message("#[present] #[when(version & 0x1)] #[bits(4)] pub extra: Option<u8>,"),
        "`#[present]` and `#[when]` both gate this field",
    );
    refused(
        &bits_message("#[when(version & 0x1)] #[bits(4)] pub extra: u8,"),
        "`#[when]` needs an `Option` field",
    );
    refused(
        &bits_message("#[when(version > 0)] #[bits(4)] pub extra: Option<u8>,"),
        "`#[when(version > 0)]` works only on a `#[seek]` record",
    );
    refused(
        &bits_message("#[when(version & 0)] #[bits(4)] pub extra: Option<u8>,"),
        "`#[when(version & 0)]` masks no bits",
    );
}

#[test]
fn a_bit_flag_is_an_earlier_field_of_solved_width_that_is_always_there() {
    refused(
        &bits_message(
            "#[when(later & 0x1)] #[bits(4)] pub extra: Option<u8>, #[bits(4)] pub later: u8,",
        ),
        "`later` is not a field this message reads before `extra`",
    );
    refused(
        &bits_message(
            "#[present] #[bits(4)] pub absent: Option<u8>, #[when(absent & 0x1)] #[bits(4)] pub extra: Option<u8>,",
        ),
        "the flag `absent` is itself sometimes absent",
    );
    refused(
        &bits_message(
            "#[var] pub n: ExpGolomb, #[when(n & 0x1)] #[bits(4)] pub extra: Option<u8>,",
        ),
        "the flag `n` reads its width off the wire",
    );
    refused(
        &bits_message("#[when(version & 0x10)] #[bits(4)] pub extra: Option<u8>,"),
        "the mask reaches bit 4 of `version`, which is 4 bits wide",
    );
    refused(
        &bits_message("pub urgent: bool, #[when(urgent & 0x1)] #[bits(4)] pub extra: Option<u8>,"),
        "`urgent` is a bool, and a mask tests the bits of an integer",
    );
    refused(
        &bits_message("#[when(version)] #[bits(4)] pub extra: Option<u8>,"),
        "`version` is an integer and a bare test reads a `bool`",
    );
    refused(
        &bits_message(
            "#[when(version & 0x3)] #[bits(2)] pub first: Option<u8>, #[when(version & 0x2)] #[bits(4)] pub extra: Option<u8>,",
        ),
        "bit 1 of `version` already marks `first` present",
    );
    refused(
        &bits_message(
            "pub urgent: bool, #[when(urgent)] #[bits(2)] pub first: Option<u8>, #[when(urgent)] #[bits(4)] pub extra: Option<u8>,",
        ),
        "bit 0 of `urgent` already marks `first` present",
    );
}

#[test]
fn present_outside_a_bits_message() {
    refused(
        r#"#[derive(Message)]
pub struct Packet {
    pub flags: u8,
    #[present]
    pub extra: Option<u16>,
}"#,
        "`#[present]` needs a bit-addressed message",
    );
}

#[test]
fn present_on_a_field_that_is_always_there() {
    refused(
        &bits_message("#[present] #[bits(4)] pub extra: u8,"),
        "`#[present]` needs an `Option` field",
    );
}

#[test]
fn an_option_in_a_bits_message_without_a_presence_bit() {
    refused(
        &bits_message("#[bits(4)] pub extra: Option<u8>,"),
        "nothing says whether this `Option` is present",
    );
}

#[test]
fn a_bit_field_whose_type_carries_no_width() {
    refused(&bits_message("pub level: u8,"), "`u8` needs a width in a bit-addressed message");
    refused(&bits_message("pub tag: Tag,"), "`Tag` needs a width");
    refused(&bits_message("pub label: String,"), "`String` cannot be a bit field");
}

#[test]
fn a_bit_field_wider_than_the_cursor() {
    refused(
        &bits_message("#[bits(96)] pub wide: Wide,"),
        "#[bits(96)] is wider than a bit field, which holds 64 bits",
    );
    refused(
        &bits_message("#[bits(12)] pub narrow: u8,"),
        "#[bits(12)] is wider than `u8`, which holds 8 bits",
    );
}

#[test]
fn endian_on_a_bits_message() {
    refused(
        r#"#[derive(Message)]
#[message(bits, endian = be)]
pub struct Header {
    #[bits(4)]
    pub version: u8,
}"#,
        "a bit-addressed message has no byte order",
    );
}

#[test]
fn a_bits_message_with_no_fields() {
    refused(
        r#"#[derive(Message)]
#[message(bits)]
pub struct Header {}"#,
        "a message must have at least one field",
    );
}

#[test]
fn bits_on_a_catalogue_of_arms() {
    refused(
        r#"#[derive(Message)]
#[message(bits)]
pub enum Body {
    #[value(0)]
    Ping(u8),
}"#,
        "`bits` does not apply to an enum",
    );
}

#[test]
fn a_bit_field_of_no_bits() {
    refused(
        &bits_message("#[bits(0)] pub nothing: u8,"),
        "#[bits(0)] is empty. A field needs at least one bit",
    );
}

#[test]
fn needs_on_a_bit_addressed_message() {
    refused(
        r#"#[derive(Message)]
#[message(bits, needs(stride: u8))]
pub struct M {
    #[bits(4)]
    pub a: u8,
    #[bits(4)]
    pub b: u8,
}"#,
        "`M`: a bit-addressed message cannot take `needs`",
    );
}
