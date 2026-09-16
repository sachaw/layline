//! `#[count(n)] #[stride(s)]`: a run whose element stride is a field, the ELF `e_shentsize` shape.
//! Every byte is computed by hand.

#![cfg(feature = "derive")]

use layline::table::{By, Span, Start};
use layline::{Layout, Message, ParseError, table::fixed_bits};

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 2)]
struct Pair {
    a: u8,
    b: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Epoch {
    n: u8,
    sb_length: u8,
    #[count(n)]
    #[stride(sb_length)]
    #[bytes(2)]
    sub_blocks: Vec<Pair>,
    trailer: u16,
}

const NARROW: [u8; 8] = [2, 2, 1, 2, 3, 4, 0xEF, 0xBE];

const WIDE: [u8; 12] = [2, 4, 1, 2, 0xFF, 0xFF, 3, 4, 0xFF, 0xFF, 0xEF, 0xBE];

fn pairs() -> Vec<Pair> {
    vec![Pair { a: 1, b: 2 }, Pair { a: 3, b: 4 }]
}

#[test]
fn a_stride_equal_to_the_element_is_the_walk_it_already_had() {
    let (e, used) = Epoch::decode(&NARROW).expect("parses");
    assert_eq!(e.n, 2);
    assert_eq!(e.sb_length, 2);
    assert_eq!(e.sub_blocks, pairs());
    assert_eq!(e.trailer, 0xBEEF);
    assert_eq!(used, 8, "2 header + 2 * 2 + 2 trailer, by hand");
}

#[test]
fn the_padding_is_stepped_over_and_not_read() {
    let (e, used) = Epoch::decode(&WIDE).expect("parses");
    assert_eq!(e.sb_length, 4, "the stride field on the wire");
    assert_eq!(e.sub_blocks, pairs(), "decode reads the pairs and skips the padding");
    assert_eq!(e.trailer, 0xBEEF, "the field after the run starts after the last stride");
    assert_eq!(used, 12, "2 header + 2 * 4 + 2 trailer, by hand");
}

#[test]
fn a_count_of_zero_steps_nowhere() {
    let wire = [0u8, 4, 0xEF, 0xBE];
    let (e, used) = Epoch::decode(&wire).expect("parses");
    assert!(e.sub_blocks.is_empty());
    assert_eq!(e.trailer, 0xBEEF);
    assert_eq!(used, 4);
}

#[test]
fn a_stride_shorter_than_the_element_is_refused_by_name() {
    let wire = [2u8, 1, 1, 2, 3, 4, 0xEF, 0xBE];
    assert_eq!(Epoch::decode(&wire), Err(ParseError::Malformed { field: "sb_length", at: 2 }));

    let zero = [2u8, 0, 1, 2, 3, 4, 0xEF, 0xBE];
    assert_eq!(Epoch::decode(&zero), Err(ParseError::Malformed { field: "sb_length", at: 2 }));
}

#[test]
fn a_step_whose_padding_the_body_ran_out_of_is_a_short_body() {
    let truncated = &WIDE[..9];
    assert_eq!(
        Epoch::decode(truncated),
        Err(ParseError::Short { need_bytes: 10, got_bytes: 9, at: 6 }),
        "the second step ends at byte 10, and the body has nine"
    );
}

#[test]
fn the_count_and_the_stride_are_both_back_patched() {
    let e = Epoch { n: 99, sb_length: 77, sub_blocks: pairs(), trailer: 0xBEEF };
    assert_eq!(e.encode(), NARROW.to_vec(), "hand-computed");
    assert_eq!(<Pair as Layout>::WIRE_BYTES, 2, "byte 1 above");
}

#[test]
fn a_wider_wire_re_encodes_at_the_width_this_build_knows() {
    let (e, _) = Epoch::decode(&WIDE).expect("parses");
    assert_eq!(e.sb_length, 4, "as it arrived");
    assert_eq!(e.encode(), NARROW.to_vec(), "encode writes at this build's element width");

    let (again, used) = Epoch::decode(&e.encode()).expect("parses");
    assert_eq!(again, Epoch { n: 2, sb_length: 2, sub_blocks: pairs(), trailer: 0xBEEF });
    assert_eq!(used, 8);
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 2, endian = be)]
struct Word {
    v: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct EpochBe {
    n: u8,
    sb_length: u8,
    #[count(n)]
    #[stride(sb_length)]
    #[bytes(2)]
    sub_blocks: Vec<Word>,
    trailer: u16,
}

#[test]
fn a_big_endian_strided_run_round_trips() {
    let wire = [2u8, 3, 0x01, 0x02, 0xFF, 0x03, 0x04, 0xFF, 0xBE, 0xEF];
    let (e, used) = EpochBe::decode(&wire).expect("parses");
    assert_eq!(e.sub_blocks, [Word { v: 0x0102 }, Word { v: 0x0304 }]);
    assert_eq!(e.trailer, 0xBEEF);
    assert_eq!(used, 10, "2 header + 2 * 3 + 2 trailer, by hand");

    assert_eq!(e.encode(), vec![2, 2, 0x01, 0x02, 0x03, 0x04, 0xBE, 0xEF]);
    let (again, used) = EpochBe::decode(&e.encode()).expect("parses");
    assert_eq!(again.sub_blocks, e.sub_blocks);
    assert_eq!(again.trailer, 0xBEEF);
    assert_eq!(used, 8);
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 260)]
struct Wide {
    tag: u32,
    body: [u8; 256],
}

#[derive(Debug, Clone, PartialEq, Message)]
struct WideRun {
    n: u8,
    sb_length: u8,
    #[count(n)]
    #[stride(sb_length)]
    #[bytes(260)]
    items: Vec<Wide>,
}

#[test]
#[should_panic(expected = "field `sb_length` is too narrow for the wire width of one element")]
fn a_stride_field_too_narrow_for_the_element_is_caught_by_derived_fits() {
    let _ = WideRun { n: 0, sb_length: 0, items: vec![Wide { tag: 7, body: [0; 256] }] }.encode();
}

#[test]
fn the_row_names_the_count_and_the_stride() {
    let table = <Epoch as Message>::SEGMENTS;
    assert_eq!(table.len(), 3, "block, run, block");

    assert_eq!(table[0].start, Start::At(0));
    assert_eq!(table[0].span, Span::Fixed(16), "two `u8`s, by hand");

    let run = table[1];
    assert_eq!(run.name, "sub_blocks");
    assert_eq!(run.start, Start::At(16));
    assert_eq!(run.span, Span::Strided { by: By::field("n"), stride: By::field("sb_length") });
    assert_eq!(run.decoded_by, Some("Pair"));
    assert!(!run.is_fixed(), "a run with a stride field has no computed length");

    assert_eq!(table[2].start, Start::After { segment: "sub_blocks", bits: 0 });
    assert_eq!(table[2].span, Span::Fixed(16));
    assert_eq!(fixed_bits(table), 16, "the computed offsets stop at the run");
}

#[test]
fn the_audit_record_carries_both_fields() {
    let text = <Epoch as layline::Message>::audit().to_string();
    assert!(
        text.contains("G Epoch sub_blocks at,16 strided,n,1,0,sb_length,1,0 - Pair\n"),
        "one line, fixed arity, both fields:\n{text}"
    );
}
