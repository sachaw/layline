//! The arithmetic of attribute expressions, total over `i64`.
//!
//! Each operation takes and returns an `Option`: `None` is a number the expression does not have,
//! because an operation overflowed, divided by zero or shifted by a count outside `0..64`. Decode
//! refuses such bytes and encode refuses such a value, so no expression wraps silently.

/// `a + b`.
#[must_use]
pub fn add(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    a?.checked_add(b?)
}

/// `a - b`.
#[must_use]
pub fn sub(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    a?.checked_sub(b?)
}

/// `a * b`.
#[must_use]
pub fn mul(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    a?.checked_mul(b?)
}

/// `a / b`, rounded toward zero.
#[must_use]
pub fn div(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    a?.checked_div(b?)
}

/// `a % b`, with the sign of `a`.
#[must_use]
pub fn rem(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    a?.checked_rem(b?)
}

/// `-a`.
#[must_use]
pub fn neg(a: Option<i64>) -> Option<i64> {
    a?.checked_neg()
}

/// `a & b`, on the two's-complement bits.
#[must_use]
pub fn bit_and(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    Some(a? & b?)
}

/// `a | b`, on the two's-complement bits.
#[must_use]
pub fn bit_or(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    Some(a? | b?)
}

/// `a ^ b`, on the two's-complement bits.
#[must_use]
pub fn bit_xor(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    Some(a? ^ b?)
}

/// `a << b`: `a * 2^b`, so a bit shifted out is an overflow.
#[must_use]
pub fn shl(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    let (a, b) = (a?, b?);
    if !(0..64).contains(&b) {
        return None;
    }
    i64::try_from(i128::from(a) << b).ok()
}

/// `a >> b`: `a / 2^b`, rounded down.
#[must_use]
pub fn shr(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    let (a, b) = (a?, b?);
    if !(0..64).contains(&b) {
        return None;
    }
    Some(a >> b)
}

/// `a == b`.
#[must_use]
pub fn eq<T: PartialEq>(a: Option<T>, b: Option<T>) -> Option<bool> {
    Some(a? == b?)
}

/// `a != b`.
#[must_use]
pub fn ne<T: PartialEq>(a: Option<T>, b: Option<T>) -> Option<bool> {
    Some(a? != b?)
}

/// `a < b`.
#[must_use]
pub fn lt(a: Option<i64>, b: Option<i64>) -> Option<bool> {
    Some(a? < b?)
}

/// `a <= b`.
#[must_use]
pub fn le(a: Option<i64>, b: Option<i64>) -> Option<bool> {
    Some(a? <= b?)
}

/// `a > b`.
#[must_use]
pub fn gt(a: Option<i64>, b: Option<i64>) -> Option<bool> {
    Some(a? > b?)
}

/// `a >= b`.
#[must_use]
pub fn ge(a: Option<i64>, b: Option<i64>) -> Option<bool> {
    Some(a? >= b?)
}

/// `a && b`. `b` is not evaluated when `a` is false, so it may be undefined there.
#[must_use]
pub fn both(a: Option<bool>, b: impl FnOnce() -> Option<bool>) -> Option<bool> {
    if a? { b() } else { Some(false) }
}

/// `a || b`. `b` is not evaluated when `a` is true, so it may be undefined there.
#[must_use]
pub fn either(a: Option<bool>, b: impl FnOnce() -> Option<bool>) -> Option<bool> {
    if a? { Some(true) } else { b() }
}

/// `!a`.
#[must_use]
pub fn not(a: Option<bool>) -> Option<bool> {
    Some(!a?)
}

/// `if c { a } else { b }`. Only the branch `c` takes is evaluated, so the other may be undefined.
#[must_use]
pub fn pick<T>(
    c: Option<bool>,
    a: impl FnOnce() -> Option<T>,
    b: impl FnOnce() -> Option<T>,
) -> Option<T> {
    if c? { a() } else { b() }
}

/// `a` as the integer type `T` a parameter takes, if it has a value and `T` holds it.
#[must_use]
pub fn narrow<T: crate::WireInt>(a: Option<i64>) -> Option<T> {
    let a = a?;
    crate::bound::fits_in_field::<T>(a, 0).then(|| T::from_i64(a))
}

/// A parameter's value as a number: an integer's own, or 1 for a `bool` that holds.
pub trait Number {
    /// The number.
    fn number(&self) -> i64;
}

impl<T: crate::WireInt> Number for T {
    fn number(&self) -> i64 {
        self.to_i64()
    }
}

impl Number for bool {
    fn number(&self) -> i64 {
        i64::from(*self)
    }
}

/// Whether bits are set: a bit test used as a condition.
#[must_use]
pub fn any_bit(a: Option<i64>) -> Option<bool> {
    Some(a? != 0)
}

/// A number encode works an expression back from: a length or count, or a discriminant.
pub trait Wide: Copy {
    /// The number as an `i64`. A length past `i64::MAX` cannot be in memory.
    fn wide(self) -> i64;
}

