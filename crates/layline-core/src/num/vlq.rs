//! MSB-first variable-length quantities.
use crate::{Buffer, EncodeError, ParseError, VarCodec, WireInt};

/// An unsigned variable-length quantity of at most `N` bytes, as in MIDI files and JEL.
///
/// Each byte holds seven bits, the most significant group first, and every byte but the last has
/// its top bit set. `N` is `1..=9`, so `Vlq<N>` holds `0..=Vlq::<N>::MAX`, `2^(7N) - 1`.
///
/// Decode refuses an `N`-th byte with its top bit set as
/// [`Malformed`](ParseError::Malformed), since the value would run past `N` bytes. A leading
/// `0x80` pads without changing the value, as a LEB128 one does, and decode accepts it within the
/// `N` bytes. Encode writes the fewest bytes.
///
/// ```
/// use layline_core::VarCodec;
/// use layline_core::num::Vlq;
///
/// // The Standard MIDI File examples: 0x2000 is two bytes, 0x0FFFFFFF four.
/// let mut out = Vec::new();
/// Vlq::<4>(0x2000).encode(&mut out).unwrap();
/// assert_eq!(out, [0xC0, 0x00]);
/// assert_eq!(Vlq::<4>::decode(&[0xFF, 0xFF, 0xFF, 0x7F]), Ok((Vlq(0x0FFF_FFFF), 4)));
///
/// // A fifth byte is past the cap.
/// assert!(Vlq::<4>::decode(&[0x81, 0x80, 0x80, 0x80, 0x00]).is_err());
///
/// // A value above `MAX` would need a fifth byte, so encode refuses it.
/// assert!(Vlq::<4>(1 << 28).encode(&mut out).is_err());
/// ```
///
/// Encode refuses a value above [`MAX`](Self::MAX) as [`Unfit`](EncodeError::Unfit), rather
/// than write bytes that decode refuses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Vlq<const N: usize>(pub u64);

impl<const N: usize> Vlq<N> {
    const BYTES: usize = {
        assert!(N >= 1 && N <= 9, "Vlq<N>: N must be in 1..=9");
        N
    };

    /// The largest value `N` bytes hold.
    pub const MAX: u64 = (1u64 << (7 * Self::BYTES)) - 1;

    /// The value, or `None` if it needs more than `N` bytes.
    #[must_use]
    pub const fn new(value: u64) -> Option<Self> {
        if value <= Self::MAX { Some(Self(value)) } else { None }
    }
}

impl<const N: usize> From<Vlq<N>> for u64 {
    fn from(v: Vlq<N>) -> Self {
        v.0
    }
}

impl<const N: usize> VarCodec for Vlq<N> {
    const ENCODE_FALLIBLE: bool = true;

    fn decode(bytes: &[u8]) -> Result<(Self, usize), ParseError> {
        let cap = Self::BYTES;
        let mut value = 0u64;
        for (i, &b) in bytes.iter().enumerate() {
            value = value << 7 | u64::from(b & 0x7F);
            if b & 0x80 == 0 {
                return Ok((Self(value), i + 1));
            }
            if i + 1 == cap {
                return Err(ParseError::Malformed { field: "Vlq", at: 0 });
            }
        }
        Err(ParseError::Short { need_bytes: bytes.len() + 1, got_bytes: bytes.len(), at: 0 })
    }

    fn encode<B: Buffer>(&self, out: &mut B) -> Result<(), EncodeError> {
        if self.0 > Self::MAX {
            return Err(EncodeError::Unfit {
                field: "Vlq",
                value: i128::from(self.0),
                bits: 7 * Self::BYTES as u32,
            });
        }
        let mut bytes = [0u8; 9];
        let mut first = bytes.len();
        let mut rest = self.0;
        loop {
            first -= 1;
            let more = if first == bytes.len() - 1 { 0 } else { 0x80 };
            bytes[first] = (rest & 0x7F) as u8 | more;
            rest >>= 7;
            if rest == 0 {
                return Ok(out.push(&bytes[first..])?);
            }
        }
    }
}

impl<const N: usize> WireInt for Vlq<N> {
    fn to_i64(&self) -> i64 {
        self.0.to_i64()
    }

