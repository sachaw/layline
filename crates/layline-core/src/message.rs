//! The decode error type.

/// The nesting depth, in messages, past which decode returns [`ParseError::TooDeep`].
///
/// One limit for the whole crate, since a per-type limit cannot bound mutual recursion.
pub const MAX_NESTING_DEPTH: u32 = 128;

/// Why a decode failed.
///
/// # Where
///
/// Every `at` is a byte index into the slice passed to the failing call. A nested decode reports
/// positions in its own bytes, and the parent [`rebases`](Self::rebased) them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParseError {
    /// The input ends before the message does.
    Short {
        /// Bytes needed.
        need_bytes: usize,
        /// Bytes available.
        got_bytes: usize,
        /// Where the short read starts. See [Where](Self#where).
        at: usize,
    },
    /// A count, discriminant or element breaks the format.
    Malformed {
        /// The field with the bad value.
        field: &'static str,
        /// The segment's first byte. See [Where](Self#where).
        at: usize,
    },
    /// A `#[checksum(..)]` field does not match the bytes it covers.
    Checksum {
        /// The checksum field.
        field: &'static str,
        /// The value computed from the covered bytes.
        expected: i64,
        /// The value on the wire.
        actual: i64,
    },
    /// The prefix bits differ from `prefix_value`, so the frame is a different kind.
    Prefix {
        /// The layout's `prefix_value`.
        expected: u64,
        /// The prefix bits on the wire.
        got: u64,
    },
    /// A `#[range(..)]` field is out of range.
    OutOfRange {
        /// The field.
        field: &'static str,
        /// The value on the wire.
        value: i64,
        /// The lowest allowed value.
        lo: i64,
        /// The highest allowed value.
        hi: i64,
    },
    /// A `#[magic(..)]` field does not match its constant.
    ///
    /// There is no `actual`, since [`at`](Self::Magic::at) indexes the caller's own bytes.
    Magic {
        /// The field.
        field: &'static str,
        /// The constant's bytes, in wire order.
        expected: &'static [u8],
        /// Index of the field's first byte. See [Where](Self#where).
        at: usize,
    },
    /// Nesting exceeded [`MAX_NESTING_DEPTH`].
    TooDeep {
        /// [`MAX_NESTING_DEPTH`].
        limit: u32,
    },
}

impl ParseError {
    /// This error with every `at` shifted by `base`.
    ///
    /// `Short`'s lengths are unchanged.
    #[must_use]
    pub const fn rebased(self, base: usize) -> Self {
        match self {
            ParseError::Short { need_bytes, got_bytes, at } => {
                ParseError::Short { need_bytes, got_bytes, at: at + base }
            }
            ParseError::Malformed { field, at } => ParseError::Malformed { field, at: at + base },
            ParseError::Magic { field, expected, at } => {
                ParseError::Magic { field, expected, at: at + base }
            }
            other => other,
        }
    }
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Short { need_bytes, got_bytes, at } => {
                write!(
                    f,
                    "read at byte {at} needs {need_bytes} bytes, but only {got_bytes} are available"
                )
            }
            Self::Malformed { field, at } => write!(f, "`{field}` at byte {at} is malformed"),
            Self::Checksum { field, expected, actual } => {
                write!(f, "checksum `{field}` is {actual:#x}, expected {expected:#x}")
            }
            Self::Prefix { expected, got } => {
                write!(f, "prefix is {got:#b}, expected {expected:#b}")
            }
            Self::OutOfRange { field, value, lo, hi } => {
                write!(f, "`{field}` is {value}, outside {lo}..={hi}")
            }
            Self::Magic { field, expected, at } => {
                write!(f, "magic `{field}` at byte {at} is not ")?;
                for byte in *expected {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            }
            Self::TooDeep { limit } => write!(f, "nested deeper than {limit} messages"),
        }
    }
}

impl core::error::Error for ParseError {}

#[cfg(test)]
mod tests {
    use super::{MAX_NESTING_DEPTH, ParseError};

    /// Every variant. The match in `at` forces a new variant to be listed.
    fn every_variant() -> [ParseError; 7] {
        [
            ParseError::Short { need_bytes: 9, got_bytes: 4, at: 2 },
            ParseError::Malformed { field: "n", at: 3 },
            ParseError::Checksum { field: "crc", expected: 1, actual: 2 },
            ParseError::Prefix { expected: 1, got: 2 },
            ParseError::OutOfRange { field: "hour", value: 30, lo: 0, hi: 23 },
            ParseError::Magic { field: "sync", expected: b"LN", at: 5 },
            ParseError::TooDeep { limit: MAX_NESTING_DEPTH },
        ]
    }

    #[test]
    fn rebasing_shifts_every_position_a_variant_carries() {
        /// The `at` field, for the variants that have one.
        fn at(e: &ParseError) -> Option<usize> {
            match *e {
                ParseError::Short { at, .. }
                | ParseError::Malformed { at, .. }
                | ParseError::Magic { at, .. } => Some(at),
                ParseError::Checksum { .. }
                | ParseError::Prefix { .. }
                | ParseError::OutOfRange { .. }
                | ParseError::TooDeep { .. } => None,
            }
        }

        for e in every_variant() {
            let moved = e.rebased(100);
            match (at(&e), at(&moved)) {
                (Some(before), Some(after)) => {
                    assert_eq!(after, before + 100, "{e:?} did not rebase");
                }
                (None, None) => {
                    assert_eq!(moved, e, "{e:?} carries no position and must not change")
                }
                _ => unreachable!("rebasing does not change which variant an error is"),
            }
        }
    }

    #[test]
    fn rebasing_leaves_the_lengths_alone() {
        let e = ParseError::Short { need_bytes: 9, got_bytes: 4, at: 2 };
        assert_eq!(e.rebased(100), ParseError::Short { need_bytes: 9, got_bytes: 4, at: 102 });
    }
}
