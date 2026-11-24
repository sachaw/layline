//! A number the wire states about other fields, which encode writes.

use core::cmp::Ordering;
use core::fmt;
use core::hash::{Hash, Hasher};

/// A count, length, offset, selector or flag that the wire states about other fields, away from
/// what it describes.
///
/// It carries no information of its own, so it compares equal to every other `Stated<T>`, hashes
/// to nothing, and its [`Default`] is as good as any value. Decode reads it, and [`get`] gives
/// what the wire said. Encode writes it from the fields it describes and never reads it.
///
/// A message field of this type must be written by an attribute: an invertible expression that
/// reads it, such as `#[count(n)]`, a presence test such as `#[when(flags & 0x01)]`, or a
/// `#[switch(kind)]`. Otherwise the type is refused.
///
/// ```
/// use layline_core::Stated;
///
/// let (decoded, built) = (Stated::new(2u8), Stated::<u8>::default());
/// assert_eq!(decoded, built);
/// assert_eq!((decoded.get(), built.get()), (2, 0));
/// ```
///
/// [`get`]: Self::get
#[derive(Clone, Copy, Default)]
pub struct Stated<T>(T);

impl<T> Stated<T> {
    /// `value`, as decode reads it.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// The number decode read, or the one `new` or `Default` gave.
    #[must_use]
    pub fn get(self) -> T {
        self.0
    }

    /// A reference to [`get`](Self::get)'s number.
    #[must_use]
    pub const fn as_ref(&self) -> &T {
        &self.0
    }
}

impl<T> From<T> for Stated<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T: fmt::Debug> fmt::Debug for Stated<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Stated({:?})", self.0)
    }
}

impl<T> PartialEq for Stated<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl<T> Eq for Stated<T> {}

impl<T> PartialOrd for Stated<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Stated<T> {
    fn cmp(&self, _: &Self) -> Ordering {
        Ordering::Equal
    }
}

impl<T> Hash for Stated<T> {
    fn hash<H: Hasher>(&self, _: &mut H) {}
}

#[cfg(feature = "serde")]
impl<T: serde_core::Serialize> serde_core::Serialize for Stated<T> {
    fn serialize<S: serde_core::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_stated_value_is_equal_and_hashes_alike() {
        use core::hash::BuildHasher;
        struct Sum(u64);
        impl Hasher for Sum {
            fn finish(&self) -> u64 {
                self.0
            }
            fn write(&mut self, bytes: &[u8]) {
                self.0 += bytes.len() as u64 + 1;
            }
        }
        struct Build;
        impl BuildHasher for Build {
            type Hasher = Sum;
            fn build_hasher(&self) -> Sum {
                Sum(0)
            }
        }
        let (a, b) = (Stated::new(3u8), Stated::<u8>::default());
        assert_eq!(a, b);
        assert_eq!(a.cmp(&b), Ordering::Equal);
        assert_eq!(Build.hash_one(a), Build.hash_one(b));
        assert_eq!((a.get(), b.get(), *a.as_ref()), (3, 0, 3));
    }
}
