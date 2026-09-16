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
                #[must_use]
                pub fn #name(&self) -> #ty {
                    #read
                }
                #[doc = #set_doc]
                pub fn #setter(&mut self, value: #ty) {
                    #write
                }
            }
        });

        let names: Vec<&syn::Ident> = self.items.iter().map(|item| &item.ident).collect();
        view_shell(
            &self.vis,
            owned,
            &view,
            self.wire_bytes,
            &names,
            quote! { #(#accessors)* },
            self.root,
        )
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
    let decode_doc = "Decode every field into the owned type.";
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

            #[doc = #decode_doc]
            #[must_use]
            pub fn decode(&self) -> #owned {
                #owned::decode(&self.0)
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
