//! Parses one field of each layout kind.

use proc_macro2::Span;
use syn::Type;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;

use layline_codegen::StatedUnit;

use super::declared::*;
use super::misplaced::{no_assertions_in_bit_mode, refuse_message_vocabulary, refuse_unbound};
use crate::attr::int_arg;
use crate::claim::{At, MAGIC_PINS_BYTES, Magic, Range, at_attr, magic_lit, range_attr};
use crate::ty::{array_dims, array_of, bare_ident};
use crate::width::{
    BYTE_MODE_HAS_NO_BIT_FIELDS, declared_width, nonzero, one_bit_bool, width_said,
};

struct OverlayArgs {
    name: syn::Ident,
    ty: Type,
}

impl Parse for OverlayArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse()?;
        input.parse::<syn::Token![:]>()?;
        let ty = input.parse()?;
        Ok(Self { name, ty })
    }
}

fn wider_than_the_accessor(ident: &syn::Ident, bits: u32, max: u32) -> String {
    let mut msg = format!("field `{ident}`: #[bits({bits})] is wider than {max} bits");
    if max == 64 {
        msg.push_str(". Use `u128` for up to 128 bits, or split it into named fields");
    } else {
        msg.push_str(". Split it into named fields, or nest a `#[layout(bytes = N)]` layout");
    }
    msg
}

pub(super) fn word_field(f: &syn::Field) -> syn::Result<WordField> {
    let ident = f.ident.clone().expect("named field");
    let kind = word_kind(&f.ty);

    let mut bits: Option<(u32, Span)> = None;
    let mut at: Option<At> = None;
    let mut overlays = Vec::new();
    let mut range: Option<Range> = None;

    let owner = format!("field `{ident}`");
    for attr in &f.attrs {
        if attr.path().is_ident("magic") || attr.path().is_ident("checksum") {
            return Err(no_assertions_in_bit_mode(attr, &ident));
        }
        refuse_message_vocabulary(attr, &ident, "a `bits = N` layout")?;
        if attr.path().is_ident("bytes") {
            return Err(syn::Error::new(
                attr.meta.span(),
                format!(
                    "field `{ident}`: #[bytes] does not apply in a `bits = N` layout. \
                     Write #[bits(N)]"
                ),
            ));
        }
        if attr.path().is_ident("range") {
            crate::attr::once_attr(&mut range, attr, &owner, || range_attr(attr, &ident))?;
        } else if attr.path().is_ident("bits") {
            crate::attr::once_attr(&mut bits, attr, &owner, || int_arg(attr))?;
        } else if attr.path().is_ident("at") {
            crate::attr::once_attr(&mut at, attr, &owner, || {
                at_attr(attr, &ident.to_string(), StatedUnit::Bit)
            })?;
        } else if attr.path().is_ident("overlay") {
            let args: OverlayArgs = attr.parse_args()?;
            overlays.push(Overlay { name: args.name, ty: args.ty, span: attr.meta.span() });
        } else {
            refuse_unbound(attr, &ident, "a `bits = N` layout")?;
        }
    }

    let (bits, bits_span) =
        declared_width(&ident.to_string(), "bits", &f.ty, bits)?.ok_or_else(|| {
            syn::Error::new(
                ident.span(),
                format!(
                    "field `{ident}`: no width. Write #[bits(N)], or use a `U<N>` or `I<N>` type"
                ),
            )
        })?;

    let max = if matches!(kind, WordKind::Uint(128)) { 128 } else { 64 };
    nonzero(&ident.to_string(), bits, bits_span)?;
    if bits > max {
        return Err(syn::Error::new(bits_span, wider_than_the_accessor(&ident, bits, max)));
    }
    fits_its_type(&ident, &kind, bits, bits_span)?;

    Ok(WordField { ident, ty: f.ty.clone(), kind, bits, bits_span, at, overlays, range })
}

