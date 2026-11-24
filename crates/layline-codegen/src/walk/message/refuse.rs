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

/// What encoding a held `kind` adds to the holder's refusals.
pub(super) enum Held {
    /// A constant the holder joins into its own: a layout's, codec's or var codec's.
    Leaf(TokenStream),
    /// A message the holder links to in its [`Node`](layline_core::__private::Node), which may
    /// hold the holder in turn.
    Link(TokenStream),
}

/// What encoding a held `kind` adds, or `None` if it adds nothing: a scalar, or this item.
pub(super) fn held_refusal(kind: &Kind, owner: &str, root: &Root) -> Option<Held> {
    match kind {
        Kind::Nested { ty, .. } | Kind::NestedArray { ty, .. } => {
            let p = path(ty);
            Some(Held::Leaf(quote!(<#p as #root::Layout>::ENCODE_FALLIBLE)))
        }
        Kind::Var { ty } => {
            let p = path(ty);
            Some(Held::Leaf(quote!(<#p as #root::VarCodec>::ENCODE_FALLIBLE)))
        }
        Kind::Codec { ty, .. } => {
            let p = path(ty);
            Some(Held::Leaf(quote!(<#p as #root::FieldCodec>::ENCODE_FALLIBLE)))
        }
        Kind::Msg { ty, .. } if names_self(ty, owner) => None,
        Kind::Msg { ty, .. } => {
            let p = path(ty);
            Some(Held::Link(quote!(<#p as #root::Message>::NODE)))
        }
        _ => None,
    }
}

/// Where a body ends: by itself, and through the types its last field holds.
#[derive(Default)]
pub(super) struct End {
    /// Whether the body reads to its end by itself.
    pub(super) open: bool,
    /// The types whose open end is the body's.
    pub(super) through: Vec<TokenStream>,
}

/// An item's `NODE`, `OPEN_ENDED` and `ENCODE_FALLIBLE`, and the value of `ENCODE_FALLIBLE` that a
/// signature names.
pub(super) struct Constants {
    /// `const NODE`, or nothing for an item that keeps the default.
    pub(super) node: TokenStream,
    pub(super) open_ended: TokenStream,
    pub(super) encode_fallible: TokenStream,
    pub(super) fallible: TokenStream,
}

impl Constants {
    /// Every constant, for an item with an open end.
    pub(super) fn all(&self) -> TokenStream {
        let Self { node, open_ended, encode_fallible, .. } = self;
        quote! { #node #open_ended #encode_fallible }
    }
}

/// The constants of the item `name`, in its impl of `of`, that refuses `own` by itself, links to
/// `links` and ends as `end` says. `id` distinguishes its node from every other.
///
/// An item that links to nothing keeps the default node, and its constants are what it decides
/// alone. Otherwise its node is a `static`, and each constant is a search from it, which a cycle
/// of types cannot make depend on itself.
pub(super) fn constants(
    own: &TokenStream,
    links: &[TokenStream],
    end: &End,
    (name, of): (&TokenStream, &TokenStream),
    id: &str,
    root: &Root,
) -> Constants {
    let bool_ty = root.prelude().bool;
    let open = end.open;
    let links = dedup(links);
    let ends = if open { Vec::new() } else { dedup(&end.through) };
    let reach = quote!(#root::__private::reach);
    let (node, link) = (quote!(#root::__private::Node), quote!(#root::__private::Link));
    let linked = !links.is_empty() || !ends.is_empty();
    let open_ended = if ends.is_empty() {
        quote!(#open)
    } else {
        quote!(#reach::open_ended(<Self as #of>::NODE))
    };
    let (fallible, value) = match known(own) {
        Some(true) => (quote!(true), quote!(true)),
        _ if links.is_empty() => (own.clone(), own.clone()),
        _ => {
            (quote!(<#name as #of>::ENCODE_FALLIBLE), quote!(#reach::refuses(<Self as #of>::NODE)))
        }
    };
    let node = linked.then(|| {
        quote! {
            const NODE: #link = {
                static NODE: #node = #node {
                    id: ::core::concat!(
                        ::core::line!(), ":", ::core::column!(), ":", #id, ":", ::core::module_path!()
                    ),
                    open_ended: #open,
                    refuses: #own,
                    ends: &[#(#ends),*],
                    holds: &[#(#links),*],
                };
                #link::new(&NODE)
            };
        }
    });
    Constants {
        node: node.unwrap_or_default(),
        open_ended: quote! { const OPEN_ENDED: #bool_ty = #open_ended; },
        encode_fallible: quote! { const ENCODE_FALLIBLE: #bool_ty = #value; },
        fallible,
    }
}

fn dedup(all: &[TokenStream]) -> Vec<TokenStream> {
    let mut seen: Vec<String> = Vec::new();
    let mut kept = Vec::new();
    for t in all {
        let s = t.to_string();
        if !seen.contains(&s) {
            seen.push(s);
            kept.push(t.clone());
        }
    }
    kept
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
    fn a_held_message_is_a_link_and_this_one_adds_nothing() {
        let root = Root::default();
        let held = |ty: &str, boxed| Kind::Msg { ty: ty.into(), boxed, with: Vec::new() };
        assert!(held_refusal(&held("Node", true), "Node", &root).is_none());
        let Some(Held::Link(other)) = held_refusal(&held("Leaf", true), "Node", &root) else {
            panic!("a link")
        };
        assert!(other.to_string().contains("NODE"), "{other}");
    }

    #[test]
    fn an_item_that_links_nowhere_decides_its_constants_alone() {
        let root = Root::default();
        let (m, of) = (quote!(M), quote!(::layline::Message));
        let this = (&m, &of);
        let alone = constants(&quote!(false), &[], &End::default(), this, "M", &root);
        assert_eq!(alone.fallible.to_string(), "false");
        assert!(alone.node.is_empty());
        let linked = constants(&quote!(false), &[quote!(L)], &End::default(), this, "M", &root);
        assert!(linked.node.to_string().contains("static NODE"), "{}", linked.node);
        assert!(linked.fallible.to_string().contains("ENCODE_FALLIBLE"));
        let refusing = constants(&quote!(true), &[quote!(L)], &End::default(), this, "M", &root);
        assert_eq!(refusing.fallible.to_string(), "true");
    }
}
