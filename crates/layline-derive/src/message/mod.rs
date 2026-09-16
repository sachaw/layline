//! `#[derive(Message)]` for structs, and for the enums a `#[switch]` chooses from.

mod attrs;
mod bits;
mod choice;
mod container;
mod field;
mod kind;
mod lower;
mod validate;

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Type};

use layline_codegen::{BitOrder, Collection, Coverage, Endian, Kind, Param, Root, Scalar};

use crate::claim::{ChecksumSpec, Ref};
use bits::bit_field;
use container::{MessageAttr, container_attr};
use field::msg_field;
use lower::lower;
use validate::{check_coverage, check_predicates, invalid, reference_site};

pub fn derive(input: &DeriveInput) -> syn::Result<TokenStream> {
    if matches!(input.data, Data::Enum(_)) {
        return choice::derive(input);
    }
    let parsed = parse(input)?;
    let (msg, checks) = lower(&parsed)?;

    check_coverage(&parsed)?;

    layline_codegen::validate(&layline_codegen::Item::Message(msg.clone()))
        .map_err(|why| invalid(&parsed, &why))?;
    check_predicates(&parsed)?;

    let parts = layline_codegen::__derive::message_parts(&msg, &parsed.root)
        .map_err(|why| syn::Error::new(parsed.ident.span(), format!("{why}")))?;

    let spanned = parts.checks.into_iter().map(|c| {
        let span = reference_site(&parsed, &c.field)
            .map(|(r, _, _)| r.span)
            .or_else(|| parsed.fields.iter().find(|f| f.name == c.field).map(|f| f.walk_span))
            .unwrap_or_else(|| parsed.ident.span());
        crate::check::respan(c.tokens, span)
    });
    let blocks = parts.blocks;
    let codec = parts.codec;
    Ok(quote! {
        #(#checks)*
        #(#spanned)*
        #blocks
        #codec
    })
}

struct Parsed {
    ident: syn::Ident,
    endian: Endian,
    /// `#[message(bits, order = ..)]`.
    bits: Option<BitOrder>,
    /// `#[message(needs(..))]`.
    needs: Vec<Param>,
    root: Root,
    fields: Vec<MsgField>,
}

struct MsgField {
    name: String,
    span: Span,
    /// The attribute that makes this field variable-size, else the field itself.
    walk_span: Span,
    body: Body,
    stated: Option<crate::claim::At>,
    /// `#[with(..)]` names, kept for error spans.
    with: Vec<Ref>,
}

enum Body {
    /// Part of a fixed-size block.
    Fixed { kind: Kind, claims: Claims },
    /// A `#[var]` or `#[text]` value, sized by reading it.
    Value {
        kind: Kind,
        /// The `#[len(n)]` for a length-prefixed string.
        len_ref: Option<Ref>,
    },
    Repeat {
        element: Kind,
        count: CountSpec,
        /// `#[seek(field)]`: where the elements start.
        at: Option<Ref>,
        /// Checks `#[bytes(N)]` against a nested element's size.
        assert: Option<Box<ElementSize>>,
        collection: Collection,
    },
    /// `#[seek(f)]`: one record at the byte offset in an earlier field.
    Placed {
        kind: Kind,
        /// The field with the byte offset.
        at: Ref,
        /// `Some` on an `Option<T>`: the field that is zero when the record is absent.
        absent: Option<PlacedAbsence>,
        /// Checks `#[bytes(N)]` against a nested layout's size.
        assert: Option<Box<ElementSize>>,
    },
    /// `#[switch(kind)]`: an arm chosen by an earlier value.
    Switch {
        on: SwitchOn,
        /// The `#[derive(Message)]` enum that declares the arms.
        ty: String,
        window: SwitchWindow,
    },
    /// `#[checksum(Crc, over = ..)]`: a check value over a range of bytes.
    Checksum {
        repr: Scalar,
        /// The `layline::Checksum` type, as written.
        algorithm: String,
        over: Coverage,
        /// The field the range starts at.
        from_ref: Option<Ref>,
        /// The field the range ends at or runs through.
        to_ref: Option<Ref>,
        /// Whether the end was written `..=`.
        inclusive: bool,
    },
    /// A field of a bit-addressed message.
    Bits {
        kind: Kind,
        /// The presence test of an `Option<T>` field.
        absent: Option<BitPresence>,
    },
    /// An optional value: `#[when(flags & 0x01)] x: Option<T>`.
    Opt {
        /// `None`: present when bytes remain.
        when: Option<WhenSpec>,
        kind: Kind,
        claims: Claims,
    },
}

