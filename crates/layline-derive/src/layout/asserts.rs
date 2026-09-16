//! `#[magic]`, `#[checksum]` and `#[range]`: checked on decode and written on encode.

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote, quote_spanned};

use layline_codegen::Root;

use super::access::byte_fns;
use super::{Body, Item, Plan, Prim, Reading, body_ty};

struct MagicAt {
    field: String,
    /// Byte offset in the record.
    at: usize,
    /// In wire order.
    bytes: Vec<u8>,
}

struct CheckAt {
    field: syn::Ident,
    /// Byte offset in the record.
    at: usize,
    width: usize,
    algorithm: syn::Path,
    /// Covered byte ranges, half-open.
    runs: Vec<(usize, usize)>,
    from: layline_codegen::CoverFrom,
    to: layline_codegen::CoverTo,
    /// Whether the checksum's own bytes are skipped within the range.
    excludes_self: bool,
}

fn value_bounds(body: &Body) -> Option<(i128, i128)> {
    let Body::Packed(p) = body else { return None };
    let (signed, bits) = match p.reading {
        Reading::Uint(_) => (false, p.bits),
        Reading::Int => (true, p.bits),
        Reading::Native(Prim::U8) => (false, 8),
        Reading::Native(Prim::U16) => (false, 16),
        Reading::Native(Prim::U32) => (false, 32),
        Reading::Native(Prim::U64) => (false, 64),
        Reading::Native(Prim::I8) => (true, 8),
        Reading::Native(Prim::I16) => (true, 16),
        Reading::Native(Prim::I32) => (true, 32),
        Reading::Native(Prim::I64) => (true, 64),
        Reading::Native(_) | Reading::Bool | Reading::Codec => return None,
    };
    if bits == 0 || bits > 64 {
        return None;
    }
    let bits = i128::from(bits);
    Some(if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    })
}

#[derive(Default)]
pub(super) struct Asserts {
    magic: Vec<MagicAt>,
    check: Vec<CheckAt>,
    range: Vec<RangeAt>,
}

struct RangeAt {
    field: syn::Ident,
    lo: i128,
    hi: i128,
}

impl Asserts {
    pub(super) fn any(&self) -> bool {
        !self.magic.is_empty() || !self.check.is_empty() || !self.range.is_empty()
    }
}

