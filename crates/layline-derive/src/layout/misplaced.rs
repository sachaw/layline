//! Refusals for layout attributes in the wrong place.

use syn::spanned::Spanned;

/// Must mirror the `attributes(..)` list on `derive_layout` in `lib.rs`.
pub(super) const LAYOUT_ATTRS: &[&str] = &[
    "layout", "bits", "codec", "bytes", "at", "overlay", "magic", "checksum", "range", "count",
    "len", "stride", "fill", "seek", "text", "until", "var", "message", "switch", "when",
];

/// The attributes in [`LAYOUT_ATTRS`] that only `#[derive(Message)]` reads.
const MESSAGE_VOCABULARY: &[&str] = &[
    "count", "len", "stride", "fill", "seek", "text", "until", "var", "message", "switch", "when",
];

pub(super) fn walk_vocabulary(name: &str) -> bool {
    MESSAGE_VOCABULARY.contains(&name)
}

pub(super) fn refuse_unbound(
    attr: &syn::Attribute,
    ident: &syn::Ident,
    mode: &str,
) -> syn::Result<()> {
    let Some(name) = crate::attr::registered(attr, LAYOUT_ATTRS) else { return Ok(()) };
    let msg = match name {
        "layout" => format!("field `{ident}`: `#[{name}]` belongs above the struct"),
        "codec" => {
            format!("field `{ident}`: `#[codec(N)]` does not apply in {mode}. Write `#[bits(N)]`")
        }
        other => format!("field `{ident}`: `#[{other}]` does not apply in {mode}. Remove it"),
    };
    Err(syn::Error::new(attr.meta.span(), msg))
}

pub(super) fn refuse_message_vocabulary(
    attr: &syn::Attribute,
    ident: &syn::Ident,
    mode: &str,
) -> syn::Result<()> {
    let Some(name) = crate::attr::registered(attr, MESSAGE_VOCABULARY) else { return Ok(()) };
    Err(syn::Error::new(
        attr.meta.span(),
        format!(
            "field `{ident}`: #[{name}] is not supported in {mode}. \
             Use #[derive(Message)]"
        ),
    ))
}

pub(super) fn no_assertions_in_bit_mode(attr: &syn::Attribute, ident: &syn::Ident) -> syn::Error {
    let what = if attr.path().is_ident("magic") { "#[magic]" } else { "#[checksum]" };
    syn::Error::new(
        attr.meta.span(),
        format!(
            "field `{ident}`: {what} needs whole bytes, and a bit-addressed layout has none. \
             Use `#[layout(bytes = N)]`"
        ),
    )
}
