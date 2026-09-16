//! Derived numbers on encode: their values, range checks, and slots filled in once known.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use super::body::Own;
use super::checksum::written_check;
use super::repeat::element_width;
use super::text::text_wire_len;
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{Derived, SlotOf, path};
use crate::{Endian, Error, Kind, Root, Segment};

/// The `bool` a derived flag holds. Presence is the only derivation that is not a number.
pub(super) fn derived_flag(d: &Derived, own: Own) -> Option<TokenStream> {
    match d {
        Derived::Presence { of } => {
            let opt = own.get(of);
            Some(quote!(#opt.is_some()))
        }
        _ => None,
    }
}

/// The number encode writes into a derived field, as an `i64` expression.
///
/// `field` names it in errors. A [`Presence`](Derived::Presence) is a flag: use [`derived_flag`].
pub(super) fn derived_value(d: &Derived, own: Own, field: &str, root: &Root) -> TokenStream {
    let Prelude { some, none, .. } = root.prelude();
    match d {
        Derived::Count { of, scale, offset, cap } => {
            let coll = own.get(of);
            affine_inverse(quote!(#coll.len()), (*scale, *offset), *cap, (field, d.what()), root)
        }
        Derived::Squared { of, scale, offset, cap } => {
            let coll = own.get(of);
            let capped = cap.map(|cap| {
                let lit = Literal::usize_unsuffixed(cap);
                let msg = format!(
                    "field `{field}`: the square collection's length passes its cap of {cap}"
                );
                quote! { ::core::assert!(__len <= #lit, #msg); }
            });
            let order = quote! {
                {
                    let __len = #coll.len();
                    #capped
                    let __side = __len.isqrt();
                    ::core::assert!(
                        __side * __side == __len,
                        "a square collection needs n * n elements, and \
                         no whole number squares to its length"
                    );
                    __side
                }
            };
            affine_inverse(order, (*scale, *offset), None, (field, d.what()), root)
        }
        Derived::TextLen { of, codec, scale, offset, cap } => {
            let text = own.get(of);
            let n = text_wire_len(codec, quote!(#text.as_str()), root);
            affine_inverse(n, (*scale, *offset), *cap, (field, d.what()), root)
        }
        Derived::Offset { .. } | Derived::ByteLen { .. } => reserved(),
        Derived::Stride { elem, scale, offset } => affine_inverse(
            element_width(elem, root),
            (*scale, *offset),
            None,
            (field, d.what()),
            root,
        ),
        Derived::ArmLen { of, scale, offset, cap } => {
            let arm = arm_ident(of);
            affine_inverse(quote!(#arm), (*scale, *offset), *cap, (field, d.what()), root)
        }
        Derived::Discriminant { of, field } => {
            let body = own.get(of);
            let held = own.get(field);
            quote! {
                match #root::Choice::discriminant(&#body) {
                    #some(__d) => __d,
                    #none => #root::WireInt::to_i64(&#held),
                }
            }
        }
        Derived::Flags { field, bits } => {
            let held = own.get(field);
            let governed =
                Literal::i64_unsuffixed(bits.iter().fold(0u64, |all, (m, _)| all | m) as i64);
            let set = bits.iter().map(|(mask, of)| {
                let opt = own.get(of);
                let mask = Literal::i64_unsuffixed(*mask as i64);
                quote!(| (if #opt.is_some() { #mask } else { 0 }))
            });
            quote! {
                (#root::WireInt::to_i64(&#held) & !#governed) #(#set)*
            }
        }
        Derived::Presence { of } => {
            unreachable!("presence of `{of}` is a flag: ask `derived_flag` first")
        }
        Derived::Checksum { algorithm, over } => {
            let value = written_check(&path(algorithm), over, root);
            quote! {
                #root::WireInt::to_i64(&#value)
            }
        }
    }
}

/// The placeholder a reserved field holds until [`stamp_slot`] overwrites it.
fn reserved() -> TokenStream {
    quote!(0)
}

/// A reserved field: where encode wrote its placeholder, and how to overwrite it.
pub(super) struct Slot {
    /// The collection whose number goes here, or the checksum field.
    pub(super) coll: String,
    pub(super) of: SlotOf,
    /// The local holding the slot's byte position in `out`.
    pub(super) mark: Ident,
    pub(super) width_bytes: usize,
    pub(super) write: SlotWrite,
}

impl SlotOf {
    /// The field's role, as errors name it.
    pub(super) fn what(self) -> &'static str {
        match self {
            SlotOf::Position => "offset",
            SlotOf::Length => "length",
        }
    }

    /// How the segment relates to `field`, as errors phrase it.
    fn tie(self, field: &str, record: bool) -> String {
        match (self, record) {
            (SlotOf::Position, _) => format!("is placed at offset field `{field}`"),
            (SlotOf::Length, false) => {
                format!("takes its byte length from length field `{field}`")
            }
            (SlotOf::Length, true) => {
                format!("is present exactly when length field `{field}` is non-zero")
            }
        }
    }

    /// The derivation this slot holds, as [`Derived::what`] names it.
    fn claim(self, record: bool) -> &'static str {
        match self {
            SlotOf::Position => Derived::Offset { of: String::new(), record }.what(),
            SlotOf::Length => Derived::ByteLen { of: String::new(), record }.what(),
        }
    }
}

/// The local holding a windowed switch's encoded length in bytes.
///
/// Encode writes the arm, measures it, and rewinds. The length field comes earlier and may be a
/// varint, so there are no fixed bytes to reserve.
pub(super) fn arm_ident(switch: &str) -> Ident {
    format_ident!("__arm_{}", switch)
}

/// The local holding the byte position of the slot reserved for `coll`.
pub(super) fn slot_ident(of: SlotOf, coll: &str) -> Ident {
    match of {
        SlotOf::Position => format_ident!("__slot_{}", coll),
        SlotOf::Length => format_ident!("__span_{}", coll),
    }
}

/// Which segment's number a stamp writes, and into which field.
#[derive(Clone, Copy)]
pub(super) struct StampAt<'a> {
    pub(super) owner: &'a str,
    pub(super) coll: &'a str,
    pub(super) of: SlotOf,
    pub(super) field: &'a str,
    pub(super) endian: Endian,
    /// Whether the segment is a single record. Only the error message uses it.
    pub(super) record: bool,
}

/// Write `value` into the slot reserved for `coll`, refusing a field with no reserved slot.
pub(super) fn stamp_slot(
    slots: &[Slot],
    at: StampAt<'_>,
    value: &TokenStream,
    root: &Root,
) -> Result<TokenStream, Error> {
    let StampAt { owner, coll, of, field, endian, record } = at;
    let what = of.what();
    let Some(slot) = slots.iter().find(|s| s.coll == coll && s.of == of) else {
        let tie = of.tie(field, record);
        let article = if what.starts_with(['a', 'e', 'i', 'o', 'u']) { "An" } else { "A" };
        return Err(Error::Refused(format!(
            "{owner}: `{coll}` {tie}, which has no fixed width. \
             {article} {what} field must be a fixed-width integer in a block"
        )));
    };
    Ok(fill_reserved(owner, slot, field, of.claim(record), value, endian, root))
}

/// Overwrite a slot's placeholder with `value`, panicking if the field is too narrow.
pub(super) fn fill_reserved(
    owner: &str,
    slot: &Slot,
    field: &str,
    claim: &str,
    value: &TokenStream,
    endian: Endian,
    root: &Root,
) -> TokenStream {
    let Slot { mark, width_bytes, write, .. } = slot;
    let width = Literal::usize_unsuffixed(*width_bytes);
    let narrow = too_narrow(field, claim);
    let fill = match write {
        SlotWrite::Scalar { ty } => {
            let to = match endian {
                Endian::Be => quote!(to_be_bytes),
                Endian::Le => quote!(to_le_bytes),
            };
            let fits = fits(Target::Field(ty, 0), &quote!(__len), &narrow, root);
            quote! {
                #fits
                let __pos = <#ty as #root::WireInt>::from_i64(__len);
                out.written_mut()[#mark..#mark + #width].copy_from_slice(&__pos.#to());
            }
        }
        SlotWrite::Nested { value: held, ty, outer, name } => {
            let (get, get_mut) = nested_accessors(owner, outer, name);
            let into = Target::Nested { get: quote!(#get(&__nested)), child: ty, name };
            let fits = fits(into, &quote!(__len), &narrow, root);
            quote! {
                let mut __nested = #held.clone();
                #fits
                *#get_mut(&mut __nested) = #root::WireInt::from_i64(__len);
                out.written_mut()[#mark..#mark + #width].copy_from_slice(&__nested.encode());
            }
        }
        SlotWrite::Check { sub, field: inner, ty } => {
            let fits = fits(Target::Field(ty, 0), &quote!(__len), &narrow, root);
            quote! {
                #fits
                let __ck = #sub { #inner: <#ty as #root::WireInt>::from_i64(__len) };
                out.written_mut()[#mark..#mark + #width].copy_from_slice(&__ck.encode());
            }
        }
    };
    quote! {
        {
            let __len = #value;
            #fill
        }
    }
}

/// How a slot's bytes are rewritten once its number is known.
pub(super) enum SlotWrite {
    /// A whole scalar field of a block.
    Scalar { ty: TokenStream },
    /// A checksum field: its one-field layout, rebuilt with the checksum and re-encoded.
    Check { sub: Ident, field: Ident, ty: TokenStream },
    /// A field inside a nested layout: the child, rebuilt with the number and re-encoded.
    Nested {
        /// The local holding the child the block wrote, with its other derived fields already set.
        value: Ident,
        ty: TokenStream,
        /// The block field holding the child, as named in the [`nested_probe`] accessors.
        outer: String,
        name: String,
    },
}

/// The type of the nested layout field `name` in a block of this body.
pub(super) fn nested_in_blocks<'a>(segments: &'a [Segment], name: &str) -> Option<&'a str> {
    segments.iter().find_map(|seg| match seg {
        Segment::Block(fields) => fields.iter().find_map(|f| match &f.kind {
            Kind::Nested { ty, .. } if f.name == name => Some(ty.as_str()),
            _ => None,
        }),
        _ => None,
    })
}

/// Assert that `outer.inner` is a field of nested layout `ty`, and emit accessors for it.
///
/// All reads and writes go through the accessors, so a field type that is not a `WireInt` fails
/// once, at the attribute.
pub(super) fn nested_probe(
    owner: &str,
    ty: &str,
    outer: &str,
    inner: &str,
    d: &Derived,
    root: &Root,
) -> Result<TokenStream, Error> {
    let p: syn::Path = syn::parse_str(ty).map_err(|_| {
        Error::Refused(format!("{owner}: nested layout `{ty}` is not a valid type path"))
    })?;
    let id = ident(inner);
    let (get, get_mut) = nested_accessors(owner, outer, inner);
    let missing = format!(
        "`{owner}`: `{outer}.{inner}` names no field of nested layout `{ty}`. Check the spelling"
    );
    let number = if matches!(d, Derived::Presence { .. }) {
        quote!()
    } else {
        quote! {
            #[allow(non_snake_case, dead_code, clippy::explicit_auto_deref)]
            fn #get(__nested: &#p) -> &impl #root::WireInt {
                &(*__nested).#id
            }
            #[allow(non_snake_case, dead_code, clippy::explicit_auto_deref)]
            fn #get_mut(__nested: &mut #p) -> &mut impl #root::WireInt {
                &mut (*__nested).#id
            }
        }
    };
    Ok(quote! {
        const _: () = ::core::assert!(
            #root::table::field_width(<#p as #root::Layout>::FIELDS, #inner) != 0,
            #missing
        );
        #number
    })
}

/// The read and write accessors [`nested_probe`] emits for `outer.inner` of `owner`.
pub(super) fn nested_accessors(owner: &str, outer: &str, inner: &str) -> (Ident, Ident) {
    (
        format_ident!("__layline_{}_{}_{}", owner, outer, inner),
        format_ident!("__layline_{}_{}_{}_mut", owner, outer, inner),
    )
}

/// The value of `field` where `field * scale + offset` equals `source`.
///
/// Encode panics where decode could not read it back: no whole solution, or above `cap`.
pub(super) fn affine_inverse(
    source: TokenStream,
    (scale, offset): (u32, i32),
    cap: Option<usize>,
    (field, what): (&str, &str),
    root: &Root,
) -> TokenStream {
    let Prelude { i64, .. } = root.prelude();
    let capped = cap.map(|cap| {
        let lit = Literal::usize_unsuffixed(cap);
        let msg =
            format!("field `{field}`: the {what} passes its cap of {cap}, and decode refuses it");
        quote! { ::core::assert!(__source <= #lit, #msg); }
    });
    let (s, o) = (i64::from(scale), i64::from(offset));
    let stated = match (scale, offset) {
        (1, 0) => quote!(__source as #i64),
        (1, _) => quote!(__source as #i64 - #o),
        _ => {
            let msg = format!(
                "field `{field}` cannot state the {what} exactly: no whole `{field} * {scale} + {offset}` equals it"
            );
            quote! {{
                let __n = __source as #i64 - #o;
                ::core::assert!(__n % #s == 0, #msg);
                __n / #s
            }}
        }
    };
    quote! {{
        let __source = #source;
        #capped
        #stated
    }}
}

/// The field a number is written into.
pub(super) enum Target<'a> {
    /// A `ty` field this many bits wide. 0 means the whole type.
    Field(&'a TokenStream, u64),
    /// A field of a nested layout. Only the [`nested_probe`] accessor knows its type.
    Nested {
        /// The accessor's read of the field.
        get: TokenStream,
        /// The nested layout's type.
        child: &'a TokenStream,
        /// The field's name in the child's `FIELDS`.
        name: &'a str,
    },
}

/// Assert that `value` reads back unchanged from the field.
pub(super) fn fits(into: Target<'_>, value: &TokenStream, msg: &str, root: &Root) -> TokenStream {
    match into {
        Target::Field(ty, width) => {
            let width = Literal::u32_unsuffixed(width as u32);
            quote! {
                ::core::assert!(#root::__private::fits_in_field::<#ty>(#value, #width), #msg);
            }
        }
        Target::Nested { get, child, name } => quote! {
            ::core::assert!(
                #root::WireInt::__fits_in_field(
                    #get,
                    #value,
                    #root::table::field_width(<#child as #root::Layout>::FIELDS, #name),
                ),
                #msg
            );
        },
    }
}

/// The refusal for a derived number its field is too narrow to hold.
pub(super) fn too_narrow(field: &str, what: &str) -> String {
    format!(
        "field `{field}` is too narrow for the {what} the wire derives it from. Widen the field"
    )
}
