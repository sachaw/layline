//! Parsed layout declarations, and their conversion to a `Plan`.

use proc_macro2::Span;
use syn::Type;

use layline_codegen::{BitOrder, Endian, Root};

use super::asserts::Asserts;
use super::{Body, Item, Packed, Plan, Reading};
use crate::claim::{At, Magic, Range};

pub enum Declared {
    /// `bits = N`: bit fields over a whole number of bytes.
    Word(WordLayout),
    /// `bytes = N`: scalars at whole-byte offsets.
    Bytes(ByteLayout),
    /// `words = N`: 16-bit words holding bit fields.
    Words(WordsLayout),
}

pub struct WordsLayout {
    pub ident: syn::Ident,
    pub vis: syn::Visibility,
    pub endian: Endian,
    pub order: BitOrder,
    pub view: bool,
    /// May be zero, for a header-only message.
    pub words: usize,
    pub fields: Vec<WordsField>,
}

pub struct WordsField {
    pub ident: syn::Ident,
    pub kind: WordsKind,
    pub span: Span,
    pub at: Option<At>,
    /// `#[overlay(name: Type)]`, on bit fields only.
    pub overlays: Vec<Overlay>,
    pub range: Option<Range>,
}

pub enum WordsKind {
    /// `#[bits(N)]`, N ≤ 16, inside one word.
    Bits { ty: Type, wk: WordKind, bits: u32, bits_span: Span },
    /// A `#[bytes(N)]` nested layout, word-aligned.
    Nested { ty: Type, bytes: usize, span: Span },
    /// `[u16; K]`, word-aligned.
    ArrayU16 { len: usize },
    /// `[u8; K]`, byte-aligned.
    ArrayU8 { len: usize },
}

pub struct WordLayout {
    pub ident: syn::Ident,
    pub vis: syn::Visibility,
    pub endian: Endian,
    pub order: BitOrder,
    /// A multiple of 8, and the whole wire size.
    pub bits: u32,
    /// Low bits reserved for a dispatcher. Fields cover `prefix..bits`, and encode leaves these zero.
    pub prefix: u32,
    /// Encode writes this value into the prefix, and decode refuses any other.
    pub prefix_value: Option<u128>,
    pub view: bool,
    pub fields: Vec<WordField>,
}

pub struct ByteLayout {
    pub ident: syn::Ident,
    pub vis: syn::Visibility,
    pub endian: Endian,
    pub wire_bytes: usize,
    pub view: bool,
    pub fields: Vec<ByteField>,
}

/// The Rust type a bit field is read as.
pub enum WordKind {
    Uint(u32),
    /// Sign-extended from the declared width.
    Int(u32),
    Bool,
    /// A `FieldCodec` type. A const assertion checks the width against its `BITS`.
    Codec,
}

pub struct WordField {
    pub ident: syn::Ident,
    pub ty: Type,
    pub kind: WordKind,
    pub bits: u32,
    pub bits_span: Span,
    pub at: Option<At>,
    pub overlays: Vec<Overlay>,
    pub range: Option<Range>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prim {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
}

impl Prim {
    pub(super) fn of(ident: &syn::Ident) -> Option<Self> {
        Some(match ident.to_string().as_str() {
            "u8" => Self::U8,
            "u16" => Self::U16,
            "u32" => Self::U32,
            "u64" => Self::U64,
            "i8" => Self::I8,
            "i16" => Self::I16,
            "i32" => Self::I32,
            "i64" => Self::I64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            _ => return None,
        })
    }

    pub fn width(self) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::I64 | Self::F64 => 8,
        }
    }

    pub fn ident(self) -> syn::Ident {
        let name = match self {
            Self::U8 => "u8",
            Self::U16 => "u16",
            Self::U32 => "u32",
            Self::U64 => "u64",
            Self::I8 => "i8",
            Self::I16 => "i16",
            Self::I32 => "i32",
            Self::I64 => "i64",
            Self::F32 => "f32",
            Self::F64 => "f64",
        };
        syn::Ident::new(name, Span::call_site())
    }
}

pub enum ByteKind {
    Scalar(Prim),
    /// A scalar array, dimensions outermost first.
    Array(Prim, Vec<usize>),
    /// A `FieldCodec` type stored as an 8, 16, 32 or 64-bit scalar.
    Codec(Box<CodecField>),
    /// A `#[bytes(N)]` nested layout. A const assertion checks N against its `WIRE_BYTES`.
    Nested(Box<NestedField>),
    /// `[Child; K]`, where `#[bytes(N)]` is one element's size.
    NestedArray(Box<NestedField>, usize),
    /// `[CodecType; K]`, where `#[codec(N)]` is one element's scalar width.
    CodecArray(Box<CodecField>, usize),
}

pub struct CodecField {
    pub ty: Type,
    pub bits: u32,
    pub bits_span: Span,
}

pub struct NestedField {
    pub ty: Type,
    pub bytes: usize,
    pub span: Span,
}

pub struct ByteField {
    pub ident: syn::Ident,
    pub kind: ByteKind,
    pub span: Span,
    pub at: Option<At>,
    pub magic: Option<Magic>,
    pub check: Option<crate::claim::ChecksumSpec>,
    pub range: Option<Range>,
}

pub struct Overlay {
    pub name: syn::Ident,
    pub ty: Type,
    pub span: Span,
}

