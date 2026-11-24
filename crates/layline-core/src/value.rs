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

#[cfg(test)]
mod tests {
    use super::*;

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
