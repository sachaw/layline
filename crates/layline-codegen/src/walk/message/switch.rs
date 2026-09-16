//! A switch: the arm an earlier value selects, optionally inside a window.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use super::Check;
use super::body::{Bind, Item, Own, Walk};
use super::cursor::windowed;
use super::recursion::bounded_check;
use super::table::Row;
use crate::walk::tokens::ident;
use crate::walk::{doc_attr, path, row};
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
        let d = doc_attr(doc);
        self.public_fields.extend(quote! { #d pub #ident: #cty, });
        self.binds.push(Bind { name: ident.clone(), ty: quote!(#cty), value: quote!(#ident) });
        let span = match window {
            None => row::Span::Chosen { on: on.clone() },
            Some(Len::Bytes(n)) => row::Span::ChosenIn { on: on.clone(), bits: *n as u64 * 8 },
            Some(Len::Field { by, .. }) => {
                row::Span::ChosenWithin { on: on.clone(), by: by.clone() }
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
            Discriminant::Field(f) => self.env.get(owner, "#[switch]", f)?,
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
                let lit = Literal::usize_unsuffixed(*n);
                let msg =
                    format!("union `{field}` is {n} bytes and its arm did not fill it exactly");
                quote! { ::core::assert!(out.len() - __chosen == #lit, #msg); }
            }
            _ => quote!(),
        };
        let (stated, msg) = match on {
            Discriminant::Field(f) => {
                let n = self.held(f, own);
                (
                    quote!(#root::WireInt::to_i64(&#n)),
                    format!(
                        "field `{f}`: switch `{field}` holds the open arm, but the catalogue defines an arm \
                         for this value of `{f}`. Decode would read that arm instead"
                    ),
                )
            }
            Discriminant::BodyLength => (
                quote!((out.len() - __chosen) as #i64),
                format!(
                    "field `{field}`: the open arm's byte length selects a defined arm. \
                     Decode would read that arm instead"
                ),
            ),
        };
        self.byte_steps.extend(quote! {
            let __chosen = out.len();
            #root::Choice::encode_into(&#held, out)?;
            #filled
            ::core::assert!(
                #root::Choice::discriminant(&#held).is_some()
                    || !<#cty as #root::Choice>::defines(#stated),
                #msg
            );
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
