//! Borrowed views of layouts.

/// A `#[repr(transparent)]` view over a [`Layout`]'s wire bytes.
///
/// A view is unchecked: it reads whatever bytes it is given, as a reader that inspects a corrupt
/// record must. [`verify`](Self::verify) runs the layout's checks when the reader chooses, and
/// [`checked`](Self::checked) views only bytes that pass them.
///
/// Generated views also have an infallible `from_wire(&[u8; N]) -> &Self`, `checked(&[u8; N])`,
/// and, for a layout with a magic or a checksum, `seal(&mut self)`, which writes them back after
/// the setters.
///
/// [`Layout`]: crate::Layout
pub trait View: Sized {
    /// The layout this view decodes to.
    type Owned: crate::Layout;

    /// View `bytes` in place, or `None` if the length is not the layout's size.
    #[must_use]
    fn from_slice(bytes: &[u8]) -> Option<&Self>;

    /// A mutable [`from_slice`](Self::from_slice). Writes go to the caller's buffer.
    #[must_use]
    fn from_slice_mut(bytes: &mut [u8]) -> Option<&mut Self>;

    /// The wire bytes.
    fn as_wire(&self) -> &[u8];

    /// Decode into the owned layout, without its checks.
    fn decode(&self) -> Self::Owned;

    /// Run the checks the owned layout's `decode` runs: magic bytes, prefix and checksums.
    ///
    /// # Errors
    ///
    /// The first check the bytes fail.
    fn verify(&self) -> Result<(), crate::ParseError> {
        Ok(())
    }

    /// View `bytes` in place if they are the layout's size and pass [`verify`](Self::verify).
    ///
    /// # Errors
    ///
    /// [`ParseError::Short`](crate::ParseError::Short) if the length is wrong, or the first
    /// check the bytes fail.
    fn checked(bytes: &[u8]) -> Result<&Self, crate::ParseError> {
        let need = <Self::Owned as crate::Layout>::WIRE_BYTES;
        let view = Self::from_slice(bytes).ok_or(crate::ParseError::Short {
            need_bytes: need,
            got_bytes: bytes.len(),
            at: 0,
        })?;
        view.verify()?;
        Ok(view)
    }
}
