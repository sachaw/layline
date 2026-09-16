//! Recursion checks for cycles that span several items.
//!
//! Each test is the multi-item version of a check made on a single message.

#![cfg(feature = "emit")]

#[path = "support/mod.rs"]
mod support;

use layline_codegen::Item;
use layline_codegen::emit::{Module, generate};
use layline_codegen::{
    Arm, By, ChoiceDef, Collection, Count, Discriminant, Field, Kind, Len, Presence, Scalar,
    Segment,
};
use support::msg;

fn item(name: &str, segments: Vec<Segment>) -> Item {
    Item::Message(msg(name, segments))
}

/// A required nested message, stored inline unless `boxed`.
fn holds(field: &str, ty: &str, boxed: bool) -> Segment {
    Segment::Value {
        name: field.into(),
        kind: Kind::Msg { ty: ty.into(), boxed, with: Vec::new() },
        doc: None,
        stated: None,
    }
}

/// A sequence of `ty` sized by an earlier length field. It can be empty and lives on the heap.
fn sequence(field: &str, ty: &str, len: &str) -> Segment {
    Segment::repeat(
        field,
        Kind::Msg { ty: ty.into(), boxed: false, with: Vec::new() },
        Count::Window(Len::Field { by: By::field(len), cap: None }),
        None,
        Collection::Vec,
    )
}

fn module(items: Vec<Item>) -> Module {
    Module::new(items)
}

fn emit_module(module: &Module) -> Result<String, layline_codegen::Error> {
    generate(module).map(|g| g.source)
}

