//! Bit cursors for reading and writing bit fields.
//!
//! `MSB = true` fills each byte from the top bit down. A field is at most 64 bits wide.

use crate::ParseError;

/// A value whose width in bits is read from the wire.
///
/// The bit-level [`VarCodec`](crate::VarCodec). A `#[var]` field in a `#[message(bits)]` uses one.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a `BitCodec` for this message's bit order",
    label = "wrong bit order, or not a `BitCodec` at all",
    note = "a coding that reads the top of each byte first implements `BitCodec<true>`, \
            and one that reads the bottom first implements `BitCodec<false>`. \
            Change the message's `order`, or use a coding for this order"
)]
pub trait BitCodec<const MSB: bool>: Sized {
    /// Decode from bit `at_bit` of `bytes`. Returns the value and the bits consumed.
    ///
    /// A successful decode must consume at least one bit.
    ///
    /// # Errors
    ///
    /// [`ParseError::Short`] if the bytes end inside the value, [`ParseError::Malformed`] if the
    /// value is too wide for `Self`.
    fn decode(bytes: &[u8], at_bit: usize) -> Result<(Self, usize), ParseError>;

    /// Write this value to the bit cursor.
    ///
    /// # Errors
    ///
    /// [`Overflow`](crate::Overflow) if the buffer has no room.
    fn encode<B: crate::Buffer>(
        &self,
        out: &mut BitWriter<'_, B, MSB>,
    ) -> Result<(), crate::Overflow>;
}

/// The low `n` bits set, for `n <= 8`. Avoids `mask64`'s branch in the inner loop.
const fn chunk_mask(n: u32) -> u64 {
    debug_assert!(n <= 8, "a chunk is at most one byte wide");
    (1u64 << n) - 1
}

/// A bit cursor that reads from a byte slice.
#[derive(Debug, Clone)]
pub struct BitReader<'a, const MSB: bool> {
    bytes: &'a [u8],
    /// Bits read so far.
    at: u64,
}

impl<'a, const MSB: bool> BitReader<'a, MSB> {
    /// A cursor at the first bit of `bytes`.
    ///
    /// `MSB` is `true` to read the top of each byte first.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// A cursor at bit `at_bit` of `bytes`. A position past the end reads as [`Short`](ParseError::Short).
    #[must_use]
    pub const fn at(bytes: &'a [u8], at_bit: usize) -> Self {
        Self { bytes, at: at_bit as u64 }
    }

    /// Read the next `bits` bits as an integer.
    ///
    /// # Errors
    ///
    /// [`ParseError::Short`] if the slice ends inside the field. `at` is the field's first byte.
    pub fn read(&mut self, bits: u32) -> Result<u64, ParseError> {
        debug_assert!(bits <= 64, "a bit field is at most 64 bits wide");
        let end = self.at.saturating_add(u64::from(bits));
        let need = usize::try_from(end.div_ceil(8)).unwrap_or(usize::MAX);
        if need > self.bytes.len() {
            return Err(ParseError::Short {
                need_bytes: need,
                got_bytes: self.bytes.len(),
                at: (self.at / 8) as usize,
            });
        }
        let mut value = 0u64;
        let mut left = bits;
        while left > 0 {
            let byte = u64::from(self.bytes[(self.at / 8) as usize]);
            let used = (self.at % 8) as u32;
            let free = 8 - used;
            let take = if left < free { left } else { free };
            if MSB {
                value = (value << take) | ((byte >> (free - take)) & chunk_mask(take));
            } else {
                value |= ((byte >> used) & chunk_mask(take)) << (bits - left);
            }
            self.at += u64::from(take);
            left -= take;
        }
        Ok(value)
    }

    /// Read the next bit.
    ///
    /// # Errors
    ///
    /// [`ParseError::Short`] if the slice ends first.
    pub fn bit(&mut self) -> Result<bool, ParseError> {
        Ok(self.read(1)? != 0)
    }

    /// Skip `bits` bits.
    ///
    /// # Errors
    ///
    /// [`ParseError::Short`] if the slice ends inside them. `at` is the first skipped byte.
    pub fn skip(&mut self, bits: usize) -> Result<(), ParseError> {
        let end = self.at.saturating_add(bits as u64);
        let need = usize::try_from(end.div_ceil(8)).unwrap_or(usize::MAX);
        if need > self.bytes.len() {
            return Err(ParseError::Short {
                need_bytes: need,
                got_bytes: self.bytes.len(),
                at: (self.at / 8) as usize,
            });
        }
        self.at = end;
        Ok(())
    }

    /// Bits read so far.
    #[must_use]
    pub const fn position(&self) -> usize {
        self.at as usize
    }

    /// Bytes touched so far, counting a partly read byte.
    #[must_use]
    pub const fn bytes_consumed(&self) -> usize {
        self.at.div_ceil(8) as usize
    }
}

