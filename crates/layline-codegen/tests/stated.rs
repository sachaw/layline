//! Expected positions are worked out by hand from the field widths and bit order.

#[path = "support/mod.rs"]
mod support;

use layline_codegen::{
    BitOrder, By, Collection, Container, Count, Endian, Field, Invalid, Kind, LayoutDef, Len,
    Presence, Scalar, Segment, Stated,
};
use support::{msg, u, validate_layout, validate_message};

fn layout(container: &Container, fields: Vec<Field>) -> LayoutDef {
    LayoutDef::new("L", container.clone(), fields)
}

fn refusal(why: Invalid) -> String {
    match why {
        Invalid::Stated(text) => text,
        other => panic!("expected `Invalid::Stated`, got {other:?}"),
    }
}

/// `a` takes bits 0..2, so `b` starts at bit 2.
#[test]
fn a_bit_position_in_a_word_container_is_the_running_sum_under_lsb() {
    let container = Container::word(16, Endian::Le, BitOrder::Lsb);
    let fields = vec![
        Field::new("a", u(2)).with_stated(Stated::bit(0)),
        Field::new("b", u(14)).with_stated(Stated::bit(2)),
    ];
    validate_layout(&layout(&container, fields)).expect("both fields are where they say they are");

    let wrong = vec![Field::new("a", u(2)), Field::new("b", u(14)).with_stated(Stated::bit(3))];
    let why = validate_layout(&layout(&container, wrong)).expect_err("bit 3 is not bit 2");
    assert_eq!(
        refusal(why),
        "field `b`: #[at(bit = 3)] but the solver placed it at bit 2",
        "same message as the derive",
    );
}

/// Declared, `a` is at bit 0 and `b` at bit 2. Physically, `a` is at bit 14 and `b` at bit 0.
#[test]
fn a_msb_container_states_the_declared_position() {
    let container = Container::word(16, Endian::Be, BitOrder::Msb);
    let fields = vec![
        Field::new("a", u(2)).with_stated(Stated::bit(0)),
        Field::new("b", u(14)).with_stated(Stated::bit(2)),
    ];
    validate_layout(&layout(&container, fields)).expect("the declared positions");

    let physical = vec![Field::new("a", u(2)).with_stated(Stated::bit(14)), Field::new("b", u(14))];
    let why = validate_layout(&layout(&container, physical))
        .expect_err("bit 14 is physical, not declared");
    assert_eq!(
        refusal(why),
        "field `a`: #[at(bit = 14)] (declared numbering, msb) is physical bit 0, but the \
         solver placed it at physical bit 14 — declared bit 0",
    );

    let beyond = vec![Field::new("a", u(2)).with_stated(Stated::bit(15)), Field::new("b", u(14))];
    let why =
        validate_layout(&layout(&container, beyond)).expect_err("bits 15..17 are not in the word");
    assert_eq!(
        refusal(why),
        "field `a`: #[at(bit = 15)] (declared numbering, msb) does not fit the 16-bit \
         container, but the solver placed it at physical bit 14 — declared bit 0",
    );
}

#[test]
fn a_stated_position_is_published_where_it_landed() {
    assert_eq!(Stated::bit(0).published(14), Stated::bit(14));
    assert_eq!(Stated::byte(1).published(16), Stated::byte(2));
    assert_eq!(Stated::byte(0).published(12), Stated::bit(12));
}

/// Declared, `a`, `b` and `c` are at bytes 0, 1 and 2. Physically, they are at bytes 3, 2 and 0.
#[test]
fn a_byte_unit_states_a_position_in_a_bit_addressed_container() {
    let container = Container::word(32, Endian::Be, BitOrder::Msb);
    let fields = vec![
        Field::new("a", u(8)).with_stated(Stated::byte(0)),
        Field::new("b", u(8)).with_stated(Stated::byte(1)),
        Field::new("c", u(16)).with_stated(Stated::byte(2)),
    ];
    validate_layout(&layout(&container, fields)).expect("byte positions in a word container");

    let wrong = vec![
        Field::new("a", u(8)).with_stated(Stated::byte(0)),
        Field::new("b", u(8)).with_stated(Stated::byte(2)),
        Field::new("c", u(16)).with_stated(Stated::byte(2)),
    ];
    let why = validate_layout(&layout(&container, wrong)).expect_err("byte 2 is not byte 1");
    assert_eq!(
        refusal(why),
        "field `b`: #[at(byte = 2)] (declared numbering, msb) is physical byte 1, but the \
         solver placed it at physical byte 2 — declared byte 1",
        "a byte mismatch reports bytes in both numberings",
    );
}

