//! Magic, length and a trailing CRC-32 over bytes `4..`, as a `FrameSpec`.

#![cfg(feature = "frame")]

#[path = "support/rng.rs"]
mod rng;

use layline_core::Checksum;
use layline_core::checksum::Crc;
use layline_core::frame::{CorruptPolicy, Decoder, FrameSpec, Integrity, Parsed, parse};
use rng::Rng;

const MAGIC: u32 = 0xBAE5_AB17;
const HEADER: usize = 12;
const MAX_PAYLOAD: usize = 4096;

/// CRC-32/ISO-HDLC (IEEE 802.3).
type Crc32 = Crc<u32, 0xEDB8_8320, 0xFFFF_FFFF, 0xFFFF_FFFF, true>;

fn crc32(data: &[u8]) -> u32 {
    <Crc32 as Checksum>::compute(data)
}

/// Magic, seq, ack, id, len, payload, CRC32.
struct MagicLenCrc32;

impl FrameSpec for MagicLenCrc32 {
    const CANDIDATE_LEN: usize = 4;
    const HEADER_LEN: usize = HEADER;
    const MAX_FRAME: usize = HEADER + MAX_PAYLOAD + 4;
    const ON_CORRUPT: CorruptPolicy = CorruptPolicy::Consume;

    fn candidate(bytes: &[u8]) -> bool {
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) == MAGIC
    }

    fn frame_len(header: &[u8]) -> Option<usize> {
        let len = u16::from_le_bytes([header[10], header[11]]) as usize;
        Some(HEADER + len + 4)
    }

    fn verify(frame: &[u8]) -> bool {
        let n = frame.len();
        let expected = u32::from_le_bytes([frame[n - 4], frame[n - 3], frame[n - 2], frame[n - 1]]);
        crc32(&frame[4..n - 4]) == expected
    }

    fn body(frame: &[u8]) -> core::ops::Range<usize> {
        HEADER..frame.len() - 4
    }
}

fn encode(seq: u16, ack: u16, id: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + payload.len() + 4);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&ack.to_le_bytes());
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    out.extend_from_slice(payload);
    let crc = crc32(&out[4..]);
    out.extend_from_slice(&crc.to_le_bytes());
    out
}

#[test]
fn encode_parse_identity() {
    let frame = encode(1, 0, 401, &[5, 6, 7]);
    assert_eq!(
        parse::<MagicLenCrc32>(&frame),
        Parsed::Frame {
            consumed: HEADER + 3 + 4,
            body: HEADER..HEADER + 3,
            integrity: Integrity::Verified,
        }
    );
}

#[test]
fn a_corrupt_frame_is_consumed_and_reported() {
    let mut frame = encode(2, 1, 401, &[9; 16]);
    let n = frame.len();
    frame[HEADER + 3] ^= 0x01;
    assert_eq!(
        parse::<MagicLenCrc32>(&frame),
        Parsed::Frame { consumed: n, body: HEADER..n - 4, integrity: Integrity::Corrupt },
        "a damaged-but-real frame is consumed, not resynchronized past, \
         so the stream keeps moving"
    );
}

#[test]
fn an_oversized_length_field_is_rejected_without_waiting() {
    let mut frame = encode(3, 0, 401, &[]);
    frame[10] = 0xFF;
    frame[11] = 0xFF;
    match parse::<MagicLenCrc32>(&frame) {
        Parsed::Desync { cause, .. } => {
            assert_eq!(cause, layline_core::frame::DesyncCause::TooLarge);
        }
        other => panic!("expected TooLarge desync, got {other:?}"),
    }
}

#[test]
fn frames_are_recovered_from_noise_and_arbitrary_chunking() {
    let mut rng = Rng(0xB17_CA1);
    let payloads: Vec<Vec<u8>> = (0..30)
        .map(|i| {
            let mut p = vec![0u8; rng.below(50)];
            rng.fill(&mut p);
            p.push(i as u8);
            p
        })
        .collect();

    let mut stream = Vec::new();
    for (i, payload) in payloads.iter().enumerate() {
        let mut noise = vec![0u8; rng.below(15)];
        rng.fill(&mut noise);
        stream.extend_from_slice(&noise);
        stream.extend_from_slice(&encode(i as u16, 0, 101, payload));
    }

    let mut decoder: Decoder<MagicLenCrc32, 8192> = Decoder::new();
    let frames = chunked_feed!(decoder, stream, rng);

    let recovered: Vec<&Vec<u8>> =
        frames.iter().filter(|(_, i)| *i == Integrity::Verified).map(|(b, _)| b).collect();
    assert_eq!(recovered, payloads.iter().collect::<Vec<_>>());
    assert_eq!(decoder.stats().checksum_errors, 0);
}
