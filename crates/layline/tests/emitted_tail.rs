//! A tail from `layline-codegen`'s model: RTP-shaped padding, counted by the last byte, and a
//! CRC trailer over everything before it. The emitted source compiles and round-trips.
//! `support/emitted_tail.rs` is the generator's output, and a test keeps it current.

#![cfg(feature = "derive")]

use layline::{Message, Stated};
use layline_codegen::emit::{Module, generate};
use layline_codegen::{
    Collection, Count, CoverFrom, CoverTo, Coverage, Endian, Expr, Field, Item, Kind, MessageDef,
    Op, Presence, Scalar, Segment,
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