    fn from_i64(n: i64) -> Self {
        Self(n as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::Vlq;
    use crate::{Buffer, EncodeError, Fixed, ParseError, VarCodec};

    fn wire<const N: usize>(v: u64) -> ([u8; 9], usize) {
        let mut room = [0u8; 9];
        let mut out = Fixed::new(&mut room);
        Vlq::<N>(v).encode(&mut out).unwrap();
        let n = out.into_written().len();
        (room, n)
    }

    #[test]
    fn each_boundary_takes_the_fewest_bytes_and_reads_back() {
        let cases: [(u64, &[u8]); 8] = [
            (0, &[0x00]),
            (127, &[0x7F]),
            (128, &[0x81, 0x00]),
            (16_383, &[0xFF, 0x7F]),
            (16_384, &[0x81, 0x80, 0x00]),
            ((1 << 21) - 1, &[0xFF, 0xFF, 0x7F]),
            (1 << 21, &[0x81, 0x80, 0x80, 0x00]),
            ((1 << 28) - 1, &[0xFF, 0xFF, 0xFF, 0x7F]),
        ];
        for (value, bytes) in cases {
            let (room, n) = wire::<4>(value);
            assert_eq!(&room[..n], bytes, "{value}");
            assert_eq!(Vlq::<4>::decode(bytes), Ok((Vlq(value), n)), "{value}");
        }
    }

    #[test]
    fn the_widest_cap_holds_sixty_three_bits() {
        assert_eq!(Vlq::<9>::MAX, (1 << 63) - 1);
        let (room, n) = wire::<9>(Vlq::<9>::MAX);
        assert_eq!(n, 9);
        assert_eq!(Vlq::<9>::decode(&room[..n]), Ok((Vlq(Vlq::<9>::MAX), 9)));
    }

    #[test]
    fn a_continuation_on_the_last_allowed_byte_is_malformed() {
        let malformed = ParseError::Malformed { field: "Vlq", at: 0 };
        assert_eq!(Vlq::<4>::decode(&[0x81, 0x80, 0x80, 0x80, 0x00]), Err(malformed));
        assert_eq!(Vlq::<4>::decode(&[0xFF, 0xFF, 0xFF, 0xFF]), Err(malformed));
        assert_eq!(Vlq::<1>::decode(&[0x80, 0x00]), Err(malformed));
    }

    #[test]
    fn a_value_cut_short_needs_one_more_byte() {
        assert_eq!(
            Vlq::<4>::decode(&[0x81, 0x80]),
            Err(ParseError::Short { need_bytes: 3, got_bytes: 2, at: 0 })
        );
        assert_eq!(
            Vlq::<4>::decode(&[]),
            Err(ParseError::Short { need_bytes: 1, got_bytes: 0, at: 0 })
        );
    }

    #[test]
    fn padding_inside_the_cap_reads_as_the_value() {
        assert_eq!(Vlq::<4>::decode(&[0x80, 0x80, 0x7F]), Ok((Vlq(127), 3)));
    }

    #[test]
    fn new_refuses_a_value_past_the_cap() {
        assert_eq!(Vlq::<4>::new(268_435_455), Some(Vlq(268_435_455)));
        assert_eq!(Vlq::<4>::new(268_435_456), None);
    }

    #[test]
    fn encode_refuses_a_value_past_the_cap_and_writes_nothing() {
        let mut room = [0u8; 9];
        let mut out = Fixed::new(&mut room);
        assert_eq!(
            Vlq::<4>(1 << 28).encode(&mut out),
            Err(EncodeError::Unfit { field: "Vlq", value: 1 << 28, bits: 28 })
        );
        assert_eq!(out.written(), &[]);
        assert_eq!(
            Vlq::<1>(u64::MAX).encode(&mut out),
            Err(EncodeError::Unfit { field: "Vlq", value: i128::from(u64::MAX), bits: 7 })
        );
    }

    #[test]
    fn a_full_buffer_is_an_overflow() {
        let mut room = [0u8; 1];
        let mut out = Fixed::new(&mut room);
        assert_eq!(Vlq::<4>(128).encode(&mut out), Err(EncodeError::Overflow));
    }
}
