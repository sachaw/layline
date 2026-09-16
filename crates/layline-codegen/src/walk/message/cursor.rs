//! Decode's cursor: reads, seeks, windows and their bounds checks.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;

use crate::Root;
use crate::root::Prelude;

/// Refuse a message nested past `MAX_NESTING_DEPTH`.
pub(super) fn too_deep(root: &Root) -> TokenStream {
    let Prelude { err, .. } = root.prelude();
    quote! {
        if __depth > #root::MAX_NESTING_DEPTH {
            return #err(#root::ParseError::TooDeep {
                limit: #root::MAX_NESTING_DEPTH,
            });
        }
    }
}

/// Move the cursor to `start`, the offset read from `field`.
pub(super) fn seek_to(start: &TokenStream, field: &str, root: &Root) -> TokenStream {
    let Prelude { err, usize, .. } = root.prelude();
    quote! {
        let __to = #usize::try_from(#start)
            .map_err(|_| #root::ParseError::Malformed { field: #field, at: __at })?;
        if __to > __body.len() {
            return #err(#root::ParseError::Short { need_bytes: __to, got_bytes: __body.len(), at: __at });
        }
        __at = __to;
    }
}

/// Refuse an `__n` read from the wire that is above its cap, if it has one.
pub(super) fn cap_guard(cap: Option<usize>, field: &str, root: &Root) -> TokenStream {
    let Prelude { err, .. } = root.prelude();
    match cap.map(Literal::usize_unsuffixed) {
        Some(c) => quote! {
            if __n > #c {
                return #err(#root::ParseError::Malformed { field: #field, at: __at });
            }
        },
        None => quote!(),
    }
}

/// Refuse a self-delimiting read that consumed no bytes.
pub(super) fn consumed_nothing(what: &str, root: &Root) -> TokenStream {
    let Prelude { err, .. } = root.prelude();
    quote! {
        if __used == 0 {
            return #err(#root::ParseError::Malformed { field: #what, at: __at });
        }
    }
}

/// The next `len` bytes at the cursor, or `Short`.
pub(super) fn take(len: &TokenStream, root: &Root) -> TokenStream {
    quote! {
        __body.get(__at..__at + #len).ok_or(#root::ParseError::Short {
            need_bytes: __at + #len,
            got_bytes: __body.len(),
            at: __at,
        })?
    }
}

/// `let #bind = ..; __at += #len;`: read one hidden layout at the cursor.
pub(super) fn read_layout(bind: &Ident, sub: &Ident, len: &Literal, root: &Root) -> TokenStream {
    let bytes = take(&quote!(#len), root);
    quote! {
        let #bind = <#sub as #root::Layout>::decode_slice(#bytes)
            .map_err(|__err| __err.rebased(__at))?;
        __at += #len;
    }
}

/// `let __e = ..; __at = __end;`: run `inner` over the next `__n` bytes as if they were the body.
///
/// `inner` binds `__e` and advances `__at`. If it does not consume the window exactly, decode fails
/// `Malformed` on `field`. When `stated` (the length came from a field), a `Short` inside the
/// window is also `Malformed`, because the length was wrong.
pub(super) fn windowed(field: &str, stated: bool, inner: TokenStream, root: &Root) -> TokenStream {
    let Prelude { ok, err, result, u8, .. } = root.prelude();
    let malformed =
        |at: TokenStream| quote!(#root::ParseError::Malformed { field: #field, at: #at });
    let (overrun, filled) = (malformed(quote!(__at)), malformed(quote!(0)));
    let rebase = if stated {
        quote! {
            |__err| match __err {
                #root::ParseError::Short { .. } => #overrun,
                __err => __err.rebased(__at),
            }
        }
    } else {
        quote!(|__err| __err.rebased(__at))
    };
    quote! {
        let __end = __at.checked_add(__n).ok_or(#overrun)?;
        let __window = __body.get(__at..__end).ok_or(#root::ParseError::Short {
            need_bytes: __end,
            got_bytes: __body.len(),
            at: __at,
        })?;
        let __walk = |__body: &[#u8]| -> #result<_, #root::ParseError> {
            let mut __at = 0usize;
            #inner
            if __at != __body.len() {
                return #err(#filled);
            }
            #ok(__e)
        };
        let __e = __walk(__window).map_err(#rebase)?;
        __at = __end;
    }
}
