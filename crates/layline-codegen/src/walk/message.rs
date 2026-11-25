//! A message's decode, encode and segment table.

mod bits;
mod block;
mod body;
mod checksum;
mod choice;
mod cursor;
mod env;
mod eval;
mod opt;
mod patch;
mod placed;
mod prefix;
mod recursion;
mod refuse;
mod repeat;
mod switch;
mod table;
mod text;
mod value;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use bits::bits_parts;
use body::{Item, Own, Walked, walk_segments};
#[cfg(feature = "emit")]
pub(crate) use choice::emit_choice;
pub use choice::{ChoiceParts, choice_parts};
use cursor::too_deep;
use recursion::{names_self, open_end, unbounded_recursion};
use refuse::refuses_itself;
pub use refuse::{known, refusal, signatures};
use table::covers_table;

use crate::Derive;
use crate::root::Prelude;
#[cfg(feature = "emit")]
use crate::walk::derive_attr;
#[cfg(feature = "emit")]
use crate::walk::doc_attr;
use crate::walk::tokens::ident;
use crate::walk::{doc_lines, references, row, scalar_ty};
use crate::{Endian, Error, MessageDef, Root};

/// A compile-time assertion about a type known only by name.
///
/// Kept apart from [`MessageParts::blocks`] so a derive can point the error at the attribute.
pub struct Check {
    /// The reference (`header.n`) or field (`body`) the assertion is about.
    pub field: String,
    /// The `const _: () = assert!(..)` item.
    pub tokens: TokenStream,
}

/// A generated message, in separate pieces.
///
/// The `emit` backend wraps [`fields`](Self::fields) in a struct declaration. A derive keeps the
/// user's struct.
pub struct MessageParts {
    /// Field declarations, in wire order.
    pub fields: TokenStream,
    /// Assertions about types known only by name.
    pub checks: Vec<Check>,
    /// Hidden `#[derive(Layout)]` types the codec reads through.
    ///
    /// One per [`Segment::Block`](crate::Segment::Block), per fixed optional field and per checksum field.
    pub blocks: TokenStream,
    /// `impl Message for Name { const SEGMENTS; fn decode_with_nested; fn encode_into_with }`.
    pub codec: TokenStream,
}

/// [`lower_message`] plus the struct declaration.
///
/// # Errors
///
/// Whatever [`lower_message`] refuses.
#[cfg(feature = "emit")]
pub(crate) fn emit_message(
    m: &MessageDef,
    derives: &[Derive],
    root: &Root,
) -> Result<(TokenStream, Vec<row::SegmentDef>), Error> {
    let name = ident(&m.name);
    let der = derive_attr(derives, &m.derives)?;
    let doc = doc_attr(&m.doc);
    let (MessageParts { fields, checks, blocks, codec }, rows) = lower_message(m, derives, root)?;
    let checks = checks.into_iter().map(|c| c.tokens);
    let generics = m.lifetime.as_ref().map(|lifetime| {
        let lifetime = syn::Lifetime::new(&format!("'{lifetime}"), proc_macro2::Span::call_site());
        quote!(<#lifetime>)
    });
    Ok((
        quote! {
            #doc
            #der
            pub struct #name #generics {
                #fields
            }

            #(#checks)*

            #blocks

            #codec
        },
        rows,
    ))
}

/// Generate a message's parts.
///
/// # Errors
///
/// [`Error::Invalid`] when [`validate`](crate::validate()) refuses it. [`Error::Refused`] for a
/// shape that cannot be generated.
pub fn message_parts(
    m: &MessageDef,
    derives: &[Derive],
    root: &Root,
) -> Result<MessageParts, Error> {
    lower_message(m, derives, root).map(|(parts, _)| parts)
}

