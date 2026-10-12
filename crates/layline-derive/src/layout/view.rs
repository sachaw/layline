//! `<Name>View`, which reads and writes a record in place over its wire bytes.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use layline_codegen::Root;

use super::access::{Place, Writing};
use super::{Plan, body_ty};

impl Plan<'_> {
    pub(super) fn view_type(&self) -> TokenStream {
        let owned = &self.ident;
        let view = format_ident!("{}View", self.ident);
        let place = Place { buf: quote!(self.0), buf_ref: quote!(&self.0), hoisted: false };

        let accessors = self.items.iter().enumerate().map(|(i, item)| {
            let name = &item.ident;
            let setter = format_ident!("set_{}", name);
            let ty = body_ty(&item.body);
            let read = self.read(&item.body, item.phys_bit, i, &place);
            let writing = Writing { place: &place, idx: i, name, fits_guard: false };
            let write = self.write(&item.body, item.phys_bit, &writing, &quote!(value));
            let get_doc = format!("Read `{name}`.");
            let set_doc = format!("Write `{name}` in place.");
            quote! {
                #[doc = #get_doc]
                #[inline]
                #[must_use]
                pub fn #name(&self) -> #ty {
                    #read
                }
                #[doc = #set_doc]
                #[inline]
                pub fn #setter(&mut self, value: #ty) {
                    #write
                }
            }
        });

        let names: Vec<&syn::Ident> = self.items.iter().map(|item| &item.ident).collect();
        let checks = self.view_checks(&place);
        view_shell(
            &self.vis,
            owned,
            &view,
            self.wire_bytes,
            &names,
            quote! { #(#accessors)* #checks },
            self.root,
        )
    }

    /// `decode`, `verify`, `checked`, `checked_mut` and, for a record with a magic or a checksum,
    /// `seal`.
    fn view_checks(&self, place: &Place) -> TokenStream {
        let root = self.root;
        let owned = &self.ident;
        let wire_bytes = self.wire_bytes;
        let wire = quote!(wire);
        let read_wire = Place { buf: wire.clone(), buf_ref: wire.clone(), hoisted: false };
        let field_of = |name: &syn::Ident| {
            let (i, item) = self
                .items
                .iter()
                .enumerate()
                .find(|(_, item)| &item.ident == name)
                .expect("a check names a field of this layout");
            self.read(&item.body, item.phys_bit, i, &read_wire)
        };
        let magic = self.magic_checks(&quote!(#owned));
        let prefix = self.prefix_check(false);
        let sums = self.check_verify(&field_of);
        let checked_decode = self.asserts.any() || self.internal.0 || self.prefix_value.is_some();
        let decoded = if checked_decode {
            let fields = self.items.iter().enumerate().map(|(i, item)| {
                let name = &item.ident;
                let value = self.read(&item.body, item.phys_bit, i, place);
                quote! { #name: #value, }
            });
            quote! { #owned { #(#fields)* } }
        } else {
            quote! { #owned::decode(&self.0) }
        };
        let seal = (self.asserts.stamped() || self.prefix_value.is_some()).then(|| {
            let out = Place { buf: quote!(__out), buf_ref: quote!(__out), hoisted: false };
            let prefix = self.prefix_value.map(|v| {
                let access = self.prefix_window();
                let super::access::Access::Window { reg, .. } = &access else {
                    unreachable!("prefix_window builds a window")
                };
                let v = super::access::win_literal(*reg, v);
                self.store(&access, &out, quote!(#v))
            });
            let magic = self.magic_writes();
            let sums = self.checksum_writes();
            quote! {
                /// Write what `encode` writes that no field holds, over the bytes as they are now:
                /// the prefix, each magic, then each checksum in declaration order. Call it after
                /// the setters.
                #[inline]
                pub fn seal(&mut self) {
                    let __out = &mut self.0;
                    #prefix
                    #magic
                    #sums
                }
            }
        });
        quote! {
            /// Decode every field into the owned type, without the record's checks.
            ///
            /// [`verify`](Self::verify) runs them.
            #[must_use]
            pub fn decode(&self) -> #owned {
                #decoded
            }

            /// Run the checks `decode` on the owned type runs, over the bytes as they are now.
            ///
            /// # Errors
            ///
            /// The first check the bytes fail.
            #[inline]
            pub fn verify(&self) -> ::core::result::Result<(), #root::ParseError> {
                let wire = &self.0;
                let _ = wire;
                #magic
                #prefix
                #sums
                ::core::result::Result::Ok(())
            }

            /// View borrowed wire bytes that pass [`verify`](Self::verify).
            ///
            /// # Errors
            ///
            /// The first check the bytes fail.
            #[inline]
            pub fn checked(
                wire: &[::core::primitive::u8; #wire_bytes],
            ) -> ::core::result::Result<&Self, #root::ParseError> {
                let view = Self::from_wire(wire);
                view.verify()?;
                ::core::result::Result::Ok(view)
            }

            /// A mutable [`checked`](Self::checked).
            ///
            /// # Errors
            ///
            /// The first check the bytes fail.
            #[inline]
            pub fn checked_mut(
                wire: &mut [::core::primitive::u8; #wire_bytes],
            ) -> ::core::result::Result<&mut Self, #root::ParseError> {
                let view = Self::from_wire_mut(wire);
                view.verify()?;
                ::core::result::Result::Ok(view)
            }

            #seal
        }
    }
}

fn view_shell(
    vis: &syn::Visibility,
    owned: &syn::Ident,
    view: &syn::Ident,
    wire_bytes: usize,
    field_names: &[&syn::Ident],
    accessors: TokenStream,
    root: &Root,
) -> TokenStream {
    let view_str = view.to_string();
    let debug_fields = field_names.iter().map(|name| {
        let name_str = name.to_string();
        quote! { .field(#name_str, &self.#name()) }
    });
    let doc = format!(
        "[`{owned}`] viewed in place over its wire bytes.\n\n\
         Accessors read or write one field without decoding the rest.",
    );
    quote! {
        #[doc = #doc]
        #[repr(transparent)]
        #[derive(Clone, Copy, PartialEq, Eq)]
        #vis struct #view([::core::primitive::u8; #wire_bytes]);

        #[automatically_derived]
        impl #view {
            /// Wrap owned wire bytes.
            #[must_use]
            pub const fn new(wire: [::core::primitive::u8; #wire_bytes]) -> Self {
                Self(wire)
            }

            /// All bytes zero.
            #[must_use]
            pub const fn zeroed() -> Self {
                Self([0u8; #wire_bytes])
            }

            /// View borrowed wire bytes.
            #[must_use]
            pub fn from_wire(wire: &[::core::primitive::u8; #wire_bytes]) -> &Self {
                // SAFETY: `Self` is `#[repr(transparent)]` over `[u8; WIRE_BYTES]`.
                unsafe { &*::core::ptr::from_ref(wire).cast::<Self>() }
            }

            /// View borrowed wire bytes mutably. Writes go to the caller's buffer.
            #[must_use]
            pub fn from_wire_mut(wire: &mut [::core::primitive::u8; #wire_bytes]) -> &mut Self {
                // SAFETY: as in `from_wire`.
                unsafe { &mut *::core::ptr::from_mut(wire).cast::<Self>() }
            }

            /// The wire bytes.
            #[must_use]
            pub const fn as_wire(&self) -> &[::core::primitive::u8; #wire_bytes] {
                &self.0
            }

            #accessors
        }

        #[automatically_derived]
        impl #root::View for #view {
            type Owned = #owned;

            fn from_slice(bytes: &[::core::primitive::u8]) -> ::core::option::Option<&Self> {
                match <&[::core::primitive::u8; #wire_bytes]>::try_from(bytes) {
                    ::core::result::Result::Ok(wire) => {
                        ::core::option::Option::Some(Self::from_wire(wire))
                    }
                    ::core::result::Result::Err(_) => ::core::option::Option::None,
                }
            }

            fn from_slice_mut(
                bytes: &mut [::core::primitive::u8],
            ) -> ::core::option::Option<&mut Self> {
                match <&mut [::core::primitive::u8; #wire_bytes]>::try_from(bytes) {
                    ::core::result::Result::Ok(wire) => {
                        ::core::option::Option::Some(Self::from_wire_mut(wire))
                    }
                    ::core::result::Result::Err(_) => ::core::option::Option::None,
                }
            }

            fn as_wire(&self) -> &[::core::primitive::u8] {
                let wire: &[::core::primitive::u8; #wire_bytes] = #view::as_wire(self);
                wire
            }

            fn decode(&self) -> Self::Owned {
                #view::decode(self)
            }

            fn verify(&self) -> ::core::result::Result<(), #root::ParseError> {
                #view::verify(self)
            }
        }

        #[automatically_derived]
        impl ::core::convert::From<&#owned> for #view {
            fn from(owned: &#owned) -> Self {
                Self(owned.encode())
            }
        }

        #[automatically_derived]
        impl ::core::fmt::Debug for #view {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.debug_struct(#view_str)
                    #(#debug_fields)*
                    .finish()
            }
        }
    }
}
