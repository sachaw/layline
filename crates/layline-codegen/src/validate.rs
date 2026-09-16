//! Structural rules every model item must pass.

mod bits;
mod catalogue;
mod coverage;
mod layout;
mod message;
mod names;

use alloc::format;
use alloc::string::String;

pub(crate) use catalogue::{validate_dispatch, validate_enum};
#[cfg(feature = "walk")]
pub use layout::validate_container;
pub(crate) use layout::validate_layout;
pub(crate) use message::{validate_choice, validate_message};

use crate::{Field, Item, Kind};

/// Why an item failed validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    /// The fields do not cover every bit of the container.
    Tiling {
        /// The bits the fields cover.
        covered_bits: u64,
        /// The bits the container declares.
        declared_bits: u64,
    },
    /// A words-mode bit field crosses a 16-bit word boundary.
    CrossesWord(String),
    /// A field's `#[at]` position does not match its computed position.
    Stated(String),
    /// A reference names no earlier field.
    UnknownReference(String),
    /// A count, length, offset or switch refers to a field that is not an integer.
    NonIntegerReference(String),
    /// A [`Presence::Flag`](crate::Presence::Flag) refers to a field that is not a `bool`.
    NotAFlagReference(String),
    /// A dotted reference, as written, reaches into a field that is not a nested layout.
    NotANestedLayout(String),
    /// A segment follows one that reads to the end of the body.
    AfterOpenEnd(String),
    /// Two arms have the same value.
    DuplicateArm(u64),
    /// A refusal about one field.
    Field {
        /// The field name.
        at: String,
        /// The reason, without the `field` prefix that [`Display`](core::fmt::Display) adds.
        why: String,
    },
    /// Any other refusal.
    Other(String),
}

impl Invalid {
    /// A refusal about the field `at`.
    #[must_use]
    pub fn field(at: impl AsRef<str>, why: String) -> Self {
        Invalid::Field { at: String::from(at.as_ref()), why }
    }
}

impl core::fmt::Display for Invalid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Invalid::Tiling { covered_bits, declared_bits } => {
                write!(f, "the fields cover {covered_bits} of the container's {declared_bits} bits")
            }
            Invalid::CrossesWord(m)
            | Invalid::Stated(m)
            | Invalid::UnknownReference(m)
            | Invalid::NonIntegerReference(m)
            | Invalid::NotAFlagReference(m)
            | Invalid::NotANestedLayout(m)
            | Invalid::AfterOpenEnd(m)
            | Invalid::Other(m) => f.write_str(m),
            Invalid::Field { at, why } => write!(f, "field `{at}`: {why}"),
            Invalid::DuplicateArm(v) => write!(f, "two arms have the value {v}"),
        }
    }
}

impl core::error::Error for Invalid {}

/// Checks a model item.
///
/// Without the `walk` feature, type paths get only a lexical check, so generation can still
/// refuse a path that passes here.
///
/// # Errors
///
/// An [`Invalid`] naming the item.
pub fn validate(item: &Item) -> Result<(), Invalid> {
    match item {
        Item::Layout(l) => validate_layout(l),
        Item::Enum(e) => validate_enum(e),
        Item::Choice(c) => validate_choice(c),
        Item::Dispatch(d) => validate_dispatch(d),
        Item::Message(m) => validate_message(m),
        #[cfg(feature = "walk")]
        Item::Verbatim(_) => Ok(()),
    }
}

fn is_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    (first.is_alphabetic() || first == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
        && !matches!(name, "_" | "self" | "Self" | "super" | "crate")
}

#[cfg(feature = "walk")]
fn is_type_path(ty: &str) -> bool {
    syn::parse_str::<syn::Path>(ty).is_ok()
}

/// A lexical approximation of `syn`'s path parser.
#[cfg(not(feature = "walk"))]
fn is_type_path(ty: &str) -> bool {
    let ty = ty.trim();
    let Some(first) = ty.chars().next() else { return false };
    if !(first.is_alphabetic() || first == '_' || first == ':') {
        return false;
    }
    let mut depth = 0i32;
    for c in ty.chars() {
        match c {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            c if c.is_alphanumeric() || " _:,'&[];()-".contains(c) => {}
            _ => return false,
        }
    }
    depth == 0
}

/// Refuses a `what` called `name` that is not a Rust identifier.
fn ident(what: &str, name: &str) -> Result<(), Invalid> {
    if is_ident(name) {
        return Ok(());
    }
    Err(Invalid::Other(format!(
        "{what} `{name}` is not a Rust identifier. Use letters, digits and `_`, \
         not starting with a digit"
    )))
}

/// Refuses a type named by field `at` that is not a Rust type path.
fn type_path(at: &str, what: &str, ty: &str) -> Result<(), Invalid> {
    if is_type_path(ty) {
        return Ok(());
    }
    Err(Invalid::field(
        at,
        format!("{what} `{ty}` is not a Rust type path, such as `crate::wire::Head`"),
    ))
}

/// Checks every type path a kind names.
fn kind_paths(at: &str, kind: &Kind) -> Result<(), Invalid> {
    match kind {
        Kind::Codec { ty, .. } => type_path(at, "the codec type", ty),
        Kind::Nested { ty, .. } | Kind::NestedArray { ty, .. } => {
            type_path(at, "the nested layout", ty)
        }
        Kind::Var { ty } => type_path(at, "the `VarCodec` type", ty),
        Kind::Msg { ty, .. } => type_path(at, "the nested message", ty),
        // `check_value` refuses a blank algorithm with its own message.
        Kind::Checksum { algorithm, .. } if algorithm.trim().is_empty() => Ok(()),
        Kind::Checksum { algorithm, .. } => type_path(at, "the checksum algorithm", algorithm),
        Kind::Text { codec: Some(ty), .. } => type_path(at, "the `TextCodec` type", ty),
        Kind::Scalar(_) | Kind::Array(..) | Kind::Text { .. } => Ok(()),
    }
}

/// Checks a field's name and type paths.
fn field_names(f: &Field) -> Result<(), Invalid> {
    ident("field", &f.name)?;
    kind_paths(&f.name, &f.kind)
}
