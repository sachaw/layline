//! `#[derive(Message)]` on an enum whose arms a `#[switch]` field chooses from.
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, LitInt, Type};

use layline_codegen::{Arm, ChoiceDef, Field, Kind, Segment};

use super::container::{MESSAGE_ATTRS, MessageAttr, container_attr};
use crate::attr::{Key, noted, variant_key};
use crate::ty::{box_element, is_plain_path, scalar_of, variable_size};

fn refuse_misplaced_on_arm(v: &syn::Variant) -> syn::Result<()> {
    let is_other = v.attrs.iter().any(|a| a.path().is_ident("other"));
    for attr in &v.attrs {
        let Some(name) = crate::attr::registered(attr, MESSAGE_ATTRS) else { continue };
        let msg = match name {
            "value" | "other" => continue,
            "bytes" if !is_other => continue,
            "bytes" => format!(
                "variant `{}`: the #[other] arm takes its size from the switch. \
                 Remove `#[bytes]`, or declare `{}([u8; N])`",
                v.ident, v.ident,
            ),
            "message" => format!(
                "variant `{}`: `#[message(..)]` belongs on the enum. Move it above the enum",
                v.ident,
            ),
            "with" => format!(
                "variant `{}`: `#[with(..)]` does not apply to a variant. \
                 Wrap the message in a `#[derive(Message)]` struct and use that struct as the arm",
                v.ident,
            ),
            other => format!(
                "variant `{}`: `#[{other}]` belongs on a field, not a variant. \
                 Move it to a field of the arm's type",
                v.ident,
            ),
        };
        return Err(syn::Error::new(attr.meta.span(), msg));
    }
    for f in &v.fields {
        if let Some(attr) =
            f.attrs.iter().find(|a| crate::attr::registered(a, MESSAGE_ATTRS).is_some())
        {
            let name = crate::attr::registered(attr, MESSAGE_ATTRS).expect("just found");
            return Err(syn::Error::new(
                attr.meta.span(),
                format!(
                    "variant `{}`: `#[{name}]` on a variant's payload has no effect. \
                     Put it on the variant, or on a field of the payload type",
                    v.ident,
                ),
            ));
        }
    }
    Ok(())
}

pub(super) fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            format!(
                "`{}`: #[derive(Message)] does not support generics. Remove the type parameters",
                input.ident
            ),
        ));
    }

    let MessageAttr { endian, root, closed, bits, needs } = container_attr(input)?;
    if let Some((_, span)) = needs {
        return Err(syn::Error::new(
            span,
            format!(
                "`{}`: `needs` does not apply to an enum. \
                 Put `#[message(needs(..))]` on an arm's type",
                input.ident
            ),
        ));
    }
    if let Some((_, span)) = bits {
        return Err(syn::Error::new(
            span,
            format!(
                "`{}`: `bits` does not apply to an enum. \
                 Put `#[message(bits)]` on an arm's type",
                input.ident
            ),
        ));
    }
    let Data::Enum(data) = &input.data else {
        unreachable!("dispatched on Data::Enum");
    };

    let mut arms: Vec<Arm> = Vec::new();
    let mut seen: Vec<(u64, Span)> = Vec::new();
    let mut open: Option<(String, Span, Option<usize>)> = None;

    for v in &data.variants {
        refuse_misplaced_on_arm(v)?;
        match variant_key(v, "discriminant", "a body this catalogue does not recognise")? {
            Key::Other(attr) => {
                if let Some((_, first, _)) = &open {
                    return Err(noted(
                        attr.meta.span(),
                        format!(
                            "variant `{}`: a second #[other]. \
                             Only one variant can hold unrecognised bodies",
                            v.ident
                        ),
                        *first,
                        "first declared here",
                    ));
                }
                let bytes = raw_variant(v)?;
                open = Some((crate::attr::name(&v.ident), attr.meta.span(), bytes));
            }
            Key::Value(lit) => {
                let value: u64 = lit.base10_parse()?;
                if i64::try_from(value).is_err() {
                    return Err(syn::Error::new(
                        lit.span(),
                        format!(
                            "variant `{}`: #[value({value})] is past `i64::MAX`, \
                             the largest supported discriminant",
                            v.ident
                        ),
                    ));
                }
                if let Some((_, first)) = seen.iter().find(|(v, _)| *v == value) {
                    return Err(noted(
                        lit.span(),
                        format!(
                            "variant `{}`: #[value({value})] is already used by another arm. \
                             Give each arm a distinct value",
                            v.ident
                        ),
                        *first,
                        "first declared here",
                    ));
                }
                seen.push((value, lit.span()));
                arms.push(Arm::new(value, &crate::attr::name(&v.ident), arm_body(v)?));
            }
        }
    }

    if open.is_none() && closed.is_none() {
        return Err(syn::Error::new(input.ident.span(), not_total(&input.ident.to_string())));
    }
    if let (Some((name, span, _)), Some(_)) = (&open, closed) {
        return Err(syn::Error::new(
            *span,
            format!(
                "`{}`: `closed` rejects unlisted discriminants, and `#[other] {name}` keeps them. \
                 Remove one",
                input.ident,
            ),
        ));
    }

    let mut choice = ChoiceDef::new(&crate::attr::name(&input.ident), arms).with_endian(endian);
    if let Some((other_name, _, other_bytes)) = open {
        choice = choice.with_other(&other_name);
        if let Some(bytes) = other_bytes {
            choice = choice.with_other_bytes(bytes);
        }
    }

    layline_codegen::validate(&layline_codegen::Item::Choice(choice.clone()))
        .map_err(|why| invalid(input, &choice, &why))?;

    let parts = layline_codegen::__derive::choice_parts(&choice, &root)
        .map_err(|why| syn::Error::new(input.ident.span(), format!("{why}")))?;

    let spanned = parts.checks.into_iter().map(|c| {
        let span = data
            .variants
            .iter()
            .find(|v| crate::attr::name(&v.ident) == c.field)
            .map_or_else(|| input.ident.span(), |v| v.ident.span());
        crate::check::respan(c.tokens, span)
    });
    let blocks = parts.blocks;
    let codec = parts.codec;
    Ok(quote! {
        #(#spanned)*
        #blocks
        #codec
    })
}

