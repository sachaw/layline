//! A switch: the arm an earlier value selects, optionally inside a window.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use super::Check;
use super::body::{Bind, Item, Own, Walk};
use super::cursor::windowed;
use super::eval::number;
use super::recursion::bounded_check;
use super::table::Row;
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{Derived, doc_attr, path, row};
use crate::{Discriminant, Error, Len, Root, Segment};

/// Assert that every arm of the union `ty` is exactly `bytes` bytes.
fn tiling_check(owner: &str, field: &str, ty: &str, bytes: usize, root: &Root) -> TokenStream {
    let p = path(ty);
    let bits = Literal::u64_unsuffixed(bytes as u64 * 8);
    let msg = format!(
        "`{owner}`: `{field}` gives union `{ty}` {bytes} bytes, but an arm is not {bytes} bytes wide. \
         Declare the arm's spare bytes as a field"
    );
    quote! {
        const _: () = ::core::assert!(
            #root::table::arms_fit(<#p as #root::Choice>::ARMS, #bits),
            #msg
        );
    }
}

impl Walk<'_> {
    pub(super) fn switch(
        &mut self,
        item: &mut Item<'_>,
        own: Own,
        seg: &Segment,
        last: bool,
    ) -> Result<(), Error> {
        let i64 = self.root.prelude().i64;
        let (root, owner) = (self.root, self.owner);
        let Segment::Switch { name: field, on, choice, window, doc, .. } = seg else {
            unreachable!("just matched")
        };
        let ident = ident(field);
        let cty: syn::Path = syn::parse_str(choice).map_err(|_| {
            Error::Refused(format!("{owner}: switch type `{choice}` is not a valid type path"))
        })?;
        item.via.push(quote!(<#cty as #root::Choice>::ENCODE_FALLIBLE));
        let d = doc_attr(doc);
        self.public_fields.extend(quote! { #d pub #ident: #cty, });
        self.binds.push(Bind { name: ident.clone(), ty: quote!(#cty), value: quote!(#ident) });
        let span = match window {
            None => row::Span::Chosen { on: row::On::of(on) },
            Some(Len::Bytes(n)) => row::Span::ChosenIn { on: row::On::of(on), bits: *n as u64 * 8 },
            Some(Len::Field { by, .. }) => {
                row::Span::ChosenWithin { on: row::On::of(on), by: row::By::of(by) }
            }
            Some(other) => {
                return Err(Error::Refused(format!(
                    "{owner}: switch `{field}` has window {other:?}. \
                     Use a fixed byte count or an earlier field"
                )));
            }
        };
        self.rows.push(Row::new(field, span).decoded_by(Some(choice)));

        let disc = match on {
            Discriminant::Field(on) => self.env.number(owner, "#[switch]", on)?,
            Discriminant::BodyLength => match window {
                None => quote!(((__body.len() - __at) as #i64)),
                Some(Len::Bytes(n)) => {
                    let n = Literal::usize_unsuffixed(*n);
                    quote!(#n as #i64)
                }
                Some(_) => {
                    return Err(Error::Refused(format!(
                        "{owner}: switch `{field}` selects on the body length but takes its window from a field. \
                         Select on a field, or drop the window"
                    )));
                }
            },
        };
        match window {
            None => self.parse_steps.extend(quote! {
                let (#ident, __used) =
                    <#cty as #root::Choice>::decode_with_nested(#disc, &__body[__at..], __depth)
                        .map_err(|__err| __err.rebased(__at))?;
                __at += __used;
            }),
            Some(len) => {
                let bound = self.window_len(len, "switch", field)?;
                let arm = quote! {
                    let (__e, __used) =
                        <#cty as #root::Choice>::decode_with_nested(#disc, __body, __depth)?;
                    __at += __used;
                };
                let window = windowed(field, matches!(len, Len::Field { .. }), arm, root);
                self.parse_steps.extend(quote! {
                    #bound
                    #window
                    let #ident = __e;
                });
            }
        }
        let held = own.get(field);
        let filled = match window {
            Some(Len::Bytes(n)) => {
                let lit = Literal::i64_unsuffixed(*n as i64);
                quote! { #root::EncodeError::agree(#field, (out.len() - __chosen) as #i64, #lit)?; }
            }
            _ => quote!(),
        };
        let Prelude { some, err, .. } = self.root.prelude();
        // The discriminant the fields state, which the arm must agree with unless encode writes
        // it from the arm.
        let mut agree = TokenStream::new();
        let stated = match on {
            Discriminant::Field(by) => {
                let selects =
                    |d: &Derived| matches!(d, Derived::Discriminant { of, .. } if of == field);
                if !self.writes(selects) {
                    let check = self.agree(own, field, quote!(__d), by)?;
                    agree.extend(quote! {
                        if let #some(__d) = #root::Choice::discriminant(&#held) {
                            #check
                        }
                    });
                }
                let n = number(by, &|name, want| self.written(own, name, want), root)?;
                quote! {
                    match #n {
                        #some(__n) => __n,
                        _ => return #err(#root::EncodeError::Inexact { field: #field, value: 0 }),
                    }
                }
            }
            Discriminant::BodyLength => quote!((out.len() - __chosen) as #i64),
        };
        if let Some(Len::Field { by, .. }) = window {
            let sizes = |d: &Derived| matches!(d, Derived::ArmLen { of, .. } if of == field);
            if !self.writes(sizes) {
                agree.extend(self.agree(own, field, quote!(out.len() - __chosen), by)?);
            }
        }
        self.byte_steps.extend(quote! {
            let __chosen = out.len();
            #root::Choice::encode_into(&#held, out)?;
            #filled
            #agree
            if #root::Choice::discriminant(&#held).is_none() {
                let __stated = #stated;
                if <#cty as #root::Choice>::defines(__stated) {
                    return #err(#root::EncodeError::Claimed { field: #field, value: __stated });
                }
            }
        });

        match window {
            None if !last => item.checks.push(Check {
                field: String::from(field),
                tokens: bounded_check(owner, field, choice, "switch", root),
            }),
            Some(Len::Bytes(n)) => item.checks.push(Check {
                field: String::from(field),
                tokens: tiling_check(owner, field, choice, *n, root),
            }),
            _ => {}
        }
        Ok(())
    }
}
