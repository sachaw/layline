//! Model values as tokens: types, attributes and field declarations.

use proc_macro2::TokenStream;
use quote::quote;

use crate::root::Prelude;
use crate::{CoverFrom, CoverTo, Coverage, Endian, Field, Kind, Root, Scalar, Stated};
#[cfg(feature = "emit")]
use crate::{Error, Invalid};

/// A field's type as the `TYPES` table spells it, with spaces removed.
#[cfg(feature = "emit")]
pub fn collection_ty(kind: &Kind) -> String {
    carrier_tokens(kind).to_string().replace(' ', "")
}

/// `#[derive(Debug, Clone, PartialEq, ...)]` for a generated item, then one `#[cfg_attr(..)]`
/// for each predicate the derives name.
///
/// # Errors
///
/// [`Error::Invalid`], naming `derives`, for an entry that is not a path a `#[derive]` can name,
/// a `cfg` that is not a predicate, or a path listed twice.
#[cfg(feature = "emit")]
pub fn derive_attr(derives: &[crate::emit::Derive]) -> Result<TokenStream, Error> {
    let mut always = TokenStream::new();
    let mut gated: Vec<(String, syn::Meta, Vec<syn::Path>)> = Vec::new();
    let mut listed: Vec<&str> = Vec::new();

    for d in derives {
        let path: syn::Path = syn::parse_str(&d.path).map_err(|_| {
            invalid(format!(
                "`{}` is not a valid derive path. \
                 Write one derive per entry, such as `serde::Serialize`",
                d.path
            ))
        })?;
        if listed.contains(&d.path.as_str()) {
            return Err(invalid(format!(
                "`{}` appears twice in `derives`. List each derive once",
                d.path
            )));
        }
        listed.push(&d.path);

        let Some(cfg) = &d.cfg else {
            always.extend(quote!(, #path));
            continue;
        };
        let meta: syn::Meta = syn::parse_str(cfg).map_err(|_| {
            invalid(format!(
                "`{cfg}` is not a valid `cfg` predicate. \
                 Write one predicate, such as `feature = \"serde\"`"
            ))
        })?;
        let key = quote!(#meta).to_string();
        match gated.iter_mut().find(|(seen, _, _)| *seen == key) {
            Some((_, _, paths)) => paths.push(path),
            None => gated.push((key, meta, vec![path])),
        }
    }

    let gates =
        gated.iter().map(|(_, cfg, paths)| quote! { #[cfg_attr(#cfg, derive(#(#paths),*))] });
    Ok(quote! {
        #[derive(Debug, Clone, PartialEq #always)]
        #(#gates)*
    })
}

/// An [`Error::Invalid`] naming the `derives` entry that is wrong.
#[cfg(feature = "emit")]
fn invalid(why: String) -> Error {
    Error::Invalid(String::from("derives"), Invalid::Other(why))
}

/// Parses a type path that validation already accepted.
pub(crate) fn path(ty: &str) -> syn::Path {
    syn::parse_str(ty).expect("`validate` admits only type paths")
}

/// A model name as an identifier, raw if it is a keyword.
pub(crate) fn ident(name: &str) -> proc_macro2::Ident {
    let span = proc_macro2::Span::call_site();
    if syn::parse_str::<syn::Ident>(name).is_ok() {
        proc_macro2::Ident::new(name, span)
    } else {
        proc_macro2::Ident::new_raw(name, span)
    }
}

/// Documentation, as `#[doc]` lines.
pub fn doc_attr(doc: &Option<String>) -> TokenStream {
    match doc {
        Some(text) => {
            let lines = text.lines().map(|l| {
                let l = format!(" {l}");
                quote! { #[doc = #l] }
            });
            quote! { #(#lines)* }
        }
        None => quote!(),
    }
}

/// [`Scalar::primitive`], as tokens.
pub fn scalar_ty(s: Scalar) -> TokenStream {
    let ident = ident(s.primitive());
    quote!(#ident)
}

/// The Rust type for a field's [`Kind`].
///
/// Scalars stay bare primitives, the only form the `Layout` derive accepts. `String` and `Box`
/// use `root`'s prelude.
pub fn kind_ty(kind: &Kind, root: &Root) -> TokenStream {
    let Prelude { string, boxed, .. } = root.prelude();
    match kind {
        Kind::Msg { ty, boxed: b, with: _ } => {
            let p = path(ty);
            if *b { quote!(#boxed<#p>) } else { quote!(#p) }
        }
        Kind::Text { .. } => string,
        other => carrier_tokens(other),
    }
}

/// [`kind_ty`] as the `TYPES` table spells it, without the root.
fn carrier_tokens(kind: &Kind) -> TokenStream {
    match kind {
        Kind::Scalar(s) => scalar_ty(*s),
        Kind::Array(s, dims) => dims.iter().rev().fold(scalar_ty(*s), |t, d| {
            let d = proc_macro2::Literal::usize_unsuffixed(*d);
            quote!([#t; #d])
        }),
        Kind::Codec { ty, .. } | Kind::Nested { ty, .. } => {
            let p = path(ty);
            quote!(#p)
        }
        Kind::NestedArray { ty, len, .. } => {
            let p = path(ty);
            let len = proc_macro2::Literal::usize_unsuffixed(*len);
            quote!([#p; #len])
        }
        Kind::Var { ty } => {
            let p = path(ty);
            quote!(#p)
        }
        Kind::Msg { ty, boxed, with: _ } => {
            let p = path(ty);
            if *boxed { quote!(Box<#p>) } else { quote!(#p) }
        }
        Kind::Checksum { repr, .. } => scalar_ty(*repr),
        Kind::Text { .. } => quote!(String),
    }
}

/// The width attribute of a `#[derive(Layout)]` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidthAttr {
    /// `#[bits(N)]`: a bit field.
    Bits(u64),
    /// `#[codec(N)]`: a `FieldCodec` value `N` bits wide, in a byte container.
    Codec(u64),
    /// `#[bytes(N)]`: the width of a nested layout.
    Bytes(u64),
}

/// The width attribute a `#[derive(Layout)]` field of `kind` needs.
pub fn width_attr(kind: &Kind, bit_addressed: bool) -> Option<WidthAttr> {
    match kind {
        Kind::Scalar(s) => bit_addressed.then(|| WidthAttr::Bits(s.bits())),
        Kind::Codec { bits, .. } if bit_addressed => Some(WidthAttr::Bits(*bits)),
        Kind::Codec { bits, .. } => Some(WidthAttr::Codec(*bits)),
        Kind::Nested { bytes, .. } | Kind::NestedArray { bytes, .. } => {
            Some(WidthAttr::Bytes(*bytes as u64))
        }
        Kind::Array(..)
        | Kind::Var { .. }
        | Kind::Msg { .. }
        | Kind::Text { .. }
        | Kind::Checksum { .. } => None,
    }
}

/// [`width_attr`], as an attribute.
fn field_attrs(kind: &Kind, bit_addressed: bool) -> TokenStream {
    let (key, n) = match width_attr(kind, bit_addressed) {
        None => return quote!(),
        Some(WidthAttr::Bits(n)) => (quote!(bits), n),
        Some(WidthAttr::Codec(n)) => (quote!(codec), n),
        Some(WidthAttr::Bytes(n)) => (quote!(bytes), n),
    };
    let n = proc_macro2::Literal::u64_unsuffixed(n);
    quote! { #[#key(#n)] }
}

/// `, endian = be` for a `#[layout(..)]` argument list, or nothing for little-endian.
pub fn endian_arg(endian: Endian) -> TokenStream {
    match endian {
        Endian::Le => quote!(),
        Endian::Be => quote!(, endian = be),
    }
}

fn stated_attr(stated: Option<Stated>) -> TokenStream {
    let Some(s) = stated else {
        return quote!();
    };
    let noun = ident(s.unit.name());
    let pos = proc_macro2::Literal::u64_unsuffixed(s.pos);
    quote! { #[at(#noun = #pos)] }
}

/// A field's value checks as attributes: `#[magic(..)]`, `#[range(..)]` and `#[checksum(..)]`.
fn assert_attr(f: &Field) -> TokenStream {
    let magic = f.magic.as_deref().map(|bytes| {
        let lit: TokenStream = magic_literal(bytes).parse().expect("a byte string is tokens");
        quote! { #[magic(#lit)] }
    });
    let range = f.range.map(|b| {
        let lo = b.lo.map(proc_macro2::Literal::i64_unsuffixed);
        let hi = b.hi.map(proc_macro2::Literal::i64_unsuffixed);
        quote! { #[range(#lo..=#hi)] }
    });
    let check = match &f.kind {
        Kind::Checksum { algorithm, over, .. } => {
            let algo = path(algorithm);
            let range = coverage_range(over);
            quote! { #[checksum(#algo, over = #range)] }
        }
        _ => quote!(),
    };
    quote! { #magic #range #check }
}

/// `bytes` as a byte-string literal for `#[magic(..)]`, either all text or all hex escapes.
fn magic_literal(bytes: &[u8]) -> String {
    let text = bytes.iter().all(|b| (0x20..=0x7E).contains(b));
    let mut out = String::from("b\"");
    for &byte in bytes {
        match byte {
            _ if !text => out.push_str(&format!("\\x{byte:02x}")),
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            other => out.push(other as char),
        }
    }
    out.push('"');
    out
}

fn coverage_range(over: &Coverage) -> TokenStream {
    let from = match &over.from {
        CoverFrom::Field(name) => {
            let f = ident(name);
            quote!(#f)
        }
        _ => quote!(),
    };
    match &over.to {
        CoverTo::Before(name) => {
            let t = ident(name);
            quote!(#from..#t)
        }
        CoverTo::After(name) => {
            let t = ident(name);
            quote!(#from..=#t)
        }
        _ => quote!(#from..),
    }
}

/// One field declaration, with its docs, width attribute, `#[at]` position and value checks.
pub fn emit_field(f: &Field, bit_addressed: bool, root: &Root) -> TokenStream {
    let Field { name, kind, doc, stated, magic: _, range: _ } = f;
    let name = ident(name);
    let doc = doc_attr(doc);
    let attrs = field_attrs(kind, bit_addressed);
    let at = stated_attr(*stated);
    let asserts = assert_attr(f);
    let ty = kind_ty(kind, root);
    quote! { #doc #attrs #at #asserts pub #name: #ty, }
}

/// A message's hidden `#[derive(Layout)]` block, checked by the derive like any layout.
pub(crate) fn hidden_block(
    root: &Root,
    name: &proc_macro2::Ident,
    len: &proc_macro2::Literal,
    endian: &TokenStream,
    fields: TokenStream,
) -> TokenStream {
    let crate_arg = root.crate_arg();
    quote! {
        #[derive(Debug, Clone, PartialEq, #root::Layout)]
        #[layout(bytes = #len #endian, internal #crate_arg)]
        #[doc(hidden)]
        pub struct #name {
            #fields
        }
    }
}

/// One field of a hidden block, without docs or an `#[at]` position.
///
/// `#[at]` counts from the start of the record, but a block can start partway through it.
pub(crate) fn block_field(f: &Field, root: &Root) -> TokenStream {
    let Field { name, kind, doc: _, stated: _, magic: _, range: _ } = f;
    let name = ident(name);
    let attrs = field_attrs(kind, false);
    let asserts = assert_attr(f);
    let ty = kind_ty(kind, root);
    quote! { #attrs #asserts pub #name: #ty, }
}

/// One field of a message struct, wrapped in `Option` when `optional`.
///
/// It has no width attribute. The hidden layout that reads the field sets its width.
pub fn emit_message_field(f: &Field, optional: bool, root: &Root) -> TokenStream {
    let Prelude { option, .. } = root.prelude();
    let Field { name, kind, doc, stated: _, magic: _, range: _ } = f;
    let name = ident(name);
    let doc = doc_attr(doc);
    let ty = kind_ty(kind, root);
    let ty = if optional { quote!(#option<#ty>) } else { ty };
    quote! { #doc pub #name: #ty, }
}

#[cfg(test)]
mod tests {
    use super::magic_literal;

    #[test]
    fn a_magic_literal_is_all_text_or_all_escapes() {
        assert_eq!(magic_literal(b"SEAL"), r#"b"SEAL""#);
        assert_eq!(magic_literal(b"LAYOUT  "), r#"b"LAYOUT  ""#);
        assert_eq!(magic_literal(&[0xEF, 0x49]), r#"b"\xef\x49""#);
        assert_eq!(magic_literal(&[0x55, 0xAA]), r#"b"\x55\xaa""#);
        assert_eq!(magic_literal(b"a\"b\\c"), r#"b"a\"b\\c""#);
    }
}
