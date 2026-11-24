//! An attribute's expression: `layline_codegen::Expr`, with a span for each name it reads.

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::spanned::Spanned;

use layline_codegen::Expr;

use crate::claim::Ref;

/// An expression as an attribute wrote it.
pub(crate) struct Value {
    pub(crate) expr: Expr,
    /// Each field or parameter it reads, where it was written, in order.
    pub(crate) refs: Vec<Ref>,
    /// The whole expression's span.
    pub(crate) span: Span,
}

impl Value {
    /// Parses `tokens` with the one expression parser, `Expr::parse`. `what` is the attribute,
    /// such as `#[count]`, as a refusal names it.
    pub(crate) fn parse(tokens: &TokenStream, name: &str, what: &str) -> syn::Result<Self> {
        Self::parse_in(tokens, &format!("field `{name}`"), what)
    }

    /// [`parse`](Self::parse), with `owner` naming what the attribute is on, as in
    /// ``variant `Pdw` ``.
    pub(crate) fn parse_in(tokens: &TokenStream, owner: &str, what: &str) -> syn::Result<Self> {
        let span = tokens.span();
        let expr = Expr::parse(&tokens.to_string()).map_err(|why| {
            syn::Error::new(span, format!("{owner}: {what} takes an expression. {why}"))
        })?;
        let mut refs = Vec::new();
        names(tokens.clone(), &mut refs);
        // The tokens find each name's span. The parser decides what is a name.
        let read = expr.names();
        refs.retain(|r: &Ref| read.contains(&r.name.as_str()));
        Ok(Self { expr, refs, span })
    }

    /// Copies `v`. `Value` is not `Clone`, as [`Ref`] is not.
    pub(crate) fn same(v: &Value) -> Value {
        Value { expr: v.expr.clone(), refs: v.refs.iter().map(Ref::same).collect(), span: v.span }
    }

    /// The reference to `target`, if the expression reads it.
    pub(crate) fn find(&self, target: &str) -> Option<&Ref> {
        self.refs.iter().find(|r| r.name == target)
    }

    /// The bare field the expression is, if it is one.
    pub(crate) fn bare(&self) -> Option<&Ref> {
        self.expr.as_name().and_then(|n| self.find(n))
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.expr)
    }
}

/// Every `name` or `outer.inner` in `tokens` that is not part of a `::` path, with its span.
fn names(tokens: TokenStream, out: &mut Vec<Ref>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let punct =
        |t: Option<&TokenTree>, c: char| matches!(t, Some(TokenTree::Punct(p)) if p.as_char() == c);
    let mut i = 0;
    while i < trees.len() {
        match &trees[i] {
            TokenTree::Group(g) => names(g.stream(), out),
            TokenTree::Ident(id) => {
                let path = (i > 0 && punct(trees.get(i - 1), ':')) || punct(trees.get(i + 1), ':');
                if !path {
                    let mut name = syn::ext::IdentExt::unraw(id).to_string();
                    if punct(trees.get(i + 1), '.')
                        && let Some(TokenTree::Ident(inner)) = trees.get(i + 2)
                    {
                        name.push('.');
                        name.push_str(&syn::ext::IdentExt::unraw(inner).to_string());
                        i += 2;
                    }
                    out.push(Ref { name, span: id.span() });
                }
            }
            _ => {}
        }
        i += 1;
    }
}

/// An attribute's arguments: the expression, if any, and each `key = value` after it.
pub(crate) struct Args {
    pub(crate) expr: Option<TokenStream>,
    pub(crate) keys: Vec<(syn::Ident, TokenStream)>,
}

/// Splits `#[attr(expr, key = value, ..)]` at its top-level commas.
pub(crate) fn args(attr: &syn::Attribute) -> syn::Result<Args> {
    let mut out = Args { expr: None, keys: Vec::new() };
    let syn::Meta::List(list) = &attr.meta else { return Ok(out) };
    let mut parts: Vec<Vec<TokenTree>> = vec![Vec::new()];
    for t in list.tokens.clone() {
        match &t {
            TokenTree::Punct(p) if p.as_char() == ',' => parts.push(Vec::new()),
            _ => parts.last_mut().expect("one part").push(t),
        }
    }
    for part in parts.into_iter().filter(|p| !p.is_empty()) {
        let keyed = match (&part.first(), part.get(1), part.get(2)) {
            (Some(TokenTree::Ident(k)), Some(TokenTree::Punct(eq)), next)
                if eq.as_char() == '='
                    && !matches!(next, Some(TokenTree::Punct(p)) if p.as_char() == '=') =>
            {
                Some(k.clone())
            }
            _ => None,
        };
        match keyed {
            Some(key) => out.keys.push((key, part[2..].iter().cloned().collect())),
            None if out.expr.is_none() && out.keys.is_empty() => {
                out.expr = Some(part.into_iter().collect());
            }
            None => {
                let tokens: TokenStream = part.into_iter().collect();
                return Err(syn::Error::new(
                    tokens.span(),
                    "expected one expression, then `key = value` arguments",
                ));
            }
        }
    }
    Ok(out)
}
