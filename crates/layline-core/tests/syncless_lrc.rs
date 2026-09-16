//! A self-checking header with no sync word, as a `FrameSpec`: a 5-byte header (LRC, id, len,
//! CRC16-LE), then a body of at most 255 bytes.

#![cfg(feature = "frame")]

#[path = "support/rng.rs"]
mod rng;

use layline_core::Checksum;
use layline_core::checksum::Crc;

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

/// CRC-16/CCITT-FALSE.
type Crc16Ccitt = Crc<u16, 0x1021, 0xFFFF, 0x0000, false>;
use layline_core::frame::{CorruptPolicy, Decoder, FrameSpec, Integrity, Parsed, parse};
use rng::Rng;

struct SynclessLrc;

impl FrameSpec for SynclessLrc {
    const CANDIDATE_LEN: usize = 5;
    const HEADER_LEN: usize = 5;
    const MAX_FRAME: usize = 5 + 255;
    const ON_CORRUPT: CorruptPolicy = CorruptPolicy::Resync;
    const SCAN_PAST_INCOMPLETE: bool = true;

    fn candidate(bytes: &[u8]) -> bool {
        bytes[0] == <Lrc8 as Checksum>::compute(&bytes[1..5])
    }

    fn frame_len(header: &[u8]) -> Option<usize> {
        Some(5 + header[2] as usize)
    }

    fn verify(frame: &[u8]) -> bool {
        let crc = u16::from_le_bytes([frame[3], frame[4]]);
        <Crc16Ccitt as Checksum>::compute(&frame[5..]) == crc
    }

    fn body(frame: &[u8]) -> core::ops::Range<usize> {
        5..frame.len()
    }
}

fn encode(id: u8, body: &[u8]) -> Vec<u8> {
    assert!(body.len() <= 255);
    let crc = <Crc16Ccitt as Checksum>::compute(body);
    let [crc_lo, crc_hi] = crc.to_le_bytes();
    let mut out = vec![0, id, body.len() as u8, crc_lo, crc_hi];
    out[0] = <Lrc8 as Checksum>::compute(&out[1..5]);
    out.extend_from_slice(body);
    out
}

#[test]
fn encode_parse_identity() {
    let frame = encode(20, &[1, 2, 3, 4]);
    assert_eq!(
        parse::<SynclessLrc>(&frame),
        Parsed::Frame { consumed: 9, body: 5..9, integrity: Integrity::Verified }
    );
    let empty = encode(23, &[]);
    assert_eq!(
        parse::<SynclessLrc>(&empty),
        Parsed::Frame { consumed: 5, body: 5..5, integrity: Integrity::Verified }
    );
}

#[test]
fn a_lure_costs_neither_data_nor_latency() {
    let real1 = encode(28, &[0xAA; 7]);
    let real2 = encode(29, &[0xBB; 35]);
    let mut lure = vec![0, 20, 40, 0x12, 0x34];
    lure[0] = <Lrc8 as Checksum>::compute(&lure[1..5]);

    let mut stream = lure;
    stream.extend_from_slice(&real1);
    stream.extend_from_slice(&real2);

    let mut decoder: Decoder<SynclessLrc, 260> = Decoder::new();
    let mut rng = Rng(0xA22);
    let frames = chunked_feed!(decoder, stream, rng);

    assert_eq!(frames.len(), 2, "both real frames, nothing else");
    assert_eq!(frames[0].0, vec![0xAA; 7]);
    assert_eq!(frames[1].0, vec![0xBB; 35]);
    assert!(frames.iter().all(|(_, i)| *i == Integrity::Verified));
    assert!(
        decoder.stats().bytes_discarded >= 5,
        "the lure's bytes were accounted as discarded noise"
    );
}

#[test]
fn frames_are_recovered_from_noise_and_arbitrary_chunking() {
    let mut rng = Rng(0x1CE);
    let bodies: Vec<Vec<u8>> = (0..40)
        .map(|i| {
            let mut b = vec![0u8; rng.below(60)];
            rng.fill(&mut b);
            b.push(i as u8);
            b
        })
        .collect();

    let mut stream = Vec::new();
    for body in &bodies {
        let mut noise = vec![0u8; rng.below(20)];
        rng.fill(&mut noise);
        stream.extend_from_slice(&noise);
        stream.extend_from_slice(&encode(0x50, body));
    }

    let mut decoder: Decoder<SynclessLrc, 260> = Decoder::new();
    let frames = chunked_feed!(decoder, stream, rng);

    // Random noise can form a valid frame, but these seeds produce none.
    let recovered: Vec<&Vec<u8>> =
        frames.iter().filter(|(_, i)| *i == Integrity::Verified).map(|(b, _)| b).collect();
    assert_eq!(recovered, bodies.iter().collect::<Vec<_>>());
    assert_eq!(decoder.stats().frames as usize, frames.len());
}

#[test]
fn a_noise_candidate_cannot_stall_a_complete_frame_behind_it() {
    let real = encode(28, &[0xAA; 7]);
    let mut lure = vec![0, 20, 40, 0x12, 0x34];
    lure[0] = <Lrc8 as Checksum>::compute(&lure[1..5]);

    let mut stream = lure;
    stream.extend_from_slice(&real);

    let mut decoder: Decoder<SynclessLrc, 260> = Decoder::new();
    decoder.push(&stream);
    let popped = decoder.pop().expect("the real frame, without waiting");
    assert_eq!(popped.body, &[0xAA; 7]);
    assert_eq!(popped.integrity, Integrity::Verified);
    assert_eq!(popped.frame[1], 28);
    assert_eq!(popped.frame.len(), 12);
}

#[test]
fn truncated_input_reports_incomplete_not_desync() {
    let frame = encode(20, &[9; 10]);
    for cut in 1..frame.len() {
        match parse::<SynclessLrc>(&frame[..cut]) {
            Parsed::Incomplete { .. } => {}
            other => panic!("cut at {cut}: expected Incomplete, got {other:?}"),
        }
    }
}