pub(super) fn byte_field(f: &syn::Field) -> syn::Result<ByteField> {
    let ident = f.ident.clone().expect("named field");
    let span = f.ident.as_ref().map_or_else(|| f.span(), syn::Ident::span);

    let mut bits: Option<(u32, Span)> = None;
    let mut bytes: Option<(usize, Span)> = None;
    let mut at: Option<At> = None;
    let mut magic: Option<Magic> = None;
    let mut check: Option<crate::claim::ChecksumSpec> = None;
    let mut range: Option<Range> = None;
    let owner = format!("field `{ident}`");
    for attr in &f.attrs {
        refuse_message_vocabulary(attr, &ident, "a `bytes = N` layout")?;
        if attr.path().is_ident("magic") {
            crate::attr::once_attr(&mut magic, attr, &owner, || {
                Ok(Magic { lit: magic_lit(attr, &ident)?, span: attr.meta.span() })
            })?;
        } else if attr.path().is_ident("checksum") {
            crate::attr::once_attr(&mut check, attr, &owner, || {
                crate::claim::parse_checksum(attr, &ident.to_string())
            })?;
        } else if attr.path().is_ident("range") {
            crate::attr::once_attr(&mut range, attr, &owner, || range_attr(attr, &ident))?;
        } else if attr.path().is_ident("codec") {
            crate::attr::once_attr(&mut bits, attr, &owner, || int_arg(attr))?;
        } else if attr.path().is_ident("bytes") {
            crate::attr::once_attr(&mut bytes, attr, &owner, || int_arg(attr))?;
        } else if attr.path().is_ident("bits") {
            return Err(syn::Error::new(
                attr.meta.span(),
                format!("field `{ident}`: {BYTE_MODE_HAS_NO_BIT_FIELDS}"),
            ));
        } else if attr.path().is_ident("overlay") {
            return Err(syn::Error::new(
                attr.meta.span(),
                format!("field `{ident}`: #[overlay] does not apply in a `bytes = N` layout"),
            ));
        } else if attr.path().is_ident("at") {
            crate::attr::once_attr(&mut at, attr, &owner, || {
                at_attr(attr, &ident.to_string(), StatedUnit::Byte)
            })?;
        } else {
            refuse_unbound(attr, &ident, "a `bytes = N` layout")?;
        }
    }

    if bytes.is_none() {
        bits = declared_width(&ident.to_string(), "codec", &f.ty, bits)?;
    }

    let kind = if let Some((bytes, sspan)) = bytes {
        if bits.is_some() {
            return Err(syn::Error::new(
                sspan,
                format!("field `{ident}`: #[codec] and #[bytes] are both set. Use one"),
            ));
        }
        if bytes == 0 {
            return Err(syn::Error::new(
                sspan,
                format!("field `{ident}`: #[bytes(0)] is empty. A nested layout needs a size"),
            ));
        }
        if let Some((elem, len)) = array_of(&f.ty) {
            ByteKind::NestedArray(
                Box::new(NestedField { ty: elem.clone(), bytes, span: sspan }),
                len,
            )
        } else {
            ByteKind::Nested(Box::new(NestedField { ty: f.ty.clone(), bytes, span: sspan }))
        }
    } else if let Some((bits, bits_span)) = bits {
        if let Some((elem, len)) = array_of(&f.ty)
            && byte_kind(&f.ty).is_none()
        {
            if !matches!(bits, 8 | 16 | 32 | 64) {
                return Err(syn::Error::new(
                    bits_span,
                    format!(
                        "field `{ident}`: {} is not 8, 16, 32, or 64. \
                         A codec array element must fill a whole scalar",
                        width_said("codec", bits, elem),
                    ),
                ));
            }
            let kind = ByteKind::CodecArray(
                Box::new(CodecField { ty: elem.clone(), bits, bits_span }),
                len,
            );
            assertable(&ident, &kind, magic.as_ref(), check.as_ref())?;
            return Ok(ByteField { ident, kind, span, at, magic, check, range });
        }
        if byte_kind(&f.ty).is_some() {
            return Err(syn::Error::new(
                bits_span,
                format!(
                    "field `{ident}`: a scalar takes its width from its type. \
                     Remove the #[codec(N)]"
                ),
            ));
        }
        if !matches!(bits, 8 | 16 | 32 | 64) {
            return Err(syn::Error::new(
                bits_span,
                format!(
                    "field `{ident}`: {} is not 8, 16, 32, or 64. \
                     A codec field must fill a whole scalar",
                    width_said("codec", bits, &f.ty),
                ),
            ));
        }
        ByteKind::Codec(Box::new(CodecField { ty: f.ty.clone(), bits, bits_span }))
    } else {
        byte_kind(&f.ty).ok_or_else(|| {
            syn::Error::new(
                f.ty.span(),
                format!(
                    "field `{ident}`: unsupported type. Use a scalar (u8..u64, i8..i64, f32, \
                     f64), a scalar array, or a `FieldCodec` type with #[codec(N)]"
                ),
            )
        })?
    };

    assertable(&ident, &kind, magic.as_ref(), check.as_ref())?;
    Ok(ByteField { ident, kind, span, at, magic, check, range })
}

