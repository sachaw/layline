//! `#[count(n * n)]`: a run whose element count is the square of a field, with every byte computed by hand.

#![cfg(feature = "derive")]

use layline::table::{By, Span};
use layline::{Message, ParseError, table::fixed_bits};

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Covariance {
    order: u16,
    #[count(order * order)]
    cells: Vec<u16>,
    trailer: u16,
}

const WIRE: [u8; 22] = [
    0x00, 0x03, //
    0x00, 0x01, 0x00, 0x02, 0x00, 0x03, //
    0x00, 0x04, 0x00, 0x05, 0x00, 0x06, //
    0x00, 0x07, 0x00, 0x08, 0x00, 0x09, //
    0xBE, 0xEF, //
];

#[test]
fn the_run_holds_the_square_and_the_field_behind_it_lands() {
    let (m, used) = Covariance::decode(&WIRE).expect("parses");
    assert_eq!(used, WIRE.len());
    assert_eq!(m.order, 3);
    assert_eq!(m.cells, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(m.trailer, 0xBEEF, "decode reads `trailer` after nine elements");
}

#[test]
fn encode_recomputes_the_order_from_the_collection() {
    let m = Covariance { order: 99, cells: (1..=9).collect(), trailer: 0xBEEF };
    assert_eq!(m.encode(), WIRE);
}

#[test]
#[should_panic(expected = "no whole number squares to its length")]
fn encode_refuses_a_collection_that_is_not_square() {
    let _ = Covariance { order: 0, cells: vec![1, 2, 3, 4, 5, 6, 7], trailer: 0 }.encode();
}

#[test]
fn a_body_short_of_the_square_is_refused() {
    assert!(matches!(Covariance::decode(&WIRE[..12]), Err(ParseError::Short { .. })));
}

#[test]
fn the_table_says_squared_rather_than_counted() {
    let row = Covariance::SEGMENTS.iter().find(|s| s.name == "cells").expect("a row for it");
    assert_eq!(row.span, Span::Squared { by: By::field("order"), each: Some(16) });
    assert_eq!(fixed_bits(Covariance::SEGMENTS), 16, "the computed offsets stop at the run");
}

#[test]
fn the_artifact_publishes_the_square() {
    let text = format!("{}", <Covariance as layline::Message>::audit());
    assert!(text.contains("squared,order,1,0,16"), "no squared row in:\n{text}");
}
