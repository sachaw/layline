//! A catalogue: its arms, dispatch, and one segment table per arm.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use super::Check;
use super::body::{Bind, Item, Own, Walked, walk_segments};
use super::cursor::too_deep;
use super::recursion::{names_self, open_ended_expr};
use super::table::covers_table;
#[cfg(feature = "emit")]
use crate::Derive;
use crate::root::Prelude;
#[cfg(feature = "emit")]
use crate::walk::derive_attr;
use crate::walk::tokens::ident;
use crate::walk::{doc_attr, references, row};
use crate::{ChoiceDef, Error, Root};

/// A generated [`ChoiceDef`], in separate pieces. The `emit` backend writes the enum declaration.
pub struct ChoiceParts {
    /// Variant declarations in arm order, open variant last.
    pub variants: TokenStream,
    /// Assertions about types known only by name.
    pub checks: Vec<Check>,
    /// Hidden per-arm layouts, and the constants the arms assert.
    pub blocks: TokenStream,
    /// `impl Choice for Name { const ARMS; fn decode_with_nested; fn encode_into }`.
    pub codec: TokenStream,
}

/// The enum, its `impl Choice`, and each arm's rows.
///
/// # Errors
///
/// Whatever [`lower_choice`] refuses.
#[cfg(feature = "emit")]
pub(crate) fn emit_choice(
    c: &ChoiceDef,
    derives: &[Derive],
    root: &Root,
    cyclic: &[(String, bool)],
) -> Result<(TokenStream, Vec<row::ArmDef>), Error> {
    let name = ident(&c.name);
    let der = derive_attr(derives, &c.derives)?;
    let doc = doc_attr(&c.doc);
    let (ChoiceParts { variants, checks, blocks, codec }, arms) = lower_choice(c, root, cyclic)?;
    let checks = checks.into_iter().map(|c| c.tokens);
    Ok((
        quote! {
            #doc
            #der
            pub enum #name {
                #variants
            }

            #(#checks)*

            #blocks

            #codec
        },
        arms,
    ))
}

/// Generate a catalogue's parts.
///
/// # Errors
///
/// [`Error::Invalid`] when [`validate`](crate::validate()) refuses it. [`Error::Refused`] for a
/// shape that cannot be generated.
pub fn choice_parts(c: &ChoiceDef, root: &Root) -> Result<ChoiceParts, Error> {
    lower_choice(c, root, &[]).map(|(parts, _)| parts)
}

