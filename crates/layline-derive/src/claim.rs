//! Field attributes shared by `Layout` and `Message`: `#[at]`, `#[range]`, `#[magic]`, `#[checksum]`.

use proc_macro2::Span;
use syn::LitInt;
use syn::spanned::Spanned;

use layline_codegen::{CoverFrom, CoverTo, Coverage, Endian, Stated, StatedUnit};

/// A reference to another field of this struct.
///
/// The name may cross one dot, as in `header.n_entries`, to reach a field of an earlier nested layout.
pub(crate) struct Ref {
    pub(crate) name: String,
    pub(crate) span: Span,
}

impl Ref {
    /// Copies `r`. `Ref` is not `Clone`.
    pub(crate) fn same(r: &Ref) -> Ref {
        Ref { name: r.name.clone(), span: r.span }
    }
}

/// The position from `#[at(bit = N)]` or `#[at(byte = N)]`.
///
/// Numbered as declared, so MSB0 under `order = msb`.
#[derive(Debug, Clone, Copy)]
pub struct At {
    pub stated: Stated,
    pub span: Span,
}

/// `natural` is the unit suggested when the attribute gives a bare number.
pub(crate) fn at_attr(attr: &syn::Attribute, name: &str, natural: StatedUnit) -> syn::Result<At> {
    let span = attr.meta.span();
    if let Ok(lit) = attr.parse_args::<LitInt>() {
        return Err(syn::Error::new(span, bare_at_needs_a_unit(&lit, natural)));
    }

    let mut at: Option<At> = None;
    attr.parse_nested_meta(|meta| {
        let unit = if meta.path.is_ident("bit") {
            StatedUnit::Bit
        } else if meta.path.is_ident("byte") {
            StatedUnit::Byte
        } else {
            return Err(meta
                .error(format!("field `{name}`: expected `#[at(bit = N)]` or `#[at(byte = N)]`")));
        };
        if at.is_some() {
            return Err(
                meta.error(format!("field `{name}`: #[at] takes one position. Remove the second"))
            );
        }
        let pos = meta.value()?.parse::<LitInt>()?.base10_parse()?;
        at = Some(At { stated: Stated::new(unit, pos), span });
        Ok(())
    })?;

    at.ok_or_else(|| {
        syn::Error::new(
            span,
            format!("field `{name}`: expected `#[at(bit = N)]` or `#[at(byte = N)]`"),
        )
    })
}

fn bare_at_needs_a_unit(lit: &LitInt, natural: StatedUnit) -> String {
    let n = lit.base10_digits();
    format!("`#[at({n})]` needs a unit. Write `#[at({} = {n})]`", natural.name(),)
}

/// The values `#[range(lo..=hi)]` allows.
#[derive(Clone)]
pub struct Range {
    /// `None` means the type's minimum.
    pub lo: Option<i128>,
    /// `None` means the type's maximum.
    pub hi: Option<i128>,
    pub span: Span,
}

pub(crate) fn range_attr(attr: &syn::Attribute, ident: &syn::Ident) -> syn::Result<Range> {
    let shape = || {
        syn::Error::new(
            attr.meta.span(),
            format!(
                "field `{ident}`: `#[range]` takes a range of integer literals. \
                 Write `#[range(0..=255)]`, `#[range(..=255)]`, or `#[range(1..)]`"
            ),
        )
    };
    let expr: syn::ExprRange = attr.parse_args().map_err(|_| shape())?;
    if expr.end.is_some() && !matches!(expr.limits, syn::RangeLimits::Closed(_)) {
        return Err(syn::Error::new(
            attr.meta.span(),
            format!("field `{ident}`: `#[range]` needs an inclusive upper bound. Write `..=hi`"),
        ));
    }
    let bound = |e: Option<&syn::Expr>| -> syn::Result<Option<i128>> {
        let Some(e) = e else { return Ok(None) };
        let (negative, lit) = match e {
            syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => (true, &*u.expr),
            other => (false, other),
        };
        let syn::Expr::Lit(l) = lit else { return Err(shape()) };
        let syn::Lit::Int(n) = &l.lit else { return Err(shape()) };
        let v: i128 = n.base10_parse()?;
        Ok(Some(if negative { -v } else { v }))
    };
    let (lo, hi) = (bound(expr.start.as_deref())?, bound(expr.end.as_deref())?);
    if let (Some(lo), Some(hi)) = (lo, hi)
        && lo > hi
    {
        return Err(syn::Error::new(
            attr.meta.span(),
            format!("field `{ident}`: `#[range({lo}..={hi})]` is empty. Put the lower bound first"),
        ));
    }
    Ok(Range { lo, hi, span: attr.meta.span() })
}

/// The constant from `#[magic(b"SEAL")]` or `#[magic(0xAA55)]`.
pub struct Magic {
    pub lit: syn::Lit,
    pub span: Span,
}

pub(crate) fn magic_lit(attr: &syn::Attribute, ident: &syn::Ident) -> syn::Result<syn::Lit> {
    let lit: syn::Lit = attr.parse_args().map_err(|_| magic_shape(attr.meta.span(), ident))?;
    match &lit {
        syn::Lit::ByteStr(_) | syn::Lit::Int(_) => Ok(lit),
        syn::Lit::Str(s) => Err(syn::Error::new(
            s.span(),
            format!(
                "field `{ident}`: #[magic] takes bytes, not a string. Write `b\"{}\"`",
                s.value(),
            ),
        )),
        other => Err(magic_shape(other.span(), ident)),
    }
}

