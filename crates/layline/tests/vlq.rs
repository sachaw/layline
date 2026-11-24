//! `num::Vlq`: an MSB-first variable-length quantity with a byte cap, as a `#[var]` field.

#![cfg(feature = "derive")]

use layline::{Message, ParseError};
use layline_core::num::Vlq;

#[path = "support/rng.rs"]
mod rng;

/// A JEL-style record: an id of at most four bytes, then a count of the readings that follow.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Emitter {
    kind: u8,
    #[var]
    id: Vlq<4>,
    #[var]
    n: Vlq<2>,
    #[count(n)]
    readings: Vec<u16>,
}

fn emitter(id: u64) -> Emitter {
    Emitter { kind: 7, id: Vlq(id), n: Vlq(0), readings: vec![0x0102, 0x0304] }
}

#[test]
fn each_boundary_id_takes_the_fewest_bytes_in_a_message() {
    let cases: [(u64, &[u8]); 7] = [
        (0, &[0x00]),
        (127, &[0x7F]),
        (128, &[0x81, 0x00]),
        (16_383, &[0xFF, 0x7F]),
        (16_384, &[0x81, 0x80, 0x00]),
        ((1 << 21) - 1, &[0xFF, 0xFF, 0x7F]),
        ((1 << 28) - 1, &[0xFF, 0xFF, 0xFF, 0x7F]),
    ];
    for (id, bytes) in cases {
        let wire = emitter(id).encode();
        let mut want = vec![7];
        want.extend_from_slice(bytes);
        want.extend_from_slice(&[0x02, 0x01, 0x02, 0x03, 0x04]);
        assert_eq!(wire, want, "id {id}");

        let (back, used) = Emitter::decode(&wire).expect("decodes");
        assert_eq!(used, wire.len());
        assert_eq!(back, Emitter { n: Vlq(2), ..emitter(id) }, "encode fills in the count");
    }
}

#[test]
fn an_id_past_the_cap_is_malformed() {
    let wire = [7, 0x81, 0x80, 0x80, 0x80, 0x00, 0x00];
    assert_eq!(Emitter::decode(&wire), Err(ParseError::Malformed { field: "id", at: 1 }));
}

#[test]
fn an_id_cut_short_is_short() {
    assert!(matches!(Emitter::decode(&[7, 0x81, 0x80]), Err(ParseError::Short { .. })));
}

#[test]
fn a_count_read_from_a_vlq_bounds_the_run() {
    // Two bytes of count, `0x81 0x00`, is 128 readings, and only one follows.
    assert!(matches!(
        Emitter::decode(&[7, 0x05, 0x81, 0x00, 0xAA, 0xBB]),
        Err(ParseError::Short { .. })
    ));
}

#[test]
#[should_panic(expected = "Vlq<4>: 268435456 needs more than 4 bytes")]
fn encode_refuses_an_id_past_the_cap() {
    let _ = emitter(1 << 28).encode();
}

/// A run of ids, as a `Vec<Uleb128>` is one.
#[derive(Debug, Clone, PartialEq, Message)]
struct Roster {
    n: u8,
    #[var]
    #[count(n)]
    ids: Vec<Vlq<4>>,
}

#[test]
fn a_run_of_vlqs_reads_each_to_its_own_length() {
    let m = Roster { n: 0, ids: vec![Vlq(5), Vlq(300), Vlq((1 << 28) - 1)] };
    let wire = m.encode();
    assert_eq!(wire, [3, 0x05, 0x82, 0x2C, 0xFF, 0xFF, 0xFF, 0x7F]);
    assert_eq!(Roster::decode(&wire), Ok((Roster { n: 3, ..m }, wire.len())));
}

/// Random bytes either fail to decode or round-trip to the same value.
#[test]
fn a_random_wire_either_errors_or_round_trips() {
    let mut r = rng::Rng(0x5EED_0F1D);
    let mut decoded = 0;
    for i in 0..4096 {
        let len = (r.next() as usize) % 16;
        let mut wire = vec![0u8; len];
        r.fill(&mut wire);
        let Ok((value, used)) = Emitter::decode(&wire) else { continue };
        decoded += 1;
        assert!(used <= wire.len(), "wire {i}");
        let out = value.encode();
        assert!(out.len() <= used, "wire {i}: the fewest bytes are no more than the wire's");
        assert_eq!(Emitter::decode(&out), Ok((value, out.len())), "wire {i}");
    }
    assert!(decoded > 0, "no random wire decoded, so nothing was proven");
}
