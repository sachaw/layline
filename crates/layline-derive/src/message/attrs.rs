//! Field attribute parsing.

use proc_macro2::Span;
use syn::LitInt;
use syn::spanned::Spanned;

use layline_codegen::Scalar;

use super::container::MESSAGE_ATTRS;
use super::*;
use crate::attr::bare;
use crate::claim::{Ref, parse_checksum};
use crate::value::{Args, Value, args};

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
        align: None,
        valid: None,
        present: None,
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
            crate::attr::once_attr(&mut a.present, attr, &owner, || {
                bare(attr, name, "#[present]").map(|()| aspan)
            })?;
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
        } else if path.is_ident("valid") {
            crate::attr::once_attr(&mut a.valid, attr, &owner, || {
                let Args { expr: Some(tokens), keys } = args(attr)? else {
                    return Err(syn::Error::new(
                        aspan,
                        format!(
                            "field `{name}`: #[valid] takes a condition, such as `#[valid(kind == 1)]`"
                        ),
                    ));
                };
                if let Some((key, _)) = keys.first() {
                    return Err(syn::Error::new(
                        key.span(),
                        format!("field `{name}`: #[valid] takes no keys"),
                    ));
                }
                Value::parse(&tokens, name, "#[valid]")
            })?;
        } else if path.is_ident("align") {
            crate::attr::once_attr(&mut a.align, attr, &owner, || parse_align(attr, name))?;
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
                "field `{name}`: `#[at({} = {})]` conflicts with `#[seek({seek})]`, which reads the position from `{seek}`. \
                 Remove `#[at]`",
                at.stated.unit.name(),
                at.stated.pos,
            ),
        ));
    }

    Ok(a)
}

fn parse_align(attr: &syn::Attribute, name: &str) -> syn::Result<(u64, Span)> {
    let (n, span) = crate::attr::int_arg::<u64>(attr)?;
    if n < 2 {
        return Err(syn::Error::new(
            span,
            format!(
                "field `{name}`: #[align({n})] pads nothing. \
                 Write the multiple the padding reaches, such as #[align(4)]"
            ),
        ));
    }
    Ok((n, span))
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

/// The expression of a value-taking attribute, refusing one that reads no field or parameter:
/// a fixed number is the type's to state.
fn expression(
    attr: &syn::Attribute,
    expr: Option<proc_macro2::TokenStream>,
    name: &str,
    what: &str,
    fixed: &str,
) -> syn::Result<Value> {
    let open = what.trim_end_matches(']');
    let Some(tokens) = expr else {
        return Err(syn::Error::new(
            attr.meta.span(),
            format!("field `{name}`: {what} needs an expression. Write `{open}(n)`, {fixed}"),
        ));
    };
    let value = Value::parse(&tokens, name, what)?;
    if value.refs.is_empty() {
        return Err(syn::Error::new(
            value.span,
            format!(
                "field `{name}`: {what} reads no field or parameter, so `{value}` is a fixed \
                 number. Write `{open}(n)`, {fixed}"
            ),
        ));
    }
    Ok(value)
}

/// Refuses the `scale` and `offset` keys, which the expression now holds.
fn moved_key(key: &syn::Ident, name: &str, what: &str, by: Option<&Value>) -> syn::Error {
    let open = what.trim_end_matches(']');
    let by = by.map_or_else(|| String::from("n"), ToString::to_string);
    let example = if key == "scale" { format!("{by} * 2") } else { format!("{by} - 4") };
    syn::Error::new(
        key.span(),
        format!(
            "field `{name}`: `{key}` is part of the expression now. Write `{open}({example})]`"
        ),
    )
}

fn parse_count(attr: &syn::Attribute, name: &str, bytes: bool) -> syn::Result<CountSpec> {
    let what = if bytes { "#[len]" } else { "#[count]" };
    let unit = if bytes { "byte length" } else { "count" };
    let Args { expr, keys } = args(attr)?;
    let prefixed = keys.iter().any(|(key, _)| key == "prefix");
    let by = match (expr, prefixed) {
        (None, true) => None,
        (expr, _) => Some(expression(
            attr,
            expr,
            name,
            what,
            &format!(
                "where `n` is an earlier field holding the {unit}, or `{}(prefix = u8)` for one \
                 just before the value",
                what.trim_end_matches(']')
            ),
        )?),
    };
    let mut cap: Option<usize> = None;
    let mut prefix: Option<Scalar> = None;
    for (key, value) in keys {
        if key == "cap" {
            let lit: LitInt = syn::parse2(value)?;
            cap = Some(lit.base10_parse()?);
        } else if key == "prefix" {
            if let Some(by) = &by {
                return Err(syn::Error::new(
                    key.span(),
                    format!(
                        "field `{name}`: {what} takes an expression or a `prefix`, not both. \
                         A prefix is the {unit} itself, just before the value. A number that is \
                         not, such as a count of words, is a `Stated<T>` field the expression \
                         reads: `{}({by})]`",
                        what.trim_end_matches(']'),
                    ),
                ));
            }
            prefix = Some(prefix_type(&value, name, what)?);
        } else if key == "scale" || key == "offset" {
            return Err(moved_key(&key, name, what, by.as_ref()));
        } else {
            return Err(syn::Error::new(
                key.span(),
                format!("field `{name}`: expected `cap` or `prefix` after the expression"),
            ));
        }
    }
    Ok(match (by, prefix) {
        (_, Some(repr)) => CountSpec::Prefix { repr, cap, bytes },
        (Some(by), None) if bytes => CountSpec::Window { by, cap },
        (Some(by), None) => CountSpec::Field { by, cap },
        (None, None) => unreachable!("an expression is parsed unless a prefix is given"),
    })
}

/// The integer of `prefix = T`: a whole-byte integer type.
fn prefix_type(value: &proc_macro2::TokenStream, name: &str, what: &str) -> syn::Result<Scalar> {
    let refused = || {
        syn::Error::new(
            value.span(),
            format!(
                "field `{name}`: {what}'s `prefix` is the integer type just before the value: \
                 `u8`, `u16`, `u32`, `u64`, or a signed one"
            ),
        )
    };
    let ty: syn::Type = syn::parse2(value.clone()).map_err(|_| refused())?;
    match crate::ty::scalar_of(&ty) {
        Some(s @ (Scalar::U(8 | 16 | 32 | 64) | Scalar::I(8 | 16 | 32 | 64))) => Ok(s),
        _ => Err(refused()),
    }
}

fn parse_stride(attr: &syn::Attribute, name: &str) -> syn::Result<StrideSpec> {
    let Args { expr, keys } = args(attr)?;
    let by = expression(
        attr,
        expr,
        name,
        "#[stride]",
        "where `s` is an earlier field holding the element size in bytes",
    )?;
    if let Some((key, _)) = keys.first() {
        return Err(if key == "cap" {
            syn::Error::new(
                key.span(),
                format!("field `{name}`: #[stride] takes no `cap`. Put `cap` on `#[count]`"),
            )
        } else if key == "scale" || key == "offset" {
            moved_key(key, name, "#[stride]", Some(&by))
        } else {
            syn::Error::new(key.span(), format!("field `{name}`: #[stride] takes no keys"))
        });
    }
    Ok(StrideSpec { by, span: attr.meta.span() })
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
             such as `#[with(a, b)]` or `#[with(hdr.n)]`"
        )
    };
    let refs = attr
        .parse_args_with(|input: syn::parse::ParseStream<'_>| {
            syn::punctuated::Punctuated::<Ref, syn::Token![,]>::parse_terminated_with(
                input, field_ref,
            )
        })
        .map_err(|_| syn::Error::new(attr.meta.span(), shape()))?;
    if refs.is_empty() {
        return Err(syn::Error::new(attr.meta.span(), shape()));
    }
    Ok(refs.into_iter().collect())
}

