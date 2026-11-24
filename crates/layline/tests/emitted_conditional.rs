//! `if c { a } else { b }` from `layline-codegen`'s model: a DW-shaped packet whose marker bit or
//! payload type 127 selects the described bytes, and an arm whose count is words or bytes as a
//! parameter says. The emitted source compiles and round-trips.
//! `support/emitted_conditional.rs` is the generator's output, and a test keeps it current.

#![cfg(feature = "derive")]

use layline::{EncodeError, Message, Stated};
use layline_codegen::emit::{Module, generate};
use layline_codegen::{
    Arm, ChoiceDef, Collection, Count, Discriminant, Endian, Expr, Field, Item, Kind, MessageDef,
    Op, Param, Scalar, Segment,
};

include!("support/emitted_conditional.rs");

const EMITTED: &str = "support/emitted_conditional.rs";

/// The arm a marked packet, or payload type 127, selects.
const DESCRIBED: u8 = 128;

fn u8_() -> Kind {
    Kind::Scalar(Scalar::U(8))
}

fn flag() -> Kind {
    Kind::Scalar(Scalar::ByteBool)
}

/// The module `support/emitted_conditional.rs` holds.
fn packets() -> Module {
    let selector = Expr::if_else(
        Expr::binary(
            Op::Or,
            Expr::name("marker"),
            Expr::binary(Op::Eq, Expr::name("pt"), Expr::int(127)),
        ),
        Expr::constant("DESCRIBED"),
        Expr::name("pt"),
    );
    let count = Expr::if_else(
        Expr::name("wide"),
        Expr::binary(Op::Mul, Expr::name("n"), Expr::int(2)),
        Expr::name("n"),
    );
    Module::new(vec![
        Item::Message(
            MessageDef::new(
                "Packet",
                vec![
                    Segment::Block(vec![
                        Field::new("marker", flag()),
                        Field::new("pt", u8_()),
                        Field::new("wide", flag()),
                    ]),
                    Segment::switch("payload", Discriminant::Field(selector), "Payload", None)
                        .with_context(vec!["wide".into()]),
                ],
            )
            .with_endian(Endian::Be),
        ),
        Item::Choice(
            ChoiceDef::new(
                "Payload",
                vec![
                    Arm::new(
                        1,
                        "Words",
                        vec![
                            Segment::Block(vec![Field::new("n", u8_()).with_stated()]),
                            Segment::repeat(
                                "bytes",
                                u8_(),
                                Count::Field { by: count, cap: None },
                                None,
                                Collection::Vec,
                            ),
                        ],
                    ),
                    Arm::new(
                        128,
                        "Described",
                        vec![Segment::repeat(
                            "bytes",
                            u8_(),
                            Count::Fill { cap: None },
                            None,
                            Collection::Vec,
                        )],
                    ),
                ],
            )
            .with_needs(vec![Param::new("wide", Scalar::Bool)])
            .with_endian(Endian::Be),
        ),
    ])
}

#[test]
fn the_committed_source_is_what_the_generator_emits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(EMITTED);
    let source = generate(&packets()).expect("the module emits").source;
    if std::env::var_os("LAYLINE_BLESS").is_some() {
        std::fs::write(&path, &source).expect("write the emitted source");
    }
    let committed = std::fs::read_to_string(&path).expect("read the emitted source");
    assert_eq!(committed, source, "stale: rerun with LAYLINE_BLESS=1");
}

fn words(wide: bool, bytes: &[u8]) -> Packet {
    Packet {
        marker: false,
        pt: 1,
        wide,
        payload: Payload::Words { n: Stated::default(), bytes: bytes.to_vec() },
    }
}

#[test]
fn an_emitted_selector_over_two_fields_picks_the_arm() {
    for (packet, wire) in [
        (words(false, &[7, 8]), vec![0, 1, 0, 2, 7, 8]),
        (words(true, &[7, 8]), vec![0, 1, 1, 1, 7, 8]),
        (
            Packet { marker: true, pt: 1, wide: false, payload: Payload::Described(vec![1]) },
            vec![1, 1, 0, 1],
        ),
        (
            Packet { marker: false, pt: 127, wide: true, payload: Payload::Described(vec![]) },
            vec![0, 127, 1],
        ),
    ] {
        assert_eq!(packet.encode().unwrap(), wire, "{packet:?}");
        assert_eq!(Packet::decode(&wire), Ok((packet, wire.len())));
    }
}

#[test]
fn an_emitted_conditional_count_writes_its_field_or_refuses() {
    assert_eq!(words(true, &[7]).encode(), Err(EncodeError::Inexact { field: "n", value: 1 }));
    let marked = Packet { marker: true, ..words(false, &[]) };
    assert_eq!(
        marked.encode(),
        Err(EncodeError::Mismatch { field: "payload", value: 1, stated: 128 })
    );
}
