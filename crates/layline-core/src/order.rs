//! Byte order as a type, for a message that reads its order from its holder.
//!
//! A message declared `#[message(needs(big_endian: bool), endian = needs(big_endian))]` has one
//! decode and one encode, generic over [`ByteOrder`]. The parameter picks [`Be`] or [`Le`] once
//! per message, and each field then reads in that order with no test of its own. A message with a
//! fixed `endian` is the same code at one order.
//!
//! ```
//! use layline_core::order::{Be, ByteOrder, Le};
//!
//! fn word<O: ByteOrder>(bytes: [u8; 4]) -> u32 {
//!     O::read(bytes)
//! }
//!
//! assert_eq!(word::<Be>([0, 0, 1, 2]), 0x0102);
//! assert_eq!(word::<Le>([2, 1, 0, 0]), 0x0102);
//! ```

/// A byte order: [`Be`] or [`Le`]. Sealed.
pub trait ByteOrder:
    crate::sealed::ByteOrder + Copy + Default + core::fmt::Debug + 'static
{
    /// Whether the most significant byte comes first.
    const BIG: bool;

    /// The value whose wire bytes, in this order, are `bytes`.
    fn read<T: Ordered>(bytes: T::Bytes) -> T;

    /// The wire bytes of `value`, in this order.
    fn write<T: Ordered>(value: T) -> T::Bytes;
}

/// Big-endian: the most significant byte first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Be;

/// Little-endian: the least significant byte first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Le;

impl crate::sealed::ByteOrder for Be {}
impl crate::sealed::ByteOrder for Le {}

impl ByteOrder for Be {
    const BIG: bool = true;

    #[inline]
    fn read<T: Ordered>(bytes: T::Bytes) -> T {
        T::from_be(bytes)
    }

    #[inline]
    fn write<T: Ordered>(value: T) -> T::Bytes {
        value.to_be()
    }
}

impl ByteOrder for Le {
    const BIG: bool = false;

    #[inline]
    fn read<T: Ordered>(bytes: T::Bytes) -> T {
        T::from_le(bytes)
    }

    #[inline]
    fn write<T: Ordered>(value: T) -> T::Bytes {
        value.to_le()
    }
}

/// A scalar with a byte order: the integers and floats. Sealed.
pub trait Ordered: crate::sealed::Ordered + Copy {
    /// The wire bytes, `[u8; size_of::<Self>()]`.
    type Bytes;

    /// From big-endian bytes.
    fn from_be(bytes: Self::Bytes) -> Self;

    /// From little-endian bytes.
    fn from_le(bytes: Self::Bytes) -> Self;

    /// To big-endian bytes.
    fn to_be(self) -> Self::Bytes;

    /// To little-endian bytes.
    fn to_le(self) -> Self::Bytes;
}

macro_rules! ordered {
    ($($t:ty),*) => {$(
        impl crate::sealed::Ordered for $t {}

        impl Ordered for $t {
            type Bytes = [u8; core::mem::size_of::<$t>()];

            #[inline]
            fn from_be(bytes: Self::Bytes) -> Self {
                <$t>::from_be_bytes(bytes)
            }

            #[inline]
            fn from_le(bytes: Self::Bytes) -> Self {
                <$t>::from_le_bytes(bytes)
            }

            #[inline]
            fn to_be(self) -> Self::Bytes {
                self.to_be_bytes()
            }

            #[inline]
            fn to_le(self) -> Self::Bytes {
                self.to_le_bytes()
            }
        }
    )*};
}

ordered!(u8, u16, u32, u64, u128, i8, i16, i32, i64, i128, f32, f64);

#[cfg(test)]
mod tests {
    use super::{Be, ByteOrder, Le};

    #[test]
    fn each_order_round_trips_every_width() {
        fn both<O: ByteOrder>() {
            assert_eq!(O::read::<u16>(O::write(0x1234u16)), 0x1234);
            assert_eq!(O::read::<i64>(O::write(-5i64)), -5);
            assert_eq!(O::read::<f64>(O::write(1.5f64)), 1.5);
            assert_eq!(O::read::<u128>(O::write(u128::MAX - 1)), u128::MAX - 1);
        }
        both::<Be>();
        both::<Le>();
    }

    #[test]
    fn the_orders_are_mirror_images() {
        assert_eq!(Be::write(0x0102_0304u32), [1, 2, 3, 4]);
        assert_eq!(Le::write(0x0102_0304u32), [4, 3, 2, 1]);
        assert_eq!(Be::read::<i16>([0xFF, 0xFE]), -2);
        assert_eq!(Le::read::<i16>([0xFE, 0xFF]), -2);
        const { assert!(Be::BIG && !Le::BIG) };
    }
}