fn arm_body(v: &syn::Variant) -> syn::Result<Vec<Segment>> {
    let bytes = crate::attr::single(&v.attrs, "bytes", format_args!("variant `{}`", v.ident))?;
    if matches!(v.fields, syn::Fields::Unit) {
        if let Some(attr) = bytes {
            return Err(syn::Error::new(
                attr.meta.span(),
                format!(
                    "variant `{}`: a unit variant has no bytes to size. \
                     Add a payload, or remove `#[bytes(N)]`",
                    v.ident,
                ),
            ));
        }
        return Ok(Vec::new());
    }
    let ty = payload(v, &|| {
        format!(
            "variant `{}`: an arm needs one payload type. \
             Write `#[value(N)] {}(Body)`, where `Body` is a message, a `#[bytes(N)]` layout, or a scalar",
            v.ident, v.ident,
        )
    })?;

    let (ty, boxed) = match box_element(ty) {
        Some(inner) => (inner, true),
        None => (ty, false),
    };

    if let Some(s) = scalar_of(ty) {
        if let Some(attr) = bytes {
            return Err(syn::Error::new(
                attr.meta.span(),
                format!(
                    "variant `{}`: `{}` already has a fixed size. Remove #[bytes]",
                    v.ident,
                    crate::attr::spelled(ty),
                ),
            ));
        }
        return Ok(vec![Segment::Block(vec![Field::new("inner", Kind::Scalar(s))])]);
    }
    if variable_size(ty) || !is_plain_path(ty) {
        return Err(syn::Error::new(
            ty.span(),
            format!(
                "variant `{}`: `{}` cannot be an arm payload. \
                 Use a `#[derive(Message)]` type, a `#[bytes(N)]` layout, or a scalar, or mark the variant #[other]",
                v.ident,
                crate::attr::spelled(ty),
            ),
        ));
    }

    let Some(attr) = bytes else {
        return Ok(vec![Segment::Value {
            stated: None,
            name: String::from("inner"),
            kind: Kind::Msg { ty: crate::attr::spelled(ty), boxed, with: Vec::new() },
            doc: None,
        }]);
    };
    let lit: LitInt = attr.parse_args()?;
    let bytes: usize = lit.base10_parse()?;
    if bytes == 0 {
        return Err(syn::Error::new(
            lit.span(),
            format!(
                "variant `{}`: an arm cannot be zero bytes. Write #[bytes(N)] with N above zero",
                v.ident,
            ),
        ));
    }
    Ok(vec![Segment::Block(vec![Field::new(
        "inner",
        Kind::Nested { ty: crate::attr::spelled(ty), bytes },
    )])])
}

fn raw_variant(v: &syn::Variant) -> syn::Result<Option<usize>> {
    let shape = || {
        format!(
            "variant `{name}`: the #[other] variant holds unrecognised bytes. \
             Write `#[other] {name}(Vec<u8>)`, or `{name}([u8; N])` for a fixed-size union",
            name = v.ident,
        )
    };
    let ty = payload(v, &shape)?;
    if crate::ty::is_byte_vec(ty) {
        return Ok(None);
    }
    match crate::ty::array_of(ty) {
        Some((elem, n)) if scalar_of(elem) == Some(layline_codegen::Scalar::U(8)) => Ok(Some(n)),
        _ => Err(syn::Error::new(ty.span(), shape())),
    }
}

fn payload<'a>(v: &'a syn::Variant, shape: &dyn Fn() -> String) -> syn::Result<&'a Type> {
    let Fields::Unnamed(fields) = &v.fields else {
        return Err(syn::Error::new(v.ident.span(), shape()));
    };
    if fields.unnamed.len() != 1 {
        return Err(syn::Error::new(v.fields.span(), shape()));
    }
    Ok(&fields.unnamed[0].ty)
}

fn not_total(name: &str) -> String {
    format!(
        "`{name}`: unlisted discriminants have no arm. \
         Add `#[other] Unknown(Vec<u8>)`, or mark the enum `#[message(closed)]`"
    )
}

fn invalid(input: &DeriveInput, choice: &ChoiceDef, why: &layline_codegen::Invalid) -> syn::Error {
    match why {
        layline_codegen::Invalid::AfterOpenEnd(_) => syn::Error::new(
            input.ident.span(),
            format!(
                "`{}`: an arm has a field after one that reads to the end of the arm. \
                 Move that field last",
                choice.name,
            ),
        ),
        other => syn::Error::new(input.ident.span(), format!("`{}`: {other}", input.ident)),
    }
}
