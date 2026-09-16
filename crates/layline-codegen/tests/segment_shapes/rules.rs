//! Rules that code generators can rely on.

use super::*;

#[test]
fn a_count_must_name_an_earlier_field() {
    let m = msg(
        "Bad",
        vec![Segment::repeat(
            "xs",
            Kind::Scalar(Scalar::U(8)),
            Count::Field { by: By::field("missing"), cap: None },
            None,
            Collection::Vec,
        )],
    );
    assert!(matches!(
        validate_message(&m),
        Err(layline_codegen::Invalid::UnknownReference(f)) if f == "missing"
    ));
}

#[test]
fn nothing_may_follow_an_open_ended_segment() {
    let m = msg(
        "Bad",
        vec![
            Segment::repeat(
                "tail",
                Kind::Scalar(Scalar::U(8)),
                Count::Fill { cap: None },
                None,
                Collection::Vec,
            ),
            Segment::Block(vec![field("after", Kind::Scalar(Scalar::U(8)))]),
        ],
    );
    assert!(matches!(validate_message(&m), Err(layline_codegen::Invalid::AfterOpenEnd(_))));
}

#[test]
fn switch_arms_must_be_distinct() {
    let c = choice(
        "Bad",
        true,
        vec![
            Arm::new(
                1,
                "A",
                vec![Segment::repeat(
                    "tail",
                    Kind::Scalar(Scalar::U(8)),
                    Count::Fill { cap: None },
                    None,
                    Collection::Vec,
                )],
            ),
            Arm::new(
                1,
                "B",
                vec![Segment::repeat(
                    "tail",
                    Kind::Scalar(Scalar::U(8)),
                    Count::Fill { cap: None },
                    None,
                    Collection::Vec,
                )],
            ),
        ],
    );
    assert_eq!(validate_choice(&c), Err(layline_codegen::Invalid::DuplicateArm(1)));
}

#[test]
fn a_switch_is_terminal_only_when_an_arm_is() {
    let bounded = vec![
        Arm::new(0, "Ping", vec![Segment::Block(vec![field("seq", Kind::Scalar(Scalar::U(16)))])]),
        Arm::new(1, "Data", vec![Segment::Block(vec![field("payload", Kind::bytes(4))])]),
    ];
    assert!(!choice("Bounded", false, bounded.clone()).open_ended());

    assert!(choice("Open", true, bounded.clone()).open_ended());

    let mut fills = bounded;
    fills.push(Arm::new(
        2,
        "Trailing",
        vec![Segment::repeat(
            "tail",
            Kind::Scalar(Scalar::U(8)),
            Count::Fill { cap: None },
            None,
            Collection::Vec,
        )],
    ));
    assert!(choice("Fills", false, fills).open_ended());
}

#[test]
fn a_closed_choice_needs_at_least_one_arm() {
    assert!(validate_choice(&choice("Empty", false, vec![])).is_err());
    // An open choice with no arms still decodes, as the tag and the remaining bytes.
    assert_eq!(validate_choice(&choice("Empty", true, vec![])), Ok(()));
}

#[test]
fn a_switch_may_discriminate_on_a_discovered_value() {
    let m = msg(
        "Tlv",
        vec![
            Segment::Value {
                stated: None,
                name: "tag".into(),
                kind: Kind::Var { ty: "layline_core::num::Uleb128".into() },
                doc: None,
            },
            switch("body", Discriminant::Field("tag".into()), "TlvBody"),
        ],
    );
    assert_eq!(validate_message(&m), Ok(()));

    let late = msg("Bad", vec![switch("body", Discriminant::Field("tag".into()), "TlvBody")]);
    assert!(matches!(
        validate_message(&late),
        Err(Invalid::UnknownReference(f)) if f == "tag"
    ));
}

#[test]
fn a_words_mode_field_crossing_its_word_is_rejected() {
    let layout = LayoutDef::new(
        "Bad",
        Container::Words { words: 2, endian: Endian::Le, order: layline_codegen::BitOrder::Lsb },
        vec![
            field("a", Kind::Scalar(Scalar::U(10))),
            field("b", Kind::Scalar(Scalar::U(10))),
            field("c", Kind::Scalar(Scalar::U(12))),
        ],
    );
    assert!(matches!(
        validate_layout(&layout),
        Err(layline_codegen::Invalid::CrossesWord(f)) if f == "b"
    ));
}

#[test]
fn offset_directory() {
    let m = msg(
        "DirectoryFile",
        vec![
            Segment::Block(vec![
                field("entry_table_offset", Kind::Scalar(Scalar::I(32))),
                field("entry_count", Kind::Scalar(Scalar::I(32))),
            ]),
            Segment::repeat(
                "entries",
                Kind::Nested { ty: "TableEntry".into(), bytes: 132 },
                Count::Field { by: By::field("entry_count"), cap: None },
                Some("entry_table_offset"),
                Collection::Vec,
            ),
        ],
    );
    assert_eq!(validate_message(&m), Ok(()));

    let bad = msg(
        "DirectoryFile",
        vec![Segment::repeat(
            "t",
            Kind::Scalar(Scalar::I(32)),
            Count::Fill { cap: None },
            Some("nowhere"),
            Collection::Vec,
        )],
    );
    assert!(matches!(validate_message(&bad), Err(Invalid::UnknownReference(f)) if f == "nowhere"));
}

