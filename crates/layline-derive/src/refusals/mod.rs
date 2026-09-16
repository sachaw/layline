//! Tests for derive refusals and the compile-time checks the derives emit.

mod attrs;
mod bits;
mod catalogue;
mod checks;
mod checksum;
mod codec;
mod dispatch;
mod layout;
mod message;
mod needs;
mod optional;
mod switch;

use syn::DeriveInput;

/// Runs every derive on every item, as the compiler does.
fn each_derive(src: &str, mut sink: impl FnMut(syn::Result<proc_macro2::TokenStream>)) {
    let file: syn::File = syn::parse_str(src).expect("a test declaration parses");
    for item in file.items {
        let tokens = match &item {
            syn::Item::Struct(s) => quote::quote!(#s),
            syn::Item::Enum(e) => quote::quote!(#e),
            _ => continue,
        };
        let Ok(input) = syn::parse2::<DeriveInput>(tokens) else { continue };
        for attr in &input.attrs {
            if !attr.path().is_ident("derive") {
                continue;
            }
            let mut names = Vec::new();
            let _ = attr.parse_nested_meta(|m| {
                if let Some(i) = m.path.segments.last() {
                    names.push(i.ident.to_string());
                }
                Ok(())
            });
            for n in names {
                let r = match n.as_str() {
                    "Message" => crate::message::derive(&input),
                    "Layout" => crate::layout::derive(&input),
                    "FieldCodec" => crate::codec::derive(&input),
                    "Dispatch" => crate::dispatch::derive(&input),
                    _ => continue,
                };
                sink(r);
            }
        }
    }
}

fn refusals(src: &str) -> String {
    let mut out = String::new();
    each_derive(src, |r| {
        if let Err(e) = r {
            out.push_str(&e.to_string());
            out.push('\n');
        }
    });
    out
}

#[track_caller]
fn refused(src: &str, reason: &str) {
    let got = refusals(src);
    assert!(!got.is_empty(), "the derives accepted this declaration");
    assert!(got.contains(reason), "refused, but not for that reason:\n{got}");
}

/// Tokens as text, without spaces or trailing commas.
/// rustfmt adds trailing commas to multi-line fragments and `quote!` does not.
fn tokens(t: &proc_macro2::TokenStream) -> String {
    let mut text = t.to_string().replace(' ', "");
    for closing in [",)", ",]", ",}", ",>"] {
        text = text.replace(closing, &closing[1..]);
    }
    text
}

/// The derives' combined output for `src`, as [`tokens`]. Panics on a refusal.
fn expansion(src: &str) -> String {
    let mut out = String::new();
    each_derive(src, |r| match r {
        Ok(t) => out.push_str(&tokens(&t)),
        Err(e) => panic!("refused: {e}"),
    });
    out
}

/// Asserts the derives accept `src` and their output contains `fragment`.
#[track_caller]
fn expands_with(src: &str, fragment: proc_macro2::TokenStream) {
    let got = expansion(src);
    let want = tokens(&fragment);
    assert!(got.contains(&want), "the expansion does not contain\n{fragment}\n\nexpansion:\n{got}");
}

/// Asserts `derive` refuses `src` with exactly one error, and `stub` still emits each `surface` item.
/// The stub keeps later uses of the type from raising more errors.
#[track_caller]
fn refused_once_with_stub(
    src: &str,
    derive: fn(&DeriveInput) -> syn::Result<proc_macro2::TokenStream>,
    stub: fn(&DeriveInput) -> proc_macro2::TokenStream,
    surface: &[proc_macro2::TokenStream],
) {
    let input: DeriveInput = syn::parse_str(src).expect("a declaration");
    let err = derive(&input).expect_err("the derive accepted this declaration");
    assert_eq!(err.into_iter().count(), 1, "one refusal is one error");
    let left = tokens(&stub(&input));
    for item in surface {
        let want = tokens(item);
        assert!(left.contains(&want), "the stub does not carry\n{item}\n\nstub:\n{left}");
    }
}

const NESTED_COUNT: &str = r#"#[derive(Message)]
pub struct Packet {
    #[bytes(1)]
    pub header: PackedHeader,
    #[count(header.n_entires)]
    pub items: Vec<u8>,
}"#;
