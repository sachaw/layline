//! Parses a field of a byte-addressed message into a [`Body`].

use proc_macro2::Span;
use syn::Type;
use syn::spanned::Spanned;

use layline_codegen::{Collection, Kind, Scalar};

use super::attrs::{parse_attrs, reject_second};
use super::kind::{
    MsgAttrs, TextAttrs, VarAttrs, element_kind, fixed_kind, message_field, text_field, var_field,
};
use super::*;
use crate::claim::Ref;
use crate::ty::{is_plain_path, option_element, run_element, scalar_of, vec_element};

pub(super) fn msg_field(f: &syn::Field) -> syn::Result<MsgField> {
    let name = crate::attr::name(f.ident.as_ref().expect("named field"));
    let span = f.ident.as_ref().map_or_else(|| f.span(), syn::Ident::span);
    let mut a = parse_attrs(f, &name)?;
    let when = a.when.take();
    let with: Vec<Ref> = a.with.iter().flat_map(|(refs, _)| refs).map(Ref::same).collect();

    let stated = a.at;
    let walk_span = walked_by(&a)
        .map(|(_, s)| s)
        .or_else(|| when.as_ref().map(|w| w.span))
        .or_else(|| a.seek.as_ref().map(|r| r.span))
        .unwrap_or(span);

    let body = match (when, option_element(&f.ty)) {
        (Some(when), Some(inner)) => optional(&name, when, field_body(inner, &name, span, a)?)?,
        (Some(when), None) => {
            return Err(syn::Error::new(
                when.span,
                format!(
                    "field `{name}`: #[when(..)] needs an `Option` field. Declare it `Option<{}>`",
                    crate::attr::spelled(&f.ty),
                ),
            ));
        }
        (None, Some(inner)) if a.seek.is_some() => {
            let Some(r) = &a.seek else { unreachable!("just matched") };
            let named = (r.name.clone(), r.span);
            match field_body(inner, &name, span, a)? {
                Body::Placed { kind, at, assert, .. } => {
                    Body::Placed { kind, at, absent: Some(PlacedAbsence::ZeroOffset), assert }
                }
                _ => {
                    let (at, at_span) = named;
                    return Err(syn::Error::new(
                        at_span,
                        format!(
                            "field `{name}`: `#[seek({at})]` on an `Option` needs a record. \
                             Use a `#[bytes(N)]` nested layout or a `#[message]` type"
                        ),
                    ));
                }
            }
        }
        (None, Some(inner)) if matches!(a.count, Some((CountSpec::Fill { .. }, _))) => {
            let Some((CountSpec::Fill { cap }, cspan)) = a.count.take() else {
                unreachable!("just matched")
            };
            if cap.is_some() {
                return Err(syn::Error::new(
                    cspan,
                    format!(
                        "field `{name}`: an `Option` has no count to cap. Write `#[fill]` without `cap`"
                    ),
                ));
            }
            if let Body::Fixed { kind, claims } = field_body(inner, &name, span, a)? {
                let body = Body::Opt { when: None, kind, claims };
                return Ok(MsgField { name, span, walk_span, body, stated, with });
            }
            return Err(syn::Error::new(
                span,
                format!(
                    "field `{name}`: `#[fill]` on an `Option` needs a fixed-size value. \
                     Use a fixed-size type"
                ),
            ));
        }
        (None, Some(_)) => {
            return Err(syn::Error::new(
                span,
                format!(
                    "field `{name}`: nothing says whether this `Option` is present. \
                     Add `#[when(flags & 0x01)]`, or `#[seek(off)]` where a zero offset means absent"
                ),
            ));
        }
        (None, None) => field_body(&f.ty, &name, span, a)?,
    };
    Ok(MsgField { name, span, walk_span, body, stated, with })
}

