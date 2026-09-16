//! Checks on emitted source, and on the model and the derive agreeing about a layout.
//!
//! There are no snapshots. Each test checks one property of the output.

#![cfg(feature = "emit")]

use layline_codegen::__derive::field_rows;
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