fn parse_switch(attr: &syn::Attribute, name: &str) -> syn::Result<SwitchOn> {
    if attr.parse_args::<syn::Token![..]>().is_ok() {
        return Ok(SwitchOn::BodyLength(attr.meta.span()));
    }
    let shape = || {
        syn::Error::new(
            attr.meta.span(),
            format!(
                "field `{name}`: #[switch] takes a field name or `..`. \
                 Write `#[switch(n)]`, or `#[switch(..)]` to switch on the bytes remaining"
            ),
        )
    };
    let Args { expr, keys } = args(attr)?;
    if !keys.is_empty() {
        return Err(shape());
    }
    let value = Value::parse(&expr.ok_or_else(shape)?, name, "#[switch]")?;
    if value.refs.is_empty() {
        return Err(shape());
    }
    Ok(SwitchOn::Field(value))
}

pub(super) fn parse_when(attr: &syn::Attribute, name: &str) -> syn::Result<WhenSpec> {
    use layline_codegen::{Expr, Op};
    let span = attr.meta.span();
    let Args { expr: Some(tokens), keys } = args(attr)? else {
        return Err(when_shape(span, name));
    };
    if !keys.is_empty() {
        return Err(when_shape(span, name));
    }
    let value = Value::parse(&tokens, name, "#[when]")?;
    // `!` on bits reads as Rust's bitwise not.
    let mut negated = &value.expr;
    while let Expr::Not(inner) = negated {
        if let Expr::Binary(op, ..) = &**inner
            && !op.compares()
            && !op.joins()
        {
            let hint = if *op == Op::BitAnd {
                format!("Write `#[when({inner} == 0)]`")
            } else {
                String::from("Write `#[when(!flag)]` with an earlier `bool`")
            };
            return Err(syn::Error::new(
                span,
                format!(
                    "field `{name}`: `!` on an integer flips its bits, so it does not test them. \
                     {hint}"
                ),
            ));
        }
        negated = inner;
    }
    if let Some(t) = value.expr.test()
        && let Some(0) = t.mask.and_then(Expr::fold)
    {
        return Err(syn::Error::new(
            span,
            format!(
                "field `{name}`: `#[when({} & 0)]` masks no bits. \
                 Name the bit that makes the field present",
                t.field,
            ),
        ));
    }
    Ok(WhenSpec { value, span })
}

fn when_shape(span: Span, name: &str) -> syn::Error {
    syn::Error::new(
        span,
        format!(
            "field `{name}`: expected a condition, such as `#[when(flags & MASK)]`, \
             `#[when(flag)]` for an earlier `bool`, or `#[when(kind == 3 && len > 0)]`"
        ),
    )
}

fn parse_at(attr: &syn::Attribute, name: &str) -> syn::Result<crate::claim::At> {
    crate::claim::at_attr(attr, name, layline_codegen::StatedUnit::Byte)
}

fn parse_seek(attr: &syn::Attribute, name: &str) -> syn::Result<Value> {
    let shape = || {
        syn::Error::new(
            attr.meta.span(),
            format!(
                "field `{name}`: `#[seek(..)]` takes a field name, or an expression over one. \
                 Write `#[seek(off)]`, where `off` is an earlier field holding the byte offset"
            ),
        )
    };
    let Args { expr, keys } = args(attr)?;
    if !keys.is_empty() {
        return Err(shape());
    }
    let value = Value::parse(&expr.ok_or_else(shape)?, name, "#[seek]")?;
    if value.refs.is_empty() {
        return Err(shape());
    }
    Ok(value)
}
