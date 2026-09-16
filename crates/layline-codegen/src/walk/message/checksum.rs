//! Checksums: the positions a range needs, the decode check, and the encode stamp.

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};

use super::body::{Bind, Item, Own, Walk};
use super::cursor::read_layout;
use super::patch::{Slot, SlotWrite, derived_value, fill_reserved};
use super::table::Row;
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{SlotOf, back_patched, doc_attr, hidden_block, row, scalar_ty};
use crate::{CoverFrom, CoverTo, Coverage, Error, Field, Kind, Root, Segment};

impl Coverage {
    /// Whether the range ends after the checksum field's own bytes start.
    ///
    /// `order` gives a name's position in wire order.
    fn runs_past(&self, field: &str, order: impl Fn(&str) -> Option<usize>) -> bool {
        let Some(ck) = order(field) else { return false };
        match &self.to {
            CoverTo::Here => false,
            CoverTo::Before(f) => order(f).is_some_and(|i| i > ck),
            CoverTo::After(f) => order(f).is_some_and(|i| i >= ck),
        }
    }

    /// Whether the range skips the checksum field's own bytes.
    ///
    /// `order` gives a name's position in wire order.
    pub(crate) fn excises(
        &self,
        field: &str,
        order: impl Fn(&str) -> Option<usize> + Copy,
    ) -> bool {
        let Some(ck) = order(field) else { return false };
        let at = |n: &str| order(n).map(|i| (i as u64, i as u64 + 1));
        self.resolve((ck as u64, ck as u64 + 1), at).is_some_and(|c| c.excises())
    }

    /// Whether the range includes a byte that a later segment still overwrites.
    ///
    /// So a range entirely before the field may still not be final when the field is reached.
    fn covers_a_later_patch(
        &self,
        field: &str,
        order: impl Fn(&str) -> Option<usize> + Copy,
        patches: &[(String, String)],
    ) -> bool {
        let Some(ck) = order(field) else { return false };
        let at = |n: &str| order(n).map(|i| (i as u64, i as u64 + 1));
        let Some(c) = self.resolve((ck as u64, ck as u64 + 1), at) else { return false };
        patches.iter().any(|(slot, filler)| match (order(slot), order(filler)) {
            (Some(s), Some(g)) => c.from <= s as u64 && (s as u64) < c.to && g > ck,
            _ => false,
        })
    }
}

/// The local name prefixes for a range's ends, for decode or encode.
#[derive(Clone)]
struct Marks {
    at: &'static str,
    end: &'static str,
    /// This body's first byte, in the same coordinates as the range.
    start: TokenStream,
}

/// One run of covered bytes: its start, and its end unless it runs to the end.
type Run = (TokenStream, Option<TokenStream>);

impl Marks {
    fn decode() -> Self {
        Self { at: "__at_", end: "__end_", start: quote!(0) }
    }

    fn encode() -> Self {
        Self { at: "__out_", end: "__outend_", start: quote!(__base) }
    }