fn assertable(
    ident: &syn::Ident,
    kind: &ByteKind,
    magic: Option<&Magic>,
    check: Option<&crate::claim::ChecksumSpec>,
) -> syn::Result<()> {
    if let (Some(m), Some(_)) = (magic, check) {
        return Err(syn::Error::new(
            m.span,
            format!(
                "field `{ident}`: #[magic] and #[checksum] cannot share a field. \
                 Put them on separate fields"
            ),
        ));
    }
    if let Some(m) = magic {
        let whole_bytes = match kind {
            ByteKind::Scalar(s) => !matches!(s, Prim::F32 | Prim::F64),
            ByteKind::Array(Prim::U8, _) => true,
            _ => false,
        };
        if !whole_bytes {
            return Err(syn::Error::new(m.span, format!("field `{ident}`: {MAGIC_PINS_BYTES}")));
        }
        if matches!(kind, ByteKind::Array(..)) && matches!(m.lit, syn::Lit::Int(_)) {
            return Err(syn::Error::new(
                m.span,
                format!(
                    "field `{ident}`: a `[u8; N]` field needs a byte string magic. \
                     Write `#[magic(b\"..\")]`"
                ),
            ));
        }
    }
    if let Some(c) = check
        && !matches!(kind, ByteKind::Scalar(s) if !matches!(s, Prim::F32 | Prim::F64))
    {
        return Err(syn::Error::new(
            c.span,
            format!(
                "field `{ident}`: a #[checksum] field must be an integer scalar \
                 (`u8`..`u64`, `i8`..`i64`)"
            ),
        ));
    }
    Ok(())
}

