//! Optional fields.

use proc_macro2::Literal;
use quote::{format_ident, quote};

use super::body::{Bind, Item, Own, Walk, needs_clone};
use super::cursor::read_layout;
use super::table::{Row, deferred_type, span_of_kind};
use super::value::{value_read, value_write};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{block_field, emit_message_field, hidden_block, kind_ty, row};
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
            Presence::Mask { field: flag, mask } => {
                let read = self.env.get(owner, "#[when]", flag)?;
                let bit = Literal::i64_unsuffixed(*mask as i64);
                quote!(((#read) & #bit) != 0)
            }
            Presence::Flag { field: flag } => self.env.get_flag(owner, "#[when]", flag)?,
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
                Presence::Remaining => row::Presence::Remaining,
                Presence::Mask { field: flag, mask } => {
                    row::Presence::Flag { field: flag.clone(), mask: *mask }
                }
                Presence::Flag { field: flag } => {
                    row::Presence::Flag { field: flag.clone(), mask: 1 }
                }
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
            row = row.table(&sub, core::slice::from_ref(f), bits, item.endian);
            let take = if needs_clone(&f.kind) { quote!(__v.clone()) } else { quote!(*__v) };
            let read = read_layout(&format_ident!("__o"), &sub, &len, root);
            (
                quote! {
                    #read
                    #some(__o.#ident)
                },
                quote! { out.push(&#sub { #ident: #take }.encode())?; },
            )
        } else {
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
        self.byte_steps.extend(quote! {
            if let #some(__v) = #held {
                #write_one
            }
        });

        self.held_message(item, &f.name, &f.kind, last)
    }
}
