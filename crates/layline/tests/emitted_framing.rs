//! Framing from `layline-codegen`'s model: prefixes, presence flags and a `Stated` field emit
//! source that compiles, round-trips, and compares a decoded value equal to a built one.
//! `support/emitted_framing.rs` is the generator's output, and a test keeps it current.

#![cfg(feature = "derive")]

use layline::{EncodeError, Message, Stated};
use layline_codegen::emit::{Module, generate};
use layline_codegen::{
    Collection, Count, Endian, Expr, Field, Item, Kind, Len, MessageDef, Op, Presence, Scalar,
    Segment,
};

include!("support/emitted_framing.rs");

const EMITTED: &str = "support/emitted_framing.rs";

/// The message `support/emitted_framing.rs` holds.
fn record() -> Module {
    let text = |repr| Kind::Text { len: Len::Prefix { repr, cap: None }, codec: None };
    let bytes = Kind::Scalar(Scalar::U(8));
    Module::new(vec![Item::Message(
        MessageDef::new(
            "Record",
            vec![
                Segment::Block(vec![
                    Field::new("n", Kind::Scalar(Scalar::U(8))).with_stated(),
                    Field::new("tag", Kind::Scalar(Scalar::U(16))),
                    Field::new("flags", Kind::Scalar(Scalar::U(8))).with_stated(),
                ]),
                Segment::opt(
                    Presence::When(Expr::binary(Op::BitAnd, Expr::name("flags"), Expr::int(0x01))),
                    Field::new("a", Kind::Scalar(Scalar::U(8))),
                ),
                Segment::opt(
                    Presence::When(Expr::binary(Op::BitAnd, Expr::name("flags"), Expr::int(0x02))),
                    Field::new("b", Kind::Scalar(Scalar::U(8))),
                ),
                Segment::repeat(
                    "items",
                    Kind::Scalar(Scalar::U(16)),
                    Count::Field { by: Expr::name("n"), cap: None },
                    None,
                    Collection::Vec,
                ),
                Segment::value("name", text(Scalar::U(8))),
                Segment::repeat(
                    "blob",
                    bytes.clone(),
                    Count::Window(Len::Prefix { repr: Scalar::U(16), cap: Some(64) }),
                    None,
                    Collection::Vec,
                ),
                Segment::opt(Presence::Prefix, Field::new("note", text(Scalar::U(16)))),
                Segment::repeat(
                    "path",
                    bytes,
                    Count::Prefix { repr: Scalar::I(16), cap: None },
                    None,
                    Collection::Vec,
                )
                .with_presence(Presence::Prefix),
            ],
        )
        .with_endian(Endian::Be),
    )])
}

#[test]
fn the_committed_source_is_what_the_generator_emits() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(EMITTED);
    let source = generate(&record()).expect("the message emits").source;
    if std::env::var_os("LAYLINE_BLESS").is_some() {
        std::fs::write(&path, &source).expect("write the emitted source");
    }
    let committed = std::fs::read_to_string(&path).expect("read the emitted source");
    assert_eq!(committed, source, "stale: rerun with LAYLINE_BLESS=1");
}

#[test]
fn an_emitted_record_frames_its_fields_and_compares_equal_decoded() {
    let built = Record {
        n: Stated::default(),
        tag: 0x0102,
        flags: Stated::default(),
        a: None,
        b: Some(3),
        items: vec![7],
        name: "ab".into(),
        blob: vec![9, 9],
        note: None,
        path: Some(vec![5]),
    };
    let wire = built.encode().unwrap();
    assert_eq!(
        wire,
        [1, 0x01, 0x02, 0x02, 3, 0, 7, 2, b'a', b'b', 0, 2, 9, 9, 0, 1, 0, 1, 5],
        "n, tag, flags, b, items, name.len, name, blob.len, blob, note.present, path.present, \
         path.count, path",
    );
    let (decoded, used) = Record::decode(&wire).unwrap();
    assert_eq!((decoded.n.get(), used), (1, wire.len()));
    assert_eq!(decoded, built);

    let over = Record { blob: vec![0; 65], ..built };
    assert_eq!(over.encode(), Err(EncodeError::OverCap { field: "blob", len: 65, cap: 64 }));
}
