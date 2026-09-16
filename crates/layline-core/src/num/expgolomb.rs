//! Exp-Golomb coding.
use crate::bits::{BitCodec, BitWriter};
use crate::{ParseError, WireInt};

/// Unsigned Exp-Golomb, H.264 §9.1 `ue(v)`.
///
/// A code word is `n` zero bits, a one bit, then `n` value bits. The value is `2^n - 1` plus
/// those bits. It reads the top of each byte first, so it implements only `BitCodec<true>`.
///
/// ```
/// use layline_core::BitCodec;
/// use layline_core::num::ExpGolomb;
///
/// // H.264 Table 9-1: codeNum 0 is `1`, 1 is `010`, 2 is `011`, 3 is `00100`.
/// assert_eq!(ExpGolomb::decode(&[0b1000_0000], 0), Ok((ExpGolomb(0), 1)));
/// assert_eq!(ExpGolomb::decode(&[0b0100_0000], 0), Ok((ExpGolomb(1), 3)));
/// assert_eq!(ExpGolomb::decode(&[0b0010_0000], 0), Ok((ExpGolomb(3), 5)));
/// ```
///
/// H.264 `se(v)` is `-unzigzag(codeNum)`, using [`zigzag`](super::zigzag) with the sign flipped:
///
/// ```
/// use layline_core::num::{ExpGolomb, unzigzag, zigzag};
/// use layline_core::{BitCodec, BitWriter, Buffer, Overflow, ParseError};
///
/// #[derive(Debug, PartialEq)]
/// struct Se(i64);
///
/// impl BitCodec<true> for Se {
///     fn decode(bytes: &[u8], at_bit: usize) -> Result<(Self, usize), ParseError> {
///         let (code, used) = ExpGolomb::decode(bytes, at_bit)?;
///         Ok((Se(-unzigzag(code.0)), used))
///     }
///
///     fn encode<B: Buffer>(&self, out: &mut BitWriter<'_, B, true>) -> Result<(), Overflow> {
///         ExpGolomb(zigzag(-self.0)).encode(out)
///     }
/// }
///
/// // H.264 Table 9-3: code numbers 0, 1, 2 are the values 0, 1, -1.
/// assert_eq!(Se::decode(&[0b1000_0000], 0), Ok((Se(0), 1)));
/// assert_eq!(Se::decode(&[0b0100_0000], 0), Ok((Se(1), 3)));
/// assert_eq!(Se::decode(&[0b0110_0000], 0), Ok((Se(-1), 3)));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExpGolomb(pub u64);

impl From<u64> for ExpGolomb {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<ExpGolomb> for u64 {
    fn from(v: ExpGolomb) -> Self {
        v.0
    }
}

impl ExpGolomb {
    /// The number of leading zero bits, and the bits after the one bit.
    const fn split(self) -> (u32, u64) {
        if self.0 == u64::MAX {
            return (64, 0);
        }
        let zeros = (self.0 + 1).ilog2();
        (zeros, self.0 + 1 - (1u64 << zeros))
    }
}

impl BitCodec<true> for ExpGolomb {
    fn decode(bytes: &[u8], at_bit: usize) -> Result<(Self, usize), ParseError> {
        let mut r = crate::BitReader::<true>::at(bytes, at_bit);
        let mut zeros = 0u32;
        while !r.bit()? {
            zeros += 1;
            if zeros > 64 {
                return Err(ParseError::Malformed { field: "ExpGolomb", at: at_bit / 8 });
            }
        }
        let suffix = if zeros == 0 { 0 } else { r.read(zeros)? };
        let base = crate::mask::mask64(zeros);
        let value = base
            .checked_add(suffix)
            .ok_or(ParseError::Malformed { field: "ExpGolomb", at: at_bit / 8 })?;
        Ok((Self(value), r.position() - at_bit))
    }

    fn encode<B: crate::Buffer>(
        &self,
        out: &mut BitWriter<'_, B, true>,
    ) -> Result<(), crate::Overflow> {
        let (zeros, suffix) = self.split();
        out.write(0, zeros)?;
        out.bit(true)?;
        out.write(suffix, zeros)
    }
}

impl WireInt for ExpGolomb {
    fn to_i64(&self) -> i64 {
        self.0.to_i64()
    }

    fn from_i64(n: i64) -> Self {
        Self(n as u64)
    }
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// The code word for `value`, as a string of bits.
    fn word<T: BitCodec<true>>(value: &T) -> alloc::string::String {
        let mut bytes = Vec::new();
        let mut w = BitWriter::<_, true>::new(&mut bytes);
        value.encode(&mut w).expect("a vector has room");
        let bits = w.position();
        (0..bits).map(|i| if bytes[i / 8] >> (7 - i % 8) & 1 == 1 { '1' } else { '0' }).collect()
    }

