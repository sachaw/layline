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

/// A flag stored in `BITS` bits that is set when any of them is.
///
/// Decode reads any nonzero value as `true`, and encode writes `true` as 1. Use it where a format
/// tests a flag with `!= 0`. A plain `bool` in a byte-mode layout is strict instead: 0 and 1 are
/// the only values that decode.
///
/// ```
/// use layline_core::{FieldCodec, Nonzero};
///
/// assert_eq!(Nonzero::<8>::from_raw(0x02), Nonzero(true));
/// assert_eq!(Nonzero::<8>(true).to_raw(), 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Nonzero<const BITS: u32 = 8>(pub bool);

impl<const BITS: u32> Nonzero<BITS> {
    /// Whether the flag is set.
    #[inline]
    #[must_use]
    pub const fn get(self) -> bool {
        self.0
    }
}

impl<const BITS: u32> From<bool> for Nonzero<BITS> {
    #[inline]
    fn from(set: bool) -> Self {
        Self(set)
    }
}

impl<const BITS: u32> From<Nonzero<BITS>> for bool {
    #[inline]
    fn from(flag: Nonzero<BITS>) -> Self {
        flag.0
    }
}

impl<const BITS: u32> FieldCodec for Nonzero<BITS> {
    const BITS: u32 = {
        assert!(BITS >= 1 && BITS <= 64, "a `Nonzero` is 1 to 64 bits wide");
        BITS
    };

    #[inline]
    fn from_raw(raw: u64) -> Self {
        Self(raw != 0)
    }

    #[inline]
    fn to_raw(&self) -> u64 {
        u64::from(self.0)
    }
}