/// [`message_parts`] plus the table rows.
fn lower_message(
    m: &MessageDef,
    derives: &[Derive],
    root: &Root,
) -> Result<(MessageParts, Vec<row::SegmentDef>), Error> {
    let Prelude { ok, result, u8, u32, usize, .. } = root.prelude();
    crate::validate::validate_message(m).map_err(|why| Error::Invalid(m.name.clone(), why))?;
    if m.bits.is_some() {
        return bits_parts(m, derives, root);
    }
    let name = ident(&m.name);
    let skip = super::tokens::serde_skip(derives, &m.derives);
    let mut item = Item::new(&m.name, m.endian, root, &m.needs, skip);
    item.lifetime = m.lifetime.as_deref();
    if let Some(r) = references(&m.segments)
        .into_iter()
        .find(|r| !r.avoidable && names_self(r.to.name(), &m.name))
    {
        return Err(Error::Refused(unbounded_recursion(&m.name, r.field)));
    }
    let all: Vec<crate::Segment> = m.segments.iter().chain(&m.tail).cloned().collect();
    let Walked { fields, base, parse, build, bytes, table, rows, .. } =
        walk_segments(&mut item, &all, m.segments.len(), Own::Struct)?;
    // A message with a tail reads to the end of what it is given.
    let end = match m.tail.is_empty() {
        true => open_end(&m.segments, &m.name, root),
        false => refuse::End { open: true, through: Vec::new() },
    };
    let covered = covers_table(&rows, root).map(|covers| {
        quote! {
            /// The bytes each `#[checksum]` field covers.
            const COVERED: &'static [#root::table::CoverDef<'static>] = &[#covers];
        }
    });

    let literals = table.clone();
    let table = quote! {
        /// Each segment in wire order, with its start and length.
        const SEGMENTS: &'static [#root::table::SegmentDef<'static>] = &[#table];

        #covered
    };
    // A borrowing message is never nested, so it decodes at depth zero.
    let too_deep = match m.lifetime {
        Some(_) => quote!(),
        None => too_deep(&quote!(#root::Message), root),
    };
    let max_depth = max_depth(m.depth, root);
    // The body reads and writes in the order `__O`: a fixed one here, or the parameter's.
    let fixed = |body: &TokenStream| match &m.endian_from {
        Some(_) => quote!(),
        None => fixed_order(m.endian, body, root),
    };
    let (parse_order, bytes_order) = (fixed(&parse), fixed(&bytes));
    let decode = quote! {
        #parse_order
        #too_deep
        let __body = body;
        let mut __at = 0usize;
        let mut __hi = 0usize;
        #parse
        #ok((Self { #build }, __hi))
    };
    let own = refusal(refuses_itself(&bytes), &item.via);
    let buffered = quote!(#root::EncodeError);
    let encode = quote! {
        #bytes_order
        #base
        #bytes
        #ok(())
    };

    let (ctx_decl, ctx_ty, params) = if m.needs.is_empty() {
        (quote!(), quote!(()), quote!())
    } else {
        let ctx_name = format_ident!("{}Ctx", m.name);
        let rows = m.needs.iter().map(|p| {
            let (name, ty) = (p.name.as_str(), p.repr.primitive());
            quote! { #root::table::ParamDef::new(#name, #ty), }
        });
        (
            context_struct(&m.name, &m.needs),
            quote!(#ctx_name),
            quote! {
                /// The parameters the holder supplies, in declaration order.
                const PARAMS: &'static [#root::table::ParamDef<'static>] = &[#(#rows)*];
            },
        )
    };
    let (owner, str_ty) = (m.name.as_str(), root.prelude().primitive("str"));
    if let Some(lifetime) = &m.lifetime {
        let of = {
            let lifetime =
                syn::Lifetime::new(&format!("'{lifetime}"), proc_macro2::Span::call_site());
            quote!(#root::MessageRef<#lifetime>)
        };
        let named = {
            let lifetime = syn::Lifetime::new("'static", proc_macro2::Span::call_site());
            (quote!(#name<#lifetime>), quote!(#root::MessageRef<#lifetime>))
        };
        let refuse::Constants { node, encode_fallible, fallible, .. } = refuse::constants(
            &own,
            &item.links,
            &refuse::End::default(),
            (&named.0, &named.1),
            owner,
            root,
        );
        let constants = quote!(#node #encode_fallible);
        let inherent_ref = inherent_encodes(&fallible, &of, None, root);
        let lifetime = syn::Lifetime::new(&format!("'{lifetime}"), proc_macro2::Span::call_site());
        let codec = quote! {
            impl<#lifetime> #root::MessageRef<#lifetime> for #name<#lifetime> {
                const NAME: &'static #str_ty = #owner;

                /// Each segment in wire order, with its start and length.
                const SEGMENTS: &'static [#root::table::SegmentDef<'static>] = &[#literals];

                #covered

                #constants

                fn decode(body: &#lifetime [#u8]) -> #result<(Self, #usize), #root::ParseError> {
                    let __depth: #u32 = 0;
                    let __ctx = ();
                    #decode
                }

                fn encode_into<__B: #root::Buffer>(
                    &self,
                    out: &mut __B,
                ) -> #result<(), #buffered> {
                    let __ctx = ();
                    #encode
                }
            }
            impl<#lifetime> #name<#lifetime> {
                #inherent_ref
            }
        };
        return Ok((MessageParts { fields, checks: item.checks, blocks: item.items, codec }, rows));
    }
    let consts = refuse::constants(
        &own,
        &item.links,
        &end,
        (&quote!(#name), &quote!(#root::Message)),
        owner,
        root,
    );
    let (constants, fallible) = (consts.all(), consts.fallible);
    let inherent =
        inherent_encodes(&fallible, &quote!(#root::Message), Some(!m.needs.is_empty()), root);
    // A body with no fixed field of its own reads nothing in the order: its holdings pick theirs.
    let ordered = names_order(&decode) || names_order(&encode);
    let (decode, encode, held) = match &m.endian_from {
        Some(param) if ordered => held_order(
            &name,
            param,
            decode,
            encode,
            &buffered,
            &quote!(#root::Message),
            (quote!(), quote!()),
            root,
        ),
        _ => (decode, encode, quote!()),
    };
    let codec = quote! {
        #ctx_decl

        impl #root::Message for #name {
            const NAME: &'static #str_ty = #owner;

            type Ctx = #ctx_ty;

            #table

            #params

            #constants

            #max_depth

            fn decode_with_nested(
                body: &[#u8],
                __depth: #u32,
                __ctx: Self::Ctx,
            ) -> #result<(Self, #usize), #root::ParseError> {
                #decode
            }

            fn encode_into_with<__B: #root::Buffer>(
                &self,
                out: &mut __B,
                __ctx: Self::Ctx,
            ) -> #result<(), #buffered> {
                #encode
            }
        }
        impl #name {
            #inherent
        }

        #held
    };

    Ok((MessageParts { fields, checks: item.checks, blocks: item.items, codec }, rows))
}

/// `const MAX_DEPTH`, if the item sets its own.
pub(super) fn max_depth(depth: Option<u32>, root: &Root) -> TokenStream {
    let u32 = root.prelude().u32;
    match depth {
        Some(n) => {
            let n = proc_macro2::Literal::u32_unsuffixed(n);
            quote! { const MAX_DEPTH: #u32 = #n; }
        }
        None => quote!(),
    }
}

/// The inherent encodes of an item whose `ENCODE_FALLIBLE` is `fallible`, shadowing the trait
/// `of`'s: they return `Overflow` alone and a bare vector when it is `false`.
///
/// `needs` is `Some` for a [`Message`](crate::MessageDef), saying whether it takes parameters,
/// and `None` for a borrowing message or a catalogue, which take none.
pub(super) fn inherent_encodes(
    fallible: &TokenStream,
    of: &TokenStream,
    needs: Option<bool>,
    root: &Root,
) -> TokenStream {
    let vec = quote!(#root::__private::Vec<::core::primitive::u8>);
    let (into, _) = refuse::signatures(fallible, &quote!(()), root);
    let (_, bytes) = refuse::signatures(fallible, &vec, root);
    // A literal needs no braces, and emitted source would warn on them.
    let known = match refuse::known(fallible) {
        Some(_) => fallible.clone(),
        None => quote!({ #fallible }),
    };
    let fin = quote!(<#root::__private::Fallibility<#known> as #root::__private::Buffers>::finish);
    let fin_vec = quote! {
        <#root::__private::Fallibility<#known> as #root::__private::Encodes<#vec>>::finish
    };
    let errors = match refuse::known(fallible) {
        Some(false) => "# Errors\n\n`Overflow` if `out` has no room.",
        Some(true) => {
            "# Errors\n\n`EncodeError`: a full buffer, or a value the type's declarations refuse."
        }
        None => {
            "# Errors\n\n`Overflow` if `out` has no room, or, when a held type refuses values, \
                 `EncodeError` naming that too."
        }
    };
    let vec_doc = match refuse::known(fallible) {
        Some(false) => "",
        _ => "\n\n# Errors\n\n`EncodeError` for a value the type's declarations refuse.",
    };
    let into_doc = doc_lines(&format!("Append the wire bytes to `out`.\n\n{errors}"));
    // A `Result` is already `#[must_use]`.
    let must_use = (refuse::known(fallible) == Some(false)).then(|| quote!(#[must_use]));
    let vec_doc = doc_lines(&format!("The wire bytes, in a new vector.{vec_doc}"));
    match needs {
        None => quote! {
            #into_doc
            #[inline]
            pub fn encode_into<__B: #root::Buffer>(
                &self,
                out: &mut __B,
            ) -> ::core::result::Result<(), #into> {
                #fin(<Self as #of>::encode_into(self, out))
            }

            #root::__private::alloc_only! {
                #vec_doc
                #[inline]
                #must_use
                pub fn encode(&self) -> #bytes {
                    #fin_vec(<Self as #of>::encode(self))
                }
            }
        },
        Some(with) => {
            let plain = (!with).then(|| {
                quote! {
                    #into_doc
                    #[inline]
                    pub fn encode_into<__B: #root::Buffer>(
                        &self,
                        out: &mut __B,
                    ) -> ::core::result::Result<(), #into> {
                        #fin(<Self as #of>::encode_into_with(self, out, ()))
                    }

                    #root::__private::alloc_only! {
                        #vec_doc
                        #[inline]
                        #must_use
                        pub fn encode(&self) -> #bytes {
                            #fin_vec(<Self as #of>::encode_with(self, ()))
                        }
                    }
                }
            });
            quote! {
                #into_doc
                #[inline]
                pub fn encode_into_with<__B: #root::Buffer>(
                    &self,
                    out: &mut __B,
                    ctx: <Self as #of>::Ctx,
                ) -> ::core::result::Result<(), #into> {
                    #fin(<Self as #of>::encode_into_with(self, out, ctx))
                }

                #root::__private::alloc_only! {
                    #vec_doc
                    #[inline]
                    #must_use
                    pub fn encode_with(&self, ctx: <Self as #of>::Ctx) -> #bytes {
                        #fin_vec(<Self as #of>::encode_with(self, ctx))
                    }
                }

                #plain
            }
        }
    }
}

/// `type __O = Le;` or `Be`: the order a body with a fixed `endian` reads and writes in, if
/// `body` reads or writes a scalar.
pub(super) fn fixed_order(endian: Endian, body: &TokenStream, root: &Root) -> TokenStream {
    if !names_order(body) {
        return quote!();
    }
    let order = match endian {
        Endian::Le => quote!(#root::__private::Le),
        Endian::Be => quote!(#root::__private::Be),
    };
    quote! { type __O = #order; }
}

/// Whether `body` reads or writes in the order `__O`.
pub(super) fn names_order(body: &TokenStream) -> bool {
    body.clone().into_iter().any(|t| match t {
        proc_macro2::TokenTree::Ident(i) => i == "__O",
        proc_macro2::TokenTree::Group(g) => names_order(&g.stream()),
        _ => false,
    })
}

/// An item whose order is the parameter `param`: `decode` and `encode` bodies generic over `__O`,
/// and the trait methods that pick `Be` or `Le` once from the context.
///
/// `of` is the trait, and `lead` the parameters decode takes before `body`, declared and passed:
/// a catalogue's discriminant.
#[allow(clippy::too_many_arguments)]
pub(super) fn held_order(
    name: &proc_macro2::Ident,
    param: &str,
    decode: TokenStream,
    encode: TokenStream,
    buffered: &TokenStream,
    of: &TokenStream,
    (lead_decl, lead): (TokenStream, TokenStream),
    root: &Root,
) -> (TokenStream, TokenStream, TokenStream) {
    let Prelude { result, u8, u32, usize, .. } = root.prelude();
    let p = ident(param);
    let (be, le) = (quote!(#root::__private::Be), quote!(#root::__private::Le));
    let order = quote!(#root::__private::ByteOrder);
    let held = quote! {
        impl #name {
            /// Decode in the byte order `__O`.
            #[doc(hidden)]
            fn __layline_decode<__O: #order>(
                #lead_decl
                body: &[#u8],
                __depth: #u32,
                __ctx: <Self as #of>::Ctx,
            ) -> #result<(Self, #usize), #root::ParseError> {
                #decode
            }

            /// Encode in the byte order `__O`.
            #[doc(hidden)]
            fn __layline_encode<__O: #order, __B: #root::Buffer>(
                &self,
                out: &mut __B,
                __ctx: <Self as #of>::Ctx,
            ) -> #result<(), #buffered> {
                #encode
            }
        }
    };
    (
        quote! {
            if __ctx.#p {
                Self::__layline_decode::<#be>(#lead body, __depth, __ctx)
            } else {
                Self::__layline_decode::<#le>(#lead body, __depth, __ctx)
            }
        },
        quote! {
            if __ctx.#p {
                self.__layline_encode::<#be, __B>(out, __ctx)
            } else {
                self.__layline_encode::<#le, __B>(out, __ctx)
            }
        },
        held,
    )
}

/// The `<Name>Ctx` struct: one field per parameter, and `new`.
///
/// Use sites call `<Name as Message>::Ctx::new(..)` positionally, because a proc macro cannot see
/// the callee's parameter names.
pub(super) fn context_struct(owner: &str, needs: &[crate::Param]) -> TokenStream {
    let name = format_ident!("{}Ctx", owner);
    let doc = format!("The parameters `{owner}` reads from its holder.");
    let fields = needs.iter().map(|p| {
        let id = ident(&p.name);
        let ty = scalar_ty(p.repr);
        let doc = format!("Parameter `{}`.", p.name);
        quote! { #[doc = #doc] pub #id: #ty, }
    });
    let args = needs.iter().map(|p| {
        let id = ident(&p.name);
        let ty = scalar_ty(p.repr);
        quote! { #id: #ty, }
    });
    let inits = needs.iter().map(|p| {
        let id = ident(&p.name);
        quote! { #id, }
    });
    let new_doc = format!("Build a context for `{owner}`, parameters in declaration order.");
    // A real has no `Eq` or `Hash`.
    let real = needs.iter().any(|p| matches!(p.repr, crate::Scalar::F32 | crate::Scalar::F64));
    let derives = if real {
        quote!(Debug, Clone, Copy, PartialEq)
    } else {
        quote!(Debug, Clone, Copy, PartialEq, Eq, Hash)
    };
    quote! {
        #[doc = #doc]
        #[derive(#derives)]
        pub struct #name {
            #(#fields)*
        }

        impl #name {
            #[doc = #new_doc]
            #[must_use]
            pub fn new(#(#args)*) -> Self {
                Self { #(#inits)* }
            }
        }
    }
}
