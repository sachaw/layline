//! Bit-addressed messages.

use super::*;

/// A bit-addressed message with two fixed bit fields followed by `extra`.
fn bits_msg(extra: Vec<Segment>) -> MessageDef {
    let mut segments = vec![Segment::block(vec![
        field("version", Kind::Scalar(Scalar::U(4))),
        field("urgent", Kind::Scalar(Scalar::Bool)),
    ])];
    segments.extend(extra);
    MessageDef::new("Bits", segments).with_bits(layline_codegen::BitOrder::Msb)
}

#[test]
fn a_bit_addressed_message_takes_bit_fields_and_inline_presence_bits() {
    assert_eq!(validate_message(&bits_msg(Vec::new())), Ok(()));
    assert_eq!(
        validate_message(&bits_msg(vec![Segment::opt(
            Presence::Bit,
            field("origin", Kind::Scalar(Scalar::U(12))),
        )])),
        Ok(())
    );
    assert_eq!(
        validate_message(&bits_msg(vec![Segment::block(vec![field(
            "tag",
            Kind::Codec { ty: "Tag".into(), bits: 6 },
        )])])),
        Ok(())
    );
}

#[test]
fn a_bit_addressed_message_refuses_every_other_segment_kind() {
    let cases = vec![
        Segment::value("label", Kind::Text { len: Len::Fill, codec: None }),
        Segment::repeat(
            "items",
            Kind::Scalar(Scalar::U(8)),
            Count::Field { by: By::field("version"), cap: None },
            None,
            Collection::Vec,
        ),
        Segment::placed("record", Kind::Nested { ty: "Entry".into(), bytes: 4 }, "version", None),
        switch("body", Discriminant::Field("version".into()), "Body"),
        Segment::checksum("crc", Scalar::U(8), "Lrc8", layline_codegen::Coverage::whole()),
    ];
    for seg in cases {
        let why = validate_message(&bits_msg(vec![seg]));
        assert!(
            matches!(&why, Err(Invalid::Field { why, .. }) if why.contains("bit-addressed message")),
            "{why:?}"
        );
    }
}

#[test]
fn a_bit_addressed_message_refuses_a_field_the_cursor_cannot_read() {
    let cases = vec![
        Kind::bytes(2),
        Kind::Nested { ty: "Entry".into(), bytes: 4 },
        Kind::Msg { ty: "Inner".into(), boxed: false, with: Vec::new() },
        Kind::Text { len: Len::Bytes(4), codec: None },
    ];
    for kind in cases {
        let why = validate_message(&bits_msg(vec![Segment::block(vec![field("odd", kind)])]));
        assert!(
            matches!(&why, Err(Invalid::Field { why, .. }) if why.contains("no bit width")),
            "{why:?}"
        );
    }

    let wide = bits_msg(vec![Segment::block(vec![field(
        "wide",
        Kind::Codec { ty: "Wide".into(), bits: 96 },
    )])]);
    assert!(
        matches!(validate_message(&wide), Err(Invalid::Field { why, .. }) if why.contains("at most 64"))
    );

    // A variable-length value must be its own segment, not a block field.
    let blocked =
        bits_msg(vec![Segment::block(vec![field("n", Kind::Var { ty: "ExpGolomb".into() })])]);
    assert!(
        matches!(validate_message(&blocked), Err(Invalid::Field { why, .. }) if why.contains("constant offsets")),
        "{:?}",
        validate_message(&blocked)
    );
}

#[test]
fn a_bit_addressed_message_takes_a_value_of_discovered_extent_and_a_field_behind_an_earlier_flag() {
    let var = bits_msg(vec![
        Segment::value("n", Kind::Var { ty: "ExpGolomb".into() }),
        Segment::block(vec![field("tail", Kind::Scalar(Scalar::U(3)))]),
    ]);
    assert_eq!(validate_message(&var), Ok(()));

    let masked = bits_msg(vec![Segment::opt(
        Presence::Mask { field: "version".into(), mask: 0x4 },
        field("origin", Kind::Scalar(Scalar::U(12))),
    )]);
    assert_eq!(validate_message(&masked), Ok(()));

    let flagged = bits_msg(vec![Segment::opt(
        Presence::Flag { field: "urgent".into() },
        field("origin", Kind::Scalar(Scalar::U(12))),
    )]);
    assert_eq!(validate_message(&flagged), Ok(()));
}

