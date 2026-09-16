//! Trait bounds that generated code places on field types.
//!
//! A `#[count]`, `#[len]` or `#[offset]` field needs [`WireInt`]. A bare `#[when(f)]` flag needs
//! [`WireFlag`]. A `#[bytes(N)]` field needs [`Nestable`].

/// An integer read from the wire, such as a count, length or offset.
///
/// Implemented for the integer primitives, [`Uleb128`] and [`Sleb128`].
///
/// [`Uleb128`]: crate::num::Uleb128
/// [`Sleb128`]: crate::num::Sleb128
///
/// ```
/// use layline_core::WireInt;
///
/// assert_eq!(WireInt::to_i64(&300u16), 300);
/// assert_eq!(u16::from_i64(300), 300);
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a count, length, offset, discriminant or flags field",
    label = "not a `WireInt`",
    note = "use an integer type (`u8`..`u64`, `i8`..`i64`) or a type that implements `layline::WireInt`"
)]
pub trait WireInt: Sized {
    /// The value as an `i64`, saturating at `i64::MAX`.
    fn to_i64(&self) -> i64;

    /// A value holding `n`.
    fn from_i64(n: i64) -> Self;

    /// [`fits_in_field`] called through a value, so a type that is not `WireInt` gets one error.
    #[doc(hidden)]
    fn __fits_in_field(&self, value: i64, width: u32) -> bool {
        fits_in_field::<Self>(value, width)
    }
}

/// The one-bit flag a bare `#[when(f)]` tests. Encode sets it from `is_some`.
///
/// Only `bool` implements it.
///
/// ```
/// use layline_core::__private::WireFlag;
///
/// assert!(WireFlag::to_flag(&true));
/// assert_eq!(bool::from_flag(false), false);
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a presence flag",
    label = "not a one-bit flag",
    note = "a bare `#[when(f)]` needs `f` to be a `bool`. \
            For a wider field, give a mask: `#[when(f & 0x01)]`"
)]
pub trait WireFlag: crate::sealed::WireFlag + Sized {
    /// Whether the optional field is present.
    fn to_flag(&self) -> bool;

    /// A flag set from `present`.
    fn from_flag(present: bool) -> Self;
}

impl crate::sealed::WireFlag for bool {}

impl WireFlag for bool {
    fn to_flag(&self) -> bool {
        *self
    }

    fn from_flag(present: bool) -> Self {
        present
    }
}

/// A layout a message can contain, as in `#[bytes(N)] header: Header`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` must derive `Debug`, `Clone` and `PartialEq` to be nested in a message",
    label = "nested here",
    note = "write `#[derive(Debug, Clone, PartialEq, Layout)]`"
)]
pub trait Nestable: crate::sealed::Nestable {}

#[diagnostic::do_not_recommend]
impl<T> crate::sealed::Nestable for T where T: core::fmt::Debug + Clone + PartialEq {}

#[diagnostic::do_not_recommend]
impl<T> Nestable for T where T: core::fmt::Debug + Clone + PartialEq {}

macro_rules! wire_int_scalar {
    ($($t:ty),* $(,)?) => {
        $(
            impl WireInt for $t {
                fn to_i64(&self) -> i64 {
                    *self as i64
                }

                fn from_i64(n: i64) -> Self {
                    n as Self
                }
            }
        )*
    };
}

wire_int_scalar!(u8, u16, u32, i8, i16, i32, i64);

impl WireInt for u64 {
    fn to_i64(&self) -> i64 {
        // Saturate: a wrapped count would be negative and read as small.
        (*self).min(i64::MAX as u64) as i64
    }

    fn from_i64(n: i64) -> Self {
        n as Self
    }
}

/// Whether `value` survives storing into a `width`-bit field of type `T`.
///
/// `width == 0` means the full width of `T`.
///
/// ```
/// use layline_core::__private::fits_in_field;
///
/// // Three bits, unsigned: 0..=7.
/// assert!(fits_in_field::<u8>(7, 3));
/// assert!(!fits_in_field::<u8>(8, 3));
/// // Four bits, signed: -8..=7.
/// assert!(fits_in_field::<i8>(-8, 4));
/// assert!(!fits_in_field::<i8>(8, 4));
/// // Full width: the range of `u8`.
/// assert!(fits_in_field::<u8>(255, 8));
/// assert!(!fits_in_field::<u8>(256, 8));
/// ```
pub fn fits_in_field<T: WireInt>(value: i64, width: u32) -> bool {
    if T::to_i64(&T::from_i64(value)) != value {
        return false;
    }
    if width == 0 || width >= 64 {
        return true;
    }
    let mask = crate::mask::mask64(width);
    let raw = (value as u64) & mask;
    let signed = T::to_i64(&T::from_i64(-1)) < 0;
    let back =
        if signed && (raw >> (width - 1)) & 1 == 1 { (raw | !mask) as i64 } else { raw as i64 };
    back == value
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_u64_past_the_i64_range_saturates() {
        use super::WireInt;
        assert_eq!(u64::MAX.to_i64(), i64::MAX);
        assert_eq!((i64::MAX as u64 + 1).to_i64(), i64::MAX);
        assert_eq!(7u64.to_i64(), 7);
    }

    use super::fits_in_field;

    #[test]
    fn an_unsigned_field_holds_its_own_range_and_no_more() {
        for w in 1..=8u32 {
            let hi = (1i64 << w) - 1;
            assert!(fits_in_field::<u8>(hi, w), "{hi} in {w} unsigned bits");
            assert!(!fits_in_field::<u8>(hi + 1, w), "{} in {w} unsigned bits", hi + 1);
            assert!(!fits_in_field::<u8>(-1, w), "-1 in {w} unsigned bits");
        }
    }

    #[test]
    fn a_signed_field_holds_both_ends_of_its_own_range() {
        for w in 2..=8u32 {
            let hi = (1i64 << (w - 1)) - 1;
            let lo = -(1i64 << (w - 1));
            assert!(fits_in_field::<i8>(hi, w), "{hi} in {w} signed bits");
            assert!(fits_in_field::<i8>(lo, w), "{lo} in {w} signed bits");
            assert!(!fits_in_field::<i8>(hi + 1, w));
            assert!(!fits_in_field::<i8>(lo - 1, w));
        }
    }

    #[test]
    fn the_carrier_still_has_the_last_word() {
        assert!(!fits_in_field::<u8>(300, 16));
        assert!(fits_in_field::<u16>(300, 16));
        assert!(fits_in_field::<u16>(65535, 0));
        assert!(!fits_in_field::<u16>(65536, 0));
    }
}
