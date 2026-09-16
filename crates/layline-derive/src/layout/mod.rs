//! `#[derive(Layout)]` for fixed-size records.

mod access;
mod asserts;
mod declared;
mod emit;
mod field;
mod misplaced;
mod parse;
mod solve;
mod view;

use proc_macro2::{Span, TokenStream};
use quote::quote;

use layline_codegen::{Endian, Root};

use crate::claim::At;
use asserts::Asserts;
use declared::{Internal, Overlay, Prim};

pub fn derive(input: &syn::DeriveInput) -> syn::Result<TokenStream> {
    let parsed = parse::parse(input)?;
    expand(parsed.declared, &parsed.root, parsed.internal)
}

fn expand(
    declared: declared::Declared,
    root: &Root,
    internal: Internal,
) -> syn::Result<TokenStream> {
    let mut plan = declared::plan(declared, root);
    plan.internal = internal;
    plan.solve()?;
    plan.resolve_assertions()?;
    Ok(plan.emit())
}

struct Plan<'a> {
    ident: syn::Ident,
    vis: syn::Visibility,
    root: &'a Root,
    grid: layline_codegen::Container,
    endian: Endian,
    /// Always `wire_bytes * 8`.
    bits: u32,
    wire_bytes: usize,
    /// Low bits reserved for a dispatcher's id.
    prefix: u32,
    prefix_value: Option<u128>,
    view: bool,
    items: Vec<Item>,
    asserts: Asserts,
    internal: Internal,
}

struct Item {
    ident: syn::Ident,
    span: Span,
    at: Option<At>,
    body: Body,
    overlays: Vec<Overlay>,
    phys_bit: u64,
    magic: Option<crate::claim::Magic>,
    check: Option<crate::claim::ChecksumSpec>,
    range: Option<crate::claim::Range>,
}

impl Body {
    /// Arrays of codec fields become arrays of scalars, since `field_rows` reads only width and dimensions.
    fn kind(&self) -> layline_codegen::Kind {
        use layline_codegen::{Kind, Scalar};
        let mut dims = Vec::new();
        let mut body = self;
        while let Body::Repeat { body: inner, len } = body {
            dims.push(*len);
            body = inner;
        }
        match body {
            Body::Packed(p) => {
                let scalar = match p.reading {
                    Reading::Int => Scalar::I(u64::from(p.bits)),
                    _ => Scalar::U(u64::from(p.bits)),
                };
                if dims.is_empty() {
                    match p.reading {
                        Reading::Codec => Kind::Codec {
                            ty: quote!(#{ &p.ty }).to_string(),
                            bits: u64::from(p.bits),
                        },
                        _ => Kind::Scalar(scalar),
                    }
                } else {
                    Kind::Array(scalar, dims)
                }
            }
            Body::Nested { ty, bytes, .. } => {
                let ty = quote!(#ty).to_string();
                if dims.is_empty() {
                    Kind::Nested { ty, bytes: *bytes }
                } else {
                    Kind::NestedArray { ty, bytes: *bytes, len: dims.iter().product() }
                }
            }
            Body::Repeat { .. } => unreachable!("the loop above peeled every repeat"),
        }
    }
}

enum Body {
    /// A bit field or a scalar.
    Packed(Packed),
    Nested {
        ty: syn::Type,
        bytes: usize,
        span: Span,
    },
    /// `len` consecutive copies of a body.
    Repeat {
        body: Box<Body>,
        len: usize,
    },
}

struct Packed {
    ty: syn::Type,
    reading: Reading,
    bits: u32,
    bits_span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    Uint(u32),
    /// Sign-extended from the declared width.
    Int,
    /// One bit, through `FieldCodec`.
    Bool,
    Codec,
    /// A whole scalar at its own width.
    Native(Prim),
}

impl Body {
    fn width(&self) -> u64 {
        match self {
            Body::Packed(p) => u64::from(p.bits),
            Body::Nested { bytes, .. } => *bytes as u64 * 8,
            Body::Repeat { body, len } => body.width() * *len as u64,
        }
    }

    fn element_bits(&self) -> u64 {
        match self {
            Body::Repeat { body, .. } => body.width(),
            other => other.width(),
        }
    }

    fn describe(&self) -> String {
        match self {
            Body::Nested { .. } => "a nested layout".into(),
            Body::Repeat { body, .. } => match &**body {
                Body::Packed(Packed { reading: Reading::Native(s), .. }) => {
                    format!("a [{}; N] array", s.ident())
                }
                _ => "an array".into(),
            },
            Body::Packed(_) => "a field".into(),
        }
    }
}

impl Packed {
    fn wide(&self) -> bool {
        matches!(self.reading, Reading::Uint(prim) if prim > 64 || self.bits > 64)
    }
}

impl Plan<'_> {
    fn interior_boundary(&self) -> Option<u64> {
        self.grid.interior_boundary()
    }

    fn prefix_physical(&self) -> u64 {
        use layline_codegen::{Kind, Scalar};
        self.grid.physical(&Kind::Scalar(Scalar::U(u64::from(self.prefix))), 0)
    }

    fn unit(&self) -> (u64, &'static str) {
        if self.grid.is_bit_addressed() { (1, "bit") } else { (8, "byte") }
    }
}

fn body_ty(body: &Body) -> TokenStream {
    match body {
        Body::Packed(p) => {
            let ty = &p.ty;
            quote!(#ty)
        }
        Body::Nested { ty, .. } => quote!(#ty),
        Body::Repeat { body, len } => {
            let elem = body_ty(body);
            quote!([#elem; #len])
        }
    }
}
