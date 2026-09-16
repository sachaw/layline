//! Converts parsed fields to segments, plus compile-time checks on the types they name.

use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned};

use layline_codegen::{
    Count, Discriminant, Endian, Field, Kind, Len, MessageDef, Presence, Root, Segment,
};

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
    endian: Endian,
) -> syn::Result<Field> {
    let magic = claims
        .magic
        .as_ref()
        .map(|m| {
            crate::claim::magic_bytes(m, name, (kind.bits().unwrap_or(0) / 8) as usize, endian)
        })
        .transpose()?;
    let mut field = Field::new(name, kind.clone());
    field.range = claims.range;
    field.stated = stated;
    field.magic = magic;
    Ok(field)
}

pub(super) fn model_by(by: &Ref, scale: u32, offset: i32) -> layline_codegen::By {
    layline_codegen::By::new(&by.name, scale, offset)
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
                block.push(solved_field(
                    &f.name,
                    kind,
                    claims,
                    f.stated.map(|a| a.stated),
                    p.endian,
                )?);
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
                        CountSpec::Field { by, scale, offset, cap } => {
                            Count::Field { by: model_by(by, *scale, *offset), cap: *cap }
                        }
                        CountSpec::Squared { by, scale, offset, cap } => {
                            Count::Squared { by: model_by(by, *scale, *offset), cap: *cap }
                        }
                        CountSpec::Window { by, scale, offset, cap } => Count::Window(Len::Field {
                            by: model_by(by, *scale, *offset),
                            cap: *cap,
                        }),
                        CountSpec::Strided { by, scale, offset, cap, stride } => Count::Strided {
                            by: model_by(by, *scale, *offset),
                            cap: *cap,
                            stride: model_by(&stride.by, stride.scale, stride.offset),
                        },
                        CountSpec::Fill { cap } => Count::Fill { cap: *cap },
                        CountSpec::Terminated { mask } => Count::Terminated { mask: *mask },
                        CountSpec::Until { terminator } => {
                            Count::Window(Len::Until { terminator: *terminator })
                        }
                    },
                    at: at.as_ref().map(|r| r.name.clone()),
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
                    at: at.name.clone(),
                    absent: absent.as_ref().map(|g| match g {
                        PlacedAbsence::ZeroOffset => layline_codegen::Absence::ZeroOffset,
                        PlacedAbsence::ZeroLength(by) => {
                            layline_codegen::Absence::ZeroLength { by: by.name.clone() }
                        }
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
                        SwitchOn::Field(r) => Discriminant::Field(r.name.clone()),
                        SwitchOn::BodyLength(_) => Discriminant::BodyLength,
                    },
                    choice: ty.clone(),
                    window: match window {
                        SwitchWindow::Open => None,
                        SwitchWindow::Fixed(n) => Some(Len::Bytes(*n)),
                        SwitchWindow::Field { by, scale, offset, cap } => {
                            Some(Len::Field { by: model_by(by, *scale, *offset), cap: *cap })
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
                    && matches!(when.test, WhenTest::Flag)
                    && let (outer, Some(_)) = layline_codegen::__derive::split_ref(&when.by.name)
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
                        &when.by,
                        &child,
                        when.span,
                        &p.root,
                    ));
                }
                segments.push(Segment::Opt {
                    when: when.as_ref().map_or(Presence::Remaining, WhenSpec::presence),
                    field: solved_field(
                        &f.name,
                        kind,
                        claims,
                        f.stated.map(|a| a.stated),
                        p.endian,
                    )?,
                });
            }
        }
    }
    flush(&mut segments, &mut block);

    let msg = MessageDef::new(&crate::attr::name(&p.ident), segments)
        .with_endian(p.endian)
        .with_needs(p.needs.clone());
    Ok((msg, checks))
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
    let mut names: Vec<&str> = Vec::new();
    for f in &p.fields {
        match &f.body {
            Body::Repeat { count, at, .. } => {
                names.extend(count.by().map(|by| by.name.as_str()));
                names.extend(count.stride().map(|by| by.name.as_str()));
                if let Some(r) = at {
                    names.push(&r.name);
                }
            }
            Body::Placed { at, absent, .. } => {
                names.push(&at.name);
                if let Some(PlacedAbsence::ZeroLength(by)) = absent {
                    names.push(&by.name);
                }
            }
            Body::Value { len_ref: Some(r), .. } => names.push(&r.name),
            Body::Switch { on, window, .. } => {
                names.extend(on.field().map(|r| r.name.as_str()));
                names.extend(window.field().map(|r| r.name.as_str()));
            }
            Body::Opt { when: Some(when), .. } => names.push(&when.by.name),
            _ => {}
        }
    }
    names
}

fn wire_int_check(name: &str, ty: &str, span: Span, root: &Root) -> TokenStream {
    check::bound("carries_an_integer", name, &path(ty), quote!(WireInt), span, root)
}

fn var_codec_check(name: &str, ty: &str, span: Span, root: &Root) -> TokenStream {
    check::bound("is_self_delimiting", name, &path(ty), quote!(VarCodec), span, root)
}
