//! Borrowed views of layouts.

/// A `#[repr(transparent)]` view over a [`Layout`]'s wire bytes.
///
/// Generated views also have an infallible `from_wire(&[u8; N]) -> &Self`.
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

    /// Decode into the owned layout.
    fn decode(&self) -> Self::Owned;
}