/// `a` takes bits 0..3, so `b` starts at bit 3.
#[test]
fn a_byte_claim_about_a_field_between_bytes_reports_in_bits() {
    let container = Container::word(16, Endian::Le, BitOrder::Lsb);
    let fields = vec![Field::new("a", u(3)), Field::new("b", u(13)).with_stated(Stated::byte(0))];
    let why =
        validate_layout(&layout(&container, fields)).expect_err("bit 3 is not a byte boundary");
    assert_eq!(
        refusal(why),
        "field `b`: #[at(byte = 0)] but the solver placed it at bit 3 — not a byte boundary",
    );
}

#[test]
fn byte_mode_states_byte_positions() {
    let container = Container::Bytes { bytes: 12, endian: Endian::Le };
    let fields = vec![
        Field::new("id", u(8)).with_stated(Stated::byte(0)),
        Field::new("t", u(32)).with_stated(Stated::byte(1)),
        Field::new("pad", Kind::bytes(3)).with_stated(Stated::byte(5)),
        Field::new("sample", Kind::Scalar(Scalar::F32)).with_stated(Stated::byte(8)),
    ];
    validate_layout(&layout(&container, fields)).expect("positions match");

    let wrong = vec![
        Field::new("id", u(8)).with_stated(Stated::byte(0)),
        Field::new("t", u(32)).with_stated(Stated::byte(4)),
        Field::new("pad", Kind::bytes(3)).with_stated(Stated::byte(5)),
        Field::new("sample", Kind::Scalar(Scalar::F32)).with_stated(Stated::byte(8)),
    ];
    let why = validate_layout(&layout(&container, wrong)).expect_err("byte 4 is not byte 1");
    assert_eq!(
        refusal(why),
        "field `t`: #[at(byte = 4)] but the solver placed it at byte 1",
        "byte mode has no bit order, so no msb note",
    );
}

#[test]
fn byte_mode_accepts_a_bit_unit_too() {
    let container = Container::Bytes { bytes: 5, endian: Endian::Le };
    let fields = vec![
        Field::new("id", u(8)).with_stated(Stated::bit(0)),
        Field::new("t", u(32)).with_stated(Stated::bit(8)),
    ];
    validate_layout(&layout(&container, fields)).expect("bits count in byte mode too");
}

/// `f0` and `f1` fill word 0, so `f2` starts at byte 2.
#[test]
fn words_mode_states_positions_across_the_word_grid() {
    let container = Container::Words { words: 2, endian: Endian::Be, order: BitOrder::Lsb };
    let fields = vec![
        Field::new("f0", u(4)).with_stated(Stated::bit(0)),
        Field::new("f1", u(12)).with_stated(Stated::bit(4)),
        Field::new("f2", u(16)).with_stated(Stated::byte(2)),
    ];
    validate_layout(&layout(&container, fields)).expect("positions match");

    let wrong = vec![
        Field::new("f0", u(4)),
        Field::new("f1", u(12)),
        Field::new("f2", u(16)).with_stated(Stated::byte(0)),
    ];
    let why = validate_layout(&layout(&container, wrong)).expect_err("byte 0 is word 0");
    assert_eq!(refusal(why), "field `f2`: #[at(byte = 0)] but the solver placed it at byte 2");
}

