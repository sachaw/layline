//! Field attribute parsing.

use proc_macro2::Span;
use syn::LitInt;
use syn::spanned::Spanned;

use super::container::MESSAGE_ATTRS;
use super::*;
use crate::attr::bare;
use crate::claim::{Ref, parse_checksum};

fn field_ref(input: syn::parse::ParseStream<'_>) -> syn::Result<Ref> {
    let first: syn::Ident = input.parse()?;
    let span = first.span();
    let mut name = crate::attr::name(&first);
    dotted_tail(input, &mut name)?;
    Ok(Ref { name, span })
}

fn dotted_tail(input: syn::parse::ParseStream<'_>, name: &mut String) -> syn::Result<()> {
    if !input.peek(syn::Token![.]) {
        return Ok(());
    }
    input.parse::<syn::Token![.]>()?;
    let inner: syn::Ident = input.parse()?;
    name.push('.');
    name.push_str(&crate::attr::name(&inner));
    if input.peek(syn::Token![.]) {
        return Err(too_deep(inner.span(), name));
    }
    Ok(())
}

fn too_deep(span: Span, so_far: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "`{so_far}.…` reaches more than one level deep. \
             Name a field of this struct, or a field of one of its nested layouts"
        ),
    )
}

fn field_expr_ref(f: &syn::ExprField) -> Option<Ref> {
    let syn::Expr::Path(base) = &*f.base else {
        return None;
    };
    let outer = base.path.get_ident()?;
    let syn::Member::Named(inner) = &f.member else {
        return None;
    };
    Some(Ref { name: format!("{outer}.{inner}"), span: outer.span() })
}

fn set_kind(
    slot: &mut Option<(KindWord, Span)>,
    word: KindWord,
    span: Span,
    name: &str,
) -> syn::Result<()> {
    if let Some((have, _)) = slot {
        let msg = if core::mem::discriminant(have) == core::mem::discriminant(&word) {
            format!(
                "field `{name}`: duplicate {} attribute.{}",
                word.spelling(),
                word.duplicate_note()
            )
        } else {
            format!(
                "field `{name}`: {} and {} conflict. \
                 Use at most one of `#[text]`, `#[var]`, `#[message]`, `#[switch]`, `#[checksum]`",
                have.spelling(),
                word.spelling()
            )
        };
        return Err(syn::Error::new(span, msg));
    }
    *slot = Some((word, span));
    Ok(())
}

