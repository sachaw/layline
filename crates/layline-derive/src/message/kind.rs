//! The `Kind` of a text, var, message, fixed or element field.

use proc_macro2::Span;
use syn::Type;
use syn::spanned::Spanned;

use layline_codegen::{Collection, Kind, Len, Scalar};

use super::lower::model_by;
use super::*;
use crate::claim::Ref;
use crate::ty::{array_of, box_element, is_plain_path, is_string, scalar_of, variable_size};

pub(super) struct TextAttrs<'a> {
    pub(super) codec: Option<String>,
    pub(super) tspan: Span,
    pub(super) count: Option<(CountSpec, Span)>,
    pub(super) until: Option<(u8, Span)>,
    pub(super) bytes: Option<(usize, Span)>,
    pub(super) elem: Option<&'a Type>,
}

pub(super) fn text_field(ty: &Type, name: &str, span: Span, a: TextAttrs<'_>) -> syn::Result<Body> {
    let TextAttrs { codec, tspan, count, until, bytes, elem } = a;

    if let Some(elem_ty) = elem {
        if !is_string(elem_ty) {
            return Err(syn::Error::new(
                elem_ty.span(),
                format!(
                    "field `{name}`: #[text] needs `String` elements, not `{}`. \
                     Declare a `Vec<String>`",
                    crate::attr::spelled(elem_ty),
                ),
            ));
        }
        let Some((count, _)) = count else {
            return Err(syn::Error::new(
                span,
                format!("field `{name}`: a `Vec<String>` needs a count. Add one of {REPEAT_ATTRS}"),
            ));
        };
        let len = match (until, bytes) {
            (Some((terminator, _)), None) => Len::Until { terminator },
            (None, Some((n, bspan))) => {
                if n == 0 {
                    return Err(syn::Error::new(
                        bspan,
                        format!(
                            "field `{name}`: an element cannot be zero bytes. \
                             Write #[bytes(N)] with N above zero"
                        ),
                    ));
                }
                Len::Bytes(n)
            }
            (Some(_), Some((_, bspan))) => {
                return Err(syn::Error::new(
                    bspan,
                    format!(
                        "field `{name}`: #[until] and #[bytes] both set the element size. Remove one"
                    ),
                ));
            }
            (None, None) => {
                return Err(syn::Error::new(
                    tspan,
                    format!(
                        "field `{name}`: each string needs a size. \
                         Add `#[until(0)]` or `#[bytes(N)]`"
                    ),
                ));
            }
        };
        return Ok(Body::Repeat {
            element: Kind::Text { len, codec },
            count,
            at: None,
            assert: None,
            collection: Collection::Vec,
        });
    }

    if !is_string(ty) {
        return Err(syn::Error::new(
            ty.span(),
            format!(
                "field `{name}`: #[text] needs a `String`, not `{}`. \
                 Declare a `String`, or use `#[fill]` on a `Vec<u8>`",
                crate::attr::spelled(ty),
            ),
        ));
    }

    let mut given: Vec<&'static str> = Vec::new();
    if let Some((c, _)) = &count {
        given.push(c.spelling());
    }
    if until.is_some() {
        given.push("#[until]");
    }
    if bytes.is_some() {
        given.push("#[bytes]");
    }
    if given.len() > 1 {
        return Err(syn::Error::new(
            tspan,
            format!(
                "field `{name}`: the length is set {} times ({}). Use one of {TEXT_LENS}",
                given.len(),
                given.join(", "),
            ),
        ));
    }

    let (len, len_ref) = match (count, until, bytes) {
        (Some((CountSpec::Window { by, scale, offset, cap }, _)), _, _) => {
            (Len::Field { by: model_by(&by, scale, offset), cap }, Some(by))
        }
        (Some((CountSpec::Field { by, .. }, cspan)), _, _) => {
            return Err(syn::Error::new(
                cspan,
                format!(
                    "field `{name}`: #[count] does not apply to a string. \
                     Write `#[len({})]` for its size in bytes",
                    by.name,
                ),
            ));
        }
        (Some((CountSpec::Terminated { .. } | CountSpec::Until { .. }, _)), _, _) => {
            unreachable!("#[until] on a text field is kept in `until`")
        }
        (Some((CountSpec::Fill { cap: Some(_) }, cspan)), _, _) => {
            return Err(syn::Error::new(
                cspan,
                format!(
                    "field `{name}`: a string has no elements to cap. \
                     Write `#[fill]`, or `#[len(n, cap = N)]` for a bounded string"
                ),
            ));
        }
        (Some((CountSpec::Fill { cap: None }, _)), _, _) => (Len::Fill, None),
        (Some((CountSpec::Squared { by, .. }, cspan)), _, _) => {
            return Err(syn::Error::new(
                cspan,
                format!(
                    "field `{name}`: `#[count({0} * {0})]` does not apply to a string. \
                     Write `#[len({0})]`",
                    by.name,
                ),
            ));
        }
        (Some((CountSpec::Strided { stride, .. }, _)), _, _) => {
            return Err(syn::Error::new(
                stride.span,
                format!(
                    "field `{name}`: #[stride] does not apply to a string. \
                     Write `#[until(0)]` or `#[bytes(N)]`"
                ),
            ));
        }
        (None, Some((terminator, _)), _) => (Len::Until { terminator }, None),
        (None, None, Some((n, _))) => (Len::Bytes(n), None),
        (None, None, None) => {
            return Err(syn::Error::new(
                tspan,
                format!("field `{name}`: #[text] needs a size. Add one of {TEXT_LENS}"),
            ));
        }
    };
    Ok(Body::Value { kind: Kind::Text { len, codec }, len_ref })
}