/// `f0` fills word 0, so it stays at byte 0. The `f1` array spans two whole words and stays at byte 2.
#[test]
fn a_run_of_whole_words_is_not_mirrored() {
    let container = Container::Words { words: 3, endian: Endian::Be, order: BitOrder::Msb };
    let fields = vec![
        Field::new("f0", u(16)).with_stated(Stated::byte(0)),
        Field::new("f1", Kind::array(Scalar::U(16), 2)).with_stated(Stated::byte(2)),
    ];
    validate_layout(&layout(&container, fields)).expect("a whole-word array keeps its position");
}

/// Declared, `hi` is at byte 0 and `lo` at byte 1. Physically, `hi` is at byte 1 and `lo` at byte 0.
/// `raw` is at byte 2 either way.
#[test]
fn msb_mirrors_within_each_word_and_not_across_them() {
    let container = Container::Words { words: 2, endian: Endian::Be, order: BitOrder::Msb };
    let fields = vec![
        Field::new("hi", u(8)).with_stated(Stated::byte(0)),
        Field::new("lo", u(8)).with_stated(Stated::byte(1)),
        Field::new("raw", Kind::bytes(2)).with_stated(Stated::byte(2)),
    ];
    validate_layout(&layout(&container, fields)).expect("positions match");

    let wrong = vec![
        Field::new("hi", u(8)).with_stated(Stated::byte(1)),
        Field::new("lo", u(8)),
        Field::new("raw", Kind::bytes(2)),
    ];
    let why = validate_layout(&layout(&container, wrong)).expect_err("msb flips within one word");
    assert_eq!(
        refusal(why),
        "field `hi`: #[at(byte = 1)] (declared numbering, msb) is physical byte 0, but the \
         solver placed it at physical byte 1 — declared byte 0",
        "msb flips within one 16-bit word",
    );

    let across = vec![
        Field::new("hi", u(8)).with_stated(Stated::bit(12)),
        Field::new("lo", u(8)),
        Field::new("raw", Kind::bytes(2)),
    ];
    let why = validate_layout(&layout(&container, across)).expect_err("bits 12..20 cross a word");
    assert_eq!(
        refusal(why),
        "field `hi`: #[at(bit = 12)] (declared numbering, msb) does not fit the 16-bit word, \
         but the solver placed it at physical bit 8 — declared bit 0",
    );
}

/// Positions continue across blocks, so `kind` is at byte 2.
#[test]
fn a_stated_position_in_a_message_block_counts_from_the_body() {
    let segments = vec![
        Segment::Block(vec![
            Field::new("sync", u(8)).with_stated(Stated::byte(0)),
            Field::new("len", u(8)).with_stated(Stated::byte(1)),
        ]),
        Segment::Block(vec![Field::new("kind", u(16)).with_stated(Stated::byte(2))]),
    ];
    validate_message(&msg("M", segments)).expect("positions continue across blocks");

    let restarted = vec![
        Segment::Block(vec![Field::new("sync", u(8)), Field::new("len", u(8))]),
        Segment::Block(vec![Field::new("kind", u(16)).with_stated(Stated::byte(0))]),
    ];
    let why = validate_message(&msg("M", restarted)).expect_err("byte 0 is the sync byte");
    assert_eq!(refusal(why), "field `kind`: #[at(byte = 0)] but the solver placed it at byte 2");
}

/// `a` (1 byte), the CRC (2 bytes) and the payload (4 bytes) put `b` at byte 7.
#[test]
fn a_checksum_and_a_byte_bounded_run_advance_the_stated_numbering() {
    let segments = vec![
        Segment::Block(vec![Field::new("a", u(8)).with_stated(Stated::byte(0))]),
        Segment::Checksum {
            stated: None,
            name: "crc".into(),
            repr: Scalar::U(16),
            algorithm: "Crc16".into(),
            over: layline_codegen::Coverage::whole(),
            doc: None,
        },
        Segment::repeat("payload", u(8), Count::Window(Len::Bytes(4)), None, Collection::Vec),
        Segment::Block(vec![Field::new("b", u(8)).with_stated(Stated::byte(7))]),
    ];
    validate_message(&msg("M", segments)).expect("everything before `b` has a fixed size");
}