fn optional(name: &str, when: WhenSpec, body: Body) -> syn::Result<Body> {
    let mut claims = Claims::default();
    let kind = match body {
        Body::Bits { .. } => {
            unreachable!("a bit-addressed message parses no field through `optional`")
        }
        Body::Fixed { kind, claims: c } => {
            claims = c;
            kind
        }
        Body::Value { len_ref: Some(r), .. } => {
            return Err(syn::Error::new(
                r.span,
                format!(
                    "field `{name}`: an optional string cannot take its length from `{}`, \
                     which is always present. Write `#[until(0)]` or `#[bytes(N)]`",
                    r.name,
                ),
            ));
        }
        Body::Value { kind, .. } => kind,
        Body::Repeat { .. } => {
            return Err(syn::Error::new(
                when.span,
                format!(
                    "field `{name}`: #[when(..)] cannot make a collection optional. \
                     Remove the `Option` and use a count of zero"
                ),
            ));
        }
        Body::Switch { .. } => {
            return Err(syn::Error::new(
                when.span,
                format!(
                    "field `{name}`: #[when] cannot be combined with #[switch]. \
                     Remove `#[when]` and add an arm with no fields for absence"
                ),
            ));
        }
        Body::Checksum { .. } => {
            return Err(syn::Error::new(
                when.span,
                format!(
                    "field `{name}`: a checksum cannot be optional. Remove `#[when]` and the `Option`"
                ),
            ));
        }
        Body::Placed { kind, at, assert, .. } => match when.test {
            WhenTest::Positive if when.by.name == at.name => {
                return Err(syn::Error::new(
                    when.span,
                    format!(
                        "field `{name}`: `#[when({0} > 0)]` repeats what `#[seek({0})]` already implies. \
                         Remove `#[when]`, or name the record's length field",
                        at.name,
                    ),
                ));
            }
            WhenTest::Positive => {
                return Ok(Body::Placed {
                    kind,
                    at,
                    absent: Some(PlacedAbsence::ZeroLength(when.by)),
                    assert,
                });
            }
            WhenTest::Mask(_) | WhenTest::Flag => {
                let test = match when.test {
                    WhenTest::Mask(mask) => format!("{} & {mask:#x}", when.by.name),
                    _ => when.by.name.clone(),
                };
                return Err(syn::Error::new(
                    when.span,
                    format!(
                        "field `{name}`: `#[when({})]` and `#[seek({})]` both decide whether the record is present. \
                         Remove `#[when]`, or write `#[when(len > 0)]` with the record's length field",
                        test, at.name,
                    ),
                ));
            }
        },
        Body::Opt { .. } => unreachable!("`#[when]` is taken before the body is decided"),
    };
    if matches!(when.test, WhenTest::Positive) {
        return Err(syn::Error::new(
            when.span,
            format!(
                "field `{name}`: `#[when({0} > 0)]` works only with `#[seek]`. \
                 Write `#[when(flags & 0x01)]` with the bit that means present",
                when.by.name,
            ),
        ));
    }
    Ok(Body::Opt { when: Some(when), kind, claims })
}

fn walked_by(a: &FieldAttrs) -> Option<(&'static str, Span)> {
    a.kind
        .as_ref()
        .map(|(k, sp)| (k.spelling(), *sp))
        .or_else(|| a.count.as_ref().map(|(c, sp)| (c.spelling(), *sp)))
}

