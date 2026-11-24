//! Adjacent framing: a length, count or presence flag immediately before what it describes.
//!
//! It is no field of the struct. Decode reads it into a local, and encode writes it from the
//! value it describes.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use super::cursor::{cap_guard, take};
use super::table::Row;
use crate::root::Prelude;
use crate::walk::{row, scalar_ty};
use crate::{Count, Len, Root, Scalar};

/// The integer before a collection or text, and its cap: a [`Count::Prefix`] or a
/// [`Len::Prefix`] window.
#[derive(Clone, Copy)]
pub(super) struct Prefix {
    pub(super) repr: Scalar,
    pub(super) cap: Option<usize>,
    /// Whether it counts bytes, rather than elements.
    pub(super) bytes: bool,
}

impl Prefix {
    /// The prefix of a collection, if it has one.
    pub(super) fn of_count(count: &Count) -> Option<Self> {
        match count {
            Count::Prefix { repr, cap } => Some(Self { repr: *repr, cap: *cap, bytes: false }),
            Count::Window(len) => Self::of_len(len),
            _ => None,
        }
    }

    /// The prefix of a length, if it is one.
    pub(super) fn of_len(len: &Len) -> Option<Self> {
        match len {
            Len::Prefix { repr, cap } => Some(Self { repr: *repr, cap: *cap, bytes: true }),
            _ => None,
        }
    }

    /// The `SEGMENTS` row's name: `<field>.len` or `<field>.count`.
    pub(super) fn row_name(self, field: &str) -> alloc::string::String {
        let what = if self.bytes { "len" } else { "count" };
        alloc::format!("{field}.{what}")
    }

    /// Its own row in the `SEGMENTS` table.
    pub(super) fn row(self, name: &str) -> Row<'_> {
        Row::new(name, row::Span::Fixed(self.repr.bits()))
    }

    fn width(self) -> Literal {
        Literal::usize_unsuffixed((self.repr.bits() / 8) as usize)
    }

    /// `let __n = ..;`: read the prefix at the cursor and move past it. A negative number, or one
    /// past the cap, is `Malformed` naming `field`.
    pub(super) fn read(self, field: &str, root: &Root) -> TokenStream {
        let Prelude { some, .. } = root.prelude();
        let (ty, width) = (scalar_ty(self.repr), self.width());
        let bytes = take(&quote!(#width), root);
        let cap = cap_guard(self.cap, field, root);
        quote! {
            let __n = {
                let __p = <__O as #root::__private::ByteOrder>::read::<#ty>(
                    #bytes.try_into().expect("sized"),
                );
                __at += #width;
                #root::__private::value::size(#some(#root::WireInt::to_i64(&__p)))
                    .ok_or(#root::ParseError::Malformed { field: #field, at: __at })?
            };
            #cap
        }
    }

    /// Write `n`, a `usize` known before the value is written. Refuses a number past the cap
    /// with `OverCap`, and one the prefix cannot hold with `Unfit`, both naming `field`.
    pub(super) fn write(self, field: &str, n: TokenStream, root: &Root) -> TokenStream {
        let checked = self.checked(field, root);
        quote! {
            {
                let __len: ::core::primitive::usize = #n;
                #checked
                out.push(&__prefix)?;
            }
        }
    }

    /// Reserve the prefix's bytes before a value whose length is known only once it is written,
    /// and fill them in after: the statements before, and the statements after.
    pub(super) fn reserve(self, field: &str, root: &Root) -> (TokenStream, TokenStream) {
        let width = self.width();
        let mark = mark(field);
        let checked = self.checked(field, root);
        (
            quote! {
                let #mark = out.len();
                out.pad(#width)?;
            },
            quote! {
                {
                    let __len = out.len() - #mark - #width;
                    #checked
                    out.written_mut()[#mark..#mark + #width].copy_from_slice(&__prefix);
                }
            },
        )
    }

    /// `let __prefix = ..;`: `__len` as the prefix's bytes, refused past the cap or the integer.
    fn checked(self, field: &str, root: &Root) -> TokenStream {
        let ty = scalar_ty(self.repr);
        let cap = self.cap.map(|cap| {
            let cap = Literal::usize_unsuffixed(cap);
            quote! { #root::EncodeError::cap(#field, __len, #cap)?; }
        });
        quote! {
            #cap
            let __v = ::core::primitive::i64::try_from(__len).unwrap_or(::core::primitive::i64::MAX);
            #root::EncodeError::fit::<#ty>(#field, __v, 0)?;
            let __prefix = <__O as #root::__private::ByteOrder>::write(
                <#ty as #root::WireInt>::from_i64(__v),
            );
        }
    }
}

/// The local holding the position of `field`'s reserved prefix.
fn mark(field: &str) -> Ident {
    format_ident!("__prefix_{}", field)
}

/// A byte-addressed `#[present]` flag: one byte, 0 or 1, immediately before the value.
pub(super) mod flag {
    use super::*;

    /// Its row in the `SEGMENTS` table, named `<field>.present`.
    pub(in super::super) fn row_name(field: &str) -> alloc::string::String {
        alloc::format!("{field}.present")
    }

    /// The row the value it gates takes: present when the flag's bit is set.
    pub(in super::super) fn gates(field: &str) -> row::Presence {
        row::Presence::Flag { field: row_name(field), mask: 1 }
    }

    /// `let __flag = ..;`: read the flag at the cursor and move past it, refusing any byte but
    /// 0 or 1 with `Malformed` naming `field`.
    pub(in super::super) fn read(field: &str, root: &Root) -> TokenStream {
        let Prelude { some, none, err, .. } = root.prelude();
        quote! {
            let __flag = {
                let __flag = match __body.get(__at) {
                    #some(0) => false,
                    #some(1) => true,
                    #some(_) => {
                        return #err(#root::ParseError::Malformed { field: #field, at: __at });
                    }
                    #none => {
                        return #err(#root::ParseError::Short {
                            need_bytes: __at + 1,
                            got_bytes: __body.len(),
                            at: __at,
                        });
                    }
                };
                __at += 1;
                __flag
            };
        }
    }

    /// Write the flag from `present`, a `bool` expression.
    pub(in super::super) fn write(present: TokenStream) -> TokenStream {
        quote! { out.push(&[::core::primitive::u8::from(#present)])?; }
    }
}
