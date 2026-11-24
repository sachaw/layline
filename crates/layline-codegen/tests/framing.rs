//! Framing in the model: a prefix on a count or length, a presence prefix, and a framing field,
//! which encode writes and an attribute must.

#[path = "support/mod.rs"]
mod support;

use layline_codegen::{
    BitOrder, Collection, Count, Expr, Field, Invalid, Kind, Len, Presence, Scalar, Segment,
};
use support::{bytes, msg, u, validate_layout, validate_message};

fn text(repr: Scalar) -> Kind {
    Kind::Text { len: Len::Prefix { repr, cap: None }, codec: None }
}

fn refused_field(why: Result<(), Invalid>, field: &str, reason: &str) {
    match why {
        Err(Invalid::Field { at, why }) if at == field && why.contains(reason) => {}
        other => panic!("expected `{field}` refused with \"{reason}\", got {other:?}"),
    }
}

/// JET's header: a string8 key, then a value under a `u16` length.
fn header() -> Vec<Segment> {
    vec![
        Segment::value("key", text(Scalar::U(8))),
        Segment::repeat(
            "value",
            u(8),
            Count::Window(Len::Prefix { repr: Scalar::U(16), cap: Some(16384) }),
            None,
            Collection::Vec,
        ),
    ]
}

#[test]
fn prefixes_and_a_presence_prefix_are_valid_where_they_frame_a_value() {
    validate_message(&msg("Header", header())).expect("a string8 and a blob16");
    let nullable = vec![
        Segment::opt(Presence::Prefix, Field::new("name", text(Scalar::U(16)))),
        Segment::repeat(
            "path",
            u(8),
            Count::Prefix { repr: Scalar::I(16), cap: None },
            None,
            Collection::Vec,
        )
        .with_presence(Presence::Prefix),
    ];
    validate_message(&msg("Nullable", nullable)).expect("OpenWire's nullable values");
}

#[test]
fn a_prefix_is_a_whole_byte_integer() {
    for repr in [Scalar::U(12), Scalar::U(128), Scalar::Bool, Scalar::F32] {
        let why = validate_message(&msg("M", vec![Segment::value("name", text(repr))]));
        refused_field(why, "name", "a prefix is an integer of 8, 16, 32 or 64 bits");
    }
}

#[test]
fn a_framing_field_must_be_written() {
    let counted = |by: &str| {
        vec![
            Segment::block(vec![Field::new("n", u(8)).with_stated(), Field::new("m", u(8))]),
            Segment::repeat(
                "items",
                u(8),
                Count::Field { by: Expr::parse(by).expect("parses"), cap: None },
                None,
                Collection::Vec,
            ),
        ]
    };
    validate_message(&msg("M", counted("n * 2"))).expect("`n * 2` works back to `n`");
    refused_field(validate_message(&msg("M", counted("m"))), "n", "no attribute writes it");
    refused_field(validate_message(&msg("M", counted("n * m"))), "n", "no attribute writes it");

    let flagged = vec![
        Segment::block(vec![Field::new("has", Kind::Scalar(Scalar::ByteBool)).with_stated()]),
        Segment::opt(Presence::When(Expr::name("has")), Field::new("x", u(16))),
    ];
    validate_message(&msg("M", flagged)).expect("a presence test writes a flag");
}

#[test]
fn a_framing_field_is_a_number_or_a_flag_in_a_message() {
    let arr = vec![Segment::block(vec![Field::new("n", Kind::bytes(2)).with_stated()])];
    refused_field(validate_message(&msg("M", arr)), "n", "framing is a number or a flag");

    let layout = bytes(1, vec![Field::new("n", u(8)).with_stated()]);
    refused_field(validate_layout(&layout), "n", "a layout states nothing about other fields");

    let bits = msg("M", vec![Segment::block(vec![Field::new("n", u(8)).with_stated()])])
        .with_bits(BitOrder::Lsb);
    refused_field(validate_message(&bits), "n", "a bit-addressed message writes no framing");
}

#[test]
fn an_optional_collection_states_its_own_size() {
    let by_field = vec![
        Segment::block(vec![Field::new("n", u(8))]),
        Segment::repeat(
            "items",
            u(8),
            Count::Field { by: Expr::name("n"), cap: None },
            None,
            Collection::Vec,
        )
        .with_presence(Presence::Prefix),
    ];
    refused_field(validate_message(&msg("M", by_field)), "items", "states its own size");

    let remaining = vec![
        Segment::repeat(
            "items",
            u(8),
            Count::Prefix { repr: Scalar::U(8), cap: None },
            None,
            Collection::Vec,
        )
        .with_presence(Presence::Remaining),
    ];
    refused_field(validate_message(&msg("M", remaining)), "items", "present by a flag");
}

#[cfg(feature = "walk")]
#[test]
fn the_walk_gives_each_prefix_a_row_and_writes_a_framing_field() {
    use layline_codegen::__derive::message_parts;
    use layline_codegen::Root;

    let mut segments =
        vec![Segment::block(vec![Field::new("n", u(8)).with_stated(), Field::new("tag", u(16))])];
    segments.push(Segment::repeat(
        "items",
        u(16),
        Count::Field { by: Expr::name("n"), cap: None },
        None,
        Collection::Vec,
    ));
    segments.extend(header());
    let parts = message_parts(&msg("M", segments), &[], &Root::default()).expect("renders");
    let codec = parts.codec.to_string().replace(' ', "");
    assert!(codec.contains(r#"SegmentDef::new("key.len""#), "{codec}");
    assert!(codec.contains(r#"SegmentDef::new("value.len""#), "{codec}");
    assert!(codec.contains(r#"By::field("value.len")"#), "{codec}");
    assert!(codec.contains("n:::layline::Stated::new("), "decode wraps the number: {codec}");
    let fields = parts.fields.to_string().replace(' ', "");
    assert!(fields.contains("pubn:::layline::Stated<u8>"), "{fields}");
    assert!(!fields.contains("key_len") && !fields.contains("value.len"), "{fields}");
}
