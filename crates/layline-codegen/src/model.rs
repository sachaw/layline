//! The layout model: generator input as plain data.

mod choice;
mod coverage;
mod derive;
mod dispatch;
mod enumeration;
mod field;
mod layout;
mod message;

pub use choice::{Arm, ChoiceDef, Discriminant};
pub use coverage::{CoverFrom, CoverTo, Coverage, Covered};
pub use derive::Derive;
pub use dispatch::{DispatchArm, DispatchDef};
pub use enumeration::{EnumDef, Variant};
pub use field::{By, Field, Kind, Len, Range, Scalar, Stated};
pub(crate) use layout::positioned;
pub use layout::{BitOrder, Container, Endian, LayoutDef, Unit};
pub use message::{Absence, Collection, Count, MessageDef, Param, Presence, Segment, split_ref};

/// One item of a model.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Item {
    /// An enumeration.
    Enum(EnumDef),
    /// A fixed-size layout.
    Layout(LayoutDef),
    /// The arms of a [`Segment::Switch`].
    Choice(ChoiceDef),
    /// Bodies selected by an id the enclosing message has already read.
    Dispatch(DispatchDef),
    /// A variable-length message.
    Message(MessageDef),
    /// Rust code copied into the output as is, such as accessors, trait impls and constants.
    ///
    /// Refused if it declares a codec, since the model generates those.
    #[cfg(feature = "walk")]
    Verbatim(proc_macro2::TokenStream),
}
