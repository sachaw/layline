//! A header checksum that gates the length field, as a `FrameSpec`: five little-endian header words
//! (sync, id, count, flags, ck1), then `count` data words and a second checksum when `count > 0`.

#![cfg(feature = "frame")]

#[path = "support/rng.rs"]
mod rng;

use layline_core::frame::{CorruptPolicy, Decoder, FrameSpec, Integrity, Parsed, parse};
use rng::Rng;

/// Negated sum-16 of the words: a correct block plus its checksum word sums to zero.
fn sum16_neg(words: &[u16]) -> u16 {
    words.iter().fold(0u16, |acc, &w| acc.wrapping_add(w)).wrapping_neg()
}

/// Whether the block, checksum word included, sums to zero.
fn sum16_ok(words: &[u16]) -> bool {
    words.iter().fold(0u16, |acc, &w| acc.wrapping_add(w)) == 0
}

const SYNC_WORD: u16 = 0x81FF;
const HEADER_BYTES: usize = 10;

fn word_at(buf: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([buf[i], buf[i + 1]])
}

struct HeaderCkLen;

impl FrameSpec for HeaderCkLen {
    const CANDIDATE_LEN: usize = 2;
    const HEADER_LEN: usize = HEADER_BYTES;
    const MAX_FRAME: usize = HEADER_BYTES + 512 * 2 + 2;
    const ON_CORRUPT: CorruptPolicy = CorruptPolicy::Consume;

    fn candidate(bytes: &[u8]) -> bool {
        word_at(bytes, 0) == SYNC_WORD
    }

    fn frame_len(header: &[u8]) -> Option<usize> {
        let words = [
            word_at(header, 0),
            word_at(header, 2),
            word_at(header, 4),
            word_at(header, 6),
            word_at(header, 8),
        ];
        if !sum16_ok(&words) {
            return None;
        }
        let count = words[2] as usize;
        Some(if count == 0 { HEADER_BYTES } else { HEADER_BYTES + count * 2 + 2 })
    }

    fn verify(frame: &[u8]) -> bool {
        if frame.len() == HEADER_BYTES {
            return true;
        }
        let mut sum = 0u16;
        let mut i = HEADER_BYTES;
        while i < frame.len() {
            sum = sum.wrapping_add(word_at(frame, i));
            i += 2;
        }
        sum == 0
    }

    fn body(frame: &[u8]) -> core::ops::Range<usize> {
        if frame.len() == HEADER_BYTES {
            HEADER_BYTES..HEADER_BYTES
        } else {
            HEADER_BYTES..frame.len() - 2
        }
    }
}

fn encode(message_id: u16, flags: u16, data_words: &[u16]) -> Vec<u8> {
    let count = data_words.len() as u16;
    let ck1 = sum16_neg(&[SYNC_WORD, message_id, count, flags]);
    let mut out = Vec::new();
    for w in [SYNC_WORD, message_id, count, flags, ck1] {
        out.extend_from_slice(&w.to_le_bytes());
    }
    if count > 0 {
        for w in data_words {
            out.extend_from_slice(&w.to_le_bytes());
        }
        out.extend_from_slice(&sum16_neg(data_words).to_le_bytes());
    }
    out
}

#[test]
fn encode_parse_identity_with_and_without_body() {
    let framed = encode(5000, 0x0800, &[0xAB, 0xCD, 0xEF]);
    assert_eq!(
        parse::<HeaderCkLen>(&framed),
        Parsed::Frame {
            consumed: HEADER_BYTES + 8,
            body: HEADER_BYTES..HEADER_BYTES + 6,
            integrity: Integrity::Verified,
        }
    );

    let header_only = encode(0, 0x0040, &[]);
    assert_eq!(
        parse::<HeaderCkLen>(&header_only),
        Parsed::Frame {
            consumed: HEADER_BYTES,
            body: HEADER_BYTES..HEADER_BYTES,
            integrity: Integrity::Verified,
        }
    );
}

#[test]
fn a_corrupt_length_field_is_never_trusted() {
    let mut framed = encode(5000, 0, &[1, 2, 3]);
    framed[4] ^= 0x40;
    match parse::<HeaderCkLen>(&framed) {
        Parsed::Desync { cause, .. } => {
            assert_eq!(cause, layline_core::frame::DesyncCause::BadHeader);
        }
        other => panic!("expected BadHeader desync, got {other:?}"),
    }
}

#[test]
fn a_corrupt_body_is_consumed_and_delivered_degraded() {
    let mut framed = encode(5000, 0, &[0x1111, 0x2222]);
    let n = framed.len();
    framed[HEADER_BYTES] ^= 0xFF;
    assert_eq!(
        parse::<HeaderCkLen>(&framed),
        Parsed::Frame { consumed: n, body: HEADER_BYTES..n - 2, integrity: Integrity::Corrupt },
        "the header was trusted, so the frame is consumed; the body is \
         handed over marked corrupt for the caller to keep as Unknown"
    );
}

#[test]
fn frames_are_recovered_from_noise_and_arbitrary_chunking() {
    let mut rng = Rng(0x6551);
    let messages: Vec<Vec<u16>> = (0..30)
        .map(|i| {
            let mut words = vec![0u16; rng.below(16)];
            for w in &mut words {
                *w = rng.next() as u16;
            }
            words.push(i as u16);
            words
        })
        .collect();

    let mut stream = Vec::new();
    for words in &messages {
        let mut noise = vec![0u8; rng.below(12)];
        rng.fill(&mut noise);
        stream.extend_from_slice(&noise);
        stream.extend_from_slice(&encode(5000, 0, words));
    }

    let mut decoder: Decoder<HeaderCkLen, 2048> = Decoder::new();
    let frames = chunked_feed!(decoder, stream, rng);

    let recovered: Vec<Vec<u16>> = frames
        .iter()
        .filter(|(_, i)| *i == Integrity::Verified)
        .map(|(b, _)| b.as_chunks::<2>().0.iter().copied().map(u16::from_le_bytes).collect())
        .collect();
    assert_eq!(recovered, messages);
}
