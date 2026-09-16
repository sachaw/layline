//! The runtime path and prelude spelling for generated code.

use proc_macro2::{Group, Span, TokenStream, TokenTree};
use quote::{ToTokens, format_ident, quote};

/// How generated code spells prelude names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spelling {
    /// Fully qualified, such as `::core::primitive::u32` and `<root>::__private::Vec`.
    ///
    /// For derives, whose output lands in modules where these names may be shadowed.
    Hygienic,
    /// Unqualified, such as `u32`, `Some` and `Vec`. For committed source.
    Bare,
}

/// Prelude names in the root's [`Spelling`].
///
/// Runtime paths such as `<root>::Message` are not listed. They are always qualified.
pub(crate) struct Prelude {
    pub ok: TokenStream,
    pub err: TokenStream,
    pub some: TokenStream,
    pub none: TokenStream,
    pub option: TokenStream,
    pub result: TokenStream,
    pub vec: TokenStream,
    pub string: TokenStream,
    pub boxed: TokenStream,
    pub u8: TokenStream,
    pub u32: TokenStream,
    pub u64: TokenStream,
    pub usize: TokenStream,
    pub i64: TokenStream,
    pub bool: TokenStream,
    qualified: bool,
}

impl Prelude {
    /// The primitive `name`, in this prelude's spelling.
    pub(crate) fn primitive(&self, name: &str) -> TokenStream {
        let ident = format_ident!("{name}");
        if self.qualified { quote!(::core::primitive::#ident) } else { quote!(#ident) }
    }
}

/// The path to the layline runtime, and the prelude [`Spelling`].
///
/// Set the path when a crate renames the dependency, as in `wire = { package = "layline" }`.
#[derive(Clone)]
pub struct Root {
    path: syn::Path,
    spelling: Spelling,
}

impl Root {
    /// A root at `path`, such as a derive's `crate = <path>`.
    #[must_use]
    pub fn new(path: syn::Path, spelling: Spelling) -> Self {
        Self { path, spelling }
    }

    /// The runtime path.
    #[must_use]
    pub fn path(&self) -> &syn::Path {
        &self.path
    }

    fn is_default(&self) -> bool {
        self.path.to_token_stream().to_string()
            == Self::default().path.to_token_stream().to_string()
    }

    /// `, crate = <path>` for the container attribute of generated derives.
    ///
    /// Empty for the default root, so default output matches omitting the argument.
    pub(crate) fn crate_arg(&self) -> TokenStream {
        if self.is_default() {
            return TokenStream::new();
        }
        let path = &self.path;
        quote! { , crate = #path }
    }

    /// The prelude names, spelled for this root.
    pub(crate) fn prelude(&self) -> Prelude {
        let root = &self.path;
        match self.spelling {
            Spelling::Hygienic => Prelude {
                ok: quote!(::core::result::Result::Ok),
                err: quote!(::core::result::Result::Err),
                some: quote!(::core::option::Option::Some),
                none: quote!(::core::option::Option::None),
                option: quote!(::core::option::Option),
                result: quote!(::core::result::Result),
                vec: quote!(#root::__private::Vec),
                string: quote!(#root::__private::String),
                boxed: quote!(#root::__private::Box),
                u8: quote!(::core::primitive::u8),
                u32: quote!(::core::primitive::u32),
                u64: quote!(::core::primitive::u64),
                usize: quote!(::core::primitive::usize),
                i64: quote!(::core::primitive::i64),
                bool: quote!(::core::primitive::bool),
                qualified: true,
            },
            Spelling::Bare => Prelude {
                ok: quote!(Ok),
                err: quote!(Err),
                some: quote!(Some),
                none: quote!(None),
                option: quote!(Option),
                result: quote!(Result),
                vec: quote!(Vec),
                string: quote!(String),
                boxed: quote!(Box),
                u8: quote!(u8),
                u32: quote!(u32),
                u64: quote!(u64),
                usize: quote!(usize),
                i64: quote!(i64),
                bool: quote!(bool),
                qualified: false,
            },
        }
    }
}

/// `root`'s path with every token spanned to `span`.
///
/// `quote_spanned!` only spans the tokens it writes itself.
#[must_use]
pub fn respan(root: &Root, span: Span) -> TokenStream {
    fn each(tokens: TokenStream, span: Span) -> TokenStream {
        tokens
            .into_iter()
            .map(|tree| match tree {
                TokenTree::Group(g) => {
                    let mut re = Group::new(g.delimiter(), each(g.stream(), span));
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
    each(root.path.to_token_stream(), span)
}

impl Default for Root {
    /// `::layline`, hygienic.
    fn default() -> Self {
        Self { path: syn::parse_quote!(::layline), spelling: Spelling::Hygienic }
    }
}

impl ToTokens for Root {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.path.to_tokens(tokens);
    }
}

impl core::fmt::Debug for Root {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let spelling = match self.spelling {
            Spelling::Hygienic => "",
            Spelling::Bare => ", bare",
        };
        write!(f, "Root({}{spelling})", self.path.to_token_stream())
    }
}

#[cfg(test)]
mod tests {
    use super::{Root, Spelling};
    use quote::quote;

    fn lowered(root: &Root) -> proc_macro2::TokenStream {
        let p = root.prelude();
        let (vec, u8, u32, usize, i64, result, option, some, ok, boxed) =
            (p.vec, p.u8, p.u32, p.usize, p.i64, p.result, p.option, p.some, p.ok, p.boxed);
        quote! {
            pub entries: #vec<Entry>,
            fn decode_nested(body: &[#u8], __depth: #u32) -> #result<(Self, #usize), #root::ParseError> {
                let __n: #option<#i64> = #some(1);
                #ok((#boxed::new(x), 0))
            }
        }
    }

    #[test]
    fn a_hygienic_root_qualifies_every_prelude_name() {
        let out = lowered(&Root::default()).to_string();
        let want = quote! {
            pub entries: ::layline::__private::Vec<Entry>,
            fn decode_nested(
                body: &[::core::primitive::u8],
                __depth: ::core::primitive::u32
            ) -> ::core::result::Result<(Self, ::core::primitive::usize), ::layline::ParseError> {
                let __n: ::core::option::Option<::core::primitive::i64> =
                    ::core::option::Option::Some(1);
                ::core::result::Result::Ok((::layline::__private::Box::new(x), 0))
            }
        }
        .to_string();
        assert_eq!(out, want);
    }

    #[test]
    fn a_bare_root_spells_the_prelude_bare_and_the_runtime_qualified() {
        let root = Root::new(syn::parse_quote!(::layline), Spelling::Bare);
        let out = lowered(&root).to_string();
        let want = quote! {
            pub entries: Vec<Entry>,
            fn decode_nested(body: &[u8], __depth: u32) -> Result<(Self, usize), ::layline::ParseError> {
                let __n: Option<i64> = Some(1);
                Ok((Box::new(x), 0))
            }
        }
        .to_string();
        assert_eq!(out, want);
    }

    #[test]
    fn a_named_primitive_follows_the_spelling() {
        let bare = Root::new(syn::parse_quote!(::wire), Spelling::Bare);
        assert_eq!(bare.prelude().primitive("u16").to_string(), "u16");
        assert_eq!(
            Root::default().prelude().primitive("u16").to_string(),
            quote!(::core::primitive::u16).to_string()
        );
        assert_eq!(bare.prelude().vec.to_string(), "Vec");
        assert_eq!(
            Root::new(syn::parse_quote!(::wire), Spelling::Hygienic).prelude().vec.to_string(),
            quote!(::wire::__private::Vec).to_string()
        );
    }

    #[test]
    fn only_the_default_root_writes_no_crate_arg() {
        assert!(Root::default().crate_arg().is_empty());
        assert!(Root::new(syn::parse_quote!(::layline), Spelling::Bare).crate_arg().is_empty());
        assert_eq!(
            Root::new(syn::parse_quote!(::wire), Spelling::Bare).crate_arg().to_string(),
            quote!(, crate = ::wire).to_string()
        );
    }
}