/// `a` (1 byte) and `name` (8 bytes) put `b` at byte 9.
#[test]
fn a_fixed_run_of_text_advances_the_stated_numbering() {
    let segments = |pos: u64| {
        vec![
            Segment::Block(vec![Field::new("a", u(8))]),
            Segment::Value {
                stated: None,
                name: "name".into(),
                kind: Kind::Text { len: Len::Bytes(8), codec: Some("Ascii".into()) },
                doc: None,
            },
            Segment::Block(vec![Field::new("b", u(8)).with_stated(Stated::byte(pos))]),
        ]
    };
    validate_message(&msg("M", segments(9))).expect("1 + 8 bytes puts `b` at byte 9");
    let why = validate_message(&msg("M", segments(1))).expect_err("byte 1 is the first name byte");
    assert_eq!(refusal(why), "field `b`: #[at(byte = 1)] but the solver placed it at byte 9");

    #[cfg(feature = "walk")]
    {
        let table = layline_codegen::__derive::message_parts(
            &msg("M", segments(9)),
            &layline_codegen::Root::default(),
        )
        .expect("renders")
        .codec
        .to_string()
        .replace(' ', "");
        assert!(table.contains("Start::At(72)"), "byte 9 is bit 72: {table}");
        assert!(!table.contains("Start::After"), "a fixed-size run gives a fixed start: {table}");
    }
}

/// The validator and the generated table both take positions from `Segment::fixed_bits`.
#[test]
#[cfg(feature = "walk")]
fn the_published_table_starts_where_a_stated_position_says() {
    let segments = || {
        vec![
            Segment::Block(vec![Field::new("a", u(8))]),
            Segment::Checksum {
                stated: None,
                name: "crc".into(),
                repr: Scalar::U(16),
                algorithm: "Crc16".into(),
                over: layline_codegen::Coverage::whole(),
                doc: None,
            },
            Segment::repeat("payload", u(8), Count::Window(Len::Bytes(4)), None, Collection::Vec),
            Segment::Block(vec![Field::new("b", u(8)).with_stated(Stated::byte(7))]),
        ]
    };
    validate_message(&msg("M", segments())).expect("`b` is at byte 7");

    let mut wrong = segments();
    wrong.pop();
    wrong.push(Segment::Block(vec![Field::new("b", u(8)).with_stated(Stated::byte(8))]));
    let why = validate_message(&msg("M", wrong)).expect_err("byte 8 is not byte 7");
    assert_eq!(refusal(why), "field `b`: #[at(byte = 8)] but the solver placed it at byte 7");

    let table = layline_codegen::__derive::message_parts(
        &msg("M", segments()),
        &layline_codegen::Root::default(),
    )
    .expect("renders")
    .codec
    .to_string()
    .replace(' ', "");
    assert!(table.contains("Start::At(56)"), "byte 7 is bit 56: {table}");
    assert!(!table.contains("Start::After"), "every size here is fixed: {table}");
}

#[test]
fn an_optional_field_may_state_the_position_it_has_when_it_is_there() {
    let present = vec![
        Segment::Block(vec![Field::new("flags", u(8)).with_stated(Stated::byte(0))]),
        Segment::Opt {
            when: Presence::Mask { field: "flags".into(), mask: 0x01 },
            field: Field::new("extra", u(16)).with_stated(Stated::byte(1)),
        },
    ];
    validate_message(&msg("M", present)).expect("`extra` is at byte 1 when present");

    let wrong = vec![
        Segment::Block(vec![Field::new("flags", u(8))]),
        Segment::Opt {
            when: Presence::Mask { field: "flags".into(), mask: 0x01 },
            field: Field::new("extra", u(16)).with_stated(Stated::byte(2)),
        },
    ];
    let why = validate_message(&msg("M", wrong)).expect_err("byte 2 is not byte 1");
    assert_eq!(refusal(why), "field `extra`: #[at(byte = 2)] but the solver placed it at byte 1");
}

