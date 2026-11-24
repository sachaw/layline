//! References to field names.

use super::*;

#[test]
fn a_count_may_not_name_a_later_discovered_value() {
    let m = msg(
        "Backwards",
        vec![
            Segment::repeat(
                "value",
                Kind::Scalar(Scalar::U(8)),
                Count::Field { by: Expr::name("len"), cap: None },
                None,
                Collection::Vec,
            ),
            Segment::Value {
                position: None,
                name: "len".into(),
                kind: Kind::Var { ty: "Uleb128".into() },
                doc: None,
            },
        ],
    );
    assert!(matches!(validate_message(&m), Err(Invalid::UnknownReference(f)) if f == "len"));
}

#[test]
fn a_reference_must_name_something_with_a_number_in_it() {
    let text = Kind::Text { len: Len::Until { terminator: 0 }, codec: None };
    let m = msg(
        "Bad",
        vec![
            Segment::Value { name: "label".into(), kind: text, doc: None, position: None },
            Segment::repeat(
                "items",
                Kind::Scalar(Scalar::U(8)),
                Count::Field { by: Expr::name("label"), cap: None },
                None,
                Collection::Vec,
            ),
        ],
    );
    assert!(matches!(
        validate_message(&m),
        Err(Invalid::NonIntegerReference(f)) if f == "label"
    ));

    let m = msg(
        "Bad",
        vec![
            Segment::Block(vec![field("pair", Kind::Nested { ty: "Pair".into(), bytes: 2 })]),
            Segment::repeat(
                "items",
                Kind::Scalar(Scalar::U(8)),
                Count::Field { by: Expr::name("pair"), cap: None },
                None,
                Collection::Vec,
            ),
        ],
    );
    assert!(matches!(
        validate_message(&m),
        Err(Invalid::NonIntegerReference(f)) if f == "pair"
    ));

    let m = msg(
        "Bad",
        vec![
            Segment::Block(vec![
                field("n", Kind::Scalar(Scalar::U(16))),
                field("ratio", Kind::Scalar(Scalar::F32)),
            ]),
            Segment::repeat(
                "items",
                Kind::Scalar(Scalar::U(8)),
                Count::Field { by: Expr::name("n"), cap: None },
                Some(layline_codegen::Expr::name("ratio")),
                Collection::Vec,
            ),
        ],
    );
    assert!(matches!(
        validate_message(&m),
        Err(Invalid::NonIntegerReference(f)) if f == "ratio"
    ));
}