pub(super) struct VarAttrs<'a> {
    pub(super) count: Option<(CountSpec, Span)>,
    pub(super) bytes: Option<(usize, Span)>,
    pub(super) at: Option<Ref>,
    pub(super) elem: Option<&'a Type>,
}

pub(super) fn var_field(
    field_ty: &Type,
    name: &str,
    span: Span,
    a: VarAttrs<'_>,
) -> syn::Result<Body> {
    let VarAttrs { count, bytes, at, elem } = a;
    let ty = elem.unwrap_or(field_ty);
    if let Some((_, bspan)) = bytes {
        return Err(syn::Error::new(
            bspan,
            format!(
                "field `{name}`: #[bytes(N)] and #[var] conflict on `{}`. Remove one",
                crate::attr::spelled(ty),
            ),
        ));
    }
    if scalar_of(ty).is_some() {
        return Err(syn::Error::new(
            ty.span(),
            format!(
                "field `{name}`: `{}` has a fixed size, so #[var] does not apply. \
                 Use `layline::num::Uleb128` or `Sleb128`",
                crate::attr::spelled(ty),
            ),
        ));
    }
    if variable_size(ty) {
        return Err(syn::Error::new(
            ty.span(),
            format!(
                "field `{name}`: `{}` has no `VarCodec` impl. \
                 For text write `#[text] #[until(0)] {name}: String`",
                crate::attr::spelled(ty),
            ),
        ));
    }
    if !is_plain_path(ty) {
        return Err(syn::Error::new(
            ty.span(),
            format!(
                "field `{name}`: #[var] needs a `layline::VarCodec` type path, not `{}`. \
                 Name the codec type directly",
                crate::attr::spelled(ty),
            ),
        ));
    }

    let kind = Kind::Var { ty: crate::attr::spelled(ty) };
    match (elem, count) {
        (Some(_), Some((count, _))) => {
            Ok(Body::Repeat { element: kind, count, at, assert: None, collection: Collection::Vec })
        }
        (Some(_), None) => Err(syn::Error::new(
            span,
            format!("field `{name}`: a `Vec` needs a count. Add one of {REPEAT_ATTRS}"),
        )),
        (None, Some((count, cspan))) => Err(syn::Error::new(
            cspan,
            format!(
                "field `{name}`: {} needs a `Vec`. Declare a `Vec<{}>`",
                count.spelling(),
                crate::attr::spelled(ty),
            ),
        )),
        (None, None) => {
            if let Some(r) = at {
                return Err(syn::Error::new(
                    r.span,
                    format!(
                        "field `{name}`: #[seek] cannot place a #[var] value. \
                         Wrap it in a `#[bytes(N)]` nested layout"
                    ),
                ));
            }
            Ok(Body::Value { kind, len_ref: None })
        }
    }
}

