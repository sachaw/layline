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
        let rewritten = trimmed
            .strip_prefix("#[doc = \"")
            .and_then(|r| r.strip_suffix("\"]"))
            .map(|body| format!("{indent}///{}", unescape(body)))
            .or_else(|| {
                trimmed
                    .strip_prefix("#![doc = \"")
                    .and_then(|r| r.strip_suffix("\"]"))
                    .map(|body| format!("{indent}//!{}", unescape(body)))
            });
        out.push_str(rewritten.as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
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

    #[test]
    fn malformed_emission_fails_at_generation() {
        let Err(Error::Internal(why)) = render("pub struct".parse().expect("tokens")) else {
            panic!("tokens that are not a file are the generator's bug, reported as one");
        };
        assert!(why.contains("emitted tokens do not parse as a file"), "{why}");
        assert!(why.contains("pub struct"), "the tokens are named: {why}");
    }
}
