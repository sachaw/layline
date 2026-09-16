//! Attribute parsing helpers.

use proc_macro2::Span;
use syn::LitInt;
use syn::spanned::Spanned;

use layline_codegen::{BitOrder, Endian};

pub(crate) fn spelled(tokens: &impl quote::ToTokens) -> String {
    tokens.to_token_stream().to_string().replace(' ', "")
}

/// The identifier without its `r#` prefix.
pub(crate) fn name(ident: &syn::Ident) -> String {
    syn::ext::IdentExt::unraw(ident).to_string()
}

/// Fills `slot`, or refuses a second value as `{owner}: duplicate {what}`.
fn once<T>(
    slot: &mut Option<T>,
    span: proc_macro2::Span,
    owner: impl std::fmt::Display,
    what: impl std::fmt::Display,
    value: impl FnOnce() -> syn::Result<T>,
) -> syn::Result<()> {
    if slot.is_some() {
        return Err(syn::Error::new(span, format!("{owner}: duplicate {what}")));
    }
    *slot = Some(value()?);
    Ok(())
}

/// [`once`] for an attribute of a field, a variant or a type.
pub(crate) fn once_attr<T>(
    slot: &mut Option<T>,
    attr: &syn::Attribute,
    owner: impl std::fmt::Display,
    value: impl FnOnce() -> syn::Result<T>,
) -> syn::Result<()> {
    let what = format!("#[{}] attribute", spelled(attr.path()));
    once(slot, syn::spanned::Spanned::span(&attr.meta), owner, what, value)
}

/// [`once`] for a key inside a type's `#[attr(..)]`.
pub(crate) fn once_key<T>(
    slot: &mut Option<T>,
    meta: &syn::meta::ParseNestedMeta<'_>,
    owner: &syn::Ident,
    attr: &str,
    value: impl FnOnce() -> syn::Result<T>,
) -> syn::Result<()> {
    let what = format!("`{}` in #[{attr}]", spelled(&meta.path));
    once(slot, syn::spanned::Spanned::span(&meta.path), format_args!("`{owner}`"), what, value)
}

/// The attribute named `name`, refusing duplicates.
pub(crate) fn single<'a>(
    attrs: &'a [syn::Attribute],
    name: &str,
    owner: impl std::fmt::Display,
) -> syn::Result<Option<&'a syn::Attribute>> {
    let mut found = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident(name)) {
        once_attr(&mut found, attr, &owner, || Ok(attr))?;
    }
    Ok(found)
}

/// `known` must mirror the derive's `attributes(..)` list.
pub(crate) fn registered<'a>(attr: &syn::Attribute, known: &[&'a str]) -> Option<&'a str> {
    known.iter().copied().find(|k| attr.path().is_ident(k))
}

/// `key = N` inside a type's attribute.
pub(crate) fn int_value<N>(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<N>
where
    N: std::str::FromStr,
    N::Err: std::fmt::Display,
{
    meta.value()?.parse::<LitInt>()?.base10_parse()
}

/// `#[attr(N)]`, with the attribute's span.
pub(crate) fn int_arg<N>(attr: &syn::Attribute) -> syn::Result<(N, Span)>
where
    N: std::str::FromStr,
    N::Err: std::fmt::Display,
{
    Ok((attr.parse_args::<LitInt>()?.base10_parse()?, attr.meta.span()))
}

/// `endian = le | be`.
pub(crate) fn endian_value(
    meta: &syn::meta::ParseNestedMeta<'_>,
    ident: &syn::Ident,
) -> syn::Result<Endian> {
    match meta.value()?.parse::<syn::Ident>()?.to_string().as_str() {
        "le" => Ok(Endian::Le),
        "be" => Ok(Endian::Be),
        _ => Err(meta.error(format!("`{ident}`: expected `endian = le` or `endian = be`"))),
    }
}

/// `order = lsb | msb`.
pub(crate) fn order_value(
    meta: &syn::meta::ParseNestedMeta<'_>,
    ident: &syn::Ident,
) -> syn::Result<BitOrder> {
    match meta.value()?.parse::<syn::Ident>()?.to_string().as_str() {
        "lsb" => Ok(BitOrder::Lsb),
        "msb" => Ok(BitOrder::Msb),
        _ => Err(meta.error(format!("`{ident}`: expected `order = lsb` or `order = msb`"))),
    }
}

pub(crate) fn bare(attr: &syn::Attribute, name: &str, what: &str) -> syn::Result<()> {
    match &attr.meta {
        syn::Meta::Path(_) => Ok(()),
        other => {
            Err(syn::Error::new(other.span(), format!("field `{name}`: {what} takes no arguments")))
        }
    }
}

/// How a variant of a `FieldCodec`, `Dispatch` or catalogue enum is keyed.
pub(crate) enum Key<'a> {
    Value(LitInt),
    Other(&'a syn::Attribute),
}

/// The variant's `#[value(N)]` or `#[other]`.
///
/// `noun` names what a value keys. `rest` names what the `#[other]` variant keeps.
pub(crate) fn variant_key<'a>(v: &'a syn::Variant, noun: &str, rest: &str) -> syn::Result<Key<'a>> {
    let owner = format!("variant `{}`", v.ident);
    let other = single(&v.attrs, "other", &owner)?;
    let value = single(&v.attrs, "value", &owner)?;
    match (other, value) {
        (Some(other), Some(value)) => Err(noted(
            value.meta.span(),
            format!(
                "{owner}: both #[other] and #[value(..)] are set, and #[other] takes any {noun}. \
                 Remove the #[value]"
            ),
            other.meta.span(),
            "#[other] declared here",
        )),
        (Some(other), None) => Ok(Key::Other(other)),
        (None, Some(value)) => Ok(Key::Value(value.parse_args()?)),
        (None, None) => Err(syn::Error::new(
            v.ident.span(),
            format!(
                "{owner}: needs #[value(N)] for the {noun} it decodes from, \
                 or #[other] to keep {rest}"
            ),
        )),
    }
}

/// A refusal at `span`, with a note at the declaration it concerns.
pub(crate) fn noted(span: Span, msg: String, at: Span, note: &str) -> syn::Error {
    let mut err = syn::Error::new(span, msg);
    err.combine(syn::Error::new(at, note));
    err
}
