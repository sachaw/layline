//! Fixed-width field types.

/// A type stored in a fixed number of bits.
///
/// `from_raw` accepts any [`BITS`](Self::BITS)-bit value, and `to_raw` returns the same bits.
pub trait FieldCodec: Sized {
    /// Width in bits, `1..=64`.
    const BITS: u32;

    /// Decode from raw bits.
    fn from_raw(raw: u64) -> Self;

    /// The raw bits.
    fn to_raw(&self) -> u64;
}

impl FieldCodec for bool {
    const BITS: u32 = 1;

    #[inline]
    fn from_raw(raw: u64) -> Self {
        raw & 1 != 0
    }

    #[inline]
    fn to_raw(&self) -> u64 {
        *self as u64
    }
}

impl FieldCodec for f32 {
    const BITS: u32 = 32;

    #[inline]
    fn from_raw(raw: u64) -> Self {
        f32::from_bits(raw as u32)
    }

    #[inline]
    fn to_raw(&self) -> u64 {
        u64::from(self.to_bits())
    }
}

impl FieldCodec for f64 {
    const BITS: u32 = 64;

    #[inline]
    fn from_raw(raw: u64) -> Self {
        f64::from_bits(raw)
    }

    #[inline]
    fn to_raw(&self) -> u64 {
        self.to_bits()
    }
}