#[test]
fn a_bit_addressed_message_refuses_a_flag_it_cannot_write_the_presence_bits_back_into() {
    let optional = field("origin", Kind::Scalar(Scalar::U(12)));
    let cases: Vec<(Vec<Segment>, &str)> = vec![
        (
            vec![Segment::opt(Presence::Mask { field: "later".into(), mask: 1 }, optional.clone())],
            "is not a field this message reads before",
        ),
        (
            vec![
                Segment::opt(Presence::Bit, field("absent", Kind::Scalar(Scalar::U(4)))),
                Segment::opt(Presence::Mask { field: "absent".into(), mask: 1 }, optional.clone()),
            ],
            "itself sometimes absent",
        ),
        (
            vec![
                Segment::value("n", Kind::Var { ty: "ExpGolomb".into() }),
                Segment::opt(Presence::Mask { field: "n".into(), mask: 1 }, optional.clone()),
            ],
            "reads its width off the wire",
        ),
        (
            vec![Segment::opt(
                Presence::Mask { field: "version".into(), mask: 0x10 },
                optional.clone(),
            )],
            "reaches bit 4 of `version`, which is 4 bits wide",
        ),
        (
            vec![Segment::opt(
                Presence::Mask { field: "version".into(), mask: 0 },
                optional.clone(),
            )],
            "masks no bits",
        ),
        (
            vec![Segment::opt(Presence::Flag { field: "version".into() }, optional.clone())],
            "a bare test reads a `bool`",
        ),
        (
            vec![Segment::opt(
                Presence::Mask { field: "urgent".into(), mask: 1 },
                optional.clone(),
            )],
            "a mask tests the bits of an integer",
        ),
        (
            vec![
                Segment::opt(
                    Presence::Mask { field: "version".into(), mask: 0x3 },
                    field("first", Kind::Scalar(Scalar::U(2))),
                ),
                Segment::opt(
                    Presence::Mask { field: "version".into(), mask: 0x2 },
                    optional.clone(),
                ),
            ],
            "bit 1 of `version` already marks `first` present",
        ),
        (vec![Segment::opt(Presence::Remaining, optional)], "no trailing bytes to test"),
    ];
    for (extra, reason) in cases {
        let why = validate_message(&bits_msg(extra));
        assert!(
            matches!(&why, Err(Invalid::Field { why, .. }) if why.contains(reason)),
            "{reason}: {why:?}"
        );
    }
}

#[test]
fn an_inline_presence_bit_is_refused_in_a_byte_addressed_message() {
    let m = msg(
        "Packet",
        vec![
            Segment::block(vec![field("flags", Kind::Scalar(Scalar::U(8)))]),
            Segment::opt(Presence::Bit, field("extra", Kind::Scalar(Scalar::U(16)))),
        ],
    );
    let why = validate_message(&m);
    assert!(
        matches!(&why, Err(Invalid::Field { why, .. }) if why.contains("byte-addressed message")),
        "{why:?}"
    );
}

#[test]
fn a_bit_addressed_message_has_at_least_one_field_and_no_byte_order() {
    let empty = MessageDef::new("Bits", vec![]).with_bits(layline_codegen::BitOrder::Lsb);
    assert!(
        matches!(validate_message(&empty), Err(Invalid::Other(why)) if why.contains("at least one field"))
    );

    let ordered = bits_msg(Vec::new()).with_endian(Endian::Be);
    assert!(
        matches!(validate_message(&ordered), Err(Invalid::Other(why)) if why.contains("no byte order"))
    );
}