fn field_body(ty: &Type, name: &str, span: Span, a: FieldAttrs) -> syn::Result<Body> {
    let walked = walked_by(&a);
    let FieldAttrs {
        count,
        stride,
        seek,
        bits,
        bytes,
        until,
        range,
        magic,
        when: _,
        with,
        kind,
        at: _,
    } = a;
    let at = seek;

    if let Some((_, wspan)) = &with
        && !matches!(kind, Some((KindWord::Message, _)))
    {
        return Err(syn::Error::new(
            *wspan,
            format!(
                "field `{name}`: `#[with(..)]` needs a `#[message]` field. \
                 Add `#[message]`, or remove `#[with]`"
            ),
        ));
    }

    let claimed = range
        .as_ref()
        .map(|r| ("#[range]", r.span))
        .or_else(|| magic.as_ref().map(|m| ("#[magic]", m.span)));
    if let Some((says, span)) = claimed
        && let Some((what, _)) = walked
    {
        let why = if what == "#[checksum]" {
            "Remove it"
        } else {
            "Put it on a field of the element type"
        };
        return Err(syn::Error::new(
            span,
            format!("field `{name}`: {says} cannot be combined with {what}. {why}"),
        ));
    }

    if let Some((KindWord::Checksum(ck), ckspan)) = &kind {
        let others = [
            count.as_ref().map(|(c, _)| c.spelling()),
            at.as_ref().map(|_| "#[seek]"),
            bits.map(|_| "#[codec]"),
            bytes.map(|_| "#[bytes]"),
            until.map(|_| "#[until]"),
            stride.as_ref().map(|_| "#[stride]"),
        ];
        if let Some(other) = others.into_iter().flatten().next() {
            return Err(syn::Error::new(
                *ckspan,
                format!(
                    "field `{name}`: #[checksum] cannot be combined with {other}. Remove {other}"
                ),
            ));
        }
        if vec_element(ty).is_some() {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "field `{name}`: #[checksum] needs one integer, not `{}`. \
                     Declare it `<{} as layline::Checksum>::Output`",
                    crate::attr::spelled(ty),
                    crate::attr::spelled(&ck.algorithm),
                ),
            ));
        }
        let Some(repr @ (Scalar::U(_) | Scalar::I(_))) = scalar_of(ty) else {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "field `{name}`: a checksum must be an integer, not `{}`. \
                     Declare it `<{} as layline::Checksum>::Output`",
                    crate::attr::spelled(ty),
                    crate::attr::spelled(&ck.algorithm),
                ),
            ));
        };
        let Some((KindWord::Checksum(ck), _)) = kind else { unreachable!("just matched") };
        return Ok(Body::Checksum {
            repr,
            algorithm: crate::attr::spelled(&ck.algorithm),
            over: ck.over,
            from_ref: ck.from_ref,
            to_ref: ck.to_ref,
            inclusive: ck.inclusive,
        });
    }

    if let Some((KindWord::Switch(_), sspan)) = &kind {
        if let Some((c, cspan)) = &count
            && !matches!(c, CountSpec::Window { .. })
        {
            return Err(syn::Error::new(
                *cspan,
                format!(
                    "field `{name}`: #[switch] cannot take {}. \
                     Write `#[len(g)]`, where `g` is an earlier field holding the size in bytes",
                    c.spelling(),
                ),
            ));
        }
        let others = [
            at.as_ref().map(|_| "#[seek]"),
            bits.map(|_| "#[codec]"),
            until.map(|_| "#[until]"),
            stride.as_ref().map(|_| "#[stride]"),
        ];
        if let Some(other) = others.into_iter().flatten().next() {
            return Err(syn::Error::new(
                *sspan,
                format!(
                    "field `{name}`: #[switch] cannot be combined with {other}. \
                     Remove {other}. Size the union with #[bytes(N)] or #[len(g)]"
                ),
            ));
        }
        let Some((KindWord::Switch(on), _)) = kind else { unreachable!("just matched") };
        if !is_plain_path(ty) {
            return Err(syn::Error::new(
                ty.span(),
                format!(
                    "field `{name}`: #[switch] needs a `#[derive(Message)]` enum, not `{}`. \
                     Declare the field as that enum",
                    crate::attr::spelled(ty),
                ),
            ));
        }
        if let Some((0, bspan)) = bytes {
            return Err(syn::Error::new(
                bspan,
                format!(
                    "field `{name}`: #[bytes(0)] gives the union no bytes. \
                     Remove it, or give the union's real size"
                ),
            ));
        }
        if let (SwitchOn::BodyLength(_), Some((n, bspan))) = (&on, bytes) {
            return Err(syn::Error::new(
                bspan,
                format!(
                    "field `{name}`: #[switch(..)] reads the remaining length, and #[bytes({n})] fixes it. \
                     Remove #[bytes({n})], or switch on a field"
                ),
            ));
        }
        let window = match (count, bytes) {
            (None, None) => SwitchWindow::Open,
            (None, Some((n, _))) => SwitchWindow::Fixed(n),
            (Some((CountSpec::Window { by, .. }, lspan)), Some((n, _))) => {
                return Err(syn::Error::new(
                    lspan,
                    format!(
                        "field `{name}`: #[len({})] and #[bytes({n})] both set the union's size. \
                         Remove one",
                        by.name,
                    ),
                ));
            }
            (Some((CountSpec::Window { by, .. }, lspan)), None)
                if matches!(on, SwitchOn::BodyLength(_)) =>
            {
                return Err(syn::Error::new(
                    lspan,
                    format!(
                        "field `{name}`: #[switch(..)] reads the remaining length, and #[len({})] sets it. \
                         Remove #[len({0})], or write #[switch(n)]",
                        by.name,
                    ),
                ));
            }
            (Some((CountSpec::Window { by, scale, offset, cap }, _)), None) => {
                SwitchWindow::Field { by, scale, offset, cap }
            }
            (Some(_), _) => unreachable!("a count that is not a byte length was refused above"),
        };
        return Ok(Body::Switch { on, ty: crate::attr::spelled(ty), window });
    }

    let discovered = match &kind {
        Some((k @ (KindWord::Text(_) | KindWord::Var), _)) => Some(k.spelling()),
        _ => None,
    };
    if let (Some(what), Some((_, bspan))) = (discovered, bits) {
        return Err(syn::Error::new(
            bspan,
            format!(
                "field `{name}`: #[codec(N)] cannot be combined with {what}, which is sized by reading it. \
                 Remove one"
            ),
        ));
    }
    if let (true, Some(r)) = (matches!(kind, Some((KindWord::Text(_), _))), &at) {
        return Err(syn::Error::new(
            r.span,
            format!(
                "field `{name}`: #[seek] cannot place a string. \
                 Wrap it in a `#[bytes(N)]` nested layout"
            ),
        ));
    }

    let run = run_element(ty);
    let elem = run.map(|(e, _)| e);
    let collection = run.map_or(Collection::Vec, |(_, c)| c);

    // `#[until]` counts a collection. On a string, `text_field` handles it.
    let is_text = matches!(kind, Some((KindWord::Text(_), _)));
    let (count, until) = match (until, elem.is_some() && !is_text) {
        (Some((spec, uspan)), true) => {
            reject_second(&count, uspan, name)?;
            let count = match spec {
                UntilSpec::Mask(mask) => CountSpec::Terminated { mask },
                UntilSpec::Byte(terminator) => CountSpec::Until { terminator },
            };
            (Some((count, uspan)), None)
        }
        (until, _) => (count, until),
    };

    if let (Some((_, uspan)), false) = (&until, is_text) {
        return Err(syn::Error::new(
            *uspan,
            format!(
                "field `{name}`: #[until] needs a string or a `Vec` of one-byte elements, not `{}`. \
                 Write #[fill] or #[var]",
                crate::attr::spelled(ty),
            ),
        ));
    }

    if let Some(s) = &stride
        && let Some(what) = kind.as_ref().map(|(k, _)| k.spelling())
    {
        return Err(syn::Error::new(
            s.span,
            format!("field `{name}`: #[stride] cannot be combined with {what}. Remove #[stride]"),
        ));
    }

    match kind {
        Some((KindWord::Text(codec), tspan)) => {
            let until = match until {
                Some((UntilSpec::Byte(b), uspan)) => Some((b, uspan)),
                Some((UntilSpec::Mask(_), uspan)) => {
                    return Err(syn::Error::new(
                        uspan,
                        format!(
                            "field `{name}`: `#[until(mask = M)]` does not apply to a string. \
                             Write `#[until(0)]` or `#[until(b'\\n')]`"
                        ),
                    ));
                }
                None => None,
            };
            return text_field(
                ty,
                name,
                span,
                TextAttrs { codec, tspan, count, until, bytes, elem },
            );
        }
        Some((KindWord::Var | KindWord::Message, _))
            if matches!(
                count,
                Some((CountSpec::Until { .. } | CountSpec::Terminated { .. }, _))
            ) =>
        {
            let (_, uspan) = count.expect("matched Some");
            let elem_ty = elem.expect("an #[until] count is only made for a `Vec`");
            return Err(syn::Error::new(
                uspan,
                format!(
                    "field `{name}`: #[until] needs one-byte elements, and `{}` is self-delimiting. \
                     Write `#[count(n)]`, `#[len(n)]`, or `#[fill]`",
                    crate::attr::spelled(elem_ty),
                ),
            ));
        }
        Some((KindWord::Var, _)) => {
            return var_field(ty, name, span, VarAttrs { count, bytes, at, elem });
        }
        Some((KindWord::Message, _)) => {
            let with = with.map_or_else(Vec::new, |(refs, _)| refs);
            return message_field(ty, name, span, MsgAttrs { count, bits, bytes, at, elem, with });
        }
        Some((KindWord::Switch(_) | KindWord::Checksum(_), _)) => {
            unreachable!("a switch and a checksum are answered before this point")
        }
        None => {
            if let Some((c @ CountSpec::Window { .. }, cspan)) = &count
                && elem.is_none()
            {
                return Err(syn::Error::new(
                    *cspan,
                    format!(
                        "field `{name}`: {} needs a `Vec`, not `{}`. \
                         Declare a `Vec<T>`, or add `#[text]` or `#[var]`",
                        c.spelling(),
                        crate::attr::spelled(ty),
                    ),
                ));
            }
        }
    }

    match (elem, count) {
        (Some(elem_ty), Some((count, cspan))) => {
            let (element, assert) = element_kind(name, elem_ty, bytes, bits)?;
            if matches!(count, CountSpec::Until { .. }) && element.bits() != Some(8) {
                let width = match element.bits() {
                    Some(bits) => format!("{} bytes wide", bits / 8),
                    None => "of variable size".to_string(),
                };
                return Err(syn::Error::new(
                    cspan,
                    format!(
                        "field `{name}`: #[until(b)] needs one-byte elements, and `{}` is {width}. \
                         Write `#[until(mask = M)]`, or use a count",
                        crate::attr::spelled(elem_ty),
                    ),
                ));
            }
            let count = with_stride(name, count, cspan, stride)?;
            Ok(Body::Repeat { element, count, at, assert, collection })
        }
        (Some(_), None) => Err(syn::Error::new(
            span,
            format!("field `{name}`: a `Vec` needs a count. Add one of {REPEAT_ATTRS}"),
        )),
        (None, Some((count, cspan))) => Err(syn::Error::new(
            cspan,
            format!(
                "field `{name}`: {} needs a `Vec`, not `{}`. \
                 Declare a `Vec<T>`, or write `#[when(flags & 0x01)]` on an `Option<T>`",
                count.spelling(),
                crate::attr::spelled(ty),
            ),
        )),
        (None, None) => {
            if let Some(s) = stride {
                return Err(syn::Error::new(
                    s.span,
                    format!(
                        "field `{name}`: #[stride] needs a `Vec`, not `{}`. Remove #[stride]",
                        crate::attr::spelled(ty),
                    ),
                ));
            }
            let (kind, bound) = fixed_kind(name, ty, bits, bytes, range.as_ref())?;
            if let Some(m) = &magic
                && !matches!(
                    &kind,
                    Kind::Scalar(Scalar::U(_) | Scalar::I(_)) | Kind::Array(Scalar::U(8), _)
                )
            {
                return Err(syn::Error::new(
                    m.span,
                    format!("field `{name}`: {}", crate::claim::MAGIC_PINS_BYTES),
                ));
            }
            let claims = Claims { range: bound, magic };
            let Some(r) = at else { return Ok(Body::Fixed { kind, claims }) };
            let Kind::Nested { bytes: child_bytes, .. } = &kind else {
                return Err(syn::Error::new(
                    r.span,
                    format!(
                        "field `{name}`: #[seek] needs a record, not `{}`. \
                         Use a `#[derive(Layout)]` type with `#[bytes(N)]`",
                        crate::attr::spelled(ty),
                    ),
                ));
            };
            let assert = Some(Box::new(ElementSize {
                ty: ty.clone(),
                bytes: *child_bytes,
                span: bytes.expect("a nested kind came from #[bytes]").1,
            }));
            Ok(Body::Placed { kind, at: r, absent: None, assert })
        }
    }
}