#[test]
fn one_record_at_a_stated_offset() {
    let block = || {
        Segment::Block(vec![
            field("table_at", Kind::Scalar(Scalar::U(16))),
            field("symb_at", Kind::Scalar(Scalar::U(16))),
        ])
    };
    let placed = |name: &str, kind: Kind, at: &str, optional: bool| Segment::Placed {
        stated: None,
        name: name.into(),
        kind,
        at: at.into(),
        absent: optional.then_some(Absence::ZeroOffset),
        doc: None,
    };

    let m = msg(
        "Container",
        vec![
            block(),
            placed("table", Kind::Nested { ty: "Table".into(), bytes: 8 }, "table_at", false),
            placed(
                "symb",
                Kind::Msg { ty: "Symb".into(), boxed: false, with: Vec::new() },
                "symb_at",
                true,
            ),
        ],
    );
    assert_eq!(validate_message(&m), Ok(()));

    assert_eq!(m.segments[1].fixed_bits(), None);
    assert!(!m.segments[1].open_ended());

    assert_eq!(m.segments[1].places(), ["table"]);
    assert!(m.segments[2].places().is_empty());
    assert_eq!(m.segments[2].placed_optionally(), Some("symb"));

    let bad = msg(
        "Container",
        vec![placed("table", Kind::Nested { ty: "Table".into(), bytes: 8 }, "nowhere", false)],
    );
    assert!(matches!(validate_message(&bad), Err(Invalid::UnknownReference(f)) if f == "nowhere"));

    let scalar = msg(
        "Container",
        vec![block(), placed("n", Kind::Scalar(Scalar::U(32)), "table_at", false)],
    );
    assert!(validate_message(&scalar).is_err());

    let empty = msg(
        "Container",
        vec![
            block(),
            placed("table", Kind::Nested { ty: "Table".into(), bytes: 0 }, "table_at", false),
        ],
    );
    assert!(validate_message(&empty).is_err());
}

#[test]
fn a_placed_record_may_be_gated_on_the_length_that_measures_it() {
    let msg_with = |absent: Option<Absence>| {
        msg(
            "Container",
            vec![
                Segment::Block(vec![
                    field("table_at", Kind::Scalar(Scalar::U(16))),
                    field("table_size", Kind::Scalar(Scalar::U(16))),
                ]),
                Segment::Placed {
                    stated: None,
                    name: "table".into(),
                    kind: Kind::Msg { ty: "Table".into(), boxed: false, with: Vec::new() },
                    at: "table_at".into(),
                    absent,
                    doc: None,
                },
            ],
        )
    };
    let m = msg_with(Some(Absence::ZeroLength { by: "table_size".into() }));
    assert_eq!(validate_message(&m), Ok(()));

    assert!(m.segments[1].places().is_empty());
    assert_eq!(m.segments[1].placed_optionally(), Some("table"));

    let unknown = msg_with(Some(Absence::ZeroLength { by: "nowhere".into() }));
    assert!(
        matches!(validate_message(&unknown), Err(Invalid::UnknownReference(f)) if f == "nowhere")
    );

    let same = msg_with(Some(Absence::ZeroLength { by: "table_at".into() }));
    assert!(validate_message(&same).is_err());
}

/// A placed run with a fixed byte window would render the same row as a placed record.
#[test]
fn a_run_the_wire_places_may_not_state_a_literal_window() {
    let run = |at: Option<String>| {
        msg(
            "Ambiguous",
            vec![
                Segment::Block(vec![field("off", Kind::Scalar(Scalar::U(16)))]),
                Segment::Repeat {
                    stated: None,
                    collection: Collection::Vec,
                    name: "tables".into(),
                    element: Kind::Nested { ty: "Table".into(), bytes: 8 },
                    count: Count::Window(Len::Bytes(16)),
                    at,
                    doc: None,
                },
            ],
        )
    };
    assert_eq!(validate_message(&run(None)), Ok(()));
    assert_eq!(run(None).segments[1].fixed_bits(), Some(128));

    assert!(validate_message(&run(Some("off".into()))).is_err());
}

#[test]
fn a_word_container_may_be_wider_than_a_register() {
    let wide = LayoutDef::new(
        "CanFdPayload",
        Container::word(512, Endian::Be, layline_codegen::BitOrder::Msb),
        vec![
            field("version", Kind::Scalar(Scalar::U(4))),
            field("source", Kind::Scalar(Scalar::U(12))),
            field("body", Kind::Scalar(Scalar::U(64))),
            field("spare", Kind::Scalar(Scalar::U(64))),
        ],
    );
    assert!(matches!(
        validate_layout(&wide),
        Err(Invalid::Tiling { covered_bits: 144, declared_bits: 512 })
    ));

    let mut tiled = wide;
    tiled.fields.push(field("rest", Kind::bytes(46)));
    assert_eq!(validate_layout(&tiled), Ok(()));

    let mut ragged = tiled;
    ragged.container = Container::word(300, Endian::Be, layline_codegen::BitOrder::Msb);
    assert!(validate_layout(&ragged).is_err());
}
