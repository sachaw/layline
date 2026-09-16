//! Id-keyed catalogues.

/// A catalogue of payloads keyed by id. `#[derive(Dispatch)]` implements it.
///
/// Decode never fails. An unlisted id, a wrong length or a rejected body goes to the `#[other]`
/// arm unchanged, and [`unlisted`](Self::unlisted) returns its id.
///
/// `'a` is the lifetime of an `#[other]` body of type `&'a [u8]`.
pub trait Dispatch<'a>: Sized {
    /// The id type.
    type Id: Copy;

    /// The low bits of the frame that hold the id, or zero if the id comes from elsewhere.
    ///
    /// Every arm's [`Layout::PREFIX_BITS`](crate::Layout::PREFIX_BITS) must equal it.
    const PREFIX_BITS: u32 = 0;

    /// Decode `body` as the payload for `id`.
    fn decode(id: Self::Id, body: &'a [u8]) -> Self;

    /// This payload's id.
    fn id(&self) -> Self::Id;

    /// `Some(id)` for the `#[other]` arm, `None` otherwise.
    fn unlisted(&self) -> Option<Self::Id>;

    /// Append the payload's bytes to `out`.
    ///
    /// # Errors
    ///
    /// [`Overflow`](crate::Overflow) if there is no room.
    fn encode_into<B: crate::Buffer>(&self, out: &mut B) -> Result<(), crate::Overflow>;
}
