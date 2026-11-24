//! An object that holds itself, from `layline-codegen`'s model, as OpenWire's does: the
//! emitted source compiles, its constants are searched across the cycle, and `depth` bounds it.
//! `support/emitted_nested.rs` is the generator's output, and a test keeps it current.

#![cfg(feature = "derive")]

use layline::{Message, Stated};
use layline_codegen::emit::{Module, generate};
use layline_codegen::{
    Arm, ChoiceDef, Collection, Count, Discriminant, Endian, Expr, Field, Item, Kind, MessageDef,
    Presence, Scalar, Segment,
};

include!("support/emitted_nested.rs");

const EMITTED: &str = "support/emitted_nested.rs";

fn held(ty: &str, boxed: bool) -> Kind {
    Kind::Msg { ty: ty.into(), boxed, with: Vec::new() }
}

/// The module `support/emitted_nested.rs` holds.
fn objects() -> Module {
    let be = |m: MessageDef| Item::Message(m.with_endian(Endian::Be));
    Module::new(vec![
        be(MessageDef::new(
            "Object",
            vec![
                Segment::Block(vec![Field::new("kind", Kind::Scalar(Scalar::U(8))).with_stated()]),
                Segment::switch("body", Discriminant::Field(Expr::name("kind")), "Body", None),
            ],
        )
        .with_depth(16)),
        Item::Choice(
            ChoiceDef::new(
                "Body",
                vec![
                    Arm::new(
                        110,
                        "MessageId",
                        vec![Segment::value("inner", held("MessageId", true))],
                    ),
                    Arm::new(
                        120,
                        "ConnectionId",
                        vec![Segment::Block(vec![Field::new(
                            "value",
                            Kind::Scalar(Scalar::I(64)),
                        )])],
                    ),
                ],
            )
            .with_endian(Endian::Be),
        ),
        be(MessageDef::new(
            "Nested",
            vec![Segment::opt(Presence::Prefix, Field::new("object", held("Object", true)))],
        )),
        be(MessageDef::new(
            "NullArray",
            vec![
                Segment::repeat(
                    "items",
                    held("Nested", false),
                    Count::Prefix { repr: Scalar::I(16), cap: None },
                    None,
                    Collection::Vec,
                )
                .with_presence(Presence::Prefix),
            ],
        )),
        be(MessageDef::new(
            "MessageId",
            vec![
                Segment::value("path", held("NullArray", false)),
                Segment::value("producer_id", held("Nested", false)),
            ],
        )),
    ])
}

#[test]
fn the_committed_source_is_what_the_generator_emits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(EMITTED);
    let source = generate(&objects()).expect("the module emits").source;
    if std::env::var_os("LAYLINE_BLESS").is_some() {
        std::fs::write(&path, &source).expect("write the emitted source");
    }
    let committed = std::fs::read_to_string(&path).expect("read the emitted source");
    assert_eq!(committed, source, "stale: rerun with LAYLINE_BLESS=1");
}

fn object(body: Body) -> Nested {
    Nested { object: Some(Box::new(Object { kind: Stated::default(), body })) }
}

fn chain(levels: usize) -> Nested {
    (0..levels).fold(object(Body::ConnectionId(9)), |below, _| {
        object(Body::MessageId(Box::new(MessageId {
            path: NullArray { items: None },
            producer_id: below,
        })))
    })
}

#[test]
fn an_emitted_object_that_holds_itself_round_trips() {
    const _: () = assert!(!<Object as Message>::OPEN_ENDED && !<Nested as Message>::OPEN_ENDED);
    assert_eq!(<Object as Message>::MAX_DEPTH, 16);
    let id = object(Body::MessageId(Box::new(MessageId {
        path: NullArray { items: Some(vec![Nested { object: None }]) },
        producer_id: object(Body::ConnectionId(-1)),
    })));
    let wire = id.encode().unwrap();
    #[rustfmt::skip]
    assert_eq!(wire, [
        1, 110,                 // present, a MessageId
        1, 0, 1, 0,             // path: present, one, null
        1, 120, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ]);
    assert_eq!(Nested::decode(&wire).unwrap(), (id, wire.len()));
}

#[test]
fn an_emitted_chain_past_its_depth_is_refused() {
    let wire = chain(5).encode().unwrap();
    assert!(Nested::decode(&wire).is_ok());
    let wire = chain(6).encode().unwrap();
    assert_eq!(Nested::decode(&wire), Err(ParseError::TooDeep { limit: 16 }));
}
