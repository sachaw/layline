//! Checks on emitted source, and on the model and the derive agreeing about a layout.
//!
//! There are no snapshots. Each test checks one property of the output.

#![cfg(feature = "emit")]

use layline_codegen::__derive::field_rows;
use layline_codegen::Derive;
use layline_codegen::emit::{Module, generate};
use layline_codegen::{BitOrder, Container, Endian, Field, Item, Kind, LayoutDef, Scalar, Stated};

#[test]
fn a_word_container_emits_as_one_extent() {
    let module = Module::new(vec![Item::Layout(LayoutDef::new(
        "Status",
        Container::word(16, Endian::Be, BitOrder::Msb),
        vec![
            Field::new("version", Kind::Scalar(Scalar::U(4))).with_stated(Stated::bit(0)),
            Field::new("kind", Kind::Scalar(Scalar::U(8))).with_stated(Stated::bit(4)),
            Field::new("spare", Kind::Scalar(Scalar::U(4))).with_stated(Stated::bit(12)),
        ],
    ))]);

    let emitted = generate(&module).expect("the specification emits").source;
    assert!(
        emitted.contains("#[layout(bits = 16, endian = be, order = msb)]"),
        "one 16-bit container, not a run of words:\n{emitted}"
    );
}

/// Declares one layout as a model and as a `#[derive(Layout)]` type, then compares the field tables.
#[test]
fn the_published_rows_are_where_the_derive_puts_a_prefixed_layout() {
    use layline::Layout;

    #[derive(Debug, Clone, PartialEq, layline::Layout)]
    #[layout(bits = 72, prefix = 7)]
    struct Prefixed {
        #[bits(7)]
        channel_original: u8,
        #[bits(7)]
        relay_delay_2: u8,
        #[bits(51)]
        spare: u64,
    }

    let model = LayoutDef::new(
        "Prefixed",
        Container::Word {
            bits: 72,
            prefix: 7,
            prefix_value: None,
            endian: Endian::Le,
            order: BitOrder::Lsb,
        },
        vec![
            Field::new("channel_original", Kind::Scalar(Scalar::U(7))),
            Field::new("relay_delay_2", Kind::Scalar(Scalar::U(7))),
            Field::new("spare", Kind::Scalar(Scalar::U(51))),
        ],
    );
    layline_codegen::validate(&layline_codegen::Item::Layout(model.clone()))
        .expect("the fields fill the bits after the prefix");

    let host: Vec<(String, u64, u32)> = field_rows(&model.fields, &model.container)
        .into_iter()
        .map(|r| (r.name, r.extent.start(), r.extent.width()))
        .collect();
    let compiled: Vec<(String, u64, u32)> = Prefixed::FIELDS
        .iter()
        .map(|f| (f.name.to_string(), f.extent.start(), f.extent.width()))
        .collect();
    assert_eq!(host, compiled, "the model and the derive disagree");
}

/// A derive behind a `cfg` reaches generated code without making the dependency required.
#[test]
fn a_gated_derive_is_one_cfg_attr_per_predicate() {
    let module = Module::new(vec![Item::Layout(LayoutDef::new(
        "Head",
        Container::Bytes { bytes: 2, endian: Endian::Be },
        vec![Field::new("len", Kind::Scalar(Scalar::U(16)))],
    ))])
    .with_derives(vec![
        "Copy".into(),
        Derive::new("serde::Serialize").with_cfg("feature = \"serde\""),
        Derive::new("serde::Deserialize").with_cfg("feature = \"serde\""),
        Derive::new("arbitrary::Arbitrary").with_cfg("test"),
    ]);

    let emitted = generate(&module).expect("the module emits").source;
    assert!(emitted.contains("#[derive(Debug, Clone, PartialEq, Copy)]"), "{emitted}");
    assert!(
        emitted.contains(
            "#[cfg_attr(feature = \"serde\", derive(serde::Serialize, serde::Deserialize))]"
        ),
        "the two serde derives share one attribute:\n{emitted}"
    );
    assert!(emitted.contains("#[cfg_attr(test, derive(arbitrary::Arbitrary))]"), "{emitted}");
}

/// An enum takes the derives it is given and no others, as a layout does.
#[test]
fn an_enum_derives_only_what_it_is_given() {
    use layline_codegen::{EnumDef, Variant};

    let catalogue = EnumDef::new("Mode", Scalar::U(2), vec![Variant::new(0, "Idle")])
        .with_other("Unlisted")
        .with_derives(vec!["Copy".into(), "Eq".into()]);
    let emitted = generate(&Module::new(vec![Item::Enum(catalogue)])).expect("emits").source;
    assert!(emitted.contains("#[derive(Debug, Clone, PartialEq, Copy, Eq)]"), "{emitted}");
    assert_eq!(emitted.matches("Copy").count(), 1, "one derive, not two:\n{emitted}");
}