impl Wide for i64 {
    fn wide(self) -> i64 {
        self
    }
}

impl Wide for i32 {
    fn wide(self) -> i64 {
        self.into()
    }
}

impl Wide for usize {
    #[allow(clippy::cast_possible_wrap)]
    fn wide(self) -> i64 {
        self as i64
    }
}

/// A collection whose elements encode counts: none for an absent optional one.
pub trait Many {
    /// The number of elements.
    fn many(&self) -> usize;
}

impl<T> Many for [T] {
    fn many(&self) -> usize {
        self.len()
    }
}

impl<T, const N: usize> Many for [T; N] {
    fn many(&self) -> usize {
        N
    }
}

#[cfg(feature = "alloc")]
impl<T> Many for crate::__private::Vec<T> {
    fn many(&self) -> usize {
        self.len()
    }
}

#[cfg(feature = "alloc")]
impl<T> Many for crate::__private::Box<[T]> {
    fn many(&self) -> usize {
        self.len()
    }
}

impl<T: Many + ?Sized> Many for &T {
    fn many(&self) -> usize {
        (**self).many()
    }
}

impl<T: Many> Many for Option<T> {
    fn many(&self) -> usize {
        self.as_ref().map_or(0, Many::many)
    }
}

/// A number as a count or byte length: `None` when it is negative.
#[must_use]
pub fn size(a: Option<i64>) -> Option<usize> {
    usize::try_from(a?).ok()
}

/// Encode working `f * k` back: `t / k` when it divides exactly, else `None`.
#[must_use]
pub fn exactly(t: Option<i64>, k: Option<i64>) -> Option<i64> {
    let (t, k) = (t?, k?);
    if k == 0 || t.checked_rem(k)? != 0 { None } else { t.checked_div(k) }
}

/// Encode working `f << k` back: `t >> k` when no set bit is shifted out, else `None`.
#[must_use]
pub fn unshifted(t: Option<i64>, k: Option<i64>) -> Option<i64> {
    let (t, k) = (t?, k?);
    if !(0..64).contains(&k) {
        return None;
    }
    let back = t >> k;
    if shl(Some(back), Some(k)) == Some(t) { Some(back) } else { None }
}

/// Encode working `f * f` back: the root, when `t` is a perfect square, else `None`.
#[must_use]
pub fn root(t: Option<i64>) -> Option<i64> {
    let t = t?;
    if t < 0 {
        return None;
    }
    let r = t.isqrt();
    if r * r == t { Some(r) } else { None }
}

/// The arithmetic of real expressions: IEEE 754 `f64`, as Rust computes it.
///
/// An operation on reals always has a value: one too large is an infinity, and one with no
/// answer is NaN, as IEEE 754 gives them, and a comparison with NaN does not hold. A real becomes
/// an integer only through `as i64`, which has no value for NaN, an infinity, or a real outside
/// `i64`, so a count, length or offset never comes from one silently.
pub mod real {
    /// `a + b`.
    #[must_use]
    pub fn add(a: Option<f64>, b: Option<f64>) -> Option<f64> {
        Some(a? + b?)
    }

    /// `a - b`.
    #[must_use]
    pub fn sub(a: Option<f64>, b: Option<f64>) -> Option<f64> {
        Some(a? - b?)
    }

    /// `a * b`.
    #[must_use]
    pub fn mul(a: Option<f64>, b: Option<f64>) -> Option<f64> {
        Some(a? * b?)
    }

    /// `a / b`.
    #[must_use]
    pub fn div(a: Option<f64>, b: Option<f64>) -> Option<f64> {
        Some(a? / b?)
    }

    /// `a % b`, with the sign of `a`.
    #[must_use]
    pub fn rem(a: Option<f64>, b: Option<f64>) -> Option<f64> {
        Some(a? % b?)
    }

    /// `-a`.
    #[must_use]
    pub fn neg(a: Option<f64>) -> Option<f64> {
        Some(-a?)
    }

    /// `a < b`.
    #[must_use]
    pub fn lt(a: Option<f64>, b: Option<f64>) -> Option<bool> {
        Some(a? < b?)
    }

    /// `a <= b`.
    #[must_use]
    pub fn le(a: Option<f64>, b: Option<f64>) -> Option<bool> {
        Some(a? <= b?)
    }

    /// `a > b`.
    #[must_use]
    pub fn gt(a: Option<f64>, b: Option<f64>) -> Option<bool> {
        Some(a? > b?)
    }

    /// `a >= b`.
    #[must_use]
    pub fn ge(a: Option<f64>, b: Option<f64>) -> Option<bool> {
        Some(a? >= b?)
    }

