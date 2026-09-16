//! Code generation from the model, shared by the derives and the emitter.

mod derived;
mod fixed;
mod message;
mod reference;
pub(crate) mod row;
mod tokens;

pub(crate) use derived::{Derived, SlotOf, back_patched, derivations};
pub use fixed::{DispatchParts, EnumParts, LayoutParts, dispatch_parts, enum_parts, layout_parts};
pub use message::{Check, ChoiceParts, MessageParts, choice_parts, message_parts};
pub(crate) use reference::references;
pub use row::field_rows;
pub use tokens::{WidthAttr, kind_ty, width_attr};
pub(crate) use tokens::{
    block_field, doc_attr, emit_field, emit_message_field, endian_arg, hidden_block, path,
    scalar_ty,
};

#[cfg(feature = "emit")]
pub(crate) use fixed::{emit_dispatch, emit_enum, emit_layout};
#[cfg(feature = "emit")]
pub(crate) use message::{emit_choice, emit_message};
#[cfg(feature = "emit")]
pub(crate) use reference::{Reference, Referent, defers_open_end};
#[cfg(feature = "emit")]
pub(crate) use tokens::{collection_ty, derive_attr};
