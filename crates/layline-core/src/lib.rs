//! Runtime for layline.
//!
//! `no_std`, with no dependencies.
//!
//! - [`Layout`]: a fixed-size type, described by a [`FieldDef`](table::FieldDef) table.
//! - [`Message`]: a variable-length type, described by a [`SegmentDef`](table::SegmentDef) table.
//! - [`audit`]: prints those tables.
//! - [`frame`] (feature `frame`): stream framing.
//! - [`num`] (feature `num`): LEB128, Exp-Golomb and reserved values.

#![no_std]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod audit;
pub mod checksum;

#[cfg(feature = "frame")]
pub mod frame;
#[cfg(feature = "num")]
pub mod num;
#[cfg(feature = "alloc")]
pub mod text;

mod bits;
mod bound;
mod buffer;
mod codec;
mod dispatch;
mod extent;
mod field;
mod int;
mod mask;
mod message;
mod row;
mod segment;
mod slot;
mod var;
mod view;
mod word;

#[cfg(test)]
pub(crate) mod buf {
    use core::fmt;

    pub(crate) struct Buf([u8; 4096], usize);

    impl fmt::Write for Buf {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.0[self.1..self.1 + s.len()].copy_from_slice(s.as_bytes());
            self.1 += s.len();
            Ok(())
        }
    }

    impl Buf {
        pub(crate) fn of(what: impl fmt::Display) -> Self {
            use fmt::Write;
            let mut b = Buf([0; 4096], 0);
            write!(b, "{what}").unwrap();
            b
        }

        pub(crate) fn text(&self) -> &str {
            core::str::from_utf8(&self.0[..self.1]).unwrap()
        }
    }
}

/// Table row types, and the `const fn`s that check them.
///
/// Rows borrow for `'a` rather than `'static`, so a code generator can build them from its own model.
pub mod table {
    pub use crate::field::{
        ConstDef, FieldDef, ParamDef, RangeDef, StatedDef, StatedUnit, TypeDef, check_layout,
        field_width, type_is,
    };
    pub use crate::segment::{
        ArmCover, ArmDef, By, CoverDef, CoverEdge, Discriminant, Presence, SegmentDef, Span, Start,
        arms_fit, fixed_bits,
    };
}

/// Items used by generated code.
///
/// Generated code names only `layline::__private::..` and `::core::..` paths, so a user's own
/// `Vec`, `String` or `Result` cannot shadow them.
#[doc(hidden)]
pub mod __private {
    pub use ::core;

    #[cfg(feature = "alloc")]
    pub use alloc::{boxed::Box, string::String, vec, vec::Vec};

    pub use crate::bound::{Nestable, WireFlag, fits_in_field};
    pub use crate::field::assert_layout;
    pub use crate::word::BitWord;

    #[cfg(feature = "alloc")]
    pub use crate::text::{TextCodec, Utf8};

    /// Requires a `#[checksum(..)]` field's type to be the algorithm's [`Checksum::Output`].
    ///
    /// [`Checksum::Output`]: crate::Checksum::Output
    #[diagnostic::on_unimplemented(
        message = "a `{Self}` checksum does not fit a `{T}` field",
        label = "this field must be `<{Self} as layline::Checksum>::Output`",
        note = "a checksum field must have the algorithm's output type. \
                Change the field's type, or use an algorithm whose output is `{T}`"
    )]
    pub trait ChecksumFits<T>: crate::sealed::ChecksumFits {}

    #[diagnostic::do_not_recommend]
    impl<A> crate::sealed::ChecksumFits for A where A: crate::Checksum {}

    #[diagnostic::do_not_recommend]
    impl<A, T> ChecksumFits<T> for A where A: crate::Checksum<Output = T> {}
}

/// Seals the bound traits so only the blanket impls exist.
mod sealed {
    pub trait Nestable {}
    pub trait ChecksumFits {}
    pub trait WireFlag {}
    pub trait CrcWord {}
}

pub use bits::{BitCodec, BitReader, BitWriter};
pub use bound::WireInt;
pub use buffer::{Buffer, Fixed, Overflow};
pub use checksum::Checksum;
pub use codec::FieldCodec;
pub use dispatch::Dispatch;
pub use extent::Extent;
pub use field::{Layout, LayoutError};
pub use int::{I, U};
pub use message::{MAX_NESTING_DEPTH, ParseError};
pub use segment::{Choice, Message};
pub use slot::Slot;
pub use var::VarCodec;
pub use view::View;

#[cfg(feature = "alloc")]
pub use text::TextCodec;
