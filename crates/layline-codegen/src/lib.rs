//! Rust source generation for layline.
//!
//! Without features, the crate is `no_std` and provides the model types and [`validate()`].
//!
//! | Feature | Enables | Default |
//! |---|---|---|
//! | `walk` | [`Root`] and [`Error`]. Adds `proc-macro2`, `quote` and `syn`. | No |
//! | `emit` | [`emit`], which writes source files. Implies `walk`. | Yes |
//!
//! The stable API is the model types, [`validate()`], [`Error`], [`Root`] and [`emit`].
//! Model structs are `#[non_exhaustive]`. Build them with `new` and the `with_*` methods.

#![cfg_attr(not(feature = "walk"), no_std)]

extern crate alloc;

mod model;
mod validate;
#[cfg(feature = "walk")]
mod walk;

#[cfg(feature = "emit")]
mod audit;
#[cfg(feature = "emit")]
pub mod emit;
#[cfg(feature = "walk")]
mod root;
#[cfg(feature = "emit")]
mod source;

pub use layline_core::table::StatedUnit;
pub use model::{
    Absence, Arm, BitOrder, By, ChoiceDef, Collection, Container, Count, CoverFrom, CoverTo,
    Coverage, Covered, Discriminant, DispatchArm, DispatchDef, Endian, EnumDef, Field, Item, Kind,
    LayoutDef, Len, MessageDef, Param, Presence, Range, Scalar, Segment, Stated, Unit, Variant,
};
#[cfg(feature = "walk")]
pub use root::{Root, Spelling};
pub use validate::{Invalid, validate};

/// Used by `layline-derive`. Not covered by semver.
#[cfg(feature = "walk")]
#[doc(hidden)]
pub mod __derive {
    pub use crate::model::split_ref;
    pub use crate::root::respan;
    pub use crate::validate::validate_container;
    pub use crate::walk::row::FieldDef;
    pub use crate::walk::{
        Check, ChoiceParts, DispatchParts, EnumParts, LayoutParts, MessageParts, WidthAttr,
        choice_parts, dispatch_parts, enum_parts, field_rows, kind_ty, layout_parts, message_parts,
        width_attr,
    };
}

/// Why a model could not be turned into code.
#[cfg(feature = "walk")]
#[derive(Debug)]
pub enum Error {
    /// An item is invalid. Holds the item name and the reason.
    ///
    /// Unparsable names, type paths and derive paths are reported here too.
    Invalid(String, Invalid),
    /// Each item is valid, but the model as a whole is not.
    ///
    /// For example, a type of infinite size, or a verbatim item that declares a codec.
    Refused(String),
    /// The generated source did not parse. This is a bug in `layline-codegen`.
    Internal(String),
}

#[cfg(feature = "walk")]
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Invalid(name, why) => write!(f, "{name}: {why}"),
            Self::Refused(why) => f.write_str(why),
            Self::Internal(why) => write!(f, "internal error: {why}"),
        }
    }
}

#[cfg(feature = "walk")]
impl core::error::Error for Error {}