pub(super) fn words_field(f: &syn::Field) -> syn::Result<WordsField> {
    let ident = f.ident.clone().expect("named field");
    let span = f.ident.as_ref().map_or_else(|| f.span(), syn::Ident::span);

    let mut bits: Option<(u32, Span)> = None;
    let mut bytes: Option<(usize, Span)> = None;
    let mut at: Option<At> = None;
    let mut overlays = Vec::new();
    let mut range: Option<Range> = None;
    let owner = format!("field `{ident}`");
    for attr in &f.attrs {
        if attr.path().is_ident("magic") || attr.path().is_ident("checksum") {
            return Err(no_assertions_in_bit_mode(attr, &ident));
        }
        refuse_message_vocabulary(attr, &ident, "a `words = N` layout")?;
        if attr.path().is_ident("range") {
            crate::attr::once_attr(&mut range, attr, &owner, || range_attr(attr, &ident))?;
        } else if attr.path().is_ident("bits") {
            crate::attr::once_attr(&mut bits, attr, &owner, || int_arg(attr))?;
        } else if attr.path().is_ident("bytes") {
            crate::attr::once_attr(&mut bytes, attr, &owner, || int_arg(attr))?;
        } else if attr.path().is_ident("at") {
            crate::attr::once_attr(&mut at, attr, &owner, || {
                at_attr(attr, &ident.to_string(), StatedUnit::Bit)
            })?;
        } else if attr.path().is_ident("overlay") {
            let args: OverlayArgs = attr.parse_args()?;
            overlays.push(Overlay { name: args.name, ty: args.ty, span: attr.meta.span() });
        } else {
            refuse_unbound(attr, &ident, "a `words = N` layout")?;
        }
    }
    if bytes.is_none() {
        bits = declared_width(&ident.to_string(), "bits", &f.ty, bits)?;
    }
    if !overlays.is_empty() && bits.is_none() {
        return Err(syn::Error::new(
            ident.span(),
            format!("field `{ident}`: #[overlay] needs a #[bits(N)] field"),
        ));
    }

    let kind = match (bits, bytes) {
        (Some(_), Some((_, sspan))) => {
            return Err(syn::Error::new(
                sspan,
                format!("field `{ident}`: #[bits] and #[bytes] are both set. Use one"),
            ));
        }
        (Some((bits, bits_span)), None) => {
            if bits == 0 || bits > 16 {
                return Err(syn::Error::new(
                    bits_span,
                    format!(
                        "field `{ident}`: #[bits({bits})] is outside 1..=16. \
                         A field of a `words = N` layout must fit in one word"
                    ),
                ));
            }
            let wk = word_kind(&f.ty);
            fits_its_type(&ident, &wk, bits, bits_span)?;
            WordsKind::Bits { ty: f.ty.clone(), wk, bits, bits_span }
        }
        (None, Some((bytes, sspan))) => {
            if bytes == 0 || bytes % 2 != 0 {
                return Err(syn::Error::new(
                    sspan,
                    format!(
                        "field `{ident}`: #[bytes({bytes})] is not a whole number of words. \
                         Use an even, nonzero byte count"
                    ),
                ));
            }
            WordsKind::Nested { ty: f.ty.clone(), bytes, span: sspan }
        }
        (None, None) => {
            if let Some((elem, len)) = array_of(&f.ty)
                && let Some(elem) = bare_ident(elem)
            {
                match elem.to_string().as_str() {
                    "u16" => WordsKind::ArrayU16 { len },
                    "u8" => WordsKind::ArrayU8 { len },
                    other => {
                        return Err(syn::Error::new(
                            f.ty.span(),
                            format!(
                                "field `{ident}`: a `words = N` layout supports `[u16; K]` \
                                 and `[u8; K]` arrays, not `[{other}; K]`"
                            ),
                        ));
                    }
                }
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    format!(
                        "field `{ident}`: no width. Write #[bits(N)] for a bit field, \
                         #[bytes(N)] for a nested layout, or use `[u16; K]` or `[u8; K]`"
                    ),
                ));
            }
        }
    };

    Ok(WordsField { ident, kind, span, at, overlays, range })
}

fn fits_its_type(ident: &syn::Ident, kind: &WordKind, bits: u32, span: Span) -> syn::Result<()> {
    match *kind {
        WordKind::Uint(width) | WordKind::Int(width) if bits > width => Err(syn::Error::new(
            span,
            format!("field `{ident}`: #[bits({bits})] is wider than its {width}-bit type"),
        )),
        WordKind::Bool => one_bit_bool(&ident.to_string(), bits, span),
        _ => Ok(()),
    }
}

fn word_kind(ty: &Type) -> WordKind {
    if let Some(ident) = bare_ident(ty) {
        return match ident.to_string().as_str() {
            "u8" => WordKind::Uint(8),
            "u16" => WordKind::Uint(16),
            "u32" => WordKind::Uint(32),
            "u64" => WordKind::Uint(64),
            "u128" => WordKind::Uint(128),
            "i8" => WordKind::Int(8),
            "i16" => WordKind::Int(16),
            "i32" => WordKind::Int(32),
            "i64" => WordKind::Int(64),
            "bool" => WordKind::Bool,
            _ => WordKind::Codec,
        };
    }
    WordKind::Codec
}

fn byte_kind(ty: &Type) -> Option<ByteKind> {
    if let Some(ident) = bare_ident(ty) {
        return Prim::of(ident).map(ByteKind::Scalar);
    }
    let (elem, dims) = array_dims(ty)?;
    Prim::of(bare_ident(elem)?).map(|scalar| ByteKind::Array(scalar, dims))
}
