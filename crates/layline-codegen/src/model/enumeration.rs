//! Enumerations.

use alloc::string::String;
use alloc::vec::Vec;

use super::Scalar;

/// An enumeration of named values, with an optional fallback variant for unlisted values.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EnumDef {
    /// The type name.
    pub name: String,
    /// The value's type.
    pub repr: Scalar,
    /// The variants, in declaration order.
    pub variants: Vec<Variant>,
    /// The fallback variant, which holds an unlisted value. `None` means every value is listed.
    pub other: Option<String>,
    /// Documentation.
    pub doc: Option<String>,
}

impl EnumDef {
    /// An enumeration with no fallback, so it must list every value.
    #[must_use]
    pub fn new(name: &str, repr: Scalar, variants: Vec<Variant>) -> Self {
        Self { name: String::from(name), repr, variants, other: None, doc: None }
    }

    /// Adds a fallback variant `other(repr)` that holds any unlisted value.
    #[must_use]
    pub fn with_other(self, other: &str) -> Self {
        Self { other: Some(String::from(other)), ..self }
    }

    /// Sets the documentation.
    #[must_use]
    pub fn with_doc(self, doc: &str) -> Self {
        Self { doc: Some(String::from(doc)), ..self }
    }
}

/// One variant of an [`EnumDef`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Variant {
    /// The value on the wire.
    pub value: i64,
    /// The variant name.
    pub name: String,
    /// Documentation.
    pub doc: Option<String>,
}

impl Variant {
    /// `name = value`, undocumented.
    #[must_use]
    pub fn new(value: i64, name: &str) -> Self {
        Self { value, name: String::from(name), doc: None }
    }

    /// Sets the documentation.
    #[must_use]
    pub fn with_doc(self, doc: &str) -> Self {
        Self { doc: Some(String::from(doc)), ..self }
    }
}
