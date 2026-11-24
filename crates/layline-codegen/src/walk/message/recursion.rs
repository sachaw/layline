//! Self-reference and open-ended bodies.

use proc_macro2::TokenStream;
use quote::quote;

use super::Check;
use super::body::{Item, Walk};
use super::refuse::End;
use crate::walk::path;
use crate::{Error, Field, Kind, Root, Segment};

/// Whether `ty` names the item being generated.
pub(super) fn names_self(ty: &str, owner: &str) -> bool {
    let ty = ty.trim();
    ty == owner || ty == "Self"
}

/// The refusal for a field that always contains its own type.
pub(super) fn unbounded_recursion(owner: &str, field: &str) -> String {
    format!(
        "`{owner}`: `{field}` is a nested `{owner}` that is always present, so the wire never ends. \
         Put it behind an empty collection, a flag, or a switch arm"
    )
}

/// Where a body ends: by itself, or through the type its last field holds. A condition after it
/// takes no bytes.
pub(super) fn open_end(segments: &[Segment], owner: &str, root: &Root) -> End {
    let through = |ty: &str, of: TokenStream| {
        if names_self(ty, owner) {
            return End::default();
        }
        let p = path(ty);
        End { open: false, through: vec![quote!(<#p as #root::#of>::NODE)] }
    };
    match segments.iter().rfind(|s| !matches!(s, Segment::Valid { .. })) {
        Some(Segment::Switch { choice, window: None, .. }) => through(choice, quote!(Choice)),
        Some(
            Segment::Value { kind: Kind::Msg { ty, .. }, .. }
            | Segment::Opt { field: Field { kind: Kind::Msg { ty, .. }, .. }, .. }
            | Segment::Placed { kind: Kind::Msg { ty, .. }, .. },
        ) => through(ty, quote!(Message)),
        Some(seg) => End { open: seg.open_ended(), through: Vec::new() },
        None => End::default(),
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
