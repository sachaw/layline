//! `#[message(bits)]`: every field a run of bits on one cursor.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::Type;
use syn::spanned::Spanned;

use layline_codegen::{BitOrder, Field, Kind, MessageDef, Presence, Root, Scalar, Segment};

use super::attrs::parse_when;
use super::container::MESSAGE_ATTRS;
use super::lower::{flush, path};
use super::*;
use crate::attr::bare;
use crate::check;
use crate::ty::{is_plain_path, option_element, scalar_of, variable_size};
use crate::width::{nonzero, one_bit_bool};

/// The error for a byte-message attribute on a bit-addressed field.
fn not_in_a_bits_message(name: &str, word: &str) -> String {
    let why = match word {
        "count" | "len" | "fill" | "until" | "stride" => {
            "Declare the collection in the enclosing message"
        }
        "seek" => "Declare the record in the enclosing message",
        "at" => "Remove it. `SEGMENTS` gives each field's bit offset",
        "switch" => "Declare the switch in the enclosing message",
        "text" | "message" => "Declare the field in the enclosing message",
        "checksum" => "Declare the checksum in the enclosing message",
        "magic" | "range" => "Check the value after decoding",
        "with" => "Declare the nested message in the enclosing message",
        "bytes" | "codec" => "Write `#[bits(N)]`",
        _ => "Put it on a variant of the enum a `#[switch]` names",
    };
    format!("field `{name}`: `#[{word}]` does not apply in a bit-addressed message. {why}")
}

pub(super) fn bit_field(f: &syn::Field) -> syn::Result<MsgField> {
    let name = crate::attr::name(f.ident.as_ref().expect("named field"));
    let span = f.ident.as_ref().map_or_else(|| f.span(), syn::Ident::span);
    let mut bits: Option<(u32, Span)> = None;
    let mut present: Option<Span> = None;
    let mut when: Option<WhenSpec> = None;
    let mut var: Option<Span> = None;

    let owner = format!("field `{name}`");
    for attr in &f.attrs {
        let aspan = attr.meta.span();
        if attr.path().is_ident("bits") {
            crate::attr::once_attr(&mut bits, attr, &owner, || crate::attr::int_arg(attr))?;
        } else if attr.path().is_ident("present") {
            crate::attr::once_attr(&mut present, attr, &owner, || {
                bare(attr, &name, "#[present]").map(|()| aspan)
            })?;
        } else if attr.path().is_ident("when") {
            crate::attr::once_attr(&mut when, attr, &owner, || parse_when(attr, &name))?;
        } else if attr.path().is_ident("var") {
            crate::attr::once_attr(&mut var, attr, &owner, || {
                bare(attr, &name, "#[var]").map(|()| aspan)
            })?;
        } else if let Some(word) = crate::attr::registered(attr, MESSAGE_ATTRS) {
            return Err(syn::Error::new(aspan, not_in_a_bits_message(&name, word)));
        }
    }

    if let Some(when) = &when
        && matches!(when.test, WhenTest::Positive)
    {
        return Err(syn::Error::new(
            when.span,
            format!(
                "field `{name}`: `#[when({} > 0)]` works only on a `#[seek]` record. \
                 Write `#[when({} & 0x01)]`, or `#[present]` for a presence bit",
                when.by.name, when.by.name,
            ),
        ));
    }
    if let (Some(_), Some(when)) = (present, &when) {
        return Err(syn::Error::new(
            when.span,
            format!("field `{name}`: `#[present]` and `#[when]` both gate this field. Use one"),
        ));
    }
    if let (Some(vspan), Some((width, _))) = (var, bits) {
        return Err(syn::Error::new(
            vspan,
            format!(
                "field `{name}`: `#[var]` and `#[bits({width})]` both set the width. Remove one"
            ),
        ));
    }

    let walk_span =
        present.or(var).or(when.as_ref().map(|w| w.span)).or(bits.map(|(_, s)| s)).unwrap_or(span);
    let absent = present.is_some() || when.is_some();
    let inner = option_element(&f.ty);
    let of = |ty: &Type| -> syn::Result<Kind> {
        match var {
            Some(vspan) => bit_var_kind(&name, ty, vspan),
            None => bit_kind(&name, ty, bits),
        }
    };
    let kind = match (absent, inner) {
        (true, Some(inner)) => of(inner)?,
        (true, None) => {
            let what = if present.is_some() { "#[present]" } else { "#[when]" };
            return Err(syn::Error::new(
                present.or(when.as_ref().map(|w| w.span)).expect("one of the two"),
                format!(
                    "field `{name}`: `{what}` needs an `Option` field. \
                     Declare it `Option<{}>`, or remove `{what}`",
                    crate::attr::spelled(&f.ty),
                ),
            ));
        }
        (false, Some(_)) => {
            return Err(syn::Error::new(
                span,
                format!(
                    "field `{name}`: nothing says whether this `Option` is present. \
                     Add `#[present]` for a presence bit before the field"
                ),
            ));
        }
        (false, None) => of(&f.ty)?,
    };

    let absent = match (present, when) {
        (Some(_), _) => Some(BitPresence::Present),
        (None, Some(when)) => Some(BitPresence::When(when)),
        (None, None) => None,
    };

    Ok(MsgField {
        name,
        span,
        walk_span,
        body: Body::Bits { kind, absent },
        stated: None,
        with: Vec::new(),
    })
}

