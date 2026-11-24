//! `#[with]` values that are expressions, from `layline-codegen`'s model: Kaitai's
//! `compound_ext((extension_len - 1) - len)` passes a computed count to a nested message, and a
//! switch passes a scaled length and a tested bit to its arm. The emitted source compiles and
//! round-trips. `support/emitted_passed.rs` is the generator's output, and a test keeps it
//! current.

#![cfg(feature = "derive")]

use layline::{EncodeError, Message};
use layline_codegen::emit::{Module, generate};
use layline_codegen::{
    Arm, ChoiceDef, Collection, Count, Discriminant, Expr, Field, Item, Kind, MessageDef, Param,
    Scalar, Segment,
};

include!("support/emitted_passed.rs");

const EMITTED: &str = "support/emitted_passed.rs";

fn u8_() -> Kind {
    Kind::Scalar(Scalar::U(8))
}

fn parse(e: &str) -> Expr {
    Expr::parse(e).expect("an expression")
}

/// A message of `n` elements, `n` its holder's.
fn counted(name: &str, element: Kind, needs: Vec<Param>) -> MessageDef {
    MessageDef::new(
        name,
        vec![Segment::repeat(
            "items",
            element,
            Count::Field { by: Expr::name("n"), cap: None },
            None,
            Collection::Vec,
        )],
    )
    .with_needs(needs)
}

/// The module `support/emitted_passed.rs` holds.
fn passed() -> Module {
    Module::new(vec![
        Item::Message(MessageDef::new(
            "Extension",
            vec![
                Segment::Block(vec![Field::new("extension_len", u8_()), Field::new("len", u8_())]),
                Segment::value(
                    "compound",
                    Kind::Msg {
                        ty: "Compound".into(),
                        boxed: false,
                        with: vec![parse("extension_len - 1 - len")],
                    },
                ),
            ],
        )),
        Item::Message(counted("Compound", u8_(), vec![Param::new("n", Scalar::U(8))])),
        Item::Message(MessageDef::new(
            "Frame",
            vec![
                Segment::Block(vec![
                    Field::new("kind", u8_()),
                    Field::new("flags", u8_()),
                    Field::new("words", u8_()),
                ]),
                Segment::switch("body", Discriminant::Field(Expr::name("kind")), "Body", None)
                    .with_context(vec![parse("words * 4"), parse("flags & 0x80 != 0")]),
            ],
        )),
        Item::Choice(
            ChoiceDef::new(
                "Body",
                vec![Arm::new(
                    1,
                    "Samples",
                    vec![Segment::value(
                        "inner",
                        Kind::Msg {
                            ty: "Samples".into(),
                            boxed: false,
                            with: vec![parse("bytes / 2"), Expr::name("big")],
                        },
                    )],
                )],
            )
            .with_needs(vec![Param::new("bytes", Scalar::U(16)), Param::new("big", Scalar::Bool)])
            .with_endian_from("big"),
        ),
        Item::Message(
            counted(
                "Samples",
                Kind::Scalar(Scalar::U(16)),
                vec![Param::new("n", Scalar::U(16)), Param::new("big", Scalar::Bool)],
            )
            .with_endian_from("big"),
        ),
    ])
}

#[test]
fn the_committed_source_is_what_the_generator_emits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(EMITTED);
    let source = generate(&passed()).expect("the module emits").source;
    if std::env::var_os("LAYLINE_BLESS").is_some() {
        std::fs::write(&path, &source).expect("write the emitted source");
    }
    let committed = std::fs::read_to_string(&path).expect("read the emitted source");
    assert_eq!(committed, source, "stale: rerun with LAYLINE_BLESS=1");
}

fn extension(extension_len: u8, len: u8, items: &[u8]) -> Extension {
    Extension { extension_len, len, compound: Compound { items: items.to_vec() } }
}

#[test]
fn an_emitted_holder_passes_a_computed_count() {
    let ext = extension(5, 2, &[7, 8]);
    let wire = ext.encode().unwrap();
    assert_eq!(wire, [5, 2, 7, 8]);
    assert_eq!(Extension::decode(&wire), Ok((ext, 4)));
    assert_eq!(
        extension(5, 1, &[7, 8]).encode(),
        Err(EncodeError::Mismatch { field: "items", value: 2, stated: 3 })
    );
    assert_eq!(
        extension(1, 1, &[]).encode(),
        Err(EncodeError::Unfit { field: "compound", value: -1, bits: 0 })
    );
    assert_eq!(
        Extension::decode(&[1, 1]),
        Err(ParseError::Malformed { field: "extension_len", at: 2 })
    );
}

#[test]
fn an_emitted_switch_passes_a_scaled_length_and_a_tested_bit() {
    for (flags, one) in [(0x80, [0x00, 0x01]), (0x00, [0x01, 0x00])] {
        let frame =
            Frame { kind: 1, flags, words: 1, body: Body::Samples(Samples { items: vec![1, 2] }) };
        let wire = frame.encode().unwrap();
        assert_eq!(wire[3..5], one);
        assert_eq!(Frame::decode(&wire), Ok((frame, wire.len())));
    }
}
