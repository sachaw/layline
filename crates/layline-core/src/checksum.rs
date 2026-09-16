//! The `Checksum` trait, and built-in checksums.
use core::marker::PhantomData;

use crate::WireInt;

/// The algorithm in `#[checksum(Algorithm, over = ..)]`.
///
/// Bytes arrive in runs, so a checksum can skip its own field. The result must depend only on the bytes.
pub trait Checksum {
    /// The check value's type, which is also the field's type.
    type Output: WireInt;

    /// The running state between runs.
    type State;

    /// The initial state.
    fn init() -> Self::State;

    /// Fold one run of bytes into the state. Called once per run, in wire order.
    fn update(state: Self::State, bytes: &[u8]) -> Self::State;

    /// The check value for the final state.
    fn finish(state: Self::State) -> Self::Output;

    /// The check value of one run of bytes.
    fn compute(bytes: &[u8]) -> Self::Output {
        Self::finish(Self::update(Self::init(), bytes))
    }
}

/// A CRC register type: `u8`, `u16`, `u32` or `u64`.
pub trait CrcWord: crate::sealed::CrcWord + WireInt + Copy {
    /// The register width in bits.
    const BITS: u32;

    /// The low `BITS` bits of `register`.
    fn from_register(register: u64) -> Self;

    /// This word as a register.
    fn register(self) -> u64;
}

macro_rules! crc_word {
    ($($t:ty),*) => {$(
        impl crate::sealed::CrcWord for $t {}

        impl CrcWord for $t {
            const BITS: u32 = <$t>::BITS;

            fn from_register(register: u64) -> Self {
                register as $t
            }

            fn register(self) -> u64 {
                self as u64
            }
        }
    )*};
}

crc_word!(u8, u16, u32, u64);

/// A CRC defined by its catalogue parameters: width, polynomial, initial value, final XOR and reflection.
///
/// `W` sets the width. When `REFLECT` is set, `POLY` is the reversed polynomial, as catalogues list
/// it. For example, CRC-16/MODBUS is `0xA001`, the reverse of `0x8005`.
///
/// Common parameters, with the check value for `b"123456789"`:
///
/// | algorithm | `W` | `POLY` | `INIT` | `XOROUT` | `REFLECT` | check |
/// |---|---|---|---|---|---|---|
/// | CRC-8/SMBUS | `u8` | `0x07` | `0x00` | `0x00` | `false` | `0xF4` |
/// | CRC-16/CCITT-FALSE | `u16` | `0x1021` | `0xFFFF` | `0x0000` | `false` | `0x29B1` |
/// | CRC-16/XMODEM | `u16` | `0x1021` | `0x0000` | `0x0000` | `false` | `0x31C3` |
/// | CRC-16/X-25 (RFC 1662 FCS-16) | `u16` | `0x8408` | `0xFFFF` | `0xFFFF` | `true` | `0x906E` |
/// | CRC-16/KERMIT | `u16` | `0x8408` | `0x0000` | `0x0000` | `true` | `0x2189` |
/// | CRC-16/MODBUS | `u16` | `0xA001` | `0xFFFF` | `0x0000` | `true` | `0x4B37` |
/// | CRC-16/USB | `u16` | `0xA001` | `0xFFFF` | `0xFFFF` | `true` | `0xB4C8` |
/// | CRC-32/ISO-HDLC (PNG, zlib, Ethernet) | `u32` | `0xEDB88320` | `0xFFFFFFFF` | `0xFFFFFFFF` | `true` | `0xCBF43926` |
/// | CRC-32/BZIP2 | `u32` | `0x04C11DB7` | `0xFFFFFFFF` | `0xFFFFFFFF` | `false` | `0xFC891918` |
/// | CRC-64/ECMA-182 | `u64` | `0x42F0E1EBA9EA3693` | `0` | `0` | `false` | `0x6C40DF5F0B497347` |
///
/// ```
/// use layline_core::Checksum;
/// use layline_core::checksum::Crc;
///
/// // CRC-32/ISO-HDLC.
/// type Crc32 = Crc<u32, 0xEDB8_8320, 0xFFFF_FFFF, 0xFFFF_FFFF, true>;
/// assert_eq!(<Crc32 as Checksum>::compute(b"123456789"), 0xCBF4_3926);
/// ```
///
/// A parameter wider than `W` does not compile:
///
/// ```compile_fail
/// use layline_core::Checksum;
/// use layline_core::checksum::Crc;
///
/// // A 17-bit polynomial in a 16-bit register.
/// let _ = <Crc<u16, 0x1_1021, 0xFFFF, 0, false> as Checksum>::compute(b"");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Crc<W, const POLY: u64, const INIT: u64, const XOROUT: u64, const REFLECT: bool>(
    PhantomData<fn() -> W>,
);

