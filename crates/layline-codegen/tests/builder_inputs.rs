#[path = "support/mod.rs"]
mod support;

use layline_codegen::{
    Collection, Container, Count, Discriminant, Endian, Field, Invalid, Kind, LayoutDef,
    MessageDef, Scalar, Segment,
};
use support::{bytes, validate_layout, validate_message};

#[test]
fn a_field_whose_type_is_not_a_type_is_invalid_not_a_panic() {
    let l = bytes(1, vec![Field::new("a", Kind::Codec { ty: "not a type!!".into(), bits: 8 })]);
    let Err(Invalid::Field { at, why }) = validate_layout(&l) else {
        panic!("expected `Invalid::Field` for a type that does not parse");
    };
    assert_eq!(at, "a");
    assert!(why.contains("not a type!!"), "names the bad type: {why}");
    assert!(why.contains("not a Rust type path"), "{why}");
}

#[test]
fn every_kind_that_names_a_type_is_checked() {
    let bad = |kind: Kind, len: usize| {
        let l = bytes(len, vec![Field::new("f", kind)]);
        assert!(matches!(validate_layout(&l), Err(Invalid::Field { .. })), "{l:?}");
    };
    bad(Kind::Nested { ty: "no!".into(), bytes: 4 }, 4);
    bad(Kind::NestedArray { ty: "no!".into(), bytes: 4, len: 2 }, 8);
    bad(Kind::checksum(Scalar::U(8), "no!", layline_codegen::Coverage::whole()), 1);

    let msg = |seg: Segment| MessageDef::new("M", vec![seg]);
    for m in [
        msg(Segment::value("v", Kind::Var { ty: "no!".into() })),
        msg(Segment::value("v", Kind::Msg { ty: "no!".into(), boxed: false, with: Vec::new() })),
        msg(Segment::repeat(
            "r",
            Kind::Msg { ty: "no!".into(), boxed: false, with: Vec::new() },
            Count::Fill { cap: None },
            None,
            Collection::Vec,
        )),
        msg(Segment::switch("s", Discriminant::BodyLength, "no!", None)),
        msg(Segment::checksum("c", Scalar::U(8), "no!", layline_codegen::Coverage::whole())),
    ] {
        assert!(matches!(validate_message(&m), Err(Invalid::Field { .. })), "{m:?}");
    }
}

#[test]
fn a_name_that_is_not_an_identifier_is_invalid() {
    let l = bytes(1, vec![Field::new("not an ident", Kind::Scalar(Scalar::U(8)))]);
    let Err(Invalid::Other(why)) = validate_layout(&l) else {
        panic!("expected `Invalid::Other` for a field name with a space");
    };
    assert!(why.contains("`not an ident`") && why.contains("identifier"), "{why}");

    let l = LayoutDef::new(
        "1L",
        Container::Bytes { bytes: 1, endian: Endian::Le },
        vec![Field::new("a", Kind::Scalar(Scalar::U(8)))],
    );
    assert!(
        matches!(validate_layout(&l), Err(Invalid::Other(_))),
        "rejects a type name starting with a digit"
    );

    let m = MessageDef::new(
        "M",
        vec![
            Segment::block(vec![Field::new("head", Kind::Nested { ty: "Head".into(), bytes: 1 })]),
            Segment::repeat(
                "items",
                Kind::Scalar(Scalar::U(8)),
                Count::Field { by: layline_codegen::By::field("head.n!"), cap: None },
                None,
                Collection::Vec,
            ),
        ],
    );
    assert!(
        matches!(validate_message(&m), Err(Invalid::Other(_))),
        "rejects a bad name after the dot"
    );
}

#[cfg(feature = "emit")]
#[test]
fn a_module_string_that_does_not_parse_is_invalid() {
    use layline_codegen::Error;
    use layline_codegen::Item;
    use layline_codegen::emit::{Module, generate};

    let item = || Item::Layout(bytes(1, vec![Field::new("a", Kind::Scalar(Scalar::U(8)))]));

    let derives = Module::new(vec![item()]).with_derives(vec!["not a path!!".into()]);
    let Err(Error::Invalid(what, Invalid::Other(why))) = generate(&derives) else {
        panic!("expected `Error::Invalid` for a derive that is not a path");
    };
    assert_eq!(what, "derives");
    assert!(why.contains("not a path!!"), "{why}");

    let uses = Module::new(vec![item()]).with_uses(vec!["not a use!!".into()]);
    let Err(Error::Invalid(what, Invalid::Other(why))) = generate(&uses) else {
        panic!("expected `Error::Invalid` for a `use` that is not a path");
    };
    assert_eq!(what, "uses");
    assert!(why.contains("not a use!!"), "{why}");

    let fine = Module::new(vec![item()])
        .with_derives(vec!["serde::Serialize".into()])
        .with_uses(vec!["super::Brand".into()]);
    generate(&fine).expect("valid paths");
}