impl Plan<'_> {
    pub(super) fn resolve_assertions(&mut self) -> syn::Result<()> {
        let mut extents: Vec<(String, usize, usize)> = Vec::new();
        for item in &self.items {
            let width = item.body.width();
            if item.phys_bit % 8 != 0 || width % 8 != 0 {
                if item.magic.is_some() || item.check.is_some() {
                    return Err(syn::Error::new(
                        item.span,
                        format!(
                            "field `{}`: #[magic] and #[checksum] need whole bytes, \
                             but this field covers bits {}..{}",
                            item.ident,
                            item.phys_bit,
                            item.phys_bit + width,
                        ),
                    ));
                }
                continue;
            }
            extents.push((
                item.ident.to_string(),
                (item.phys_bit / 8) as usize,
                ((item.phys_bit + width) / 8) as usize,
            ));
        }
        let start_of = |name: &str| extents.iter().find(|(n, ..)| n == name).map(|&(_, a, _)| a);
        let end_of = |name: &str| extents.iter().find(|(n, ..)| n == name).map(|&(.., b)| b);
        let order_of = |name: &str| extents.iter().position(|(n, ..)| n == name);

        let mut asserts = Asserts::default();
        for item in &self.items {
            let Some(r) = &item.range else { continue };
            let Some((min, max)) = value_bounds(&item.body) else {
                return Err(syn::Error::new(
                    r.span,
                    format!("field `{}`: `#[range]` needs an integer field", item.ident,),
                ));
            };
            let (lo, hi) = (r.lo.unwrap_or(min), r.hi.unwrap_or(max));
            if i64::try_from(lo).is_err() || i64::try_from(hi).is_err() {
                return Err(syn::Error::new(
                    r.span,
                    format!(
                        "field `{}`: `#[range({lo}..={hi})]` does not fit `i64`. \
                         Use bounds within `i64`",
                        item.ident,
                    ),
                ));
            }
            if lo < min || hi > max {
                return Err(syn::Error::new(
                    r.span,
                    format!(
                        "field `{}`: `#[range({lo}..={hi})]` goes beyond the field's {min}..={max}. \
                         Narrow it",
                        item.ident,
                    ),
                ));
            }
            if (lo, hi) == (min, max) {
                return Err(syn::Error::new(
                    r.span,
                    format!(
                        "field `{}`: `#[range({lo}..={hi})]` allows every value the field holds. \
                         Remove it, or narrow it",
                        item.ident,
                    ),
                ));
            }
            asserts.range.push(RangeAt { field: item.ident.clone(), lo, hi });
        }
        for item in &self.items {
            let name = item.ident.to_string();
            let (at, end) = match (start_of(&name), end_of(&name)) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };
            if let Some(m) = &item.magic {
                asserts.magic.push(MagicAt {
                    field: name.clone(),
                    at,
                    bytes: crate::claim::magic_bytes(
                        m,
                        &item.ident.to_string(),
                        end - at,
                        self.endian,
                    )?,
                });
            }
            let Some(spec) = &item.check else { continue };
            asserts.check.push(self.resolve_check(item, spec, at, end, &extents)?);
        }

        for c in &asserts.check {
            let (Some(here), name) = (order_of(&c.field.to_string()), c.field.to_string()) else {
                continue;
            };
            for other in &asserts.check {
                let other_name = other.field.to_string();
                if other_name == name {
                    continue;
                }
                let covered =
                    c.runs.iter().any(|&(a, b)| other.at >= a && other.at + other.width <= b);
                if covered && order_of(&other_name).is_some_and(|i| i > here) {
                    return Err(syn::Error::new(
                        c.field.span(),
                        format!(
                            "field `{name}`: the checksum range covers `{other_name}`, a later checksum. \
                             Narrow the range, or only cover earlier checksums"
                        ),
                    ));
                }
            }
        }

        self.asserts = asserts;
        Ok(())
    }

    fn resolve_check(
        &self,
        item: &Item,
        spec: &crate::claim::ChecksumSpec,
        at: usize,
        end: usize,
        extents: &[(String, usize, usize)],
    ) -> syn::Result<CheckAt> {
        use layline_codegen::{CoverFrom, CoverTo};

        let field = item.ident.to_string();
        let named = |name: &str, r: Option<&crate::claim::Ref>, which: &str| {
            let found = extents.iter().find(|(n, ..)| n == name);
            found.map(|&(_, a, b)| (a, b)).ok_or_else(|| {
                syn::Error::new(
                    r.map_or_else(|| spec.span, |r| r.span),
                    format!(
                        "field `{field}`: `over` cannot {which} `{name}`, which is not a field of this layout"
                    ),
                )
            })
        };
        let self_bound = |r: Option<&crate::claim::Ref>, sentence: &str| -> syn::Result<()> {
            match r {
                Some(r) if r.name == field => Err(syn::Error::new(r.span, sentence.to_string())),
                _ => Ok(()),
            }
        };

        self_bound(
            spec.from_ref.as_ref(),
            &format!(
                "field `{field}`: `over` cannot begin at the checksum itself. \
                 Name the first covered field"
            ),
        )?;
        self_bound(
            spec.to_ref.as_ref(),
            &if spec.inclusive {
                format!(
                    "field `{field}`: `over = ..={field}` includes the checksum's own bytes. \
                     Name the last covered field"
                )
            } else {
                format!(
                    "field `{field}`: `over = ..{field}` is the default end. \
                     Write `over = ..` or `over = f..`"
                )
            },
        )?;

        let mut ends: Vec<(String, (u64, u64))> = Vec::new();
        if let CoverFrom::Field(name) = &spec.over.from {
            let (a, b) = named(name, spec.from_ref.as_ref(), "begin at")?;
            ends.push((name.clone(), (a as u64, b as u64)));
        }
        match &spec.over.to {
            CoverTo::Before(name) => {
                let (a, b) = named(name, spec.to_ref.as_ref(), "end at")?;
                ends.push((name.clone(), (a as u64, b as u64)));
            }
            CoverTo::After(name) => {
                let (a, b) = named(name, spec.to_ref.as_ref(), "run through")?;
                ends.push((name.clone(), (a as u64, b as u64)));
            }
            _ => {}
        }
        let resolved = spec
            .over
            .resolve((at as u64, end as u64), |n| {
                ends.iter().find(|(k, _)| k == n).map(|&(_, v)| v)
            })
            .expect("every bound name was looked up above");
        let (from, to) = (resolved.from as usize, resolved.to as usize);

        if to < from {
            return Err(syn::Error::new(
                spec.span,
                format!("field `{field}`: `over` runs backwards, from byte {from} to byte {to}"),
            ));
        }
        let runs: Vec<(usize, usize)> =
            resolved.runs().into_iter().map(|(a, b)| (a as usize, b as usize)).collect();
        if runs.iter().all(|&(a, b)| a >= b) {
            return Err(syn::Error::new(
                spec.span,
                format!("field `{field}`: `over` covers no bytes. Widen the range"),
            ));
        }

        Ok(CheckAt {
            field: item.ident.clone(),
            at,
            width: end - at,
            algorithm: spec.algorithm.clone(),
            runs: runs.into_iter().filter(|&(a, b)| a < b).collect(),
            from: spec.over.from.clone(),
            to: spec.over.to.clone(),
            excludes_self: resolved.excises(),
        })
    }
}

