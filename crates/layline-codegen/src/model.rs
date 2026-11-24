//! The layout model: generator input as plain data.

mod choice;
mod coverage;
mod derive;
mod dispatch;
mod enumeration;
mod expr;
mod field;
mod layout;
mod message;

pub use choice::{Arm, ChoiceDef, Discriminant};
pub use coverage::{CoverFrom, CoverTo, Coverage, Covered};
pub use derive::Derive;
pub use dispatch::{DispatchArm, DispatchDef};
pub use enumeration::{EnumDef, Variant};
pub(crate) use expr::Ty;
pub use expr::{Expr, Op, Test};
#[cfg(feature = "walk")]
pub(crate) use expr::{Inverse, Undo};
pub use field::{Field, Kind, Len, Position, Range, Scalar};
pub(crate) use layout::positioned;
pub use layout::{BitOrder, Container, Endian, LayoutDef, Unit};
pub use message::{Absence, Collection, Count, MessageDef, Param, Presence, Segment, split_ref};

/// One item of a model.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Item {
    /// An enumeration. It implements `FieldCodec` and `WireInt`, as `#[derive(FieldCodec)]`
    /// writes, so a field of it can select a switch's arm.
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
