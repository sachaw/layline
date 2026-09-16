//! A bit-addressed message: a bit cursor over the body, one read per field, zero padding to the byte.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use super::body::Own;
use super::cursor::too_deep;
use super::patch::{Target, fits};
use super::table::{row_tokens, span_of_kind};
use super::{Check, MessageParts};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{emit_message_field, kind_ty, path, row, scalar_ty};
use crate::{BitOrder, Error, Field, Kind, MessageDef, Presence, Root, Scalar, Segment};

/// A value in encode: by reference and by value.
type Held<'a> = (&'a TokenStream, &'a TokenStream);

/// Convert the cursor's `u64` to one field's type.
fn bit_decode(kind: &Kind, raw: &TokenStream, root: &Root) -> TokenStream {
    let Prelude { i64, .. } = root.prelude();
    match kind {
        Kind::Scalar(Scalar::U(64)) => quote!(#raw),
        Kind::Scalar(s @ Scalar::U(_)) => {
            let ty = scalar_ty(*s);
            quote!(#raw as #ty)
        }
        Kind::Scalar(s @ Scalar::I(n)) => {
            let ty = scalar_ty(*s);
            let wide = if *n == 64 {
                quote!((#raw as #i64))
            } else {
                let shift = Literal::u64_unsuffixed(64 - *n);
                quote!((((#raw << #shift) as #i64) >> #shift))
            };
            if *n == 64 { wide } else { quote!(#wide as #ty) }
        }
        Kind::Scalar(Scalar::F32) => {
            let f32 = root.prelude().primitive("f32");
            let u32 = root.prelude().primitive("u32");
            quote!(<#f32>::from_bits(#raw as #u32))
        }
        Kind::Scalar(Scalar::F64) => {
            let f64 = root.prelude().primitive("f64");
            quote!(<#f64>::from_bits(#raw))
        }
        other => {
            let ty = kind_ty(other, root);
            quote!(<#ty as #root::FieldCodec>::from_raw(#raw))
        }
    }
}

/// Convert one field to the `u64` the cursor writes.
fn bit_encode(kind: &Kind, (by_ref, by_value): Held<'_>, root: &Root) -> TokenStream {
    let Prelude { i64, u64, .. } = root.prelude();
    match kind {
        Kind::Scalar(Scalar::U(64)) => quote!(#by_value),
        Kind::Scalar(Scalar::U(_)) => quote!(#by_value as #u64),
        Kind::Scalar(Scalar::I(64)) => quote!(#by_value as #u64),
        Kind::Scalar(Scalar::I(_)) => quote!((#by_value as #i64) as #u64),
        Kind::Scalar(Scalar::F32) => quote!(#by_value.to_bits() as #u64),
        Kind::Scalar(Scalar::F64) => quote!(#by_value.to_bits()),
        other => {
            let ty = kind_ty(other, root);
            quote!(<#ty as #root::FieldCodec>::to_raw(#by_ref))
        }
    }
}

/// Assert that a codec field's declared width equals the codec's `BITS`.
fn bit_codec_check(owner: &str, f: &Field, root: &Root) -> Option<Check> {
    let Kind::Codec { ty, bits } = &f.kind else {
        return None;
    };
    let path = path(ty);
    let bits = Literal::u64_unsuffixed(*bits);
    let msg = format!(
        "{owner}: the width of field `{}` differs from `<{ty} as FieldCodec>::BITS`. \
         Declare the codec's width",
        f.name
    );
    Some(Check {
        field: f.name.clone(),
        tokens: quote! {
            const _: () = ::core::assert!(
                <#path as #root::FieldCodec>::BITS as ::core::primitive::u64 == #bits,
                #msg
            );
        },
    })
}

/// Generate a bit-addressed message.
///
/// Encode writes presence bits and padding. Decode reads the bits and skips the padding.
pub(super) fn bits_parts(
    m: &MessageDef,
    derives: &[crate::Derive],
    root: &Root,
) -> Result<(MessageParts, Vec<row::SegmentDef>), Error> {
    let Prelude { ok, some, none, result, u8, u32, usize, bool, .. } = root.prelude();
    let name = ident(&m.name);
    let msb = matches!(m.bits, Some(BitOrder::Msb));

    let skip = crate::walk::tokens::serde_skip(derives, &m.derives);
    let declared = bit_declared(m)?;
    let flags = bit_flags(&declared);

    let mut fields = TokenStream::new();
    let mut checks: Vec<Check> = Vec::new();
    let mut parse = TokenStream::new();
    let mut build = TokenStream::new();
    let mut writes = TokenStream::new();
    let mut rows: Vec<row::SegmentDef> = Vec::new();
    // Bits since the last optional or variable-size row, or since the body's start.
    let mut at = 0u64;
    let mut segment: Option<String> = None;

    for (f, presence) in &declared {
        let width = f.kind.width();
        let ident = ident(&f.name);
        let optional = !matches!(presence, BitPresence::Always);
        fields.extend(emit_message_field(f, optional, root, &skip));
        build.extend(quote!(#ident,));
        checks.extend(bit_codec_check(&m.name, f, root));

        let read = bit_read(&f.name, &f.kind, msb, root);
        let field = (f.name.as_str(), &f.kind);
        let by_ref = Own::Struct.by_ref(&f.name);
        let write_held = bit_write(field, (&by_ref, &Own::Struct.get(&f.name)), msb, root);
        let write_inner = bit_write(field, (&quote!(__v), &quote!((*__v))), msb, root);

        let start = match presence {
            // The presence bit is at `at`, so the field it gates begins one bit later.
            BitPresence::Present => bit_start(&segment, at + 1),
            _ => bit_start(&segment, at),
        };
        let when = match presence {
            BitPresence::Always => None,
            BitPresence::Present => Some(row::Presence::Bit),
            BitPresence::Mask { field, mask } => {
                Some(row::Presence::Flag { field: field.clone(), mask: *mask })
            }
        };

        match presence {
            BitPresence::Always => {
                parse.extend(quote! { let #ident = #read; });
                if flags.iter().any(|(flag, _)| *flag == f.name) {
                    let pos = bit_pos_ident(&f.name);
                    writes.extend(quote! { let #pos = #root::BitWriter::position(&__bits); });
                }
                writes.extend(write_held);
            }
            BitPresence::Present => {
                parse.extend(quote! {
                    let #ident = if __bits.bit()? { #some(#read) } else { #none };
                });
                writes.extend(quote! {
                    match #by_ref {
                        #some(__v) => {
                            __bits.bit(true)?;
                            #write_inner
                        }
                        #none => __bits.bit(false)?,
                    }
                });
            }
            BitPresence::Mask { field, mask } => {
                let test = bit_flag_test(&declared, field, *mask, root);
                parse.extend(quote! {
                    let #ident = if #test { #some(#read) } else { #none };
                });
                writes.extend(quote! {
                    if let #some(__v) = #by_ref {
                        #write_inner
                    }
                });
            }
        }

        rows.push(row::SegmentDef {
            name: f.name.clone(),
            start,
            span: span_of_kind(&f.kind),
            when,
            decoded_by: match &f.kind {
                Kind::Var { ty } => Some(ty.clone()),
                _ => None,
            },
            table: None,
            covers: None,
        });

        if optional || f.kind.bits().is_none() {
            segment = Some(f.name.clone());
            at = 0;
        } else {
            at += width;
        }
    }

    // Flags are written before the fields they absent, then patched once presence is known.
    for (flag, governed) in &flags {
        let f = declared
            .iter()
            .map(|(f, _)| f)
            .find(|f| f.name == *flag)
            .expect("`validate` resolves every flag to an earlier field");
        let pos = bit_pos_ident(flag);
        let w = Literal::u64_unsuffixed(f.kind.width());
        let held = bit_encode(&f.kind, (&Own::Struct.by_ref(flag), &Own::Struct.get(flag)), root);
        let all = Literal::u64_unsuffixed(governed.iter().fold(0u64, |all, (mask, _)| all | mask));
        let set = governed.iter().map(|(mask, of)| {
            let opt = Own::Struct.get(of);
            let mask = Literal::u64_unsuffixed(*mask);
            quote!(| (if #opt.is_some() { #mask } else { 0 }))
        });
        writes.extend(quote! {
            __bits.patch(#pos, #w, (#held & !#all) #(#set)*);
        });
    }

    let table = rows.iter().map(|row| row_tokens(row, root));
    let (owner, str_ty) = (m.name.as_str(), root.prelude().primitive("str"));
    let too_deep = too_deep(root);
    let codec = quote! {
        impl #root::Message for #name {
            const NAME: &'static #str_ty = #owner;

            type Ctx = ();

            /// Each field in wire order, with its start and length.
            const SEGMENTS: &'static [#root::table::SegmentDef<'static>] = &[#(#table)*];

            const OPEN_ENDED: #bool = false;

            fn decode_with_nested(
                body: &[#u8],
                __depth: #u32,
                __ctx: (),
            ) -> #result<(Self, #usize), #root::ParseError> {
                #too_deep
                let mut __bits = #root::BitReader::<#msb>::new(body);
                #parse
                #ok((Self { #build }, __bits.bytes_consumed()))
            }

            fn encode_into_with<__B: #root::Buffer>(
                &self,
                out: &mut __B,
                __ctx: (),
            ) -> #result<(), #root::Overflow> {
                let mut __bits = #root::BitWriter::<_, #msb>::new(out);
                #writes
                #ok(())
            }
        }
    };

    Ok((MessageParts { fields, checks, blocks: TokenStream::new(), codec }, rows))
}

/// When a field of a bit-addressed message is present.
enum BitPresence {
    Always,
    /// When the bit just before the field is set.
    Present,
    /// When bits of an earlier field are set. A bare `bool` flag is its single bit.
    Mask {
        field: String,
        /// The bits that make this field present.
        mask: u64,
    },
}

/// A bit-addressed message's fields in wire order, each with its presence.
fn bit_declared(m: &MessageDef) -> Result<Vec<(Field, BitPresence)>, Error> {
    let mut declared: Vec<(Field, BitPresence)> = Vec::new();
    for seg in &m.segments {
        match seg {
            Segment::Block(block) => {
                declared.extend(block.iter().map(|f| (f.clone(), BitPresence::Always)));
            }
            Segment::Opt { when, field } => {
                let presence = match when {
                    Presence::Bit => BitPresence::Present,
                    Presence::Mask { field: flag, mask } => {
                        BitPresence::Mask { field: flag.clone(), mask: *mask }
                    }
                    Presence::Flag { field: flag } => {
                        BitPresence::Mask { field: flag.clone(), mask: 1 }
                    }
                    Presence::Remaining => {
                        return Err(Error::Refused(format!(
                            "{}: `{}` is present when bytes remain, which a bit-addressed message cannot test. \
                             Use `#[present]`, or a flag in an earlier field",
                            m.name, field.name,
                        )));
                    }
                };
                declared.push((field.clone(), presence));
            }
            Segment::Value { name, kind: kind @ Kind::Var { .. }, doc, .. } => {
                let mut f = Field::new(name, kind.clone());
                f.doc = doc.clone();
                declared.push((f, BitPresence::Always));
            }
            other => {
                return Err(Error::Refused(format!(
                    "{}: `{}` cannot be in a bit-addressed message. \
                     Use fixed fields, optional fields or `VarCodec` values",
                    m.name,
                    other.name().unwrap_or("?"),
                )));
            }
        }
    }
    Ok(declared)
}

/// Each flag field, with the `(mask, field)` pairs it gates, in declaration order.
fn bit_flags(declared: &[(Field, BitPresence)]) -> Vec<(String, Vec<(u64, String)>)> {
    let mut flags: Vec<(String, Vec<(u64, String)>)> = Vec::new();
    for (f, presence) in declared {
        let BitPresence::Mask { field, mask } = presence else { continue };
        let bit = (*mask, f.name.clone());
        match flags.iter_mut().find(|(flag, _)| flag == field) {
            Some((_, bits)) => bits.push(bit),
            None => flags.push((field.clone(), vec![bit])),
        }
    }
    flags
}

/// The local holding the bit position a flag field was written at.
fn bit_pos_ident(flag: &str) -> Ident {
    format_ident!("__flag_{}", flag)
}

/// The presence test for a `#[when]` field, over its flag's decoded local.
fn bit_flag_test(
    declared: &[(Field, BitPresence)],
    flag: &str,
    mask: u64,
    root: &Root,
) -> TokenStream {
    let ident = ident(flag);
    let kind = declared
        .iter()
        .find(|(f, _)| f.name == flag)
        .map(|(f, _)| &f.kind)
        .expect("`validate` resolves every flag to an earlier field");
    if matches!(kind, Kind::Scalar(Scalar::Bool)) && mask == 1 {
        return quote!(#ident);
    }
    let raw = bit_encode(kind, (&quote!(&#ident), &quote!(#ident)), root);
    let mask = Literal::u64_unsuffixed(mask);
    quote!((#raw & #mask) != 0)
}

/// Read one field from the cursor, as an expression.
fn bit_read(name: &str, kind: &Kind, msb: bool, root: &Root) -> TokenStream {
    let Prelude { err, .. } = root.prelude();
    match kind {
        Kind::Var { ty } => {
            let p = path(ty);
            quote! {
                {
                    let __at = #root::BitReader::position(&__bits);
                    let (__v, __used) = <#p as #root::BitCodec<#msb>>::decode(body, __at)?;
                    if __used == 0 {
                        return #err(#root::ParseError::Malformed { field: #name, at: __at / 8 });
                    }
                    __bits.skip(__used)?;
                    __v
                }
            }
        }
        other => {
            let w = Literal::u64_unsuffixed(other.width());
            let value = bit_decode(other, &quote!(__raw), root);
            quote! {
                {
                    let __raw = __bits.read(#w)?;
                    #value
                }
            }
        }
    }
}

/// Write one field to the cursor, panicking if the value does not fit its width.
fn bit_write((name, kind): (&str, &Kind), held: Held<'_>, msb: bool, root: &Root) -> TokenStream {
    let (by_ref, by_value) = held;
    if let Kind::Var { .. } = kind {
        return quote! { #root::BitCodec::<#msb>::encode(#by_ref, &mut __bits)?; };
    }
    let bits = kind.width();
    let w = Literal::u64_unsuffixed(bits);
    let msg = format!("field `{name}`: value does not fit #[bits({bits})]");
    let raw = bit_encode(kind, held, root);
    let fits = match kind {
        Kind::Scalar(Scalar::Bool) | Kind::Codec { .. } if bits < 64 => {
            Some(quote! { ::core::assert!(#raw >> #w == 0, #msg); })
        }
        Kind::Scalar(Scalar::U(n) | Scalar::I(n)) if *n < 64 => {
            let (ty, i64) = (kind_ty(kind, root), root.prelude().i64);
            Some(fits(Target::Field(&ty, bits), &quote!(#by_value as #i64), &msg, root))
        }
        _ => None,
    };
    quote! {
        #fits
        __bits.write(#raw, #w)?;
    }
}

/// Where a row starts: a bit of the body, or bits past the last variable-size row.
fn bit_start(segment: &Option<String>, at: u64) -> row::Start {
    match segment {
        None => row::Start::At(at),
        Some(segment) => row::Start::After { segment: segment.clone(), bits: at },
    }
}
