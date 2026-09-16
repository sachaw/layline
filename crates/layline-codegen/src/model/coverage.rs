//! The bytes a checksum covers.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// The bytes a checksum covers, as `over = from..to`.
///
/// The range may include bytes after the checksum, but never the checksum itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Coverage {
    /// Where the range starts.
    pub from: CoverFrom,
    /// Where the range ends.
    pub to: CoverTo,
}

impl Coverage {
    /// `over = <from>..<to>`.
    #[must_use]
    pub fn new(from: CoverFrom, to: CoverTo) -> Self {
        Self { from, to }
    }

    /// `over = ..`: from the start of the message to the checksum.
    #[must_use]
    pub fn whole() -> Self {
        Self { from: CoverFrom::Start, to: CoverTo::Here }
    }

    /// `over = field..`: from `field` to the checksum.
    #[must_use]
    pub fn from_field(field: &str) -> Self {
        Self { from: CoverFrom::Field(String::from(field)), to: CoverTo::Here }
    }

    /// Sets the end.
    #[must_use]
    pub fn to(self, to: CoverTo) -> Self {
        Self { to, ..self }
    }

    /// Resolves the range for a checksum at `own`.
    ///
    /// `at` returns a field's `(start, end)`. Returns `None` if a name does not resolve.
    #[must_use]
    pub fn resolve(
        &self,
        own: (u64, u64),
        at: impl Fn(&str) -> Option<(u64, u64)>,
    ) -> Option<Covered> {
        let (start, end) = own;
        let from = match &self.from {
            CoverFrom::Start => 0,
            CoverFrom::Field(f) => at(f)?.0,
        };
        let to = match &self.to {
            CoverTo::Here => start,
            CoverTo::Before(f) => at(f)?.0,
            CoverTo::After(f) => at(f)?.1,
        };
        Some(Covered::new(from, to, (start, end)))
    }
}

/// A resolved [`Coverage`], in the caller's units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Covered {
    /// Start of the range.
    pub from: u64,
    /// End of the range, exclusive.
    pub to: u64,
    /// The checksum's own `(start, end)`.
    pub own: (u64, u64),
}

impl Covered {
    /// The range `from..to` for a checksum at `own`.
    #[must_use]
    pub fn new(from: u64, to: u64, own: (u64, u64)) -> Self {
        Self { from, to, own }
    }

    /// Whether the checksum sits inside the range and must be skipped.
    #[must_use]
    pub fn excises(&self) -> bool {
        self.from <= self.own.0 && self.to > self.own.0
    }

    /// The covered spans, without the checksum's own bytes.
    #[must_use]
    pub fn runs(&self) -> Vec<(u64, u64)> {
        if self.excises() {
            vec![(self.from, self.own.0), (self.own.1, self.to)]
        } else {
            vec![(self.from, self.to)]
        }
    }
}

/// Where a checksum's range starts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CoverFrom {
    /// The first byte of the message.
    Start,
    /// The first byte of a named field, such as the one after a sync word.
    Field(String),
}

/// Where a checksum's range ends.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CoverTo {
    /// Just before the checksum: `over = a..`. The default.
    Here,
    /// Just before a named field: `over = a..b`.
    Before(String),
    /// Just after a named field: `over = a..=b`.
    After(String),
}
