//! Placeholder impls emitted beside a failed derive's `compile_error!`.
//!
//! They stop later uses of the type adding errors such as "no function named `decode`".
//! Generic types get no stub, because guessing their parameters would add another error.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput};

pub fn layout(input: &DeriveInput) -> TokenStream {
    if !input.generics.params.is_empty() {
        return TokenStream::new();
    }
    let root = crate::root::of(input);
    let ident = &input.ident;
    quote! {
        #[automatically_derived]
        impl #root::Layout for #ident {
            const NAME: &'static ::core::primitive::str = "";
            const WIRE_BYTES: ::core::primitive::usize = 0;
            const FIELDS: &'static [#root::table::FieldDef<'static>] = &[];
            fn decode_slice(
                _: &[::core::primitive::u8],
            ) -> ::core::result::Result<Self, #root::ParseError> {
                ::core::unimplemented!()
            }
            fn encode_into<__B: #root::Buffer>(
                &self,
                _: &mut __B,
            ) -> ::core::result::Result<(), #root::Overflow> {
                ::core::unimplemented!()
            }
        }
    }
}

/// `impl Message` for a struct, `impl Choice` for an enum.
pub fn message(input: &DeriveInput) -> TokenStream {
    if !input.generics.params.is_empty() {
        return TokenStream::new();
    }
    let root = crate::root::of(input);
    let ident = &input.ident;
    match &input.data {
        Data::Enum(_) => quote! {
            #[automatically_derived]
            impl #root::Choice for #ident {
                const NAME: &'static ::core::primitive::str = "";
                const ARMS: &'static [#root::table::ArmDef<'static>] = &[];
                fn decode_with_nested(
                    _: ::core::primitive::i64,
                    _: &[::core::primitive::u8],
                    _: ::core::primitive::u32,
                ) -> ::core::result::Result<(Self, ::core::primitive::usize), #root::ParseError>
                {
                    ::core::unimplemented!()
                }
                fn discriminant(&self) -> ::core::option::Option<::core::primitive::i64> {
                    ::core::unimplemented!()
                }
                fn encode_into<__B: #root::Buffer>(
                    &self,
                    _: &mut __B,
                ) -> ::core::result::Result<(), #root::Overflow> {
                    ::core::unimplemented!()
                }
            }
        },
        _ => quote! {
            #[automatically_derived]
            impl #root::Message for #ident {
                const NAME: &'static ::core::primitive::str = "";
                type Ctx = ();
                const SEGMENTS: &'static [#root::table::SegmentDef<'static>] = &[];
                fn decode_with_nested(
                    _: &[::core::primitive::u8],
                    _: ::core::primitive::u32,
                    _: (),
                ) -> ::core::result::Result<(Self, ::core::primitive::usize), #root::ParseError>
                {
                    ::core::unimplemented!()
                }
                fn encode_into_with<__B: #root::Buffer>(
                    &self,
                    _: &mut __B,
                    _: (),
                ) -> ::core::result::Result<(), #root::Overflow> {
                    ::core::unimplemented!()
                }
            }
        },
    }
}

pub fn field_codec(input: &DeriveInput) -> TokenStream {
    if !input.generics.params.is_empty() {
        return TokenStream::new();
    }
    let root = crate::root::of(input);
    let ident = &input.ident;
    quote! {
        #[automatically_derived]
        impl #root::FieldCodec for #ident {
            const BITS: ::core::primitive::u32 = 1;
            fn from_raw(_: ::core::primitive::u64) -> Self {
                ::core::unimplemented!()
            }
            fn to_raw(&self) -> ::core::primitive::u64 {
                ::core::unimplemented!()
            }
        }
    }
}

/// The id type comes from `#[dispatch(id = ..)]`, or defaults to `u8`.
pub fn dispatch(input: &DeriveInput) -> TokenStream {
    if input.generics.params.iter().any(|p| !matches!(p, syn::GenericParam::Lifetime(_))) {
        return TokenStream::new();
    }
    let root = crate::root::of(input);
    let ident = &input.ident;
    let mut id_ty: syn::Type = syn::parse_quote!(::core::primitive::u8);
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("dispatch")) {
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                id_ty = meta.value()?.parse()?;
            } else if meta.input.peek(syn::Token![=]) {
                let _: TokenStream = meta.value()?.parse()?;
            }
            Ok(())
        });
    }
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let (body_lt, dispatch_generics) = crate::dispatch::body_lifetime(&input.generics);
    quote! {
        #[automatically_derived]
        impl #dispatch_generics #root::Dispatch<#body_lt> for #ident #ty_generics #where_clause {
            type Id = #id_ty;
            fn decode(_: #id_ty, _: &#body_lt [::core::primitive::u8]) -> Self {
                ::core::unimplemented!()
            }
            fn id(&self) -> #id_ty {
                ::core::unimplemented!()
            }
            fn unlisted(&self) -> ::core::option::Option<#id_ty> {
                ::core::unimplemented!()
            }
            fn encode_into<__B: #root::Buffer>(
                &self,
                _: &mut __B,
            ) -> ::core::result::Result<(), #root::Overflow> {
                ::core::unimplemented!()
            }
        }
    }
}
