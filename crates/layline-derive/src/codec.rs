//! `#[derive(FieldCodec)]` for fixed-width newtypes and enums.
use layline_codegen::{Root, Scalar, Spelling};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{Data, DataEnum, DataStruct, DeriveInput, Fields, LitInt, Token, Type};

use crate::attr::{Key, noted, variant_key};
use crate::ty::scalar_of;

pub fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            format!(
                "`{}`: #[derive(FieldCodec)] does not support generics. \
                 Remove the type parameters",
                input.ident
            ),
        ));
    }

    let (width, root) = bits_attr(input)?;
    refuse_misplaced(input)?;

    match &input.data {
        Data::Struct(data) => newtype(input, data, width, &root),
        Data::Enum(data) => enum_codec(input, data, width, &root),
        Data::Union(_) => Err(syn::Error::new(
            input.ident.span(),
            format!("`{}`: #[derive(FieldCodec)] needs a newtype struct or an enum", input.ident),
        )),
    }
}

/// Must mirror the `attributes(..)` list on `derive_field_codec` in `lib.rs`.
const CODEC_ATTRS: &[&str] = &["bits", "value", "other"];

fn refuse_misplaced(input: &DeriveInput) -> syn::Result<()> {
    let ident = &input.ident;
    for attr in &input.attrs {
        if let Some(name) = crate::attr::registered(attr, CODEC_ATTRS)
            && name != "bits"
        {
            let msg = match &input.data {
                Data::Enum(_) => format!(
                    "`{ident}`: `#[{name}]` belongs on a variant. \
                     Only `#[bits(N)]` goes above the enum"
                ),
                _ => format!(
                    "`{ident}`: `#[{name}]` belongs on an enum variant, not a newtype. \
                     Write `#[bits(N)]` above the newtype"
                ),
            };
            return Err(syn::Error::new(attr.meta.span(), msg));
        }
    }
    let on_field = |f: &syn::Field, owner: String| -> syn::Result<()> {
        let Some(attr) = f.attrs.iter().find(|a| crate::attr::registered(a, CODEC_ATTRS).is_some())
        else {
            return Ok(());
        };
        let name = crate::attr::registered(attr, CODEC_ATTRS).expect("just found");
        let msg = match name {
            "bits" => format!(
                "{owner}: `#[bits(N)]` sets the width of the whole type. Write it above `{ident}`"
            ),
            other => format!("{owner}: `#[{other}]` belongs on a variant, not a field"),
        };
        Err(syn::Error::new(attr.meta.span(), msg))
    };
    match &input.data {
        Data::Struct(data) => {
            for f in &data.fields {
                on_field(f, format!("`{ident}`'s field"))?;
            }
        }
        Data::Enum(data) => {
            for v in &data.variants {
                if let Some(attr) = v.attrs.iter().find(|a| a.path().is_ident("bits")) {
                    return Err(syn::Error::new(
                        attr.meta.span(),
                        format!(
                            "variant `{}`: `#[bits(N)]` sets the enum's width. \
                             Write it above the enum",
                            v.ident,
                        ),
                    ));
                }
                for f in &v.fields {
                    on_field(f, format!("variant `{}`", v.ident))?;
                }
            }
        }
        Data::Union(_) => {}
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Width {
    bits: u32,
    span: Span,
}

impl Width {
    fn max(self) -> u64 {
        u64::MAX >> (64 - self.bits)
    }

    fn cardinality(self) -> u128 {
        1u128 << self.bits
    }

    fn narrowest_primitive(self) -> &'static str {
        match self.bits {
            0..=8 => "u8",
            9..=16 => "u16",
            17..=32 => "u32",
            _ => "u64",
        }
    }

    fn mask(self, raw: TokenStream) -> TokenStream {
        if self.bits == 64 {
            return raw;
        }
        let mask = LitInt::new(&format!("{}u64", self.max()), Span::call_site());
        quote!(#raw & #mask)
    }
}

struct BitsAttr {
    bits: LitInt,
    root: Root,
}

impl Parse for BitsAttr {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let bits: LitInt = input.parse()?;
        let mut root = Root::default();
        if !input.is_empty() {
            input.parse::<Token![,]>()?;
            if !input.is_empty() {
                input.parse::<Token![crate]>().map_err(|_| {
                    input.error("expected `#[bits(N)]` or `#[bits(N, crate = <path>)]`")
                })?;
                input.parse::<Token![=]>()?;
                root = Root::new(input.parse()?, Spelling::Hygienic);
                if !input.is_empty() {
                    return Err(input.error("expected `#[bits(N, crate = <path>)]` and no more"));
                }
            }
        }
        Ok(Self { bits, root })
    }
}

fn bits_attr(input: &DeriveInput) -> syn::Result<(Width, Root)> {
    let owner = format!("`{}`", input.ident);
    let attr = crate::attr::single(&input.attrs, "bits", owner)?.ok_or_else(|| {
        syn::Error::new(
            input.ident.span(),
            format!(
                "`{}`: missing #[bits(N)]. Add the width in bits, such as `#[bits(3)]`",
                input.ident
            ),
        )
    })?;
    let BitsAttr { bits: lit, root } = attr
        .parse_args()
        .map_err(|e| syn::Error::new(e.span(), format!("`{}`: {e}", input.ident)))?;
    let bits: u32 = lit.base10_parse()?;
    if bits == 0 || bits > 64 {
        return Err(syn::Error::new(
            attr.meta.span(),
            format!("`{}`: #[bits({bits})] is outside 1..=64", input.ident),
        ));
    }
    Ok((Width { bits, span: attr.meta.span() }, root))
}

fn newtype(
    input: &DeriveInput,
    data: &DataStruct,
    width: Width,
    root: &Root,
) -> syn::Result<TokenStream> {
    let Fields::Unnamed(fields) = &data.fields else {
        return Err(syn::Error::new(input.ident.span(), newtype_shape(&input.ident)));
    };
    if fields.unnamed.len() != 1 {
        return Err(syn::Error::new(data.fields.span(), newtype_shape(&input.ident)));
    }

    let ty = &fields.unnamed[0].ty;
    let (inner, signed) = collection(&input.ident, ty)?;
    if width.bits > inner {
        return Err(syn::Error::new(
            width.span,
            format!(
                "`{}`: #[bits({})] does not fit `{}`, which has {inner} bits. \
                 Use `{}`, or a smaller width",
                input.ident,
                width.bits,
                crate::attr::spelled(ty),
                width.narrowest_primitive(),
            ),
        ));
    }

    let ident = &input.ident;
    let bits = width.bits;
    let masked = width.mask(quote!(raw));
    let (build, unwrap) = if signed {
        let shift = proc_macro2::Literal::u32_unsuffixed(64 - bits);
        let mask = width.mask(quote!(self.0 as ::core::primitive::i64 as ::core::primitive::u64));
        (
            quote!(Self(((((#masked) << #shift) as ::core::primitive::i64) >> #shift) as #ty)),
            quote!(#mask),
        )
    } else if inner == 64 {
        // No cast, to avoid `clippy::unnecessary_cast` in user code.
        (quote!(Self(#masked)), quote!(self.0))
    } else {
        (quote!(Self((#masked) as #ty)), quote!(self.0 as ::core::primitive::u64))
    };

    let wire_int = wire_int_impl(ident, root);
    Ok(quote! {
        #[automatically_derived]
        impl #root::FieldCodec for #ident {
            const BITS: ::core::primitive::u32 = #bits;

            fn from_raw(raw: ::core::primitive::u64) -> Self {
                #build
            }

            fn to_raw(&self) -> ::core::primitive::u64 {
                #unwrap
            }
        }

        #wire_int
    })
}

/// Lets the type serve as a count, length, offset, discriminant or flags field.
fn wire_int_impl(ident: &syn::Ident, root: &Root) -> TokenStream {
    quote! {
        #[automatically_derived]
        impl #root::WireInt for #ident {
            fn to_i64(&self) -> ::core::primitive::i64 {
                <Self as #root::FieldCodec>::to_raw(self) as ::core::primitive::i64
            }

            fn from_i64(n: ::core::primitive::i64) -> Self {
                <Self as #root::FieldCodec>::from_raw(n as ::core::primitive::u64)
            }
        }
    }
}

fn newtype_shape(ident: &syn::Ident) -> String {
    format!(
        "`{ident}`: #[derive(FieldCodec)] on a struct needs one unnamed integer field. \
         Write `#[bits(15)] struct TrackNumber(u16);`"
    )
}

fn collection(ident: &syn::Ident, ty: &Type) -> syn::Result<(u32, bool)> {
    match scalar_of(ty) {
        Some(Scalar::U(bits)) => return Ok((bits as u32, false)),
        Some(Scalar::I(bits)) => return Ok((bits as u32, true)),
        _ => {}
    }
    let name = crate::attr::spelled(ty);
    Err(syn::Error::new(
        ty.span(),
        format!(
            "`{ident}`: `{name}` is not an integer primitive. \
             Use `u8`..`u64` or `i8`..`i64`, or implement `FieldCodec` by hand"
        ),
    ))
}

struct Listed<'a> {
    ident: &'a syn::Ident,
    value: u64,
    value_span: Span,
}

struct Open<'a> {
    ident: &'a syn::Ident,
    ty: &'a Type,
    collection: u32,
}

fn enum_codec(
    input: &DeriveInput,
    data: &DataEnum,
    width: Width,
    root: &Root,
) -> syn::Result<TokenStream> {
    let mut listed: Vec<Listed<'_>> = Vec::new();
    let mut open: Option<(Open<'_>, Span)> = None;

    for v in &data.variants {
        match variant_key(v, "raw value", "every unlisted raw value")? {
            Key::Other(attr) => {
                if let Some((_, first)) = &open {
                    return Err(noted(
                        attr.meta.span(),
                        format!(
                            "variant `{}`: a second #[other]. Keep one #[other] variant",
                            v.ident
                        ),
                        *first,
                        "first declared here",
                    ));
                }
                open = Some((other_variant(v, width)?, attr.meta.span()));
            }
            Key::Value(lit) => {
                let value: u64 = lit.base10_parse()?;
                if value > width.max() {
                    return Err(syn::Error::new(
                        lit.span(),
                        format!(
                            "variant `{}`: #[value({value})] does not fit #[bits({})]. The largest value is {}",
                            v.ident,
                            width.bits,
                            width.max(),
                        ),
                    ));
                }
                if let Some(prev) = listed.iter().find(|l| l.value == value) {
                    return Err(noted(
                        lit.span(),
                        format!(
                            "variant `{}`: #[value({value})] is already used by another variant",
                            v.ident
                        ),
                        prev.value_span,
                        "first declared here",
                    ));
                }
                if !matches!(v.fields, Fields::Unit) {
                    return Err(syn::Error::new(
                        v.fields.span(),
                        format!(
                            "variant `{}`: a #[value({value})] variant cannot carry data. Make \
                             it a unit variant, or use #[other] to keep the raw value",
                            v.ident,
                        ),
                    ));
                }
                listed.push(Listed { ident: &v.ident, value, value_span: lit.span() });
            }
        }
    }

    if open.is_none() && (listed.len() as u128) != width.cardinality() {
        return Err(syn::Error::new(
            input.ident.span(),
            not_total(&input.ident.to_string(), listed.len(), width),
        ));
    }

    Ok(expand_enum(&input.ident, &listed, open.map(|(o, _)| o), width, root))
}

fn other_variant<'a>(v: &'a syn::Variant, width: Width) -> syn::Result<Open<'a>> {
    let shape = || {
        format!(
            "variant `{}`: #[other] needs one unsigned integer field of at least {} bits. \
             Write `#[other] {}({})`",
            v.ident,
            width.bits,
            v.ident,
            width.narrowest_primitive(),
        )
    };

    let Fields::Unnamed(fields) = &v.fields else {
        return Err(syn::Error::new(v.ident.span(), shape()));
    };
    if fields.unnamed.len() != 1 {
        return Err(syn::Error::new(v.fields.span(), shape()));
    }

    let ty = &fields.unnamed[0].ty;
    let collection = unsigned_width(ty).ok_or_else(|| syn::Error::new(ty.span(), shape()))?;
    if collection < width.bits {
        return Err(syn::Error::new(
            ty.span(),
            format!(
                "variant `{}`: `{}` has {collection} bits, too few for #[bits({})]. Use `{}`",
                v.ident,
                crate::attr::spelled(ty),
                width.bits,
                width.narrowest_primitive(),
            ),
        ));
    }
    Ok(Open { ident: &v.ident, ty, collection })
}

fn not_total(name: &str, listed: usize, width: Width) -> String {
    let bits = width.bits;
    let total = width.cardinality();
    let missing = total - listed as u128;
    format!(
        "`{name}`: {missing} of the {total} values #[bits({bits})] can hold have no variant. \
         Add `#[other] Undefined({prim})`, or list all {total}",
        prim = width.narrowest_primitive(),
    )
}

fn expand_enum(
    ident: &syn::Ident,
    listed: &[Listed<'_>],
    open: Option<Open<'_>>,
    width: Width,
    root: &Root,
) -> TokenStream {
    let bits = width.bits;
    let value_lit = |v: u64| LitInt::new(&format!("{v}u64"), Span::call_site());

    let (decode_arms, decode_tail): (Vec<_>, TokenStream) = match &open {
        Some(open) => {
            let arms = listed
                .iter()
                .map(|l| {
                    let (v, id) = (value_lit(l.value), l.ident);
                    quote!(#v => Self::#id,)
                })
                .collect();
            let (id, ty) = (open.ident, open.ty);
            let carried = if open.collection == 64 { quote!(other) } else { quote!(other as #ty) };
            (arms, quote!(other => Self::#id(#carried),))
        }
        None => {
            let (last, init) = listed
                .split_last()
                .expect("a total enum over 1..=64 bits names at least two values");
            let arms = init
                .iter()
                .map(|l| {
                    let (v, id) = (value_lit(l.value), l.ident);
                    quote!(#v => Self::#id,)
                })
                .collect();
            let id = last.ident;
            (arms, quote!(_ => Self::#id,))
        }
    };

    let encode_arms = listed.iter().map(|l| {
        let (v, id) = (value_lit(l.value), l.ident);
        quote!(Self::#id => #v,)
    });
    let encode_tail = open.as_ref().map(|open| {
        let id = open.ident;
        let carried = if open.collection == 64 {
            quote!(*raw)
        } else {
            quote!(*raw as ::core::primitive::u64)
        };
        quote!(Self::#id(raw) => #carried,)
    });

    let raw = width.mask(quote!(raw));
    let wire_int = wire_int_impl(ident, root);

    quote! {
        #[automatically_derived]
        impl #root::FieldCodec for #ident {
            const BITS: ::core::primitive::u32 = #bits;

            fn from_raw(raw: ::core::primitive::u64) -> Self {
                match #raw {
                    #(#decode_arms)*
                    #decode_tail
                }
            }

            fn to_raw(&self) -> ::core::primitive::u64 {
                match self {
                    #(#encode_arms)*
                    #encode_tail
                }
            }
        }

        #wire_int
    }
}

fn unsigned_width(ty: &Type) -> Option<u32> {
    match scalar_of(ty)? {
        Scalar::U(bits) => Some(bits as u32),
        _ => None,
    }
}
