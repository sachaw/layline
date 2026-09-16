//! `#[checksum(Algorithm, over = <range>)]`: encode recomputes the value and decode checks it.
//! Every expected byte here is computed by hand from the algorithm's definition.

#![cfg(feature = "derive")]

#[path = "support/checksum.rs"]
mod checksum;

use checksum::RotateAddSum;
use layline::checksum::{Crc, Sum16Neg};
/// Two's-complement LRC-8: sum the bytes, then negate.
struct Lrc8;

impl Checksum for Lrc8 {
    type Output = u8;
    type State = u8;

    fn init() -> u8 {
        0
    }

    fn update(state: u8, bytes: &[u8]) -> u8 {
        bytes.iter().fold(state, |acc, &b| acc.wrapping_add(b))
    }

    fn finish(state: u8) -> u8 {
        state.wrapping_neg()
    }
}

/// The algorithms these tests use.
type Crc16Ccitt = Crc<u16, 0x1021, 0xFFFF, 0x0000, false>;
type Sum16NegBe = Sum16Neg<true>;
use layline::{Checksum, Message, ParseError};

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Whole {
    magic: u16,
    len: u16,
    #[count(len)]
    payload: Vec<u8>,
    #[checksum(Crc16Ccitt, over = ..)]
    crc: u16,
}

#[test]
fn a_checksum_over_the_whole_message_is_recomputed_on_encode() {
    let m = Whole { magic: 0xABCD, len: 999, payload: vec![1, 2, 3], crc: 0 };
    let bytes = m.encode();

    let covered = [0xAB, 0xCD, 0x00, 0x03, 1, 2, 3];
    let want = <Crc16Ccitt as Checksum>::compute(&covered);
    let mut expected = covered.to_vec();
    expected.extend_from_slice(&want.to_be_bytes());
    assert_eq!(bytes, expected, "the wire contains the recomputed CRC");
    assert_ne!(want, 0, "the recomputed CRC differs from the value in the struct");

    let (back, used) = Whole::decode(&bytes).expect("round-trips");
    assert_eq!(used, bytes.len());
    assert_eq!(back.crc, want);
    assert_eq!((back.magic, back.len, back.payload), (0xABCD, 3, vec![1, 2, 3]));
}