impl<W: CrcWord, const POLY: u64, const INIT: u64, const XOROUT: u64, const REFLECT: bool> Checksum
    for Crc<W, POLY, INIT, XOROUT, REFLECT>
{
    type Output = W;
    type State = W;

    fn init() -> W {
        const {
            let mask = crate::mask::mask64(W::BITS);
            assert!(POLY & !mask == 0, "a Crc's POLY is wider than its word");
            assert!(INIT & !mask == 0, "a Crc's INIT is wider than its word");
            assert!(XOROUT & !mask == 0, "a Crc's XOROUT is wider than its word");
        }
        W::from_register(INIT)
    }

    fn update(state: W, bytes: &[u8]) -> W {
        let top = 1u64 << (W::BITS - 1);
        let mask = crate::mask::mask64(W::BITS);
        let mut crc = state.register();
        for &byte in bytes {
            if REFLECT {
                crc ^= u64::from(byte);
                for _ in 0..8 {
                    crc = if crc & 1 != 0 { (crc >> 1) ^ POLY } else { crc >> 1 };
                }
            } else {
                crc ^= u64::from(byte) << (W::BITS - 8);
                for _ in 0..8 {
                    crc = if crc & top != 0 { (crc << 1) ^ POLY } else { crc << 1 } & mask;
                }
            }
        }
        W::from_register(crc)
    }

    fn finish(state: W) -> W {
        W::from_register(state.register() ^ XOROUT)
    }
}

/// The state of a [`Sum16Neg`]: the running sum and any unpaired byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Sum16State {
    sum: u16,
    half: Option<u8>,
}

fn sum16_feed(mut state: Sum16State, bytes: &[u8], pair: fn(u8, u8) -> u16) -> Sum16State {
    let rest = match state.half.take() {
        Some(held) => match bytes.split_first() {
            None => {
                state.half = Some(held);
                return state;
            }
            Some((&next, rest)) => {
                state.sum = state.sum.wrapping_add(pair(held, next));
                rest
            }
        },
        None => bytes,
    };
    let (words, tail) = rest.as_chunks::<2>();
    for &[lo, hi] in words {
        state.sum = state.sum.wrapping_add(pair(lo, hi));
    }
    if let [last] = tail {
        state.half = Some(*last);
    }
    state
}

fn sum16_close(state: Sum16State, pair: fn(u8, u8) -> u16) -> u16 {
    match state.half {
        None => state.sum,
        Some(last) => state.sum.wrapping_add(pair(last, 0)),
    }
    .wrapping_neg()
}

fn pair_le(first: u8, second: u8) -> u16 {
    u16::from_le_bytes([first, second])
}

fn pair_be(first: u8, second: u8) -> u16 {
    u16::from_be_bytes([first, second])
}

/// The negated 16-bit sum of bytes paired into words, so a valid block plus its check word sums to zero.
///
/// `BE` sets the byte order within a word. An odd final byte is padded with zero. An odd byte at
/// the end of one run pairs with the first byte of the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Sum16Neg<const BE: bool>;

impl<const BE: bool> Sum16Neg<BE> {
    /// Combines two bytes into a word in this byte order.
    const PAIR: fn(u8, u8) -> u16 = if BE { pair_be } else { pair_le };
}