fn with_stride(
    name: &str,
    count: CountSpec,
    cspan: Span,
    stride: Option<StrideSpec>,
) -> syn::Result<CountSpec> {
    let Some(stride) = stride else { return Ok(count) };
    match count {
        CountSpec::Field { by, scale, offset, cap } => {
            if by.name == stride.by.name {
                return Err(syn::Error::new(
                    stride.by.span,
                    format!(
                        "field `{name}`: `{}` cannot be both the count and the stride. \
                         Use a separate field for each",
                        by.name,
                    ),
                ));
            }
            Ok(CountSpec::Strided { by, scale, offset, cap, stride })
        }
        CountSpec::Window { .. } => {
            let mut err = syn::Error::new(
                stride.span,
                format!(
                    "field `{name}`: #[stride] needs #[count], not #[len]. \
                     Write #[count(n)] for the number of elements and #[stride(s)] for the element size"
                ),
            );
            err.combine(syn::Error::new(cspan, "#[len] declared here"));
            Err(err)
        }
        other => {
            let mut err = syn::Error::new(
                stride.span,
                format!(
                    "field `{name}`: #[stride] needs #[count], not {}. \
                     Write #[count(n)], where `n` is an earlier field holding the number of elements",
                    other.spelling(),
                ),
            );
            err.combine(syn::Error::new(cspan, "declared here"));
            Err(err)
        }
    }
}