/// The presence test of a bit-addressed field.
enum BitPresence {
    /// `#[present]`: a presence bit just before the field.
    Present,
    /// `#[when(f & MASK)]` or `#[when(f)]`: bits of an earlier field.
    When(WhenSpec),
}

#[derive(Default)]
struct Claims {
    range: Option<layline_codegen::Range>,
    magic: Option<crate::claim::Magic>,
}

enum PlacedAbsence {
    /// `#[seek(off)] x: Option<T>`: absent when the offset is zero.
    ZeroOffset,
    /// `#[when(len > 0)]`: absent when the length is zero.
    ZeroLength(Ref),
}

struct WhenSpec {
    by: Ref,
    test: WhenTest,
    /// The whole attribute's span.
    span: Span,
}

enum WhenTest {
    /// `field & MASK`.
    Mask(u64),
    /// `field`, a `bool`.
    Flag,
    /// `field > 0`, where the field is the record's length.
    Positive,
}

impl WhenSpec {
    fn presence(&self) -> layline_codegen::Presence {
        let field = self.by.name.clone();
        match self.test {
            WhenTest::Mask(mask) => layline_codegen::Presence::Mask { field, mask },
            WhenTest::Flag => layline_codegen::Presence::Flag { field },
            WhenTest::Positive => {
                unreachable!("`> 0` outside a placed record is refused at the field")
            }
        }
    }
}

/// The byte size of a switch.
enum SwitchWindow {
    /// Unsized: the arm reads what it needs.
    Open,
    /// `#[bytes(N)]`.
    Fixed(usize),
    /// `#[len(g)]`: read from an earlier field.
    Field { by: Ref, scale: u32, offset: i32, cap: Option<usize> },
}

impl SwitchWindow {
    fn field(&self) -> Option<&Ref> {
        match self {
            Self::Field { by, .. } => Some(by),
            Self::Open | Self::Fixed(_) => None,
        }
    }
}

enum SwitchOn {
    /// An earlier field, by name.
    Field(Ref),
    /// `#[switch(..)]`: the number of bytes remaining.
    BodyLength(Span),
}

impl SwitchOn {
    fn field(&self) -> Option<&Ref> {
        match self {
            Self::Field(r) => Some(r),
            Self::BodyLength(_) => None,
        }
    }
}

fn bound(body: &Body) -> Option<&Kind> {
    match body {
        Body::Fixed { kind, .. } | Body::Value { kind, .. } | Body::Bits { kind, absent: None } => {
            Some(kind)
        }
        Body::Bits { .. }
        | Body::Repeat { .. }
        | Body::Placed { .. }
        | Body::Switch { .. }
        | Body::Opt { .. }
        | Body::Checksum { .. } => None,
    }
}

fn sometimes_absent(body: &Body) -> bool {
    matches!(
        body,
        Body::Opt { .. }
            | Body::Placed { absent: Some(_), .. }
            | Body::Bits { absent: Some(_), .. }
    )
}

enum CountSpec {
    Field {
        by: Ref,
        scale: u32,
        offset: i32,
        cap: Option<usize>,
    },
    /// `#[count(n * n)]`: `n * n` elements.
    Squared {
        by: Ref,
        scale: u32,
        offset: i32,
        cap: Option<usize>,
    },
    /// `#[len(n)]`: the collection spans `n` bytes.
    Window {
        by: Ref,
        scale: u32,
        offset: i32,
        cap: Option<usize>,
    },
    /// `#[count(n)] #[stride(s)]`: `n` elements, `s` bytes apart.
    Strided {
        by: Ref,
        scale: u32,
        offset: i32,
        cap: Option<usize>,
        stride: StrideSpec,
    },
    Fill {
        cap: Option<usize>,
    },
    /// `#[until(mask = M)]`: elements up to and including the first that matches the mask.
    Terminated {
        mask: u8,
    },
    /// `#[until(b)]`: one-byte elements up to the first equal to `b`, which is consumed.
    Until {
        terminator: u8,
    },
}

impl CountSpec {
    fn spelling(&self) -> &'static str {
        match self {
            Self::Field { .. } | Self::Squared { .. } | Self::Strided { .. } => "#[count]",
            Self::Window { .. } => "#[len]",
            Self::Fill { .. } => "#[fill]",
            Self::Terminated { .. } | Self::Until { .. } => "#[until]",
        }
    }

    fn open_ended(&self) -> bool {
        !matches!(
            self,
            Self::Field { .. }
                | Self::Squared { .. }
                | Self::Window { .. }
                | Self::Strided { .. }
                | Self::Terminated { .. }
                | Self::Until { .. }
        )
    }

    fn by(&self) -> Option<&Ref> {
        match self {
            Self::Field { by, .. }
            | Self::Squared { by, .. }
            | Self::Window { by, .. }
            | Self::Strided { by, .. } => Some(by),
            Self::Fill { .. } | Self::Terminated { .. } | Self::Until { .. } => None,
        }
    }

    fn stride(&self) -> Option<&Ref> {
        match self {
            Self::Strided { stride, .. } => Some(&stride.by),
            _ => None,
        }
    }
}