#[test]
fn a_position_cannot_be_stated_behind_an_extent_the_wire_decides() {
    let segments = vec![
        Segment::Block(vec![Field::new("n", u(8))]),
        Segment::Value {
            name: "tag".into(),
            kind: Kind::Var { ty: "Uleb128".into() },
            doc: None,
            stated: None,
        },
        Segment::Block(vec![Field::new("x", u(8)).with_stated(Stated::byte(2))]),
    ];
    let why = validate_message(&msg("M", segments)).expect_err("a varint has no fixed width");
    assert!(
        !matches!(why, Invalid::Stated(_)),
        "an unknown offset is not a position mismatch: {why:?}",
    );
    let text = why.to_string();
    assert!(
        text.contains("field `x`") && text.contains("variable-length segment"),
        "it names the field and the reason: {text}",
    );
}

#[test]
fn a_counted_run_ends_the_stated_numbering() {
    let segments = vec![
        Segment::Block(vec![Field::new("n", u(8))]),
        Segment::repeat(
            "entries",
            u(8),
            Count::Field { by: By::field("n"), cap: None },
            None,
            Collection::Vec,
        ),
        Segment::Block(vec![Field::new("x", u(8)).with_stated(Stated::byte(2))]),
    ];
    let why = validate_message(&msg("M", segments)).expect_err("the offset of `x` depends on `n`");
    assert!(why.to_string().contains("only known at run time"), "the reason: {why:?}",);
}

#[cfg(feature = "emit")]
mod emitted {
    use super::{layout, u};
    use layline_codegen::Item;
    use layline_codegen::emit::{Module, generate};
    use layline_codegen::{BitOrder, Container, Endian, Field, Kind, Scalar, Stated};

    fn module(items: Vec<Item>) -> Module {
        Module::new(items)
    }

    fn emit_module(module: &Module) -> Result<String, layline_codegen::Error> {
        generate(module).map(|g| g.source)
    }

    #[test]
    fn an_emitted_declaration_carries_the_stated_position() {
        let container = Container::Bytes { bytes: 6, endian: Endian::Le };
        let fields = vec![
            Field::new("id", u(16)).with_stated(Stated::byte(0)),
            Field::new("sample", Kind::Scalar(Scalar::F32)).with_stated(Stated::byte(2)),
        ];
        let out = emit_module(&module(vec![Item::Layout(layout(&container, fields))]))
            .expect("a well-formed layout emits");
        assert!(out.contains("#[at(byte = 0)]"), "{out}");
        assert!(out.contains("#[at(byte = 2)]"), "{out}");
    }

    /// The attribute keeps the declared msb numbering, which is what the derive expects.
    #[test]
    fn an_emitted_bit_position_is_written_in_bits() {
        let container = Container::word(16, Endian::Be, BitOrder::Msb);
        let fields = vec![
            Field::new("a", u(4)).with_stated(Stated::bit(0)),
            Field::new("b", u(12)).with_stated(Stated::bit(4)),
        ];
        let out = emit_module(&module(vec![Item::Layout(layout(&container, fields))]))
            .expect("a well-formed layout emits");
        assert!(out.contains("#[at(bit = 0)]"), "{out}");
        assert!(out.contains("#[at(bit = 4)]"), "{out}");
    }

    #[test]
    fn a_wrong_stated_position_is_refused_before_anything_is_written() {
        let container = Container::Bytes { bytes: 6, endian: Endian::Le };
        let fields = vec![
            Field::new("id", u(16)).with_stated(Stated::byte(0)),
            Field::new("sample", Kind::Scalar(Scalar::F32)).with_stated(Stated::byte(4)),
        ];
        let why = emit_module(&module(vec![Item::Layout(layout(&container, fields))]))
            .expect_err("byte 4 is not byte 2");
        let text = format!("{why}");
        assert!(text.contains("byte = 4"), "{text}");
        assert!(text.contains("placed it at byte 2"), "{text}");
    }
}
