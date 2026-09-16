//! Low-bit masks.

/// The low `bits` bits set, saturating at 64.
pub(crate) const fn mask64(bits: u32) -> u64 {
    if bits >= 64 { u64::MAX } else { (1u64 << bits) - 1 }
}

/// [`mask64`] for `u128`.
pub(crate) const fn mask128(bits: u32) -> u128 {
    if bits >= 128 { u128::MAX } else { (1u128 << bits) - 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mask_covers_its_own_width_and_no_more() {
        assert_eq!(mask64(0), 0);
        assert_eq!(mask64(1), 1);
        assert_eq!(mask64(8), 0xFF);
        assert_eq!(mask64(63), u64::MAX >> 1);
        assert_eq!(mask64(64), u64::MAX);
        assert_eq!(mask64(65), u64::MAX, "saturates");

        assert_eq!(mask128(0), 0);
        assert_eq!(mask128(64), u64::MAX as u128);
        assert_eq!(mask128(128), u128::MAX);
        assert_eq!(mask128(129), u128::MAX);
    }

    #[test]
    fn every_width_is_the_count_of_bits_it_names() {
        for bits in 0..=64u32 {
            assert_eq!(mask64(bits).count_ones(), bits, "mask64({bits})");
        }
        for bits in 0..=128u32 {
            assert_eq!(mask128(bits).count_ones(), bits, "mask128({bits})");
        }
    }
}