pub(super) fn parse_attrs(f: &syn::Field, name: &str) -> syn::Result<FieldAttrs> {
    let mut a = FieldAttrs {
        magic: None,
        kind: None,
        count: None,
        stride: None,
        at: None,
        seek: None,
        bits: None,
        bytes: None,
        until: None,
        when: None,
        with: None,
        range: None,
    };

    let owner = format!("field `{name}`");
    for attr in &f.attrs {
        let path = attr.path();
        let aspan = attr.meta.span();
        if path.is_ident("count") {
            reject_second(&a.count, aspan, name)?;
            a.count = Some((parse_count(attr, name, false)?, aspan));
        } else if path.is_ident("len") {
            reject_second(&a.count, aspan, name)?;
            a.count = Some((parse_count(attr, name, true)?, aspan));
        } else if path.is_ident("stride") {
            crate::attr::once_attr(&mut a.stride, attr, &owner, || parse_stride(attr, name))?;
        } else if path.is_ident("fill") {
            reject_second(&a.count, aspan, name)?;
            a.count = Some((CountSpec::Fill { cap: parse_fill(attr, name)? }, aspan));
        } else if path.is_ident("at") {
            crate::attr::once_attr(&mut a.at, attr, &owner, || parse_at(attr, name))?;
        } else if path.is_ident("seek") {
            crate::attr::once_attr(&mut a.seek, attr, &owner, || parse_seek(attr, name))?;
        } else if path.is_ident("codec") {
            crate::attr::once_attr(&mut a.bits, attr, &owner, || crate::attr::int_arg(attr))?;
        } else if path.is_ident("bits") {
            return Err(syn::Error::new(
                aspan,
                format!("field `{name}`: {}", crate::width::BYTE_MODE_HAS_NO_BIT_FIELDS),
            ));
        } else if path.is_ident("bytes") {
            crate::attr::once_attr(&mut a.bytes, attr, &owner, || crate::attr::int_arg(attr))?;
        } else if path.is_ident("text") {
            set_kind(&mut a.kind, KindWord::Text(parse_text(attr, name)?), aspan, name)?;
        } else if path.is_ident("until") {
            crate::attr::once_attr(&mut a.until, attr, &owner, || {
                Ok((parse_until(attr, name)?, aspan))
            })?;
        } else if path.is_ident("var") {
            bare(attr, name, "#[var]")?;
            set_kind(&mut a.kind, KindWord::Var, aspan, name)?;
        } else if path.is_ident("message") {
            bare(attr, name, "#[message]")?;
            set_kind(&mut a.kind, KindWord::Message, aspan, name)?;
        } else if path.is_ident("switch") {
            set_kind(&mut a.kind, KindWord::Switch(parse_switch(attr, name)?), aspan, name)?;
        } else if path.is_ident("present") {
            return Err(syn::Error::new(
                aspan,
                format!(
                    "field `{name}`: `#[present]` needs a bit-addressed message. \
                     Use `#[when(flags & 0x01)]`, or add `#[message(bits)]`"
                ),
            ));
        } else if path.is_ident("when") {
            crate::attr::once_attr(&mut a.when, attr, &owner, || parse_when(attr, name))?;
        } else if path.is_ident("with") {
            crate::attr::once_attr(&mut a.with, attr, &owner, || {
                Ok((parse_with(attr, name)?, aspan))
            })?;
        } else if path.is_ident("checksum") {
            set_kind(&mut a.kind, KindWord::Checksum(parse_checksum(attr, name)?), aspan, name)?;
        } else if path.is_ident("range") {
            crate::attr::once_attr(&mut a.range, attr, &owner, || {
                crate::claim::range_attr(attr, f.ident.as_ref().expect("named"))
            })?;
        } else if path.is_ident("magic") {
            crate::attr::once_attr(&mut a.magic, attr, &owner, || {
                let lit = crate::claim::magic_lit(attr, f.ident.as_ref().expect("named"))?;
                Ok(crate::claim::Magic { lit, span: aspan })
            })?;
        } else if let Some(word) = crate::attr::registered(attr, MESSAGE_ATTRS) {
            let msg = match word {
                "value" | "other" => format!(
                    "field `{name}`: `#[{word}]` belongs on an enum variant. \
                     Use `#[switch(n)]` here and `#[value(N)]` on the arms"
                ),
                other => format!(
                    "field `{name}`: `#[{other}]` is not valid on a message field. Remove it"
                ),
            };
            return Err(syn::Error::new(aspan, msg));
        }
    }

    if let (Some(at), Some(seek)) = (&a.at, &a.seek) {
        return Err(syn::Error::new(
            at.span,
            format!(
                "field `{name}`: `#[at({} = {})]` conflicts with `#[seek({})]`, which reads the position from `{}`. \
                 Remove `#[at]`",
                at.stated.unit.name(),
                at.stated.pos,
                seek.name,
                seek.name,
            ),
        ));
    }

    Ok(a)
}

fn parse_text(attr: &syn::Attribute, name: &str) -> syn::Result<Option<String>> {
    match &attr.meta {
        syn::Meta::Path(_) => Ok(None),
        syn::Meta::List(list) => {
            let codec: syn::Path = list.parse_args().map_err(|_| {
                syn::Error::new(
                    list.span(),
                    format!(
                        "field `{name}`: #[text(..)] takes one `TextCodec` type, such as `#[text(Latin1)]`. \
                         Bare `#[text]` is UTF-8"
                    ),
                )
            })?;
            Ok(Some(crate::attr::spelled(&codec)))
        }
        syn::Meta::NameValue(nv) => Err(syn::Error::new(
            nv.span(),
            format!(
                "field `{name}`: write `#[text]` for UTF-8, or `#[text(Codec)]` naming a `TextCodec`"
            ),
        )),
    }
}

fn parse_until(attr: &syn::Attribute, name: &str) -> syn::Result<UntilSpec> {
    if let Ok(lit) = attr.parse_args::<syn::LitByte>() {
        return Ok(UntilSpec::Byte(lit.value()));
    }
    if let Ok(lit) = attr.parse_args::<LitInt>() {
        return lit.base10_parse::<u8>().map(UntilSpec::Byte).map_err(|_| {
            syn::Error::new(
                lit.span(),
                format!("field `{name}`: a terminator is one byte, in `0..=255`"),
            )
        });
    }
    let mut mask: Option<u8> = None;
    let parsed = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("mask") {
            let lit = meta.value()?.parse::<LitInt>()?;
            mask = Some(lit.base10_parse::<u8>().map_err(|_| {
                syn::Error::new(
                    lit.span(),
                    format!("field `{name}`: the mask tests one byte, so it must be in `0..=255`"),
                )
            })?);
            Ok(())
        } else {
            Err(meta
                .error(format!("field `{name}`: expected `#[until(b)]` or `#[until(mask = M)]`")))
        }
    });
    match (parsed, mask) {
        (Ok(()), Some(0)) => Err(syn::Error::new(
            attr.meta.span(),
            format!(
                "field `{name}`: `#[until(mask = 0)]` masks no bits. \
                 Name the bits the last element sets"
            ),
        )),
        (Ok(()), Some(mask)) => Ok(UntilSpec::Mask(mask)),
        _ => Err(syn::Error::new(
            attr.meta.span(),
            format!("field `{name}`: expected `#[until(b)]` or `#[until(mask = M)]`"),
        )),
    }
}

