//! Enumerations.

use alloc::string::String;
use alloc::vec::Vec;

use super::Derive;

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
    /// The variant marked `#[default]`, which needs an ungated `Default` derive.
    pub default: Option<String>,
    /// Derives for this enumeration, written after the module's.
    pub derives: Vec<Derive>,
    /// Documentation for the fallback variant. `None` writes a general sentence.
    pub other_doc: Option<String>,
}

impl EnumDef {
    /// An enumeration with no fallback, so it must list every value.
    #[must_use]
    pub fn new(name: &str, repr: Scalar, variants: Vec<Variant>) -> Self {
        Self {
            name: String::from(name),
            repr,
            variants,
            other: None,
            doc: None,
            default: None,
            derives: Vec::new(),
            other_doc: None,
        }
    }

    /// Sets the fallback variant's documentation.
    #[must_use]
    pub fn with_other_doc(self, doc: &str) -> Self {
        Self { other_doc: Some(String::from(doc)), ..self }
    }

    /// Marks `variant` `#[default]`, which the derives must match with an ungated `Default`.
    ///
    /// The variant must be a listed one, not the fallback, which carries a value.
    #[must_use]
    pub fn with_default(self, variant: &str) -> Self {
        Self { default: Some(String::from(variant)), ..self }
    }

    /// Sets the derives written on this enumeration, after the module's.
    #[must_use]
    pub fn with_derives(self, derives: Vec<Derive>) -> Self {
        Self { derives, ..self }
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
