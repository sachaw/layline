//! Self-reference and open-ended bodies.

use proc_macro2::TokenStream;
use quote::quote;

use super::Check;
use super::body::{Item, Walk};
use crate::walk::path;
use crate::{Error, Field, Kind, Root, Segment};

/// Whether `ty` names the item being generated.
pub(super) fn names_self(ty: &str, owner: &str) -> bool {
    let ty = ty.trim();
    ty == owner || ty == "Self"
}

/// Whether `ty` is open-ended, when that is known here: a type on the cycle, or the item itself.
fn resolved(ty: &str, owner: &str, cyclic: &[(String, bool)]) -> Option<bool> {
    let ty = ty.trim();
    if let Some((_, value)) = cyclic.iter().find(|(n, _)| n == ty) {
        return Some(*value);
    }
    names_self(ty, owner).then_some(false)
}

/// The refusal for a field that always contains its own type.
pub(super) fn unbounded_recursion(owner: &str, field: &str) -> String {
    format!(
        "`{owner}`: `{field}` is a nested `{owner}` that is always present, so the wire never ends. \
         Put it behind an empty collection, a flag, or a switch arm"
    )
}

/// Whether the body runs to its end: a `bool` literal, or the last type's `OPEN_ENDED`.
pub(super) fn open_ended_expr(
    segments: &[Segment],
    owner: &str,
    cyclic: &[(String, bool)],
    root: &Root,
) -> TokenStream {
    let forward = |ty: &str, of: TokenStream| {
        if let Some(value) = resolved(ty, owner, cyclic) {
            return quote!(#value);
        }
        let p = path(ty);
        quote!(<#p as #root::#of>::OPEN_ENDED)
    };
    match segments.last() {
        Some(Segment::Switch { choice, window: None, .. }) => forward(choice, quote!(Choice)),
        Some(
            Segment::Value { kind: Kind::Msg { ty, .. }, .. }
            | Segment::Opt { field: Field { kind: Kind::Msg { ty, .. }, .. }, .. }
            | Segment::Placed { kind: Kind::Msg { ty, .. }, .. },
        ) => forward(ty, quote!(Message)),
        Some(seg) if seg.open_ended() => quote!(true),
        _ => quote!(false),
    }
}

/// Assert that `ty`, which another field follows, does not run to the end of the body.
pub(super) fn bounded_check(
    owner: &str,
    field: &str,
    ty: &str,
    what: &str,
    root: &Root,
) -> TokenStream {
    let p = path(ty);
    let of = if what == "switch" { quote!(Choice) } else { quote!(Message) };
    let msg = format!(
        "`{owner}`: a field follows `{field}`, but {what} `{ty}` runs to the end of the body. \
         Put `{field}` last, size every arm as `[u8; N]`, or close the catalogue"
    );
    quote! {
        const _: () = ::core::assert!(!<#p as #root::#of>::OPEN_ENDED, #msg);
    }
}

/// Refuse a value that directly contains itself.
fn sized_check(owner: &str, field: &str, ty: &str, boxed: bool) -> Result<(), Error> {
    if boxed || !names_self(ty, owner) {
        return Ok(());
    }
    Err(Error::Refused(format!(
        "`{owner}`: `{field}` contains a `{owner}` directly, so the type has infinite size. \
         Use `Box<{owner}>`, or `Vec<{owner}>` for a sequence"
    )))
}

impl Walk<'_> {
    /// Refuse a nested message with infinite size, and assert it is bounded if a segment follows.
    pub(super) fn held_message(
        &self,
        item: &mut Item<'_>,
        field: &str,
        kind: &Kind,
        last: bool,
    ) -> Result<(), Error> {
        let Kind::Msg { ty, boxed, .. } = kind else { return Ok(()) };
        sized_check(self.owner, field, ty, *boxed)?;
        if !last {
            item.checks.push(Check {
                field: String::from(field),
                tokens: bounded_check(self.owner, field, ty, "nested message", self.root),
            });
        }
        Ok(())
    }
}
