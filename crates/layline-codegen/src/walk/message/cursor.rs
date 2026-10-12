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

/// As [`windowed`] with a stated length, over `__n` rounded up to a multiple of `align`.
///
/// `inner` may end anywhere from `__n` to that bound, since a record that states its own size can
/// count the padding. The cursor moves to the bound.
pub(super) fn windowed_padded(
    field: &str,
    align: u64,
    inner: TokenStream,
    root: &Root,
) -> TokenStream {
    let Prelude { ok, err, result, u8, .. } = root.prelude();
    let malformed =
        |at: TokenStream| quote!(#root::ParseError::Malformed { field: #field, at: #at });
    let (overrun, short) = (malformed(quote!(__at)), malformed(quote!(0)));
    let align = Literal::usize_unsuffixed(align as usize);
    quote! {
        let __least = __n;
        let __padded = __n.checked_add((#align - __n % #align) % #align).ok_or(#overrun)?;
        let __end = __at.checked_add(__padded).ok_or(#overrun)?;
        let __window = __body.get(__at..__end).ok_or(#root::ParseError::Short {
            need_bytes: __end,
            got_bytes: __body.len(),
            at: __at,
        })?;
        let __walk = |__body: &[#u8]| -> #result<_, #root::ParseError> {
            let mut __at = 0usize;
            #inner
            if __at < __least {
                return #err(#short);
            }
            #ok(__e)
        };
        let __e = __walk(__window).map_err(|__err| match __err {
            #root::ParseError::Short { .. } => #overrun,
            __err => __err.rebased(__at),
        })?;
        __at = __end;
    }
}

/// Skip the padding after a value that began at `__pad_from`, up to the next multiple of `align`
/// bytes from there.
pub(super) fn skip_padding(align: u64, root: &Root) -> TokenStream {
    let Prelude { err, .. } = root.prelude();
    let align = Literal::usize_unsuffixed(align as usize);
    quote! {
        {
            let __to = __at + (#align - (__at - __pad_from) % #align) % #align;
            if __to > __body.len() {
                return #err(#root::ParseError::Short {
                    need_bytes: __to,
                    got_bytes: __body.len(),
                    at: __at,
                });
            }
            __at = __to;
        }
    }
}

/// Write zeros after a value that began at `out` position `__pad_from`, up to the next multiple of
/// `align` bytes from there.
///
/// Up to 64, the zeros are one push from a constant; past that, one push per byte.
pub(super) fn write_padding(align: u64) -> TokenStream {
    let n = Literal::usize_unsuffixed(align as usize);
    let pad = quote! { (#n - (out.len() - __pad_from) % #n) % #n };
    if align <= 64 {
        let most = Literal::usize_unsuffixed(align as usize - 1);
        quote! {
            {
                const __ZEROS: [::core::primitive::u8; #most] = [0; #most];
                if let ::core::option::Option::Some(__z) = __ZEROS.get(..#pad) {
                    out.push(__z)?;
                }
            }
        }
    } else {
        quote! {
            for _ in 0..#pad {
                out.push(&[0u8])?;
            }
        }
    }
}
