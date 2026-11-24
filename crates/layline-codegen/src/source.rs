//! Rendering tokens as formatted source.

use proc_macro2::TokenStream;

use crate::Error;

/// Parses tokens as a file, pretty-prints it, and turns `#[doc]` attributes back into comments.
///
/// # Errors
///
/// [`Error::Internal`], including the tokens, when they do not parse as a file.
pub(crate) fn render(tokens: TokenStream) -> Result<String, Error> {
    let file: syn::File = syn::parse2(tokens.clone()).map_err(|e| {
        Error::Internal(format!("emitted tokens do not parse as a file: {e}\n{tokens}"))
    })?;
    Ok(restore_doc_comments(&prettyplease::unparse(&file)))
}

fn restore_doc_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let rewritten =
            doc_line(trimmed, "#[doc = ", "]").map(|body| format!("{indent}///{body}")).or_else(
                || doc_line(trimmed, "#![doc = ", "]").map(|body| format!("{indent}//!{body}")),
            );
        out.push_str(rewritten.as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

/// The text of a line that is one `#[doc]` attribute of one line, as a macro body prints it.
fn doc_line(line: &str, open: &str, close: &str) -> Option<String> {
    let lit = line.strip_prefix(open)?.strip_suffix(close)?;
    let text = syn::parse_str::<syn::LitStr>(lit).ok()?.value();
    (!text.contains('\n')).then_some(text)
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::*;

    #[test]
    fn docs_survive_the_round_trip() {
        let src = render(quote! {
            //! The module.
            #[doc = " A \"quoted\" thing."]
            pub struct S;
        })
        .expect("a file");
        assert!(src.contains("//! The module."), "{src}");
        assert!(src.contains("/// A \"quoted\" thing."), "{src}");
        assert!(!src.contains("#[doc"), "{src}");
    }

    /// A `/** */` block, re-indented by rustfmt, would be a doctest that rustdoc runs.
    #[test]
    fn a_doc_of_several_paragraphs_is_line_comments() {
        let doc = crate::walk::doc_lines("One.\n\n# Errors\n\nTwo.");
        let src = render(quote! {
            #doc
            pub struct S;
            m! { #doc fn f() {} }
        })
        .expect("a file");
        assert!(!src.contains("/**"), "{src}");
        assert!(src.contains("/// One.\n///\n/// # Errors\n///\n/// Two.\n"), "{src}");
        for line in src.lines().filter(|l| l.trim_start().starts_with("///")) {
            assert!(!line.contains("#[doc"), "one comment per attribute: {src}");
        }
    }

    #[test]
    fn malformed_emission_fails_at_generation() {
        let Err(Error::Internal(why)) = render("pub struct".parse().expect("tokens")) else {
            panic!("tokens that are not a file are the generator's bug, reported as one");
        };
        assert!(why.contains("emitted tokens do not parse as a file"), "{why}");
        assert!(why.contains("pub struct"), "the tokens are named: {why}");
    }
}