impl Plan<'_> {
    pub(super) fn magic_const(&self) -> Option<TokenStream> {
        if self.asserts.magic.is_empty() {
            return None;
        }
        let n = self.asserts.magic.len();
        let rows = self.asserts.magic.iter().map(|m| {
            let bytes = m.bytes.iter().map(|b| Literal::u8_suffixed(*b));
            quote! { &[ #(#bytes),* ] }
        });
        Some(quote! {
            /// The `#[magic]` constants, in declaration order.
            #[doc(hidden)]
            const __LAYLINE_MAGIC: [&'static [::core::primitive::u8]; #n] = [ #(#rows),* ];
        })
    }

    pub(super) fn const_defs(&self) -> Vec<TokenStream> {
        let root = self.root;
        self.asserts
            .magic
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let field = &m.field;
                quote! {
                    #root::table::ConstDef::new(#field, Self::__LAYLINE_MAGIC[#i])
                }
            })
            .collect()
    }

    pub(super) fn range_defs(&self) -> Vec<TokenStream> {
        let root = self.root;
        self.asserts
            .range
            .iter()
            .map(|r| {
                let field = r.field.to_string();
                let (lo, hi) =
                    (Literal::i64_suffixed(r.lo as i64), Literal::i64_suffixed(r.hi as i64));
                quote! { #root::table::RangeDef::new(#field, #lo, #hi) }
            })
            .collect()
    }

    pub(super) fn cover_defs(&self) -> Vec<TokenStream> {
        use layline_codegen::{CoverFrom, CoverTo};
        let root = self.root;
        self.asserts
            .check
            .iter()
            .map(|c| {
                let field = c.field.to_string();
                let from = match &c.from {
                    CoverFrom::Field(name) => quote!(#root::table::CoverEdge::Before(#name)),
                    _ => quote!(#root::table::CoverEdge::Start),
                };
                let to = match &c.to {
                    CoverTo::Before(name) => quote!(#root::table::CoverEdge::Before(#name)),
                    CoverTo::After(name) => quote!(#root::table::CoverEdge::After(#name)),
                    _ => quote!(#root::table::CoverEdge::Before(#field)),
                };
                let excludes_self = c.excludes_self;
                quote! {
                    #root::table::CoverDef::new(#field, #from, #to, #excludes_self)
                }
            })
            .collect()
    }

    pub(super) fn check_fits(&self) -> Vec<TokenStream> {
        let root = self.root;
        self.asserts
            .check
            .iter()
            .map(|c| {
                let (algo, field) = (&c.algorithm, &c.field);
                let ty = self.field_ty(field);
                let assert = format_ident!("__layline_checksum_fits_{}_{}", self.ident, field);
                let spanned = layline_codegen::__derive::respan(root, field.span());
                quote_spanned! {field.span()=>
                    const _: () = {
                        #[allow(non_snake_case)]
                        fn #assert<A: #spanned::__private::ChecksumFits<T>, T>() {}
                        let _ = #assert::<#algo, #ty>;
                    };
                }
            })
            .collect()
    }

    fn field_ty(&self, ident: &syn::Ident) -> TokenStream {
        self.items
            .iter()
            .find(|i| &i.ident == ident)
            .map_or_else(|| quote!(::core::primitive::u8), |i| body_ty(&i.body))
    }

    pub(super) fn magic_checks(&self) -> TokenStream {
        let root = self.root;
        let checks = self.asserts.magic.iter().enumerate().map(|(i, m)| {
            let (at, field) = (m.at, &m.field);
            let end = m.at + m.bytes.len();
            quote! {
                if wire[#at..#end] != *Self::__LAYLINE_MAGIC[#i] {
                    return ::core::result::Result::Err(#root::ParseError::Magic {
                        field: #field,
                        expected: Self::__LAYLINE_MAGIC[#i],
                        at: #at,
                    });
                }
            }
        });
        quote! { #(#checks)* }
    }

    pub(super) fn range_checks(&self) -> TokenStream {
        let root = self.root;
        let checks = self.asserts.range.iter().map(|r| {
            let (field, name) = (&r.field, r.field.to_string());
            let (lo, hi) = (Literal::i64_suffixed(r.lo as i64), Literal::i64_suffixed(r.hi as i64));
            quote! {
                {
                    // `contains` avoids `clippy::manual_range_contains` in user code.
                    let __got = #root::WireInt::to_i64(&__value.#field);
                    if !(#lo..=#hi).contains(&__got) {
                        return ::core::result::Result::Err(#root::ParseError::OutOfRange {
                            field: #name,
                            value: __got,
                            lo: #lo,
                            hi: #hi,
                        });
                    }
                }
            }
        });
        quote! { #(#checks)* }
    }

    pub(super) fn range_guards(&self) -> TokenStream {
        let root = self.root;
        let guards = self.asserts.range.iter().map(|r| {
            let field = &r.field;
            let (lo, hi) = (Literal::i64_suffixed(r.lo as i64), Literal::i64_suffixed(r.hi as i64));
            let msg =
                format!("field `{}`: value is outside `#[range({}..={})]`", r.field, r.lo, r.hi,);
            quote! {
                {
                    let __v = #root::WireInt::to_i64(&self.#field);
                    ::core::assert!((#lo..=#hi).contains(&__v), #msg);
                }
            }
        });
        quote! { #(#guards)* }
    }

    pub(super) fn check_verify(&self) -> TokenStream {
        let root = self.root;
        let checks = self.asserts.check.iter().map(|c| {
            let (field, name) = (&c.field, c.field.to_string());
            let want = fold_over(&c.algorithm, &c.runs, quote!(wire), root);
            quote! {
                {
                    let __want = #root::WireInt::to_i64(&#want);
                    let __got = #root::WireInt::to_i64(&__value.#field);
                    if __want != __got {
                        return ::core::result::Result::Err(#root::ParseError::Checksum {
                            field: #name,
                            expected: __want,
                            actual: __got,
                        });
                    }
                }
            }
        });
        quote! { #(#checks)* }
    }

    pub(super) fn magic_writes(&self) -> TokenStream {
        let writes = self.asserts.magic.iter().enumerate().map(|(i, m)| {
            let (at, end) = (m.at, m.at + m.bytes.len());
            quote! { __out[#at..#end].copy_from_slice(Self::__LAYLINE_MAGIC[#i]); }
        });
        quote! { #(#writes)* }
    }

    pub(super) fn checksum_writes(&self) -> TokenStream {
        let root = self.root;
        let clear = self.asserts.check.iter().map(|c| {
            let (at, end) = (c.at, c.at + c.width);
            quote! { __out[#at..#end].fill(0); }
        });
        let writes = self.asserts.check.iter().map(|c| {
            let (at, end) = (c.at, c.at + c.width);
            let value = fold_over(&c.algorithm, &c.runs, quote!(__out), root);
            let ty = self.field_ty(&c.field);
            let to_bytes = byte_fns(self.endian).1;
            quote! {
                {
                    let __ck: #ty = #value;
                    __out[#at..#end].copy_from_slice(&__ck.#to_bytes());
                }
            }
        });
        quote! { #(#clear)* #(#writes)* }
    }
}

fn fold_over(
    algo: &syn::Path,
    runs: &[(usize, usize)],
    buf: TokenStream,
    root: &Root,
) -> TokenStream {
    let slices: Vec<TokenStream> = runs
        .iter()
        .map(|&(a, b)| {
            quote! { &#buf[#a..#b] }
        })
        .collect();
    match slices.as_slice() {
        [one] => quote!(<#algo as #root::Checksum>::compute(#one)),
        many => {
            let folded = many.iter().fold(
                quote!(<#algo as #root::Checksum>::init()),
                |acc, run| quote!(<#algo as #root::Checksum>::update(#acc, #run)),
            );
            quote!(<#algo as #root::Checksum>::finish(#folded))
        }
    }
}
