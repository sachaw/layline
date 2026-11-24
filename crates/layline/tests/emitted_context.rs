//! Context into a switch's arms, from `layline-codegen`'s model: a DW-shaped packet whose
//! payload type picks an element run that needs the header's mask, count and byte order, and an
//! inline arm that reads them itself. The emitted source compiles and round-trips.
//! `support/emitted_context.rs` is the generator's output, and a test keeps it current.

#![cfg(feature = "derive")]

use layline::Message;
use layline_codegen::emit::{Module, generate};
use layline_codegen::{
    Arm, ChoiceDef, Collection, Count, Discriminant, Endian, Expr, Field, Item, Kind, MessageDef,
    Op, Param, Presence, Scalar, Segment,
};

include!("support/emitted_context.rs");

const EMITTED: &str = "support/emitted_context.rs";

fn u(bits: u64) -> Kind {
    Kind::Scalar(Scalar::U(bits))
}

fn params() -> Vec<Param> {
    vec![
        Param::new("mask", Scalar::U(32)),
        Param::new("count", Scalar::U(16)),
        Param::new("big", Scalar::Bool),
    ]
}

fn masked(bit: i64, name: &str, kind: Kind) -> Segment {
    Segment::opt(
        Presence::When(Expr::binary(Op::BitAnd, Expr::name("mask"), Expr::int(bit))),
        Field::new(name, kind),
    )
}

/// The module `support/emitted_context.rs` holds.
fn packets() -> Module {
    let msg = |ty: &str, with: &[&str]| Kind::Msg {
        ty: ty.into(),
        boxed: false,
        with: with.iter().map(|w| Expr::name(w)).collect(),
    };
    Module::new(vec![
        Item::Message(
            MessageDef::new(
                "Packet",
                vec![
                    Segment::Block(vec![
                        Field::new("payload_type", u(8)),
                        Field::new("mask", u(32)),
                        Field::new("num_elements", u(16)),
                        Field::new("big_endian", Kind::Scalar(Scalar::ByteBool)),
                    ]),
                    Segment::switch(
                        "payload",
                        Discriminant::Field(Expr::name("payload_type")),
                        "Payload",
                        None,
                    )
                    .with_context(vec![
                        Expr::name("mask"),
                        Expr::name("num_elements"),
                        Expr::name("big_endian"),
                    ]),
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
                        "Pdw",
                        vec![Segment::value("inner", msg("Pdws", &["mask", "count", "big"]))],
                    ),
                    Arm::new(
                        3,
                        "Tagged",
                        vec![
                            Segment::Block(vec![Field::new("tag", u(16))]),
                            masked(0x01, "extra", u(32)),
                        ],
                    ),
                ],
            )
            .with_needs(params())
            .with_endian_from("big"),
        ),
        Item::Message(
            MessageDef::new(
                "Pdws",
                vec![Segment::repeat(
                    "elements",
                    msg("Pdw", &["mask", "big"]),
                    Count::Field { by: Expr::name("count"), cap: None },
                    None,
                    Collection::Vec,
                )],
            )
            .with_needs(params())
            .with_endian_from("big"),
        ),
        Item::Message(
            MessageDef::new(
                "Pdw",
                vec![
                    masked(0x01, "toa", Kind::Scalar(Scalar::I(64))),
                    masked(0x02, "pw", Kind::Scalar(Scalar::F32)),
                ],
            )
            .with_needs(vec![Param::new("mask", Scalar::U(32)), Param::new("big", Scalar::Bool)])
            .with_endian_from("big"),
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

#[test]
fn an_emitted_packet_passes_its_header_to_the_arm() {
    for big_endian in [false, true] {
        let packet = Packet {
            payload_type: 1,
            mask: 0x01,
            num_elements: 2,
            big_endian,
            payload: Payload::Pdw(Pdws {
                elements: vec![Pdw { toa: Some(7), pw: None }, Pdw { toa: Some(-1), pw: None }],
            }),
        };
        let wire = packet.encode().unwrap();
        assert_eq!(wire.len(), 8 + 16);
        let seven = if big_endian { 7i64.to_be_bytes() } else { 7i64.to_le_bytes() };
        assert_eq!(&wire[8..16], seven);
        assert_eq!(Packet::decode(&wire).unwrap(), (packet, wire.len()));
    }
}

#[test]
fn an_inline_arm_reads_the_parameters_itself() {
    let packet = Packet {
        payload_type: 3,
        mask: 0x01,
        num_elements: 0,
        big_endian: true,
        payload: Payload::Tagged { tag: 0x0102, extra: Some(0x0A0B_0C0D) },
    };
    let wire = packet.encode().unwrap();
    assert_eq!(&wire[8..], [1, 2, 0x0A, 0x0B, 0x0C, 0x0D]);
    assert_eq!(Packet::decode(&wire).unwrap(), (packet, wire.len()));
}
