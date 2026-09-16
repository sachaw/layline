//! Whole-message checks, and codegen errors pointed at the attribute that caused them.

use layline_codegen::{Kind, Len};

use super::*;
use crate::claim::Ref;

pub(super) fn invalid(p: &Parsed, why: &layline_codegen::Invalid) -> syn::Error {
    match why {
        layline_codegen::Invalid::UnknownReference(target) => unknown_reference(p, target),
        layline_codegen::Invalid::NonIntegerReference(target) => non_integer_reference(p, target),
        layline_codegen::Invalid::NotAFlagReference(target) => not_a_flag_reference(p, target),
        layline_codegen::Invalid::NotANestedLayout(target) => not_a_nested_layout(p, target),
        layline_codegen::Invalid::AfterOpenEnd(_) => after_open_end(p),
        layline_codegen::Invalid::Field { at, why } => syn::Error::new(
            p.fields.iter().find(|f| f.name == *at).map_or_else(|| p.ident.span(), |f| f.span),
            format!("field `{at}`: {why}"),
        ),
        other => syn::Error::new(p.ident.span(), format!("`{}`: {other}", p.ident)),
    }
}

pub(super) fn reference_site<'a>(
    p: &'a Parsed,
    target: &str,
) -> Option<(&'a Ref, usize, &'static str)> {
    for (i, f) in p.fields.iter().enumerate() {
        if let Some(r) = f.with.iter().find(|r| r.name == target) {
            return Some((r, i, "#[with]"));
        }
        match &f.body {
            Body::Repeat { count, at, .. } => {
                if let Some(by) = count.by()
                    && by.name == target
                {
                    return Some((by, i, count.spelling()));
                }
                if let Some(by) = count.stride()
                    && by.name == target
                {
                    return Some((by, i, "#[stride]"));
                }
                if let Some(r) = at
                    && r.name == target
                {
                    return Some((r, i, "#[seek]"));
                }
            }
            Body::Placed { at, .. } if at.name == target => {
                return Some((at, i, "#[seek]"));
            }
            Body::Placed { absent: Some(PlacedAbsence::ZeroLength(by)), .. }
                if by.name == target =>
            {
                return Some((by, i, "#[when]"));
            }
            Body::Value { len_ref: Some(r), .. } if r.name == target => {
                return Some((r, i, "#[len]"));
            }
            Body::Switch { on, .. } if on.field().is_some_and(|r| r.name == target) => {
                return Some((on.field().expect("checked"), i, "#[switch]"));
            }
            Body::Switch { window, .. } if window.field().is_some_and(|r| r.name == target) => {
                return Some((window.field().expect("checked"), i, "#[len]"));
            }
            Body::Opt { when: Some(when), .. } if when.by.name == target => {
                return Some((&when.by, i, "#[when]"));
            }
            _ => {}
        }
    }
    None
}

fn other_claim<'a>(p: &'a Parsed, target: &str) -> Option<(&'a str, &'static str)> {
    for f in &p.fields {
        let what = match &f.body {
            Body::Repeat { count: CountSpec::Field { by, .. }, .. } if by.name == target => "count",
            Body::Repeat { count: CountSpec::Window { by, .. }, .. } if by.name == target => {
                "byte length"
            }
            Body::Repeat { count: CountSpec::Strided { by, .. }, .. } if by.name == target => {
                "count"
            }
            Body::Repeat { count: CountSpec::Strided { stride, .. }, .. }
                if stride.by.name == target =>
            {
                "stride"
            }
            Body::Repeat { at: Some(r), .. } if r.name == target => "offset",
            Body::Placed { at, .. } if at.name == target => "offset",
            Body::Placed { absent: Some(PlacedAbsence::ZeroLength(by)), .. }
                if by.name == target =>
            {
                "byte length"
            }
            Body::Value { len_ref: Some(r), .. } if r.name == target => "length prefix",
            Body::Switch { on, .. } if on.field().is_some_and(|r| r.name == target) => {
                "discriminant"
            }
            Body::Switch { window, .. } if window.field().is_some_and(|r| r.name == target) => {
                "byte length"
            }
            _ => continue,
        };
        return Some((&f.name, what));
    }
    None
}