    /// `a as i64`: rounded toward zero, and `None` for NaN, an infinity, or a real outside `i64`,
    /// where Rust's `as` would saturate.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn int(a: Option<f64>) -> Option<i64> {
        // -2^63 and 2^63, exactly.
        const LO: f64 = -9_223_372_036_854_775_808.0;
        const HI: f64 = 9_223_372_036_854_775_808.0;
        let a = a?;
        // NaN is in no range.
        (LO..HI).contains(&a).then_some(a as i64)
    }

    /// `a as f64`: the nearest real, as Rust's `as` rounds.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn of(a: Option<i64>) -> Option<f64> {
        Some(a? as f64)
    }

    /// A real field or parameter's value, as an `f64`.
    pub trait Real {
        /// The value. An `f32` widens exactly.
        fn real(&self) -> f64;
    }

    impl Real for f64 {
        fn real(&self) -> f64 {
            *self
        }
    }

    impl Real for f32 {
        fn real(&self) -> f64 {
            f64::from(*self)
        }
    }

    /// A real parameter's type, which a real expression is passed as.
    pub trait FromReal: Sized {
        /// `a` as this type, or `None` where it has no such value.
        fn from_real(a: f64) -> Option<Self>;
    }

    impl FromReal for f64 {
        fn from_real(a: f64) -> Option<Self> {
            Some(a)
        }
    }

    impl FromReal for f32 {
        /// The nearest `f32`, or `None` for a finite real past `f32`'s range, which would round
        /// to an infinity.
        #[allow(clippy::cast_possible_truncation)]
        fn from_real(a: f64) -> Option<Self> {
            let narrow = a as f32;
            if narrow.is_infinite() && a.is_finite() { None } else { Some(narrow) }
        }
    }

    /// `a` as the real type `T` a parameter takes, if it has a value and `T` holds it.
    #[must_use]
    pub fn narrow<T: FromReal>(a: Option<f64>) -> Option<T> {
        T::from_real(a?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_becomes_an_integer_only_where_it_has_one() {
        assert_eq!(real::int(Some(512.9)), Some(512));
        assert_eq!(real::int(Some(-512.9)), Some(-512));
        assert_eq!(real::int(Some(f64::NAN)), None);
        assert_eq!(real::int(Some(f64::INFINITY)), None);
        assert_eq!(real::int(Some(-9_223_372_036_854_775_808.0)), Some(i64::MIN));
        assert_eq!(real::int(Some(9_223_372_036_854_775_808.0)), None);
        assert_eq!(real::of(Some(276)), Some(276.0));
        assert_eq!(real::div(Some(1.0), Some(0.0)), Some(f64::INFINITY));
        assert_eq!(real::lt(Some(f64::NAN), Some(1.0)), Some(false));
        assert_eq!(real::rem(Some(-7.5), Some(2.0)), Some(-1.5));
        assert_eq!(real::narrow::<f32>(Some(1e300)), None);
        assert_eq!(real::narrow::<f32>(Some(f64::INFINITY)), Some(f32::INFINITY));
        assert_eq!(real::narrow::<f64>(Some(1e300)), Some(1e300));
    }

    #[test]
    fn arithmetic_refuses_what_it_cannot_hold() {
        assert_eq!(add(Some(i64::MAX), Some(1)), None);
        assert_eq!(mul(Some(1 << 62), Some(2)), None);
        assert_eq!(div(Some(7), Some(0)), None);
        assert_eq!(rem(Some(7), Some(0)), None);
        assert_eq!(div(Some(i64::MIN), Some(-1)), None);
        assert_eq!(neg(Some(i64::MIN)), None);
        assert_eq!(sub(Some(3), Some(5)), Some(-2));
    }

    #[test]
    fn a_shift_loses_no_bits_and_stays_in_range() {
        assert_eq!(shl(Some(1), Some(62)), Some(1 << 62));
        assert_eq!(shl(Some(1), Some(63)), None);
        assert_eq!(shl(Some(-1), Some(63)), Some(i64::MIN));
        assert_eq!(shl(Some(1), Some(64)), None);
        assert_eq!(shl(Some(1), Some(-1)), None);
        assert_eq!(shr(Some(-8), Some(1)), Some(-4));
        assert_eq!(shr(Some(8), Some(64)), None);
    }

    #[test]
    fn logic_short_circuits_past_an_undefined_side() {
        assert_eq!(both(Some(false), || None), Some(false));
        assert_eq!(either(Some(true), || None), Some(true));
        assert_eq!(both(Some(true), || None), None);
        assert_eq!(not(Some(true)), Some(false));
        assert_eq!(any_bit(Some(4)), Some(true));
    }

    #[test]
    fn working_back_is_exact_or_refused() {
        assert_eq!(exactly(Some(12), Some(4)), Some(3));
        assert_eq!(exactly(Some(13), Some(4)), None);
        assert_eq!(exactly(Some(13), Some(0)), None);
        assert_eq!(unshifted(Some(12), Some(2)), Some(3));
        assert_eq!(unshifted(Some(13), Some(2)), None);
        assert_eq!(root(Some(49)), Some(7));
        assert_eq!(root(Some(50)), None);
        assert_eq!(root(Some(-4)), None);
        assert_eq!(size(Some(-1)), None);
        assert_eq!(size(Some(3)), Some(3));
    }
}
