//! A `layline-codegen` enum as a switch's selector: the emitted source compiles and selects.
//! `support/emitted_kind.rs` is the generator's output, and a test keeps it current.

#![cfg(feature = "derive")]

use layline::{Message, ParseError};
use layline_codegen::emit::{Module, generate};
use layline_codegen::{EnumDef, Item, Scalar, Variant};

include!("support/emitted_kind.rs");

const EMITTED: &str = "support/emitted_kind.rs";

/// The enum `support/emitted_kind.rs` holds.
fn kind() -> Module {
    Module::new(vec![Item::Enum(
        EnumDef::new("Kind", Scalar::U(8), vec![Variant::new(1, "Ping"), Variant::new(2, "Pong")])
            .with_other("Unknown"),
    )])
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
enum Body {
    #[value(1)]
    Ping(u16),
    #[value(2)]
    Pong(u32),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Frame {
    #[codec(8)]
    kind: Kind,
    #[switch(kind)]
    body: Body,
}

#[test]
fn the_committed_source_is_what_the_generator_emits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(EMITTED);
    let source = generate(&kind()).expect("the enum emits").source;
    if std::env::var_os("LAYLINE_BLESS").is_some() {
        std::fs::write(&path, &source).expect("write the emitted source");
    }
    let committed = std::fs::read_to_string(&path).expect("read the emitted source");
    assert_eq!(committed, source, "stale: rerun with LAYLINE_BLESS=1");
}

#[test]
fn an_emitted_enum_selects_the_arm() {
    for (frame, wire) in [
        (Frame { kind: Kind::Ping, body: Body::Ping(0x0102) }, vec![1, 0x02, 0x01]),
        (Frame { kind: Kind::Pong, body: Body::Pong(7) }, vec![2, 7, 0, 0, 0]),
    ] {
        assert_eq!(frame.encode(), wire, "{frame:?}");
        assert_eq!(Frame::decode(&wire), Ok((frame, wire.len())));
    }
}

#[test]
fn encode_states_the_selector_from_the_arm() {
    let frame = Frame { kind: Kind::Unknown(9), body: Body::Pong(7) };
    assert_eq!(frame.encode()[0], 2);
}

#[test]
fn a_value_the_catalogue_does_not_name_is_refused() {
    assert!(matches!(Frame::decode(&[3, 0]), Err(ParseError::Malformed { .. })));
}