fn choices(p: &Parsed, at_index: usize) -> String {
    let earlier: Vec<String> = p.fields[..at_index]
        .iter()
        .filter_map(|f| match bound(&f.body) {
            Some(k) if k.is_integer() => Some(format!("`{}`", f.name)),
            Some(Kind::Nested { .. }) => {
                Some(format!("a field inside `{0}` (`{0}.<field>`)", f.name))
            }
            _ => None,
        })
        .collect();
    if earlier.is_empty() {
        String::from("No earlier field is an integer")
    } else {
        format!("Earlier integer fields: {}", earlier.join(", "))
    }
}

fn unknown_reference(p: &Parsed, target: &str) -> syn::Error {
    let Some((r, at_index, what)) = reference_site(p, target) else {
        return syn::Error::new(
            p.ident.span(),
            format!("`{}`: `{target}` is not a field of this message", p.ident),
        );
    };
    let owner = &p.fields[at_index].name;
    let choices = choices(p, at_index);
    let (outer, inner) = layline_codegen::__derive::split_ref(target);

    let later = p.fields.iter().any(|f| {
        f.name == outer
            && match inner {
                None => bound(&f.body).is_some_and(Kind::is_integer),
                Some(_) => matches!(bound(&f.body), Some(Kind::Nested { .. })),
            }
    });
    if later {
        return syn::Error::new(
            r.span,
            format!(
                "field `{owner}`: {what} names `{target}`, which comes after this field. \
                 Move `{outer}` above `{owner}`. {choices}"
            ),
        );
    }
    syn::Error::new(
        r.span,
        format!(
            "field `{owner}`: {what} names `{target}`, which is not an integer field of this message. \
             {choices}"
        ),
    )
}

fn non_integer_reference(p: &Parsed, target: &str) -> syn::Error {
    let Some((r, at_index, what)) = reference_site(p, target) else {
        return syn::Error::new(
            p.ident.span(),
            format!("`{}`: `{target}` does not carry a number", p.ident),
        );
    };
    let owner = &p.fields[at_index].name;
    let carries = carried_by(p, target);
    let choices = choices(p, at_index);
    syn::Error::new(
        r.span,
        format!(
            "field `{owner}`: {what} needs an integer, and `{target}` is {carries}. \
             Name an earlier integer or `#[var]` field. {choices}"
        ),
    )
}

fn not_a_flag_reference(p: &Parsed, target: &str) -> syn::Error {
    let Some((r, at_index, _)) = reference_site(p, target) else {
        return syn::Error::new(
            p.ident.span(),
            format!("`{}`: `{target}` is not a `bool`", p.ident),
        );
    };
    let owner = &p.fields[at_index].name;
    let carries = carried_by(p, target);
    let fix = if carried_by(p, target) == "an integer" {
        format!("Write `#[when({target} & 0x01)]` to test a bit")
    } else {
        format!("Name a `bool`, or write `#[when({target} & 0x01)]`. {}", choices(p, at_index),)
    };
    syn::Error::new(
        r.span,
        format!(
            "field `{owner}`: #[when({target})] needs a `bool`, and `{target}` is {carries}. {fix}"
        ),
    )
}

fn carried_by(p: &Parsed, target: &str) -> &'static str {
    let (outer, _) = layline_codegen::__derive::split_ref(target);
    p.fields
        .iter()
        .find(|f| f.name == outer)
        .and_then(|f| bound(&f.body))
        .map_or("no number", Kind::describe)
}