/// A bit cursor that appends to a [`Buffer`](crate::Buffer).
///
/// Unused bits in the last byte are zero.
#[derive(Debug)]
pub struct BitWriter<'a, B, const MSB: bool> {
    out: &'a mut B,
    /// The buffer's length in bytes when the cursor was created.
    base: usize,
    /// Bits used in the last byte, `0..8`.
    used: u32,
}

impl<'a, B: crate::Buffer, const MSB: bool> BitWriter<'a, B, MSB> {
    /// A cursor at the end of `out`.
    ///
    /// `MSB` is `true` to fill the top of each byte first.
    #[must_use]
    pub fn new(out: &'a mut B) -> Self {
        let base = out.len();
        Self { out, base, used: 0 }
    }

    /// Append the low `bits` bits of `value`.
    ///
    /// # Errors
    ///
    /// [`Overflow`](crate::Overflow) if a new byte does not fit. The cursor and buffer are unchanged.
    pub fn write(&mut self, value: u64, bits: u32) -> Result<(), crate::Overflow> {
        let (len, used) = (self.out.len(), self.used);
        let result = self.append(value, bits);
        if result.is_err() {
            self.out.rewind(len);
            self.used = used;
            if let (1.., Some(last)) = (used, self.out.written_mut().last_mut()) {
                *last &= if MSB { 0xFF << (8 - used) } else { (1 << used) - 1 };
            }
        }
        result
    }

    fn append(&mut self, value: u64, bits: u32) -> Result<(), crate::Overflow> {
        debug_assert!(bits <= 64, "a bit field is at most 64 bits wide");
        let mut left = bits;
        while left > 0 {
            if self.used == 0 {
                self.out.push(&[0])?;
            }
            let free = 8 - self.used;
            let take = if left < free { left } else { free };
            let chunk = if MSB {
                (((value >> (left - take)) & chunk_mask(take)) as u8) << (free - take)
            } else {
                (((value >> (bits - left)) & chunk_mask(take)) as u8) << self.used
            };
            let last =
                self.out.written_mut().last_mut().expect("a byte was pushed for the free bits");
            *last |= chunk;
            self.used = (self.used + take) % 8;
            left -= take;
        }
        Ok(())
    }

    /// Append one bit.
    ///
    /// # Errors
    ///
    /// As [`write`](Self::write).
    pub fn bit(&mut self, set: bool) -> Result<(), crate::Overflow> {
        self.write(u64::from(set), 1)
    }

    /// Overwrite `bits` bits at `at_bit`. Those bits must already be written.
    pub fn patch(&mut self, at_bit: usize, bits: u32, value: u64) {
        debug_assert!(bits <= 64, "a bit field is at most 64 bits wide");
        debug_assert!(
            at_bit + bits as usize <= self.position(),
            "a patch lies inside the bits already written"
        );
        let base = self.base;
        let bytes = self.out.written_mut();
        let mut at = at_bit as u64;
        let mut left = bits;
        while left > 0 {
            let used = (at % 8) as u32;
            let free = 8 - used;
            let take = if left < free { left } else { free };
            let (chunk, hole) = if MSB {
                let shift = free - take;
                (
                    (((value >> (left - take)) & chunk_mask(take)) as u8) << shift,
                    (chunk_mask(take) as u8) << shift,
                )
            } else {
                (
                    (((value >> (bits - left)) & chunk_mask(take)) as u8) << used,
                    (chunk_mask(take) as u8) << used,
                )
            };
            let byte = &mut bytes[base + (at / 8) as usize];
            *byte = (*byte & !hole) | chunk;
            at += u64::from(take);
            left -= take;
        }
    }

    /// Bits written so far.
    #[must_use]
    pub fn position(&self) -> usize {
        (self.out.len() - self.base) * 8 - if self.used == 0 { 0 } else { 8 - self.used as usize }
    }
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use super::*;
    use crate::Buffer;
    use crate::mask::mask64;
    use alloc::vec::Vec;

    /// A mixed bit pattern, cut to `bits`.
    fn sample(bits: u32) -> u64 {
        let v = 0x9E37_79B9_7F4A_7C15u64;
        if bits == 64 { v } else { v & mask64(bits) }
    }

