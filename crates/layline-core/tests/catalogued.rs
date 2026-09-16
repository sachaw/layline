//! A frame whose length comes from a catalogue, as a `FrameSpec`: `0xAA`, a selection byte, one
//! record per set bit, then an XOR check.

#![cfg(feature = "frame")]

use layline_core::frame::{CorruptPolicy, Decoder, DesyncCause, FrameSpec, Measure, Parsed, parse};

/// Bytes each selection bit adds. `None` marks a bit this build does not know.
const CATALOGUE: [Option<usize>; 8] =
    [Some(4), Some(8), Some(2), None, Some(12), Some(1), Some(1), Some(1)];

struct Catalogued;

impl FrameSpec for Catalogued {
    const CANDIDATE_LEN: usize = 1;
    const MAX_FRAME: usize = 64;
    const ON_CORRUPT: CorruptPolicy = CorruptPolicy::Resync;
    const SCAN_PAST_INCOMPLETE: bool = true;

    fn candidate(bytes: &[u8]) -> bool {
        bytes[0] == 0xAA
    }

    fn measure(bytes: &[u8]) -> Measure {
        let Some(selection) = bytes.get(1) else {
            return Measure::More(1);
        };
        if *selection == 0 {
            return Measure::Reject;
        }
        let mut total = 2;
        for (bit, width) in CATALOGUE.iter().enumerate() {
            if selection & (1 << bit) == 0 {
                continue;
            }
            match width {
                Some(width) => total += width,
                None => return Measure::Unmeasurable,
            }
        }
        Measure::Frame(total + 1)
    }

    fn verify(frame: &[u8]) -> bool {
        let stated = frame[frame.len() - 1];
        stated == frame[1..frame.len() - 1].iter().fold(0u8, |sum, b| sum ^ b)
    }

    fn body(frame: &[u8]) -> core::ops::Range<usize> {
        2..frame.len() - 1
    }
}

fn frame(selection: u8) -> Vec<u8> {
    let payload: usize =
        (0..8).filter(|bit| selection & (1 << bit) != 0).filter_map(|bit| CATALOGUE[bit]).sum();
    let mut out = vec![0xAA, selection];
    out.extend(std::iter::repeat_n(0x5A, payload));
    let check = out[1..].iter().fold(0u8, |sum, b| sum ^ b);
    out.push(check);
    out
}

#[test]
fn a_frame_measured_by_its_catalogue_is_framed() {
    let wire = frame(0b0001_0001);
    match parse::<Catalogued>(&wire) {
        Parsed::Frame { consumed, body, .. } => {
            assert_eq!(consumed, wire.len());
            assert_eq!(body.len(), 16);
        }
        other => panic!("expected a frame, got {other:?}"),
    }
    for selection in [0b0000_0001u8, 0b0000_0010, 0b1111_0111] {
        let wire = frame(selection);
        assert!(
            matches!(parse::<Catalogued>(&wire), Parsed::Frame { consumed, .. } if consumed == wire.len()),
            "selection {selection:08b}"
        );
    }
}

#[test]
fn an_element_the_catalogue_lacks_is_unmeasurable_and_not_corruption() {
    let mut wire = vec![0xAA, 0b0000_1000, 0, 0, 0, 0];
    wire.push(0);
    match parse::<Catalogued>(&wire) {
        Parsed::Desync { discard, cause } => {
            assert_eq!(discard, 1, "resyncs by one byte, like any rejection");
            assert_eq!(cause, DesyncCause::Unmeasurable);
        }
        other => panic!("expected an unmeasurable frame, got {other:?}"),
    }
}

#[test]
fn the_decoder_counts_it_apart_from_a_checksum_failure() {
    let mut wire = vec![0xAA, 0b0000_1000, 0, 0, 0, 0, 0];
    wire.extend_from_slice(&frame(0b0000_0001));

    let mut decoder = Decoder::<Catalogued, 128>::new();
    decoder.push(&wire);
    let popped = decoder.pop().expect("the frame behind the unreadable one");
    assert_eq!(popped.body.len(), 4);

    let stats = decoder.stats();
    assert_eq!(stats.unmeasurable, 1);
    assert_eq!(stats.checksum_errors, 0, "nothing here was corrupt");
    assert!(stats.bytes_discarded > 0);
}

#[test]
fn a_corrupt_frame_still_counts_as_corrupt() {
    let mut wire = frame(0b0000_0001);
    let last = wire.len() - 1;
    wire[last] ^= 0xFF;
    wire.extend_from_slice(&frame(0b0000_0010));

    let mut decoder = Decoder::<Catalogued, 128>::new();
    decoder.push(&wire);
    let popped = decoder.pop().expect("the good frame behind the bad one");
    assert_eq!(popped.body.len(), 8);

    let stats = decoder.stats();
    assert_eq!(stats.checksum_errors, 1);
    assert_eq!(stats.unmeasurable, 0);
}

#[test]
fn a_measured_but_incomplete_frame_waits_for_the_rest() {
    let wire = frame(0b0001_0000);
    assert!(matches!(
        parse::<Catalogued>(&wire[..4]),
        Parsed::Incomplete { need } if need == wire.len() - 4
    ));
}