impl<const BE: bool> Checksum for Sum16Neg<BE> {
    type Output = u16;
    type State = Sum16State;

    fn init() -> Sum16State {
        Sum16State::default()
    }

    fn update(state: Sum16State, bytes: &[u8]) -> Sum16State {
        sum16_feed(state, bytes, Self::PAIR)
    }

    fn finish(state: Sum16State) -> u16 {
        sum16_close(state, Self::PAIR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The input CRC catalogues give check values for.
    const CHECK: &[u8] = b"123456789";

    /// Two's-complement LRC-8, written as a user would write one.
    struct Lrc8;

    impl Checksum for Lrc8 {
        type Output = u8;
        type State = u8;

        fn init() -> u8 {
            0
        }

        fn update(state: u8, bytes: &[u8]) -> u8 {
            bytes.iter().fold(state, |acc, &b| acc.wrapping_add(b))
        }

        fn finish(state: u8) -> u8 {
            state.wrapping_neg()
        }
    }

    type Ccitt = Crc<u16, 0x1021, 0xFFFF, 0x0000, false>;
    type Crc32 = Crc<u32, 0xEDB8_8320, 0xFFFF_FFFF, 0xFFFF_FFFF, true>;
    type Sum16NegLe = Sum16Neg<false>;
    type Sum16NegBe = Sum16Neg<true>;

    /// The wrapping 16-bit sum of `words`.
    fn word_sum(words: impl Iterator<Item = u16>) -> u16 {
        words.fold(0u16, |acc, w| acc.wrapping_add(w))
    }

    fn words_le(bytes: &[u8]) -> impl Iterator<Item = u16> + '_ {
        bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
    }

    fn words_be(bytes: &[u8]) -> impl Iterator<Item = u16> + '_ {
        bytes.chunks(2).map(|c| u16::from_be_bytes([c[0], c.get(1).copied().unwrap_or(0)]))
    }

    #[test]
    fn the_published_parameters_give_their_published_check_values() {
        assert_eq!(<Crc<u8, 0x07, 0x00, 0x00, false> as Checksum>::compute(CHECK), 0xF4);
        assert_eq!(<Crc<u16, 0x1021, 0xFFFF, 0x0000, false> as Checksum>::compute(CHECK), 0x29B1);
        assert_eq!(<Crc<u16, 0x1021, 0x0000, 0x0000, false> as Checksum>::compute(CHECK), 0x31C3);
        assert_eq!(<Crc<u16, 0x8408, 0xFFFF, 0xFFFF, true> as Checksum>::compute(CHECK), 0x906E);
        assert_eq!(<Crc<u16, 0x8408, 0x0000, 0x0000, true> as Checksum>::compute(CHECK), 0x2189);
        assert_eq!(<Crc<u16, 0xA001, 0xFFFF, 0x0000, true> as Checksum>::compute(CHECK), 0x4B37);
        assert_eq!(<Crc<u16, 0xA001, 0xFFFF, 0xFFFF, true> as Checksum>::compute(CHECK), 0xB4C8);
        assert_eq!(<Crc32 as Checksum>::compute(CHECK), 0xCBF4_3926);
        assert_eq!(
            <Crc<u32, 0x04C1_1DB7, 0xFFFF_FFFF, 0xFFFF_FFFF, false> as Checksum>::compute(CHECK),
            0xFC89_1918
        );
        assert_eq!(
            <Crc<u64, 0x42F0_E1EB_A9EA_3693, 0, 0, false> as Checksum>::compute(CHECK),
            0x6C40_DF5F_0B49_7347
        );
    }

