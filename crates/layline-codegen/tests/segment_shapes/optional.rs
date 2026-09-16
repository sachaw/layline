//! Optional fields.

use super::*;

#[test]
fn a_predicate_must_name_an_earlier_number() {
    let missing = msg("Missing", vec![opt("flags", 1, field("x", Kind::Scalar(Scalar::U(8))))]);
    assert_eq!(validate_message(&missing), Err(Invalid::UnknownReference("flags".into())));

    let later = msg(
        "Later",
        vec![
            opt("flags", 1, field("x", Kind::Scalar(Scalar::U(8)))),
            Segment::Block(vec![field("flags", Kind::Scalar(Scalar::U(8)))]),
        ],
    );
    assert_eq!(validate_message(&later), Err(Invalid::UnknownReference("flags".into())));

    let not_a_number = msg(
        "Wrong",
        vec![
            Segment::Value {
                stated: None,
                name: "flags".into(),
                kind: Kind::Text { len: Len::Until { terminator: 0 }, codec: None },
                doc: None,
            },
            opt("flags", 1, field("x", Kind::Scalar(Scalar::U(8)))),
        ],
    );
    assert_eq!(validate_message(&not_a_number), Err(Invalid::NonIntegerReference("flags".into())));
}

#[test]
fn an_optional_field_binds_no_name() {
    let m = msg(
        "Chained",
        vec![
            Segment::Block(vec![field("flags", Kind::Scalar(Scalar::U(8)))]),
            opt("flags", 1, field("n", Kind::Scalar(Scalar::U(8)))),
            Segment::repeat(
                "items",
                Kind::Scalar(Scalar::U(8)),
                Count::Field { by: By::field("n"), cap: None },
                None,
                Collection::Vec,
            ),
        ],
    );
    assert_eq!(validate_message(&m), Err(Invalid::UnknownReference("n".into())));
}

#[test]
fn a_bare_flag_must_name_an_earlier_bool() {
    let flag = |target: &str| Segment::Opt {
        when: Presence::Flag { field: target.into() },
        field: field("x", Kind::Scalar(Scalar::U(8))),
    };

    let missing = msg("Missing", vec![flag("urgent")]);
    assert_eq!(validate_message(&missing), Err(Invalid::UnknownReference("urgent".into())));

    let a_number = msg(
        "Word",
        vec![Segment::Block(vec![field("flags", Kind::Scalar(Scalar::U(8)))]), flag("flags")],
    );
    assert_eq!(validate_message(&a_number), Err(Invalid::NotAFlagReference("flags".into())));

    let text = msg(
        "Text",
        vec![
            Segment::Value {
                stated: None,
                name: "label".into(),
                kind: Kind::Text { len: Len::Until { terminator: 0 }, codec: None },
                doc: None,
            },
            flag("label"),
        ],
    );
    assert_eq!(validate_message(&text), Err(Invalid::NotAFlagReference("label".into())));

    let into_a_scalar = msg(
        "Reach",
        vec![Segment::Block(vec![field("flags", Kind::Scalar(Scalar::U(8)))]), flag("flags.bit")],
    );
    assert_eq!(
        validate_message(&into_a_scalar),
        Err(Invalid::NotANestedLayout("flags.bit".into()))
    );
}

#[test]
fn a_bool_answers_only_the_presence_question() {
    let block = || Segment::Block(vec![field("urgent", Kind::Scalar(Scalar::Bool))]);

    let asked_a_flag = msg(
        "Flagged",
        vec![
            block(),
            Segment::Opt {
                when: Presence::Flag { field: "urgent".into() },
                field: field("deadline", Kind::Scalar(Scalar::U(16))),
            },
        ],
    );
    assert_eq!(validate_message(&asked_a_flag), Ok(()));

    let asked_a_number = msg(
        "Counted",
        vec![
            block(),
            Segment::repeat(
                "items",
                Kind::Scalar(Scalar::U(8)),
                Count::Field { by: By::field("urgent"), cap: None },
                None,
                Collection::Vec,
            ),
        ],
    );
    assert_eq!(
        validate_message(&asked_a_number),
        Err(Invalid::NonIntegerReference("urgent".into())),
        "a `bool` is not a count"
    );

    let masked = msg(
        "Masked",
        vec![block(), opt("urgent", 0x01, field("deadline", Kind::Scalar(Scalar::U(16))))],
    );
    assert_eq!(validate_message(&masked), Err(Invalid::NonIntegerReference("urgent".into())));
}

#[test]
fn a_mask_must_select_something() {
    let m = msg(
        "Never",
        vec![
            Segment::Block(vec![field("flags", Kind::Scalar(Scalar::U(8)))]),
            opt("flags", 0, field("x", Kind::Scalar(Scalar::U(8)))),
        ],
    );
    assert!(validate_message(&m).is_err());
}

#[test]
fn an_optional_string_delimits_itself() {
    let m = msg(
        "Prefixed",
        vec![
            Segment::Block(vec![
                field("flags", Kind::Scalar(Scalar::U(8))),
                field("n", Kind::Scalar(Scalar::U(8))),
            ]),
            opt(
                "flags",
                1,
                field(
                    "s",
                    Kind::Text { len: Len::Field { by: By::field("n"), cap: None }, codec: None },
                ),
            ),
        ],
    );
    assert!(validate_message(&m).is_err());
}

#[test]
fn an_optional_fill_is_open_ended() {
    let fill = opt("flags", 0x80, field("note", Kind::Text { len: Len::Fill, codec: None }));
    assert!(fill.open_ended());
    assert!(!opt("flags", 1, field("x", Kind::Scalar(Scalar::U(8)))).open_ended());

    let m = msg(
        "Greedy",
        vec![
            Segment::Block(vec![field("flags", Kind::Scalar(Scalar::U(8)))]),
            fill,
            Segment::Block(vec![field("after", Kind::Scalar(Scalar::U(8)))]),
        ],
    );
    assert!(matches!(validate_message(&m), Err(Invalid::AfterOpenEnd(_))));
}
