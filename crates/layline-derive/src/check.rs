//! Compile-time checks on types the derive cannot resolve.

use proc_macro2::{Group, Span, TokenStream, TokenTree};
use quote::{format_ident, quote_spanned};

use layline_codegen::Root;

pub(crate) fn respan(tokens: TokenStream, span: Span) -> TokenStream {
    tokens
        .into_iter()
        .map(|tree| match tree {
            TokenTree::Group(g) => {
                let mut re = Group::new(g.delimiter(), respan(g.stream(), span));
                re.set_span(span);
                TokenTree::Group(re)
            }
            mut other => {
                other.set_span(span);
                other
            }
        })
        .collect()
}

/// Escapes braces so `text` can be an `assert!` message.
pub(crate) fn assert_msg(text: String) -> String {
    text.replace('{', "{{").replace('}', "}}")
}

/// Emits a check that `ty` implements `bound`, a path under the root, reported at `span`.
///
/// The compiler prints the probe function's name beside an unmet bound.
pub(crate) fn bound(
    probe: &str,
    field: &str,
    ty: &TokenStream,
    bound: TokenStream,
    span: Span,
    root: &Root,
) -> TokenStream {
    let root = layline_codegen::__derive::respan(root, span);
    let probe = format_ident!("__layline_{probe}_{field}");
    quote_spanned! {span=>
        const _: () = {
            #[allow(non_snake_case)]
            fn #probe<T: #root::#bound>() {}
            let _ = #probe::<#ty>;
        };
    }
}
