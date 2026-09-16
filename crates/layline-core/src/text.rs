//! Text encodings for `#[text]` fields.
use alloc::string::String;

use crate::{Buffer, Overflow};

/// The encoding of a `#[text(Codec)]` field. A bare `#[text]` uses [`Utf8`].
///
/// Text ends are found on bytes. `#[until(b)]` ends at the first byte `b`, and `#[bytes(N)]`
/// strips trailing zero bytes before decoding. Encode panics, naming the field, if the text would
/// not read back: an `#[until(b)]` text containing `b`, or a `#[bytes(N)]` text longer than `N`
/// bytes or ending in a zero byte. An encoding whose characters can contain these bytes, such as
/// UTF-16, needs `#[len]` or `#[fill]` instead.
///
/// ```
/// use layline_core::{Buffer, Overflow, TextCodec};
///
/// /// ISO/IEC 8859-1: byte `n` is code point `U+00n`.
/// struct Latin1;
///
/// impl TextCodec for Latin1 {
///     fn decode(bytes: &[u8]) -> Option<String> {
///         Some(bytes.iter().map(|&b| char::from(b)).collect())
///     }
///
///     fn encode_char<B: Buffer>(c: char, out: &mut B) -> Result<(), Overflow> {
///         out.push(&[u8::try_from(u32::from(c)).unwrap_or(b'?')])
///     }
///
///     fn encoded_len(text: &str) -> usize {
///         text.chars().count()
///     }
/// }
///
/// assert_eq!(Latin1::decode(&[0xC9, 0x74, 0xE9]).as_deref(), Some("Été"));
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a text encoding",
    label = "not a `layline::TextCodec`",
    note = "`#[text(Codec)]` needs a type that implements `TextCodec`. A bare `#[text]` is UTF-8"
)]
pub trait TextCodec {
    /// The decoded string, or `None` if the bytes are invalid.
    fn decode(bytes: &[u8]) -> Option<String>;

    /// Append one character's bytes.
    ///
    /// # Errors
    ///
    /// [`Overflow`] if there is no room.
    fn encode_char<B: Buffer>(c: char, out: &mut B) -> Result<(), Overflow>;

    /// The encoded length of `text`, in bytes.
    fn encoded_len(text: &str) -> usize;
}

/// UTF-8, the encoding of a bare `#[text]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Utf8;

impl TextCodec for Utf8 {
    fn decode(bytes: &[u8]) -> Option<String> {
        core::str::from_utf8(bytes).ok().map(String::from)
    }

    fn encode_char<B: Buffer>(c: char, out: &mut B) -> Result<(), Overflow> {
        out.push(c.encode_utf8(&mut [0; 4]).as_bytes())
    }

    fn encoded_len(text: &str) -> usize {
        text.len()
    }
}