fn bit_var_kind(name: &str, ty: &Type, vspan: Span) -> syn::Result<Kind> {
    if !is_plain_path(ty) || variable_size(ty) {
        return Err(syn::Error::new(
            vspan,
            format!(
                "field `{name}`: `#[var]` needs a `BitCodec` type, not `{}`. \
                 Use a type such as `layline::num::ExpGolomb`",
                crate::attr::spelled(ty),
            ),
        ));
    }
    Ok(Kind::Var { ty: crate::attr::spelled(ty) })
}

fn bit_kind(name: &str, ty: &Type, declared: Option<(u32, Span)>) -> syn::Result<Kind> {
    let width = crate::width::declared_width(name, "bits", ty, declared)?;
    let too_wide = |bits: u32, span: Span, max: u32, why: &str| {
        syn::Error::new(
            span,
            format!(
                "field `{name}`: #[bits({bits})] is wider than {why}, which holds {max} bits. \
                 Use at most {max} bits"
            ),
        )
    };

    if crate::ty::bare_ident(ty).is_some_and(|id| id == "bool") {
        if let Some((bits, bspan)) = width {
            one_bit_bool(name, bits, bspan)?;
        }
        return Ok(Kind::Scalar(Scalar::Bool));
    }

    if let Some(s) = scalar_of(ty) {
        let (signed, prim) = match s {
            Scalar::U(n) => (false, n as u32),
            Scalar::I(n) => (true, n as u32),
            _ => {
                let carried = if matches!(s, Scalar::F32) { 32 } else { 64 };
                if let Some((bits, bspan)) = width
                    && bits != carried
                {
                    return Err(syn::Error::new(
                        bspan,
                        format!(
                            "field `{name}`: #[bits({bits})] does not fit `{}`, which is always {carried} bits. \
                             Write `#[bits({carried})]`, or remove it",
                            crate::attr::spelled(ty),
                        ),
                    ));
                }
                return Ok(Kind::Scalar(s));
            }
        };
        let Some((bits, bspan)) = width else {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "field `{name}`: `{}` needs a width in a bit-addressed message. \
                     Add `#[bits(N)]` with N from 1 to {prim}, or use `U<N>` or `I<N>`",
                    crate::attr::spelled(ty),
                ),
            ));
        };
        nonzero(name, bits, bspan)?;
        if bits > prim {
            return Err(too_wide(bits, bspan, prim, &format!("`{}`", crate::attr::spelled(ty))));
        }
        let n = u64::from(bits);
        return Ok(Kind::Scalar(if signed { Scalar::I(n) } else { Scalar::U(n) }));
    }

    if (is_plain_path(ty) || crate::width::carried_bits(ty).is_some()) && !variable_size(ty) {
        let Some((bits, bspan)) = width else {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "field `{name}`: `{}` needs a width. \
                     Add `#[bits(N)]` for a `FieldCodec` type, or use `U<N>` or `I<N>`",
                    crate::attr::spelled(ty),
                ),
            ));
        };
        nonzero(name, bits, bspan)?;
        if bits > 64 {
            return Err(too_wide(bits, bspan, 64, "a bit field"));
        }
        return Ok(Kind::Codec { ty: crate::attr::spelled(ty), bits: u64::from(bits) });
    }

    Err(syn::Error::new(
        ty.span(),
        format!(
            "field `{name}`: `{}` cannot be a bit field. \
             Use a scalar with `#[bits(N)]`, `U<N>`, `I<N>`, or a `FieldCodec` type with `#[bits(N)]`",
            crate::attr::spelled(ty),
        ),
    ))
}

/// Builds a bit-addressed message: a block per run of fixed fields, an `Opt` per optional field,
/// and a `Value` per `#[var]`.
pub(super) fn lower_bits(p: &Parsed, order: BitOrder) -> (MessageDef, Vec<TokenStream>) {
    let mut segments: Vec<Segment> = Vec::new();
    let mut block: Vec<Field> = Vec::new();
    let mut checks: Vec<TokenStream> = Vec::new();
    for f in &p.fields {
        let Body::Bits { kind, absent } = &f.body else {
            unreachable!("every field of a bit-addressed message is parsed by `bit_field`")
        };
        if let Kind::Var { ty } = kind {
            checks.push(bit_codec_check(&f.name, ty, f.span, order, &p.root));
        }
        let field = Field::new(&f.name, kind.clone());
        match absent {
            Some(absent) => {
                flush(&mut segments, &mut block);
                let when = match absent {
                    BitPresence::Present => Presence::Bit,
                    BitPresence::When(when) => when.presence(),
                };
                segments.push(Segment::opt(when, field));
            }
            None if matches!(kind, Kind::Var { .. }) => {
                flush(&mut segments, &mut block);
                segments.push(Segment::Value {
                    name: f.name.clone(),
                    doc: None,
                    stated: None,
                    kind: kind.clone(),
                });
            }
            None => block.push(field),
        }
    }
    flush(&mut segments, &mut block);
    (MessageDef::new(&crate::attr::name(&p.ident), segments).with_bits(order), checks)
}

/// Requires a `#[var]` type to implement `BitCodec` in the message's bit order.
fn bit_codec_check(name: &str, ty: &str, span: Span, order: BitOrder, root: &Root) -> TokenStream {
    let msb = order == BitOrder::Msb;
    check::bound("reads_its_own_bits", name, &path(ty), quote!(BitCodec<#msb>), span, root)
}
