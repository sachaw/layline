//! Layout and message shapes the model accepts.

use super::*;

#[test]
fn fixed_block() {
    let layout = LayoutDef::new(
        "SystemState",
        Container::Bytes { bytes: 100, endian: Endian::Le },
        vec![
            field("system_status", Kind::Codec { ty: "SystemStatus".into(), bits: 16 }),
            field("filter_status", Kind::Codec { ty: "FilterStatus".into(), bits: 16 }),
            field("unix_time_seconds", Kind::Scalar(Scalar::U(32))),
            field("microseconds", Kind::Scalar(Scalar::U(32))),
            field("lla", Kind::array(Scalar::F64, 3)),
            field("velocities", Kind::array(Scalar::F32, 3)),
            field("body_acceleration", Kind::array(Scalar::F32, 3)),
            field("g_force", Kind::Scalar(Scalar::F32)),
            field("orientation", Kind::array(Scalar::F32, 3)),
            field("angular_velocity", Kind::array(Scalar::F32, 3)),
            field("sigmas", Kind::array(Scalar::F32, 3)),
        ],
    );
    assert_eq!(validate_layout(&layout), Ok(()));
    assert_eq!(validate_message(&as_message(&layout)), Ok(()));
}

#[test]
fn constant_count_nested_array() {
    let layout = LayoutDef::new(
        "PortConfiguration",
        Container::Bytes { bytes: 4 + 4 * 30, endian: Endian::Le },
        vec![
            field("permanent", Kind::Scalar(Scalar::U(32))),
            field("blocks", Kind::NestedArray { ty: "PortBlock".into(), bytes: 30, len: 4 }),
        ],
    );
    assert_eq!(validate_layout(&layout), Ok(()));
}

/// An eight-byte argument union at byte 4 puts the payload at byte 12, whichever arm is used.
/// Without a fixed size, the payload position cannot be checked.
#[test]
fn switch_with_a_fixed_footprint() {
    let c = choice(
        "Args",
        true,
        vec![
            Arm::new(
                0x05,
                "GetBlock",
                vec![Segment::Block(vec![
                    field("id", Kind::Scalar(Scalar::U(32))),
                    field("index", Kind::Scalar(Scalar::U(32))),
                ])],
            ),
            Arm::new(
                0x06,
                "GetBlockDone",
                vec![Segment::Block(vec![
                    field("id", Kind::Scalar(Scalar::U(32))),
                    field("spare", Kind::bytes(4)),
                ])],
            ),
        ],
    );
    assert_eq!(validate_choice(&c), Ok(()));

    let m = msg(
        "Packet",
        vec![
            Segment::Block(vec![
                field("command", Kind::Scalar(Scalar::U(8))),
                field("bitmap", Kind::Scalar(Scalar::U(16))),
                field("counter", Kind::Scalar(Scalar::U(8))),
            ]),
            switch_in("args", Discriminant::Field("command".into()), "Args", 8),
            Segment::Block(vec![
                Field::new("payload", Kind::Scalar(Scalar::U(32))).with_stated(Stated::byte(12)),
            ]),
        ],
    );
    assert_eq!(validate_message(&m), Ok(()));

    assert_eq!(m.segments[1].fixed_bits(), Some(64));
    let loose = msg(
        "Loose",
        vec![
            Segment::Block(vec![field("command", Kind::Scalar(Scalar::U(8)))]),
            switch("args", Discriminant::Field("command".into()), "Args"),
            Segment::Block(vec![
                Field::new("payload", Kind::Scalar(Scalar::U(32))).with_stated(Stated::byte(12)),
            ]),
        ],
    );
    assert!(validate_message(&loose).is_err());
}

/// A switch on body length has no tag field to write. With a fixed size, every arm would decode as
/// the arm whose value equals that size.
#[test]
fn switch_on_body_length_with_a_footprint_is_refused() {
    let m = msg(
        "Reading",
        vec![
            switch_in("form", Discriminant::BodyLength, "Form", 4),
            Segment::Block(vec![field("trailer", Kind::Scalar(Scalar::U(8)))]),
        ],
    );
    assert!(validate_message(&m).is_err());

    let loose = msg("Reading", vec![switch("form", Discriminant::BodyLength, "Form")]);
    assert_eq!(validate_message(&loose), Ok(()));
}

