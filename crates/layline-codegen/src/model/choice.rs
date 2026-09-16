//! Choices: the arms a switch selects between.

use alloc::string::String;
use alloc::vec::Vec;

use super::{Endian, Segment};

/// What a switch selects on.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Discriminant {
    /// The value of an earlier field.
    ///
    /// `outer.inner` names a field of a nested layout.
    Field(String),
    /// The number of bytes left in the body.
    BodyLength,
}

/// One arm of a switch.
///
/// An arm's fields can refer only to each other.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Arm {
    /// The discriminant value that selects this arm.
    pub value: u64,
    /// The arm's segments, in wire order.
    pub body: Vec<Segment>,
    /// The variant name.
    pub name: String,
    /// Documentation.
    pub doc: Option<String>,
}

impl Arm {
    /// An undocumented arm.
    #[must_use]
    pub fn new(value: u64, name: &str, body: Vec<Segment>) -> Self {
        Self { value, body, name: String::from(name), doc: None }
    }

    /// Sets the documentation.
    #[must_use]
    pub fn with_doc(self, doc: &str) -> Self {
        Self { doc: Some(String::from(doc)), ..self }
    }

    /// Whether the arm reads to the end of its input.
    #[must_use]
    pub fn open_ended(&self) -> bool {
        self.body.iter().any(Segment::open_ended)
    }
}

/// The arms of a [`Segment::Switch`].
///
/// With a fallback [`other`](Self::other) variant, an unmatched value keeps its bytes: the rest of
/// the body, or [`other_bytes`](Self::other_bytes) of them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ChoiceDef {
    /// The type name.
    pub name: String,
    /// Byte order of the arms' fields.
    pub endian: Endian,
    /// The arms, in declaration order.
    pub arms: Vec<Arm>,
    /// The fallback variant for an unmatched value. `None` makes it a decode error.
    pub other: Option<String>,
    /// The fallback's size in bytes, when the choice is a union.
    ///
    /// `None` takes the rest of the body, so no field can follow the switch.
    pub other_bytes: Option<usize>,
    /// Documentation.
    pub doc: Option<String>,
}

impl ChoiceDef {
    /// A little-endian choice with no fallback.
    #[must_use]
    pub fn new(name: &str, arms: Vec<Arm>) -> Self {
        Self {
            name: String::from(name),
            endian: Endian::Le,
            arms,
            other: None,
            other_bytes: None,
            doc: None,
        }
    }

    /// Sets the byte order.
    #[must_use]
    pub fn with_endian(self, endian: Endian) -> Self {
        Self { endian, ..self }
    }

    /// Adds a fallback variant that keeps an unmatched value's bytes.
    #[must_use]
    pub fn with_other(self, other: &str) -> Self {
        Self { other: Some(String::from(other)), ..self }
    }

    /// Makes the choice a union of `bytes` bytes.
    ///
    /// Every arm must fill exactly that size, and a field can follow the switch.
    #[must_use]
    pub fn with_other_bytes(self, bytes: usize) -> Self {
        Self { other_bytes: Some(bytes), ..self }
    }

    /// Sets the documentation.
    #[must_use]
    pub fn with_doc(self, doc: &str) -> Self {
        Self { doc: Some(String::from(doc)), ..self }
    }

    /// Whether the choice reads to the end of the body.
    ///
    /// True when any arm does, or when the fallback has no fixed size.
    #[must_use]
    pub fn open_ended(&self) -> bool {
        (self.other.is_some() && self.other_bytes.is_none())
            || self.arms.iter().any(Arm::open_ended)
    }
}
