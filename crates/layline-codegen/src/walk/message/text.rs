//! Text fields: codec lookup, decode and encode.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use super::prefix::Prefix;
use crate::root::Prelude;
use crate::walk::path;
use crate::{Len, Root};

/// Strip the zero padding from the end of a fixed run of text.
pub(super) fn text_unpad() -> TokenStream {
    quote! {
        let __s = &__s[..__s.iter().rposition(|__b| *__b != 0).map_or(0, |__i| __i + 1)];
    }
}

/// The `TextCodec` a text field names, or UTF-8 for a bare `#[text]`.
fn text_codec(codec: &Option<String>, root: &Root) -> TokenStream {
    match codec {
        Some(ty) => {
            let p = path(ty);
            quote!(#p)
        }
        None => quote!(#root::__private::Utf8),
    }
}

/// Decode the text in `__s`. Bytes the codec rejects are `Malformed`.
pub(super) fn text_decode(codec: &Option<String>, what: &str, root: &Root) -> TokenStream {
    let Prelude { some, none, err, .. } = root.prelude();
    let codec = text_codec(codec, root);
    quote! {
        match <#codec as #root::__private::TextCodec>::decode(__s) {
            #some(__t) => __t,
            #none => return #err(#root::ParseError::Malformed { field: #what, at: __at }),
        }
    }
}

/// The number of bytes `text` encodes to.
pub(super) fn text_wire_len(codec: &Option<String>, text: TokenStream, root: &Root) -> TokenStream {
    let codec = text_codec(codec, root);
    quote!(<#codec as #root::__private::TextCodec>::encoded_len(#text))
}

/// Encode text, refusing text that would not decode back.
pub(super) fn text_write(
    (field, value): (&str, TokenStream),
    len: &Len,
    codec: &Option<String>,
    root: &Root,
) -> TokenStream {
    let Prelude { some, err, .. } = root.prelude();
    let codec = text_codec(codec, root);
    let (refuse, close) = match len {
        Len::Until { terminator } => {
            let term = Literal::u8_unsuffixed(*terminator);
            (
                quote! {
                    if __text.contains(&#term) {
                        return #err(#root::EncodeError::Delimiter { field: #field, byte: #term });
                    }
                },
                quote! { out.push(&[#term])?; },
            )
        }
        Len::Bytes(n) => {
            let lit = Literal::usize_unsuffixed(*n);
            (
                quote! {
                    #root::EncodeError::cap(#field, __text.len(), #lit)?;
                    if __text.last() == #some(&0) {
                        return #err(#root::EncodeError::Delimiter { field: #field, byte: 0 });
                    }
                },
                quote! { out.pad(__mark + #lit - out.len())?; },
            )
        }
        Len::Field { .. } | Len::Fill | Len::Prefix { .. } => (quote!(), quote!()),
    };
    let (reserve, stamp) = match Prefix::of_len(len) {
        Some(p) => p.reserve(field, root),
        None => (quote!(), quote!()),
    };
    quote! {
        {
            #reserve
            let __mark = out.len();
            for __c in #value.chars() {
                <#codec as #root::__private::TextCodec>::encode_char(__c, &mut *out)?;
            }
            let __text = &out.written()[__mark..];
            #refuse
            #close
            #stamp
        }
    }
}
