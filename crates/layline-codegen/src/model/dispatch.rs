//! Dispatches: bodies selected by an id the enclosing message has already read.

use alloc::string::String;
use alloc::vec::Vec;

use super::Derive;

use super::Scalar;

/// Bodies selected by an id the enclosing message has already read, as `#[derive(Dispatch)]`
/// declares.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DispatchDef {
    /// The type name.
    pub name: String,
    /// The id's type: `#[dispatch(id = ..)]`.
    pub id: Scalar,
    /// The arms, in declaration order.
    pub arms: Vec<DispatchArm>,
    /// The fallback variant for an unlisted id.
    pub other: String,
    /// Documentation.
    pub doc: Option<String>,
    /// Derives for this catalogue, written after the module's.
    pub derives: Vec<Derive>,
    /// The id's width in bits inside every payload: `#[dispatch(.., prefix = K)]`.
    ///
    /// Zero reads the id from elsewhere, and each payload's `Layout::PREFIX_BITS` must match.
    pub prefix: u32,
    /// Documentation for the fallback variant. `None` writes a general sentence.
    pub other_doc: Option<String>,
    /// The fallback variant's body type. `None` is `Vec<u8>`.
    ///
    /// Any type that is `From<&[u8]>` and `Deref<Target = [u8]>` works, such as a fixed-size
    /// type that keeps the body without allocating.
    pub other_body: Option<String>,
}

impl DispatchDef {
    /// A dispatch with its fallback variant named `Unknown`.
    #[must_use]
    pub fn new(name: &str, id: Scalar, arms: Vec<DispatchArm>) -> Self {
        Self {
            name: String::from(name),
            id,
            arms,
            other: String::from("Unknown"),
            doc: None,
            derives: Vec::new(),
            prefix: 0,
            other_doc: None,
            other_body: None,
        }
    }

    /// Sets the fallback variant's documentation.
    #[must_use]
    pub fn with_other_doc(self, doc: &str) -> Self {
        Self { other_doc: Some(String::from(doc)), ..self }
    }

    /// Sets the id's width in bits inside every payload.
    ///
    /// Each payload declares the same width with `Container::Word { prefix, .. }`, and the
    /// generated code asserts that they agree.
    #[must_use]
    pub fn with_prefix(self, prefix: u32) -> Self {
        Self { prefix, ..self }
    }

    /// Sets the fallback variant's body type, in place of `Vec<u8>`.
    #[must_use]
    pub fn with_other_body(self, ty: &str) -> Self {
        Self { other_body: Some(String::from(ty)), ..self }
    }

    /// Sets the derives written on this catalogue, after the module's.
    #[must_use]
    pub fn with_derives(self, derives: Vec<Derive>) -> Self {
        Self { derives, ..self }
    }

    /// Renames the fallback variant.
    #[must_use]
    pub fn with_other(self, other: &str) -> Self {
        Self { other: String::from(other), ..self }
    }

    /// Sets the documentation.
    #[must_use]
    pub fn with_doc(self, doc: &str) -> Self {
        Self { doc: Some(String::from(doc)), ..self }
    }
}

/// One arm of a [`DispatchDef`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DispatchArm {
    /// The id that selects this arm.
    pub id: i64,
    /// The variant name.
    pub name: String,
    /// The body type, which implements `Layout`.
    pub ty: String,
    /// Documentation.
    pub doc: Option<String>,
}

impl DispatchArm {
    /// `#[value(id)] name(ty)`, undocumented.
    #[must_use]
    pub fn new(id: i64, name: &str, ty: &str) -> Self {
        Self { id, name: String::from(name), ty: String::from(ty), doc: None }
    }

    /// Sets the documentation.
    #[must_use]
    pub fn with_doc(self, doc: &str) -> Self {
        Self { doc: Some(String::from(doc)), ..self }
    }
}
