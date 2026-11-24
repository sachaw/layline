//! Text codecs a format names, written as a consumer writes them.
#![allow(dead_code)]

use layline::{Buffer, EncodeError};
use layline_core::TextCodec;

/// US-ASCII: one byte per character, refusing bytes above `0x7F` and characters past `U+007F`.
pub struct Ascii;

impl TextCodec for Ascii {
    const ENCODE_FALLIBLE: bool = true;

    fn decode(bytes: &[u8]) -> Option<String> {
        bytes.is_ascii().then(|| bytes.iter().map(|&b| char::from(b)).collect())
    }

    fn encode_char<B: Buffer>(c: char, out: &mut B) -> Result<(), EncodeError> {
        if !c.is_ascii() {
            return Err(EncodeError::Unencodable { field: "Ascii", character: c });
        }
        Ok(out.push(&[c as u8])?)
    }

    fn encoded_len(text: &str) -> usize {
        text.chars().count()
    }
}

/// ISO/IEC 8859-1: byte `n` is code point `U+00n`, refusing characters past `U+00FF`.
pub struct Latin1;

impl TextCodec for Latin1 {
    const ENCODE_FALLIBLE: bool = true;

    fn decode(bytes: &[u8]) -> Option<String> {
        Some(bytes.iter().map(|&b| char::from(b)).collect())
    }

    fn encode_char<B: Buffer>(c: char, out: &mut B) -> Result<(), EncodeError> {
        let byte = u8::try_from(u32::from(c))
            .map_err(|_| EncodeError::Unencodable { field: "Latin1", character: c })?;
        Ok(out.push(&[byte])?)
    }

    fn encoded_len(text: &str) -> usize {
        text.chars().count()
    }
}
