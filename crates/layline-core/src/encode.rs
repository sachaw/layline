//! The encode error type, and what each type's encode can fail with.

use crate::field::Fallibility;
use crate::{Overflow, WireInt};

/// Why an encode failed.
///
/// Every variant but [`Overflow`](Self::Overflow) names a field and the declaration its value
/// breaks. Decode would refuse the bytes, or read back a different value, so encode refuses them
/// first. `field` is the field's name in the type that declares it, as [`ParseError`] names it; a
/// dotted name is a field of a nested layout.
///
/// [`ParseError`]: crate::ParseError
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EncodeError {
    /// The buffer has no room. [`Overflow`](crate::Overflow) as an `EncodeError`.
    Overflow,
    /// A number does not fit the bits of the field that holds it: a field narrower than its type,
    /// or the field that states a length, count, offset or discriminant.
    Unfit {
        /// The field.
        field: &'static str,
        /// The number.
        value: i128,
        /// The field's width, or zero for the whole of its type.
        bits: u32,
    },
    /// A `#[range(..)]` field is out of range.
    OutOfRange {
        /// The field.
        field: &'static str,
        /// The value.
        value: i64,
        /// The lowest allowed value.
        lo: i64,
        /// The highest allowed value.
        hi: i64,
    },
    /// A collection or text is longer than its `cap`, or than the fixed bytes it is given.
    OverCap {
        /// The collection or text.
        field: &'static str,
        /// Its length, in the cap's unit.
        len: u64,
        /// The cap.
        cap: u64,
    },
    /// No whole value of the field states this number: `field * scale + offset` skips it, or it
    /// is not a square.
    Inexact {
        /// The field encode works back.
        field: &'static str,
        /// The number it must state.
        value: i64,
    },
    /// A number the value gives differs from one stated elsewhere: a parameter, a `#[when]` flag
    /// the holder writes, or a fixed size.
    Mismatch {
        /// The field.
        field: &'static str,
        /// The number the value gives.
        value: i64,
        /// The number stated. For presence, 1 is present.
        stated: i64,
    },
    /// The value holds the byte that ends it, so decode would stop early: a terminator, an
    /// `#[until]` mask, or the zero padding of fixed-width text.
    Delimiter {
        /// The field.
        field: &'static str,
        /// The byte.
        byte: u8,
    },
    /// The value encodes to no bytes where decode needs at least one.
    Empty {
        /// The field.
        field: &'static str,
    },
    /// An `#[other]` arm holds a discriminant that a listed arm claims, so decode would read that
    /// arm.
    Claimed {
        /// The switch field.
        field: &'static str,
        /// The discriminant.
        value: i64,
    },
}

impl EncodeError {
    /// [`Unfit`](Self::Unfit) unless `value` survives storing into a `width`-bit field of type `T`.
    ///
    /// `width == 0` means the whole of `T`.
    ///
    /// # Errors
    ///
    /// [`Unfit`](Self::Unfit), naming `field`.
    #[doc(hidden)]
    pub fn fit<T: WireInt>(field: &'static str, value: i64, width: u32) -> Result<(), Self> {
        if crate::bound::fits_in_field::<T>(value, width) {
            Ok(())
        } else {
            Err(Self::Unfit { field, value: i128::from(value), bits: width })
        }
    }

    /// [`fit`](Self::fit) through a value, so a type that is not `WireInt` gets one error.
    ///
    /// # Errors
    ///
    /// As [`fit`](Self::fit).
    #[doc(hidden)]
    pub fn fit_as<T: WireInt>(
        _: &T,
        field: &'static str,
        value: i64,
        width: u32,
    ) -> Result<(), Self> {
        Self::fit::<T>(field, value, width)
    }

    /// [`OutOfRange`](Self::OutOfRange) unless `lo <= value <= hi`.
    ///
    /// # Errors
    ///
    /// [`OutOfRange`](Self::OutOfRange), naming `field`.
    #[doc(hidden)]
    pub const fn range(field: &'static str, value: i64, lo: i64, hi: i64) -> Result<(), Self> {
        if lo <= value && value <= hi {
            Ok(())
        } else {
            Err(Self::OutOfRange { field, value, lo, hi })
        }
    }

    /// [`OverCap`](Self::OverCap) unless `len <= cap`.
    ///
    /// # Errors
    ///
    /// [`OverCap`](Self::OverCap), naming `field`.
    #[doc(hidden)]
    pub const fn cap(field: &'static str, len: usize, cap: usize) -> Result<(), Self> {
        if len <= cap {
            Ok(())
        } else {
            Err(Self::OverCap { field, len: len as u64, cap: cap as u64 })
        }
    }