/// [`choice_parts`] plus each arm's rows. `cyclic` says which types on `c`'s cycles are open-ended.
fn lower_choice(
    c: &ChoiceDef,
    root: &Root,
    cyclic: &[(String, bool)],
) -> Result<(ChoiceParts, Vec<row::ArmDef>), Error> {
    let Prelude { ok, err, some, none, option, result, vec, u8, u32, usize, i64, bool, .. } =
        root.prelude();
    crate::validate::validate_choice(c).map_err(|why| Error::Invalid(c.name.clone(), why))?;
    let name = ident(&c.name);
    let mut item = Item::new(&c.name, c.endian, root, &[]);

    let mut variants = TokenStream::new();
    let mut decode_arms = TokenStream::new();
    let mut encode_arms = TokenStream::new();
    let mut which_arms = TokenStream::new();
    let mut open_terms: Vec<TokenStream> = Vec::new();
    let mut arm_tables: Vec<TokenStream> = Vec::new();
    let mut arm_rows: Vec<row::ArmDef> = Vec::new();

    if c.other.is_none()
        && !c.arms.is_empty()
        && c.arms.iter().all(|arm| {
            references(&arm.body)
                .into_iter()
                .any(|r| !r.avoidable && names_self(r.to.name(), &c.name))
        })
    {
        return Err(Error::Refused(format!(
            "`{}`: every arm always contains a `{}`, so the wire never ends. \
             Let one arm avoid it, or add an `#[other]` arm",
            c.name, c.name,
        )));
    }

    for arm in &c.arms {
        let variant = ident(&arm.name);
        let Walked { base, binds, parse, build, bytes, table, rows, .. } =
            walk_segments(&mut item, &arm.body, Own::Arm)?;

        let (decl, ctor, pat) = match binds.as_slice() {
            [] => (quote!(,), quote!(), quote!()),
            [Bind { name, ty, value }] => (quote!((#ty),), quote!((#value)), quote!((#name))),
            many => {
                let decls = many.iter().map(|Bind { name, ty, .. }| quote!(#name: #ty,));
                let ids = many.iter().map(|b| &b.name);
                (quote!({ #(#decls)* },), quote!({ #build }), quote!({ #(#ids),* }))
            }
        };
        let doc = doc_attr(&arm.doc);
        variants.extend(quote! { #doc #variant #decl });

        let value = Literal::i64_unsuffixed(arm.value as i64);
        let name = arm.name.as_str();
        arm_rows.push(row::ArmDef {
            name: String::from(name),
            value: Some(arm.value as i64),
            segments: rows,
        });
        arm_tables.push(quote! {
            #root::table::ArmDef::new(#name, #some(#value), &[#table]),
        });
        decode_arms.extend(quote! {
            #value => {
                let mut __at = 0usize;
                let mut __hi = 0usize;
                #parse
                #ok((Self::#variant #ctor, __hi))
            }
        });
        encode_arms.extend(quote! {
            #[allow(unused_variables)]
            Self::#variant #pat => { #base #bytes }
        });
        which_arms.extend(quote! {
            Self::#variant { .. } => #some(#value),
        });

        let arm_open = open_ended_expr(&arm.body, &c.name, cyclic, root);
        if arm_open.to_string() != "false" {
            open_terms.push(arm_open);
        }
    }

    let what = c.name.as_str();
    let (decode_tail, encode_tail, which_tail) = if let Some(other_name) = c.other.as_deref() {
        let other = ident(other_name);
        let (payload, row_span, span_def, decode) = match c.other_bytes {
            Some(n) => {
                let len = Literal::usize_unsuffixed(n);
                let bits = Literal::u64_unsuffixed(n as u64 * 8);
                (
                    quote!([#u8; #len]),
                    row::Span::Fixed(n as u64 * 8),
                    quote!(#root::table::Span::Fixed(#bits)),
                    quote! {
                        _ => {
                            let __raw: [#u8; #len] = __body
                                .get(..#len)
                                .and_then(|__s| {
                                    <[#u8; #len]>::try_from(__s).ok()
                                })
                                .ok_or(#root::ParseError::Short {
                                    need_bytes: #len,
                                    got_bytes: __body.len(),
                                    at: 0,
                                })?;
                            #ok((Self::#other(__raw), #len))
                        }
                    },
                )
            }
            None => {
                open_terms = vec![quote!(true)];
                (
                    quote!(#vec<#u8>),
                    row::Span::Fill { cap: None },
                    quote!(#root::table::Span::Fill { cap: ::core::option::Option::None }),
                    quote! { _ => #ok((Self::#other(__body.to_vec()), __body.len())), },
                )
            }
        };
        variants.extend(quote! {
            /// The bytes of an undefined discriminant, kept verbatim so they re-encode unchanged.
            #other(#payload),
        });
        arm_rows.push(row::ArmDef {
            name: String::from(other_name),
            value: None,
            segments: vec![row::SegmentDef {
                name: String::from(other_name),
                start: row::Start::At(0),
                span: row_span,
                when: None,
                decoded_by: None,
                table: None,
                covers: None,
            }],
        });
        arm_tables.push(quote! {
            #root::table::ArmDef::new(#other_name, #none, &[
                    #root::table::SegmentDef::new(#other_name, #root::table::Start::At(0), #span_def, None, #none, &[]),
                ]),
        });
        (
            decode,
            quote! { Self::#other(__raw) => out.push(__raw)?, },
            quote! { Self::#other(_) => #none, },
        )
    } else {
        (
            quote! { _ => #err(#root::ParseError::Malformed { field: #what, at: 0 }), },
            quote!(),
            quote!(),
        )
    };

    let open_ended = if open_terms.iter().any(|t| t.to_string() == "true") {
        quote!(true)
    } else if open_terms.is_empty() {
        quote!(false)
    } else {
        quote!(#(#open_terms)||*)
    };

    let per_arm: Vec<TokenStream> = arm_rows
        .iter()
        .filter_map(|arm| {
            let covers = covers_table(&arm.segments, root)?;
            let name = &arm.name;
            Some(quote! {
                #root::table::ArmCover::new(#name, &[#covers]),
            })
        })
        .collect();
    let covered = (!per_arm.is_empty()).then(|| {
        quote! {
            /// The bytes each arm's `#[checksum]` fields cover.
            const COVERED: &'static [#root::table::ArmCover<'static>] = &[#(#per_arm)*];
        }
    });

    let (owner, str_ty) = (c.name.as_str(), root.prelude().primitive("str"));
    let too_deep = too_deep(root);
    let codec = quote! {
        impl #root::Choice for #name {
            const NAME: &'static #str_ty = #owner;

            /// The arms in declaration order, open arm last, each with its value and segment table.
            ///
            /// Positions count from the arm's first bit.
            const ARMS: &'static [#root::table::ArmDef<'static>] = &[#(#arm_tables)*];

            #covered

            const OPEN_ENDED: #bool = #open_ended;

            fn decode_with_nested(
                discriminant: #i64,
                body: &[#u8],
                __depth: #u32,
            ) -> #result<(Self, #usize), #root::ParseError> {
                #too_deep
                let __body = body;
                match discriminant {
                    #decode_arms
                    #decode_tail
                }
            }

            fn discriminant(&self) -> #option<#i64> {
                match self {
                    #which_arms
                    #which_tail
                }
            }

            fn encode_into<__B: #root::Buffer>(
                &self,
                out: &mut __B,
            ) -> #result<(), #root::Overflow> {
                match self {
                    #encode_arms
                    #encode_tail
                }
                #ok(())
            }
        }
    };

    Ok((ChoiceParts { variants, checks: item.checks, blocks: item.items, codec }, arm_rows))
}
