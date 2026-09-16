//! `#[checksum]` refusals.

use super::*;

#[test]
fn message_checksum_backwards_range() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u16,
    pub len: u16,
    #[checksum(wire::Crc16Ccitt, over = trailer..=len)]
    pub crc: u16,
    pub trailer: u16,
}"#,
        "the checksum range starts at `trailer` but runs through `len`, which comes first",
    );
}

#[test]
fn message_checksum_covers_itself() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u16,
    pub len: u16,
    #[checksum(wire::Crc16Ccitt, over = len..=crc)]
    pub crc: u16,
}"#,
        "field `crc`: the checksum range cannot include `crc`, the checksum itself",
    );
}

#[test]
fn message_checksum_empty_range() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    #[checksum(wire::Crc16Ccitt, over = ..)]
    pub crc: u16,
    pub magic: u16,
}"#,
        "the checksum range ends at this field, so it covers nothing",
    );
}

#[test]
fn message_checksum_end_says_the_default() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u16,
    pub len: u16,
    #[checksum(wire::Crc16Ccitt, over = len..crc)]
    pub crc: u16,
}"#,
        "the checksum range already ends at `crc` by default",
    );
}

#[test]
fn message_checksum_later_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u16,
    #[checksum(wire::Crc16Ccitt, over = trailer..)]
    pub crc: u16,
    pub trailer: u16,
}"#,
        "the checksum range starts at `trailer` but ends at `crc`, which comes first",
    );
}

#[test]
fn message_checksum_on_a_collection() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u16,
    #[checksum(wire::Crc16Ccitt, over = ..)]
    pub crc: Vec<u16>,
}"#,
        "field `crc`: #[checksum] needs one integer, not `Vec<u16>`",
    );
}

#[test]
fn message_checksum_over_an_optional_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub flags: u16,
    #[when(flags & 0x01)]
    pub stamp: Option<u32>,
    #[checksum(wire::Crc16Ccitt, over = stamp..)]
    pub crc: u16,
}"#,
        "the checksum range starts at `stamp`, which is present only when a flag says so",
    );
}

#[test]
fn message_checksum_range_through_nothing() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u16,
    #[checksum(wire::Crc16Ccitt, over = ..=)]
    pub crc: u16,
}"#,
        "#[checksum(..)] takes one form",
    );
}

#[test]
fn message_checksum_unknown_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub struct Packet {
    pub magic: u16,
    pub len: u16,
    #[count(len)]
    pub payload: Vec<u8>,
    #[checksum(wire::Crc16Ccitt, over = header..)]
    pub crc: u16,
}"#,
        "the checksum range starts at `header`, which is not a field of this message",
    );
}

#[test]
fn range_on_a_checksum() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
struct ChecksumAndRange {
    len: u8,
    #[checksum(wire::Lrc8, over = ..)]
    #[range(0..=127)]
    ck: u8,
}"#,
        "#[range] cannot be combined with #[checksum]",
    );
}
