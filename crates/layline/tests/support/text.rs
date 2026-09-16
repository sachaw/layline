//! Text codecs a format names, written as a consumer writes them.
#![allow(dead_code)]

use layline::{Buffer, Overflow};
use layline_core::TextCodec;

/// US-ASCII: one byte per character, refusing bytes above `0x7F`; a wider character writes `?`.
pub struct Ascii;

impl TextCodec for Ascii {
    fn decode(bytes: &[u8]) -> Option<String> {
        bytes.is_ascii().then(|| bytes.iter().map(|&b| char::from(b)).collect())
    }

    fn encode_char<B: Buffer>(c: char, out: &mut B) -> Result<(), Overflow> {
        out.push(&[if c.is_ascii() { c as u8 } else { b'?' }])
    }

    fn encoded_len(text: &str) -> usize {
        text.chars().count()
    }
}

/// ISO/IEC 8859-1: byte `n` is code point `U+00n`; a wider character writes `?`.
pub struct Latin1;

impl TextCodec for Latin1 {
    fn decode(bytes: &[u8]) -> Option<String> {
        Some(bytes.iter().map(|&b| char::from(b)).collect())
    }

    fn encode_char<B: Buffer>(c: char, out: &mut B) -> Result<(), Overflow> {
        out.push(&[u8::try_from(u32::from(c)).unwrap_or(b'?')])
    }

    fn encoded_len(text: &str) -> usize {
        text.chars().count()
    }
}
