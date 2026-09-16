//! Verbatim items, copied through unless they declare a codec.

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use crate::Error;

/// Codec derives, matched by last path segment, so `layline::Layout` matches `Layout`.
const CODEC_DERIVES: [&str; 4] = ["Layout", "Message", "FieldCodec", "Dispatch"];

/// Refuses a verbatim item that derives a codec or has `#[layout(..)]`, at any depth.
pub(super) fn refuse_codec(item: &TokenStream) -> Result<(), Error> {
    let Some(what) = codec_attr(item.clone()) else {
        return Ok(());
    };
    Err(Error::Refused(format!(
        "a verbatim item contains `{what}`. Declare codecs with `Item::Layout`, \
         `Item::Message`, `Item::Choice`, `Item::Dispatch` or `Item::Enum`"
    )))
}

/// The first codec attribute in `tokens`, as written.
fn codec_attr(tokens: TokenStream) -> Option<String> {
    let mut trees = tokens.into_iter().peekable();
    while let Some(tree) = trees.next() {
        match tree {
            TokenTree::Punct(p) if p.as_char() == '#' => {
                if let Some(TokenTree::Punct(bang)) = trees.peek()
                    && bang.as_char() == '!'
                {
                    trees.next();
                }
                if let Some(TokenTree::Group(g)) = trees.peek()
                    && g.delimiter() == Delimiter::Bracket
                    && let Some(what) = codec_meta(g.stream())
                {
                    return Some(what);
                }
            }
            TokenTree::Group(g) => {
                if let Some(what) = codec_attr(g.stream()) {
                    return Some(what);
                }
            }
            _ => {}
        }
    }
    None
}

/// The codec an attribute declares, either `layout(..)` or a codec in `derive(..)`.
fn codec_meta(meta: TokenStream) -> Option<String> {
    let mut trees = meta.into_iter().peekable();
    while let Some(tree) = trees.next() {
        match tree {
            TokenTree::Ident(id) => {
                let Some(TokenTree::Group(g)) = trees.peek() else {
                    continue;
                };
                if g.delimiter() != Delimiter::Parenthesis {
                    continue;
                }
                match id.to_string().as_str() {
                    "layout" => return Some(String::from("#[layout(..)]")),
                    "derive" => {
                        if let Some(d) = derived_codec(g.stream()) {
                            return Some(format!("#[derive({d})]"));
                        }
                    }
                    _ => {}
                }
            }
            TokenTree::Group(g) => {
                if let Some(what) = codec_meta(g.stream()) {
                    return Some(what);
                }
            }
            _ => {}
        }
    }
    None
}

/// The codec in a `derive(..)` list, matched by last path segment.
fn derived_codec(list: TokenStream) -> Option<String> {
    for tree in list {
        match tree {
            TokenTree::Ident(id) => {
                let name = id.to_string();
                if CODEC_DERIVES.contains(&name.as_str()) {
                    return Some(name);
                }
            }
            TokenTree::Group(g) => {
                if let Some(what) = derived_codec(g.stream()) {
                    return Some(what);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use quote::quote;

    use crate::emit::{Module, generate};
    use crate::{Container, Endian, Error, Field, Item, Kind, LayoutDef, Scalar};

    #[test]
    fn a_verbatim_item_may_not_declare_a_codec() {
        let refused = [
            ("#[derive(Layout)]", quote! { #[derive(Layout)] pub struct Rolled { pub a: u8 } }),
            ("#[derive(Message)]", quote! { #[derive(layline::Message)] pub struct Rolled; }),
            (
                "#[derive(FieldCodec)]",
                quote! { #[derive(::elsewhere::FieldCodec)] pub enum Rolled { A } },
            ),
            (
                "#[derive(Dispatch)]",
                quote! {
                    pub mod inner {
                        #[cfg_attr(feature = "wire", derive(Dispatch))]
                        pub enum Rolled { A }
                    }
                },
            ),
            ("#[layout(..)]", quote! { #[layout(bytes = 4)] pub struct Rolled { pub a: u32 } }),
        ];
        for (what, tokens) in refused {
            let module = Module { items: vec![Item::Verbatim(tokens)], ..Module::new(Vec::new()) };
            let Err(Error::Refused(why)) = generate(&module) else {
                panic!("`{what}` in a verbatim item must be refused");
            };
            assert!(why.contains(what), "the refusal names the spelling it found: {why}");
            assert!(why.contains("Item::Layout"), "and where the declaration belongs: {why}");
        }
    }

    #[test]
    fn an_ordinary_verbatim_item_is_carried_through_and_formatted() {
        let module = Module {
            doc: vec!["A consumer's module.".into()],
            items: vec![
                Item::Layout(LayoutDef {
                    name: "Entry".into(),
                    container: Container::Bytes { bytes: 4, endian: Endian::Le },
                    fields: vec![Field::new("value", Kind::Scalar(Scalar::U(32)))],
                    view: false,
                }),
                Item::Verbatim(quote! {
                    impl Entry {
                        /// Whether this entry is the zero one.
                        #[must_use] pub fn is_zero(&self) -> bool { self.value == 0 }
                    }
                    #[derive(Debug, Default)] pub struct Cursor { pub at: usize }
                }),
            ],
            ..Module::new(Vec::new())
        };
        let out = generate(&module).expect("a verbatim item that is not a codec passes").source;
        assert!(out.contains("impl Entry {\n"), "{out}");
        assert!(out.contains("    pub fn is_zero(&self) -> bool {\n"), "{out}");
        assert!(out.contains("        self.value == 0\n"), "{out}");
        assert!(out.contains("/// Whether this entry is the zero one."), "{out}");
        assert!(out.contains("#[derive(Debug, Default)]"), "{out}");
        let (decl, accessor) = (out.find("pub struct Entry"), out.find("impl Entry"));
        assert!(decl < accessor && accessor.is_some(), "{out}");
    }
}
