//! Optional fields.

use proc_macro2::Literal;
use quote::{format_ident, quote};

use super::body::{Bind, Item, Own, Walk, needs_clone};
use super::cursor::read_layout;
use super::eval::condition;
use super::refuse::held_refusal;
use super::table::{Row, deferred_type, span_of_kind};
use super::value::{value_read, value_write};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{Derived, block_field, emit_message_field, hidden_block, kind_ty, row};
use crate::{Error, Kind, Presence, Segment};

impl Walk<'_> {
    pub(super) fn opt(
        &mut self,
        item: &mut Item<'_>,
        own: Own,
        seg: &Segment,
        last: bool,
    ) -> Result<(), Error> {
        let Prelude { some, none, option, .. } = self.root.prelude();
        let (root, owner) = (self.root, self.owner);
        let Segment::Opt { when, field: f } = seg else { unreachable!("just matched") };
        let ident = ident(&f.name);
        let ty = kind_ty(&f.kind, root);
        let opt_ty = quote!(#option<#ty>);
        self.public_fields.extend(emit_message_field(f, true, root, &self.skip));
        self.binds.push(Bind { name: ident.clone(), ty: opt_ty, value: quote!(#ident) });

        let present = match when {
            Presence::When(e) => self.env.condition(owner, "#[when]", e)?,
            Presence::Remaining => {
                let need = Literal::usize_unsuffixed(f.kind.width() as usize / 8);
                quote!(__body.len().saturating_sub(__at) >= #need)
            }
            Presence::Bit => {
                return Err(Error::Refused(format!(
                    "{owner}: `{}` uses an inline presence bit, but this message is byte-addressed. \
                     Make the message bit-addressed, or absent the field on a flag",
                    f.name
                )));
            }
        };
        let mut row = Row::new(&f.name, span_of_kind(&f.kind))
            .decoded_by(deferred_type(&f.kind))
            .when(match when {
                Presence::When(e) => row::Presence::of(e),
                Presence::Remaining => row::Presence::Remaining,
                Presence::Bit => row::Presence::Bit,
            });

        let fixed = !matches!(f.kind, Kind::Var { .. } | Kind::Msg { .. } | Kind::Text { .. });
        let (parse_one, write_one) = if fixed {
            let bits = f.kind.width();
            if bits == 0 || !bits.is_multiple_of(8) {
                return Err(Error::Refused(format!(
                    "{owner}: optional field `{}` is {bits} bits, not whole bytes. \
                     Wrap it in a `#[derive(Layout)]` type and make that optional",
                    f.name
                )));
            }
            let sub = format_ident!("__{}Opt{}", owner, item.opt_n);
            item.opt_n += 1;
            let len = Literal::usize_unsuffixed((bits / 8) as usize);
            item.items.extend(hidden_block(root, &sub, &len, &self.endian, block_field(f, root)));
            item.via.push(quote!(<#sub as #root::Layout>::ENCODE_FALLIBLE));
            row = row.table(&sub, core::slice::from_ref(f), bits, item.endian);
            let take = if needs_clone(&f.kind) { quote!(__v.clone()) } else { quote!(*__v) };
            let read = read_layout(&format_ident!("__o"), &sub, &len, root);
            (
                quote! {
                    #read
                    #some(__o.#ident)
                },
                quote! { out.push(&#sub { #ident: #take }.encode_in::<__O>()?)?; },
            )
        } else {
            item.via.extend(held_refusal(&f.kind, owner, false, root));
            let ctx = self.with_ctx(&f.kind, own)?;
            let inner = value_read(&self.env, owner, &f.name, &f.kind, ctx.as_ref(), root)?;
            (
                quote! { #inner #some(#ident) },
                value_write(&f.name, &f.kind, quote!(__v), quote!(__v), ctx.as_ref(), root)?,
            )
        };
        self.rows.push(row);

        self.parse_steps.extend(quote! {
            let #ident = if #present {
                #parse_one
            } else {
                #none
            };
        });
        let held = own.by_ref(&f.name);
        // A condition encode does not set from the field, it checks against it.
        if let Presence::When(e) = when {
            let name = f.name.as_str();
            let sets = |d: &Derived| match d {
                Derived::Flags { bits, .. } => bits.iter().any(|(_, of, _)| of == name),
                Derived::Presence { of, .. } => of == name,
                _ => false,
            };
            if !self.writes(sets) {
                let root = self.root;
                let stated = condition(e, &|n, want| self.written(own, n, want), root)?;
                self.byte_steps.extend(quote! {
                    {
                        let __value = ::core::primitive::i64::from((#held).is_some());
                        match #stated {
                            #some(__stated) => #root::EncodeError::agree(
                                #name,
                                __value,
                                ::core::primitive::i64::from(__stated),
                            )?,
                            _ => {
                                return ::core::result::Result::Err(
                                    #root::EncodeError::Inexact { field: #name, value: __value },
                                );
                            }
                        }
                    }
                });
            }
        }
        self.byte_steps.extend(quote! {
            if let #some(__v) = #held {
                #write_one
            }
        });

        self.held_message(item, &f.name, &f.kind, last)
    }
}
