//! Parsing `#[message(..)]`.

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Type};

use layline_codegen::{BitOrder, Endian, Param, Root, Scalar};

use crate::ty::scalar_of;

pub(super) struct MessageAttr {
    pub(super) endian: Endian,
    pub(super) root: Root,
    /// `closed`: an unlisted discriminant is a decode error.
    pub(super) closed: Option<Span>,
    /// `bits`, with its `order`.
    pub(super) bits: Option<(BitOrder, Span)>,
    /// `needs(a: u8, ..)`: parameters the enclosing message supplies.
    pub(super) needs: Option<(Vec<Param>, Span)>,
}

/// Must mirror the `attributes(..)` list on `derive_message` in `lib.rs`.
pub(super) const MESSAGE_ATTRS: &[&str] = &[
    "message", "count", "len", "stride", "fill", "seek", "at", "codec", "bits", "bytes", "text",
    "until", "var", "switch", "when", "with", "present", "checksum", "magic", "value", "other",
    "range",
];

fn refuse_misplaced_on_container(input: &DeriveInput) -> syn::Result<()> {
    let catalogue = matches!(input.data, Data::Enum(_));
    for attr in &input.attrs {
        let Some(name) = crate::attr::registered(attr, MESSAGE_ATTRS) else { continue };
        if name == "message" {
            continue;
        }
        let ident = &input.ident;
        let arm = matches!(name, "value" | "other");
        let msg = match (catalogue, arm) {
            (false, false) => format!(
                "`{ident}`: `#[{name}]` belongs on a field, not the struct. Move it to a field"
            ),
            (false, true) => format!(
                "`{ident}`: `#[{name}]` belongs on an enum variant, not a struct. \
                 Use `#[switch(n)]` on a field and `#[value(N)]` on the arms"
            ),
            (true, true) => format!(
                "`{ident}`: `#[{name}]` belongs on a variant, not the enum. Move it to a variant"
            ),
            (true, false) => format!(
                "`{ident}`: `#[{name}]` belongs on a field, not the enum. \
                 Move it to a field of an arm's type"
            ),
        };
        return Err(syn::Error::new(attr.meta.span(), msg));
    }
    Ok(())
}

/// `needs(a: u8, b: u16)`: one name and one integer type per parameter.
fn parse_needs(
    meta: &syn::meta::ParseNestedMeta<'_>,
    ident: &syn::Ident,
) -> syn::Result<Vec<Param>> {
    let shape = format!("`{ident}`: `needs` takes `name: type` pairs, such as `needs(stride: u8)`");
    if !meta.input.peek(syn::token::Paren) {
        return Err(meta.error(shape));
    }
    let content;
    syn::parenthesized!(content in meta.input);
    let mut out = Vec::new();
    while !content.is_empty() {
        let name: syn::Ident = content.parse()?;
        content.parse::<syn::Token![:]>()?;
        let ty: Type = content.parse()?;
        let Some(repr @ (Scalar::U(_) | Scalar::I(_))) = scalar_of(&ty) else {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "`{ident}`: parameter `{name}` must be an integer, not `{}`",
                    crate::attr::spelled(&ty),
                ),
            ));
        };
        out.push(Param::new(&crate::attr::name(&name), repr));
        if content.is_empty() {
            break;
        }
        content.parse::<syn::Token![,]>()?;
    }
    if out.is_empty() {
        return Err(syn::Error::new(
            meta.path.span(),
            format!("`{ident}`: `needs()` lists no parameters. Add one, or remove `needs`"),
        ));
    }
    Ok(out)
}

pub(super) fn container_attr(input: &DeriveInput) -> syn::Result<MessageAttr> {
    refuse_misplaced_on_container(input)?;
    let default = || MessageAttr {
        endian: Endian::Le,
        root: Root::default(),
        closed: None,
        bits: None,
        needs: None,
    };
    let ident = &input.ident;
    let Some(attr) = crate::attr::single(&input.attrs, "message", format_args!("`{ident}`"))?
    else {
        return Ok(default());
    };
    let (mut endian, mut order, mut bits_span) = (None, None, None);
    let (mut needs, mut closed, mut root) = (None, None, None);
    attr.parse_nested_meta(|meta| {
        let span = meta.path.span();
        if meta.path.is_ident("endian") {
            crate::attr::once_key(&mut endian, &meta, ident, "message", || {
                Ok((crate::attr::endian_value(&meta, ident)?, span))
            })
        } else if meta.path.is_ident("order") {
            crate::attr::once_key(&mut order, &meta, ident, "message", || {
                Ok((crate::attr::order_value(&meta, ident)?, span))
            })
        } else if meta.path.is_ident("needs") {
            crate::attr::once_key(&mut needs, &meta, ident, "message", || {
                Ok((parse_needs(&meta, ident)?, span))
            })
        } else if meta.path.is_ident("closed") {
            crate::attr::once_key(&mut closed, &meta, ident, "message", || Ok(span))
        } else if meta.path.is_ident("crate") {
            crate::attr::once_key(&mut root, &meta, ident, "message", || {
                crate::root::parse_arg(&meta)
            })
        } else if meta.path.is_ident("bits") && !meta.input.peek(syn::Token![=]) {
            crate::attr::once_key(&mut bits_span, &meta, ident, "message", || Ok(span))
        } else if meta.path.is_ident("bits")
            || meta.path.is_ident("bytes")
            || meta.path.is_ident("words")
        {
            Err(meta.error(format!(
                "`{ident}`: a message has no fixed size. \
                 Remove `bits = N`, or use a `#[derive(Layout)]` type"
            )))
        } else {
            Err(meta.error(format!(
                "`{ident}`: expected `endian`, `bits`, `order`, `needs`, `closed`, or `crate`"
            )))
        }
    })?;

    let out = MessageAttr {
        endian: endian.map_or(Endian::Le, |(e, _)| e),
        root: root.unwrap_or_default(),
        closed,
        bits: None,
        needs,
    };
    match (bits_span, endian, order) {
        (Some(_), Some((_, span)), _) => Err(syn::Error::new(
            span,
            format!("`{ident}`: a bit-addressed message has no byte order. Remove `endian`"),
        )),
        (None, _, Some((_, span))) => Err(syn::Error::new(
            span,
            format!(
                "`{ident}`: `order` needs `bits`. \
                 Write `#[message(bits, order = msb)]`, or remove `order`"
            ),
        )),
        (Some(span), None, order) => {
            Ok(MessageAttr { bits: Some((order.map_or(BitOrder::Lsb, |(o, _)| o), span)), ..out })
        }
        (None, _, None) => Ok(out),
    }
}
