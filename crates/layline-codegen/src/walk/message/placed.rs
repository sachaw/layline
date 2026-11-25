//! A record at an offset read from an earlier field, optionally absent, optionally in a window of
//! its own.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use super::body::{Bind, Item, Own, Walk};
use super::cursor::{seek_to, seek_to_stream, windowed, windowed_padded, write_padding};
use super::eval::blamed;

use super::repeat::element_step;
use super::table::{Row, deferred_type, span_of_kind, span_of_len};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{SlotOf, doc_attr, kind_ty, row};
use crate::{Absence, Error, Expr, Len, Origin, Segment};

/// `write`, the bytes of a record in a window that starts at `out.len() == #start`: a stream of
/// its own, so a record it places from the stream counts from the window's first byte.
pub(super) fn in_window(write: TokenStream, start: TokenStream) -> TokenStream {
    if super::mentions(&write, "__stream") {
        quote! {{
            let __stream: ::core::primitive::usize = #start;
            #write
        }}
    } else {
        write
    }
}

impl Walk<'_> {
    pub(super) fn placed(
        &mut self,
        item: &mut Item<'_>,
        own: Own,
        seg: &Segment,
        last: bool,
    ) -> Result<(), Error> {
        let Prelude { some, none, option, i64, err, .. } = self.root.prelude();
        let (root, owner) = (self.root, self.owner);
        let Segment::Placed { name: field, kind, at, from, absent, window, doc, .. } = seg else {
            unreachable!("just matched")
        };
        let ident = ident(field);
        let inner = kind_ty(kind, root);
        let ty = if absent.is_some() { quote!(#option<#inner>) } else { inner.clone() };
        let d = doc_attr(doc);
        self.public_fields.extend(quote! { #d pub #ident: #ty, });
        self.binds.push(Bind { name: ident.clone(), ty, value: quote!(#ident) });

        item.hold(super::refuse::held_refusal(kind, owner, root));
        let ctx = self.with_ctx(kind, field, own)?;
        let step = element_step(kind, field, ctx.as_ref(), root)?;
        let span = match window {
            Some(len) => span_of_len(len, field),
            None => span_of_kind(kind, field),
        };
        let mut row = Row::new(field, span).decoded_by(deferred_type(kind)).seek(at);
        if let Some(absent) = absent {
            row = row.when(match absent {
                Absence::ZeroOffset => row::Presence::Offset,
                Absence::ZeroLength { by, .. } => row::Presence::Length { by: by.clone() },
            });
        }
        self.rows.push(row);
        // A record in a window ends where the window does, however far it would read.
        let bounded = window.is_some() || matches!(absent, Some(Absence::ZeroLength { .. }));
        self.held_message(item, field, kind, last || bounded)?;

        let read = step.read_one(true, root);
        let parse = match (absent, window) {
            // `walk_segments` has already seeked.
            (None, None) => quote! {
                #read
                let #ident = __e;
            },
            (None, Some(len)) => {
                let n = self.window_len(len, "record", field)?;
                let stated = matches!(len, Len::Field { .. });
                let window = windowed(field, stated, read, root);
                quote! {
                    #n
                    #window
                    let #ident = __e;
                }
            }
            (Some(absent), _) => {
                let start = self.env.number(owner, "#[seek]", at)?;
                let seek = match from {
                    Origin::Message => seek_to(&quote!(__start), blamed(at, field), root),
                    Origin::Stream => seek_to_stream(&quote!(__start), blamed(at, field), root),
                };
                let (test, read) = match absent {
                    Absence::ZeroOffset => (quote!(__start), read),
                    Absence::ZeroLength { by, align } => {
                        let len = Expr::name(by);
                        let n = self.env.size_n(owner, "#[when]", &len, None)?;
                        let window = match align {
                            Some(align) => windowed_padded(by, *align, read, root),
                            None => windowed(by, true, read, root),
                        };
                        (self.env.number(owner, "#[when]", &len)?, quote!(#n #window))
                    }
                };
                quote! {
                    let __start = #start;
                    let #ident = if (#test) != 0 {
                        #seek
                        #read
                        #some(__e)
                    } else {
                        #none
                    };
                }
            }
        };
        self.parse_steps.extend(parse);

        let here = self.here_from(*from);
        let stamp = |of: SlotOf, by: &Expr, value: TokenStream| {
            self.stamp_or_agree(own, field, (of, true), by, value, None)
        };
        let write_one = step.write_one();
        let held = own.get(field);
        match (absent, window) {
            (None, None) => {
                let stamp = stamp(SlotOf::Position, at, here)?;
                self.byte_steps.extend(quote! {
                    #stamp
                    let __e = &#held;
                    #write_one
                });
            }
            (None, Some(len)) => {
                let at_stamp = stamp(SlotOf::Position, at, here)?;
                let written = quote!((out.len() - __span_start) as #i64);
                let len_stamp = match len {
                    Len::Field { by, .. } => stamp(SlotOf::Length, by, written)?,
                    Len::Bytes(n) => {
                        let lit = Literal::i64_unsuffixed(*n as i64);
                        quote! { #root::EncodeError::agree(#field, #written, #lit)?; }
                    }
                    other => {
                        return Err(Error::Refused(format!(
                            "{owner}: record `{field}` has window {other:?}. \
                             Use a fixed byte count or an earlier field"
                        )));
                    }
                };
                let write = in_window(write_one, quote!(__span_start));
                self.byte_steps.extend(quote! {
                    #at_stamp
                    let __span_start = out.len();
                    let __e = &#held;
                    #write
                    #len_stamp
                });
            }
            (Some(Absence::ZeroOffset), _) => {
                let present = stamp(SlotOf::Position, at, here)?;
                let absent = stamp(SlotOf::Position, at, quote!(0i64))?;
                self.byte_steps.extend(quote! {
                    match &#held {
                        #some(__e) => {
                            #present
                            #write_one
                        }
                        #none => {
                            #absent
                        }
                    }
                });
            }
            (Some(Absence::ZeroLength { by, align }), _) => {
                let pad = align.map(|align| {
                    let pad = write_padding(align);
                    quote! {
                        let __pad_from = __span_start;
                        #pad
                    }
                });
                let len = Expr::name(by);
                let at_present = stamp(SlotOf::Position, at, here.clone())?;
                let at_absent = stamp(SlotOf::Position, at, quote!(0i64))?;
                let len_present =
                    stamp(SlotOf::Length, &len, quote!((out.len() - __span_start) as #i64))?;
                let len_absent = stamp(SlotOf::Length, &len, quote!(0i64))?;
                let write = in_window(write_one, quote!(__span_start));
                self.byte_steps.extend(quote! {
                    match &#held {
                        #some(__e) => {
                            #at_present
                            let __span_start = out.len();
                            #write
                            if out.len() == __span_start {
                                return #err(#root::EncodeError::Empty { field: #field });
                            }
                            #len_present
                            #pad
                        }
                        #none => {
                            #at_absent
                            #len_absent
                        }
                    }
                });
            }
        }
        Ok(())
    }
}