fn magic_shape(span: Span, ident: &syn::Ident) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "field `{ident}`: #[magic(..)] takes one literal. \
             Write a byte string such as `b\"SEAL\"`, or an integer such as `0xAA55`"
        ),
    )
}

pub(crate) const MAGIC_PINS_BYTES: &str =
    "#[magic] needs an integer scalar (`u8`..`u64`, `i8`..`i64`) or a `[u8; N]`";

/// The magic constant's bytes in wire order.
pub(crate) fn magic_bytes(
    m: &Magic,
    ident: &str,
    width: usize,
    endian: Endian,
) -> syn::Result<Vec<u8>> {
    match &m.lit {
        syn::Lit::ByteStr(s) => {
            let bytes = s.value();
            if bytes.len() != width {
                return Err(syn::Error::new(
                    m.span,
                    format!(
                        "field `{ident}`: #[magic] is {} byte{} but the field is {width}. \
                         Match the field's size",
                        bytes.len(),
                        if bytes.len() == 1 { "" } else { "s" },
                    ),
                ));
            }
            Ok(bytes)
        }
        syn::Lit::Int(n) => {
            let value: u128 = n.base10_parse()?;
            if width < 16 && value >= 1u128 << (width * 8) {
                return Err(syn::Error::new(
                    m.span,
                    format!(
                        "field `{ident}`: #[magic({})] does not fit {width} byte{}",
                        n.base10_digits(),
                        if width == 1 { "" } else { "s" },
                    ),
                ));
            }
            let le = value.to_le_bytes();
            let mut bytes: Vec<u8> = le[..width].to_vec();
            if matches!(endian, Endian::Be) {
                bytes.reverse();
            }
            Ok(bytes)
        }
        other => {
            Err(syn::Error::new(other.span(), format!("field `{ident}`: not a magic literal")))
        }
    }
}

/// `#[checksum(Algorithm, over = <range>)]`.
pub(crate) struct ChecksumSpec {
    pub(crate) algorithm: syn::Path,
    pub(crate) over: Coverage,
    pub(crate) from_ref: Option<Ref>,
    pub(crate) to_ref: Option<Ref>,
    /// Whether the end was written `..=`.
    pub(crate) inclusive: bool,
    pub(crate) span: Span,
}

/// Parses `#[checksum(Algorithm, over = <range>)]`. Both arguments are required.
///
/// The range bounds are field names:
///
/// ```text
/// over = ..            the whole message, up to this field
/// over = f..           from `f`, up to this field
/// over = f..g          from `f`, up to where `g` starts
/// over = f..=g         from `f`, through `g`'s last byte
/// ```
///
/// A range that ends past this field includes this field.
/// To cover to the end of the body, name its last field.
pub(crate) fn parse_checksum(attr: &syn::Attribute, name: &str) -> syn::Result<ChecksumSpec> {
    let span = attr.meta.span();
    let (algorithm, range) = attr
        .parse_args_with(|input: syn::parse::ParseStream| {
            let algorithm: syn::Path = input.parse()?;
            input.parse::<syn::Token![,]>()?;
            let over: syn::Ident = input.parse()?;
            if over != "over" {
                return Err(syn::Error::new(over.span(), "expected `over`"));
            }
            input.parse::<syn::Token![=]>()?;
            let range: syn::ExprRange = input.parse()?;
            Ok((algorithm, range))
        })
        .map_err(|_| checksum_shape(span, name))?;

    let inclusive = matches!(range.limits, syn::RangeLimits::Closed(_));

    let bound = |expr: Option<&syn::Expr>, which: &str| -> syn::Result<Option<Ref>> {
        let Some(expr) = expr else { return Ok(None) };
        if matches!(expr, syn::Expr::Field(_)) {
            return Err(syn::Error::new(
                expr.span(),
                format!(
                    "field `{name}`: `over` cannot {which} at a nested field. \
                     Name the nested layout itself"
                ),
            ));
        }
        let syn::Expr::Path(p) = expr else {
            return Err(checksum_shape(span, name));
        };
        let Some(ident) = p.path.get_ident() else {
            return Err(syn::Error::new(
                p.span(),
                format!(
                    "field `{name}`: `over` must {which} at a bare field name, not `{}`. \
                     Name a field of this struct",
                    crate::attr::spelled(&p.path),
                ),
            ));
        };
        Ok(Some(Ref { name: crate::attr::name(ident), span: ident.span() }))
    };

    let from_ref = bound(range.start.as_deref(), "begin")?;
    let to_ref = bound(range.end.as_deref(), "end")?;
    let over = Coverage::new(
        match &from_ref {
            None => CoverFrom::Start,
            Some(r) => CoverFrom::Field(r.name.clone()),
        },
        match (&to_ref, inclusive) {
            (None, _) => CoverTo::Here,
            (Some(r), false) => CoverTo::Before(r.name.clone()),
            (Some(r), true) => CoverTo::After(r.name.clone()),
        },
    );
    Ok(ChecksumSpec { algorithm, over, from_ref, to_ref, inclusive, span })
}

fn checksum_shape(span: Span, name: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "field `{name}`: #[checksum(..)] takes one form. \
             Write `#[checksum(Algorithm, over = <range>)]` with field names as the range bounds"
        ),
    )
}
