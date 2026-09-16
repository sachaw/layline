//! Integers with a width set by a const parameter.
use core::fmt;

use crate::FieldCodec;
use crate::mask::mask64;

/// `n`, or a panic with `refused` unless `n` is in `1..=64`.
const fn checked_width(n: u32, refused: &str) -> u32 {
    assert!(matches!(n, 1..=64), "{}", refused);
    n
}

/// An unsigned integer exactly `N` bits wide, with `N` in `1..=64`.
///
/// A `U<4>` field needs no `#[bits(4)]`. `U<0>` and `U<65>` fail to compile where used.
/// `Debug` prints `U<4>(9)`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct U<const N: u32>(u64);

impl<const N: u32> U<N> {
    const WIDTH: u32 = checked_width(N, "U<N>: N must be in 1..=64");

    const MASK: u64 = mask64(Self::WIDTH);

    /// The largest value.
    pub const MAX: Self = Self(Self::MASK);

    /// Zero.
    pub const MIN: Self = Self(0);

    /// The value, or `None` if it does not fit in `N` bits.
    ///
    /// ```
    /// # use layline_core::U;
    /// const TOP: U<4> = match U::new(15) { Some(v) => v, None => unreachable!() };
    /// assert_eq!(TOP, U::<4>::MAX);
    /// ```
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value <= Self::MASK { Some(Self(value)) } else { None }
    }

    /// The value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl<const N: u32> fmt::Debug for U<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "U<{N}>({})", self.0)
    }
}

impl<const N: u32> FieldCodec for U<N> {
    const BITS: u32 = Self::WIDTH;

    fn from_raw(raw: u64) -> Self {
        Self(raw & Self::MASK)
    }

    fn to_raw(&self) -> u64 {
        self.0
    }
}

/// A two's-complement signed integer exactly `N` bits wide, with `N` in `1..=64`.
///
/// The wire form is the low `N` bits. `Debug` prints `I<4>(-7)`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct I<const N: u32>(i64);

impl<const N: u32> I<N> {
    const WIDTH: u32 = checked_width(N, "I<N>: N must be in 1..=64");

    const MASK: u64 = mask64(Self::WIDTH);

    /// The largest value.
    pub const MAX: Self = Self(i64::MAX >> (64 - Self::WIDTH));

    /// The smallest value.
    pub const MIN: Self = Self(i64::MIN >> (64 - Self::WIDTH));

    /// The value, or `None` if it is outside `MIN..=MAX`.
    #[must_use]
    pub const fn new(value: i64) -> Option<Self> {
        if value >= Self::MIN.0 && value <= Self::MAX.0 { Some(Self(value)) } else { None }
    }

    /// The value.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

impl<const N: u32> fmt::Debug for I<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I<{N}>({})", self.0)
    }
}

impl<const N: u32> FieldCodec for I<N> {
    const BITS: u32 = Self::WIDTH;

    fn from_raw(raw: u64) -> Self {
        Self(sign_extend(raw, Self::WIDTH))
    }

    fn to_raw(&self) -> u64 {
        self.0 as u64 & Self::MASK
    }
}

const fn sign_extend(raw: u64, bits: u32) -> i64 {
    let shift = 64 - bits;
    ((raw << shift) as i64) >> shift
}

impl<const N: u32> From<U<N>> for u64 {
    fn from(v: U<N>) -> Self {
        v.0
    }
}

impl<const N: u32> From<I<N>> for i64 {
    fn from(v: I<N>) -> Self {
        v.0
    }
}

#[cfg(feature = "serde")]
impl<const N: u32> serde_core::Serialize for U<N> {
    fn serialize<S: serde_core::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

#[cfg(feature = "serde")]
impl<const N: u32> serde_core::Serialize for I<N> {
    fn serialize<S: serde_core::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

macro_rules! exact_width {
    ($($n:literal => $unsigned:ty, $signed:ty;)*) => {$(
        impl From<U<$n>> for $unsigned {
            fn from(v: U<$n>) -> Self {
                v.0 as $unsigned
            }
        }

        impl From<$unsigned> for U<$n> {
            fn from(v: $unsigned) -> Self {
                Self(u64::from(v))
            }
        }

        impl From<I<$n>> for $signed {
            fn from(v: I<$n>) -> Self {
                v.0 as $signed
            }
        }

        impl From<$signed> for I<$n> {
            fn from(v: $signed) -> Self {
                Self(i64::from(v))
            }
        }
    )*};
}

exact_width! {
    8 => u8, i8;
    16 => u16, i16;
    32 => u32, i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_width_is_one_to_sixty_four_bits() {
        assert_eq!(checked_width(1, "refused"), 1);
        assert_eq!(checked_width(64, "refused"), 64);
        assert_eq!(U::<1>::WIDTH, 1);
        assert_eq!(I::<64>::WIDTH, 64);
    }

    #[test]
    #[should_panic(expected = "zero bits is not a field")]
    fn zero_bits_is_not_a_width() {
        checked_width(0, "zero bits is not a field");
    }

    #[test]
    #[should_panic(expected = "a field is at most 64")]
    fn a_width_past_64_is_refused() {
        checked_width(65, "a field is at most 64");
    }

    #[test]
    fn a_value_too_wide_has_no_constructor() {
        assert_eq!(U::<4>::new(15).map(U::get), Some(15));
        assert_eq!(U::<4>::new(16), None);
        assert_eq!(U::<4>::from_raw(0xFF).get(), 15);
        assert_eq!(U::<64>::new(u64::MAX).map(U::get), Some(u64::MAX));
    }

    #[test]
    fn a_signed_value_is_two_s_complement() {
        assert_eq!(I::<4>::MIN.get(), -8);
        assert_eq!(I::<4>::MAX.get(), 7);
        assert_eq!(I::<4>::new(8), None);
        assert_eq!(I::<4>::new(-9), None);
        assert_eq!(I::<4>::from_raw(0b1111).get(), -1);
        assert_eq!(I::<4>::from_raw(9).get(), -7);
        assert_eq!(I::<64>::MIN.get(), i64::MIN);
        assert_eq!(I::<64>::MAX.get(), i64::MAX);
    }

    #[test]
    fn every_width_round_trips_through_the_codec() {
        fn check<T: FieldCodec + PartialEq + core::fmt::Debug>(raw: u64) {
            let v = T::from_raw(raw);
            assert_eq!(v.to_raw(), raw, "round trip");
            assert_eq!(T::from_raw(v.to_raw()), v);
        }
        for raw in 0..16u64 {
            check::<U<4>>(raw);
            check::<I<4>>(raw);
        }
        check::<U<1>>(1);
        check::<I<1>>(1);
        check::<U<64>>(u64::MAX);
        check::<I<64>>(u64::MAX);
        check::<U<63>>(u64::MAX >> 1);
        check::<I<63>>(u64::MAX >> 1);
    }

    #[test]
    fn conversions_are_honest() {
        assert_eq!(u8::from(U::<8>::from_raw(200)), 200);
        assert_eq!(U::<8>::from(200u8).get(), 200);
        assert_eq!(u64::from(U::<12>::from_raw(4095)), 4095);
        assert_eq!(i8::from(I::<8>::new(-3).unwrap()), -3);
        assert_eq!(I::<8>::from(-3i8).get(), -3);
    }
}