pub(super) struct MsgAttrs<'a> {
    pub(super) count: Option<(CountSpec, Span)>,
    pub(super) bits: Option<(u32, Span)>,
    pub(super) bytes: Option<(usize, Span)>,
    pub(super) at: Option<Ref>,
    pub(super) elem: Option<&'a Type>,
    /// `#[with(f, g)]`: one earlier field per parameter of the nested message.
    pub(super) with: Vec<Ref>,
}

pub(super) fn message_field(
    field_ty: &Type,
    name: &str,
    span: Span,
    a: MsgAttrs<'_>,
) -> syn::Result<Body> {
    let MsgAttrs { count, bits, bytes, at, elem, with } = a;
    let ty = elem.unwrap_or(field_ty);
    let (ty, boxed) = match box_element(ty) {
        Some(inner) => (inner, true),
        None => (ty, false),
    };

    for (what, aspan) in
        [("#[codec(N)]", bits.map(|(_, s)| s)), ("#[bytes(N)]", bytes.map(|(_, s)| s))]
    {
        if let Some(aspan) = aspan {
            return Err(syn::Error::new(
                aspan,
                format!("field `{name}`: #[message] cannot be combined with {what}. Remove one"),
            ));
        }
    }
    if scalar_of(ty).is_some() || variable_size(ty) || !is_plain_path(ty) {
        return Err(syn::Error::new(
            ty.span(),
            format!(
                "field `{name}`: #[message] needs a `#[derive(Message)]` type, not `{}`. \
                 Use a type path, `Box<..>`, or `Vec<..>`",
                crate::attr::spelled(ty),
            ),
        ));
    }

    let with: Vec<String> = with.into_iter().map(|r| r.name).collect();
    let kind = Kind::Msg { ty: crate::attr::spelled(ty), boxed, with };
    match (elem, count) {
        (Some(_), Some((count, _))) => {
            Ok(Body::Repeat { element: kind, count, at, assert: None, collection: Collection::Vec })
        }
        (Some(_), None) => Err(syn::Error::new(
            span,
            format!(
                "field `{name}`: a `Vec` needs a count. \
                 Add `#[len(n)]`, or one of {REPEAT_ATTRS}"
            ),
        )),
        (None, Some((count, cspan))) => Err(syn::Error::new(
            cspan,
            format!(
                "field `{name}`: {} needs a `Vec`. Declare a `Vec<{}>`",
                count.spelling(),
                crate::attr::spelled(ty),
            ),
        )),
        (None, None) => match at {
            Some(at) => Ok(Body::Placed { kind, at, absent: None, assert: None }),
            None => Ok(Body::Value { kind, len_ref: None }),
        },
    }
}

pub(super) fn fixed_kind(
    name: &str,
    ty: &Type,
    bits: Option<(u32, Span)>,
    bytes: Option<(usize, Span)>,
    range: Option<&crate::claim::Range>,
) -> syn::Result<(Kind, Option<layline_codegen::Range>)> {
    let carries = fixed_carrier(name, ty, bits, bytes)?;
    let Some(r) = range else { return Ok((carries, None)) };

    if !matches!(carries, Kind::Scalar(Scalar::U(_) | Scalar::I(_))) {
        return Err(syn::Error::new(
            r.span,
            format!(
                "field `{name}`: #[range] needs an integer field, not `{}`. \
                 Move it to an integer field",
                crate::attr::spelled(ty),
            ),
        ));
    }

    let end = |v: Option<i128>, which: &str| -> syn::Result<Option<i64>> {
        match v {
            None => Ok(None),
            Some(v) => i64::try_from(v).map(Some).map_err(|_| {
                syn::Error::new(
                    r.span,
                    format!("field `{name}`: the {which} of #[range] does not fit `i64`"),
                )
            }),
        }
    };
    let bound = layline_codegen::Range::new(end(r.lo, "bottom")?, end(r.hi, "top")?);
    Ok((carries, Some(bound)))
}