/// `#[layout(.., internal)]` makes `decode` always fallible.
///
/// The derive cannot tell whether a nested type's decode can fail. Only generated code uses this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Internal(pub bool);

pub(super) fn plan(declared: Declared, root: &Root) -> Plan<'_> {
    match declared {
        Declared::Word(word) => plan_word(word, root),
        Declared::Bytes(bytes) => plan_bytes(bytes, root),
        Declared::Words(words) => plan_words(words, root),
    }
}

fn plan_word(l: WordLayout, root: &Root) -> Plan<'_> {
    let items = l
        .fields
        .into_iter()
        .map(|f| Item {
            ident: f.ident,
            span: f.bits_span,
            at: f.at,
            body: Body::Packed(Packed {
                ty: f.ty,
                reading: reading_of(&f.kind),
                bits: f.bits,
                bits_span: f.bits_span,
            }),
            overlays: f.overlays,
            phys_bit: 0,
            magic: None,
            check: None,
            range: f.range,
        })
        .collect();
    Plan {
        ident: l.ident,
        vis: l.vis,
        root,
        grid: layline_codegen::Container::Word {
            bits: u64::from(l.bits),
            prefix: u64::from(l.prefix),
            prefix_value: l.prefix_value,
            endian: l.endian,
            order: l.order,
        },
        endian: l.endian,
        bits: l.bits,
        wire_bytes: (l.bits / 8) as usize,
        prefix: l.prefix,
        prefix_value: l.prefix_value,
        view: l.view,
        items,
        asserts: Asserts::default(),
        internal: Internal(false),
    }
}

fn plan_words(l: WordsLayout, root: &Root) -> Plan<'_> {
    let items = l
        .fields
        .into_iter()
        .map(|f| {
            let span = f.span;
            let body = match f.kind {
                WordsKind::Bits { ty, wk, bits, bits_span } => {
                    Body::Packed(Packed { ty, reading: reading_of(&wk), bits, bits_span })
                }
                WordsKind::Nested { ty, bytes, span } => Body::Nested { ty, bytes, span },
                WordsKind::ArrayU16 { len } => {
                    Body::Repeat { body: Box::new(native(Prim::U16, span)), len }
                }
                WordsKind::ArrayU8 { len } => {
                    Body::Repeat { body: Box::new(native(Prim::U8, span)), len }
                }
            };
            Item {
                ident: f.ident,
                span,
                at: f.at,
                body,
                overlays: f.overlays,
                phys_bit: 0,
                magic: None,
                check: None,
                range: f.range,
            }
        })
        .collect();
    Plan {
        ident: l.ident,
        vis: l.vis,
        root,
        grid: layline_codegen::Container::Words {
            words: l.words,
            endian: l.endian,
            order: l.order,
        },
        endian: l.endian,
        bits: (l.words * 16) as u32,
        wire_bytes: l.words * 2,
        prefix: 0,
        prefix_value: None,
        view: l.view,
        items,
        asserts: Asserts::default(),
        internal: Internal(false),
    }
}

fn plan_bytes(l: ByteLayout, root: &Root) -> Plan<'_> {
    let items = l
        .fields
        .into_iter()
        .map(|f| {
            let span = f.span;
            let codec = |c: Box<super::declared::CodecField>| {
                Body::Packed(Packed {
                    ty: c.ty,
                    reading: Reading::Codec,
                    bits: c.bits,
                    bits_span: c.bits_span,
                })
            };
            let nested = |n: Box<super::declared::NestedField>| Body::Nested {
                ty: n.ty,
                bytes: n.bytes,
                span: n.span,
            };
            let body = match f.kind {
                ByteKind::Scalar(s) => native(s, span),
                ByteKind::Array(s, dims) => dims
                    .iter()
                    .rev()
                    .fold(native(s, span), |body, &len| Body::Repeat { body: Box::new(body), len }),
                ByteKind::Codec(c) => codec(c),
                ByteKind::Nested(n) => nested(n),
                ByteKind::NestedArray(n, len) => Body::Repeat { body: Box::new(nested(n)), len },
                ByteKind::CodecArray(c, len) => Body::Repeat { body: Box::new(codec(c)), len },
            };
            Item {
                ident: f.ident,
                span,
                at: f.at,
                body,
                overlays: Vec::new(),
                phys_bit: 0,
                magic: f.magic,
                check: f.check,
                range: f.range,
            }
        })
        .collect();
    Plan {
        ident: l.ident,
        vis: l.vis,
        root,
        grid: layline_codegen::Container::Bytes { bytes: l.wire_bytes, endian: l.endian },
        endian: l.endian,
        bits: (l.wire_bytes * 8) as u32,
        wire_bytes: l.wire_bytes,
        prefix: 0,
        prefix_value: None,
        view: l.view,
        items,
        asserts: Asserts::default(),
        internal: Internal(false),
    }
}

fn reading_of(wk: &WordKind) -> Reading {
    match wk {
        WordKind::Uint(prim) => Reading::Uint(*prim),
        WordKind::Int(_) => Reading::Int,
        WordKind::Bool => Reading::Bool,
        WordKind::Codec => Reading::Codec,
    }
}

fn native(s: Prim, span: Span) -> Body {
    let ident = s.ident();
    Body::Packed(Packed {
        ty: syn::parse_quote!(#ident),
        reading: Reading::Native(s),
        bits: (s.width() * 8) as u32,
        bits_span: span,
    })
}
