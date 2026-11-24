//! Self-delimiting values.

/// A value whose width in bytes is read from the wire.
///
/// ```
/// use layline_core::{Buffer, EncodeError, Fixed, ParseError, VarCodec};
///
/// struct One;
/// impl VarCodec for One {
///     fn decode(b: &[u8]) -> Result<(Self, usize), ParseError> {
///         b.first().map(|_| (One, 1)).ok_or(ParseError::Short { need_bytes: 1, got_bytes: 0, at: 0 })
///     }
///     fn encode<B: Buffer>(&self, out: &mut B) -> Result<(), EncodeError> {
///         Ok(out.push(&[1])?)
///     }
/// }
///
/// let mut room = [0u8; 1];
/// let mut out = Fixed::new(&mut room);
/// One.encode(&mut out).unwrap();
/// assert_eq!(out.written(), &[1]);
/// ```
pub trait VarCodec: Sized {
    /// Decode from the start of `bytes`. Returns the value and the bytes consumed.
    ///
    /// A successful decode must consume at least one byte.
    ///
    /// # Errors
    ///
    /// [`Short`](crate::ParseError::Short) if the input is truncated,
    /// [`Malformed`](crate::ParseError::Malformed) if the value is too wide for `Self`.
    fn decode(bytes: &[u8]) -> Result<(Self, usize), crate::ParseError>;

    /// Whether encode can refuse a value, as it can when the type declares something a value
    /// can violate or holds a type that does. The inherent encode a derive writes returns a
    /// `Result` exactly then; the trait's methods always do.
    ///
    /// Read by a holder, since a derive cannot see a field's type.
    const ENCODE_FALLIBLE: bool = false;

    /// Append this value to `out`.
    ///
    /// # Errors
    ///
    /// [`Overflow`](crate::Overflow) if `out` has no room. A codec that refuses values returns
    /// [`EncodeError`](crate::EncodeError), which also names a value it cannot encode.
    fn encode<B: crate::Buffer>(&self, out: &mut B) -> Result<(), crate::EncodeError>;
}
