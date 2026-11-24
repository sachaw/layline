//! A tail from `layline-codegen`'s model: RTP-shaped padding, counted by the last byte, a CRC
//! trailer over everything before it, and a listed arm that reads to the tail past a condition.
//! One length that sizes two values is written by the first and checked by the second. The
//! emitted source compiles and round-trips.
//! `support/emitted_tail.rs` is the generator's output, and a test keeps it current.

#![cfg(feature = "derive")]

use layline::{EncodeError, Message, Stated};
use layline_codegen::emit::{Module, generate};
use layline_codegen::{
    Arm, ChoiceDef, Collection, Count, CoverFrom, CoverTo, Coverage, Discriminant, Endian, Expr,
    Field, Item, Kind, Len, MessageDef, Op, Param, Presence, Scalar, Segment,
};

/// CRC-32/ISO-HDLC.
type Crc32 = layline::checksum::Crc<u32, 0xEDB8_8320, 0xFFFF_FFFF, 0xFFFF_FFFF, true>;

include!("support/emitted_tail.rs");

const EMITTED: &str = "support/emitted_tail.rs";

fn bytes() -> Kind {
    Kind::Scalar(Scalar::U(8))
}

/// The module `support/emitted_tail.rs` holds.
fn tails() -> Module {
    let padded = || Presence::When(Expr::name("padded"));
    Module::new(vec![
        Item::Message(
            MessageDef::new(
                "Packet",
                vec![
                    Segment::Block(vec![
                        Field::new("padded", Kind::Scalar(Scalar::ByteBool)),
                        Field::new("kind", bytes()),
                    ]),
                    Segment::repeat(
                        "payload",
                        bytes(),
                        Count::Fill { cap: None },
                        None,
                        Collection::Vec,
                    ),
                ],
            )
            .with_tail(vec![
                Segment::repeat(
                    "padding",
                    bytes(),
                    Count::Field {
                        by: Expr::binary(Op::Sub, Expr::name("pad_count"), Expr::int(1)),
                        cap: None,
                    },
                    None,
                    Collection::Vec,
                )
                .with_presence(padded()),
                Segment::opt(padded(), Field::new("pad_count", bytes()).with_stated()),
            ])
            .with_endian(Endian::Be),
        ),
        Item::Message(
            MessageDef::new(
                "Sealed",
                vec![
                    Segment::Block(vec![Field::new("kind", bytes())]),
                    Segment::repeat(
                        "data",
                        bytes(),
                        Count::Fill { cap: None },
                        None,
                        Collection::Vec,
                    ),
                ],
            )
            .with_tail(vec![Segment::checksum(
                "crc",
                Scalar::U(32),
                "Crc32",
                Coverage::new(CoverFrom::Field("kind".into()), CoverTo::After("data".into())),
            )]),
        ),
        Item::Choice(
            ChoiceDef::new(
                "Body",
                vec![
                    Arm::new(1, "Level", vec![Segment::Block(vec![Field::new("inner", bytes())])]),
                    Arm::new(
                        128,
                        "Described",
                        vec![Segment::repeat(
                            "inner",
                            bytes(),
                            Count::Fill { cap: None },
                            None,
                            Collection::Vec,
                        )],
                    ),
                ],
            )
            .with_endian(Endian::Be),
        ),
        Item::Message(
            MessageDef::new(
                "Dw",
                vec![
                    Segment::Block(vec![
                        Field::new("kind", bytes()),
                        Field::new("profile", Kind::Scalar(Scalar::U(16))),
                    ]),
                    Segment::switch("body", Discriminant::Field(Expr::name("kind")), "Body", None),
                    Segment::valid("profile", Expr::parse("profile == 16").unwrap()),
                ],
            )
            .with_tail(vec![Segment::Block(vec![Field::new("foot", bytes())])])
            .with_endian(Endian::Be),
        ),
        Item::Message(
            MessageDef::new(
                "Marked",
                vec![
                    Segment::Block(vec![Field::new("markings_len", bytes()).with_stated()]),
                    Segment::value(
                        "markings",
                        Kind::Text {
                            len: Len::Field {
                                by: Expr::name("markings_len"),
                                cap: None,
                                align: None,
                            },
                            codec: None,
                        },
                    ),
                    Segment::repeat(
                        "padding",
                        bytes(),
                        Count::Window(Len::Field {
                            by: Expr::parse("extension_len * 4 - (1 + markings_len)").unwrap(),
                            cap: None,
                            align: None,
                        }),
                        None,
                        Collection::Vec,
                    ),
                ],
            )
            .with_needs(vec![Param::new("extension_len", Scalar::U(16))]),
        ),
    ])
}

