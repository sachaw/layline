//! Derived numbers on encode: their values, range checks, and slots filled in once known.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::{format_ident, quote};

use super::body::{Own, Walk};
use super::checksum::written_check;
use super::eval::solve;
use super::repeat::element_width;
use super::text::text_wire_len;
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{Derived, SlotOf, path};
use crate::{Error, Expr, Kind, Root, Segment};

/// The `bool` a derived flag holds. Presence is the only derivation that is not a number.
pub(super) fn derived_flag(d: &Derived, own: Own) -> Option<TokenStream> {
    match d {
        Derived::Presence { of, set } => {
            let opt = own.get(of);
            Some(if *set { quote!(#opt.is_some()) } else { quote!(#opt.is_none()) })
        }
        _ => None,
    }
}

/// The number encode writes into a derived field, as an `i64` expression.
///
/// `field` names it, and is the one field the derivation's expression is worked back to. A
/// [`Presence`](Derived::Presence) is a flag: use [`derived_flag`].
pub(super) fn derived_value(
    d: &Derived,
    own: Own,
    field: &str,
    root: &Root,
) -> Result<TokenStream, Error> {
    let Prelude { some, none, .. } = root.prelude();
    Ok(match d {
        Derived::Count { of, by, cap } => {
            let coll = own.by_ref(of);
            let n = quote!(#root::__private::value::Many::many(#coll));
            inverted(by, n, cap.map(|cap| (cap, of.as_str())), field, root)?
        }
        Derived::TextLen { of, codec, by, cap } => {
            let text = own.get(of);
            let n = text_wire_len(codec, quote!(#text.as_str()), root);
            inverted(by, n, cap.map(|cap| (cap, of.as_str())), field, root)?
        }
        Derived::Offset { .. } | Derived::ByteLen { .. } => reserved(),
        Derived::Stride { elem, by } => inverted(by, element_width(elem, root), None, field, root)?,
        Derived::ArmLen { of, by, cap } => {
            let arm = arm_ident(of);
            inverted(by, quote!(#arm), cap.map(|cap| (cap, of.as_str())), field, root)?
        }
        Derived::Discriminant { of, field: held_field, by, framing } => {
            let body = own.get(of);
            let chosen = inverted(by, quote!(__d), None, field, root)?;
            // The open arm's discriminant is the field's own, and a framing field holds none.
            let open = if *framing {
                let err = root.prelude().err;
                quote! { return #err(#root::EncodeError::Unstated { field: #held_field }) }
            } else {
                let held = own.get(held_field);
                quote!(#root::WireInt::to_i64(&#held))
            };
            quote! {
                match #root::Choice::discriminant(&#body) {
                    #some(__d) => #chosen,
                    #none => #open,
                }
            }
        }
        Derived::Flags { field, bits, framing } => {
            let held = own.get(field);
            let masks: Vec<TokenStream> = bits.iter().map(|(m, ..)| mask_tokens(m, root)).collect();
            let set = bits.iter().zip(&masks).map(|((_, of, set), mask)| {
                let opt = own.get(of);
                let marked = if *set { quote!(#opt.is_some()) } else { quote!(#opt.is_none()) };
                quote!((if #marked { #mask } else { 0 }))
            });
            // A framing word holds no other bits, so it is the masks alone.
            if *framing {
                quote!(#(#set)|*)
            } else {
                quote! {
                    (#root::WireInt::to_i64(&#held) & !(#(#masks)|*)) #(| #set)*
                }
            }
        }
        Derived::Presence { of, .. } => {
            unreachable!("presence of `{of}` is a flag: ask `derived_flag` first")
        }
        Derived::Checksum { algorithm, over } => {
            let value = written_check(&path(algorithm), over, root);
            quote! {
                #root::WireInt::to_i64(&#value)
            }
        }
    })
}

/// A presence mask, a literal or a constant, as an `i64`.
pub(super) fn mask_tokens(mask: &Expr, root: &Root) -> TokenStream {
    match (mask.fold(), mask) {
        (Some(n), _) => {
            let n = Literal::i64_unsuffixed(n);
            quote!(#n)
        }
        (None, Expr::Const(c)) => {
            let p = path(c);
            quote!(#root::WireInt::to_i64(&#p))
        }
        _ => unreachable!("a presence test's mask is a literal or a constant"),
    }
}

/// The value of `field` where `by` equals the number `source`, as an `i64`.
///
/// Encode refuses where decode could not read it back: no exact solution, or a `source` above the
/// `cap` the collection or text named with it declares.
pub(super) fn inverted(
    by: &Expr,
    source: TokenStream,
    cap: Option<(usize, &str)>,
    field: &str,
    root: &Root,
) -> Result<TokenStream, Error> {
    let Some(inv) = by.inverse(&|name| name == field) else {
        return Err(Error::Internal(format!("`{by}` is not invertible in `{field}`")));
    };
    let capped = cap.map(|(cap, of)| {
        let lit = Literal::usize_unsuffixed(cap);
        quote! { #root::EncodeError::cap(#of, __source, #lit)?; }
    });
    let solved = solve(by, &inv, quote!(__source), field, root)?;
    Ok(quote! {{
        let __source = #source;
        #capped
        #solved
    }})
}

impl Walk<'_> {
    /// Write the number `value` into the field `by` works back to, in the slot reserved for
    /// segment `coll`, or, where this segment does not write that field, check `by` against it.
    ///
    /// `cap` is the collection's, which a stamped length must not pass.
    pub(super) fn stamp_or_agree(
        &self,
        own: Own,
        coll: &str,
        (of, record): (SlotOf, bool),
        by: &Expr,
        value: TokenStream,
        cap: Option<usize>,
    ) -> Result<TokenStream, Error> {
        let mine = |d: &Derived| match (of, d) {
            (SlotOf::Position, Derived::Offset { of, .. })
            | (SlotOf::Length, Derived::ByteLen { of, .. }) => of == coll,
            _ => false,
        };
        let into = self.derived.iter().find(|(_, d)| mine(d)).map(|(f, _)| f.as_str());
        let Some(into) = into.filter(|_| self.writes(mine)) else {
            return self.agree(own, coll, value, by);
        };
        let n = inverted(by, value, cap.map(|cap| (cap, coll)), into, self.root)?;
        if self.follows(into, coll) {
            // The field comes after the segment, in the tail: encode writes it with this number.
            let wrote = wrote_ident(into);
            return Ok(quote! { let #wrote = #n; });
        }
        let at = StampAt { owner: self.owner, coll, of, field: into, record };
        if record {
            return stamp_slot(&self.slots, at, &n, self.root);
        }
        // A collection stamps at the top of encode, so a later check can read the number.
        let wrote = wrote_ident(into);
        let stamp = stamp_slot(&self.slots, at, &quote!(#wrote), self.root)?;
        Ok(quote! {
            let #wrote = #n;
            #stamp
        })
    }
}

impl Walk<'_> {
    /// Whether the field `field` comes after the segment `coll` in wire order, so encode knows
    /// the segment's number by the time it writes the field.
    pub(super) fn follows(&self, field: &str, coll: &str) -> bool {
        let at = |name: &str| self.places.iter().position(|p| *p == name);
        matches!((at(field), at(coll)), (Some(f), Some(c)) if f > c)
    }
}

/// The local holding the number encode stamped into the reserved field `field`.
pub(super) fn wrote_ident(field: &str) -> Ident {
    format_ident!("__wrote_{}", field.replace('.', "_"))
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
    let StampAt { owner, coll, of, field, record } = at;
    let what = of.what();
    let Some(slot) = slots.iter().find(|s| s.coll == coll && s.of == of) else {
        let tie = of.tie(field, record);
        let article = if what.starts_with(['a', 'e', 'i', 'o', 'u']) { "An" } else { "A" };
        return Err(Error::Refused(format!(
            "{owner}: `{coll}` {tie}, which has no fixed width. \
             {article} {what} field must be a fixed-width integer in a block"
        )));
    };
    Ok(fill_reserved(owner, slot, field, value, root))
}

/// The number a reserved field holds until it is stamped: zero, or the nearest end of its
/// `#[range]`, so the block's own range check passes the placeholder.
pub(super) fn placeholder(range: Option<crate::Range>) -> TokenStream {
    let held = match range {
        Some(crate::Range { lo: Some(lo), .. }) if lo > 0 => lo,
        Some(crate::Range { hi: Some(hi), .. }) if hi < 0 => hi,
        _ => 0,
    };
    let held = Literal::i64_unsuffixed(held);
    quote!(#held)
}

/// Refuses a stamped number `value` outside the field's `#[range]`, as the block's encode refuses
/// every other field's.
fn in_range(field: &str, r: crate::Range, value: &TokenStream, root: &Root) -> TokenStream {
    if r.lo.is_none() && r.hi.is_none() {
        return quote!();
    }
    let lo = Literal::i64_suffixed(r.lo.unwrap_or(i64::MIN));
    let hi = Literal::i64_suffixed(r.hi.unwrap_or(i64::MAX));
    quote! { #root::EncodeError::range(#field, #value, #lo, #hi)?; }
}

/// Overwrite a slot's placeholder with `value`, refusing a number the field is too narrow for.
pub(super) fn fill_reserved(
    owner: &str,
    slot: &Slot,
    field: &str,
    value: &TokenStream,
    root: &Root,
) -> TokenStream {
    let Slot { mark, width_bytes, write, .. } = slot;
    let width = Literal::usize_unsuffixed(*width_bytes);
    let fill = match write {
        SlotWrite::Scalar { ty, range } => {
            let fits = fits(Target::Field(ty, 0), &quote!(__len), field, root);
            let ranged = range.map(|r| in_range(field, r, &quote!(__len), root));
            quote! {
                #fits
                #ranged
                let __pos = <#ty as #root::WireInt>::from_i64(__len);
                out.written_mut()[#mark..#mark + #width]
                    .copy_from_slice(&<__O as #root::__private::ByteOrder>::write(__pos));
            }
        }
        SlotWrite::Nested { value: held, ty, outer, name } => {
            let (get, get_mut) = nested_accessors(owner, outer, name);
            let into = Target::Nested { get: quote!(#get(&#held)), child: ty, name };
            let fits = fits(into, &quote!(__len), field, root);
            // Stamp the child the block wrote, not a copy of it: a second stamp into the same
            // child re-encodes it with the first one's number still in place.
            quote! {
                #fits
                *#get_mut(&mut #held) = #root::WireInt::from_i64(__len);
                <#ty as #root::Layout>::encode_into(
                    &#held,
                    &mut #root::Fixed::new(&mut out.written_mut()[#mark..#mark + #width]),
                )?;
            }
        }
        SlotWrite::Check { sub, field: inner, ty } => {
            let fits = fits(Target::Field(ty, 0), &quote!(__len), field, root);
            quote! {
                #fits
                let __ck = #sub { #inner: <#ty as #root::WireInt>::from_i64(__len) };
                out.written_mut()[#mark..#mark + #width].copy_from_slice(&__ck.encode_in::<__O>()?);
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
    /// A whole scalar field of a block, and the `#[range]` its number must fall in.
    Scalar { ty: TokenStream, range: Option<crate::Range> },
    /// A checksum field: its one-field layout, rebuilt with the checksum and re-encoded.
    Check { sub: Ident, field: Ident, ty: TokenStream },
    /// A field inside a nested layout: the child, rebuilt with the number and re-encoded.
    Nested {
        /// The local holding the child the block wrote, with its other derived fields already set.
        /// Each stamp updates it in place, so stamps into one child accumulate.
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
    number: bool,
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
    let number = if !number {
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

/// Refuse a `value` that does not read back unchanged from `field`.
pub(super) fn fits(into: Target<'_>, value: &TokenStream, field: &str, root: &Root) -> TokenStream {
    match into {
        Target::Field(ty, width) => {
            let width = Literal::u32_unsuffixed(width as u32);
            quote! {
                #root::EncodeError::fit::<#ty>(#field, #value, #width)?;
            }
        }
        Target::Nested { get, child, name } => quote! {
            #root::EncodeError::fit_as(
                #get,
                #field,
                #value,
                #root::table::field_width(<#child as #root::Layout>::FIELDS, #name),
            )?;
        },
    }
}
