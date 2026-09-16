//! A field's width in bits, from `#[bits(N)]`, `#[codec(N)]`, or a `U<N>` or `I<N>` type.

use proc_macro2::Span;
use syn::Type;
use syn::spanned::Spanned;

use crate::attr::spelled;

pub(crate) const BYTE_MODE_HAS_NO_BIT_FIELDS: &str =
    "byte mode has no bit fields. Write `#[codec(N)]` for a field narrower than its type";

/// The width in a `U<N>` or an `I<N>` type, with the type's span.
fn carried(ty: &Type) -> Option<(u32, Span)> {
    let Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let seg = p.path.segments.last()?;
    if seg.ident != "U" && seg.ident != "I" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let (1, Some(syn::GenericArgument::Const(syn::Expr::Lit(lit)))) =
        (args.args.len(), args.args.first())
    else {
        return None;
    };
    let syn::Lit::Int(n) = &lit.lit else { return None };
    Some((n.base10_parse().ok()?, ty.span()))
}

pub(crate) fn carried_bits(ty: &Type) -> Option<u32> {
    carried(ty).map(|(bits, _)| bits)
}

/// A width as refusals print it: the `U<N>` or `I<N>` type, or `#[attr(N)]`.
pub(crate) fn width_said(attr: &str, bits: u32, ty: &Type) -> String {
    match carried(ty) {
        Some(_) => format!("`{}`, of {bits} bits,", spelled(ty)),
        None => format!("#[{attr}({bits})]"),
    }
}

/// The width from `#[attr(N)]` or the type, refusing a mismatch between the two.
///
/// `attr` is `bits` or `codec`.
pub(crate) fn declared_width(
    name: &str,
    attr: &str,
    ty: &Type,
    stated: Option<(u32, Span)>,
) -> syn::Result<Option<(u32, Span)>> {
    match (stated, carried(ty)) {
        (Some((stated, span)), Some((bits, _))) if stated != bits => Err(syn::Error::new(
            span,
            format!(
                "field `{name}`: #[{attr}({stated})] disagrees with `{}`, which is {bits} bits. \
                 Remove the #[{attr}]",
                spelled(ty),
            ),
        )),
        (Some(stated), _) => Ok(Some(stated)),
        (None, carried) => Ok(carried),
    }
}

pub(crate) fn nonzero(name: &str, bits: u32, span: Span) -> syn::Result<()> {
    if bits != 0 {
        return Ok(());
    }
    Err(syn::Error::new(
        span,
        format!("field `{name}`: #[bits(0)] is empty. A field needs at least one bit"),
    ))
}

pub(crate) fn one_bit_bool(name: &str, bits: u32, span: Span) -> syn::Result<()> {
    if bits == 1 {
        return Ok(());
    }
    Err(syn::Error::new(
        span,
        format!("field `{name}`: a `bool` is one bit, not #[bits({bits})]. Write `#[bits(1)]`"),
    ))
}