#[test]
fn the_committed_source_is_what_the_generator_emits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(EMITTED);
    let source = generate(&tails()).expect("the module emits").source;
    if std::env::var_os("LAYLINE_BLESS").is_some() {
        std::fs::write(&path, &source).expect("write the emitted source");
    }
    let committed = std::fs::read_to_string(&path).expect("read the emitted source");
    assert_eq!(committed, source, "stale: rerun with LAYLINE_BLESS=1");
}

#[test]
fn an_emitted_packet_pads_at_its_end() {
    let packet = Packet {
        padded: true,
        kind: 7,
        payload: vec![1, 2],
        padding: Some(vec![0, 0]),
        pad_count: Stated::default(),
    };
    let wire = packet.encode().unwrap();
    assert_eq!(wire, [1, 7, 1, 2, 0, 0, 3]);
    assert_eq!(Packet::decode(&wire).unwrap(), (packet, 7));
    let (plain, _) = Packet::decode(&[0, 7, 1, 2, 3]).unwrap();
    assert_eq!((plain.payload, plain.padding), (vec![1, 2, 3], None));
    assert!(matches!(
        Packet::decode(&[1, 7, 1, 0]),
        Err(ParseError::Malformed { field: "pad_count", .. })
    ));
}

#[test]
fn an_emitted_trailer_checks_what_it_follows() {
    let sealed = Sealed { kind: 1, data: vec![2, 3], crc: 0 };
    let wire = sealed.encode();
    let crc = <Crc32 as layline::Checksum>::compute(&[1, 2, 3]);
    assert_eq!(&wire[3..], crc.to_le_bytes());
    assert_eq!(Sealed::decode(&wire).unwrap().0.crc, crc);
    let mut bent = wire.clone();
    bent[1] ^= 0x80;
    assert!(matches!(Sealed::decode(&bent), Err(ParseError::Checksum { field: "crc", .. })));
}

#[test]
fn an_emitted_listed_arm_reads_to_the_tail_past_a_condition() {
    let dw = Dw { kind: 128, profile: 16, body: Body::Described(vec![1, 2]), foot: 9 };
    let wire = dw.encode().unwrap();
    assert_eq!(wire, [128, 0, 16, 1, 2, 9]);
    assert_eq!(Dw::decode(&wire).unwrap(), (dw, 6));
    assert_eq!(Dw::decode(&[1, 0, 16, 7, 9]).unwrap().0.body, Body::Level(7));
    assert!(matches!(
        Dw::decode(&[1, 0, 17, 7, 9]),
        Err(ParseError::Malformed { field: "profile", .. })
    ));
    assert!(matches!(Dw::decode(&[1, 0, 16, 7, 8, 9]), Err(ParseError::Malformed { .. })));
    let bent = Dw { kind: 1, profile: 17, body: Body::Level(7), foot: 9 };
    assert_eq!(bent.encode(), Err(EncodeError::Mismatch { field: "profile", value: 0, stated: 1 }));
}

#[test]
fn an_emitted_length_for_two_values_is_written_once_and_checked_once() {
    let ctx = MarkedCtx { extension_len: 1 };
    let marked = Marked {
        markings_len: Stated::default(),
        markings: String::from("U"),
        padding: vec![0, 0],
    };
    let wire = marked.encode_with(ctx).unwrap();
    assert_eq!(wire, [1, b'U', 0, 0]);
    assert_eq!(Marked::decode_with(&wire, ctx).unwrap().0, marked);
    let long = Marked { padding: vec![0; 3], ..marked };
    assert_eq!(
        long.encode_with(ctx),
        Err(EncodeError::Mismatch { field: "padding", value: 3, stated: 2 })
    );
    assert!(Marked::decode_with(&[4, b'U', 0, 0], ctx).is_err());
}
