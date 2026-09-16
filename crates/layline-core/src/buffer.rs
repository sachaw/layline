//! Output buffers for encoding.

/// A byte buffer that encode appends to and patches.
///
/// Encode fills in computed values and reads checksum ranges through
/// [`written_mut`](Self::written_mut). Implemented for [`Vec<u8>`](alloc::vec::Vec) and [`Fixed`].
///
/// ```
/// use layline_core::{Buffer, Fixed};
///
/// let mut room = [0u8; 4];
/// let mut out = Fixed::new(&mut room);
/// out.push(&[1, 2, 3]).unwrap();
/// out.written_mut()[0] = 9;
/// assert_eq!(out.written(), &[9, 2, 3]);
/// assert!(out.push(&[4, 5]).is_err());
/// ```
pub trait Buffer {
    /// Append `bytes`.
    ///
    /// # Errors
    ///
    /// [`Overflow`], leaving the buffer unchanged.
    fn push(&mut self, bytes: &[u8]) -> Result<(), Overflow>;

    /// The bytes written so far.
    fn written(&self) -> &[u8];

    /// The bytes written so far, mutably.
    fn written_mut(&mut self) -> &mut [u8];

    /// Truncate to `to` bytes. Does nothing if `to` is at or past the end.
    fn rewind(&mut self, to: usize);

    /// The number of bytes written.
    fn len(&self) -> usize {
        self.written().len()
    }

    /// Whether no bytes are written.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Append `n` zero bytes.
    ///
    /// # Errors
    ///
    /// [`Overflow`] if there is no room.
    fn pad(&mut self, n: usize) -> Result<(), Overflow> {
        const ZEROS: [u8; 64] = [0; 64];
        let mut left = n;
        while left > 0 {
            let take = if left < ZEROS.len() { left } else { ZEROS.len() };
            self.push(&ZEROS[..take])?;
            left -= take;
        }
        Ok(())
    }
}

/// Run one encode into a new vector.
#[cfg(feature = "alloc")]
pub(crate) fn to_vec(
    encode: impl FnOnce(&mut alloc::vec::Vec<u8>) -> Result<(), Overflow>,
) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec::Vec::new();
    encode(&mut out).expect("a vector grows to fit");
    out
}

/// A buffer ran out of room during encode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Overflow;

impl core::fmt::Display for Overflow {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("the buffer has no room for the encoded bytes")
    }
}

impl core::error::Error for Overflow {}

/// A [`Buffer`] over a caller's byte slice, for encoding without an allocator.
#[derive(Debug)]
pub struct Fixed<'a> {
    room: &'a mut [u8],
    at: usize,
}

impl<'a> Fixed<'a> {
    /// An empty buffer over `room`.
    #[must_use]
    pub const fn new(room: &'a mut [u8]) -> Self {
        Self { room, at: 0 }
    }

    /// The bytes written, borrowed for the slice's lifetime.
    #[must_use]
    pub fn into_written(self) -> &'a [u8] {
        &self.room[..self.at]
    }
}

impl Buffer for Fixed<'_> {
    fn push(&mut self, bytes: &[u8]) -> Result<(), Overflow> {
        let end = self.at.checked_add(bytes.len()).ok_or(Overflow)?;
        if end > self.room.len() {
            return Err(Overflow);
        }
        self.room[self.at..end].copy_from_slice(bytes);
        self.at = end;
        Ok(())
    }

    fn written(&self) -> &[u8] {
        &self.room[..self.at]
    }

    fn written_mut(&mut self) -> &mut [u8] {
        &mut self.room[..self.at]
    }

    fn rewind(&mut self, to: usize) {
        self.at = self.at.min(to);
    }

    fn len(&self) -> usize {
        self.at
    }
}

impl<T: Buffer + ?Sized> Buffer for &mut T {
    fn push(&mut self, bytes: &[u8]) -> Result<(), Overflow> {
        T::push(self, bytes)
    }

    fn written(&self) -> &[u8] {
        T::written(self)
    }

    fn written_mut(&mut self) -> &mut [u8] {
        T::written_mut(self)
    }

    fn rewind(&mut self, to: usize) {
        T::rewind(self, to);
    }

    fn len(&self) -> usize {
        T::len(self)
    }
}

#[cfg(feature = "alloc")]
impl Buffer for alloc::vec::Vec<u8> {
    fn push(&mut self, bytes: &[u8]) -> Result<(), Overflow> {
        self.extend_from_slice(bytes);
        Ok(())
    }

    fn written(&self) -> &[u8] {
        self
    }

    fn written_mut(&mut self) -> &mut [u8] {
        self
    }

    fn rewind(&mut self, to: usize) {
        self.truncate(to);
    }

    fn len(&self) -> usize {
        alloc::vec::Vec::len(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fixed_buffer_fills_to_its_last_byte_and_refuses_the_next() {
        let mut room = [0u8; 3];
        let mut out = Fixed::new(&mut room);
        assert_eq!(out.push(&[1, 2]), Ok(()));
        assert_eq!(out.push(&[3]), Ok(()));
        assert_eq!(out.push(&[4]), Err(Overflow));
        assert_eq!(out.len(), 3);
        assert_eq!(out.into_written(), &[1, 2, 3]);
    }

    #[test]
    fn a_refused_push_writes_nothing() {
        let mut room = [0u8; 4];
        let mut out = Fixed::new(&mut room);
        out.push(&[1, 2]).unwrap();
        assert_eq!(out.push(&[3, 4, 5]), Err(Overflow));
        assert_eq!(out.written(), &[1, 2]);
    }

    #[test]
    fn a_rewind_drops_the_bytes_past_it_and_the_next_push_lands_there() {
        let mut room = [0u8; 8];
        let mut out = Fixed::new(&mut room);
        out.push(&[1, 2, 3, 4]).unwrap();
        out.rewind(2);
        out.push(&[9]).unwrap();
        assert_eq!(out.written(), &[1, 2, 9]);
    }

    #[test]
    fn a_rewind_past_the_end_drops_nothing() {
        let mut room = [0xEEu8; 8];
        let mut out = Fixed::new(&mut room);
        out.push(&[1, 2]).unwrap();
        out.rewind(5);
        assert_eq!(out.written(), &[1, 2]);
    }

    #[test]
    fn a_pad_longer_than_the_zero_run_still_writes_zeros() {
        let mut room = [0xFFu8; 200];
        let mut out = Fixed::new(&mut room);
        out.pad(130).unwrap();
        assert_eq!(out.len(), 130);
        assert!(out.written().iter().all(|b| *b == 0));
    }

    #[test]
    fn a_pad_past_the_end_is_refused() {
        let mut room = [0u8; 4];
        let mut out = Fixed::new(&mut room);
        assert_eq!(out.pad(5), Err(Overflow));
    }
}
