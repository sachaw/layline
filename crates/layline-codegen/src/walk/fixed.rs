use proc_macro2::{Literal, TokenStream};
use quote::quote;

#[cfg(feature = "emit")]
use super::derive_attr;
use super::tokens::ident;
use super::{doc_attr, emit_field, endian_arg, path};
use crate::root::Prelude;
use crate::{BitOrder, Container, DispatchDef, EnumDef, Error, LayoutDef, Root, Scalar};

/// A [`LayoutDef`] as token parts, for a caller that writes its own struct line.
pub struct LayoutParts {
    /// `#[derive(layline::Layout)]` and its `#[layout(..)]` attribute.
    pub attrs: TokenStream,
    /// Field declarations in wire order, with docs, width attributes and `#[at]` positions.
    pub fields: TokenStream,
}

/// Converts a fixed layout to its derive attributes and field declarations.
///
/// Validates the layout first.
///
/// # Errors
///
/// [`Error::Invalid`], naming the layout, when [`validate`](crate::validate()) refuses it.
pub fn layout_parts(l: &LayoutDef, root: &Root) -> Result<LayoutParts, Error> {
    crate::validate::validate_layout(l).map_err(|why| Error::Invalid(l.name.clone(), why))?;

    let c = &l.container;
    let key = ident(c.unit().keyword());
    let units = Literal::u64_unsuffixed(c.units());
    let endian = endian_arg(c.endian());
    let order = (c.order() == Some(BitOrder::Msb)).then(|| quote! { , order = msb });
    let (prefix, prefix_value) = match c {
        Container::Word { prefix: 0, .. } | Container::Bytes { .. } | Container::Words { .. } => {
            (None, None)
        }
        Container::Word { prefix, prefix_value, .. } => {
            let n = Literal::u64_unsuffixed(*prefix);
            let value = prefix_value.map(|v| {
                let width = *prefix as usize;
                let v: TokenStream = format!("0b{v:0width$b}").parse().expect("a binary literal");
                quote! { , prefix_value = #v }
            });
            (Some(quote! { , prefix = #n }), value)
        }
    };
    let view = l.view.then(|| quote! { , view });
    let crate_arg = root.crate_arg();
    let bit_addressed = c.is_bit_addressed();
    let fields = l.fields.iter().map(|f| emit_field(f, bit_addressed, root));
    Ok(LayoutParts {
        attrs: quote! {
            #[derive(#root::Layout)]
            #[layout(#key = #units #prefix #prefix_value #view #endian #order #crate_arg)]
        },
        fields: quote! { #(#fields)* },
    })
}

/// Emits a fixed layout: [`layout_parts`] plus the struct line and derives.
///
/// # Errors
///
/// Whatever [`layout_parts`] refuses.
#[cfg(feature = "emit")]
pub(crate) fn emit_layout(
    l: &LayoutDef,
    derives: &[String],
    root: &Root,
) -> Result<TokenStream, Error> {
    let name = ident(&l.name);
    let der = derive_attr(derives)?;
    let LayoutParts { attrs, fields } = layout_parts(l, root)?;
    Ok(quote! {
        #der
        #attrs
        pub struct #name {
            #fields
        }
    })
}

/// An [`EnumDef`] as token parts.
pub struct EnumParts {
    /// Variant declarations in order, then the variant for unknown values if the enum is open.
    pub variants: TokenStream,
    /// `impl FieldCodec for Name`, total in both directions.
    pub codec: TokenStream,
    /// `impl Name { pub const fn code(self) -> Repr }`, the wire value in `const` contexts.
    pub code: TokenStream,
}

/// Converts an enum to its variants, codec and `const fn code`.
///
/// # Errors
///
/// [`Error::Invalid`], naming the enumeration, when [`validate`](crate::validate()) refuses it.
pub fn enum_parts(e: &EnumDef, root: &Root) -> Result<EnumParts, Error> {
    let Prelude { u32, u64, .. } = root.prelude();
    crate::validate::validate_enum(e).map_err(|why| Error::Invalid(e.name.clone(), why))?;
    let name = ident(&e.name);
    let repr = root.prelude().primitive(e.repr.primitive());
    let bits = Literal::u32_suffixed(match e.repr {
        Scalar::U(n) | Scalar::I(n) => n as u32,
        _ => 32,
    });

    let variants = e.variants.iter().map(|v| {
        let variant = ident(&v.name);
        let vdoc = doc_attr(&Some(v.doc.clone().unwrap_or_else(|| format!("{}.", v.value))));
        quote! { #vdoc #variant, }
    });
    let other_ident = e.other.as_deref().map(ident);
    let other = other_ident.as_ref().map(|other| {
        quote! {
            /// An undefined value, kept as its raw number.
            #other(#repr),
        }
    });

    let from_arms = e.variants.iter().map(|v| {
        let variant = ident(&v.name);
        let lit = Literal::i64_unsuffixed(v.value);
        quote! { #lit => Self::#variant, }
    });
    let from_fallback = match &other_ident {
        Some(other) => quote! { other => Self::#other(other), },
        None => {
            let first = ident(&e.variants[0].name);
            quote! { _ => Self::#first, }
        }
    };
    let to_arms = e.variants.iter().map(|v| {
        let variant = ident(&v.name);
        let lit = Literal::i64_unsuffixed(v.value);
        quote! { Self::#variant => #lit, }
    });
    let to_fallback = other_ident.as_ref().map(|other| quote! { Self::#other(raw) => *raw, });

    let code_arms = e.variants.iter().map(|v| {
        let variant = ident(&v.name);
        let lit = Literal::i64_unsuffixed(v.value);
        quote! { Self::#variant => #lit, }
    });
    let code_fallback = other_ident.as_ref().map(|other| quote! { Self::#other(raw) => raw, });

    Ok(EnumParts {
        variants: quote! {
            #(#variants)*
            #other
        },
        codec: quote! {
            impl #root::FieldCodec for #name {
                const BITS: #u32 = #bits;
                fn from_raw(raw: #u64) -> Self {
                    match raw as #repr {
                        #(#from_arms)*
                        #from_fallback
                    }
                }
                fn to_raw(&self) -> #u64 {
                    (match self {
                        #(#to_arms)*
                        #to_fallback
                    }) as #u64
                }
            }
        },
        code: quote! {
            impl #name {
                /// The wire value, usable in `const` contexts.
                #[must_use]
                pub const fn code(self) -> #repr {
                    match self {
                        #(#code_arms)*
                        #code_fallback
                    }
                }
            }
        },
    })
}

/// Emits an enum: [`enum_parts`] plus the enum line, docs and derives.
///
/// Leaves out [`EnumParts::code`].
///
/// # Errors
///
/// Whatever [`enum_parts`] refuses.
#[cfg(feature = "emit")]
pub(crate) fn emit_enum(
    e: &EnumDef,
    derives: &[String],
    root: &Root,
) -> Result<TokenStream, Error> {
    let name = ident(&e.name);
    let der = derive_attr(derives)?;
    let doc = doc_attr(&e.doc);
    let EnumParts { variants, codec, .. } = enum_parts(e, root)?;
    Ok(quote! {
        #doc
        #der
        #[derive(Copy, Eq)]
        pub enum #name {
            #variants
        }

        #codec
    })
}

/// A [`DispatchDef`] as token parts, for a caller that writes its own enum line.
pub struct DispatchParts {
    /// The variants, each with `#[value(N)]` or `#[other]`.
    pub arms: TokenStream,
    /// The container attributes, including the derive.
    pub attrs: TokenStream,
}

/// Converts a dispatch catalogue to its variants and attributes.
///
/// `#[derive(Dispatch)]` writes the decoder.
///
/// # Errors
///
/// [`Error::Invalid`], naming the catalogue, when [`validate`](crate::validate()) refuses it.
pub fn dispatch_parts(d: &DispatchDef, root: &Root) -> Result<DispatchParts, Error> {
    let Prelude { vec, u8, .. } = root.prelude();
    crate::validate::validate_dispatch(d).map_err(|why| Error::Invalid(d.name.clone(), why))?;
    let crate_arg = root.crate_arg();
    let id_ty = root.prelude().primitive(d.id.primitive());
    let arms = d.arms.iter().map(|a| {
        let doc = doc_attr(&a.doc);
        let variant = ident(&a.name);
        let ty = path(&a.ty);
        let id = Literal::i64_unsuffixed(a.id);
        quote! { #doc #[value(#id)] #variant(#ty), }
    });
    let other = ident(&d.other);
    Ok(DispatchParts {
        arms: quote! {
            #(#arms)*
            /// An unknown id, kept with its body.
            #[other]
            #other { id: #id_ty, body: #vec<#u8> },
        },
        attrs: quote! {
            #[derive(#root::Dispatch)]
            #[dispatch(id = #id_ty #crate_arg)]
        },
    })
}

/// Emits a dispatch catalogue with its derives.
///
/// # Errors
///
/// Whatever [`dispatch_parts`] refuses.
#[cfg(feature = "emit")]
pub(crate) fn emit_dispatch(
    d: &DispatchDef,
    derives: &[String],
    root: &Root,
) -> Result<TokenStream, Error> {
    let name = ident(&d.name);
    let der = derive_attr(derives)?;
    let doc = doc_attr(&d.doc);
    let DispatchParts { arms, attrs } = dispatch_parts(d, root)?;
    Ok(quote! {
        #doc
        #der
        #attrs
        pub enum #name {
            #arms
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "emit")]
    use crate::Variant;
    use crate::{Container, Endian, Field, Kind};

    #[test]
    fn layout_parts_refuses_a_container_its_fields_do_not_tile() {
        let short = LayoutDef::new(
            "Header",
            Container::Bytes { bytes: 8, endian: Endian::Le },
            vec![Field::new("magic", Kind::Scalar(Scalar::U(32)))],
        );
        let Err(Error::Invalid(name, crate::Invalid::Tiling { covered_bits, declared_bits })) =
            layout_parts(&short, &Root::default())
        else {
            panic!("32 bits do not fill 64: the model is invalid, and validation names it");
        };
        assert_eq!((name.as_str(), covered_bits, declared_bits), ("Header", 32, 64));
    }

    #[test]
    #[cfg(feature = "emit")]
    fn the_parts_carry_nothing_the_caller_owns() {
        let l = LayoutDef::new(
            "Entry",
            Container::Bytes { bytes: 4, endian: Endian::Le },
            vec![Field::new("value", Kind::Scalar(Scalar::U(32)))],
        );
        let parts = layout_parts(&l, &Root::default()).expect("it tiles");
        let attrs = parts.attrs.to_string();
        let fields = parts.fields.to_string();
        assert!(attrs.contains("layline :: Layout") && attrs.contains("bytes = 4"), "{attrs}");
        for owned in ["struct", "Entry", "Debug", "Clone"] {
            assert!(!attrs.contains(owned), "the attributes have no {owned}: {attrs}");
            assert!(!fields.contains(owned), "the fields carry no {owned}: {fields}");
        }
        assert_eq!(fields, "pub value : u32 ,");

        let whole = emit_layout(&l, &["serde::Serialize".into()], &Root::default())
            .expect("it tiles")
            .to_string();
        assert!(whole.contains("Debug , Clone , PartialEq , serde :: Serialize"), "{whole}");
        assert!(whole.contains(&attrs) && whole.contains(&fields), "{whole}");
    }

    #[test]
    fn enum_parts_refuses_a_closed_catalogue_with_no_variants() {
        let e = EnumDef::new("Mode", Scalar::U(8), vec![]);
        let Err(Error::Invalid(name, why)) = enum_parts(&e, &Root::default()) else {
            panic!("a closed catalogue with no variants has no total decode");
        };
        assert_eq!(name, "Mode");
        assert!(format!("{why}").contains("not total"), "{why}");
        assert!(enum_parts(&e.with_other("Other"), &Root::default()).is_ok());
    }

    #[test]
    #[cfg(feature = "emit")]
    fn the_const_discriminant_is_published_and_not_attached() {
        let e = EnumDef::new(
            "Mode",
            Scalar::U(8),
            vec![Variant::new(0, "Idle"), Variant::new(7, "Active")],
        )
        .with_other("Other");
        let parts = enum_parts(&e, &Root::default()).expect("a catalogue");
        let code = parts.code.to_string();
        assert!(code.contains("pub const fn code (self) -> :: core :: primitive :: u8"), "{code}");
        assert!(code.contains("Self :: Active => 7"), "{code}");
        assert!(code.contains("Self :: Other (raw) => raw"), "{code}");
        assert!(parts.codec.to_string().contains("Self :: Other (raw) => * raw"));

        let whole = emit_enum(&e, &[], &Root::default()).expect("a catalogue").to_string();
        assert!(!whole.contains("fn code"), "a whole-item emission is unchanged by it:\n{whole}");
        assert!(whole.contains(&parts.codec.to_string()), "{whole}");
    }

    #[test]
    fn a_layout_with_a_view_emits_the_key() {
        let mut l = LayoutDef::new(
            "Head",
            Container::Bytes { bytes: 2, endian: Endian::Le },
            vec![Field::new("n", Kind::Scalar(Scalar::U(16)))],
        );
        let without = layout_parts(&l, &Root::default()).expect("lowers").attrs.to_string();
        assert!(!without.contains("view"), "{without}");
        l.view = true;
        let with = layout_parts(&l, &Root::default()).expect("lowers").attrs.to_string();
        assert!(with.contains("# [layout (bytes = 2 , view)]"), "{with}");
    }
}
