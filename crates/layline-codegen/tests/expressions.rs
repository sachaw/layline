//! A generator states counts, lengths and conditions as expressions, as the Kaitai front end does.
#![cfg(feature = "walk")]

use layline_codegen::__derive::message_parts;
use layline_codegen::{
    Collection, Count, Expr, Field, Item, Kind, Len, MessageDef, Presence, Root, Scalar, Segment,
    validate,
};

fn u8f(name: &str) -> Field {
    Field::new(name, Kind::Scalar(Scalar::U(8)))
}

fn rtp_extension() -> MessageDef {
    let e = |s: &str| Expr::parse(s).expect("parses");
    MessageDef::new(
        "Extension",
        vec![
            Segment::block(vec![
                Field::new("has_extension", Kind::Scalar(Scalar::ByteBool)),
                u8f("payload_type"),
                u8f("extension_len"),
                u8f("markings_len"),
            ]),
            Segment::repeat(
                "markings",
                Kind::Scalar(Scalar::U(8)),
                Count::Window(Len::Field { by: e("markings_len"), cap: None, align: None }),
                None,
                Collection::Vec,
            ),
            Segment::repeat(
                "rest",
                Kind::Scalar(Scalar::U(8)),
                Count::Window(Len::Field {
                    by: e("extension_len*4 - (13+markings_len)"),
                    cap: None,
                    align: None,
                }),
                None,
                Collection::Vec,
            ),
            Segment::opt(
                Presence::When(e("has_extension && payload_type != 3")),
                Field::new("ext", Kind::Scalar(Scalar::U(16))),
            ),
            Segment::valid("payload_type", e("payload_type < 0x80")),
        ],
    )
}

#[test]
fn the_kaitai_expressions_are_a_model() {
    let m = rtp_extension();
    assert_eq!(validate(&Item::Message(m.clone())), Ok(()));
    let parts = message_parts(&m, &[], &Root::default()).expect("generates");
    let codec = parts.codec.to_string();
    // Checked, not written: two fields in each.
    assert!(codec.contains("EncodeError :: agree"), "{codec}");
    assert!(codec.contains("\"extension_len*4-(13+markings_len)\""), "{codec}");
    assert!(codec.contains("\"has_extension&&payload_type!=3\""), "{codec}");
}

#[test]
fn an_expression_round_trips_through_its_spelling() {
    for s in ["extension_len * 4 - (13 + markings_len)", "has_extension && payload_type != 3"] {
        let e = Expr::parse(s).unwrap();
        assert_eq!(e.to_string(), s);
        assert_eq!(Expr::parse(&format!("{e:#}")).unwrap(), e);
    }
}
