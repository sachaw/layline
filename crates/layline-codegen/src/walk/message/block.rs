//! A block of fixed fields, read and written through a hidden layout.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use super::body::{Bind, Item, Own, Walk, needs_clone};
use super::cursor::read_layout;
use super::patch::{
    Slot, SlotWrite, Target, derived_flag, derived_value, fits, nested_accessors, slot_ident,
    too_narrow,
};
use super::table::Row;
use crate::model::split_ref;
use crate::walk::tokens::ident;
use crate::walk::{Derived, block_field, emit_message_field, hidden_block, kind_ty, row};
use crate::{Error, Kind, Scalar, Segment};

/// `let <slot> = __block + <byte>;`, omitting `+ 0` to avoid `clippy::identity_op` in user code.
fn in_block(slot: &Ident, byte: u64) -> TokenStream {
    if byte == 0 {
        quote! { let #slot = __block; }
    } else {
        let pos = Literal::u64_unsuffixed(byte);
        quote! { let #slot = __block + #pos; }
    }
}

impl Walk<'_> {
    pub(super) fn block(
        &mut self,
        item: &mut Item<'_>,
        own: Own,
        seg: &Segment,
    ) -> Result<(), Error> {
        let (root, owner) = (self.root, self.owner);
        let Segment::Block(fields) = seg else { unreachable!("just matched") };
        let sub = format_ident!("__{}Block{}", owner, item.block_n);
        item.block_n += 1;
        let total: u64 = fields.iter().map(|f| f.kind.width()).sum();
        if !total.is_multiple_of(8) {
            return Err(Error::Refused(format!(
                "{owner}: block does not end on a byte boundary. Pad it to a whole byte"
            )));
        }
        let len = Literal::usize_unsuffixed((total / 8) as usize);
        self.rows.push(Row::new(&sub.to_string(), row::Span::Fixed(total)).table(
            &sub,
            fields,
            total,
            item.endian,
        ));
        let decls = fields.iter().map(|f| block_field(f, root));
        item.items.extend(hidden_block(root, &sub, &len, &self.endian, quote! { #(#decls)* }));
        for f in fields {
            self.public_fields.extend(emit_message_field(f, false, root));
        }

        let mut reserve = TokenStream::new();
        let mut rebuild = TokenStream::new();
        let mut at_marks = TokenStream::new();
        let mut locals: Vec<(String, Ident, bool)> = Vec::new();
        let mut bit_at = 0u64;
        for f in fields {
            let inside: Vec<(&str, &Derived)> = self
                .derived
                .iter()
                .filter_map(|(df, d)| match split_ref(df) {
                    (outer, Some(inner)) if outer == f.name => Some((inner, d)),
                    _ => None,
                })
                .collect();
            if !inside.is_empty() {
                let Kind::Nested { bytes, .. } = &f.kind else {
                    return Err(Error::Refused(format!(
                        "{owner}: a reference reads a field inside `{}`, which is {}. \
                         Only a nested layout has fields to reference",
                        f.name,
                        f.kind.describe(),
                    )));
                };
                if !bit_at.is_multiple_of(8) {
                    return Err(Error::Refused(format!(
                        "{owner}: nested layout `{}` starts at bit {bit_at} of its block. \
                         Align it to a byte boundary",
                        f.name
                    )));
                }
                let child = kind_ty(&f.kind, root);
                let local = format_ident!("__nested_{}", f.name);
                let held = own.get(&f.name);
                let stamps = inside
                    .iter()
                    .map(|(inner, d)| {
                        let id = ident(inner);
                        if let Some(present) = derived_flag(d, own) {
                            return quote! {
                                __h.#id = #root::__private::WireFlag::from_flag(#present);
                            };
                        }
                        let dotted = format!("{}.{inner}", f.name);
                        let n = derived_value(d, own, &dotted, root);
                        let (get, get_mut) = nested_accessors(owner, &f.name, inner);
                        let into =
                            Target::Nested { get: quote!(#get(&__h)), child: &child, name: inner };
                        let fits = fits(into, &quote!(__v), &too_narrow(&dotted, d.what()), root);
                        quote! {
                            {
                                let __v = #n;
                                #fits
                                *#get_mut(&mut __h) = #root::WireInt::from_i64(__v);
                            }
                        }
                    })
                    .collect::<Vec<_>>();
                rebuild.extend(quote! {
                    let #local = {
                        let mut __h = #held.clone();
                        #(#stamps)*
                        __h
                    };
                });
                let mut held_after = false;
                for (inner, d) in &inside {
                    let Some((which, of)) = d.reserves() else { continue };
                    held_after = true;
                    let slot = slot_ident(which, of);
                    reserve.extend(in_block(&slot, bit_at / 8));
                    self.slots.push(Slot {
                        coll: String::from(of),
                        of: which,
                        mark: slot,
                        width_bytes: *bytes,
                        write: SlotWrite::Nested {
                            value: local.clone(),
                            ty: child.clone(),
                            outer: f.name.clone(),
                            name: String::from(*inner),
                        },
                    });
                }
                locals.push((f.name.clone(), local, held_after));
            }
            if let Some((which, of)) =
                self.derived.iter().find(|(df, _)| df == &f.name).and_then(|(_, d)| d.reserves())
            {
                let what = which.what();
                let width = f.kind.width();
                if !bit_at.is_multiple_of(8) || width == 0 || !width.is_multiple_of(8) {
                    return Err(Error::Refused(format!(
                        "{owner}: {what} field `{}` is {width} bits at bit {bit_at} of its block. \
                         Encode fills it in after `{of}`, so it must be whole bytes on a byte boundary",
                        f.name
                    )));
                }
                let slot = slot_ident(which, of);
                reserve.extend(in_block(&slot, bit_at / 8));
                self.slots.push(Slot {
                    coll: String::from(of),
                    of: which,
                    mark: slot,
                    width_bytes: (width / 8) as usize,
                    write: SlotWrite::Scalar { ty: kind_ty(&f.kind, root) },
                });
            }
            let edges = [
                (bit_at, ("__at_", "__out_"), &self.marked.starts, ("from", "start")),
                (
                    bit_at + f.kind.width(),
                    ("__end_", "__outend_"),
                    &self.marked.ends,
                    ("through", "end"),
                ),
            ];
            for (bit, (decode, encode), wanted, (bound, edge)) in edges {
                if !wanted.contains(&f.name.as_str()) {
                    continue;
                }
                if !bit.is_multiple_of(8) {
                    return Err(Error::Refused(format!(
                        "{owner}: a checksum covers bytes {bound} `{}`, but its {edge} is at bit {bit} of the block. \
                         Bound the range at a field on a byte boundary",
                        f.name,
                    )));
                }
                let pos = Literal::u64_unsuffixed(bit / 8);
                let decode = format_ident!("{}{}", decode, f.name);
                let encode = format_ident!("{}{}", encode, f.name);
                at_marks.extend(quote! { let #decode = __at + #pos; });
                reserve.extend(quote! { let #encode = __block + #pos; });
            }
            bit_at += f.kind.width();
        }

        let sub_var = format_ident!("__b{}", item.block_n - 1);
        self.parse_steps.extend(at_marks);
        self.parse_steps.extend(read_layout(&sub_var, &sub, &len, root));
        for f in fields {
            let fname = ident(&f.name);
            self.binds.push(Bind {
                name: fname.clone(),
                ty: kind_ty(&f.kind, root),
                value: quote!(#sub_var.#fname),
            });
            if f.kind.is_integer() {
                self.env.bind(&f.name, quote!(#sub_var.#fname));
            } else if matches!(f.kind, Kind::Scalar(Scalar::Bool)) {
                self.env.bind_flag(&f.name, quote!(#sub_var.#fname));
            } else if matches!(f.kind, Kind::Nested { .. }) {
                self.env.bind_nested(&f.name, quote!(#sub_var.#fname));
            }
        }
        let inits = fields.iter().map(|f| {
            let fname = ident(&f.name);
            let ty = kind_ty(&f.kind, root);
            if let Some((_, local, held_after)) = locals.iter().find(|(n, ..)| *n == f.name) {
                let take = if *held_after { quote!(.clone()) } else { quote!() };
                return quote! { #fname: #local #take, };
            }
            match self.derived.iter().find(|(df, _)| df == &f.name).map(|(_, d)| d) {
                Some(d) => match derived_flag(d, own) {
                    Some(present) => quote! {
                        #fname: <#ty as #root::__private::WireFlag>::from_flag(#present),
                    },
                    None => {
                        let n = derived_value(d, own, &f.name, root);
                        let narrow = too_narrow(&f.name, d.what());
                        let fits = fits(Target::Field(&ty, 0), &n, &narrow, root);
                        quote! {
                            #fname: {
                                #fits
                                <#ty as #root::WireInt>::from_i64(#n)
                            },
                        }
                    }
                },
                None => {
                    let clone = if needs_clone(&f.kind) { quote!(.clone()) } else { quote!() };
                    let held = own.get(&f.name);
                    quote! { #fname: #held #clone, }
                }
            }
        });
        let mark = if reserve.is_empty() {
            quote!()
        } else {
            quote! { let __block = out.len(); }
        };
        self.byte_steps.extend(quote! {
            #mark
            #rebuild
            out.push(&#sub { #(#inits)* }.encode())?;
            #reserve
        });
        Ok(())
    }
}