fn refusal(items: Vec<Item>) -> String {
    match emit_module(&module(items)) {
        Ok(src) => panic!("expected a refusal, got:\n{src}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn an_indirect_cycle_with_no_way_out_is_refused() {
    let why = refusal(vec![
        item("A", vec![holds("b", "B", true)]),
        item("B", vec![holds("a", "A", true)]),
    ]);
    assert!(why.contains("`A` -> `B` -> `A` recurses on every path, so no wire can end"), "{why}");
    assert!(why.contains("an empty collection"), "names the fix: {why}");
}

#[test]
fn one_avoidable_edge_makes_the_cycle_productive() {
    let items = vec![
        item("A", vec![holds("b", "B", true)]),
        item(
            "B",
            vec![
                Segment::Block(vec![Field::new("flags", Kind::Scalar(Scalar::U(8)))]),
                Segment::Opt {
                    when: Presence::Mask { field: "flags".into(), mask: 0x01 },
                    field: Field::new(
                        "a",
                        Kind::Msg { ty: "A".into(), boxed: true, with: Vec::new() },
                    ),
                },
            ],
        ),
    ];
    emit_module(&module(items)).expect("a flag bit is a base case");
}

#[test]
fn a_catalogue_with_no_terminating_arm_is_refused() {
    let why = refusal(vec![
        item(
            "Node",
            vec![
                Segment::Block(vec![Field::new("kind", Kind::Scalar(Scalar::U(8)))]),
                Segment::Switch {
                    stated: None,
                    name: "body".into(),
                    on: Discriminant::Field("kind".into()),
                    choice: "Body".into(),
                    window: None,
                    doc: None,
                },
            ],
        ),
        Item::Choice(ChoiceDef::new(
            "Body",
            vec![
                Arm::new(0, "Left", vec![holds("inner", "Node", true)]),
                Arm::new(1, "Right", vec![holds("inner", "Node", true)]),
            ],
        )),
    ]);
    assert!(why.contains("recurses on every path, so no wire can end"), "{why}");
    assert!(why.contains("an arm that leads out"), "names the catalogue fix: {why}");
}

#[test]
fn one_bounded_arm_is_enough_for_a_catalogue() {
    let items = vec![
        item(
            "Node",
            vec![
                Segment::Block(vec![Field::new("kind", Kind::Scalar(Scalar::U(8)))]),
                Segment::Switch {
                    stated: None,
                    name: "body".into(),
                    on: Discriminant::Field("kind".into()),
                    choice: "Body".into(),
                    window: None,
                    doc: None,
                },
            ],
        ),
        Item::Choice(ChoiceDef::new(
            "Body",
            vec![
                Arm::new(
                    0,
                    "Leaf",
                    vec![Segment::Block(vec![Field::new("v", Kind::Scalar(Scalar::U(8)))])],
                ),
                Arm::new(1, "Branch", vec![holds("inner", "Node", true)]),
            ],
        )),
    ];
    emit_module(&module(items)).expect("a leaf arm is a base case");
}

#[test]
fn an_open_catalogue_is_its_own_way_out() {
    let items = vec![
        item(
            "Node",
            vec![
                Segment::Block(vec![Field::new("kind", Kind::Scalar(Scalar::U(8)))]),
                Segment::Switch {
                    stated: None,
                    name: "body".into(),
                    on: Discriminant::Field("kind".into()),
                    choice: "Body".into(),
                    window: None,
                    doc: None,
                },
            ],
        ),
        Item::Choice(
            ChoiceDef::new("Body", vec![Arm::new(0, "Branch", vec![holds("inner", "Node", true)])])
                .with_other("Unknown"),
        ),
    ];
    emit_module(&module(items)).expect("the unknown arm is a base case");
}

#[test]
fn an_indirect_cycle_that_rust_cannot_size_is_refused() {
    let why = refusal(vec![
        item(
            "A",
            vec![
                Segment::Block(vec![Field::new("n", Kind::Scalar(Scalar::U(8)))]),
                Segment::Opt {
                    when: Presence::Mask { field: "n".into(), mask: 0x01 },
                    // `Option<B>` stores `B` inline, so it does not break the size cycle.
                    field: Field::new(
                        "b",
                        Kind::Msg { ty: "B".into(), boxed: false, with: Vec::new() },
                    ),
                },
            ],
        ),
        item(
            "B",
            vec![
                Segment::Block(vec![Field::new("n", Kind::Scalar(Scalar::U(8)))]),
                Segment::Opt {
                    when: Presence::Mask { field: "n".into(), mask: 0x01 },
                    field: Field::new(
                        "a",
                        Kind::Msg { ty: "A".into(), boxed: false, with: Vec::new() },
                    ),
                },
            ],
        ),
    ]);
    assert!(why.contains("`A` -> `B` -> `A` contains itself without indirection"), "{why}");
    assert!(why.contains("`Box<T>`"), "the fix: {why}");
}

#[test]
fn one_box_on_the_cycle_is_enough() {
    let items = vec![
        item(
            "A",
            vec![
                Segment::Block(vec![Field::new("n", Kind::Scalar(Scalar::U(8)))]),
                Segment::Opt {
                    when: Presence::Mask { field: "n".into(), mask: 0x01 },
                    field: Field::new(
                        "b",
                        Kind::Msg { ty: "B".into(), boxed: true, with: Vec::new() },
                    ),
                },
            ],
        ),
        item(
            "B",
            vec![
                Segment::Block(vec![Field::new("n", Kind::Scalar(Scalar::U(8)))]),
                Segment::Opt {
                    when: Presence::Mask { field: "n".into(), mask: 0x01 },
                    field: Field::new(
                        "a",
                        Kind::Msg { ty: "A".into(), boxed: false, with: Vec::new() },
                    ),
                },
            ],
        ),
    ];
    emit_module(&module(items)).expect("one indirection breaks the size cycle");
}

#[test]
fn a_sequence_is_its_own_indirection() {
    let items = vec![item(
        "Node",
        vec![
            Segment::Block(vec![Field::new("len", Kind::Scalar(Scalar::U(8)))]),
            sequence("children", "Node", "len"),
        ],
    )];
    let src = emit_module(&module(items)).expect("a self-referencing sequence emits");
    assert!(src.contains("pub children: Vec<Node>"), "{src}");
    assert!(src.contains("<Node as ::layline::Message>::decode_with_nested"), "{src}");
    // The length field bounds the sequence, so the message is not open-ended.
    assert!(src.contains("const OPEN_ENDED: bool = false;"), "{src}");
}

/// Constants that refer to each other in a cycle do not compile, so the emitter writes literals.
#[test]
fn a_cycle_in_the_open_endedness_constants_is_resolved_rather_than_refused() {
    let src = emit_module(&module(vec![
        item(
            "A",
            vec![
                Segment::Block(vec![Field::new("n", Kind::Scalar(Scalar::U(8)))]),
                Segment::Opt {
                    when: Presence::Mask { field: "n".into(), mask: 0x01 },
                    field: Field::new(
                        "b",
                        Kind::Msg { ty: "B".into(), boxed: true, with: Vec::new() },
                    ),
                },
            ],
        ),
        item(
            "B",
            vec![
                Segment::Block(vec![Field::new("n", Kind::Scalar(Scalar::U(8)))]),
                Segment::Opt {
                    when: Presence::Mask { field: "n".into(), mask: 0x01 },
                    field: Field::new(
                        "a",
                        Kind::Msg { ty: "A".into(), boxed: true, with: Vec::new() },
                    ),
                },
            ],
        ),
    ]))
    .expect("the cycle resolves");

    assert_eq!(
        src.matches("const OPEN_ENDED: bool = false;").count(),
        2,
        "both are the literal `false`:\n{src}"
    );
    assert!(!src.contains("<A as ::layline::Message>::OPEN_ENDED"), "{src}");
    assert!(!src.contains("<B as ::layline::Message>::OPEN_ENDED"), "{src}");
}

/// The `Raw` arm is open-ended by itself, so `true` must survive the cycle.
#[test]
fn a_true_that_did_not_come_from_the_cycle_survives_it() {
    let src = emit_module(&module(vec![
        item(
            "Node",
            vec![
                Segment::Block(vec![Field::new("kind", Kind::Scalar(Scalar::U(8)))]),
                Segment::Switch {
                    stated: None,
                    name: "body".into(),
                    on: Discriminant::Field("kind".into()),
                    choice: "Body".into(),
                    window: None,
                    doc: None,
                },
            ],
        ),
        Item::Choice(ChoiceDef::new(
            "Body",
            vec![
                Arm::new(
                    0,
                    "Raw",
                    vec![Segment::repeat(
                        "tail",
                        Kind::Scalar(Scalar::U(8)),
                        Count::Fill { cap: None },
                        None,
                        Collection::Vec,
                    )],
                ),
                Arm::new(1, "Branch", vec![holds("inner", "Node", true)]),
            ],
        )),
    ]))
    .expect("emits");
    assert_eq!(
        src.matches("const OPEN_ENDED: bool = true;").count(),
        2,
        "the open-ended `Raw` arm makes both `true`:\n{src}"
    );
}

#[test]
fn a_direct_self_reference_settles_to_false() {
    let items = vec![item(
        "Link",
        vec![
            Segment::Block(vec![Field::new("flags", Kind::Scalar(Scalar::U(8)))]),
            Segment::Opt {
                when: Presence::Mask { field: "flags".into(), mask: 0x80 },
                field: Field::new(
                    "next",
                    Kind::Msg { ty: "Link".into(), boxed: true, with: Vec::new() },
                ),
            },
        ],
    )];
    let src = emit_module(&module(items)).expect("an optional self-reference emits");
    assert!(
        src.contains("const OPEN_ENDED: bool = false;"),
        "expected the literal `false`, not `<Link as Message>::OPEN_ENDED`:\n{src}"
    );
    assert!(!src.contains("<Link as ::layline::Message>::OPEN_ENDED"), "{src}");
    assert!(src.contains("pub next: Option<Box<Link>>"), "{src}");
}

#[test]
fn a_field_may_not_be_both_a_byte_length_and_a_count() {
    let why = refusal(vec![
        item("Leaf", vec![Segment::Block(vec![Field::new("v", Kind::Scalar(Scalar::U(8)))])]),
        item(
            "Both",
            vec![
                Segment::Block(vec![Field::new("n", Kind::Scalar(Scalar::U(8)))]),
                sequence("kids", "Leaf", "n"),
                Segment::repeat(
                    "pad",
                    Kind::Scalar(Scalar::U(8)),
                    Count::Field { by: By::field("n"), cap: None },
                    None,
                    Collection::Vec,
                ),
            ],
        ),
    ]);
    assert!(
        why.contains("`n` is the byte length of a collection and also the length of a collection"),
        "{why}"
    );
}

/// Generated code refers to other items only by path, so a cycle cannot repeat the output.
#[test]
fn a_cycle_of_three_lowers_to_three_items_and_stops() {
    let items = vec![
        item(
            "A",
            vec![
                Segment::Block(vec![Field::new("len", Kind::Scalar(Scalar::U(8)))]),
                sequence("bs", "B", "len"),
            ],
        ),
        item(
            "B",
            vec![
                Segment::Block(vec![Field::new("len", Kind::Scalar(Scalar::U(8)))]),
                sequence("cs", "C", "len"),
            ],
        ),
        item(
            "C",
            vec![
                Segment::Block(vec![Field::new("len", Kind::Scalar(Scalar::U(8)))]),
                sequence("as_", "A", "len"),
            ],
        ),
    ];
    let src = emit_module(&module(items)).expect("emits");
    for name in ["pub struct A", "pub struct B", "pub struct C"] {
        assert_eq!(src.matches(name).count(), 1, "exactly one {name}:\n{src}");
    }
    assert!(src.contains("<B as ::layline::Message>::decode_with_nested"), "{src}");
    assert!(src.contains("<C as ::layline::Message>::decode_with_nested"), "{src}");
    assert!(src.contains("<A as ::layline::Message>::decode_with_nested"), "{src}");
}