pub(super) fn reject_second(
    count: &Option<(CountSpec, Span)>,
    span: Span,
    name: &str,
) -> syn::Result<()> {
    if let Some((first, fspan)) = count {
        let mut err = syn::Error::new(
            span,
            format!(
                "field `{name}`: {} conflicts with another count. Use one of {REPEAT_ATTRS}",
                first.spelling(),
            ),
        );
        err.combine(syn::Error::new(*fspan, "first declared here"));
        return Err(err);
    }
    Ok(())
}

fn parse_count(attr: &syn::Attribute, name: &str, bytes: bool) -> syn::Result<CountSpec> {
    let what = if bytes { "#[len]" } else { "#[count]" };
    let unit = if bytes { "byte length" } else { "count" };
    let mut by: Option<Ref> = None;
    let mut scale = 1u32;
    let mut offset = 0i32;
    let mut cap: Option<usize> = None;
    let mut scale_span = attr.meta.span();
    let mut square = false;
    let name_of = name;

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("scale") {
            scale_span = meta.path.span();
            scale = meta.value()?.parse::<LitInt>()?.base10_parse()?;
            Ok(())
        } else if meta.path.is_ident("offset") {
            let v = meta.value()?;
            let neg = v.parse::<syn::Token![-]>().is_ok();
            let n: i32 = v.parse::<LitInt>()?.base10_parse()?;
            offset = if neg { -n } else { n };
            Ok(())
        } else if meta.path.is_ident("cap") {
            cap = Some(meta.value()?.parse::<LitInt>()?.base10_parse()?);
            Ok(())
        } else if meta.input.peek(syn::Token![=]) {
            Err(meta.error(format!("field `{name}`: expected `scale`, `offset`, or `cap`")))
        } else if by.is_some() {
            Err(meta.error(format!(
                "field `{name}`: {what} takes one field name. \
                 Other arguments are `scale = S`, `offset = O` and `cap = N`",
            )))
        } else {
            let ident = meta.path.get_ident().ok_or_else(|| {
                meta.error(format!(
                    "field `{name}`: {what} takes a field name"
                ))
            })?;
            let mut name = crate::attr::name(ident);
            dotted_tail(meta.input, &mut name)?;
            if meta.input.peek(syn::Token![*]) {
                meta.input.parse::<syn::Token![*]>()?;
                let other: syn::Ident = meta.input.parse()?;
                let mut second = crate::attr::name(&other);
                dotted_tail(meta.input, &mut second)?;
                if second != name {
                    return Err(syn::Error::new(
                        other.span(),
                        format!(
                            "field `{name_owner}`: `{open}{name} * {second})]` names two different fields. \
                             Only a square is supported, such as `{open}{name} * {name})]`",
                            name_owner = name_of,
                            open = format_args!("{}(", what.trim_end_matches(']')),
                        ),
                    ));
                }
                square = true;
            }
            by = Some(Ref { name, span: ident.span() });
            Ok(())
        }
    })?;

    let by = by.ok_or_else(|| {
        syn::Error::new(
            attr.meta.span(),
            format!(
                "field `{name}`: {what} needs a field name. \
                 Write `{}(n)`, where `n` is an earlier field holding the {unit}",
                what.trim_end_matches(']'),
            ),
        )
    })?;
    if scale == 0 {
        return Err(syn::Error::new(
            scale_span,
            format!(
                "field `{name}`: `scale = 0` makes the {unit} a constant `{offset}`. \
                 Use a scale above zero, or a fixed-size array",
            ),
        ));
    }
    if square && bytes {
        return Err(syn::Error::new(
            attr.meta.span(),
            format!(
                "field `{name}`: `#[len]` cannot be a square. \
                 Use `#[count(n * n)]` to count elements"
            ),
        ));
    }
    Ok(match (bytes, square) {
        (true, _) => CountSpec::Window { by, scale, offset, cap },
        (false, true) => CountSpec::Squared { by, scale, offset, cap },
        (false, false) => CountSpec::Field { by, scale, offset, cap },
    })
}