fn fixed_carrier(
    name: &str,
    ty: &Type,
    bits: Option<(u32, Span)>,
    bytes: Option<(usize, Span)>,
) -> syn::Result<Kind> {
    if let (Some((_, bspan)), Some(_)) = (&bytes, &bits) {
        return Err(syn::Error::new(
            *bspan,
            format!("field `{name}`: #[codec] and #[bytes] are mutually exclusive. Remove one"),
        ));
    }
    if let Some((bytes, bspan)) = bytes {
        if bytes == 0 {
            return Err(syn::Error::new(
                bspan,
                format!(
                    "field `{name}`: a nested layout cannot be zero bytes. \
                     Write #[bytes(N)] with N above zero"
                ),
            ));
        }
        if let Some((elem, len)) = array_of(ty) {
            return Ok(Kind::NestedArray { ty: crate::attr::spelled(elem), bytes, len });
        }
        return Ok(Kind::Nested { ty: crate::attr::spelled(ty), bytes });
    }
    let bits = crate::width::declared_width(name, "codec", ty, bits)?;
    if let Some((bits, bspan)) = bits {
        if scalar_of(ty).is_some() {
            return Err(syn::Error::new(
                bspan,
                format!("field `{name}`: a scalar's type sets its size. Remove #[codec(N)]"),
            ));
        }
        if !matches!(bits, 8 | 16 | 32 | 64) {
            return Err(syn::Error::new(
                bspan,
                format!(
                    "field `{name}`: {} is not 8, 16, 32, or 64. \
                     Write one of those, or nest a `#[layout(bits = N)]` type with `#[bytes(N)]`",
                    crate::width::width_said("codec", bits, ty),
                ),
            ));
        }
        return Ok(Kind::Codec { ty: crate::attr::spelled(ty), bits: u64::from(bits) });
    }
    if let Some(s) = scalar_of(ty) {
        return Ok(Kind::Scalar(s));
    }
    if let Some((elem, dims)) = crate::ty::array_dims(ty)
        && let Some(s) = scalar_of(elem)
    {
        return Ok(match dims.as_slice() {
            [len] => Kind::array(s, *len),
            _ => Kind::Array(s, dims),
        });
    }
    if is_string(ty) {
        return Err(syn::Error::new(
            ty.span(),
            format!(
                "field `{name}`: a `String` needs `#[text]`. Add `#[text]` and one of {TEXT_LENS}"
            ),
        ));
    }
    Err(syn::Error::new(
        ty.span(),
        format!(
            "field `{name}`: `{}` has no known size. \
             Add `#[bytes(N)]` for a nested layout, `#[var]` for a `VarCodec` type, or `#[text]` for a string",
            crate::attr::spelled(ty),
        ),
    ))
}