fn not_a_nested_layout(p: &Parsed, target: &str) -> syn::Error {
    let (outer, inner) = layline_codegen::__derive::split_ref(target);
    let inner = inner.unwrap_or(target);
    let Some((r, at_index, what)) = reference_site(p, target) else {
        return syn::Error::new(
            p.ident.span(),
            format!(
                "`{}`: `{target}` names nothing, because `{outer}` is not a nested layout",
                p.ident
            ),
        );
    };
    let owner = &p.fields[at_index].name;
    let carries = carried_by(p, target);
    let choices = choices(p, at_index);
    syn::Error::new(
        r.span,
        format!(
            "field `{owner}`: {what} names `{target}`, but `{outer}` is {carries}, not a nested layout. \
             Give `{outer}` a `#[derive(Layout)]` type, or remove `.{inner}`. {choices}"
        ),
    )
}

pub(super) fn check_coverage(p: &Parsed) -> syn::Result<()> {
    let mandatory = |owner: &str, at: Option<usize>| -> String {
        let names: Vec<&str> = p.fields[..at.unwrap_or(p.fields.len())]
            .iter()
            .filter(|g| !sometimes_absent(&g.body) && g.name != owner)
            .map(|g| g.name.as_str())
            .collect();
        if names.is_empty() {
            String::from("No field can bound the range, so write `over = ..`")
        } else {
            format!(
                "Always-present fields: {}",
                names.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", "),
            )
        }
    };
    let resolve = |owner: &str, r: &Ref, which: &str, at: usize| -> syn::Result<usize> {
        let Some(j) = p.fields.iter().position(|g| g.name == r.name) else {
            return Err(syn::Error::new(
                r.span,
                format!(
                    "field `{owner}`: the checksum range {which} `{}`, which is not a field of this message. \
                     {}",
                    r.name,
                    mandatory(owner, None),
                ),
            ));
        };
        if sometimes_absent(&p.fields[j].body) {
            let (only, instead) = match &p.fields[j].body {
                Body::Placed { absent: Some(PlacedAbsence::ZeroLength(by)), .. } => (
                    format!("when `{}` is non-zero", by.name),
                    format!("Name `{}` itself", by.name),
                ),
                Body::Placed { at, .. } => (
                    format!("when `{}` is non-zero", at.name),
                    format!("Name `{}` itself", at.name),
                ),
                _ => (
                    String::from("when a flag says so"),
                    String::from("Name the flags word itself"),
                ),
            };
            return Err(syn::Error::new(
                r.span,
                format!(
                    "field `{owner}`: the checksum range {which} `{}`, which is present only {only}. \
                     {instead}, or a field that is always present. {}",
                    r.name,
                    mandatory(owner, Some(at)),
                ),
            ));
        }
        Ok(j)
    };

    for (i, f) in p.fields.iter().enumerate() {
        let Body::Checksum { from_ref, to_ref, inclusive, .. } = &f.body else {
            continue;
        };
        let from = from_ref.as_ref().map(|r| resolve(&f.name, r, "starts at", i)).transpose()?;
        let to = to_ref
            .as_ref()
            .map(|r| resolve(&f.name, r, if *inclusive { "runs through" } else { "ends at" }, i))
            .transpose()?;

        if from == Some(i) {
            let span = from_ref.as_ref().expect("a resolved bound was written").span;
            return Err(syn::Error::new(
                span,
                format!(
                    "field `{}`: the checksum range cannot start at the checksum itself. \
                     Start it at a field before or after `{}`",
                    f.name, f.name,
                ),
            ));
        }
        if to == Some(i) {
            let span = to_ref.as_ref().expect("a resolved bound was written").span;
            return Err(syn::Error::new(
                span,
                if *inclusive {
                    format!(
                        "field `{}`: the checksum range cannot include `{}`, the checksum itself. \
                         Remove the end bound. The range ends before `{}` by default",
                        f.name, f.name, f.name,
                    )
                } else {
                    format!(
                        "field `{}`: the checksum range already ends at `{}` by default. \
                         Remove the end bound, or name another field",
                        f.name, f.name,
                    )
                },
            ));
        }

        let last = match (to, *inclusive) {
            (Some(j), true) => Some(j),
            (Some(j), false) => j.checked_sub(1),
            (None, _) => i.checked_sub(1),
        };
        let Some(last) = last else {
            let (span, what) = match to_ref {
                Some(r) => (r.span, format!("ends at `{}`", r.name)),
                None => (f.span, String::from("ends at this field")),
            };
            return Err(syn::Error::new(
                span,
                format!(
                    "field `{}`: the checksum range {what}, so it covers nothing. \
                     End it at a later field",
                    f.name,
                ),
            ));
        };
        if from.is_some_and(|from| from > last) {
            let r = from_ref.as_ref().expect("a resolved bound was written");
            let ends = match to_ref {
                Some(t) if *inclusive => format!("runs through `{}`", t.name),
                Some(t) => format!("ends at `{}`", t.name),
                None => format!("ends at `{}`", f.name),
            };
            return Err(syn::Error::new(
                r.span,
                format!(
                    "field `{}`: the checksum range starts at `{}` but {ends}, which comes first. \
                     Write `over = {}..` or `over = {}..=<field>`. {}",
                    f.name,
                    r.name,
                    r.name,
                    r.name,
                    mandatory(&f.name, None),
                ),
            ));
        }
    }
    Ok(())
}