    /// This refusal, naming `to`. A codec does not know the field that holds it, so it names its
    /// type, and the holder renames the refusal to its own field.
    #[doc(hidden)]
    #[must_use]
    pub const fn named(self, to: &'static str) -> Self {
        match self {
            Self::Overflow => Self::Overflow,
            Self::Unfit { value, bits, .. } => Self::Unfit { field: to, value, bits },
            Self::OutOfRange { value, lo, hi, .. } => Self::OutOfRange { field: to, value, lo, hi },
            Self::OverCap { len, cap, .. } => Self::OverCap { field: to, len, cap },
            Self::Inexact { value, .. } => Self::Inexact { field: to, value },
            Self::Mismatch { value, stated, .. } => Self::Mismatch { field: to, value, stated },
            Self::Delimiter { byte, .. } => Self::Delimiter { field: to, byte },
            Self::Empty { .. } => Self::Empty { field: to },
            Self::Claimed { value, .. } => Self::Claimed { field: to, value },
        }
    }

    /// [`Mismatch`](Self::Mismatch) unless `value == stated`.
    ///
    /// # Errors
    ///
    /// [`Mismatch`](Self::Mismatch), naming `field`.
    #[doc(hidden)]
    pub const fn agree(field: &'static str, value: i64, stated: i64) -> Result<(), Self> {
        if value == stated { Ok(()) } else { Err(Self::Mismatch { field, value, stated }) }
    }
}

impl From<Overflow> for EncodeError {
    fn from(_: Overflow) -> Self {
        Self::Overflow
    }
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Overflow => f.write_str("the buffer has no room for the encoded bytes"),
            Self::Unfit { field, value, bits: 0 } => {
                write!(f, "`{field}` cannot hold {value}")
            }
            Self::Unfit { field, value, bits } => {
                write!(f, "`{field}` is {bits} bits wide and cannot hold {value}")
            }
            Self::OutOfRange { field, value, lo, hi } => {
                write!(f, "`{field}` is {value}, outside {lo}..={hi}")
            }
            Self::OverCap { field, len, cap } => {
                write!(f, "`{field}` is {len} long, past its cap of {cap}")
            }
            Self::Inexact { field, value } => {
                write!(f, "no whole value of `{field}` states {value}")
            }
            Self::Mismatch { field, value, stated } => {
                write!(f, "`{field}` gives {value}, but {stated} is stated")
            }
            Self::Delimiter { field, byte } => {
                write!(f, "`{field}` holds its end byte {byte:#04x}, so decode would stop there")
            }
            Self::Empty { field } => {
                write!(f, "`{field}` encodes to no bytes, and decode needs at least one")
            }
            Self::Claimed { field, value } => write!(
                f,
                "`{field}` holds the open arm with discriminant {value}, which a listed arm claims"
            ),
        }
    }
}

impl core::error::Error for EncodeError {}

/// What an encode that returns its bytes returns for a type whose fallibility is `FALLIBLE`:
/// `Result<T, EncodeError>` when `FALLIBLE`, and `T` otherwise.
///
/// The encode twin of [`Decoded`](crate::__private::Decoded). Each encoding trait has an
/// `ENCODE_FALLIBLE`, true exactly when the type declares something a value can violate or holds
/// a type that does. A derive cannot see a field's type, so a holder names its return type through
/// this, with `FALLIBLE` the `||` of the held types' `ENCODE_FALLIBLE`.
pub type Encoded<T, const FALLIBLE: bool> = <Fallibility<FALLIBLE> as Encodes<T>>::Out;

/// Converts between an encode's [`Encoded`] value and a `Result`.
pub trait Encodes<T> {
    /// `Result<T, EncodeError>`, or `T`.
    type Out;

    /// The encode's value, from an encode written to return a `Result`.
    ///
    /// An infallible type checks nothing and holds only infallible types, so its encode has no
    /// error to pass.
    fn finish(encoded: Result<T, EncodeError>) -> Self::Out;

    /// The encode's value as a `Result`.
    ///
    /// # Errors
    ///
    /// The encode's own error, when it has one.
    fn result(encoded: Self::Out) -> Result<T, EncodeError>;
}

impl<T> Encodes<T> for Fallibility<true> {
    type Out = Result<T, EncodeError>;

    #[inline]
    fn finish(encoded: Result<T, EncodeError>) -> Self::Out {
        encoded
    }

    #[inline]
    fn result(encoded: Self::Out) -> Result<T, EncodeError> {
        encoded
    }
}

impl<T> Encodes<T> for Fallibility<false> {
    type Out = T;

    #[inline]
    fn finish(encoded: Result<T, EncodeError>) -> T {
        match encoded {
            Ok(value) => value,
            Err(_) => {
                unreachable!("an infallible encode checks nothing and holds only infallible types")
            }
        }
    }

    #[inline]
    fn result(encoded: T) -> Result<T, EncodeError> {
        Ok(encoded)
    }
}

/// What an encode into a buffer fails with for a type whose fallibility is `FALLIBLE`:
/// [`EncodeError`] when `FALLIBLE`, and [`Overflow`] otherwise. See [`Encoded`].
pub type Buffered<const FALLIBLE: bool> = <Fallibility<FALLIBLE> as Buffers>::Error;

/// Converts an encode into a buffer to its [`Buffered`] error.
pub trait Buffers {
    /// [`EncodeError`], or [`Overflow`].
    type Error;

    /// The encode's result, from an encode written to return an [`EncodeError`].
    ///
    /// # Errors
    ///
    /// The encode's error: for an infallible type, only a full buffer.
    fn finish(encoded: Result<(), EncodeError>) -> Result<(), Self::Error>;
}

