//! `#[derive(Dispatch)]` for enums of `Layout` payloads keyed by id, with an `#[other]` catch-all.
use layline_codegen::Root;
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, LitInt};

use crate::attr::{Key, noted, variant_key};

struct Catalog<'a> {
    ident: &'a syn::Ident,
    generics: &'a syn::Generics,
    root: Root,
    id_ty: syn::Type,
    /// `prefix = K`. The id sits in the low `K` bits of every payload.
    prefix_bits: Option<u32>,
    variants: Vec<Variant<'a>>,
    unknown: &'a syn::Ident,
}

struct Variant<'a> {
    ident: &'a syn::Ident,
    ty: &'a syn::Type,
    id: LitInt,
}

pub fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    let catalog = parse(input)?;
    Ok(expand(&catalog))
}

/// Must mirror the `attributes(..)` list on `derive_dispatch` in `lib.rs`.
const DISPATCH_ATTRS: &[&str] = &["dispatch", "value", "other"];

fn refuse_misplaced(input: &DeriveInput, data: &syn::DataEnum) -> syn::Result<()> {
    for attr in &input.attrs {
        if let Some(name) = crate::attr::registered(attr, DISPATCH_ATTRS)
            && name != "dispatch"
        {
            return Err(syn::Error::new(
                attr.meta.span(),
                format!(
                    "`{}`: `#[{name}]` belongs on a variant. \
                     Only `#[dispatch(id = ..)]` goes above the enum",
                    input.ident,
                ),
            ));
        }
    }
    for v in &data.variants {
        if let Some(attr) = v.attrs.iter().find(|a| a.path().is_ident("dispatch")) {
            return Err(syn::Error::new(
                attr.meta.span(),
                format!("variant `{}`: `#[dispatch(..)]` belongs above the enum", v.ident,),
            ));
        }
        for f in &v.fields {
            if let Some(attr) =
                f.attrs.iter().find(|a| crate::attr::registered(a, DISPATCH_ATTRS).is_some())
            {
                let name = crate::attr::registered(attr, DISPATCH_ATTRS).expect("just found");
                return Err(syn::Error::new(
                    attr.meta.span(),
                    format!(
                        "variant `{}`: `#[{name}]` is ignored on a payload. Put it on the \
                         variant, or `#[dispatch]` above the enum",
                        v.ident,
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn parse(input: &DeriveInput) -> syn::Result<Catalog<'_>> {
    for param in &input.generics.params {
        if !matches!(param, syn::GenericParam::Lifetime(_)) {
            return Err(syn::Error::new(
                param.span(),
                format!(
                    "`{}`: #[derive(Dispatch)] supports only lifetime parameters. \
                     Remove the type parameters",
                    input.ident
                ),
            ));
        }
    }

    let ident = &input.ident;
    let attr = crate::attr::single(&input.attrs, "dispatch", format_args!("`{ident}`"))?
        .ok_or_else(|| {
            syn::Error::new(
                ident.span(),
                format!(
                    "`{ident}`: missing #[dispatch(id = <int type>)]. \
                     Add it with the integer type of the id"
                ),
            )
        })?;
    let mut id_ty: Option<syn::Type> = None;
    let mut prefix_bits: Option<u32> = None;
    let mut root = None;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("id") {
            crate::attr::once_key(&mut id_ty, &meta, ident, "dispatch", || meta.value()?.parse())
        } else if meta.path.is_ident("prefix") {
            crate::attr::once_key(&mut prefix_bits, &meta, ident, "dispatch", || {
                crate::attr::int_value(&meta)
            })
        } else if meta.path.is_ident("crate") {
            crate::attr::once_key(&mut root, &meta, ident, "dispatch", || {
                crate::root::parse_arg(&meta)
            })
        } else {
            Err(meta.error(format!(
                "`{ident}`: expected `id = <int type>`, `prefix = K`, or `crate = <path>`"
            )))
        }
    })?;
    let id_ty = id_ty.ok_or_else(|| {
        syn::Error::new(
            attr.meta.span(),
            format!("`{ident}`: #[dispatch] is missing `id = <int type>`"),
        )
    })?;

    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            format!(
                "`{}`: #[derive(Dispatch)] needs an enum. \
                 Declare one `#[value(N)]` variant per message and one `#[other]`",
                input.ident
            ),
        ));
    };
    refuse_misplaced(input, data)?;

    let mut variants = Vec::new();
    let mut unknown: Option<&syn::Ident> = None;
    let mut seen: Vec<(u128, proc_macro2::Span)> = Vec::new();

    for v in &data.variants {
        let id = match variant_key(v, "id", "the frames this catalogue does not list")? {
            Key::Value(id) => id,
            Key::Other(attr) => {
                if let Some(first) = unknown {
                    return Err(noted(
                        attr.meta.span(),
                        format!(
                            "variant `{}`: a second #[other]. Keep one #[other] variant",
                            v.ident
                        ),
                        first.span(),
                        "first declared here",
                    ));
                }
                let Fields::Named(named) = &v.fields else {
                    return Err(syn::Error::new(
                        v.ident.span(),
                        format!(
                            "variant `{0}`: #[other] needs the named fields `id` and `body`. \
                             Write `#[other] {0} {{ id: <int type>, body: Vec<u8> }}`",
                            v.ident
                        ),
                    ));
                };
                let mut names: Vec<String> =
                    named.named.iter().map(|f| f.ident.as_ref().unwrap().to_string()).collect();
                names.sort();
                if names != ["body", "id"] {
                    return Err(syn::Error::new(
                        v.ident.span(),
                        format!(
                            "variant `{0}`: #[other] needs exactly the fields `id` and `body`. \
                             Write `#[other] {0} {{ id: <int type>, body: Vec<u8> }}`",
                            v.ident
                        ),
                    ));
                }
                unknown = Some(&v.ident);
                continue;
            }
        };
        let value: u128 = id.base10_parse()?;
        if let Some(k) = prefix_bits
            && k < 128
            && value >> k != 0
        {
            return Err(syn::Error::new(
                id.span(),
                format!(
                    "variant `{}`: #[value({value})] does not fit #[dispatch(prefix = {k})]. \
                     The largest id is {}",
                    v.ident,
                    (1u128 << k) - 1,
                ),
            ));
        }
        if let Some((_, prev)) = seen.iter().find(|(v, _)| *v == value) {
            return Err(noted(
                id.span(),
                format!(
                    "variant `{}`: #[value({value})] is already used by another variant",
                    v.ident
                ),
                *prev,
                "first declared here",
            ));
        }
        seen.push((value, id.span()));

        let one_payload = || {
            syn::Error::new(
                v.ident.span(),
                format!(
                    "variant `{0}`: needs one `Layout` payload. \
                     Write `#[value(N)] {0}(Payload)`",
                    v.ident
                ),
            )
        };
        let Fields::Unnamed(fields) = &v.fields else {
            return Err(one_payload());
        };
        if fields.unnamed.len() != 1 {
            return Err(one_payload());
        }

        variants.push(Variant { ident: &v.ident, ty: &fields.unnamed[0].ty, id });
    }

    let unknown = unknown.ok_or_else(|| {
        syn::Error::new(
            input.ident.span(),
            format!(
                "`{}`: no variant catches unlisted ids. \
                 Add `#[other] Unknown {{ id: <int type>, body: Vec<u8> }}`",
                input.ident
            ),
        )
    })?;

    Ok(Catalog {
        ident: &input.ident,
        generics: &input.generics,
        root: root.unwrap_or_default(),
        id_ty,
        prefix_bits,
        variants,
        unknown,
    })
}

/// The lifetime of `Dispatch<'a>`, and the impl's generics with it.
///
/// An enum without a lifetime implements `Dispatch<'a>` for every `'a`.
pub(crate) fn body_lifetime(generics: &syn::Generics) -> (TokenStream, TokenStream) {
    if let Some(lt) = generics.lifetimes().next() {
        let lt = &lt.lifetime;
        let (impl_generics, _, _) = generics.split_for_impl();
        return (quote!(#lt), quote!(#impl_generics));
    }
    let mut generics = generics.clone();
    generics.params.insert(0, syn::parse_quote!('__layline));
    let (with_lt, _, _) = generics.split_for_impl();
    (quote!('__layline), quote!(#with_lt))
}

fn expand(catalog: &Catalog<'_>) -> TokenStream {
    let root = &catalog.root;
    let ident = catalog.ident;
    let id_ty = &catalog.id_ty;
    let unknown = catalog.unknown;
    let (impl_generics, ty_generics, where_clause) = catalog.generics.split_for_impl();

    let (body_lt, dispatch_generics) = body_lifetime(catalog.generics);

    let decode_arms = catalog.variants.iter().map(|v| {
        let vid = v.ident;
        let ty = v.ty;
        let id = &v.id;
        quote! {
            #id => match <#ty as #root::Layout>::decode_slice(body) {
                ::core::result::Result::Ok(payload) => Self::#vid(payload),
                ::core::result::Result::Err(_) => Self::#unknown {
                    id,
                    body: ::core::convert::From::from(body),
                },
            },
        }
    });

    let id_arms = catalog.variants.iter().map(|v| {
        let vid = v.ident;
        let id = &v.id;
        quote! { Self::#vid(_) => #id, }
    });

    let encode_arms = catalog.variants.iter().map(|v| {
        let vid = v.ident;
        let ty = v.ty;
        quote! {
            Self::#vid(payload) => <#ty as #root::Layout>::encode_into(payload, out),
        }
    });

    // A payload type used by two variants gets neither `From` nor `Slot`.
    // Types compare by last path segment, since `Sample` and `self::Sample` may be the same type.
    let type_tokens: Vec<String> = catalog
        .variants
        .iter()
        .map(|v| match v.ty {
            syn::Type::Path(p) if p.qself.is_none() => {
                crate::attr::spelled(p.path.segments.last().expect("a path has a segment"))
            }
            ty => crate::attr::spelled(ty),
        })
        .collect();
    let held_by_one_variant =
        |i: usize| type_tokens.iter().filter(|t| **t == type_tokens[i]).count() == 1;
    let slot_impls = catalog.variants.iter().enumerate().filter_map(|(i, v)| {
        if !held_by_one_variant(i) {
            return None;
        }
        let vid = v.ident;
        let ty = v.ty;
        Some(quote! {
            #[automatically_derived]
            impl #impl_generics ::core::convert::From<#ty> for #ident #ty_generics #where_clause {
                fn from(payload: #ty) -> Self {
                    Self::#vid(payload)
                }
            }

            #[automatically_derived]
            impl #impl_generics #root::Slot<#ty> for #ident #ty_generics #where_clause {
                fn slot(&self) -> ::core::option::Option<&#ty> {
                    match self {
                        Self::#vid(payload) => ::core::option::Option::Some(payload),
                        _ => ::core::option::Option::None,
                    }
                }
            }
        })
    });

    let prefix_agreement_checks = catalog.prefix_bits.map(|bits| {
        let checks = catalog.variants.iter().map(|v| {
            let ty = v.ty;
            let msg = crate::check::assert_msg(format!(
                "variant `{}`: `{}` does not match `#[dispatch(prefix = {bits})]`. \
                 Write `#[layout(.., prefix = {bits})]` on the payload",
                v.ident,
                crate::attr::spelled(ty),
            ));
            let k = proc_macro2::Literal::u32_unsuffixed(bits);
            let span = v.ident.span();
            let root = layline_codegen::__derive::respan(root, span);
            quote_spanned! {span=>
                const _: () = ::core::assert!(<#ty as #root::Layout>::PREFIX_BITS == #k, #msg);
            }
        });
        quote! { #(#checks)* }
    });

    let prefix_bits = catalog.prefix_bits.map(|bits| {
        let k = proc_macro2::Literal::u32_unsuffixed(bits);
        quote! { const PREFIX_BITS: ::core::primitive::u32 = #k; }
    });

    quote! {
        #[automatically_derived]
        impl #dispatch_generics #root::Dispatch<#body_lt> for #ident #ty_generics #where_clause {
            type Id = #id_ty;

            #prefix_bits

            fn decode(id: #id_ty, body: &#body_lt [::core::primitive::u8]) -> Self {
                match id {
                    #(#decode_arms)*
                    _ => Self::#unknown {
                        id,
                        body: ::core::convert::From::from(body),
                    },
                }
            }

            fn id(&self) -> #id_ty {
                match self {
                    #(#id_arms)*
                    Self::#unknown { id, .. } => *id,
                }
            }

            fn unlisted(&self) -> ::core::option::Option<#id_ty> {
                if let Self::#unknown { id, .. } = self {
                    ::core::option::Option::Some(*id)
                } else {
                    ::core::option::Option::None
                }
            }

            fn encode_into<__B: #root::Buffer>(
                &self,
                out: &mut __B,
            ) -> ::core::result::Result<(), #root::Overflow> {
                match self {
                    #(#encode_arms)*
                    Self::#unknown { body, .. } => {
                        let bytes: &[::core::primitive::u8] = body;
                        #root::Buffer::push(out, bytes)
                    }
                }
            }
        }

        #(#slot_impls)*

        #prefix_agreement_checks
    }
}
