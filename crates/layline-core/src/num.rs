//! Number codings: reserved values, LEB128 and Exp-Golomb.
//!
//! # Tags
//! [`Reserved`] takes an optional `Tag` type, `()` by default. Two fields with different tags
//! have different types, even with the same coding.
//!
//! ```
//! use layline_core::U;
//! use layline_core::num::Reserved;
//!
//! // Both are nine bits with 511 reserved. The tags stop one being used as the other.
//! pub struct CourseDegrees;
//! pub struct BearingDegrees;
//! type Course = Reserved<U<9>, 511, CourseDegrees>;
//! type Bearing = Reserved<U<9>, 511, BearingDegrees>;
//!
//! assert_eq!(Course::new(U::new(180).unwrap()).get(), U::new(180));
//! ```
use core::marker::PhantomData;

use crate::FieldCodec;

mod expgolomb;
mod varint;

pub use expgolomb::ExpGolomb;
pub use varint::{Sleb128, Uleb128, unzigzag, zigzag};

/// A field codec with one raw value reserved to mean `None`.
///
/// It stores the raw bits, so every bit pattern round-trips. `Tag` is a [tag](self#tags).
///
/// ```
/// use layline_core::num::Reserved;
/// use layline_core::{FieldCodec, I, U};
///
/// // 0xFF means no value.
/// type Signal = Reserved<U<8>, 0xFF>;
/// assert_eq!(Signal::from_raw(40).get(), U::new(40));
/// assert_eq!(Signal::from_raw(0xFF).get(), None);
///
/// // A signed 21-bit count with its most negative value reserved.
/// type Steps = Reserved<I<21>, { 1 << 20 }>;
/// assert_eq!(Steps::from_raw(0x1F_FFFF).get(), I::new(-1));
/// assert_eq!(Steps::none().get(), None);
///
/// // A float bit pattern reserved. The comparison is on bits, so a NaN works too.
/// type Height = Reserved<f32, { (-1e9f32).to_bits() as u64 }>;
/// assert_eq!(Height::new(120.5).get(), Some(120.5));
/// assert_eq!(Height::new(-1e9).get(), None);
/// ```
///
/// A `RAW` wider than the codec does not compile:
///
/// ```compile_fail
/// use layline_core::U;
/// use layline_core::num::Reserved;
///
/// let _ = Reserved::<U<8>, 0x100>::none();
/// ```
pub struct Reserved<C, const RAW: u64, Tag = ()>(u64, PhantomData<fn() -> (C, Tag)>);

impl<C: FieldCodec, const RAW: u64, Tag> Reserved<C, RAW, Tag> {
    const RESERVED: u64 = {
        assert!(
            RAW & !crate::mask::mask64(C::BITS) == 0,
            "a Reserved's RAW is wider than its codec"
        );
        RAW
    };

    /// The reserved value. [`get`](Self::get) returns `None`.
    #[must_use]
    pub const fn none() -> Self {
        Self(Self::RESERVED, PhantomData)
    }

    /// Wrap a value. A value whose raw bits equal `RAW` reads back as `None`.
    #[must_use]
    pub fn new(value: C) -> Self {
        Self(value.to_raw(), PhantomData)
    }

    /// The value, or `None` if the raw bits are `RAW`.
    #[must_use]
    pub fn get(&self) -> Option<C> {
        if self.0 == Self::RESERVED { None } else { Some(C::from_raw(self.0)) }
    }

    /// The raw bits.
    #[must_use]
    pub const fn raw(&self) -> u64 {
        self.0
    }
}

impl<C: FieldCodec, const RAW: u64, Tag> FieldCodec for Reserved<C, RAW, Tag> {
    const BITS: u32 = {
        let _ = Self::RESERVED;
        C::BITS
    };

    fn from_raw(raw: u64) -> Self {
        Self(raw & crate::mask::mask64(C::BITS), PhantomData)
    }

    fn to_raw(&self) -> u64 {
        self.0
    }
}

// Not derived: a derive would bound `C` and `Tag` too.
impl<C, const RAW: u64, Tag> Clone for Reserved<C, RAW, Tag> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C, const RAW: u64, Tag> Copy for Reserved<C, RAW, Tag> {}

impl<C, const RAW: u64, Tag> PartialEq for Reserved<C, RAW, Tag> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<C, const RAW: u64, Tag> Eq for Reserved<C, RAW, Tag> {}

impl<C, const RAW: u64, Tag> core::hash::Hash for Reserved<C, RAW, Tag> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl<C, const RAW: u64, Tag> core::fmt::Debug for Reserved<C, RAW, Tag> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("Reserved").field(&self.0).finish()
    }
}

impl<C: FieldCodec, const RAW: u64, Tag> Default for Reserved<C, RAW, Tag> {
    fn default() -> Self {
        Self::none()
    }
}

/// Serializes as [`get`](Reserved::get) would return.
#[cfg(feature = "serde")]
impl<C: FieldCodec + serde_core::Serialize, const RAW: u64, Tag> serde_core::Serialize
    for Reserved<C, RAW, Tag>
{
    fn serialize<S: serde_core::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.get().serialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::Reserved;
    use crate::{FieldCodec, I, U};

    struct Lat;

    #[test]
    fn every_raw_round_trips_whatever_it_means() {
        type Signal = Reserved<U<8>, 0xFF>;
        for raw in 0..=0xFFu64 {
            assert_eq!(Signal::from_raw(raw).to_raw(), raw);
        }
        assert_eq!(Signal::from_raw(0xFF).get(), None);
        assert_eq!(Signal::none().to_raw(), 0xFF);
        assert_eq!(Signal::default(), Signal::none());
        assert_eq!(<Signal as FieldCodec>::BITS, 8);
    }

    #[test]
    fn a_signed_codec_can_reserve_its_most_negative_value() {
        type Steps = Reserved<I<21>, { 1 << 20 }, Lat>;
        assert_eq!(Steps::from_raw(0).get(), I::new(0));
        assert_eq!(Steps::from_raw((1 << 20) + 1).get(), I::new(-(1 << 20) + 1));
        assert_eq!(Steps::from_raw((1 << 21) - 1).get(), I::new(-1));
        assert_eq!(Steps::none().get(), None);
        assert_eq!(Steps::new(I::new(-1).unwrap()).to_raw(), (1 << 21) - 1);
    }

    #[test]
    fn a_float_codec_tests_the_pattern_and_not_the_value() {
        type Nan = Reserved<f32, { f32::NAN.to_bits() as u64 }>;
        assert_eq!(Nan::none().get(), None);
        assert!(Nan::new(-f32::NAN).get().is_some_and(f32::is_nan), "another NaN is a value");

        type Zero = Reserved<f64, { 0.0f64.to_bits() }>;
        assert_eq!(Zero::new(0.0).get(), None, "positive zero is reserved");
        assert!(Zero::new(-0.0).get().is_some(), "negative zero is a value");
    }

    #[test]
    fn the_raw_is_masked_to_the_codec_width() {
        type Speed = Reserved<U<11>, 2047>;
        assert_eq!(Speed::from_raw(0xFFFF).to_raw(), 2047);
        assert_eq!(<Speed as FieldCodec>::BITS, 11);
    }

    #[test]
    fn a_brand_costs_the_branded_type_nothing() {
        fn plain<T: Copy + Eq + core::hash::Hash + core::fmt::Debug + Default + Send + Sync>() {}
        plain::<Reserved<U<11>, 2047, Lat>>();
    }
}
