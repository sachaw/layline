//! A record at an offset read from an earlier field, optionally absent.

use proc_macro2::TokenStream;
use quote::quote;

use super::body::{Bind, Item, Own, Walk};
use super::cursor::{seek_to, windowed};
use super::patch::{StampAt, stamp_slot};
use super::repeat::element_step;
use super::table::{Row, deferred_type, span_of_kind};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{SlotOf, doc_attr, kind_ty, row};
use crate::{Absence, By, Error, Segment};

impl Walk<'_> {
    pub(super) fn placed(
        &mut self,
        item: &mut Item<'_>,
        own: Own,
        seg: &Segment,
        last: bool,
    ) -> Result<(), Error> {
        let Prelude { some, none, option, i64, .. } = self.root.prelude();
        let (root, owner) = (self.root, self.owner);
        let Segment::Placed { name: field, kind, at, absent, doc, .. } = seg else {
            unreachable!("just matched")
        };
        let ident = ident(field);
        let inner = kind_ty(kind, root);
        let ty = if absent.is_some() { quote!(#option<#inner>) } else { inner.clone() };
        let d = doc_attr(doc);
        self.public_fields.extend(quote! { #d pub #ident: #ty, });
        self.binds.push(Bind { name: ident.clone(), ty, value: quote!(#ident) });

        let ctx = self.with_ctx(kind, own)?;
        let step = element_step(kind, item.endian, field, ctx.as_ref(), root)?;
        let mut row = Row::new(field, span_of_kind(kind)).decoded_by(deferred_type(kind)).seek(at);
        if let Some(absent) = absent {
            row = row.when(match absent {
                Absence::ZeroOffset => row::Presence::Offset,
                Absence::ZeroLength { by } => row::Presence::Length { by: by.clone() },
            });
        }
        self.rows.push(row);
        self.held_message(item, field, kind, last)?;

        let read = step.read_one(true, root);
        let parse = match absent {
            // `walk_segments` has already seeked.
            None => quote! {
                #read
                let #ident = __e;
            },
            Some(absent) => {
                let start = self.env.get(owner, "#[at]", at)?;
                let seek = seek_to(&start, at, root);
                let (test, read) = match absent {
                    Absence::ZeroOffset => (start, read),
                    Absence::ZeroLength { by } => {
                        let n = self.env.stated_n(owner, "#[when]", &By::field(by), None)?;
                        let window = windowed(by, true, read, root);
                        (self.env.get(owner, "#[when]", by)?, quote!(#n #window))
                    }
                };
                quote! {
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

        let here = self.here();
        let stamp = |of: SlotOf, into: &str, value: &TokenStream| {
            let at =
                StampAt { owner, coll: field, of, field: into, endian: item.endian, record: true };
            stamp_slot(&self.slots, at, value, root)
        };
        let write_one = step.write_one();
        let held = own.get(field);
        match absent {
            None => {
                let stamp = stamp(SlotOf::Position, at, &here)?;
                self.byte_steps.extend(quote! {
                    #stamp
                    let __e = &#held;
                    #write_one
                });
            }
            Some(Absence::ZeroOffset) => {
                let present = stamp(SlotOf::Position, at, &here)?;
                let absent = stamp(SlotOf::Position, at, &quote!(0i64))?;
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
            Some(Absence::ZeroLength { by }) => {
                let at_present = stamp(SlotOf::Position, at, &here)?;
                let at_absent = stamp(SlotOf::Position, at, &quote!(0i64))?;
                let len_present =
                    stamp(SlotOf::Length, by, &quote!((out.len() - __span_start) as #i64))?;
                let len_absent = stamp(SlotOf::Length, by, &quote!(0i64))?;
                let msg = format!(
                    "record `{field}` is present and measured zero bytes, and length field `{by}` \
                     would be 0, which means absent"
                );
                self.byte_steps.extend(quote! {
                    match &#held {
                        #some(__e) => {
                            #at_present
                            let __span_start = out.len();
                            #write_one
                            ::core::assert!(out.len() != __span_start, #msg);
                            #len_present
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
