//! A bit container loaded from wire bytes.

/// `WIRE_BYTES` wire bytes loaded into a `u128`, with container bit `N` at bit `N`.
///
/// `DATA_BITS` must equal `WIRE_BYTES * 8`, checked at compile time.
/// Read and write fields with [`Extent`](crate::Extent) on [`raw`](Self::raw) and [`set_raw`](Self::set_raw).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BitWord<const DATA_BITS: u32, const WIRE_BYTES: usize> {
    bits: u128,
}

impl<const DATA_BITS: u32, const WIRE_BYTES: usize> BitWord<DATA_BITS, WIRE_BYTES> {
    /// Bits in the container.
    pub const DATA_BITS: u32 = DATA_BITS;
    /// Bytes on the wire.
    pub const WIRE_BYTES: usize = WIRE_BYTES;

    const MASK: u128 = {
        assert!(DATA_BITS >= 1 && DATA_BITS <= 128);
        assert!(
            DATA_BITS as usize == WIRE_BYTES * 8,
            "`DATA_BITS` must be `WIRE_BYTES * 8`. Declare any unused bits as fields"
        );
        crate::mask::mask128(DATA_BITS)
    };

    /// A container with every bit zero.
    #[must_use]
    pub const fn zeroed() -> Self {
        Self { bits: 0 }
    }

    /// Load from little-endian wire bytes.
    #[must_use]
    pub const fn from_wire(bytes: &[u8; WIRE_BYTES]) -> Self {
        let mut bits: u128 = 0;
        let mut i = 0;
        while i < WIRE_BYTES {
            bits |= (bytes[i] as u128) << (i * 8);
            i += 1;
        }
        Self { bits: bits & Self::MASK }
    }

    /// Store as little-endian wire bytes.
    #[must_use]
    pub const fn to_wire(&self) -> [u8; WIRE_BYTES] {
        let mut bytes = [0u8; WIRE_BYTES];
        let mut i = 0;
        while i < WIRE_BYTES {
            bytes[i] = (self.bits >> (i * 8)) as u8;
            i += 1;
        }
        bytes
    }

    /// Load from big-endian wire bytes.
    #[must_use]
    pub const fn from_wire_be(bytes: &[u8; WIRE_BYTES]) -> Self {
        let mut bits: u128 = 0;
        let mut i = 0;
        while i < WIRE_BYTES {
            bits |= (bytes[i] as u128) << ((WIRE_BYTES - 1 - i) * 8);
            i += 1;
        }
        Self { bits: bits & Self::MASK }
    }

    /// Store as big-endian wire bytes.
    #[must_use]
    pub const fn to_wire_be(&self) -> [u8; WIRE_BYTES] {
        let mut bytes = [0u8; WIRE_BYTES];
        let mut i = 0;
        while i < WIRE_BYTES {
            bytes[i] = (self.bits >> ((WIRE_BYTES - 1 - i) * 8)) as u8;
            i += 1;
        }
        bytes
    }

    /// The container bits.
    #[must_use]
    pub const fn raw(&self) -> u128 {
        self.bits
    }

    /// Replace the container bits, masked to its width.
    pub const fn set_raw(&mut self, bits: u128) {
        self.bits = bits & Self::MASK;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Wide = BitWord<72, 9>;

    #[test]
    fn a_full_width_field_extracts_all_sixty_four_bits() {
        let data = [0xFFu8; 9];
        let w = Wide::from_wire(&data);

        assert_eq!(crate::Extent::new(0, 64).extract(w.raw()), u64::MAX);
        assert_eq!(crate::Extent::new(0, 63).extract(w.raw()), u64::MAX >> 1);
        assert_eq!(crate::Extent::new(0, 1).extract(w.raw()), 1);
        assert_eq!(
            crate::Extent::new(6, 64).extract(w.raw()),
            u64::MAX,
            "still all ones, six bits along"
        );
    }

    #[test]
    fn a_wire_round_trip_is_the_identity() {
        let mut data = [0u8; 9];
        data[0] = 0xA5;
        data[8] = 0xFF;
        assert_eq!(
            Wide::from_wire(&data).to_wire(),
            data,
            "every bit belongs to the payload, so nothing is dropped"
        );
    }

    #[test]
    fn a_byte_exact_container_has_no_spares() {
        type Two = BitWord<16, 2>;
        let w = Two::from_wire(&[0xFF, 0xFF]);
        assert_eq!(w.raw(), 0xFFFF);
        assert_eq!(w.to_wire(), [0xFF, 0xFF]);
    }
}