    /// The annotations fail to compile unless the state is the register's own width.
    #[test]
    fn a_crc_folds_in_its_own_width() {
        type Fcs16 = Crc<u16, 0x8408, 0xFFFF, 0xFFFF, true>;
        let state: u16 = <Fcs16 as Checksum>::init();
        let state: u16 = <Fcs16 as Checksum>::update(state, b"1234");
        let fcs = <Fcs16 as Checksum>::finish(<Fcs16 as Checksum>::update(state, b"56789"));
        assert_eq!(fcs, 0x906E, "RFC 1662's check value, folded in two runs");

        let narrow: u8 = <Crc<u8, 0x07, 0x00, 0x00, false> as Checksum>::init();
        let middle: u32 = <Crc32 as Checksum>::init();
        let wide: u64 = <Crc<u64, 0x42F0_E1EB_A9EA_3693, 0, 0, false> as Checksum>::init();
        assert_eq!((narrow, middle, wide), (0, 0xFFFF_FFFF, 0));
    }

    #[test]
    fn lrc8_is_the_twos_complement_of_the_byte_sum() {
        assert_eq!(<Lrc8 as Checksum>::compute(CHECK), 0x23);

        for (id, len, crc) in [(20u8, 100u8, 0x1234u16), (0, 0, 0), (255, 255, 0xFFFF)] {
            let bytes = [id, len, crc as u8, (crc >> 8) as u8];
            let expected =
                (((id as u32 + len as u32 + (crc & 0xFF) as u32 + (crc >> 8) as u32) as u8) ^ 0xFF)
                    .wrapping_add(1);
            assert_eq!(<Lrc8 as Checksum>::compute(&bytes), expected);
        }
    }

    #[test]
    fn a_block_and_its_sum16_check_word_sum_to_zero() {
        let block = [0xFF, 0x81, 0x34, 0x12, 0x00, 0x00, 0x00, 0x08];
        let check = <Sum16NegLe as Checksum>::compute(&block);
        assert_eq!(word_sum(words_le(&block).chain([check])), 0);
        assert_ne!(word_sum(words_le(&block)), 0);
    }

    #[test]
    fn feeding_bytes_agrees_with_summing_words() {
        for run in
            [b"".as_slice(), b"1", b"12", CHECK, b"a rather longer run of bytes, odd in length"]
        {
            assert_eq!(
                <Sum16NegLe as Checksum>::compute(run),
                word_sum(words_le(run)).wrapping_neg(),
                "{run:?}"
            );
            assert_eq!(
                <Sum16NegBe as Checksum>::compute(run),
                word_sum(words_be(run)).wrapping_neg(),
                "{run:?}"
            );
        }
    }

    #[test]
    fn two_runs_check_the_same_as_their_concatenation() {
        fn split_ways<A: Checksum>(whole: &[u8])
        where
            A::Output: PartialEq + core::fmt::Debug,
        {
            let want = A::compute(whole);
            for at in 0..=whole.len() {
                let (head, tail) = whole.split_at(at);
                let got = A::finish(A::update(A::update(A::init(), head), tail));
                assert_eq!(got, want, "split at {at}");
            }
        }
        split_ways::<Lrc8>(CHECK);
        split_ways::<Ccitt>(CHECK);
        split_ways::<Crc32>(CHECK);
        split_ways::<Sum16NegLe>(CHECK);
        split_ways::<Sum16NegBe>(CHECK);
        let state = <Sum16NegBe as Checksum>::init();
        let state = <Sum16NegBe as Checksum>::update(state, b"1");
        let state = <Sum16NegBe as Checksum>::update(state, b"234567");
        let state = <Sum16NegBe as Checksum>::update(state, b"89");
        assert_eq!(<Sum16NegBe as Checksum>::finish(state), 0xF62C);
    }

    #[test]
    fn sum16_over_bytes_pads_a_ragged_tail() {
        assert_eq!(<Sum16NegLe as Checksum>::compute(CHECK), 0x2AF7);
        assert_eq!(<Sum16NegBe as Checksum>::compute(CHECK), 0xF62C);

        let words = [0x3231u16, 0x3433, 0x3635, 0x3837];
        assert_eq!(
            <Sum16NegLe as Checksum>::compute(b"12345678"),
            word_sum(words.iter().copied()).wrapping_neg()
        );

        assert_ne!(
            <Sum16NegLe as Checksum>::compute(CHECK),
            <Sum16NegLe as Checksum>::compute(b"12345678"),
        );
    }
}
