//! Dispatches: bodies selected by an id the enclosing message has already read.

use alloc::string::String;
use alloc::vec::Vec;

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
}

impl DispatchDef {
    /// A dispatch with its fallback variant named `Unknown`.
    #[must_use]
    pub fn new(name: &str, id: Scalar, arms: Vec<DispatchArm>) -> Self {
        Self { name: String::from(name), id, arms, other: String::from("Unknown"), doc: None }
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