pub(super) fn element_kind(
    name: &str,
    elem: &Type,
    bytes: Option<(usize, Span)>,
    bits: Option<(u32, Span)>,
) -> syn::Result<(Kind, Option<Box<ElementSize>>)> {
    if let Some((_, bspan)) = bits {
        return Err(syn::Error::new(
            bspan,
            format!(
                "field `{name}`: #[codec] does not apply to a collection. \
                 Nest a `#[layout(bits = N)]` type and repeat it with `#[bytes(N)]`"
            ),
        ));
    }

    if let Some(s) = scalar_of(elem) {
        if let Some((_, bspan)) = bytes {
            return Err(syn::Error::new(
                bspan,
                format!(
                    "field `{name}`: `{}` already has a fixed size. Remove #[bytes]",
                    crate::attr::spelled(elem),
                ),
            ));
        }
        return Ok((Kind::Scalar(s), None));
    }

    if is_string(elem) {
        return Err(syn::Error::new(
            elem.span(),
            format!(
                "field `{name}`: `String` elements need `#[text]` and a size. \
                 Add `#[text]` with `#[until(0)]` or `#[bytes(N)]`"
            ),
        ));
    }
    if variable_size(elem) {
        return Err(syn::Error::new(
            elem.span(),
            format!(
                "field `{name}`: `{}` elements have no fixed size. \
                 Add `#[bytes(N)]`, `#[var]`, or `#[text]`",
                crate::attr::spelled(elem),
            ),
        ));
    }

    if is_plain_path(elem) {
        let Some((bytes, bspan)) = bytes else {
            return Err(syn::Error::new(
                elem.span(),
                format!(
                    "field `{name}`: `{}` elements need a size. \
                     Add `#[bytes(N)]` for a `#[derive(Layout)]` type, or `#[var]` for a `VarCodec` type",
                    crate::attr::spelled(elem),
                ),
            ));
        };
        if bytes == 0 {
            return Err(syn::Error::new(
                bspan,
                format!(
                    "field `{name}`: an element cannot be zero bytes. \
                     Write #[bytes(N)] with N above zero"
                ),
            ));
        }
        return Ok((
            Kind::Nested { ty: crate::attr::spelled(elem), bytes },
            Some(Box::new(ElementSize { ty: elem.clone(), bytes, span: bspan })),
        ));
    }

    Err(syn::Error::new(
        elem.span(),
        format!(
            "field `{name}`: `{}` cannot be a collection element. \
             Use a `#[derive(Layout)]` type with `#[bytes(N)]`, or a scalar",
            crate::attr::spelled(elem),
        ),
    ))
}

#[cfg(test)]
mod tests {
    use layline_codegen::__derive::{WidthAttr, kind_ty, width_attr};
    use layline_codegen::{Kind, Scalar};

    /// Omits `Kind::bytes`, which renders the same `[u8; n]` as `Array(U(8), n)`.
    fn block_kind_fixtures() -> Vec<(Kind, Kind)> {
        let same = |k: Kind| (k.clone(), k);
        vec![
            same(Kind::Scalar(Scalar::U(16))),
            same(Kind::Scalar(Scalar::I(64))),
            same(Kind::array(Scalar::F32, 3)),
            same(Kind::Array(Scalar::F64, vec![3, 3])),
            same(Kind::Codec { ty: String::from("Level"), bits: 8 }),
            same(Kind::Nested { ty: String::from("Head"), bytes: 4 }),
            same(Kind::NestedArray { ty: String::from("Pair"), bytes: 2, len: 3 }),
        ]
    }

    #[test]
    fn a_kind_round_trips_through_the_type_it_renders() {
        for (kind, expect) in block_kind_fixtures() {
            let rendered = kind_ty(&kind, &layline_codegen::Root::default());
            let ty: syn::Type = syn::parse2(rendered.clone())
                .unwrap_or_else(|e| panic!("`{rendered}` is not a type: {e}"));

            let (bits, bytes) = match width_attr(&kind, false) {
                Some(WidthAttr::Codec(n)) => {
                    (Some((n as u32, proc_macro2::Span::call_site())), None)
                }
                Some(WidthAttr::Bytes(n)) => {
                    (None, Some((n as usize, proc_macro2::Span::call_site())))
                }
                None => (None, None),
                Some(WidthAttr::Bits(_)) => {
                    unreachable!("a byte-addressed field is never `bits` wide")
                }
            };
            let range: Option<crate::claim::Range> = None;

            let (read_back, _) = super::fixed_kind("f", &ty, bits, bytes, range.as_ref())
                .unwrap_or_else(|e| panic!("`{rendered}` did not read back: {e}"));

            assert_eq!(
                read_back, expect,
                "`{rendered}`, rendered from {kind:?}, read back as a different kind"
            );
        }
    }
}
