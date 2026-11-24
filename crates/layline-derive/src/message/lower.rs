//! Converts parsed fields to segments, plus compile-time checks on the types they name.

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};

use layline_codegen::{Count, Discriminant, Field, Kind, Len, MessageDef, Presence, Root, Segment};

use super::bits::lower_bits;
use super::*;
use crate::check;
use crate::claim::Ref;

/// Pushes pending fixed-size fields as one block.
pub(super) fn flush(segments: &mut Vec<Segment>, block: &mut Vec<Field>) {
    if !block.is_empty() {
        segments.push(Segment::Block(std::mem::take(block)));
    }
}

/// Parses a model type string into a path.
pub(super) fn path(ty: &str) -> TokenStream {
    let path: syn::Path = syn::parse_str(ty).expect("the model holds type paths");
    quote!(#path)
}

fn solved_field(
    name: &str,
    kind: &Kind,
    claims: &Claims,
    stated: Option<layline_codegen::Stated>,
    p: &Parsed,
) -> syn::Result<Field> {
    let width = (kind.bits().unwrap_or(0) / 8) as usize;
    if let (Some(from), Some(m)) = (&p.endian_from, &claims.magic)
        && matches!(m.lit, syn::Lit::Int(_))
        && width > 1
    {
        return Err(syn::Error::new(
            m.span,
            format!(
                "field `{name}`: an integer #[magic] has no byte order until `{from}` picks one. \
                 Write its bytes in wire order: `#[magic(b\"..\")]`"
            ),
        ));
    }
    let magic = claims
        .magic
        .as_ref()
        .map(|m| crate::claim::magic_bytes(m, name, width, p.endian))
        .transpose()?;
    let mut field = Field::new(name, kind.clone());
    field.range = claims.range;
    field.stated = stated;
    field.magic = magic;
    Ok(field)
}

pub(super) fn lower(p: &Parsed) -> syn::Result<(MessageDef, Vec<TokenStream>)> {
    if let Some(order) = p.bits {
        return Ok(lower_bits(p, order));
    }
    let mut segments: Vec<Segment> = Vec::new();
    let mut block: Vec<Field> = Vec::new();
    let mut checks: Vec<TokenStream> = Vec::new();
    let named = referenced(p);

    if let Some(f) = p
        .fields
        .iter()
        .rev()
        .skip(1)
        .find(|f| matches!(&f.body, Body::Switch { on: SwitchOn::BodyLength(_), .. }))
    {
        let Body::Switch { on: SwitchOn::BodyLength(span), .. } = &f.body else {
            unreachable!("just matched")
        };
        let last = p.fields.last().expect("non-empty: this one is not last");
        return Err(syn::Error::new(
            *span,
            format!(
                "field `{}`: #[switch(..)] must be the last field, and `{}` comes after it. \
                 Move it last, or write #[switch(n)]",
                f.name, last.name,
            ),
        ));
    }

    for f in &p.fields {
        if let Body::Fixed { kind, .. } | Body::Opt { kind, .. } = &f.body
            && let Some(ty) = nestable_ty(kind)
        {
            checks.push(nestable_check(&f.name, ty, f.span, &p.root));
        }
        match &f.body {
            Body::Bits { .. } => {
                unreachable!("a bit-addressed message is lowered by `lower_bits`")
            }
            Body::Fixed { kind, claims } => {
                block.push(solved_field(&f.name, kind, claims, f.stated.map(|a| a.stated), p)?);
            }
            Body::Value { kind, .. } => {
                flush(&mut segments, &mut block);
                if let Kind::Var { ty } = kind {
                    checks.push(var_codec_check(&f.name, ty, f.span, &p.root));
                    if named.iter().any(|n| *n == f.name) {
                        checks.push(wire_int_check(&f.name, ty, f.span, &p.root));
                    }
                }
                segments.push(Segment::Value {
                    stated: f.stated.map(|a| a.stated),
                    name: f.name.clone(),
                    kind: kind.clone(),
                    doc: None,
                });
            }
            Body::Repeat { element, count, at, assert, collection } => {
                flush(&mut segments, &mut block);
                if let Some(a) = assert {
                    checks.push(element_size_check(&f.name, a, &p.root));
                }
                if let Kind::Var { ty } = element {
                    checks.push(var_codec_check(&f.name, ty, f.span, &p.root));
                }
                segments.push(Segment::Repeat {
                    stated: f.stated.map(|a| a.stated),
                    name: f.name.clone(),
                    element: element.clone(),
                    collection: *collection,
                    count: match count {
                        CountSpec::Field { by, cap } => {
                            Count::Field { by: by.expr.clone(), cap: *cap }
                        }
                        CountSpec::Window { by, cap } => Count::Window(Len::Field {
                            by: by.expr.clone(),
                            cap: *cap,
                            align: None,
                        }),
                        CountSpec::Strided { by, cap, stride } => Count::Strided {
                            by: by.expr.clone(),
                            cap: *cap,
                            stride: stride.by.expr.clone(),
                        },
                        CountSpec::Fill { cap } => Count::Fill { cap: *cap },
                        CountSpec::Terminated { mask } => Count::Terminated { mask: *mask },
                        CountSpec::Until { terminator } => {
                            Count::Window(Len::Until { terminator: *terminator })
                        }
                        CountSpec::Bytes(n) => Count::Window(Len::Bytes(*n)),
                    },
                    at: at.as_ref().map(|r| r.expr.clone()),
                    doc: None,
                });
            }
            Body::Placed { kind, at, absent, assert } => {
                flush(&mut segments, &mut block);
                if let Some(a) = assert {
                    checks.push(element_size_check(&f.name, a, &p.root));
                }
                segments.push(Segment::Placed {
                    stated: f.stated.map(|a| a.stated),
                    name: f.name.clone(),
                    kind: kind.clone(),
                    at: at.expr.clone(),
                    absent: absent.as_ref().map(|g| match g {
                        PlacedAbsence::ZeroOffset => layline_codegen::Absence::ZeroOffset,
                        PlacedAbsence::ZeroLength(by) => layline_codegen::Absence::ZeroLength {
                            by: by.name.clone(),
                            align: None,
                        },
                    }),
                    doc: None,
                });
            }
            Body::Checksum { repr, algorithm, over, .. } => {
                flush(&mut segments, &mut block);
                segments.push(Segment::Checksum {
                    stated: f.stated.map(|a| a.stated),
                    name: f.name.clone(),
                    repr: *repr,
                    algorithm: algorithm.clone(),
                    over: over.clone(),
                    doc: None,
                });
            }
            Body::Switch { on, ty, window } => {
                flush(&mut segments, &mut block);
                segments.push(Segment::Switch {
                    stated: f.stated.map(|a| a.stated),
                    name: f.name.clone(),
                    on: match on {
                        SwitchOn::Field(on) => Discriminant::Field(on.expr.clone()),
                        SwitchOn::BodyLength(_) => Discriminant::BodyLength,
                    },
                    choice: ty.clone(),
                    window: match window {
                        SwitchWindow::Open => None,
                        SwitchWindow::Fixed(n) => Some(Len::Bytes(*n)),
                        SwitchWindow::Field { by, cap } => {
                            Some(Len::Field { by: by.expr.clone(), cap: *cap, align: None })
                        }
                    },
                    doc: None,
                });
            }
            Body::Opt { when, kind, claims } => {
                flush(&mut segments, &mut block);
                if let Kind::Var { ty } = kind {
                    checks.push(var_codec_check(&f.name, ty, f.span, &p.root));
                }
                if let Some(when) = when
                    && let Some((by, t)) = when.tested()
                    && t.mask.is_none()
                    && let (outer, Some(_)) = layline_codegen::__derive::split_ref(&by.name)
                    && let Some(child) = p.fields.iter().find(|g| g.name == outer).and_then(|g| {
                        match bound(&g.body) {
                            Some(Kind::Nested { ty, .. }) => Some(ty.clone()),
                            _ => None,
                        }
                    })
                {
                    checks.push(flag_carrier_check(
                        &crate::attr::name(&p.ident),
                        &f.name,
                        by,
                        &child,
                        when.span,
                        &p.root,
                    ));
                }
                segments.push(Segment::Opt {
                    when: when.as_ref().map_or(Presence::Remaining, WhenSpec::presence),
                    field: solved_field(&f.name, kind, claims, f.stated.map(|a| a.stated), p)?,
                });
            }
        }
        if let Some((align, span)) = f.align {
            let padded = !matches!(f.body, Body::Fixed { .. } | Body::Opt { .. })
                && segments.last_mut().is_some_and(|seg| pad(seg, align));
            if !padded {
                return Err(syn::Error::new(
                    span,
                    format!(
                        "field `{}`: #[align({align})] pads a field whose byte length an earlier \
                         field states. Add `#[len(n)]`, or on a `#[seek]` record, \
                         `#[when(n > 0)]`",
                        f.name
                    ),
                ));
            }
        }
    }
    flush(&mut segments, &mut block);
    // A condition takes no bytes, so it goes after every field it may read.
    segments.extend(
        p.fields
            .iter()
            .filter_map(|f| Some(Segment::valid(&f.name, f.valid.as_ref()?.expr.clone()))),
    );

    let mut msg = MessageDef::new(&crate::attr::name(&p.ident), segments)
        .with_endian(p.endian)
        .with_needs(p.needs.clone());
    if let Some(from) = &p.endian_from {
        msg = msg.with_endian_from(&crate::attr::name(from));
    }
    if let Some(l) = &p.lifetime {
        msg = msg.with_lifetime(&l.ident.to_string());
    }
    Ok((msg, checks))
}

/// Sets `#[align(N)]` on the length that sizes `seg`, if a field states one.
fn pad(seg: &mut Segment, n: u64) -> bool {
    use layline_codegen::Absence;
    match seg {
        Segment::Repeat { count: Count::Window(Len::Field { align, .. }), .. }
        | Segment::Value { kind: Kind::Text { len: Len::Field { align, .. }, .. }, .. }
        | Segment::Switch { window: Some(Len::Field { align, .. }), .. }
        | Segment::Placed { absent: Some(Absence::ZeroLength { align, .. }), .. } => {
            *align = Some(n);
            true
        }
        _ => false,
    }
}

fn nestable_ty(kind: &Kind) -> Option<&str> {
    match kind {
        Kind::Nested { ty, .. } | Kind::NestedArray { ty, .. } | Kind::Codec { ty, .. } => {
            Some(ty.as_str())
        }
        _ => None,
    }
}

fn nestable_check(name: &str, ty: &str, span: Span, root: &Root) -> TokenStream {
    check::bound("is_nestable", name, &path(ty), quote!(__private::Nestable), span, root)
}

fn flag_carrier_check(
    owner: &str,
    field: &str,
    by: &Ref,
    child: &str,
    span: Span,
    root: &Root,
) -> TokenStream {
    let spanned = layline_codegen::__derive::respan(root, span);
    let path: syn::Path = syn::parse_str(child).expect("type path");
    let (_, inner) = layline_codegen::__derive::split_ref(&by.name);
    let inner = inner.unwrap_or(&by.name);
    let msg = crate::check::assert_msg(format!(
        "`{owner}`: field `{field}` uses `#[when({})]`, but `{inner}` of `{child}` is not a `bool`. \
         Write `#[when({} & 0x01)]`, or declare `{inner}` as `bool`",
        by.name, by.name,
    ));
    quote_spanned! {span=>
        const _: () = assert!(
            #spanned::table::field_width(<#path as #spanned::Layout>::FIELDS, #inner) == 0
                || #spanned::table::type_is(<#path as #spanned::Layout>::TYPES, #inner, "bool"),
            #msg
        );
    }
}

fn element_size_check(name: &str, e: &ElementSize, root: &Root) -> TokenStream {
    let ElementSize { ty, bytes, span } = e;
    let root = layline_codegen::__derive::respan(root, *span);
    let msg = crate::check::assert_msg(format!(
        "field `{name}`: #[bytes({bytes})] does not match `<{} as Layout>::WIRE_BYTES`. \
         Write the element's size",
        crate::attr::spelled(ty),
    ));
    quote_spanned! {*span=>
        const _: () = assert!(<#ty as #root::Layout>::WIRE_BYTES == #bytes, #msg);
    }
}

fn referenced(p: &Parsed) -> Vec<&str> {
    let mut values: Vec<&Value> = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    for f in &p.fields {
        match &f.body {
            Body::Repeat { count, at, .. } => {
                values.extend(count.by());
                values.extend(count.stride());
                values.extend(at.as_ref());
            }
            Body::Placed { at, absent, .. } => {
                values.push(at);
                if let Some(PlacedAbsence::ZeroLength(by)) = absent {
                    names.push(&by.name);
                }
            }
            Body::Value { len_ref, .. } => values.extend(len_ref.as_ref()),
            Body::Switch { on, window, .. } => {
                values.extend(on.field());
                values.extend(window.field());
            }
            Body::Opt { when: Some(when), .. } => values.push(&when.value),
            _ => {}
        }
    }
    names.extend(values.into_iter().flat_map(|v| v.refs.iter().map(|r| r.name.as_str())));
    names
}

fn wire_int_check(name: &str, ty: &str, span: Span, root: &Root) -> TokenStream {
    check::bound("carries_an_integer", name, &path(ty), quote!(WireInt), span, root)
}

fn var_codec_check(name: &str, ty: &str, span: Span, root: &Root) -> TokenStream {
    check::bound("is_self_delimiting", name, &path(ty), quote!(VarCodec), span, root)
}
