//! LEB128 and zigzag.
use crate::{Buffer, Overflow, VarCodec, WireInt};

/// Unsigned LEB128 (DWARF ULEB128, WebAssembly `varuint`).
///
/// ```
/// use layline_core::VarCodec;
/// use layline_core::num::Uleb128;
///
/// // DWARF 5, Table 7.7 "Examples of unsigned LEB128 encodings".
/// let mut out = Vec::new();
/// Uleb128(12857).encode(&mut out).unwrap();
/// assert_eq!(out, [0xB9, 0x64]);
/// assert_eq!(Uleb128::decode(&out), Ok((Uleb128(12857), 2)));
///
/// // Every byte has the continuation bit set, so the value never ends.
/// assert!(Uleb128::decode(&[0x80, 0x80, 0x80]).is_err());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uleb128(pub u64);

impl From<u64> for Uleb128 {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<Uleb128> for u64 {
    fn from(v: Uleb128) -> Self {
        v.0
    }
}

impl VarCodec for Uleb128 {
    fn decode(bytes: &[u8]) -> Result<(Self, usize), crate::ParseError> {
        let mut value = 0u64;
        let mut shift = 0u32;
        for (i, &b) in bytes.iter().enumerate() {
            if shift >= u64::BITS {
                return Err(crate::ParseError::Malformed { field: "Uleb128", at: 0 });
            }
            let payload = u64::from(b & 0x7F);
            if (payload << shift) >> shift != payload {
                return Err(crate::ParseError::Malformed { field: "Uleb128", at: 0 });
            }
            value |= payload << shift;
            if b & 0x80 == 0 {
                return Ok((Self(value), i + 1));
            }
            shift += 7;
        }
        Err(crate::ParseError::Short { need_bytes: bytes.len() + 1, got_bytes: bytes.len(), at: 0 })
    }

    fn encode<B: Buffer>(&self, out: &mut B) -> Result<(), Overflow> {
        let mut rest = self.0;
        loop {
            let mut byte = (rest & 0x7F) as u8;
            rest >>= 7;
            if rest != 0 {
                byte |= 0x80;
            }
            out.push(&[byte])?;
            if rest == 0 {
                return Ok(());
            }
        }
    }
}

impl WireInt for Uleb128 {
    fn to_i64(&self) -> i64 {
        self.0.to_i64()
    }

    fn from_i64(n: i64) -> Self {
        Self(n as u64)
    }
}

/// Signed LEB128 (DWARF SLEB128).
///
/// ```
/// use layline_core::VarCodec;
/// use layline_core::num::Sleb128;
///
/// // DWARF 5, Table 7.8 "Examples of signed LEB128 encodings".
/// let mut out = Vec::new();
/// Sleb128(-2).encode(&mut out).unwrap();
/// assert_eq!(out, [0x7E]);
///
/// // 127 needs a second byte: bit 6 of the last byte is the sign.
/// out.clear();
/// Sleb128(127).encode(&mut out).unwrap();
/// assert_eq!(out, [0xFF, 0x00]);
/// assert_eq!(Sleb128::decode(&out), Ok((Sleb128(127), 2)));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sleb128(pub i64);

impl From<i64> for Sleb128 {
    fn from(v: i64) -> Self {
        Self(v)
    }
}

impl From<Sleb128> for i64 {
    fn from(v: Sleb128) -> Self {
        v.0
    }
}

impl VarCodec for Sleb128 {
    fn decode(bytes: &[u8]) -> Result<(Self, usize), crate::ParseError> {
        let mut value = 0u64;
        let mut shift = 0u32;
        for (i, &b) in bytes.iter().enumerate() {
            if shift >= u64::BITS {
                return Err(crate::ParseError::Malformed { field: "Sleb128", at: 0 });
            }
            let payload = u64::from(b & 0x7F);
            if shift == u64::BITS - 1 {
                let fill = if payload & 1 == 0 { 0x00 } else { 0x7F };
                if payload != fill {
                    return Err(crate::ParseError::Malformed { field: "Sleb128", at: 0 });
                }
            }
            value |= payload << shift;
            shift += 7;
            if b & 0x80 == 0 {
                let mut signed = value as i64;
                if shift < u64::BITS && b & 0x40 != 0 {
                    signed |= -1i64 << shift;
                }
                return Ok((Self(signed), i + 1));
            }
        }
        Err(crate::ParseError::Short { need_bytes: bytes.len() + 1, got_bytes: bytes.len(), at: 0 })
    }

    fn encode<B: Buffer>(&self, out: &mut B) -> Result<(), Overflow> {
        let mut rest = self.0;
        loop {
            let mut byte = (rest as u64 & 0x7F) as u8;
            rest >>= 7;
            let sign_set = byte & 0x40 != 0;
            let done = (rest == 0 && !sign_set) || (rest == -1 && sign_set);
            if !done {
                byte |= 0x80;
            }
            out.push(&[byte])?;
            if done {
                return Ok(());
            }
        }
    }
}

impl WireInt for Sleb128 {
    fn to_i64(&self) -> i64 {
        self.0
    }

    fn from_i64(n: i64) -> Self {
        Self(n)
    }
}

/// Zigzag encoding: maps signed values to unsigned, keeping small magnitudes small.
///
/// `0, -1, 1, -2, 2` map to `0, 1, 2, 3, 4`. Defined for every `i64`, including [`i64::MIN`].
///
/// Combine it with a varint for protobuf `sint64`, Avro `long` or Thrift compact integers:
///
/// ```
/// use layline_core::num::{Uleb128, unzigzag, zigzag};
/// use layline_core::{Buffer, Overflow, ParseError, VarCodec};
///
/// #[derive(Debug, PartialEq)]
/// struct Delta(i64);
///
/// impl VarCodec for Delta {
///     fn decode(bytes: &[u8]) -> Result<(Self, usize), ParseError> {
///         let (raw, used) = Uleb128::decode(bytes)?;
///         Ok((Delta(unzigzag(raw.0)), used))
///     }
///
///     fn encode<B: Buffer>(&self, out: &mut B) -> Result<(), Overflow> {
///         Uleb128(zigzag(self.0)).encode(out)
///     }
/// }
///
/// let mut out = Vec::new();
/// Delta(-2).encode(&mut out).unwrap();
/// assert_eq!(out, [0x03]);
/// assert_eq!(Delta::decode(&out), Ok((Delta(-2), 1)));
/// ```
#[must_use]
pub const fn zigzag(value: i64) -> u64 {
    ((value as u64) << 1) ^ ((value >> 63) as u64)
}

/// The inverse of [`zigzag`].
#[must_use]
pub const fn unzigzag(raw: u64) -> i64 {
    (raw >> 1) as i64 ^ -((raw & 1) as i64)
}
