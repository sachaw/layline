//! Field positions within a container.
use crate::mask::mask128;

/// A field's bit offset and width, numbered LSB0.
///
/// Bit `N` of the container is bit `N` of the `u128` the accessors take.
/// Ordered by `(start, width)`, which is wire order in a valid field table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Extent {
    start: u64,
    width: u32,
}

impl Extent {
    /// `width` bits starting at bit `start`.
    ///
    /// # Panics
    ///
    /// If `width` is 0.
    #[must_use]
    pub const fn new(start: u64, width: u32) -> Self {
        assert!(width >= 1, "extent width must be at least 1");
        Self { start, width }
    }

    /// The first bit.
    #[must_use]
    pub const fn start(&self) -> u64 {
        self.start
    }

    /// The width in bits, at least 1.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// One past the last bit.
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.start + self.width as u64
    }

    const fn mask_wide(&self) -> u128 {
        mask128(self.width)
    }

    /// Read the field from a container.
    #[inline]
    #[must_use]
    pub const fn extract_wide(&self, bits: u128) -> u128 {
        debug_assert!(self.end() <= 128, "the field lies outside a u128 container");
        (bits >> self.start) & self.mask_wide()
    }

    /// Write the field into a container and return the result.
    ///
    /// `value` is masked to the field's width.
    #[inline]
    #[must_use]
    pub const fn insert_wide(&self, bits: u128, value: u128) -> u128 {
        let mask = self.mask_wide();
        (bits & !(mask << self.start)) | ((value & mask) << self.start)
    }

    /// [`extract_wide`](Self::extract_wide) for fields up to 64 bits.
    #[inline]
    #[must_use]
    pub const fn extract(&self, bits: u128) -> u64 {
        self.extract_wide(bits) as u64
    }

    /// [`insert_wide`](Self::insert_wide) for fields up to 64 bits.
    #[inline]
    #[must_use]
    pub const fn insert(&self, bits: u128, value: u64) -> u128 {
        self.insert_wide(bits, value as u128)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_and_insert_are_inverse() {
        let field = Extent::new(19, 19);
        let word = field.insert(0, 0x5_A5A5);
        assert_eq!(field.extract(word), 0x5_A5A5);
        assert_eq!(word & !((crate::mask::mask64(field.width()) as u128) << 19), 0);
    }

    #[test]
    fn insert_masks_oversized_values() {
        let field = Extent::new(4, 4);
        assert_eq!(field.insert(0, 0xFF), 0xF0);
    }

    #[test]
    fn a_full_width_64_bit_field_works() {
        let field = Extent::new(6, 64);
        let word = field.insert(0, u64::MAX);
        assert_eq!(field.extract(word), u64::MAX);
    }

    #[test]
    fn adjacent_fields_do_not_interfere() {
        let a = Extent::new(0, 13);
        let b = Extent::new(13, 7);
        let mut word = 0u128;
        word = a.insert(word, 0x1FFF);
        word = b.insert(word, 0);
        assert_eq!(a.extract(word), 0x1FFF);
        assert_eq!(b.extract(word), 0);
    }
}
