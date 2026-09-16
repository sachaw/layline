//! `Box<[T]>` as a run type: the same bytes, count and segment table as `Vec<T>`.

#![cfg(feature = "derive")]

use layline::Message;

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Boxed {
    n: u8,
    #[count(n)]
    items: Box<[u16]>,
    trailer: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Vectored {
    n: u8,
    #[count(n)]
    items: Vec<u16>,
    trailer: u16,
}

const WIRE: [u8; 9] = [0x03, 0x00, 0x01, 0x00, 0x02, 0x00, 0x03, 0xBE, 0xEF];

#[test]
fn a_boxed_run_decodes_and_re_encodes() {
    let (m, used) = Boxed::decode(&WIRE).expect("parses");
    assert_eq!(used, WIRE.len());
    assert_eq!(&*m.items, &[1u16, 2, 3]);
    assert_eq!(m.trailer, 0xBEEF, "decode reads `trailer` after the three counted elements");
    assert_eq!(m.encode(), WIRE);
}

#[test]
fn the_two_carriers_are_the_same_wire_and_the_same_table() {
    let boxed = Boxed::decode(&WIRE).expect("parses").0;
    let vectored = Vectored::decode(&WIRE).expect("parses").0;
    assert_eq!(boxed.encode(), vectored.encode());

    // Compare spans only, since generated block names include the type name.
    let spans = |s: &[layline::table::SegmentDef<'static>]| -> Vec<layline::table::Span<'static>> {
        s.iter().map(|r| r.span).collect()
    };
    assert_eq!(spans(Boxed::SEGMENTS), spans(Vectored::SEGMENTS));
    assert_eq!(
        layline::table::fixed_bits(Boxed::SEGMENTS),
        layline::table::fixed_bits(Vectored::SEGMENTS)
    );

    let run = |s: &[layline::table::SegmentDef<'static>]| {
        *s.iter().find(|r| r.name == "items").expect("a row for the run")
    };
    assert_eq!(run(Boxed::SEGMENTS).span, run(Vectored::SEGMENTS).span);
    assert_eq!(run(Boxed::SEGMENTS).decoded_by, run(Vectored::SEGMENTS).decoded_by);
}
