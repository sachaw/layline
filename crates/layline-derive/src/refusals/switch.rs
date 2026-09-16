//! `#[switch]` refusals.

use super::*;

#[test]
fn switch_footprint_on_length() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
#[message(closed)]
pub enum Form {
    #[value(2)]
    Short(u16),
    #[value(4)]
    Long(u32),
}

#[derive(Message)]
pub struct Reading {
    #[switch(..)]
    #[bytes(4)]
    pub form: Form,
    pub trailer: u8,
}"#,
        "field `form`: #[switch(..)] reads the remaining length, and #[bytes(4)] fixes it",
    );
}

#[test]
fn switch_footprint_zero() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
#[message(closed)]
pub enum Body {
    #[value(0)]
    Level(u8),
}

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[switch(kind)]
    #[bytes(0)]
    pub body: Body,
}"#,
        "#[bytes(0)] gives the union no bytes",
    );
}

#[test]
fn switch_on_length_not_last() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
pub enum Body {
    #[value(4)]
    Short(u32),
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Message)]
pub struct Packet {
    pub seq: u8,
    #[switch(..)]
    pub body: Body,
    pub trailer: u16,
}"#,
        "#[switch(..)] must be the last field, and `trailer` comes after it",
    );
}

#[test]
fn switch_by_length_with_a_window() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
#[message(closed)]
pub enum Form {
    #[value(2)]
    Short(u16),
    #[value(4)]
    Long(u32),
}

#[derive(Message)]
pub struct Reading {
    pub n: u8,
    #[switch(..)]
    #[len(n)]
    pub form: Form,
}"#,
        "field `form`: #[switch(..)] reads the remaining length, and #[len(n)] sets it",
    );
}

#[test]
fn switch_with_a_count() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
#[message(closed)]
pub enum Body {
    #[value(0)]
    Level(u8),
}

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    pub n: u8,
    #[switch(kind)]
    #[count(n)]
    pub body: Body,
}"#,
        "field `body`: #[switch] cannot take #[count]",
    );
}

#[test]
fn switch_with_a_window_and_a_footprint() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
#[message(closed)]
pub enum Body {
    #[value(0)]
    Level(u8),
}

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    pub n: u8,
    #[switch(kind)]
    #[len(n)]
    #[bytes(1)]
    pub body: Body,
}"#,
        "field `body`: #[len(n)] and #[bytes(1)] both set the union's size",
    );
}

#[test]
fn switch_window_field_claimed_twice() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
#[message(closed)]
pub enum Body {
    #[value(0)]
    Level(u8),
}

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    pub n: u8,
    #[count(n)]
    pub items: Vec<u8>,
    #[switch(kind)]
    #[len(n)]
    pub body: Body,
}"#,
        "`n` is the length of a collection and also the byte length of a switch",
    );
}

#[test]
fn switch_window_names_a_later_field() {
    refused(
        r#"use layline::Message;

#[derive(Message)]
#[message(closed)]
pub enum Body {
    #[value(0)]
    Level(u8),
}

#[derive(Message)]
pub struct Packet {
    pub kind: u8,
    #[switch(kind)]
    #[len(n)]
    pub body: Body,
    pub n: u8,
}"#,
        "#[len]",
    );
}