struct StrideSpec {
    by: Ref,
    scale: u32,
    offset: i32,
    span: Span,
}

struct ElementSize {
    ty: Type,
    bytes: usize,
    span: Span,
}

fn parse(input: &DeriveInput) -> syn::Result<Parsed> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            format!(
                "`{}`: #[derive(Message)] does not support generics. Remove the type parameters",
                input.ident
            ),
        ));
    }

    let MessageAttr { endian, root, closed, bits, needs } = container_attr(input)?;
    if let (Some((_, span)), Some(_)) = (&needs, &bits) {
        return Err(syn::Error::new(
            *span,
            format!(
                "`{}`: a bit-addressed message cannot take `needs`. Remove `bits` or `needs`",
                input.ident
            ),
        ));
    }
    if let Some(span) = closed {
        return Err(syn::Error::new(
            span,
            format!(
                "`{}`: `closed` applies only to enums. \
                 Move it to the enum that the `#[switch]` names",
                input.ident
            ),
        ));
    }

    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            format!(
                "`{}`: #[derive(Message)] applies only to structs and enums. \
                 Use `#[derive(Dispatch)]` for an id-keyed catalogue",
                input.ident
            ),
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new(
            input.ident.span(),
            format!("`{}`: #[derive(Message)] requires named fields", input.ident),
        ));
    };
    if named.named.is_empty() {
        return Err(syn::Error::new(
            input.ident.span(),
            format!("`{}`: a message must have at least one field", input.ident),
        ));
    }

    let bits = bits.map(|(order, _)| order);
    let fields = match bits {
        Some(_) => named.named.iter().map(bit_field).collect::<syn::Result<Vec<_>>>()?,
        None => named.named.iter().map(msg_field).collect::<syn::Result<Vec<_>>>()?,
    };
    let needs = needs.map(|(params, _)| params).unwrap_or_default();
    Ok(Parsed { ident: input.ident.clone(), endian, bits, needs, root, fields })
}

const REPEAT_ATTRS: &str = "`#[count(n)]`, `#[fill]`, `#[until(b)]`, or `#[until(mask = M)]`";

const TEXT_LENS: &str = "`#[len(n)]`, `#[until(0)]`, `#[bytes(N)]`, or `#[fill]`";

struct FieldAttrs {
    kind: Option<(KindWord, Span)>,
    count: Option<(CountSpec, Span)>,
    /// `#[stride(s)]`, used with `#[count]`.
    stride: Option<StrideSpec>,
    at: Option<crate::claim::At>,
    seek: Option<Ref>,
    bits: Option<(u32, Span)>,
    bytes: Option<(usize, Span)>,
    until: Option<(UntilSpec, Span)>,
    when: Option<WhenSpec>,
    /// `#[with(f, g)]`: arguments for a nested message's parameters.
    with: Option<(Vec<Ref>, Span)>,
    range: Option<crate::claim::Range>,
    magic: Option<crate::claim::Magic>,
}

enum KindWord {
    /// `#[text]`: a string, through the named `TextCodec` or UTF-8.
    Text(Option<String>),
    /// `#[var]`: a self-delimiting value.
    Var,
    /// `#[message]`: a nested `#[derive(Message)]` type.
    Message,
    /// `#[switch]`: an arm chosen by an earlier value.
    Switch(SwitchOn),
    /// `#[checksum]`: a value computed from other bytes of the message.
    Checksum(ChecksumSpec),
}

impl KindWord {
    fn spelling(&self) -> &'static str {
        match self {
            KindWord::Text(_) => "#[text]",
            KindWord::Var => "#[var]",
            KindWord::Message => "#[message]",
            KindWord::Switch(_) => "#[switch]",
            KindWord::Checksum(_) => "#[checksum]",
        }
    }

    fn duplicate_note(&self) -> &'static str {
        match self {
            KindWord::Checksum(_) => " Use one `#[checksum]` field per range",
            _ => "",
        }
    }
}

/// `#[until(b)]` or `#[until(mask = M)]`.
#[derive(Debug, Clone, Copy)]
enum UntilSpec {
    /// The run ends at the first byte equal to this, which is consumed.
    Byte(u8),
    /// The run ends after the first element whose first byte matches these bits.
    Mask(u8),
}
