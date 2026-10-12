use layline_codegen::{
    BitOrder, Container, Endian, Field, Item, Kind, LayoutDef, Len, Scalar, validate,
};

fn f(name: &str, kind: Kind) -> Field {
    Field::new(name, kind)
}

#[test]
fn a_container_refuses_a_kind_it_cannot_carry() {
    let cases: Vec<(&str, LayoutDef)> = vec![
        (
            "Words + Array(F32,3)",
            LayoutDef::new(
                "A",
                Container::Words { words: 6, endian: Endian::Le, order: BitOrder::Lsb },
                vec![f("a", Kind::array(Scalar::F32, 3))],
            ),
        ),
        (
            "Bytes{1} + eight Bool",
            LayoutDef::new(
                "B",
                Container::Bytes { bytes: 1, endian: Endian::Le },
                (0..8).map(|i| f(&format!("b{i}"), Kind::Scalar(Scalar::Bool))).collect(),
            ),
        ),
        (
            "Word{32} + Nested{4}",
            LayoutDef::new(
                "C",
                Container::word(32, Endian::Le, BitOrder::Lsb),
                vec![f("c", Kind::Nested { ty: "Head".into(), bytes: 4 })],
            ),
        ),
        (
            "Bytes + Text{Fixed}",
            LayoutDef::new(
                "D",
                Container::Bytes { bytes: 8, endian: Endian::Le },
                vec![f("d", Kind::Text { len: Len::Bytes(8), codec: Some("Ascii".into()) })],
            ),
        ),
        (
            "Word{32} + Scalar(F32)",
            LayoutDef::new(
                "E",
                Container::word(32, Endian::Le, BitOrder::Lsb),
                vec![f("e", Kind::Scalar(Scalar::F32))],
            ),
        ),
        (
            "Word{16} + Text",
            LayoutDef::new(
                "F",
                Container::word(16, Endian::Le, BitOrder::Lsb),
                vec![f("f", Kind::Text { len: Len::Bytes(2), codec: None })],
            ),
        ),
        (
            "Word{16} + Var",
            LayoutDef::new(
                "G",
                Container::word(16, Endian::Le, BitOrder::Lsb),
                vec![f("g", Kind::Var { ty: "Uleb128".into() })],
            ),
        ),
        (
            "Word{8} + ByteBool",
            LayoutDef::new(
                "I",
                Container::word(8, Endian::Le, BitOrder::Lsb),
                vec![f("i", Kind::Scalar(Scalar::ByteBool))],
            ),
        ),
        (
            "Words{1} + ByteBool + U8",
            LayoutDef::new(
                "J",
                Container::Words { words: 1, endian: Endian::Le, order: BitOrder::Lsb },
                vec![f("j", Kind::Scalar(Scalar::ByteBool)), f("k", Kind::Scalar(Scalar::U(8)))],
            ),
        ),
        (
            "Word{16} + Msg",
            LayoutDef::new(
                "H",
                Container::word(16, Endian::Le, BitOrder::Lsb),
                vec![f("h", Kind::Msg { ty: "Body".into(), boxed: false, with: Vec::new() })],
            ),
        ),
    ];
    for (label, l) in &cases {
        assert!(
            validate(&Item::Layout(l.clone())).is_err(),
            "{label} passed validation but would not compile"
        );
    }
}

#[test]
fn a_run_states_how_many_elements_it_has() {
    let l = LayoutDef::new(
        "M",
        Container::Bytes { bytes: 4, endian: Endian::Le },
        vec![Field::new("n", Kind::Array(Scalar::U(32), vec![]))],
    );
    let why = validate(&Item::Layout(l)).expect_err("an array needs dimensions").to_string();
    assert!(why.contains("no dimensions"), "{why}");
}

#[test]
fn a_byte_container_stores_a_bool_in_a_whole_byte() {
    let one_bit = LayoutDef::new(
        "Bits",
        Container::Bytes { bytes: 1, endian: Endian::Le },
        vec![f("on", Kind::Scalar(Scalar::Bool)), f("rest", Kind::Scalar(Scalar::U(7)))],
    );
    assert!(validate(&Item::Layout(one_bit)).is_err());

    let whole = LayoutDef::new(
        "Whole",
        Container::Bytes { bytes: 2, endian: Endian::Le },
        vec![f("on", Kind::Scalar(Scalar::ByteBool)), f("rest", Kind::Scalar(Scalar::U(8)))],
    );
    assert_eq!(validate(&Item::Layout(whole)), Ok(()));
}

#[test]
fn a_byte_message_refuses_a_one_bit_bool() {
    use layline_codegen::{MessageDef, Segment};
    let m = |s: Scalar| {
        Item::Message(MessageDef::new(
            "Reply",
            vec![Segment::block(vec![
                f("on", Kind::Scalar(s)),
                f("n", Kind::Scalar(Scalar::U(8))),
            ])],
        ))
    };
    let why = validate(&m(Scalar::Bool)).expect_err("one bit in a byte message").to_string();
    assert!(why.contains("Scalar::ByteBool"), "{why}");
    assert_eq!(validate(&m(Scalar::ByteBool)), Ok(()));
}

#[test]
fn an_alignment_pads_by_two_or_more() {
    use layline_codegen::{By, Collection, Count, MessageDef, Segment};
    let m = |align| {
        Item::Message(MessageDef::new(
            "Frame",
            vec![
                Segment::block(vec![f("n", Kind::Scalar(Scalar::U(8)))]),
                Segment::repeat(
                    "data",
                    Kind::Scalar(Scalar::U(8)),
                    Count::Window(Len::Field { by: By::field("n"), cap: None, align }),
                    None,
                    Collection::Vec,
                ),
            ],
        ))
    };
    assert_eq!(validate(&m(Some(2))), Ok(()));
    let why = validate(&m(Some(1))).expect_err("pads nothing").to_string();
    assert!(why.contains("pads nothing"), "{why}");
}
