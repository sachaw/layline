//! A frame that ends at a delimiter, as a `FrameSpec`: NMEA 0183's `$` body `*` two hex digits
//! `\r\n`, with an 8-bit XOR over the body.

#![cfg(feature = "frame")]

use layline_core::frame::{CorruptPolicy, Decoder, DesyncCause, FrameSpec, Measure, Parsed, parse};

/// The longest sentence. Caps the search for the terminator.
const MAX: usize = 82;

struct Nmea;

impl FrameSpec for Nmea {
    const CANDIDATE_LEN: usize = 1;
    const MAX_FRAME: usize = MAX;
    const ON_CORRUPT: CorruptPolicy = CorruptPolicy::Resync;
    const SCAN_PAST_INCOMPLETE: bool = true;

    fn frame_len(_header: &[u8]) -> Option<usize> {
        unreachable!(
            "this format measures itself: a sentence ends at its terminator, not at a length"
        )
    }

    fn candidate(bytes: &[u8]) -> bool {
        bytes[0] == b'$'
    }

    fn measure(bytes: &[u8]) -> Measure {
        let Some(star) = bytes.iter().position(|byte| *byte == b'*') else {
            return if bytes.len() >= MAX { Measure::Reject } else { Measure::More(1) };
        };
        let total = star + 5;
        if total > MAX {
            return Measure::Reject;
        }
        if bytes.len() < total {
            return Measure::More(total - bytes.len());
        }
        if &bytes[total - 2..total] == b"\r\n" { Measure::Frame(total) } else { Measure::Reject }
    }

    fn verify(frame: &[u8]) -> bool {
        let star = frame.len() - 5;
        let stated = core::str::from_utf8(&frame[star + 1..star + 3])
            .ok()
            .and_then(|text| u8::from_str_radix(text, 16).ok());
        stated == Some(lrc8_xor(&frame[1..star]))
    }

    fn body(frame: &[u8]) -> core::ops::Range<usize> {
        1..frame.len() - 5
    }
}

fn lrc8_xor(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |sum, byte| sum ^ byte)
}

fn sentence(body: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 6);
    out.push(b'$');
    out.extend_from_slice(body.as_bytes());
    out.push(b'*');
    let sum = lrc8_xor(body.as_bytes());
    out.extend_from_slice(format!("{sum:02X}").as_bytes());
    out.extend_from_slice(b"\r\n");
    out
}

#[test]
fn a_delimited_frame_is_measurable_without_a_length_field() {
    let wire = sentence("GPGGA,123519,4807.038,N");
    match parse::<Nmea>(&wire) {
        Parsed::Frame { consumed, body, integrity } => {
            assert_eq!(consumed, wire.len());
            assert_eq!(&wire[body], b"GPGGA,123519,4807.038,N");
            assert_eq!(integrity, layline_core::frame::Integrity::Verified);
        }
        other => panic!("expected a frame, got {other:?}"),
    }
}

#[test]
fn a_sentence_arriving_a_byte_at_a_time_is_delivered_once() {
    let wire = sentence("GPRMC,081836,A");
    let mut decoder = Decoder::<Nmea, 128>::new();
    let mut delivered = 0;
    for byte in &wire {
        decoder.push(&[*byte]);
        while decoder.pop().is_some() {
            delivered += 1;
        }
    }
    assert_eq!(delivered, 1);
    assert_eq!(decoder.stats().frames, 1);
    assert_eq!(decoder.stats().bytes_discarded, 0);
}

#[test]
fn a_wrong_checksum_resyncs_and_the_next_sentence_still_arrives() {
    let mut wire = sentence("GPGGA,1");
    let last = wire.len() - 3;
    wire[last] = if wire[last] == b'0' { b'1' } else { b'0' };
    wire.extend_from_slice(&sentence("GPRMC,2"));

    let mut decoder = Decoder::<Nmea, 128>::new();
    decoder.push(&wire);
    let popped = decoder.pop().expect("the good sentence behind the bad one");
    assert_eq!(popped.body, b"GPRMC,2");
    assert_eq!(decoder.stats().checksum_errors, 1);
}

#[test]
fn an_unterminated_sentence_is_rejected_once_it_outgrows_the_bound() {
    let mut wire = vec![b'$'; MAX];
    match parse::<Nmea>(&wire) {
        Parsed::Desync { cause: DesyncCause::BadHeader, .. } => {}
        other => panic!("expected a rejection at the bound, got {other:?}"),
    }
    wire.pop();
    assert!(matches!(parse::<Nmea>(&wire), Parsed::Incomplete { .. }));
}

#[test]
fn a_delimiter_scan_does_not_stall_on_a_false_start() {
    let mut wire = vec![b'$', b'n', b'o', b'i', b's', b'e'];
    wire.extend_from_slice(&sentence("GPGGA,ok"));
    let mut decoder = Decoder::<Nmea, 128>::new();
    decoder.push(&wire);
    let popped = decoder.pop().expect("the real sentence");
    assert_eq!(popped.body, b"GPGGA,ok");
}

/// `Nmea` with its bound removed.
struct Unbounded;

impl FrameSpec for Unbounded {
    const CANDIDATE_LEN: usize = 1;
    const MAX_FRAME: usize = MAX;
    const ON_CORRUPT: CorruptPolicy = CorruptPolicy::Resync;

    fn candidate(bytes: &[u8]) -> bool {
        bytes[0] == b'$'
    }

    fn measure(bytes: &[u8]) -> Measure {
        match bytes.iter().position(|byte| *byte == b'*') {
            None => Measure::More(1),
            Some(star) => Measure::Frame(star + 5),
        }
    }

    fn verify(_frame: &[u8]) -> bool {
        true
    }

    fn body(frame: &[u8]) -> core::ops::Range<usize> {
        1..frame.len() - 5
    }
}

#[test]
fn a_spec_that_never_bounds_its_search_is_named_as_the_defect_it_is() {
    let wire = vec![b'$'; MAX];
    match parse::<Unbounded>(&wire) {
        Parsed::Desync { discard, cause } => {
            assert_eq!(discard, 1);
            assert_eq!(
                cause,
                DesyncCause::Unbounded,
                "the header is fine; what failed is the spec"
            );
        }
        other => panic!("expected the backstop, got {other:?}"),
    }

    assert!(matches!(parse::<Unbounded>(&wire[..MAX - 1]), Parsed::Incomplete { .. }));
}
