//! D in the model: a tail anchored to the end, a switch that passes context to its arms, and a
//! depth bound. What the model accepts, what it refuses and why, and what the emitter writes.

#[path = "support/mod.rs"]
mod support;

#[cfg(feature = "emit")]
use layline_codegen::emit::{Module, generate};
use layline_codegen::{
    Arm, BitOrder, ChoiceDef, Collection, Count, Expr, Field, Invalid, Kind, Len, Op, Param,
    Position, Presence, Scalar, Segment,
};
#[cfg(feature = "emit")]
use layline_codegen::{Discriminant, Item};
use support::{msg, u, validate_choice, validate_message};

#[cfg(feature = "emit")]
fn emit_module(module: &Module) -> Result<String, layline_codegen::Error> {
    generate(module).map(|g| g.source)
}

fn refused_field(why: Result<(), Invalid>, field: &str, reason: &str) {
    match why {
        Err(Invalid::Field { at, why }) if at == field && why.contains(reason) => {}
        other => panic!("expected `{field}` refused with \"{reason}\", got {other:?}"),
    }
}

fn refused(why: Result<(), Invalid>, reason: &str) {
    match why {
        Err(why) if why.to_string().contains(reason) => {}
        other => panic!("expected a refusal with \"{reason}\", got {other:?}"),
    }
}

fn fill(name: &str) -> Segment {
    Segment::repeat(name, u(8), Count::Fill { cap: None }, None, Collection::Vec)
}

/// RTP's padding: a count of padding bytes, itself included, in the last byte.
fn padding() -> Vec<Segment> {
    let padded = || Presence::When(Expr::name("padded"));
    vec![
        Segment::repeat(
            "padding",
            u(8),
            Count::Field {
                by: Expr::binary(Op::Sub, Expr::name("pad_count"), Expr::int(1)),
                cap: None,
            },
            None,
            Collection::Vec,
        )
        .with_presence(padded()),
        Segment::opt(padded(), Field::new("pad_count", u(8)).with_stated()),
    ]
}

fn rtp() -> layline_codegen::MessageDef {
    msg(
        "Rtp",
        vec![
            Segment::Block(vec![Field::new("padded", Kind::Scalar(Scalar::ByteBool))]),
            fill("payload"),
        ],
    )
    .with_tail(padding())
}

#[test]
fn a_fill_may_stop_at_a_tail_that_reads_its_size_from_the_end() {
    validate_message(&rtp()).expect("RTP padding");
}

#[test]
#[cfg(feature = "emit")]
fn an_emitted_tail_counts_from_the_end() {
    let src = emit_module(&Module::new(vec![Item::Message(rtp())])).expect("emits");
    assert!(src.contains("pub pad_count: ::layline::Stated<u8>"), "{src}");
    assert!(src.contains("pub padding: Option<Vec<u8>>"), "{src}");
    assert!(
        src.contains("::layline::table::Start::Tail {"),
        "the tail's rows count from the end:\n{src}"
    );
    assert!(src.contains("const OPEN_ENDED: bool = true;"), "a tail reads to the end:\n{src}");
}

#[test]
fn a_tail_field_must_know_its_size_before_decode_reads_it() {
    let read_size = msg("M", vec![fill("body")])
        .with_tail(vec![Segment::value("rest", Kind::Var { ty: "Leb".into() })]);
    refused_field(validate_message(&read_size), "rest", "reads its own size from the wire");

    let prefixed = msg("M", vec![fill("body")]).with_tail(vec![Segment::repeat(
        "rest",
        u(8),
        Count::Prefix { repr: Scalar::U(8), cap: None },
        None,
        Collection::Vec,
    )]);
    refused_field(validate_message(&prefixed), "rest", "counted or sized");

    let placed = msg("M", vec![fill("body")]).with_tail(vec![Segment::Block(vec![
        Field::new("crc", u(16)).with_position(Position::byte(4)),
    ])]);
    refused_field(validate_message(&placed), "crc", "counts from the start");

    let bits = msg("M", vec![Segment::Block(vec![Field::new("a", u(8))])])
        .with_bits(BitOrder::Lsb)
        .with_tail(vec![Segment::Block(vec![Field::new("b", u(8))])]);
    refused(validate_message(&bits), "a bit-addressed message has no tail");
}

#[test]
fn optional_framing_is_an_integer_present_when_a_condition_holds() {
    let flagged =
        msg("M", vec![Segment::opt(Presence::Prefix, Field::new("n", u(8)).with_stated())]);
    refused_field(validate_message(&flagged), "n", "present `when` a condition");
    let flagged_tail = msg("M", vec![fill("body")])
        .with_tail(vec![Segment::opt(Presence::Prefix, Field::new("n", u(8)).with_stated())]);
    refused_field(validate_message(&flagged_tail), "n", "reads its own size from the wire");
}

fn arms() -> ChoiceDef {
    ChoiceDef::new(
        "Body",
        vec![Arm::new(
            1,
            "Ext",
            vec![Segment::repeat(
                "data",
                u(8),
                Count::Window(Len::Field {
                    by: Expr::binary(Op::Mul, Expr::name("words"), Expr::int(4)),
                    cap: None,
                    align: None,
                }),
                None,
                Collection::Vec,
            )],
        )],
    )
    .with_needs(vec![Param::new("words", Scalar::U(16))])
}

#[test]
fn an_arm_reads_the_parameters_its_choice_declares() {
    validate_choice(&arms()).expect("Kaitai's rfextgeo_ext(len)");
    let unknown = ChoiceDef::new("Body", arms().arms);
    refused(validate_choice(&unknown), "words");

    let clash = arms().with_needs(vec![Param::new("data", Scalar::U(8))]);
    refused(validate_choice(&clash), "both a parameter and a field");
    let order = arms().with_endian_from("words");
    refused(validate_choice(&order), "Declare it `words: bool`");
}

#[test]
#[cfg(feature = "emit")]
fn a_switch_passes_earlier_fields_to_its_choice() {
    let holder = |with: &str| {
        msg(
            "Holder",
            vec![
                Segment::Block(vec![Field::new("kind", u(8)), Field::new("words", u(16))]),
                Segment::switch("body", Discriminant::Field(Expr::name("kind")), "Body", None)
                    .with_context(vec![with.into()]),
            ],
        )
    };
    validate_message(&holder("words")).expect("an earlier field");
    refused(validate_message(&holder("later")), "later");
    let src = emit_module(&Module::new(vec![Item::Message(holder("words")), Item::Choice(arms())]))
        .expect("emits");
    assert!(src.contains("pub struct BodyCtx"), "{src}");
    assert!(src.contains("type Ctx = BodyCtx;"), "{src}");
    assert!(src.contains("<Body as ::layline::Choice>::Ctx::new"), "{src}");
}

#[test]
#[cfg(feature = "emit")]
fn a_depth_bound_is_the_type_s_own() {
    let m = msg("Deep", vec![Segment::Block(vec![Field::new("a", u(8))])]).with_depth(16);
    let src = emit_module(&Module::new(vec![Item::Message(m)])).expect("emits");
    assert!(src.contains("const MAX_DEPTH: u32 = 16;"), "{src}");
    assert!(src.contains("__depth > <Self as ::layline::Message>::MAX_DEPTH"), "{src}");
}