#[cfg(feature = "emit")]
#[test]
fn a_derive_that_cannot_be_written_is_invalid() {
    use layline_codegen::Error;
    use layline_codegen::Item;
    use layline_codegen::emit::{Derive, Module, generate};

    let item = || Item::Layout(bytes(1, vec![Field::new("a", Kind::Scalar(Scalar::U(8)))]));

    let cfg =
        Module::new(vec![item()]).with_derives(vec![Derive::new("Copy").with_cfg("feature = ")]);
    let Err(Error::Invalid(what, Invalid::Other(why))) = generate(&cfg) else {
        panic!("expected `Error::Invalid` for a `cfg` that is not a predicate");
    };
    assert_eq!(what, "derives");
    assert!(why.contains("`cfg` predicate"), "{why}");

    let twice = Module::new(vec![item()])
        .with_derives(vec!["Copy".into(), Derive::new("Copy").with_cfg("test")]);
    let Err(Error::Invalid(_, Invalid::Other(why))) = generate(&twice) else {
        panic!("expected `Error::Invalid` for a derive listed twice");
    };
    assert!(why.contains("appears twice"), "{why}");
}

// ---------------------------------------------------------------------------
// Message parameters
// ---------------------------------------------------------------------------

fn needs(params: Vec<layline_codegen::Param>, segs: Vec<Segment>) -> MessageDef {
    MessageDef::new("M", segs).with_needs(params)
}

fn param(name: &str, repr: Scalar) -> layline_codegen::Param {
    layline_codegen::Param::new(name, repr)
}

fn refused(m: &MessageDef, reason: &str) {
    let Err(why) = validate_message(m) else {
        panic!("the model accepted `{m:?}`");
    };
    assert!(why.to_string().contains(reason), "rejected for a different reason: {why}");
}

#[test]
fn a_parameter_joins_the_name_environment_beside_the_fields() {
    let m = needs(
        vec![param("stride", Scalar::U(8))],
        vec![Segment::repeat(
            "data",
            Kind::Scalar(Scalar::U(8)),
            Count::Window(layline_codegen::Len::Field {
                by: layline_codegen::By::field("stride"),
                cap: None,
            }),
            None,
            Collection::Vec,
        )],
    );
    assert!(validate_message(&m).is_ok(), "{:?}", validate_message(&m));
}

#[test]
fn a_parameter_that_repeats_a_field_name_is_refused() {
    let m = needs(
        vec![param("n", Scalar::U(8))],
        vec![Segment::block(vec![Field::new("n", Kind::Scalar(Scalar::U(8)))])],
    );
    refused(&m, "is both a parameter and a field");
}

#[test]
fn a_parameter_declared_twice_is_refused() {
    let m = needs(
        vec![param("n", Scalar::U(8)), param("n", Scalar::U(16))],
        vec![Segment::block(vec![Field::new("a", Kind::Scalar(Scalar::U(8)))])],
    );
    refused(&m, "declares the parameter `n` twice");
}

#[test]
fn a_parameter_that_is_not_an_integer_is_refused() {
    let m = needs(
        vec![param("scale", Scalar::F32)],
        vec![Segment::block(vec![Field::new("a", Kind::Scalar(Scalar::U(8)))])],
    );
    refused(&m, "A parameter must be an integer");
}

#[test]
fn a_parameter_a_bit_addressed_message_states_is_refused() {
    let m = needs(
        vec![param("n", Scalar::U(8))],
        vec![Segment::block(vec![Field::new("a", Kind::Scalar(Scalar::U(4)))])],
    )
    .with_bits(layline_codegen::BitOrder::Lsb);
    refused(&m, "a bit-addressed message cannot take parameters");
}

#[test]
fn a_parameter_the_wire_would_write_back_is_refused() {
    let absent = needs(
        vec![param("flags", Scalar::U(8))],
        vec![
            Segment::block(vec![Field::new("a", Kind::Scalar(Scalar::U(8)))]),
            Segment::opt(
                layline_codegen::Presence::Mask { field: "flags".into(), mask: 1 },
                Field::new("stamp", Kind::Scalar(Scalar::U(16))),
            ),
        ],
    );
    refused(&absent, "`flags` is a parameter, and encode cannot write presence bits into it");

    let placed = needs(
        vec![param("off", Scalar::U(8))],
        vec![
            Segment::block(vec![Field::new("a", Kind::Scalar(Scalar::U(8)))]),
            Segment::placed("head", Kind::Nested { ty: "Head".into(), bytes: 2 }, "off", None),
        ],
    );
    refused(&placed, "`off` is a parameter, and encode cannot write a record offset into it");
}

#[test]
fn a_with_that_reaches_through_a_nested_layout_is_refused() {
    let m = MessageDef::new(
        "M",
        vec![
            Segment::block(vec![Field::new("head", Kind::Nested { ty: "Head".into(), bytes: 2 })]),
            Segment::value(
                "child",
                Kind::Msg { ty: "Block".into(), boxed: false, with: vec![String::from("head.n")] },
            ),
        ],
    );
    refused(&m, "Each parameter takes one field");
}

#[test]
fn a_with_that_names_nothing_is_refused() {
    let m = MessageDef::new(
        "M",
        vec![Segment::value(
            "child",
            Kind::Msg { ty: "Block".into(), boxed: false, with: vec![String::from("missing")] },
        )],
    );
    assert!(matches!(validate_message(&m), Err(Invalid::UnknownReference(_))));
}