#[test]
fn switch_in_a_window_the_wire_states() {
    let window = |field: &str| Segment::Switch {
        name: "value".into(),
        on: Discriminant::Field("tag".into()),
        choice: "Value".into(),
        window: Some(Len::Field { by: By::field(field), cap: None }),
        doc: None,
        stated: None,
    };
    let m = msg(
        "Tlv",
        vec![
            Segment::Block(vec![
                field("tag", Kind::Scalar(Scalar::U(8))),
                field("len", Kind::Scalar(Scalar::U(16))),
            ]),
            window("len"),
            Segment::Block(vec![field("crc", Kind::Scalar(Scalar::U(16)))]),
        ],
    );
    assert_eq!(validate_message(&m), Ok(()));
    assert_eq!(m.segments[1].fixed_bits(), None, "the size comes from `len`");

    let unknown = msg(
        "Tlv",
        vec![Segment::Block(vec![field("tag", Kind::Scalar(Scalar::U(8)))]), window("len")],
    );
    assert_eq!(validate_message(&unknown), Err(Invalid::UnknownReference("len".into())));
    let text = msg(
        "Tlv",
        vec![
            Segment::Block(vec![
                field("tag", Kind::Scalar(Scalar::U(8))),
                field("len", Kind::Scalar(Scalar::F32)),
            ]),
            window("len"),
        ],
    );
    assert_eq!(validate_message(&text), Err(Invalid::NonIntegerReference("len".into())));

    let twice = msg(
        "Tlv",
        vec![
            Segment::Block(vec![field("len", Kind::Scalar(Scalar::U(8)))]),
            Segment::Switch {
                name: "value".into(),
                on: Discriminant::BodyLength,
                choice: "Value".into(),
                window: Some(Len::Field { by: By::field("len"), cap: None }),
                doc: None,
                stated: None,
            },
        ],
    );
    assert!(matches!(validate_message(&twice), Err(Invalid::Field { at, .. }) if at == "value"));

    let fill = msg(
        "Tlv",
        vec![
            Segment::Block(vec![field("tag", Kind::Scalar(Scalar::U(8)))]),
            Segment::Switch {
                name: "value".into(),
                on: Discriminant::Field("tag".into()),
                choice: "Value".into(),
                window: Some(Len::Fill),
                doc: None,
                stated: None,
            },
        ],
    );
    assert!(matches!(validate_message(&fill), Err(Invalid::Field { at, .. }) if at == "value"));
}

#[test]
fn switch_with_a_zero_footprint_is_refused() {
    let m = msg(
        "Empty",
        vec![
            Segment::Block(vec![field("command", Kind::Scalar(Scalar::U(8)))]),
            switch_in("args", Discriminant::Field("command".into()), "Args", 0),
        ],
    );
    assert!(validate_message(&m).is_err());
}

#[test]
fn a_stride_must_name_an_earlier_number() {
    let strided = |stride: By| {
        msg(
            "Bad",
            vec![
                Segment::Block(vec![
                    field("n", Kind::Scalar(Scalar::U(8))),
                    field("label", Kind::bytes(4)),
                ]),
                Segment::repeat(
                    "xs",
                    Kind::Scalar(Scalar::U(16)),
                    Count::Strided { by: By::field("n"), cap: None, stride },
                    None,
                    Collection::Vec,
                ),
            ],
        )
    };

    assert!(matches!(
        validate_message(&strided(By::field("missing"))),
        Err(Invalid::UnknownReference(f)) if f == "missing"
    ));
    assert!(matches!(
        validate_message(&strided(By::field("label"))),
        Err(Invalid::NonIntegerReference(f)) if f == "label"
    ));
    assert!(validate_message(&strided(By::field("n"))).is_err());
}

#[test]
fn a_strided_run_of_self_delimiting_elements_is_rejected() {
    let m = msg(
        "Bad",
        vec![
            Segment::Block(vec![
                field("n", Kind::Scalar(Scalar::U(8))),
                field("w", Kind::Scalar(Scalar::U(8))),
            ]),
            Segment::repeat(
                "v",
                Kind::Var { ty: "Uleb128".into() },
                Count::Strided { by: By::field("n"), cap: None, stride: By::field("w") },
                None,
                Collection::Vec,
            ),
        ],
    );
    assert!(validate_message(&m).is_err());
}