#[test]
fn a_corrupted_body_is_a_checksum_error_carrying_both_values() {
    let m = Whole { magic: 0xABCD, len: 0, payload: vec![1, 2, 3], crc: 0 };
    let mut bytes = m.encode();
    let good = u16::from_be_bytes([bytes[7], bytes[8]]);

    bytes[5] ^= 0xFF;
    let err = Whole::decode(&bytes).unwrap_err();

    let recomputed = <Crc16Ccitt as Checksum>::compute(&bytes[..7]);
    assert_eq!(
        err,
        ParseError::Checksum {
            field: "crc",
            expected: i64::from(recomputed),
            actual: i64::from(good),
        },
    );
    assert_ne!(recomputed, good);
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Skipped {
    magic: u16,
    len: u16,
    #[count(len)]
    payload: Vec<u8>,
    #[checksum(Crc16Ccitt, over = len..)]
    crc: u16,
}

#[test]
fn over_a_named_field_covers_a_different_range_than_over_everything() {
    let payload = vec![1, 2, 3];
    let whole = Whole { magic: 0xABCD, len: 0, payload: payload.clone(), crc: 0 }.encode();
    let skipped = Skipped { magic: 0xABCD, len: 0, payload, crc: 0 }.encode();

    assert_eq!(whole.len(), skipped.len());
    assert_eq!(whole[..7], skipped[..7]);

    let all_seven = [0xAB, 0xCD, 0x00, 0x03, 1, 2, 3];
    let from_len = [0x00, 0x03, 1, 2, 3];
    assert_eq!(&whole[7..], &<Crc16Ccitt as Checksum>::compute(&all_seven).to_be_bytes());
    assert_eq!(&skipped[7..], &<Crc16Ccitt as Checksum>::compute(&from_len).to_be_bytes());
    assert_ne!(whole[7..], skipped[7..], "the two ranges differ");

    let moved = Skipped { magic: 0x0000, len: 0, payload: vec![1, 2, 3], crc: 0 }.encode();
    assert_eq!(moved[7..], skipped[7..]);
    assert!(Skipped::decode(&moved).is_ok());

    let moved = Whole { magic: 0x0000, len: 0, payload: vec![1, 2, 3], crc: 0 }.encode();
    assert_ne!(moved[7..], whole[7..]);
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Discovered {
    kind: u8,
    #[text]
    #[until(0)]
    label: String,
    #[var]
    len: layline_core::num::Uleb128,
    #[count(len)]
    body: Vec<u8>,
    #[checksum(Lrc8, over = label..)]
    lrc: u8,
}

#[test]
fn a_checksum_covers_discovered_extents_it_could_not_have_predicted() {
    for (label, body) in [
        ("", vec![]),
        ("a", vec![7u8]),
        ("hello", vec![1, 2, 3, 4, 5]),
        ("a longer label than the last one", (0..200u32).map(|n| n as u8).collect()),
    ] {
        let m = Discovered {
            kind: 9,
            label: String::from(label),
            len: layline_core::num::Uleb128(0),
            body: body.clone(),
            lrc: 0xAA,
        };
        let bytes = m.encode();

        let mut covered = Vec::new();
        covered.extend_from_slice(label.as_bytes());
        covered.push(0);
        let n = body.len();
        assert!(n < 0x4000, "two ULEB128 bytes at most, for this hand encoding");
        if n < 0x80 {
            covered.push(n as u8);
        } else {
            covered.push((n as u8 & 0x7F) | 0x80);
            covered.push((n >> 7) as u8);
        }
        covered.extend_from_slice(&body);

        assert_eq!(bytes[0], 9);
        assert_eq!(&bytes[1..bytes.len() - 1], covered.as_slice());
        assert_eq!(*bytes.last().expect("non-empty"), <Lrc8 as Checksum>::compute(&covered));

        let sum = covered.iter().fold(bytes[bytes.len() - 1], |acc, &b| acc.wrapping_add(b));
        assert_eq!(sum, 0, "the covered bytes plus the LRC sum to zero");

        let (back, used) = Discovered::decode(&bytes).expect("round-trips");
        assert_eq!(used, bytes.len());
        assert_eq!((back.kind, back.label.as_str(), back.body), (9, label, body.clone()));
        assert_eq!(back.lrc, <Lrc8 as Checksum>::compute(&covered));

        if !body.is_empty() {
            let mut bad = bytes.clone();
            let last_body = bad.len() - 2;
            bad[last_body] ^= 0xFF;
            assert!(
                matches!(Discovered::decode(&bad), Err(ParseError::Checksum { field: "lrc", .. }),),
                "a corrupted body must not parse clean: {bad:?}",
            );
        }
    }
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct FromACollection {
    magic: u16,
    len: u16,
    #[count(len)]
    payload: Vec<u8>,
    trailer: u16,
    #[checksum(Crc16Ccitt, over = payload..)]
    crc: u16,
}

#[test]
fn a_range_may_start_at_a_collection_which_is_not_a_number() {
    let bytes =
        FromACollection { magic: 0xABCD, len: 0, payload: vec![1, 2, 3], trailer: 0x5566, crc: 0 }
            .encode();

    let covered = [1u8, 2, 3, 0x55, 0x66];
    assert_eq!(
        bytes,
        [0xAB, 0xCD, 0x00, 0x03, 1, 2, 3, 0x55, 0x66]
            .iter()
            .copied()
            .chain(<Crc16Ccitt as Checksum>::compute(&covered).to_be_bytes())
            .collect::<Vec<u8>>()
    );

    let (back, used) = FromACollection::decode(&bytes).expect("round-trips");
    assert_eq!(used, bytes.len());
    assert_eq!(back.crc, <Crc16Ccitt as Checksum>::compute(&covered));

    let mut outside = bytes.clone();
    outside[0] ^= 0xFF;
    assert!(FromACollection::decode(&outside).is_ok());
    for inside in 4..9 {
        let mut bad = bytes.clone();
        bad[inside] ^= 0xFF;
        assert!(
            matches!(FromACollection::decode(&bad), Err(ParseError::Checksum { .. })),
            "byte {inside} is inside the covered range",
        );
    }
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct TwoChecks {
    sync: u16,
    count: u8,
    #[checksum(Lrc8, over = sync..)]
    header_lrc: u8,
    #[count(count)]
    words: Vec<u16>,
    #[checksum(Sum16NegBe, over = header_lrc..)]
    body_sum: u16,
}

#[test]
fn two_checksums_cover_two_ranges_and_neither_is_read_from_the_struct() {
    let m = TwoChecks {
        sync: 0x8155,
        count: 0,
        header_lrc: 0x11,
        words: vec![0x0102, 0x0304],
        body_sum: 0x2222,
    };
    let bytes = m.encode();

    let header = [0x81, 0x55, 0x02];
    let want_lrc = <Lrc8 as Checksum>::compute(&header);
    let covered = [0x81u8, 0x55, 0x02, want_lrc, 0x01, 0x02, 0x03, 0x04];
    let want_sum = <Sum16NegBe as Checksum>::compute(&covered[3..]);

    let mut expected = covered.to_vec();
    expected.extend_from_slice(&want_sum.to_be_bytes());
    assert_eq!(bytes, expected);

    assert_eq!(&bytes[3..8], &covered[3..]);

    let (back, used) = TwoChecks::decode(&bytes).expect("round-trips");
    assert_eq!(used, bytes.len());
    assert_eq!(back.header_lrc, want_lrc);
    assert_eq!(back.body_sum, want_sum);
    assert_eq!(back.words, vec![0x0102, 0x0304]);

    let mut bad = bytes.clone();
    bad[1] ^= 0xFF;
    assert!(matches!(
        TwoChecks::decode(&bad),
        Err(ParseError::Checksum { field: "header_lrc", .. }),
    ));

    let mut bad = bytes.clone();
    bad[5] ^= 0xFF;
    assert!(
        matches!(TwoChecks::decode(&bad), Err(ParseError::Checksum { field: "body_sum", .. }),)
    );
}

/// A separate implementation, written from the format's definition.
fn rotate_add_reference(record: &[u8]) -> u16 {
    let mut sum: u16 = 0;
    for (i, &b) in record.iter().enumerate() {
        if i == 2 || i == 3 {
            continue;
        }
        sum = (sum >> 1) | ((sum & 1) << 15);
        sum = sum.wrapping_add(u16::from(b));
    }
    sum
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Excised {
    kind: u8,
    count: u8,
    #[checksum(RotateAddSum, over = ..=tail)]
    sum: u16,
    flags: u16,
    #[count(count)]
    tail: Vec<u8>,
}

#[test]
fn the_rotate_add_sum_covers_the_whole_set_except_its_own_bytes() {
    let m = Excised {
        kind: 0x85,
        count: 99,
        sum: 0xFFFF,
        flags: 0x0020,
        tail: vec![0xC0, 0x01, 0x02, 0x03],
    };
    let bytes = m.encode();

    let mut record = vec![0x85, 4, 0, 0, 0x20, 0x00, 0xC0, 0x01, 0x02, 0x03];
    let want = rotate_add_reference(&record);
    record[2] = want as u8;
    record[3] = (want >> 8) as u8;
    assert_eq!(bytes, record, "the wire contains the recomputed check value");

    // A hand-computed literal.
    assert_eq!(want, 0x5A1E);
    assert_eq!(&bytes[2..4], &[0x1E, 0x5A], "little-endian, like every other field");

    let mut other = record.clone();
    other[2] = 0xAA;
    other[3] = 0x55;
    assert_eq!(rotate_add_reference(&other), want, "bytes 2..4 are outside the sum");

    let folded = <RotateAddSum as Checksum>::finish(<RotateAddSum as Checksum>::update(
        <RotateAddSum as Checksum>::update(<RotateAddSum as Checksum>::init(), &record[..2]),
        &record[4..],
    ));
    assert_eq!(folded, want);

    let (back, used) = Excised::decode(&bytes).expect("round-trips");
    assert_eq!(used, bytes.len());
    assert_eq!(back.sum, want);
    assert_eq!(back.count, 4);
    assert_eq!(back, Excised { count: 4, sum: want, ..m });
    assert_eq!(back.encode(), bytes, "encode returns the same bytes");
}

#[test]
fn a_corrupted_excised_record_is_a_checksum_error_wherever_the_damage_is() {
    let m =
        Excised { kind: 0x85, count: 0, sum: 0, flags: 0x0020, tail: vec![0xC0, 0x01, 0x02, 0x03] };
    let bytes = m.encode();
    assert!(Excised::decode(&bytes).is_ok());

    for i in 0..bytes.len() {
        let mut bad = bytes.clone();
        bad[i] ^= 0xFF;
        match (i, Excised::decode(&bad)) {
            (1, Err(ParseError::Short { .. })) => {}
            (2 | 3, Err(ParseError::Checksum { field: "sum", .. })) => {}
            (_, Err(ParseError::Checksum { field: "sum", .. })) => {}
            (i, other) => panic!("byte {i} flipped: decode returned {other:?}"),
        }
    }

    let mut bad = bytes.clone();
    bad[6] ^= 0xFF;
    let err = Excised::decode(&bad).unwrap_err();
    let mut record = bad.clone();
    let good = u16::from_le_bytes([record[2], record[3]]);
    record[2] = 0;
    record[3] = 0;
    assert_eq!(
        err,
        ParseError::Checksum {
            field: "sum",
            expected: i64::from(rotate_add_reference(&record)),
            actual: i64::from(good),
        },
    );
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Batch {
    magic: u8,
    n: u8,
    #[checksum(Crc16Ccitt, over = attributes..=records)]
    crc: u16,
    attributes: u16,
    #[count(n)]
    records: Vec<u8>,
}

#[test]
fn a_forward_range_that_starts_after_the_checksum_excises_nothing() {
    let m = Batch { magic: 2, n: 0, crc: 0xDEAD, attributes: 0x0102, records: vec![7, 8, 9] };
    let bytes = m.encode();

    let covered = [0x01u8, 0x02, 7, 8, 9];
    let want = <Crc16Ccitt as Checksum>::compute(&covered);
    assert_eq!(bytes, [2, 3, (want >> 8) as u8, want as u8, 0x01, 0x02, 7, 8, 9]);

    let (back, used) = Batch::decode(&bytes).expect("round-trips");
    assert_eq!((used, back.crc, back.n), (bytes.len(), want, 3));

    let moved = Batch { magic: 9, ..m.clone() }.encode();
    assert_eq!(moved[2..4], bytes[2..4]);
    for inside in 4..bytes.len() {
        let mut bad = bytes.clone();
        bad[inside] ^= 0xFF;
        assert!(
            matches!(Batch::decode(&bad), Err(ParseError::Checksum { field: "crc", .. })),
            "byte {inside} is inside the covered range",
        );
    }
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct HeaderCheck {
    magic: u16,
    #[checksum(Lrc8, over = ..=len)]
    lrc: u8,
    len: u8,
    #[count(len)]
    payload: Vec<u8>,
}

#[test]
fn a_range_may_end_before_the_body_and_still_run_past_the_checksum() {
    let m = HeaderCheck { magic: 0x8155, lrc: 0x77, len: 0, payload: vec![1, 2, 3] };
    let bytes = m.encode();

    let covered = [0x81u8, 0x55, 3];
    let want = <Lrc8 as Checksum>::compute(&covered);
    assert_eq!(bytes, [0x81, 0x55, want, 3, 1, 2, 3]);
    assert_eq!(covered.iter().fold(want, |a, &b| a.wrapping_add(b)), 0);

    let (back, used) = HeaderCheck::decode(&bytes).expect("round-trips");
    assert_eq!((used, back.lrc, back.len), (7, want, 3));

    let mut outside = bytes.clone();
    outside[6] ^= 0xFF;
    assert!(HeaderCheck::decode(&outside).is_ok());
    let mut inside = bytes.clone();
    inside[0] ^= 0xFF;
    assert!(
        matches!(HeaderCheck::decode(&inside), Err(ParseError::Checksum { field: "lrc", .. }),)
    );
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct HeaderCheckExclusive {
    magic: u16,
    #[checksum(Lrc8, over = ..payload)]
    lrc: u8,
    len: u8,
    #[count(len)]
    payload: Vec<u8>,
}

#[test]
fn an_exclusive_end_past_the_field_covers_the_same_bytes_an_inclusive_one_does() {
    let inclusive = HeaderCheck { magic: 0x8155, lrc: 0, len: 0, payload: vec![1, 2, 3] }.encode();
    let exclusive =
        HeaderCheckExclusive { magic: 0x8155, lrc: 0, len: 0, payload: vec![1, 2, 3] }.encode();
    assert_eq!(inclusive, exclusive, "`..payload` and `..=len` bound the same range");
    assert_eq!(exclusive[2], <Lrc8 as Checksum>::compute(&[0x81, 0x55, 3]));
    assert!(HeaderCheckExclusive::decode(&exclusive).is_ok());

    let mut bad = exclusive.clone();
    bad[3] ^= 0xFF;
    assert!(matches!(
        HeaderCheckExclusive::decode(&bad),
        Err(ParseError::Checksum { field: "lrc", .. } | ParseError::Short { .. }),
    ));
}

#[test]
fn the_published_table_says_which_bytes_each_checksum_covered() {
    use layline::table::{CoverDef, CoverEdge};

    assert_eq!(
        <Excised as Message>::COVERED,
        &[CoverDef::new("sum", CoverEdge::Start, CoverEdge::After("tail"), true)],
    );
    assert_eq!(
        <Batch as Message>::COVERED,
        &[CoverDef::new(
            "crc",
            CoverEdge::Before("attributes"),
            CoverEdge::After("records"),
            false
        )],
    );
    assert_eq!(
        <Whole as Message>::COVERED,
        &[CoverDef::new("crc", CoverEdge::Start, CoverEdge::Before("crc"), false)],
    );
    assert_eq!(
        <HeaderCheck as Message>::COVERED,
        &[CoverDef::new("lrc", CoverEdge::Start, CoverEdge::After("len"), true)],
    );
    assert_eq!(<NoChecksum as Message>::COVERED, &[]);
}

#[derive(Debug, Clone, PartialEq, Message)]
struct NoChecksum {
    a: u16,
    b: u16,
}

#[test]
fn a_body_that_ends_before_the_checksum_is_short_not_wrong() {
    let bytes = Whole { magic: 1, len: 0, payload: vec![1, 2], crc: 0 }.encode();
    for cut in 0..bytes.len() {
        match Whole::decode(&bytes[..cut]) {
            Err(ParseError::Short { .. } | ParseError::Malformed { .. }) => {}
            other => panic!("a body cut to {cut} bytes: decode returned {other:?}"),
        }
    }
    assert!(Whole::decode(&bytes).is_ok());
}

#[derive(Debug, Clone, Copy, PartialEq, layline::Layout)]
#[layout(bytes = 2)]
struct Entry {
    value: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Directory {
    entry_at: u8,
    #[checksum(Lrc8, over = ..)]
    lrc: u8,
    #[seek(entry_at)]
    #[bytes(2)]
    entry: Entry,
}

const DIRECTORY: [u8; 4] = [0x02, 0xFE, 0xEF, 0xBE];

#[test]
fn a_check_value_waits_for_a_reservation_inside_its_range() {
    let d = Directory { entry_at: 0, lrc: 0, entry: Entry { value: 0xBEEF } };
    assert_eq!(d.encode(), DIRECTORY);
    assert_eq!(
        <Lrc8 as Checksum>::compute(&DIRECTORY[..1]),
        0xFE,
        "the covered byte is the offset field"
    );

    let (back, used) = Directory::decode(&DIRECTORY).expect("parses");
    assert_eq!((used, back.entry_at, back.entry), (DIRECTORY.len(), 2, Entry { value: 0xBEEF }));
}

#[test]
fn a_checksum_may_cover_one_that_does_not_cover_it_back() {
    #[derive(layline::Message, Debug, PartialEq)]
    struct Forward {
        body: u16,
        #[checksum(Crc16Ccitt, over = ..=b)]
        a: u16,
        #[checksum(Crc16Ccitt, over = body..=body)]
        b: u16,
    }

    let wire = Forward { body: 0x1234, a: 0, b: 0 }.encode();
    let (back, used) = Forward::decode(&wire).expect("decode reads what encode wrote");
    assert_eq!(used, wire.len());
    assert_ne!(back.a, 0, "encode wrote `a`");
    assert_ne!(back.b, 0, "encode wrote `b`");

    assert_ne!(back.a, back.b, "`a` covers `b`, and `b` covers the bytes before `a`");
}

#[test]
fn a_checksum_covers_a_byte_length_after_it_is_stamped() {
    #[derive(layline::Message, Debug, PartialEq)]
    #[message(endian = be)]
    struct EarlyCrc {
        magic: u16,
        len: u16,
        #[checksum(Crc16Ccitt, over = ..)]
        crc: u16,
        #[len(len)]
        payload: Vec<u8>,
    }

    let m = EarlyCrc { magic: 0xABCD, len: 0, crc: 0, payload: vec![1, 2, 3] };
    let wire = m.encode();

    assert_eq!(&wire[2..4], &[0x00, 0x03], "encode wrote `len`");
    let (back, used) = EarlyCrc::decode(&wire).expect("decode reads what encode wrote");
    assert_eq!((used, back.payload), (wire.len(), vec![1, 2, 3]));

    let mut bad = wire.clone();
    bad[3] ^= 0xFF;
    assert!(
        matches!(EarlyCrc::decode(&bad), Err(ParseError::Checksum { field: "crc", .. })),
        "the length is inside the covered range: {:?}",
        EarlyCrc::decode(&bad),
    );
}