    fn edge_at(&self, prefix: &str, row: &str) -> TokenStream {
        let id = format_ident!("{}{}", prefix, row);
        quote!(#id)
    }

    fn from(&self, over: &Coverage) -> TokenStream {
        match &over.from {
            CoverFrom::Start => self.start.clone(),
            CoverFrom::Field(row) => self.edge_at(self.at, row),
        }
    }

    fn to(&self, over: &Coverage, here: Option<TokenStream>) -> Option<TokenStream> {
        match &over.to {
            CoverTo::Here => here,
            CoverTo::Before(row) => Some(self.edge_at(self.at, row)),
            CoverTo::After(row) => Some(self.edge_at(self.end, row)),
        }
    }

    fn runs(&self, over: &Coverage, here: Option<TokenStream>, hole: Option<Run>) -> Vec<Run> {
        let from = self.from(over);
        let to = self.to(over, here);
        match hole {
            None => vec![(from, to)],
            Some((at, past)) => vec![(from, Some(at)), (past.expect("a hole ends"), to)],
        }
    }
}

/// The runs as slices of encode's output.
fn out_slices(runs: &[Run]) -> Vec<TokenStream> {
    runs.iter()
        .map(|(from, to)| match to {
            Some(to) => quote!(&out.written()[#from..#to]),
            None => quote!(&out.written()[#from..]),
        })
        .collect()
}

/// The runs as slices of decode's input: the `let`s and the names they bind.
fn body_slices(runs: &[Run], field: &str, root: &Root) -> (TokenStream, Vec<TokenStream>) {
    let mut lets = TokenStream::new();
    let mut names = Vec::new();
    for (i, (from, to)) in runs.iter().enumerate() {
        let name = if runs.len() == 1 {
            format_ident!("__covered")
        } else {
            format_ident!("__covered{}", i)
        };
        let range = match to {
            Some(to) => quote!(#from..#to),
            None => quote!(#from..),
        };
        lets.extend(quote! {
            let #name = __body.get(#range).ok_or(#root::ParseError::Malformed {
                field: #field,
                at: #from,
            })?;
        });
        names.push(quote!(#name));
    }
    (lets, names)
}

/// The checksum over the runs: one `compute`, or `init`/`update`/`finish` across a hole.
pub(super) fn check_value(algo: &syn::Path, runs: &[TokenStream], root: &Root) -> TokenStream {
    match runs {
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

/// Refuse a range bounded by a name with no position in this body.
fn covered(owner: &str, field: &str, over: &Coverage, places: &[&str]) -> Result<(), Error> {
    let from = match &over.from {
        CoverFrom::Start => None,
        CoverFrom::Field(row) => Some((row.as_str(), "starts at")),
    };
    let to = match &over.to {
        CoverTo::Here => None,
        CoverTo::Before(row) => Some((row.as_str(), "ends at")),
        CoverTo::After(row) => Some((row.as_str(), "runs through")),
    };
    for (row, bound) in from.into_iter().chain(to) {
        if places.contains(&row) {
            continue;
        }
        return Err(Error::Refused(format!(
            "{owner}: the range of checksum `{field}` {bound} `{row}`, which has no position of its own. \
             Name a field of this message"
        )));
    }
    Ok(())
}

/// The rows whose edges this body's checksums need.
pub(super) struct Marked<'a> {
    /// Rows whose first byte a range needs.
    pub(super) starts: Vec<&'a str>,
    /// Rows whose last byte a range needs.
    pub(super) ends: Vec<&'a str>,
}

impl<'a> Marked<'a> {
    pub(super) fn of(segments: &'a [Segment]) -> Self {
        let mut marked = Self { starts: Vec::new(), ends: Vec::new() };
        for seg in segments {
            let Segment::Checksum { name, over, .. } = seg else { continue };
            match &over.from {
                CoverFrom::Start => {}
                CoverFrom::Field(row) => marked.starts.push(row),
            }
            match &over.to {
                CoverTo::Here => {}
                CoverTo::Before(row) => marked.starts.push(row),
                CoverTo::After(row) => marked.ends.push(row),
            }
            let order = |row: &str| position_in(segments, row);
            if over.runs_past(name, order)
                || over.covers_a_later_patch(name, order, &back_patched(segments))
            {
                marked.starts.push(name);
            }
        }
        marked
    }

    /// `let __at_<name>` and `let __out_<name>`, where a range wants `name`'s first byte.
    pub(super) fn start(&self, name: &str, parse: &mut TokenStream, bytes: &mut TokenStream) {
        bind(&self.starts, ("__at_", "__out_"), name, parse, bytes);
    }

    /// `let __end_<name>` and `let __outend_<name>`, where a range wants `name`'s last byte.
    pub(super) fn end(&self, name: &str, parse: &mut TokenStream, bytes: &mut TokenStream) {
        bind(&self.ends, ("__end_", "__outend_"), name, parse, bytes);
    }
}

fn bind(
    wanted: &[&str],
    (decode, encode): (&str, &str),
    name: &str,
    parse: &mut TokenStream,
    bytes: &mut TokenStream,
) {
    if !wanted.contains(&name) {
        return;
    }
    let decode = format_ident!("{}{}", decode, name);
    let encode = format_ident!("{}{}", encode, name);
    parse.extend(quote! { let #decode = __at; });
    bytes.extend(quote! { let #encode = out.len(); });
}

fn position_in(segments: &[Segment], row: &str) -> Option<usize> {
    segments.iter().flat_map(Segment::places).position(|p| p == row)
}

/// Assert at compile time that the algorithm's output fits the field's type.
fn checksum_fits(
    owner: &str,
    field: &str,
    algorithm: &syn::Path,
    ty: &TokenStream,
    root: &Root,
) -> TokenStream {
    let assert = format_ident!("__layline_checksum_fits_{}_{}", owner, field);
    quote! {
        const _: () = {
            #[allow(non_snake_case)]
            fn #assert<A: #root::__private::ChecksumFits<T>, T>() {}
            let _ = #assert::<#algorithm, #ty>;
        };
    }
}

impl Walk<'_> {
    pub(super) fn checksum(
        &mut self,
        item: &mut Item<'_>,
        segments: &[Segment],
        own: Own,
        seg: &Segment,
    ) -> Result<(), Error> {
        let Prelude { err, .. } = self.root.prelude();
        let (root, owner) = (self.root, self.owner);
        let Segment::Checksum { name: field, repr, algorithm, over, doc, stated: _ } = seg else {
            unreachable!("just matched")
        };
        let ident = ident(field);
        let ty = scalar_ty(*repr);
        let bits = repr.bits();
        if bits == 0 || !bits.is_multiple_of(8) {
            return Err(Error::Refused(format!(
                "{owner}: checksum field `{field}` is {bits} bits. Use a whole-byte integer type"
            )));
        }
        let len = Literal::usize_unsuffixed((bits / 8) as usize);
        let algo: syn::Path = syn::parse_str(algorithm).map_err(|_| {
            Error::Refused(format!("{owner}: checksum `{algorithm}` is not a valid type path"))
        })?;

        let d = doc_attr(doc);
        self.public_fields.extend(quote! { #d pub #ident: #ty, });
        self.binds.push(Bind { name: ident.clone(), ty: ty.clone(), value: quote!(#ident) });

        let sub = format_ident!("__{}Ck{}", self.owner, item.ck_n);
        item.ck_n += 1;
        item.items.extend(hidden_block(
            root,
            &sub,
            &len,
            &self.endian,
            quote! { pub #ident: #ty, },
        ));
        item.items.extend(checksum_fits(self.owner, field, &algo, &ty, root));
        let order = |row: &str| position_in(segments, row);
        let excludes_self = over.excises(field, order);
        self.rows.push(
            Row::new(field, row::Span::Fixed(bits))
                .decoded_by(Some(algorithm))
                .table(
                    &sub,
                    core::slice::from_ref(&Field::new(field, Kind::Scalar(*repr))),
                    bits,
                    item.endian,
                )
                .covers(row::CoverDef::new(field, over, excludes_self)),
        );

        covered(self.owner, field, over, &self.places)?;
        let Some((_, d)) = self.derived.iter().find(|(df, _)| df == field) else {
            return Err(Error::Refused(format!(
                "{owner}: checksum field `{field}` is missing from the derived table"
            )));
        };
        let forward = over.runs_past(field, order);
        let late = forward || over.covers_a_later_patch(field, order, &back_patched(segments));
        let read = read_layout(&format_ident!("__ck"), &sub, &len, root);
        let read = quote! {
            #read
            let #ident = __ck.#ident;
        };
        let verify = |runs: &[Run]| {
            let (lets, names) = body_slices(runs, field, root);
            let want = check_value(&algo, &names, root);
            quote! {
                {
                    #lets
                    let __want = #root::WireInt::to_i64(&#want);
                    let __got = #root::WireInt::to_i64(&#ident);
                    if __want != __got {
                        return #err(#root::ParseError::Checksum {
                            field: #field,
                            expected: __want,
                            actual: __got,
                        });
                    }
                }
            }
        };
        if forward {
            let hole = excludes_self.then(|| {
                let at = format_ident!("__at_{}", field);
                (quote!(#at), Some(quote!(#at + #len)))
            });
            self.parse_steps.extend(read);
            self.late_checks.extend(verify(&Marks::decode().runs(over, None, hole)));
        } else {
            let runs = Marks::decode().runs(over, Some(quote!(__ck_to)), None);
            let check = verify(&runs);
            self.parse_steps.extend(quote! {
                let __ck_to = __at;
                #read
                #check
            });
        }

        if late {
            let mark = format_ident!("__out_{}", field);
            self.byte_steps.extend(quote! {
                out.push(&#sub { #ident: 0 }.encode())?;
            });
            let hole = excludes_self.then(|| (quote!(#mark), Some(quote!(#mark + #len))));
            let runs = Marks::encode().runs(over, Some(quote!(#mark)), hole);
            let value = check_value(&algo, &out_slices(&runs), root);
            let slot = Slot {
                coll: String::from(field),
                of: SlotOf::Position,
                mark,
                width_bytes: (bits / 8) as usize,
                write: SlotWrite::Check { sub: sub.clone(), field: ident.clone(), ty: ty.clone() },
            };
            self.late_stamps.extend(fill_reserved(
                self.owner,
                &slot,
                field,
                d.what(),
                &quote!(#root::WireInt::to_i64(&#value)),
                item.endian,
                root,
            ));
        } else {
            let n = derived_value(d, own, field, root);
            self.byte_steps.extend(quote! {
                {
                    let __ck = <#ty as #root::WireInt>::from_i64(#n);
                    out.push(&#sub { #ident: __ck }.encode())?;
                }
            });
        }
        Ok(())
    }
}

/// Encode's checksum over the bytes written so far, for [`patch`](super::patch).
pub(super) fn written_check(algo: &syn::Path, over: &Coverage, root: &Root) -> TokenStream {
    check_value(algo, &out_slices(&Marks::encode().runs(over, None, None)), root)
}
