//! Whether a message's or catalogue's encode can refuse a value: its `ENCODE_FALLIBLE`.

use proc_macro2::{TokenStream, TokenTree};
use quote::quote;

use super::recursion::names_self;
use crate::walk::path;
use crate::{Kind, Root};

/// Whether encode statements `body` refuse a value themselves: they name `EncodeError`.
pub(super) fn refuses_itself(body: &TokenStream) -> bool {
    body.clone().into_iter().any(|t| match t {
        TokenTree::Ident(i) => i == "EncodeError",
        TokenTree::Group(g) => refuses_itself(&g.stream()),
        _ => false,
    })
}

/// `ENCODE_FALLIBLE` of an item that refuses values itself if `own`, and otherwise as any type in
/// `via` does: `true`, `false`, or the `||` of their constants, as `DECODE_FALLIBLE` joins the
/// layouts a layout nests.
pub fn refusal(own: bool, via: &[TokenStream]) -> TokenStream {
    if own || via.iter().any(|v| v.to_string() == "true") {
        return quote!(true);
    }
    let mut seen: Vec<String> = Vec::new();
    let mut kept: Vec<&TokenStream> = Vec::new();
    for v in via {
        let s = v.to_string();
        if !seen.contains(&s) {
            seen.push(s);
            kept.push(v);
        }
    }
    if kept.is_empty() {
        return quote!(false);
    }
    quote!(#(#kept)||*)
}

/// The value of a [`refusal`], if it is the literal `true` or `false`.
pub fn known(fallible: &TokenStream) -> Option<bool> {
    match fallible.to_string().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Whether encoding a held `kind` can refuse, or `None` if it never does.
///
/// A message held through `Box` or `Vec` may hold this item in turn, and a constant cannot be
/// worked out from itself, so one other than this item counts as refusing. This item itself adds
/// nothing.
pub(super) fn held_refusal(
    kind: &Kind,
    owner: &str,
    indirect: bool,
    root: &Root,
) -> Option<TokenStream> {
    match kind {
        Kind::Nested { ty, .. } | Kind::NestedArray { ty, .. } => {
            let p = path(ty);
            Some(quote!(<#p as #root::Layout>::ENCODE_FALLIBLE))
        }
        Kind::Var { ty } => {
            let p = path(ty);
            Some(quote!(<#p as #root::VarCodec>::ENCODE_FALLIBLE))
        }
        Kind::Codec { ty, .. } => {
            let p = path(ty);
            Some(quote!(<#p as #root::FieldCodec>::ENCODE_FALLIBLE))
        }
        Kind::Msg { ty, .. } if names_self(ty, owner) => None,
        Kind::Msg { boxed: true, .. } => Some(quote!(true)),
        Kind::Msg { .. } if indirect => Some(quote!(true)),
        Kind::Msg { ty, .. } => {
            let p = path(ty);
            Some(quote!(<#p as #root::Message>::ENCODE_FALLIBLE))
        }
        _ => None,
    }
}

/// The error of an encode into a buffer, and the value of an encode that returns `t`, for an
/// item whose `ENCODE_FALLIBLE` is `fallible`: plain types when it is a literal.
pub fn signatures(
    fallible: &TokenStream,
    t: &TokenStream,
    root: &Root,
) -> (TokenStream, TokenStream) {
    match known(fallible) {
        Some(true) => {
            (quote!(#root::EncodeError), quote!(::core::result::Result<#t, #root::EncodeError>))
        }
        Some(false) => (quote!(#root::Overflow), quote!(#t)),
        None => (
            quote!(#root::__private::Buffered<{ #fallible }>),
            quote!(#root::__private::Encoded<#t, { #fallible }>),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_with_nothing_to_refuse_is_infallible() {
        assert_eq!(refusal(false, &[]).to_string(), "false");
        let (into, owned) = signatures(&quote!(false), &quote!(V), &Root::default());
        assert_eq!(into.to_string(), quote!(::layline::Overflow).to_string());
        assert_eq!(owned.to_string(), "V");
    }

    #[test]
    fn its_own_check_or_a_refusing_holding_makes_it_refuse() {
        assert_eq!(refusal(true, &[]).to_string(), "true");
        let via = [quote!(<A as ::layline::Layout>::ENCODE_FALLIBLE), quote!(true)];
        assert_eq!(refusal(false, &via).to_string(), "true");
    }

    #[test]
    fn holdings_join_once_each() {
        let a = quote!(<A as ::layline::Layout>::ENCODE_FALLIBLE);
        let b = quote!(<B as ::layline::VarCodec>::ENCODE_FALLIBLE);
        let joined = refusal(false, &[a.clone(), b.clone(), a.clone()]);
        assert_eq!(joined.to_string(), quote!(#a || #b).to_string());
        assert_eq!(known(&joined), None);
        let (into, _) = signatures(&joined, &quote!(V), &Root::default());
        assert!(into.to_string().contains("Buffered"), "{into}");
    }

    #[test]
    fn a_message_held_indirectly_refuses_unless_it_is_this_one() {
        let root = Root::default();
        let held = |ty: &str, boxed| Kind::Msg { ty: ty.into(), boxed, with: Vec::new() };
        assert!(held_refusal(&held("Node", true), "Node", true, &root).is_none());
        let other = held_refusal(&held("Leaf", false), "Node", true, &root).unwrap();
        assert_eq!(other.to_string(), "true");
        let boxed = held_refusal(&held("Leaf", true), "Node", false, &root).unwrap();
        assert_eq!(boxed.to_string(), "true");
        let direct = held_refusal(&held("Leaf", false), "Node", false, &root).unwrap();
        assert!(direct.to_string().contains("Message"), "{direct}");
    }
}
