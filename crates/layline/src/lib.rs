#![doc = include_str!("../README.md")]
#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use layline_core::{
    BitCodec, BitReader, BitWriter, Buffer, Checksum, Choice, Dispatch, Extent, FieldCodec, Fixed,
    I, Layout, LayoutError, MAX_NESTING_DEPTH, Message, Overflow, ParseError, Slot, U, VarCodec,
    View, WireInt, audit, checksum, table,
};

#[cfg(feature = "derive")]
pub use layline_derive::{Dispatch, FieldCodec, Layout, Message};

#[cfg(feature = "alloc")]
pub use layline_core::{TextCodec, text};

#[doc(hidden)]
pub use layline_core::__private;

#[cfg(feature = "frame")]
pub use layline_core::frame;

#[cfg(feature = "num")]
pub use layline_core::num;
