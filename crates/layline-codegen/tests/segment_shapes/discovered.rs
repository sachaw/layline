//! Values whose size is read from the wire.

use super::*;

#[test]
fn the_string_shapes_are_extents_not_features() {
    for len in [
        Len::Until { terminator: 0 },
        Len::Bytes(16),
        Len::Field { by: By::field("n"), cap: Some(4096) },
        Len::Fill,
    ] {
        let open_ended = matches!(len, Len::Fill);
        let m = msg(
            "Record",
            vec![
                Segment::Block(vec![field("n", Kind::Scalar(Scalar::U(16)))]),
                Segment::Value {
                    stated: None,
                    name: "name".into(),
                    kind: Kind::Text { len: len.clone(), codec: None },
                    doc: None,
                },
            ],
        );
        assert_eq!(validate_message(&m), Ok(()), "{len:?}");

        let mut segments = m.segments.clone();
        segments.push(Segment::Block(vec![field("crc", Kind::Scalar(Scalar::U(16)))]));
        let after = msg("Record", segments);
        assert_eq!(validate_message(&after).is_err(), open_ended, "a segment after {len:?}",);
    }
}

#[test]
fn a_length_prefix_must_name_an_earlier_field() {
    let m = msg(
        "Record",
        vec![Segment::Value {
            stated: None,
            name: "name".into(),
            kind: Kind::Text {
                len: Len::Field { by: By::field("nowhere"), cap: None },
                codec: None,
            },
            doc: None,
        }],
    );
    assert!(matches!(validate_message(&m), Err(Invalid::UnknownReference(f)) if f == "nowhere"));
}

#[test]
fn a_discovered_extent_cannot_live_in_a_block() {
    for kind in [
        Kind::Var { ty: "Uleb128".into() },
        Kind::Text { len: Len::Until { terminator: 0 }, codec: Some("Latin1".into()) },
    ] {
        let m = msg(
            "Bad",
            vec![Segment::Block(vec![
                field("a", Kind::Scalar(Scalar::U(8))),
                field("b", kind.clone()),
            ])],
        );
        assert!(validate_message(&m).is_err(), "{kind:?}");
    }

    let fixed = Kind::Text { len: Len::Bytes(8), codec: Some("Ascii".into()) };
    assert_eq!(fixed.bits(), Some(64));
}

#[test]
fn a_repeated_string_must_delimit_itself() {
    for len in [Len::Field { by: By::field("n"), cap: None }, Len::Fill] {
        let m = msg(
            "Bad",
            vec![
                Segment::Block(vec![field("n", Kind::Scalar(Scalar::U(16)))]),
                Segment::repeat(
                    "names",
                    Kind::Text { len: len.clone(), codec: None },
                    Count::Fill { cap: None },
                    None,
                    Collection::Vec,
                ),
            ],
        );
        assert!(validate_message(&m).is_err(), "{len:?}");
    }

    for len in [Len::Until { terminator: 0 }, Len::Bytes(12)] {
        let m = msg(
            "StringTable",
            vec![Segment::repeat(
                "names",
                Kind::Text { len: len.clone(), codec: None },
                Count::Fill { cap: None },
                None,
                Collection::Vec,
            )],
        );
        assert_eq!(validate_message(&m), Ok(()), "{len:?}");
    }
}

#[test]
fn a_value_segment_rejects_a_solved_kind() {
    let m = msg(
        "Bad",
        vec![Segment::Value {
            name: "n".into(),
            kind: Kind::Scalar(Scalar::U(32)),
            doc: None,
            stated: None,
        }],
    );
    assert!(validate_message(&m).is_err());
}