    /// The bytes written for a sequence of `(value, bits)` fields.
    fn written<const MSB: bool>(runs: &[(u64, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut w = BitWriter::<_, MSB>::new(&mut out);
        for (value, bits) in runs {
            w.write(*value, *bits).unwrap();
        }
        out
    }

    #[test]
    fn every_width_at_every_offset_round_trips_in_both_orders() {
        fn each<const MSB: bool>() {
            for offset in 0..=7u32 {
                for bits in 1..=64u32 {
                    let value = sample(bits);
                    let bytes = written::<MSB>(&[(mask64(offset), offset), (value, bits)]);
                    assert_eq!(
                        bytes.len(),
                        ((offset + bits) as usize).div_ceil(8),
                        "msb={MSB} offset={offset} bits={bits}"
                    );

                    let mut r = BitReader::<MSB>::new(&bytes);
                    if offset > 0 {
                        assert_eq!(r.read(offset).unwrap(), mask64(offset));
                    }
                    assert_eq!(
                        r.read(bits).unwrap(),
                        value,
                        "msb={MSB} offset={offset} bits={bits}"
                    );
                    assert_eq!(r.bytes_consumed(), bytes.len());
                }
            }
        }
        each::<false>();
        each::<true>();
    }

    #[test]
    fn a_nibble_pair_lands_at_the_end_of_the_byte_the_order_names() {
        assert_eq!(written::<true>(&[(0xA, 4), (0x5, 4)]), [0xA5]);
        assert_eq!(written::<false>(&[(0xA, 4), (0x5, 4)]), [0x5A]);
    }

    #[test]
    fn a_field_across_a_byte_boundary_keeps_its_order() {
        assert_eq!(written::<true>(&[(0b101, 3), (0b1_1000_0110, 9)]), [0b1011_1000, 0b0110_0000]);

        let mut r = BitReader::<true>::new(&[0b1011_1000, 0b0110_0000]);
        assert_eq!(r.read(3).unwrap(), 0b101);
        assert_eq!(r.read(9).unwrap(), 0b1_1000_0110);
    }

    #[test]
    fn the_pad_is_zero_and_the_reader_ignores_it() {
        let bytes = written::<true>(&[(0b1011_0011_0011, 13)]);
        assert_eq!(bytes.len(), 2);
        assert_eq!(bytes[1] & 0b111, 0, "the last three bits are the pad");

        let mut r = BitReader::<true>::new(&bytes);
        assert_eq!(r.read(13).unwrap(), 0b1011_0011_0011);
        assert_eq!(r.bytes_consumed(), 2);
    }

    #[test]
    fn a_bit_at_a_time_is_the_same_wire_as_a_field() {
        fn each<const MSB: bool>() {
            let mut out = Vec::new();
            let mut w = BitWriter::<_, MSB>::new(&mut out);
            for i in 0..12 {
                w.bit(i % 3 == 0).unwrap();
            }
            let mut r = BitReader::<MSB>::new(&out);
            for i in 0..12 {
                assert_eq!(r.bit().unwrap(), i % 3 == 0, "msb={MSB} bit={i}");
            }
        }
        each::<false>();
        each::<true>();
    }

    /// A test coding: three bits of width, then that many bits of value.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Widthed<const MSB: bool>(u64, u32);

    impl<const MSB: bool> BitCodec<MSB> for Widthed<MSB> {
        fn decode(bytes: &[u8], at_bit: usize) -> Result<(Self, usize), ParseError> {
            let mut r = BitReader::<MSB>::at(bytes, at_bit);
            let width = r.read(3)? as u32;
            let value = if width == 0 { 0 } else { r.read(width)? };
            Ok((Self(value, width), r.position() - at_bit))
        }

        fn encode<B: Buffer>(
            &self,
            out: &mut BitWriter<'_, B, MSB>,
        ) -> Result<(), crate::Overflow> {
            out.write(u64::from(self.1), 3)?;
            out.write(self.0, self.1)
        }
    }

    #[test]
    fn a_bit_codec_round_trips_at_every_start_offset_in_both_orders() {
        fn each<const MSB: bool>() {
            for offset in 0..=7u32 {
                for width in 0..=7u32 {
                    let value = Widthed::<MSB>(sample(width), width);
                    let mut out = Vec::new();
                    let mut w = BitWriter::<_, MSB>::new(&mut out);
                    w.write(mask64(offset), offset).unwrap();
                    let at = w.position();
                    value.encode(&mut w).unwrap();
                    let used = w.position() - at;
                    assert_eq!(used, 3 + width as usize, "offset={offset} width={width}");
                    w.write(0, 7).unwrap();
                    assert_eq!(
                        Widthed::<MSB>::decode(&out, at),
                        Ok((value, used)),
                        "offset={offset} width={width}"
                    );
                }
            }
        }
        each::<true>();
        each::<false>();
    }

    #[test]
    fn a_coding_reads_one_end_of_the_byte_and_says_so_in_its_type() {
        fn msb_only<T: BitCodec<true>>() {}
        fn lsb_only<T: BitCodec<false>>() {}
        msb_only::<Widthed<true>>();
        lsb_only::<Widthed<false>>();
        #[cfg(feature = "num")]
        msb_only::<crate::num::ExpGolomb>();
        // `lsb_only::<ExpGolomb>()` does not compile: the coding reads the top of each byte.
    }

    #[test]
    fn a_patch_rewrites_its_own_bits_and_leaves_the_bits_either_side_of_it() {
        fn each<const MSB: bool>() {
            let mut out = Vec::new();
            let mut w = BitWriter::<_, MSB>::new(&mut out);
            w.write(mask64(5), 5).unwrap();
            let at = w.position();
            w.write(0, 6).unwrap();
            w.write(mask64(9), 9).unwrap();
            assert_eq!(w.position(), 20);
            w.patch(at, 6, 0b10_1101);

            let mut r = BitReader::<MSB>::new(&out);
            assert_eq!(r.read(5).unwrap(), mask64(5));
            assert_eq!(r.read(6).unwrap(), 0b10_1101);
            assert_eq!(r.read(9).unwrap(), mask64(9));
        }
        each::<false>();
        each::<true>();
    }

    #[test]
    fn a_patch_clears_the_bits_the_new_value_does_not_set() {
        fn each<const MSB: bool>() {
            let mut out = Vec::new();
            let mut w = BitWriter::<_, MSB>::new(&mut out);
            w.write(u64::MAX, 12).unwrap();
            w.patch(2, 8, 0);
            let mut r = BitReader::<MSB>::new(&out);
            assert_eq!(r.read(2).unwrap(), 0b11);
            assert_eq!(r.read(8).unwrap(), 0);
            assert_eq!(r.read(2).unwrap(), 0b11);
        }
        each::<false>();
        each::<true>();
    }

    #[test]
    fn a_cursor_over_a_buffer_with_bytes_in_it_writes_and_patches_past_them() {
        let mut out = alloc::vec![0xAAu8, 0xBB];
        let mut w = BitWriter::<_, true>::new(&mut out);
        w.write(0b1111, 4).unwrap();
        w.write(0, 4).unwrap();
        w.patch(4, 4, 0b0101);
        assert_eq!(out, [0xAA, 0xBB, 0xF5]);
    }

    #[test]
    fn a_cursor_over_a_full_fixed_buffer_refuses_the_write() {
        let mut room = [0u8; 1];
        let mut out = crate::Fixed::new(&mut room);
        let mut w = BitWriter::<_, true>::new(&mut out);
        assert_eq!(w.write(0, 8), Ok(()));
        assert_eq!(w.write(0, 1), Err(crate::Overflow));
    }

    #[test]
    fn a_refused_write_leaves_the_cursor_and_the_bytes_as_they_were() {
        fn each<const MSB: bool>(before: u8) {
            let mut room = [0u8; 2];
            let mut out = crate::Fixed::new(&mut room);
            let mut w = BitWriter::<_, MSB>::new(&mut out);
            w.write(0b101, 3).unwrap();
            assert_eq!(w.write(u64::MAX, 20), Err(crate::Overflow));
            assert_eq!(w.position(), 3, "msb={MSB}");
            w.write(0, 5).unwrap();
            assert_eq!(out.written(), [before], "msb={MSB}");
        }
        each::<true>(0b1010_0000);
        each::<false>(0b0000_0101);
    }

    #[test]
    fn a_hostile_position_reads_short_instead_of_overflowing() {
        let mut r = BitReader::<true>::at(&[0, 0], usize::MAX);
        assert!(matches!(r.read(8), Err(ParseError::Short { .. })));
        let mut r = BitReader::<true>::new(&[0, 0]);
        r.read(1).unwrap();
        assert!(matches!(r.skip(usize::MAX), Err(ParseError::Short { .. })));
    }

    #[test]
    fn a_cursor_reports_the_bits_it_has_read_and_steps_over_the_bits_it_skips() {
        let bytes = [0b1010_1010u8, 0b0101_0101];
        let mut r = BitReader::<true>::new(&bytes);
        assert_eq!(r.position(), 0);
        r.skip(5).unwrap();
        assert_eq!(r.position(), 5);
        assert_eq!(r.read(3).unwrap(), 0b010);
        assert_eq!(r.bytes_consumed(), 1);
        assert_eq!(
            r.skip(9).unwrap_err(),
            ParseError::Short { need_bytes: 3, got_bytes: 2, at: 1 }
        );
    }

    #[test]
    fn a_read_past_the_end_reports_the_byte_it_began_in() {
        let bytes = [0xFFu8, 0x0F];
        let mut r = BitReader::<true>::new(&bytes);
        assert_eq!(r.read(12).unwrap(), 0xFF0);
        assert_eq!(
            r.read(8).unwrap_err(),
            ParseError::Short { need_bytes: 3, got_bytes: 2, at: 1 }
        );
    }

    #[test]
    fn a_value_above_its_width_is_cut_to_it() {
        assert_eq!(written::<true>(&[(u64::MAX, 5), (0, 3)]), [0b1111_1000]);
        assert_eq!(written::<false>(&[(u64::MAX, 5), (0, 3)]), [0b0001_1111]);
    }
}
