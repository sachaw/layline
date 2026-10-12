//! `#[derive(FieldCodec)]` on a struct of `#[bits(N)]` fields: a bit group narrower than a layout.

use proc_macro2::{Literal, Span, TokenStream};
use quote::{quote, quote_spanned};

use layline_codegen::{BitOrder, Root};

use super::access::{fits_check, sign_extend};
use super::declared::reading_of;
use super::field::word_field;
use super::{Packed, Reading};

/// The keys a group takes after `#[bits(N, ..)]`.
pub struct GroupKeys {
    /// Encode keeps the low bits of a value wider than its field, rather than panicking.
    pub truncate: bool,
    /// With `msb`, the first field takes the most significant bits.
    pub order: BitOrder,
}

/// `FieldCodec` for a struct whose named fields each state `#[bits(N)]` and together cover `bits`.
pub fn group_codec(
    input: &syn::DeriveInput,
    named: &syn::FieldsNamed,
    bits: u32,
    bits_span: Span,
    keys: GroupKeys,
    root: &Root,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let mut fields = Vec::new();
    for f in &named.named {
        let field = word_field(f)?;
        let refused = |what: &str, span: Span| {
            Err(syn::Error::new(
                span,
                format!(
                    "field `{}`: {what} does not apply in a `FieldCodec` group. \
                     Declare the group as a `#[derive(Layout)]`",
                    field.ident
                ),
            ))
        };
        if let Some(ov) = field.overlays.first() {
            return refused("#[overlay]", ov.span);
        }
        if let Some(r) = &field.range {
            return refused("#[range]", r.span);
        }
        if let Some(at) = &field.at {
            return refused("#[at]", at.span);
        }
        fields.push(field);
    }
    if fields.is_empty() {
        return Err(syn::Error::new(
            ident.span(),
            format!("`{ident}`: a `FieldCodec` group needs at least one `#[bits(N)]` field"),
        ));
    }
    let covered: u32 = fields.iter().map(|f| f.bits).sum();
    if covered != bits {
        let fix = if covered < bits {
            format!("Declare the other {} bits as a field", bits - covered)
        } else {
            format!("Narrow the fields by {} bits, or widen the group", covered - bits)
        };
        return Err(syn::Error::new(
            bits_span,
            format!(
                "`{ident}`: the fields cover {covered} bits, but #[bits({bits})] states {bits}. {fix}"
            ),
        ));
    }

    let mut checks = Vec::new();
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut offset = 0u32;
    for f in fields {
        let start = match keys.order {
            BitOrder::Lsb => offset,
            BitOrder::Msb => bits - offset - f.bits,
        };
        offset += f.bits;
        let p =
            Packed { ty: f.ty, reading: reading_of(&f.kind), bits: f.bits, bits_span: f.bits_span };
        let (name, ty, width) = (&f.ident, &p.ty, p.bits);
        let mask = (width < 64).then(|| Literal::u64_suffixed((1u64 << width) - 1));
        let start_lit = Literal::u32_unsuffixed(start);

        let mut raw = quote!(raw);
        if start != 0 {
            raw = quote! { (#raw >> #start_lit) };
        }
        if let Some(mask) = &mask
            && start + width < 64
        {
            raw = quote! { (#raw & #mask) };
        }
        let read = match p.reading {
            Reading::Uint(64) => raw,
            Reading::Uint(_) => quote! { #raw as #ty },
            Reading::Int => sign_extend(raw, width, ty),
            Reading::Bool | Reading::Codec => quote! { <#ty as #root::FieldCodec>::from_raw(#raw) },
            Reading::Native(_) => unreachable!("a bit field is never native"),
        };
        reads.push(quote! { #name: #read, });

        let value = quote! { self.#name };
        if !keys.truncate {
            writes.push(fits_check(&p, &value, name, root));
        }
        let raw = match p.reading {
            Reading::Uint(64) => quote! { #value },
            Reading::Uint(_) => quote! { #value as ::core::primitive::u64 },
            Reading::Int => quote! { (#value as ::core::primitive::i64) as ::core::primitive::u64 },
            Reading::Bool | Reading::Codec => {
                quote! { <#ty as #root::FieldCodec>::to_raw(&#value) }
            }
            Reading::Native(_) => unreachable!("a bit field is never native"),
        };
        let masked = match &mask {
            Some(mask) => quote! { ((#raw) & #mask) },
            None => quote! { (#raw) },
        };
        writes.push(if start == 0 {
            quote! { __bits |= #masked; }
        } else {
            quote! { __bits |= #masked << #start_lit; }
        });

        if p.reading == Reading::Codec {
            let msg = crate::check::assert_msg(format!(
                "field `{name}`: #[bits({width})] disagrees with `<{} as FieldCodec>::BITS`",
                quote!(#ty),
            ));
            let spanned = layline_codegen::__derive::respan(root, p.bits_span);
            checks.push(quote_spanned! {p.bits_span=>
                const _: () = ::core::assert!(<#ty as #spanned::FieldCodec>::BITS == #width, #msg);
            });
        }
    }

    let reg = match bits {
        0..=8 => "u8",
        9..=16 => "u16",
        17..=32 => "u32",
        _ => "u64",
    };
    let word = syn::Ident::new(reg, Span::call_site());
    let (widen, narrow) = if reg == "u64" {
        (quote!(bits), quote!(<Self as #root::FieldCodec>::to_raw(self)))
    } else {
        (
            quote!(::core::primitive::u64::from(bits)),
            quote!(<Self as #root::FieldCodec>::to_raw(self) as ::core::primitive::#word),
        )
    };
    let from_doc = format!(
        "Decode from the low {bits} bits of `bits`. Higher bits are not part of the group, and \
         are ignored."
    );
    let to_doc = format!("The group's {bits} bits, as `FieldCodec::to_raw` gives them.");
    let truncates = if keys.truncate {
        quote!()
    } else {
        quote! {
            ///
            /// # Panics
            ///
            /// If a field's value does not fit its width.
        }
    };
    let words = quote! {
        #[automatically_derived]
        impl #ident {
            #[doc = #from_doc]
            #[inline]
            #[must_use]
            pub fn from_bits(bits: ::core::primitive::#word) -> Self {
                <Self as #root::FieldCodec>::from_raw(#widen)
            }

            #[doc = #to_doc]
            #truncates
            #[inline]
            #[must_use]
            pub fn to_bits(&self) -> ::core::primitive::#word {
                #narrow
            }
        }
    };

    Ok(quote! {
        #words
        #(#checks)*

        #[automatically_derived]
        impl #root::FieldCodec for #ident {
            const BITS: ::core::primitive::u32 = #bits;

            #[inline]
            fn from_raw(raw: ::core::primitive::u64) -> Self {
                Self { #(#reads)* }
            }

            #[inline]
            fn to_raw(&self) -> ::core::primitive::u64 {
                let mut __bits: ::core::primitive::u64 = 0;
                #(#writes)*
                __bits
            }
        }
    })
}
