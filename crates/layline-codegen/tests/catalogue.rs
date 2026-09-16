//! Validation rules for enums, choices and dispatch tables.
#![cfg(feature = "walk")]

use layline_codegen::__derive::{choice_parts, enum_parts, layout_parts, message_parts};
use layline_codegen::{
    Arm, ChoiceDef, Collection, Container, Count, DispatchArm, DispatchDef, Endian, EnumDef, Field,
    Item, Kind, LayoutDef, MessageDef, Range, Root, Scalar, Segment, Variant, validate,
};

fn refused(e: &EnumDef) -> String {
    match enum_parts(e, &Root::default()) {
        Ok(p) => panic!("accepted an invalid enum:\n{}", p.codec),
        Err(e) => format!("{e:?}"),
    }
}

fn base() -> EnumDef {
    EnumDef::new("Mode", Scalar::U(8), vec![Variant::new(0, "Idle"), Variant::new(7, "Active")])
}

fn open_with(variants: Vec<Variant>) -> EnumDef {
    let mut e = base().with_other("Other");
    e.variants = variants;
    e
}

#[test]
fn a_closed_catalogue_has_to_be_total() {
    assert!(refused(&base()).contains("254"));
}

#[test]
fn a_value_has_to_fit_what_carries_it() {
    let e = open_with(vec![Variant::new(999, "Big")]);
    assert!(refused(&e).contains("0..=255"));
}

#[test]
fn a_float_is_not_a_representation() {
    let mut e = base().with_other("Other");
    e.repr = Scalar::F32;
    assert!(refused(&e).contains("float"));
}

#[test]
fn one_value_has_one_name() {
    let e = open_with(vec![Variant::new(1, "A"), Variant::new(1, "B")]);
    assert!(refused(&e).contains("DuplicateArm"));
}

#[test]
fn every_claim_a_field_makes_reaches_the_declaration() {
    let l = LayoutDef::new(
        "Both",
        Container::Bytes { bytes: 2, endian: Endian::Le },
        vec![
            Field::new("m", Kind::Scalar(Scalar::U(16)))
                .with_magic(&[1, 2])
                .with_range(Range::new(None, Some(255))),
        ],
    );
    let parts = layout_parts(&l, &Root::default()).expect("renders");
    let rendered = parts.fields.to_string();
    assert!(rendered.contains("magic"), "the constant is missing: {rendered}");
    assert!(rendered.contains("range"), "the bound is missing: {rendered}");
}

fn open_then_block() -> MessageDef {
    MessageDef::new(
        "AfterFill",
        vec![
            Segment::repeat(
                "body",
                Kind::Scalar(Scalar::U(8)),
                Count::Fill { cap: None },
                None,
                Collection::Vec,
            ),
            Segment::block(vec![Field::new("trailer", Kind::Scalar(Scalar::U(16)))]),
        ],
    )
}

fn duplicate_arms() -> ChoiceDef {
    let arm = |name: &str, field: &str| {
        Arm::new(1, name, vec![Segment::block(vec![Field::new(field, Kind::Scalar(Scalar::U(8)))])])
    };
    ChoiceDef::new("Twice", vec![arm("A", "a"), arm("B", "b")])
}

/// A checksum in an arm starts at the arm's first byte, so the arm records that offset.
#[test]
fn an_arm_binds_the_base_its_checksum_counts_from() {
    let arm = Arm::new(
        1,
        "Summed",
        vec![
            Segment::block(vec![Field::new("a", Kind::Scalar(Scalar::U(8)))]),
            Segment::Checksum {
                stated: None,
                name: "ck".into(),
                repr: Scalar::U(8),
                algorithm: "Xor".into(),
                over: layline_codegen::Coverage::whole(),
                doc: None,
            },
        ],
    );
    let choice = ChoiceDef::new("Summed", vec![arm]);
    let parts = choice_parts(&choice, &Root::default()).expect("renders");
    let codec = parts.codec.to_string();
    assert!(codec.contains("__base"), "the checksum reads `__base`: {codec}");
    assert!(codec.contains("let __base = out . len ()"), "the arm sets `__base`: {codec}");
}

#[test]
fn the_walk_refuses_a_model_the_validator_refuses() {
    let root = Root::default();

    assert!(validate(&Item::Message(open_then_block())).is_err(), "the validator refuses it");
    assert!(message_parts(&open_then_block(), &root).is_err(), "the generator rejects it too");

    assert!(validate(&Item::Choice(duplicate_arms())).is_err(), "the validator refuses it");
    assert!(choice_parts(&duplicate_arms(), &root).is_err(), "the generator rejects it too");
}

#[test]
fn a_dispatcher_owned_prefix_reaches_the_declaration() {
    let l = LayoutDef::new(
        "Body",
        Container::Word {
            bits: 32,
            prefix: 8,
            prefix_value: None,
            endian: Endian::Be,
            order: layline_codegen::BitOrder::Msb,
        },
        vec![Field::new("rest", Kind::Scalar(Scalar::U(24)))],
    );
    validate(&Item::Layout(l.clone())).expect("the fields fill the bits after the prefix");

    let parts = layout_parts(&l, &Root::default()).expect("renders");
    let rendered = parts.attrs.to_string();
    assert!(rendered.contains("prefix = 8"), "the prefix is missing: {rendered}");

    let whole = LayoutDef::new(
        &l.name,
        Container::word(32, Endian::Be, layline_codegen::BitOrder::Msb),
        vec![Field::new("all", Kind::Scalar(Scalar::U(32)))],
    );
    let rendered = layout_parts(&whole, &Root::default()).expect("renders").attrs.to_string();
    assert!(!rendered.contains("prefix"), "`prefix = 0` is omitted: {rendered}");
}

#[test]
fn a_prefix_the_size_of_the_container_is_refused() {
    let why = validate(&Item::Layout(LayoutDef::new(
        "Nothing",
        Container::Word {
            bits: 32,
            prefix: 32,
            prefix_value: None,
            endian: Endian::Be,
            order: layline_codegen::BitOrder::Msb,
        },
        vec![Field::new("rest", Kind::Scalar(Scalar::U(8)))],
    )))
    .expect_err("the prefix leaves no bits");
    assert!(format!("{why:?}").contains("no bits"), "says no bits are left: {why:?}");
}

#[test]
fn a_body_catalogue_is_checked_before_it_is_tokens() {
    let base =
        |arms: Vec<DispatchArm>| DispatchDef::new("P", Scalar::U(8), arms).with_other("Unknown");

    let twice = base(vec![DispatchArm::new(1, "A", "A"), DispatchArm::new(1, "B", "B")]);
    assert!(
        matches!(
            validate(&Item::Dispatch(twice.clone())),
            Err(layline_codegen::Invalid::DuplicateArm(1))
        ),
        "duplicate id",
    );

    let wide = base(vec![DispatchArm::new(300, "A", "A")]);
    assert!(validate(&Item::Dispatch(wide.clone())).is_err(), "300 does not fit eight bits");

    let nameless = base(vec![DispatchArm::new(1, "A", "A")]).with_other("");
    assert!(validate(&Item::Dispatch(nameless.clone())).is_err(), "the fallback arm needs a name",);

    let clash = base(vec![DispatchArm::new(1, "A", "A")]).with_other("A");
    assert!(
        validate(&Item::Dispatch(clash.clone())).is_err(),
        "an arm cannot be both listed and the fallback",
    );
}
