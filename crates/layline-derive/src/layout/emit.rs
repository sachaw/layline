//! Emits `impl Layout`, `decode`, `encode`, the field tables and overlay accessors.

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote, quote_spanned};

use layline_codegen::{Root, StatedUnit};

use super::access::{Access, Place, Writing, wide_mask_of, win_literal, wire_fns};
use super::{Body, Packed, Plan, Reading};

fn row(at: u64, name: &str, width: u32, root: &Root) -> (u64, TokenStream) {
    let start = Literal::u64_suffixed(at);
    let width = Literal::u32_suffixed(width);
    (
        at,
        quote! {
            #root::table::FieldDef::new(#name, #root::Extent::new(#start, #width))
        },
    )
}

impl Plan<'_> {
    pub(super) fn emit(&self) -> TokenStream {
        let root = self.root;
        let ident = &self.ident;
        let (bits, wire_bytes) = (self.bits, self.wire_bytes);

        let mut checks = Vec::new();
        for item in &self.items {
            self.width_checks(&item.body, &item.ident, &mut checks);
        }
        checks.extend(self.check_fits());
        let (overlay_checks, overlays) = self.overlay_accessors();

        let field_defs = self.field_defs();
        let lenses = self.lens_array();

        let decode = self.decode_body();
        let encode = self.encode_body();
        let asserting = self.asserts.any() || self.internal.0 || self.prefix_value.is_some();
        let magic_const = self.magic_const();
        let (decode_ret, decode_doc) = if asserting {
            (
                quote!(::core::result::Result<Self, #root::ParseError>),
                quote! {
                    /// Decode from wire bytes.
                    ///
                    /// # Errors
                    ///
                    /// Returns an error if the bytes fail a check this record declares.
                },
            )
        } else {
            (
                quote!(Self),
                quote! {
                    /// Decode from wire bytes.
                },
            )
        };
        let view = self.view.then(|| self.view_type());
        let covers_container = (self.bits > 0).then(|| {
            quote! {
                const _: () = {
                    if #bits != 0 {
                        #root::__private::assert_layout(
                        <#ident as #root::Layout>::FIELDS,
                        #bits as ::core::primitive::u64,
                    );
                    }
                };
            }
        });

        let tables = Tables {
            types: self.carrier_defs(),
            stated: self.stated_defs(),
            constants: self.const_defs(),
            ranges: self.range_defs(),
            covered: self.cover_defs(),
        };
        let layout_tr =
            layout_impl(ident, &tables, &field_defs, self.prefix, wire_bytes, asserting, self.root);

        quote! {
            #(#checks)*
            #(#overlay_checks)*

            #layout_tr

            #[automatically_derived]
            impl #ident {
                #lenses

                #magic_const

                #decode_doc
                #[must_use]
                pub fn decode(wire: &[::core::primitive::u8; #wire_bytes]) -> #decode_ret {
                    #decode
                }

                /// Encode to wire bytes.
                #[must_use]
                pub fn encode(&self) -> [::core::primitive::u8; #wire_bytes] {
                    #encode
                }

                #(#overlays)*
            }

            #covers_container

            #view
        }
    }

    fn lens_array(&self) -> Option<TokenStream> {
        let root = self.root;
        if !self.hoisted() {
            return None;
        }
        let n = self.items.len();
        let entries = self.items.iter().map(|item| {
            let Body::Packed(p) = &item.body else {
                unreachable!("the container grid carries bit fields only")
            };
            let (start, width) = (item.phys_bit, p.bits);
            quote! { #root::Extent::new(#start, #width) }
        });
        Some(quote! {
            #[doc(hidden)]
            const __LAYLINE_LENSES: [#root::Extent; #n] = [ #(#entries),* ];
        })
    }

    fn stated_defs(&self) -> Vec<TokenStream> {
        let root = self.root;
        self.items
            .iter()
            .filter_map(|item| {
                let stated = item.at?.stated.published(item.phys_bit);
                let field = item.ident.to_string();
                let unit = match stated.unit {
                    StatedUnit::Byte => quote!(Byte),
                    StatedUnit::Bit => quote!(Bit),
                    _ => unreachable!("the `#[at]` parser builds bits and bytes"),
                };
                let pos = Literal::u64_suffixed(stated.pos);
                Some(quote! {
                    #root::table::StatedDef::new(#field, #root::table::StatedUnit::#unit, #pos)
                })
            })
            .collect()
    }

    fn carrier_defs(&self) -> Vec<TokenStream> {
        let root = self.root;
        self.items
            .iter()
            .map(|item| {
                let field = item.ident.to_string();
                let ty = type_name(&item.body);
                quote! {
                    #root::table::TypeDef::new(#field, #ty)
                }
            })
            .collect()
    }

    fn field_defs(&self) -> Vec<TokenStream> {
        let fields: Vec<layline_codegen::Field> = self
            .items
            .iter()
            .map(|i| layline_codegen::Field::new(&i.ident.to_string(), i.body.kind()))
            .collect();
        layline_codegen::__derive::field_rows(&fields, &self.grid)
            .into_iter()
            .map(|r| row(r.extent.start(), &r.name, r.extent.width(), self.root).1)
            .collect()
    }

    fn decode_body(&self) -> TokenStream {
        let place = Place { buf: quote!(wire), buf_ref: quote!(wire), hoisted: true };
        let prologue = self.load_prologue(&place);
        let fields = self.items.iter().enumerate().map(|(i, item)| {
            let name = &item.ident;
            let value = self.read(&item.body, item.phys_bit, i, &place);
            quote! { #name: #value, }
        });
        let built = quote! {
            let _ = wire;
            #prologue
            Self { #(#fields)* }
        };
        if !self.asserts.any() && self.prefix_value.is_none() {
            return if self.internal.0 {
                quote! { ::core::result::Result::Ok({ #built }) }
            } else {
                built
            };
        }
        let magic = self.magic_checks();
        let prefix = self.prefix_check();
        let ranges = self.range_checks();
        let verify = self.check_verify();
        quote! {
            #magic
            #prefix
            let __value = { #built };
            #ranges
            #verify
            ::core::result::Result::Ok(__value)
        }
    }

    fn prefix_window(&self) -> Access {
        let at = self.prefix_physical();
        let (c0, span) = self.container_window(at, u64::from(self.prefix));
        Access::Window {
            offset: self.wire_offset(c0, span),
            len: span,
            reg: span.next_power_of_two(),
            shift: (at % 8) as u32,
            mask: wide_mask_of(self.prefix),
            wide: false,
            tail: None,
        }
    }

    /// Refuses prefix bits other than `prefix_value`.
    fn prefix_check(&self) -> TokenStream {
        let Some(expected) = self.prefix_value else {
            return quote!();
        };
        let root = self.root;
        let mask = proc_macro2::Literal::u64_unsuffixed((1u64 << self.prefix).wrapping_sub(1));
        let expected = proc_macro2::Literal::u64_unsuffixed(expected as u64);
        let read = if self.hoisted() {
            let ph = proc_macro2::Literal::u64_unsuffixed(self.prefix_physical());
            let (from, _) = wire_fns(self.endian);
            let (bits, wire_bytes) = (self.bits, self.wire_bytes);
            quote! {
                let __raw = #root::__private::BitWord::<#bits, #wire_bytes>::#from(wire).raw();
                let __got = (__raw >> #ph) as ::core::primitive::u64 & #mask;
            }
        } else {
            let Access::Window { offset, len, reg, shift, .. } = self.prefix_window() else {
                unreachable!("prefix_window builds a window")
            };
            let w = self.window_bytes(offset, len, reg, &quote!(wire));
            let shift = proc_macro2::Literal::u32_unsuffixed(shift);
            quote! {
                let __got = ((#w) >> #shift) as ::core::primitive::u64 & #mask;
            }
        };
        quote! {
            {
                #read
                if __got != #expected {
                    return ::core::result::Result::Err(#root::ParseError::Prefix {
                        expected: #expected,
                        got: __got,
                    });
                }
            }
        }
    }

    fn encode_body(&self) -> TokenStream {
        let root = self.root;
        let wire_bytes = self.wire_bytes;
        let hoisted = self.hoisted();
        let place = Place { buf: quote!(__out), buf_ref: quote!(__out), hoisted };
        let fields = self.items.iter().enumerate().map(|(i, item)| {
            let name = &item.ident;
            let value = quote! { self.#name };
            let writing = Writing { place: &place, idx: i, name, fits_guard: true };
            self.write(&item.body, item.phys_bit, &writing, &value)
        });
        if hoisted {
            let (from, to) = (quote!(zeroed), wire_fns(self.endian).1);
            let bits = self.bits;
            let prefix = self.prefix_value.map(|v| {
                let ph = self.prefix_physical();
                let v = proc_macro2::Literal::u128_unsuffixed(v);
                let ph = proc_macro2::Literal::u64_unsuffixed(ph);
                quote! { __bits |= (#v as ::core::primitive::u128) << #ph; }
            });
            quote! {
                let mut __bits: ::core::primitive::u128 = 0;
                #prefix
                #(#fields)*
                let mut __w = #root::__private::BitWord::<#bits, #wire_bytes>::#from();
                __w.set_raw(__bits);
                __w.#to()
            }
        } else {
            let magic = self.magic_writes();
            let checksums = self.checksum_writes();
            let guards = self.range_guards();
            let prefix = self.prefix_value.map(|v| {
                let access = self.prefix_window();
                let Access::Window { reg, .. } = &access else {
                    unreachable!("prefix_window builds a window")
                };
                let v = win_literal(*reg, v);
                self.store(&access, &place, quote!(#v))
            });
            quote! {
                let mut __out = [0u8; #wire_bytes];
                #guards
                #(#fields)*
                #prefix
                #magic
                #checksums
                __out
            }
        }
    }

    fn overlay_accessors(&self) -> (Vec<TokenStream>, Vec<TokenStream>) {
        let root = self.root;
        let mut checks = Vec::new();
        let mut accessors = Vec::new();
        for item in &self.items {
            let Body::Packed(p) = &item.body else {
                continue;
            };
            let (fname, fty, n) = (&item.ident, &p.ty, p.bits);
            for ov in &item.overlays {
                let getter = &ov.name;
                let setter = format_ident!("set_{}", crate::attr::name(&ov.name));
                let oty = &ov.ty;
                let primary_raw = match p.reading {
                    Reading::Bool | Reading::Codec => {
                        quote! { <#fty as #root::FieldCodec>::to_raw(&self.#fname) }
                    }
                    _ => quote! { self.#fname as ::core::primitive::u64 },
                };
                let store_raw = quote! { <#oty as #root::FieldCodec>::to_raw(&value) };
                let store = match p.reading {
                    Reading::Bool | Reading::Codec => {
                        quote! { <#fty as #root::FieldCodec>::from_raw(#store_raw) }
                    }
                    _ => quote! { (#store_raw) as #fty },
                };
                let msg = crate::check::assert_msg(format!(
                    "overlay `{}`: `<{} as FieldCodec>::BITS` does not match the field's \
                     #[bits({n})]",
                    ov.name,
                    quote!(#oty),
                ));
                let spanned = layline_codegen::__derive::respan(root, ov.span);
                checks.push(quote_spanned! {ov.span=>
                    const _: () = ::core::assert!(<#oty as #spanned::FieldCodec>::BITS == #n, #msg);
                });
                let getter_doc = format!("Read the `{fname}` bits as `{}`.", quote!(#oty));
                let setter_doc =
                    format!("Write the `{fname}` bits from a `{}` value.", quote!(#oty));
                accessors.push(quote! {
                    #[doc = #getter_doc]
                    #[must_use]
                    pub fn #getter(&self) -> #oty {
                        <#oty as #root::FieldCodec>::from_raw(#primary_raw)
                    }
                    #[doc = #setter_doc]
                    pub fn #setter(&mut self, value: #oty) {
                        self.#fname = #store;
                    }
                });
            }
        }
        (checks, accessors)
    }
}

impl Plan<'_> {
    fn width_checks(&self, body: &Body, name: &syn::Ident, out: &mut Vec<TokenStream>) {
        let root = self.root;
        match body {
            Body::Packed(Packed { ty, reading: Reading::Codec, bits, bits_span }) => {
                let msg = crate::check::assert_msg(format!(
                    "field `{name}`: #[bits({bits})] disagrees with `<{} as FieldCodec>::BITS`",
                    quote!(#ty),
                ));
                let root = layline_codegen::__derive::respan(root, *bits_span);
                out.push(quote_spanned! {*bits_span=>
                    const _: () = ::core::assert!(<#ty as #root::FieldCodec>::BITS == #bits, #msg);
                });
            }
            Body::Nested { ty, bytes, span } => {
                let msg = crate::check::assert_msg(format!(
                    "field `{name}`: #[bytes({bytes})] disagrees with `<{}>::WIRE_BYTES`",
                    quote!(#ty),
                ));
                let nests = crate::check::assert_msg(format!(
                    "field `{name}`: `{}` has a fallible `decode`, so a layout cannot nest it. \
                     Nest it in a `#[derive(Message)]`, or move its checks into this layout",
                    quote!(#ty),
                ));
                let root = self.root;
                let nesting = (!self.internal.0).then(|| {
                    quote_spanned! {*span=>
                        const _: () = ::core::assert!(!<#ty as #root::Layout>::DECODE_FALLIBLE, #nests);
                    }
                });
                out.push(quote_spanned! {*span=>
                    const _: () = ::core::assert!(<#ty as #root::Layout>::WIRE_BYTES == #bytes, #msg);
                    #nesting
                });
            }
            Body::Repeat { body, .. } => self.width_checks(body, name, out),
            Body::Packed(_) => {}
        }
    }
}

fn type_name(body: &Body) -> String {
    match body {
        Body::Packed(p) => crate::attr::spelled(&p.ty),
        Body::Nested { ty, .. } => crate::attr::spelled(ty),
        Body::Repeat { body, len } => format!("[{};{len}]", type_name(body)),
    }
}

struct Tables {
    types: Vec<TokenStream>,
    stated: Vec<TokenStream>,
    constants: Vec<TokenStream>,
    ranges: Vec<TokenStream>,
    covered: Vec<TokenStream>,
}

fn layout_impl(
    ident: &syn::Ident,
    t: &Tables,
    field_defs: &[TokenStream],
    prefix: u32,
    wire_bytes: usize,
    fallible: bool,
    root: &Root,
) -> TokenStream {
    let Tables { types, stated, constants, ranges, covered } = t;
    let name = ident.to_string();
    let decoded = if fallible {
        quote!(Self::decode(wire))
    } else {
        quote!(::core::result::Result::Ok(Self::decode(wire)))
    };
    // `DECODE_FALLIBLE` defaults to `false`.
    let fallible_const = fallible.then(|| {
        quote! { const DECODE_FALLIBLE: ::core::primitive::bool = true; }
    });
    // `PREFIX_BITS` defaults to zero.
    let prefix_bits = (prefix != 0).then(|| {
        let k = Literal::u32_unsuffixed(prefix);
        quote! { const PREFIX_BITS: ::core::primitive::u32 = #k; }
    });
    let types = (!types.is_empty()).then(|| {
        quote! {
            const TYPES: &'static [#root::table::TypeDef<'static>] = &[ #(#types),* ];
        }
    });
    let stated = (!stated.is_empty()).then(|| {
        quote! {
            const STATED: &'static [#root::table::StatedDef<'static>] = &[ #(#stated),* ];
        }
    });
    let constants = (!constants.is_empty()).then(|| {
        quote! {
            const CONSTANTS: &'static [#root::table::ConstDef<'static>] = &[ #(#constants),* ];
        }
    });
    let ranges = (!ranges.is_empty()).then(|| {
        quote! {
            const RANGES: &'static [#root::table::RangeDef<'static>] = &[ #(#ranges),* ];
        }
    });
    let covered = (!covered.is_empty()).then(|| {
        quote! {
            const COVERED: &'static [#root::table::CoverDef<'static>] = &[ #(#covered),* ];
        }
    });
    quote! {
        #[automatically_derived]
        impl #root::Layout for #ident {
            const NAME: &'static ::core::primitive::str = #name;

            const WIRE_BYTES: ::core::primitive::usize = #wire_bytes;

            const FIELDS: &'static [#root::table::FieldDef<'static>] = &[ #(#field_defs),* ];

            #fallible_const
            #prefix_bits
            #types
            #stated
            #constants
            #ranges
            #covered

            fn decode_slice(
                bytes: &[::core::primitive::u8],
            ) -> ::core::result::Result<Self, #root::ParseError> {
                match <&[::core::primitive::u8; #wire_bytes]>::try_from(bytes) {
                    ::core::result::Result::Ok(wire) => #decoded,
                    ::core::result::Result::Err(_) => {
                        ::core::result::Result::Err(#root::ParseError::Short {
                            need_bytes: #wire_bytes,
                            at: 0,
                            got_bytes: bytes.len(),
                        })
                    }
                }
            }

            fn encode_into<__B: #root::Buffer>(
                &self,
                out: &mut __B,
            ) -> ::core::result::Result<(), #root::Overflow> {
                #root::Buffer::push(out, &self.encode())
            }
        }
    }
}
