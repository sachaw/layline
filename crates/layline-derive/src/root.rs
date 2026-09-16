//! The path generated code uses to reach the runtime.
//!
//! `::layline` by default, or `crate = <path>` when the dependency is renamed.

use layline_codegen::{Root, Spelling};
use proc_macro2::TokenTree;
use syn::meta::ParseNestedMeta;

/// The root for a refusal's stub.
///
/// Reads `crate = <path>` from raw tokens, because the attribute itself may have failed to parse.
pub fn of(input: &syn::DeriveInput) -> Root {
    input
        .attrs
        .iter()
        .filter(|a| ["layout", "message", "dispatch", "bits"].iter().any(|k| a.path().is_ident(k)))
        .filter_map(|a| a.meta.require_list().ok())
        .flat_map(|list| {
            let tokens: Vec<TokenTree> = list.tokens.clone().into_iter().collect();
            tokens
                .split(|t| matches!(t, TokenTree::Punct(p) if p.as_char() == ','))
                .filter_map(|arg| match arg {
                    [TokenTree::Ident(k), TokenTree::Punct(eq), path @ ..]
                        if k == "crate" && eq.as_char() == '=' =>
                    {
                        syn::parse2(path.iter().cloned().collect()).ok()
                    }
                    _ => None,
                })
                .collect::<Vec<syn::Path>>()
        })
        .last()
        .map_or_else(Root::default, |path| Root::new(path, Spelling::Hygienic))
}

pub fn parse_arg(meta: &ParseNestedMeta<'_>) -> syn::Result<Root> {
    let path: syn::Path = meta.value()?.parse()?;
    Ok(Root::new(path, Spelling::Hygienic))
}

#[cfg(test)]
mod tests {
    use syn::DeriveInput;

    fn expand(src: &str) -> String {
        let input: DeriveInput = syn::parse_str(src).expect("a declaration");
        let derive = input
            .attrs
            .iter()
            .find(|a| a.path().is_ident("derive"))
            .map(|a| a.meta.require_list().expect("derive(..)").tokens.to_string())
            .expect("a derive");
        let out = match derive.as_str() {
            "Layout" => crate::layout::derive(&input),
            "Message" => crate::message::derive(&input),
            "FieldCodec" => crate::codec::derive(&input),
            "Dispatch" => crate::dispatch::derive(&input),
            other => panic!("no derive named {other}"),
        };
        out.unwrap_or_else(|e| panic!("{src}\nrefused: {e}")).to_string()
    }

    #[track_caller]
    fn assert_same_expansion(bare: &str, stated: &str) {
        let a = expand(bare);
        let b = expand(stated);
        assert_eq!(a, b, "`crate = ::layline` expanded differently from saying nothing");
        assert!(a.contains(":: layline ::"), "the root is written into the expansion:\n{a}");
    }

    #[test]
    fn layout_default_root_is_byte_identical_to_stating_it() {
        assert_same_expansion(
            "#[derive(Layout)] #[layout(bits = 16, prefix = 3)] struct W { #[bits(13)] a: u16 }",
            "#[derive(Layout)] #[layout(bits = 16, prefix = 3, crate = ::layline)] \
             struct W { #[bits(13)] a: u16 }",
        );
    }

    #[test]
    fn message_default_root_is_byte_identical_to_stating_it() {
        assert_same_expansion(
            "#[derive(Message)] struct M { n: u8, #[count(n)] v: Vec<u8> }",
            "#[derive(Message)] #[message(crate = ::layline)] \
             struct M { n: u8, #[count(n)] v: Vec<u8> }",
        );
        assert_same_expansion(
            "#[derive(Message)] enum C { #[value(1)] A(u8), #[other] Other(Vec<u8>) }",
            "#[derive(Message)] #[message(crate = ::layline)] \
             enum C { #[value(1)] A(u8), #[other] Other(Vec<u8>) }",
        );
    }

    #[test]
    fn field_codec_default_root_is_byte_identical_to_stating_it() {
        assert_same_expansion(
            "#[derive(FieldCodec)] #[bits(3)] struct F(u8);",
            "#[derive(FieldCodec)] #[bits(3, crate = ::layline)] struct F(u8);",
        );
    }

    #[test]
    fn dispatch_default_root_is_byte_identical_to_stating_it() {
        assert_same_expansion(
            "#[derive(Dispatch)] #[dispatch(id = u8, prefix = 3)] \
             enum D { #[value(1)] A(W), #[other] Unknown { id: u8, body: Vec<u8> } }",
            "#[derive(Dispatch)] #[dispatch(id = u8, prefix = 3, crate = ::layline)] \
             enum D { #[value(1)] A(W), #[other] Unknown { id: u8, body: Vec<u8> } }",
        );
    }

    #[test]
    fn a_renamed_root_leaves_no_default_path_behind() {
        for src in [
            "#[derive(Layout)] #[layout(bytes = 2, crate = ::wire)] struct W { a: u16 }",
            "#[derive(Message)] #[message(crate = ::wire)] \
             struct M { n: u8, #[count(n)] v: Vec<u8>, #[text] #[until(0)] s: String }",
            "#[derive(Message)] #[message(crate = ::wire)] \
             enum C { #[value(1)] A(u8), #[other] Other(Vec<u8>) }",
            "#[derive(FieldCodec)] #[bits(3, crate = ::wire)] struct F(u8);",
            "#[derive(Dispatch)] #[dispatch(id = u8, prefix = 3, crate = ::wire)] \
             enum D { #[value(1)] A(W), #[other] Unknown { id: u8, body: Vec<u8> } }",
        ] {
            let out = expand(src);
            // `__layline_*` idents and the `'__layline` lifetime do not match `:: layline`.
            assert!(!out.contains(":: layline"), "{src}\nstill names layline:\n{out}");
            assert!(out.contains(":: wire ::"), "{src}\nnever names the root:\n{out}");
        }
    }
}
