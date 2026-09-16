//! Magic values and checksums, checked through the model.

#[path = "support/mod.rs"]
mod support;

use layline_codegen::{
    Container, CoverTo, Coverage, Endian, Field, Invalid, Kind, LayoutDef, MessageDef, Scalar,
    Segment,
};
use support::{bytes, u, validate_layout, validate_message};

fn refusal(why: Invalid) -> String {
    match &why {
        Invalid::Other(_) | Invalid::Field { .. } => why.to_string(),
        other => panic!("expected an error with a message, got {other:?}"),
    }
}

#[test]
fn a_magic_is_as_wide_as_the_field_it_pins() {
    validate_layout(&bytes(
        4,
        vec![Field::new("sig", Kind::array(Scalar::U(8), 4)).with_magic(b"IVFC")],
    ))
    .expect("a four-byte magic");
    validate_layout(&bytes(1, vec![Field::new("attr", u(8)).with_magic(&[0x0F])]))
        .expect("a one-byte magic");
    validate_layout(&bytes(
        8,
        vec![Field::new("name", Kind::array(Scalar::U(8), 8)).with_magic(b"LAYOUT  ")],
    ))
    .expect("an eight-byte magic");
}

#[test]
fn a_constant_that_does_not_fill_its_field_is_refused() {
    let why = validate_layout(&bytes(4, vec![Field::new("sig", u(32)).with_magic(b"AB")]))
        .expect_err("two bytes are not four");
    let text = refusal(why);
    assert!(text.contains("`sig`") && text.contains("2 byte"), "{text}");
}

#[test]
fn a_constant_on_a_carrier_that_decides_its_own_bytes_is_refused() {
    let why = validate_layout(&bytes(
        4,
        vec![
            Field::new("mode", Kind::Codec { ty: "Mode".into(), bits: 32 })
                .with_magic(&[0, 0, 0, 1]),
        ],
    ))
    .expect_err("a codec decides its own bytes");
    assert!(refusal(why).contains("`FieldCodec` value"), "names the codec type");
}

#[test]
fn an_assertion_in_a_bit_addressed_container_is_refused() {
    let l = LayoutDef::new(
        "W",
        Container::word(16, Endian::Le, layline_codegen::BitOrder::Msb),
        vec![Field::new("sig", u(8)).with_magic(&[0xAA]), Field::new("rest", u(8))],
    );
    assert!(refusal(validate_layout(&l).expect_err("magic needs byte mode")).contains("byte mode"));
}

#[test]
fn a_layout_checksum_goes_through_the_one_coverage_rule() {
    let lrc = |over: Coverage| Kind::checksum(Scalar::U(8), "Lrc8", over);

    let why = validate_layout(&bytes(
        2,
        vec![
            Field::new("body", u(8)),
            Field::new("ck", lrc(Coverage::whole().to(CoverTo::Before("ck".into())))),
        ],
    ))
    .expect_err("`over = ..ck` is `over = ..`");
    assert!(refusal(why).contains("already ends it"));

    let why = validate_layout(&bytes(
        2,
        vec![
            Field::new("body", u(8)),
            Field::new("ck", lrc(Coverage::whole().to(CoverTo::After("ck".into())))),
        ],
    ))
    .expect_err("a checksum cannot cover its own value");
    assert!(refusal(why).contains("runs through"));

    let why = validate_layout(&bytes(
        2,
        vec![Field::new("body", u(8)), Field::new("ck", lrc(Coverage::from_field("nowhere")))],
    ))
    .expect_err("`nowhere` is not a field");
    assert!(matches!(&why, Invalid::UnknownReference(name) if name == "nowhere"), "{why:?}");

    validate_layout(&bytes(
        4,
        vec![
            Field::new("body", Kind::array(Scalar::U(8), 2)),
            Field::new("logo_crc", lrc(Coverage::from_field("body"))),
            Field::new("header_crc", lrc(Coverage::whole())),
        ],
    ))
    .expect("a checksum may cover an earlier checksum");
}

#[test]
fn a_range_covering_a_later_checksum_is_refused() {
    let lrc = |over: Coverage| Kind::checksum(Scalar::U(8), "Lrc8", over);
    let why = validate_layout(&bytes(
        3,
        vec![
            Field::new("body", u(8)),
            Field::new("first", lrc(Coverage::whole().to(CoverTo::After("second".into())))),
            Field::new("second", lrc(Coverage::from_field("body"))),
        ],
    ))
    .expect_err("`second` is computed after `first`");
    let text = refusal(why);
    assert!(text.contains("`second`") && text.contains("declared after it"), "{text}");
}

fn block_of(f: Field) -> MessageDef {
    MessageDef::new("M", vec![Segment::Block(vec![f])])
}

#[test]
fn a_blank_algorithm_is_refused_in_both_constructs() {
    let blank = validate_layout(&bytes(
        4,
        vec![
            Field::new("body", u(16)),
            Field::new("x", Kind::checksum(Scalar::U(16), "  ", Coverage::whole())),
        ],
    ))
    .expect_err("blank algorithm in a layout");
    assert!(refusal(blank).contains("requires the algorithm"), "names the missing algorithm");

    let blank = validate_message(&MessageDef::new(
        "M",
        vec![
            Segment::Block(vec![Field::new("body", u(8))]),
            Segment::Checksum {
                stated: None,
                name: "x".into(),
                repr: Scalar::U(16),
                algorithm: "  ".into(),
                over: Coverage::whole(),
                doc: None,
            },
        ],
    ))
    .expect_err("blank algorithm in a message");
    assert!(refusal(blank).contains("requires the algorithm"), "same message as for a layout");
}

#[test]
fn a_check_value_in_a_block_is_refused_by_name() {
    let why = validate_message(&block_of(Field::new(
        "x",
        Kind::checksum(Scalar::U(16), "Crc16Ccitt", Coverage::whole()),
    )))
    .expect_err("a message's checksum is a segment");
    assert!(refusal(why).contains("Segment::Checksum"), "names the segment to use");
}