impl Buffers for Fallibility<true> {
    type Error = EncodeError;

    #[inline]
    fn finish(encoded: Result<(), EncodeError>) -> Result<(), EncodeError> {
        encoded
    }
}

impl Buffers for Fallibility<false> {
    type Error = Overflow;

    #[inline]
    fn finish(encoded: Result<(), EncodeError>) -> Result<(), Overflow> {
        match encoded {
            Ok(()) => Ok(()),
            Err(EncodeError::Overflow) => Err(Overflow),
            Err(_) => {
                unreachable!("an infallible encode checks nothing and holds only infallible types")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Buffered, Buffers, EncodeError, Encoded, Encodes};
    use crate::buf::Buf;
    use crate::field::Fallibility;

    /// Every variant, so a new one is listed here and in `Display`.
    fn every_variant() -> [EncodeError; 9] {
        [
            EncodeError::Overflow,
            EncodeError::Unfit { field: "len", value: 300, bits: 8 },
            EncodeError::OutOfRange { field: "hour", value: 30, lo: 0, hi: 23 },
            EncodeError::OverCap { field: "items", len: 9, cap: 8 },
            EncodeError::Inexact { field: "words", value: 7 },
            EncodeError::Mismatch { field: "body", value: 3, stated: 4 },
            EncodeError::Delimiter { field: "name", byte: 0 },
            EncodeError::Empty { field: "items" },
            EncodeError::Claimed { field: "body", value: 1 },
        ]
    }

    #[test]
    fn each_variant_names_its_field_and_value() {
        for e in every_variant() {
            let text = Buf::of(e);
            let text = text.text();
            match e {
                EncodeError::Overflow => assert!(text.contains("no room"), "{text}"),
                EncodeError::Unfit { field, .. }
                | EncodeError::OutOfRange { field, .. }
                | EncodeError::OverCap { field, .. }
                | EncodeError::Inexact { field, .. }
                | EncodeError::Mismatch { field, .. }
                | EncodeError::Delimiter { field, .. }
                | EncodeError::Empty { field }
                | EncodeError::Claimed { field, .. } => {
                    assert!(text.contains(field), "{text} does not name {field}");
                }
            }
        }
    }

    #[test]
    fn naming_moves_every_refusal_to_the_holder_and_leaves_overflow() {
        for e in every_variant() {
            let named = e.named("holder");
            match named {
                EncodeError::Overflow => assert_eq!(e, EncodeError::Overflow),
                _ => assert!(Buf::of(named).text().contains("`holder`"), "{named:?}"),
            }
        }
    }

    #[test]
    fn the_checks_pass_at_their_bounds_and_refuse_past_them() {
        assert_eq!(EncodeError::fit::<u8>("n", 255, 0), Ok(()));
        assert_eq!(
            EncodeError::fit::<u8>("n", 256, 0),
            Err(EncodeError::Unfit { field: "n", value: 256, bits: 0 })
        );
        assert_eq!(EncodeError::fit::<i8>("n", -8, 4), Ok(()));
        assert!(EncodeError::fit::<i8>("n", 8, 4).is_err());
        assert_eq!(EncodeError::range("h", 23, 0, 23), Ok(()));
        assert_eq!(
            EncodeError::range("h", 24, 0, 23),
            Err(EncodeError::OutOfRange { field: "h", value: 24, lo: 0, hi: 23 })
        );
        assert_eq!(EncodeError::cap("v", 8, 8), Ok(()));
        assert_eq!(
            EncodeError::cap("v", 9, 8),
            Err(EncodeError::OverCap { field: "v", len: 9, cap: 8 })
        );
        assert_eq!(EncodeError::agree("p", 4, 4), Ok(()));
        assert_eq!(
            EncodeError::agree("p", 3, 4),
            Err(EncodeError::Mismatch { field: "p", value: 3, stated: 4 })
        );
    }

    #[test]
    fn the_constant_picks_the_signature() {
        let plain: Encoded<[u8; 2], false> = [1, 2];
        let checked: Encoded<[u8; 2], true> = Ok([1, 2]);
        assert_eq!(<Fallibility<false> as Encodes<[u8; 2]>>::result(plain), Ok([1, 2]));
        assert_eq!(<Fallibility<true> as Encodes<[u8; 2]>>::result(checked), Ok([1, 2]));
        assert_eq!(<Fallibility<false> as Encodes<u8>>::finish(Ok(3)), 3);
        let refused = Err(EncodeError::Empty { field: "x" });
        assert_eq!(<Fallibility<true> as Encodes<u8>>::finish(refused), refused);
        let full: Result<(), Buffered<false>> =
            <Fallibility<false> as Buffers>::finish(Err(EncodeError::Overflow));
        assert_eq!(full, Err(crate::Overflow));
        let refused = Err(EncodeError::Empty { field: "x" });
        let kept: Result<(), Buffered<true>> = <Fallibility<true> as Buffers>::finish(refused);
        assert_eq!(kept, refused);
    }
}