pub(super) fn check_predicates(p: &Parsed) -> syn::Result<()> {
    for f in &p.fields {
        let Body::Opt { when: Some(when), .. } = &f.body else {
            continue;
        };

        if let Some((owner, what)) = other_claim(p, &when.by.name) {
            return Err(syn::Error::new(
                when.by.span,
                format!(
                    "field `{}`: `{}` is already the {what} of `{owner}`, so it cannot also be a flags word. \
                     Use a separate field for the presence bits",
                    f.name, when.by.name,
                ),
            ));
        }

        let WhenTest::Mask(mask) = when.test else {
            if let Some(other) = p.fields.iter().find(|g| {
                g.name != f.name
                    && matches!(&g.body, Body::Opt { when: Some(w), .. } if w.by.name == when.by.name)
            }) {
                return Err(syn::Error::new(
                    when.by.span,
                    format!(
                        "field `{}`: `{}` is a `bool`, and `{}` also uses it. \
                         Make it an integer flags word and test one bit per field",
                        f.name, when.by.name, other.name,
                    ),
                ));
            }
            continue;
        };

        let Some(bits) = p
            .fields
            .iter()
            .find(|g| g.name == when.by.name)
            .and_then(|g| bound(&g.body))
            .and_then(Kind::bits)
        else {
            continue;
        };
        if bits < 64 && mask >> bits != 0 {
            return Err(syn::Error::new(
                when.span,
                format!(
                    "field `{}`: the mask tests bit {}, but `{}` is only {bits} bits wide. \
                     Use bits {}, or widen the field",
                    f.name,
                    63 - mask.leading_zeros(),
                    when.by.name,
                    if bits == 0 { String::from("none") } else { format!("0..={}", bits - 1) },
                ),
            ));
        }
    }
    Ok(())
}

fn after_open_end(p: &Parsed) -> syn::Error {
    let mut open: Option<(&MsgField, &'static str)> = None;
    for f in &p.fields {
        if let Some((prev, spelling)) = open {
            return syn::Error::new(
                f.span,
                format!(
                    "field `{}`: follows `{}`, whose {spelling} reads to the end of the message. \
                     Move `{}` last, or give it `#[count(n)]`",
                    f.name, prev.name, prev.name,
                ),
            );
        }
        match &f.body {
            Body::Repeat { count, .. } if count.open_ended() => {
                open = Some((f, count.spelling()));
            }
            Body::Value { kind: Kind::Text { len: Len::Fill, .. }, .. }
            | Body::Opt { kind: Kind::Text { len: Len::Fill, .. }, .. } => {
                open = Some((f, "#[fill]"));
            }
            _ => {}
        }
    }
    syn::Error::new(
        p.ident.span(),
        format!("`{}`: a field follows one that reads to the end of the message", p.ident),
    )
}