fn parse_stride(attr: &syn::Attribute, name: &str) -> syn::Result<StrideSpec> {
    let mut by: Option<Ref> = None;
    let mut scale = 1u32;
    let mut offset = 0i32;
    let mut scale_span = attr.meta.span();

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("scale") {
            scale_span = meta.path.span();
            scale = meta.value()?.parse::<LitInt>()?.base10_parse()?;
            Ok(())
        } else if meta.path.is_ident("offset") {
            let v = meta.value()?;
            let neg = v.parse::<syn::Token![-]>().is_ok();
            let n: i32 = v.parse::<LitInt>()?.base10_parse()?;
            offset = if neg { -n } else { n };
            Ok(())
        } else if meta.path.is_ident("cap") {
            Err(meta.error(format!(
                "field `{name}`: #[stride] takes no `cap`. Put `cap` on `#[count]`"
            )))
        } else if meta.input.peek(syn::Token![=]) {
            Err(meta.error(format!("field `{name}`: expected `scale` or `offset`")))
        } else if by.is_some() {
            Err(meta.error(format!(
                "field `{name}`: #[stride] takes one field name. \
                 Other arguments are `scale = S` and `offset = O`"
            )))
        } else {
            let ident = meta.path.get_ident().ok_or_else(|| {
                meta.error(format!("field `{name}`: #[stride] takes a field name"))
            })?;
            let mut name = crate::attr::name(ident);
            dotted_tail(meta.input, &mut name)?;
            by = Some(Ref { name, span: ident.span() });
            Ok(())
        }
    })?;

    let by = by.ok_or_else(|| {
        syn::Error::new(
            attr.meta.span(),
            format!(
                "field `{name}`: #[stride] needs a field name. \
                 Write `#[stride(s)]`, where `s` is an earlier field holding the element size in bytes"
            ),
        )
    })?;
    if scale == 0 {
        return Err(syn::Error::new(
            scale_span,
            format!(
                "field `{name}`: `scale = 0` makes the stride a constant `{offset}`. \
                 Use `#[bytes(N)]` instead of `#[stride]`",
            ),
        ));
    }
    Ok(StrideSpec { by, scale, offset, span: attr.meta.span() })
}

fn parse_fill(attr: &syn::Attribute, name: &str) -> syn::Result<Option<usize>> {
    if let syn::Meta::Path(_) = &attr.meta {
        return Ok(None);
    }
    let mut cap = None;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("cap") {
            cap = Some(meta.value()?.parse::<LitInt>()?.base10_parse()?);
            Ok(())
        } else {
            Err(meta.error(unknown_key(name, &meta.path, "`cap`")))
        }
    })?;
    Ok(cap)
}

fn unknown_key(name: &str, path: &syn::Path, keys: &str) -> String {
    match path.get_ident() {
        Some(k) => format!("field `{name}`: unknown key `{k}`. Expected {keys}"),
        None => format!("field `{name}`: expected {keys}"),
    }
}

/// `#[with(a, b)]`: one earlier field per parameter of the nested message, in order.
fn parse_with(attr: &syn::Attribute, name: &str) -> syn::Result<Vec<Ref>> {
    let shape = || {
        format!(
            "field `{name}`: `#[with(..)]` takes one earlier field name per parameter, \
             such as `#[with(a, b)]`"
        )
    };
    let refs = attr
        .parse_args_with(
            syn::punctuated::Punctuated::<syn::Ident, syn::Token![,]>::parse_terminated,
        )
        .map_err(|_| syn::Error::new(attr.meta.span(), shape()))?;
    if refs.is_empty() {
        return Err(syn::Error::new(attr.meta.span(), shape()));
    }
    Ok(refs.into_iter().map(|id| Ref { name: crate::attr::name(&id), span: id.span() }).collect())
}

fn parse_switch(attr: &syn::Attribute, name: &str) -> syn::Result<SwitchOn> {
    if let Ok(by) = attr.parse_args_with(field_ref) {
        return Ok(SwitchOn::Field(by));
    }
    if attr.parse_args::<syn::Token![..]>().is_ok() {
        return Ok(SwitchOn::BodyLength(attr.meta.span()));
    }
    Err(syn::Error::new(
        attr.meta.span(),
        format!(
            "field `{name}`: #[switch] takes a field name or `..`. \
             Write `#[switch(n)]`, or `#[switch(..)]` to switch on the bytes remaining"
        ),
    ))
}

