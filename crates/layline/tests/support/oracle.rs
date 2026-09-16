//! An independent bit-extraction reference for the differential tests, in an idiom unlike `Extent`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordBits {
    bits: u128,
}

impl WordBits {
    pub fn from_container(data: &[u8; 9]) -> Self {
        let mut bits: u128 = 0;
        for (i, &byte) in data.iter().enumerate() {
            bits |= (byte as u128) << (8 * i);
        }
        WordBits { bits }
    }

    /// The mask is `u128`, wide enough for a width of 64.
    pub fn field(&self, start: u8, count: u8) -> u64 {
        assert!(start + count <= 72, "field exceeds the 72-bit container");
        assert!(count <= 64, "field exceeds u64 capacity");
        let mask: u128 = (1u128 << count) - 1;
        ((self.bits >> start) & mask) as u64
    }
}