    /// Decode `word`, with ones after it to catch a decode that reads too far.
    fn read<T: BitCodec<true> + core::fmt::Debug>(word: &str) -> (T, usize) {
        let mut bytes = alloc::vec![0xFFu8; word.len().div_ceil(8) + 8];
        for (i, c) in word.chars().enumerate() {
            let bit = 0x80u8 >> (i % 8);
            if c == '0' {
                bytes[i / 8] &= !bit;
            } else {
                bytes[i / 8] |= bit;
            }
        }
        T::decode(&bytes, 0).expect("the table's code word decodes")
    }

    /// ITU-T H.264 Table 9-1: the first ten `ue(v)` code words.
    const UE: [&str; 10] =
        ["1", "010", "011", "00100", "00101", "00110", "00111", "0001000", "0001001", "0001010"];

    /// ITU-T H.264 Table 9-3: the value each of the first seven code numbers has as `se(v)`.
    const SE: [i64; 7] = [0, 1, -1, 2, -2, 3, -3];

    #[test]
    fn the_published_unsigned_table_is_the_code_word_both_ways() {
        for (n, bits) in UE.iter().enumerate() {
            let value = ExpGolomb(n as u64);
            assert_eq!(&word(&value), bits, "code number {n}");
            assert_eq!(read::<ExpGolomb>(bits), (value, bits.len()));
        }
    }

    #[test]
    fn the_published_signed_table_is_the_unsigned_code_word_for_its_code_number() {
        for (n, value) in SE.iter().enumerate() {
            assert_eq!(crate::num::zigzag(-*value), n as u64, "value {value}");
            assert_eq!(-crate::num::unzigzag(n as u64), *value, "code number {n}");
            assert_eq!(&word(&ExpGolomb(n as u64)), UE[n], "value {value}");
        }
    }

    #[test]
    fn every_value_to_a_thousand_round_trips_in_both_directions() {
        for n in 0..=1000u64 {
            let value = ExpGolomb(n);
            let bits = word(&value);
            assert_eq!(bits.len(), 2 * (64 - (n + 1).leading_zeros() as usize) - 1);
            assert_eq!(read::<ExpGolomb>(&bits), (value, bits.len()));
        }
        for v in -1000..=1000i64 {
            let code = crate::num::zigzag(-v);
            let bits = word(&ExpGolomb(code));
            assert_eq!(read::<ExpGolomb>(&bits), (ExpGolomb(code), bits.len()));
            assert_eq!(-crate::num::unzigzag(code), v, "the bijection closes");
        }
    }

    #[test]
    fn the_widest_code_number_is_sixty_four_zero_bits_and_the_bits_after_it() {
        let bits = word(&ExpGolomb(u64::MAX));
        assert_eq!(bits.len(), 129);
        assert!(bits[..64].chars().all(|c| c == '0') && &bits[64..65] == "1");
        assert_eq!(read::<ExpGolomb>(&bits), (ExpGolomb(u64::MAX), 129));

        let one_less = word(&ExpGolomb(u64::MAX - 1));
        assert_eq!(one_less.len(), 127);
        assert_eq!(read::<ExpGolomb>(&one_less), (ExpGolomb(u64::MAX - 1), 127));
    }

    #[test]
    fn a_code_word_of_more_than_sixty_five_zero_bits_is_malformed() {
        let bytes = [0u8; 16];
        assert_eq!(
            ExpGolomb::decode(&bytes, 0),
            Err(ParseError::Malformed { field: "ExpGolomb", at: 0 })
        );
    }

    #[test]
    fn a_code_word_the_bytes_end_inside_is_short() {
        // Eight zero bits, and the ninth is past the end.
        assert_eq!(
            ExpGolomb::decode(&[0u8], 0),
            Err(ParseError::Short { need_bytes: 2, got_bytes: 1, at: 1 })
        );
        assert_eq!(
            ExpGolomb::decode(&[], 0),
            Err(ParseError::Short { need_bytes: 1, got_bytes: 0, at: 0 })
        );
    }

    #[test]
    fn a_value_at_every_start_offset_reads_back_what_it_wrote() {
        let values: Vec<u64> = alloc::vec![0, 1, 2, 7, 8, 255, 1023, 65_535, u32::MAX as u64];
        for offset in 0..=7u32 {
            for v in &values {
                let mut bytes = Vec::new();
                let (at, width) = {
                    let mut w = BitWriter::<_, true>::new(&mut bytes);
                    w.write(0, offset).unwrap();
                    let at = w.position();
                    ExpGolomb(*v).encode(&mut w).unwrap();
                    let width = w.position() - at;
                    w.write(0, 7).unwrap();
                    (at, width)
                };
                assert_eq!(
                    ExpGolomb::decode(&bytes, at),
                    Ok((ExpGolomb(*v), width)),
                    "offset={offset} value={v}"
                );
            }
        }
    }
}
