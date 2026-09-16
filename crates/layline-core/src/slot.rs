//! Typed access to catalogue variants.

/// The variant of `Self` that holds a `T`.
///
/// [`slot`](Self::slot) reads it, and `From<T>` builds it. The derives implement it for each
/// payload type used by exactly one variant.
pub trait Slot<T>: From<T> {
    /// The `T` inside, or `None` for another variant.
    #[must_use]
    fn slot(&self) -> Option<&T>;
}
