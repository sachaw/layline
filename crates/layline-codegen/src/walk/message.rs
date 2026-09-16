//! A message's decode, encode and segment table.

mod bits;
mod block;
mod body;
mod checksum;
mod choice;
mod cursor;
mod env;
mod opt;
mod patch;
mod placed;
mod recursion;
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
use recursion::{names_self, open_ended_expr, unbounded_recursion};
use table::covers_table;

#[cfg(feature = "emit")]
use crate::emit::Derive;
use crate::root::Prelude;
#[cfg(feature = "emit")]
use crate::walk::derive_attr;
use crate::walk::tokens::ident;
use crate::walk::{references, row, scalar_ty};
use crate::{Error, MessageDef, Root};

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
    cyclic: &[(String, bool)],
) -> Result<(TokenStream, Vec<row::SegmentDef>), Error> {
    let name = ident(&m.name);
    let der = derive_attr(derives)?;
    let (MessageParts { fields, checks, blocks, codec }, rows) = lower_message(m, root, cyclic)?;
    let checks = checks.into_iter().map(|c| c.tokens);
    Ok((
        quote! {
            #der
            pub struct #name {
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
pub fn message_parts(m: &MessageDef, root: &Root) -> Result<MessageParts, Error> {
    lower_message(m, root, &[]).map(|(parts, _)| parts)
}

/// [`message_parts`] plus the table rows. `cyclic` says which types on `m`'s cycles are open-ended.
fn lower_message(
    m: &MessageDef,
    root: &Root,
    cyclic: &[(String, bool)],
) -> Result<(MessageParts, Vec<row::SegmentDef>), Error> {
    let Prelude { ok, result, u8, u32, usize, bool, .. } = root.prelude();
    crate::validate::validate_message(m).map_err(|why| Error::Invalid(m.name.clone(), why))?;
    if m.bits.is_some() {
        return bits_parts(m, root);
    }
    let name = ident(&m.name);
    let mut item = Item::new(&m.name, m.endian, root, &m.needs);
    if let Some(r) = references(&m.segments)
        .into_iter()
        .find(|r| !r.avoidable && names_self(r.to.name(), &m.name))
    {
        return Err(Error::Refused(unbounded_recursion(&m.name, r.field)));
    }
    let Walked { fields, base, parse, build, bytes, table, rows, .. } =
        walk_segments(&mut item, &m.segments, Own::Struct)?;
    let open_ended = open_ended_expr(&m.segments, &m.name, cyclic, root);
    let covered = covers_table(&rows, root).map(|covers| {
        quote! {
            /// The bytes each `#[checksum]` field covers.
            const COVERED: &'static [#root::table::CoverDef<'static>] = &[#covers];
        }
    });

    let table = quote! {
        /// Each segment in wire order, with its start and length.
        const SEGMENTS: &'static [#root::table::SegmentDef<'static>] = &[#table];

        #covered

        const OPEN_ENDED: #bool = #open_ended;
    };
    let too_deep = too_deep(root);
    let decode = quote! {
        #too_deep
        let __body = body;
        let mut __at = 0usize;
        let mut __hi = 0usize;
        #parse
        #ok((Self { #build }, __hi))
    };
    let encode = quote! {
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
            context_struct(m),
            quote!(#ctx_name),
            quote! {
                /// The parameters the holder supplies, in declaration order.
                const PARAMS: &'static [#root::table::ParamDef<'static>] = &[#(#rows)*];
            },
        )
    };
    let (owner, str_ty) = (m.name.as_str(), root.prelude().primitive("str"));
    let codec = quote! {
        #ctx_decl

        impl #root::Message for #name {
            const NAME: &'static #str_ty = #owner;

            type Ctx = #ctx_ty;

            #table

            #params

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
            ) -> #result<(), #root::Overflow> {
                #encode
            }
        }
    };

    Ok((MessageParts { fields, checks: item.checks, blocks: item.items, codec }, rows))
}

/// The `<Name>Ctx` struct: one field per parameter, and `new`.
///
/// Use sites call `<Name as Message>::Ctx::new(..)` positionally, because a proc macro cannot see
/// the callee's parameter names.
fn context_struct(m: &MessageDef) -> TokenStream {
    let name = format_ident!("{}Ctx", m.name);
    let owner = m.name.as_str();
    let doc = format!("The parameters `{owner}` reads from its holder.");
    let fields = m.needs.iter().map(|p| {
        let id = ident(&p.name);
        let ty = scalar_ty(p.repr);
        let doc = format!("Parameter `{}`.", p.name);
        quote! { #[doc = #doc] pub #id: #ty, }
    });
    let args = m.needs.iter().map(|p| {
        let id = ident(&p.name);
        let ty = scalar_ty(p.repr);
        quote! { #id: #ty, }
    });
    let inits = m.needs.iter().map(|p| {
        let id = ident(&p.name);
        quote! { #id, }
    });
    let new_doc = format!("Build a context for `{owner}`, parameters in declaration order.");
    quote! {
        #[doc = #doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