/// A module holding both kinds states `Copy` once, and both items take it.
#[test]
fn an_item_adds_its_own_derives_to_the_modules() {
    use layline_codegen::{EnumDef, Variant};

    let catalogue = EnumDef::new("Mode", Scalar::U(2), vec![Variant::new(0, "Idle")])
        .with_other("Unlisted")
        .with_default("Idle")
        .with_derives(vec!["Default".into()]);
    let layout = LayoutDef::new(
        "Head",
        Container::Bytes { bytes: 2, endian: Endian::Be },
        vec![Field::new("len", Kind::Scalar(Scalar::U(16)))],
    )
    .with_doc("The header, from figure 30.");

    let module = Module::new(vec![Item::Enum(catalogue), Item::Layout(layout)])
        .with_derives(vec!["Copy".into(), "Eq".into()]);
    let emitted = generate(&module).expect("emits").source;

    assert!(emitted.contains("#[derive(Debug, Clone, PartialEq, Copy, Eq, Default)]"), "{emitted}");
    assert!(emitted.contains("#[default]"), "the default variant is marked:\n{emitted}");
    assert!(emitted.contains("/// The header, from figure 30."), "{emitted}");
    assert!(
        emitted.contains("#[derive(Debug, Clone, PartialEq, Copy, Eq)]"),
        "the layout takes the module's derives alone:\n{emitted}"
    );
}

#[test]
fn a_default_variant_and_a_default_derive_have_to_agree() {
    use layline_codegen::{Derive, EnumDef, Error, Variant};

    let catalogue =
        || EnumDef::new("Mode", Scalar::U(2), vec![Variant::new(0, "Idle")]).with_other("Unlisted");
    let emit = |e: EnumDef, derives: Vec<Derive>| {
        generate(&Module::new(vec![Item::Enum(e)]).with_derives(derives))
    };

    let Err(Error::Invalid(_, why)) = emit(catalogue().with_default("Idle"), Vec::new()) else {
        panic!("a default variant with nothing deriving `Default` must be refused");
    };
    assert!(format!("{why:?}").contains("Add an ungated `Default`"), "{why:?}");

    let Err(Error::Invalid(_, why)) = emit(catalogue(), vec!["Default".into()]) else {
        panic!("deriving `Default` with no default variant must be refused");
    };
    assert!(format!("{why:?}").contains("with_default"), "{why:?}");

    let gated = vec![Derive::new("Default").with_cfg("feature = \"x\"")];
    let Err(Error::Invalid(_, why)) = emit(catalogue().with_default("Idle"), gated) else {
        panic!("a gated `Default` leaves `#[default]` without a derive when the cfg is off");
    };
    assert!(format!("{why:?}").contains("ungated"), "{why:?}");

    emit(catalogue().with_default("Idle"), vec!["Default".into()]).expect("agreeing is accepted");
}

/// A catalogue states where its id sits, and what keeps an unknown body.
#[test]
fn a_dispatch_states_its_prefix_and_its_fallback_body() {
    use layline_codegen::{DispatchArm, DispatchDef};

    let arms = vec![DispatchArm::new(0b101, "Ping", "Ping")];
    let plain = DispatchDef::new("Frame", Scalar::U(8), arms.clone());
    let emitted = generate(&Module::new(vec![Item::Dispatch(plain)])).expect("emits").source;
    assert!(emitted.contains("#[dispatch(id = u8)]"), "{emitted}");
    assert!(emitted.contains("body: Vec<u8>"), "the default keeps allocating:\n{emitted}");

    let stated = DispatchDef::new("Frame", Scalar::U(8), arms)
        .with_prefix(7)
        .with_other_body("UnknownCwBody");
    let emitted = generate(&Module::new(vec![Item::Dispatch(stated)])).expect("emits").source;
    assert!(emitted.contains("#[dispatch(id = u8, prefix = 7)]"), "{emitted}");
    assert!(emitted.contains("body: UnknownCwBody"), "{emitted}");
    assert!(!emitted.contains("Vec<u8>"), "nothing is left of the default:\n{emitted}");
    assert!(
        !emitted.contains("__private"),
        "a catalogue that keeps its body without allocating needs no prelude:\n{emitted}"
    );
}

#[test]
fn a_fallback_body_has_to_be_a_type_path() {
    use layline_codegen::{DispatchArm, DispatchDef, Error};

    let d = DispatchDef::new("Frame", Scalar::U(8), vec![DispatchArm::new(1, "Ping", "Ping")])
        .with_other_body("not a type!!");
    let Err(Error::Invalid(_, why)) = generate(&Module::new(vec![Item::Dispatch(d)])) else {
        panic!("a fallback body that is not a type path must be refused");
    };
    assert!(format!("{why:?}").contains("not a Rust type path"), "{why:?}");
}