pub(super) fn parse_when(attr: &syn::Attribute, name: &str) -> syn::Result<WhenSpec> {
    let span = attr.meta.span();
    let expr: syn::Expr = attr.parse_args().map_err(|_| when_shape(span, name))?;

    match &expr {
        syn::Expr::Path(p) => {
            if let Some(i) = p.path.get_ident() {
                let by = Ref { name: crate::attr::name(i), span: i.span() };
                return Ok(WhenSpec { by, test: WhenTest::Flag, span });
            }
        }
        syn::Expr::Field(f) if matches!(&*f.base, syn::Expr::Field(_)) => {
            let base = &f.base;
            return Err(too_deep(f.member.span(), &crate::attr::spelled(base)));
        }
        syn::Expr::Field(f) => {
            if let Some(by) = field_expr_ref(f) {
                return Ok(WhenSpec { by, test: WhenTest::Flag, span });
            }
        }
        _ => {}
    }

    let syn::Expr::Binary(b) = &expr else {
        return Err(when_shape(span, name));
    };
    if matches!(b.op, syn::BinOp::Gt(_)) && is_zero(&b.right) {
        let Some(by) = binary_ref(b)? else {
            return Err(when_shape(span, name));
        };
        return Ok(WhenSpec { by, test: WhenTest::Positive, span });
    }
    if matches!(
        b.op,
        syn::BinOp::Eq(_)
            | syn::BinOp::Ne(_)
            | syn::BinOp::Lt(_)
            | syn::BinOp::Le(_)
            | syn::BinOp::Gt(_)
            | syn::BinOp::Ge(_)
    ) {
        let whole = crate::attr::spelled(&expr);
        let left = &b.left;
        let left = crate::attr::spelled(left);
        let why = if is_zero(&b.right) {
            format!("is not supported. Write `#[when({left} > 0)]`")
        } else {
            format!(
                "is not supported. \
                 Write `#[when({left} & 0x01)]` with the bits that mean present"
            )
        };
        return Err(syn::Error::new(span, format!("field `{name}`: `#[when({whole})]` {why}")));
    }
    if !matches!(b.op, syn::BinOp::BitAnd(_)) {
        return Err(when_shape(span, name));
    }

    let Some(by) = binary_ref(b)? else {
        return Err(when_shape(span, name));
    };
    let syn::Expr::Lit(lit) = &*b.right else {
        return Err(when_shape(span, name));
    };
    let syn::Lit::Int(int) = &lit.lit else {
        return Err(when_shape(span, name));
    };
    let mask: u64 = int.base10_parse()?;
    if mask == 0 {
        return Err(syn::Error::new(
            int.span(),
            format!(
                "field `{name}`: `#[when({} & 0)]` masks no bits. \
                 Name the bit that makes the field present",
                by.name,
            ),
        ));
    }
    Ok(WhenSpec { by, test: WhenTest::Mask(mask), span })
}

fn binary_ref(b: &syn::ExprBinary) -> syn::Result<Option<Ref>> {
    Ok(match &*b.left {
        syn::Expr::Path(p) => {
            p.path.get_ident().map(|i| Ref { name: crate::attr::name(i), span: i.span() })
        }
        syn::Expr::Field(f) if matches!(&*f.base, syn::Expr::Field(_)) => {
            let base = &f.base;
            return Err(too_deep(f.member.span(), &crate::attr::spelled(base)));
        }
        syn::Expr::Field(f) => field_expr_ref(f),
        _ => None,
    })
}

fn is_zero(e: &syn::Expr) -> bool {
    matches!(
        e,
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. })
            if i.base10_parse::<u128>().is_ok_and(|v| v == 0)
    )
}

fn when_shape(span: Span, name: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "field `{name}`: expected `#[when(field & MASK)]`, \
             or `#[when(field)]` where `field` is an earlier `bool`"
        ),
    )
}

fn parse_at(attr: &syn::Attribute, name: &str) -> syn::Result<crate::claim::At> {
    crate::claim::at_attr(attr, name, layline_codegen::StatedUnit::Byte)
}

fn parse_seek(attr: &syn::Attribute, name: &str) -> syn::Result<Ref> {
    if let Ok(by) = attr.parse_args_with(field_ref) {
        return Ok(by);
    }
    Err(syn::Error::new(
        attr.meta.span(),
        format!(
            "field `{name}`: `#[seek(..)]` takes a field name. \
             Write `#[seek(off)]`, where `off` is an earlier field holding the byte offset"
        ),
    ))
}
