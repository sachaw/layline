//! Optional fields.

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};

use super::body::{Bind, Item, Own, Walk, needs_clone};
use super::cursor::read_layout;
use super::eval::condition;
use super::prefix::{Prefix, flag};
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
        if f.stated {
            return self.optional_framing(item, own, when, f);
        }
        let ident = ident(&f.name);
        let ty = kind_ty(&f.kind, root);
        let opt_ty = quote!(#option<#ty>);
        self.public_fields.extend(emit_message_field(f, true, root, &self.skip));
        self.binds.push(Bind { name: ident.clone(), ty: opt_ty, value: quote!(#ident) });

        let present = match when {
            Presence::Remaining => {
                let need = Literal::usize_unsuffixed(f.kind.width() as usize / 8);
                quote!(__body.len().saturating_sub(__at) >= #need)
            }
            other => self.presence_read(other, &f.name)?,
        };
        let gate = self.presence_rows(when, &f.name);
        let mut row = Row::new(&f.name, span_of_kind(&f.kind, &f.name))
            .decoded_by(deferred_type(&f.kind))
            .when(gate.clone());

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
            item.hold(held_refusal(&f.kind, owner, root));
            let ctx = self.with_ctx(&f.kind, own)?;
            let inner = value_read(&self.env, owner, &f.name, &f.kind, ctx.as_ref(), root)?;
            if let Kind::Text { len, .. } = &f.kind
                && let Some(p) = Prefix::of_len(len)
            {
                // The length sits between the flag and the text, in a row of its own.
                let name = p.row_name(&f.name);
                self.rows.push(p.row(&name).when(gate));
                self.rows.advance(Some(p.repr.bits()));
            }
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
        self.optional_write(own, when, (&f.name, quote!(__v)), write_one)?;

        self.held_message(item, &f.name, &f.kind, last)
    }
}

impl Walk<'_> {
    /// A `Stated<T>` on the wire only when its condition holds: decode reads it then, and holds
    /// zero otherwise, and encode writes the number its attribute works out then.
    fn optional_framing(
        &mut self,
        item: &mut Item<'_>,
        own: Own,
        when: &Presence,
        f: &crate::Field,
    ) -> Result<(), Error> {
        let Prelude { some, err, .. } = self.root.prelude();
        let (root, owner) = (self.root, self.owner);
        let Presence::When(e) = when else {
            return Err(Error::Refused(format!(
                "{owner}: framing field `{}` is present when a condition holds",
                f.name
            )));
        };
        let name = f.name.as_str();
        let ident = ident(name);
        let ty = kind_ty(&f.kind, root);
        self.public_fields.extend(emit_message_field(f, false, root, &self.skip));
        self.binds.push(Bind {
            name: ident.clone(),
            ty: quote!(#root::Stated<#ty>),
            value: quote!(#ident),
        });
        let bits = f.kind.width();
        if bits == 0 || !bits.is_multiple_of(8) {
            return Err(Error::Refused(format!(
                "{owner}: optional framing `{name}` is {bits} bits, not whole bytes"
            )));
        }
        let sub = format_ident!("__{}Opt{}", owner, item.opt_n);
        item.opt_n += 1;
        let len = Literal::usize_unsuffixed((bits / 8) as usize);
        item.items.extend(hidden_block(root, &sub, &len, &self.endian, block_field(f, root)));
        item.via.push(quote!(<#sub as #root::Layout>::ENCODE_FALLIBLE));
        let gate = self.presence_rows(when, name);
        self.rows.push(Row::new(name, span_of_kind(&f.kind, name)).when(gate).table(
            &sub,
            core::slice::from_ref(f),
            bits,
            item.endian,
        ));

        let present = self.env.condition(owner, "#[when]", e)?;
        let read = read_layout(&format_ident!("__o"), &sub, &len, root);
        self.parse_steps.extend(quote! {
            let #ident = if #present {
                #read
                #root::Stated::new(__o.#ident)
            } else {
                #root::Stated::default()
            };
        });
        self.env.bind(name, quote!(#ident.get()));

        let Some((_, d)) = self.derived.iter().find(|(df, _)| df == name) else {
            return Err(Error::Internal(format!(
                "{owner}: framing field `{name}` has no attribute that writes it"
            )));
        };
        let n = super::patch::derived_value(d, own, name, root)?;
        let fits = super::patch::fits(super::patch::Target::Field(&ty, 0), &n, name, root);
        let holds = condition(e, &|n, want| self.written(own, n, want), root)?;
        self.byte_steps.extend(quote! {
            match #holds {
                #some(true) => {
                    #fits
                    out.push(
                        &#sub { #ident: <#ty as #root::WireInt>::from_i64(#n) }
                            .encode_in::<__O>()?,
                    )?;
                }
                #some(false) => {}
                _ => return #err(#root::EncodeError::Inexact { field: #name, value: 0 }),
            }
        });
        Ok(())
    }

    /// Decode's test of whether the optional field `name` is present, as a `bool` expression. A
    /// byte-addressed flag before the field is read into `__flag` first, by statements this adds.
    pub(super) fn presence_read(
        &mut self,
        when: &Presence,
        name: &str,
    ) -> Result<TokenStream, Error> {
        let owner = self.owner;
        match when {
            Presence::When(e) => self.env.condition(owner, "#[when]", e),
            Presence::Prefix => {
                self.parse_steps.extend(flag::read(name, self.root));
                Ok(quote!(__flag))
            }
            Presence::Remaining => Err(Error::Refused(format!(
                "{owner}: `{name}` is present when bytes remain, which needs a fixed width"
            ))),
        }
    }

    /// The rows a presence test adds before the field's own, and the `when` of the field's row.
    pub(super) fn presence_rows(&mut self, when: &Presence, name: &str) -> row::Presence {
        match when {
            Presence::When(e) => row::Presence::of(e),
            Presence::Remaining => row::Presence::Remaining,
            Presence::Prefix => {
                let flag = flag::row_name(name);
                self.rows.push(Row::new(&flag, row::Span::Fixed(8)));
                self.rows.advance(Some(8));
                flag::gates(name)
            }
        }
    }

    /// Encode an optional field: its flag, or a check of a condition encode does not set, then
    /// `write` with the value bound to `bind`.
    pub(super) fn optional_write(
        &mut self,
        own: Own,
        when: &Presence,
        (name, bind): (&str, TokenStream),
        write: TokenStream,
    ) -> Result<(), Error> {
        let Prelude { some, .. } = self.root.prelude();
        let root = self.root;
        let (held, opt) = (own.by_ref(name), own.get(name));
        match when {
            Presence::Prefix => self.byte_steps.extend(flag::write(quote!(#opt.is_some()))),
            Presence::When(e) => {
                let sets = |d: &Derived| match d {
                    Derived::Flags { bits, .. } => bits.iter().any(|(_, of, _)| of == name),
                    Derived::Presence { of, .. } => of == name,
                    _ => false,
                };
                // A condition encode does not set from the field, it checks against it.
                if !self.writes(sets) {
                    let stated = condition(e, &|n, want| self.written(own, n, want), root)?;
                    self.byte_steps.extend(quote! {
                        {
                            let __value = ::core::primitive::i64::from(#opt.is_some());
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
            Presence::Remaining => {}
        }
        self.byte_steps.extend(quote! {
            if let #some(#bind) = #held {
                #write
            }
        });
        Ok(())
    }
}
