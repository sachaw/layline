//! Collections: one element type, repeated by a count, a window, a terminator or to the end.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use super::body::{Bind, Item, Own, Walk};
use super::cursor::{consumed_nothing, take, windowed};
use super::eval::blamed;
use super::prefix::Prefix;
use super::table::{Row, deferred_type, span_of_count};
use super::value::{WithCtx, discovered_read, value_write};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{Derived, SlotOf, doc_attr, kind_ty, path, scalar_ty};
use crate::{Collection, Count, Error, Kind, Len, Root, Scalar, Segment};

/// How one element is read and written, and how far it moves the cursor.
pub(super) enum Step {
    /// A fixed number of bytes.
    Fixed {
        len: usize,
        /// An expression over `__s: &[u8]` of exactly `len` bytes, giving one element.
        read: TokenStream,
        /// Statements appending `__e: &Elem` to `out`.
        write: TokenStream,
    },
    /// As many bytes as the element reports consuming.
    Discovered {
        /// Statements binding `let (__e, __used)` from `__body[__at..]`.
        read: TokenStream,
        /// Statements appending `__e: &Elem` to `out`.
        write: TokenStream,
        /// The name a malformed element is reported under.
        what: String,
    },
}

impl Step {
    /// `let __e = ..;`: read one element at the cursor.
    ///
    /// A zero-byte element is an error unless a count bounds the run (`bounded`).
    pub(super) fn read_one(&self, bounded: bool, root: &Root) -> TokenStream {
        match self {
            Step::Fixed { len, read, .. } => {
                let len = Literal::usize_unsuffixed(*len);
                let bytes = take(&quote!(#len), root);
                quote! {
                    let __s = #bytes;
                    let __e = #read;
                    __at += #len;
                }
            }
            Step::Discovered { read, what, .. } => {
                let empty = (!bounded).then(|| consumed_nothing(what, root));
                quote! {
                    #read
                    #empty
                    __at += __used;
                }
            }
        }
    }

    /// The most elements the remaining bytes can hold. Caps the capacity reserved for a count.
    fn room(&self) -> TokenStream {
        match self {
            Step::Fixed { len: 1, .. } | Step::Discovered { .. } => quote!(__body.len() - __at),
            Step::Fixed { len, .. } => {
                let len = Literal::usize_unsuffixed(*len);
                quote!((__body.len() - __at) / #len)
            }
        }
    }

    pub(super) fn write_one(&self) -> TokenStream {
        match self {
            Step::Fixed { write, .. } | Step::Discovered { write, .. } => write.clone(),
        }
    }
}

/// One element's wire width in bytes, as a `usize` expression.
pub(super) fn element_width(elem: &Kind, root: &Root) -> TokenStream {
    match elem {
        Kind::Nested { ty, .. } => {
            let p = path(ty);
            quote!(<#p as #root::Layout>::WIRE_BYTES)
        }
        other => {
            let n = Literal::usize_unsuffixed((other.width() / 8) as usize);
            quote!(#n)
        }
    }
}

/// The step for one `element` of collection `coll`.
pub(super) fn element_step(
    element: &Kind,
    coll: &str,
    ctx: Option<&WithCtx>,
    root: &Root,
) -> Result<Step, Error> {
    let Prelude { u8, .. } = root.prelude();
    match element {
        Kind::Nested { ty, bytes } => {
            if *bytes == 0 {
                return Err(Error::Refused(format!(
                    "collection element `{ty}` is zero bytes. Give it a nonzero width"
                )));
            }
            let p = path(ty);
            Ok(Step::Fixed {
                len: *bytes,
                read: quote! {
                    <#p as #root::Layout>::decode_slice(__s)
                        .map_err(|__err| __err.rebased(__at))?
                },
                write: quote! { <#p as #root::Layout>::encode_into(__e, out)?; },
            })
        }
        Kind::Scalar(s) => {
            let width = (s.bits() / 8) as usize;
            if width == 0 {
                return Err(Error::Refused(format!(
                    "collection element {s:?} is narrower than a byte. Use a whole-byte type"
                )));
            }
            let ty = scalar_ty(*s);
            let order = quote!(<__O as #root::__private::ByteOrder>);
            let byte = matches!(s, Scalar::U(8));
            let read = match (width, byte) {
                (1, true) => quote! { __s[0] },
                (1, false) => quote! { __s[0] as #ty },
                _ => quote! { #order::read::<#ty>(__s.try_into().expect("sized")) },
            };
            let write = match (width, byte) {
                (1, true) => quote! { out.push(&[*__e])?; },
                (1, false) => quote! { out.push(&[*__e as #u8])?; },
                _ => quote! { out.push(&#order::write(*__e))?; },
            };
            Ok(Step::Fixed { len: width, read, write })
        }
        Kind::Var { .. } | Kind::Msg { .. } | Kind::Text { .. } => {
            let what = match element {
                Kind::Var { ty } | Kind::Msg { ty, .. } => ty.as_str(),
                _ => coll,
            };
            Ok(Step::Discovered {
                read: discovered_read(element, what, ctx, root)?,
                write: value_write(coll, element, quote!(__e), quote!(__e), ctx, root)?,
                what: String::from(what),
            })
        }
        other => Err(Error::Refused(format!("unsupported collection element {other:?}"))),
    }
}

impl Walk<'_> {
    pub(super) fn repeat(
        &mut self,
        item: &mut Item<'_>,
        own: Own,
        seg: &Segment,
    ) -> Result<(), Error> {
        let Prelude { err, some, none, vec, boxed, .. } = self.root.prelude();
        let (root, owner) = (self.root, self.owner);
        let Segment::Repeat { name: coll, element, count, at, collection, doc, when, .. } = seg
        else {
            unreachable!("just matched")
        };
        if collection.borrows() {
            return self.borrowed(item, own, seg);
        }
        // An optional collection is read and written in place of the field, inside its test.
        let outer = own;
        let (own, outer_parse, outer_bytes) = match when {
            Some(_) => (
                Own::Arm,
                core::mem::take(&mut self.parse_steps),
                core::mem::take(&mut self.byte_steps),
            ),
            None => (own, TokenStream::new(), TokenStream::new()),
        };
        let prefix = Prefix::of_count(count);
        item.hold(super::refuse::held_refusal(element, owner, root));
        let coll_ident = ident(coll);
        let elem_ty = kind_ty(element, root);
        let (coll_ty, coll_val) = match collection {
            Collection::Boxed => {
                (quote!(#boxed<[#elem_ty]>), quote!(#coll_ident.into_boxed_slice()))
            }
            _ => (quote!(#vec<#elem_ty>), quote!(#coll_ident)),
        };
        let d = doc_attr(doc);
        let (coll_ty, coll_val, inner_val) = match when {
            Some(_) => {
                let option = root.prelude().option;
                (quote!(#option<#coll_ty>), quote!(#coll_ident), coll_val)
            }
            None => (coll_ty, coll_val.clone(), coll_val),
        };
        self.public_fields.extend(quote! { #d pub #coll_ident: #coll_ty, });
        self.binds.push(Bind { name: coll_ident.clone(), ty: coll_ty, value: coll_val });

        let ctx = self.with_ctx(element, own)?;
        let step = element_step(element, coll, ctx.as_ref(), root)?;

        let each = match &step {
            Step::Fixed { len, .. } => Some(*len as u64 * 8),
            Step::Discovered { .. } => None,
        };
        let gate = when.as_ref().map(|when| self.presence_rows(when, coll));
        if let Some(p) = prefix {
            let name = p.row_name(coll);
            let mut row = p.row(&name);
            if let Some(gate) = &gate {
                row = row.when(gate.clone());
            }
            self.rows.push(row);
            self.rows.advance(Some(p.repr.bits()));
        }
        let mut row =
            Row::new(coll, span_of_count(count, each, coll)).decoded_by(deferred_type(element));
        if let Some(at) = at {
            row = row.seek(at);
        }
        if let Some(gate) = gate {
            row = row.when(gate);
        }
        self.rows.push(row);

        let mut stamp = TokenStream::new();
        if let Some(at) = at {
            let here = self.here();
            stamp = self.stamp_or_agree(own, coll, (SlotOf::Position, false), at, here, None)?;
        }
        let read_n = |n: TokenStream| {
            let one = step.read_one(true, root);
            let room = step.room();
            quote! {
                #n
                let mut __v: #vec<#elem_ty> = #vec::with_capacity(__n.min(#room));
                for _ in 0..__n {
                    #one
                    __v.push(__e);
                }
                let #coll_ident = __v;
            }
        };

        match count {
            Count::Field { by, cap } => {
                let n = self.env.size_n(owner, "#[count]", by, *cap)?;
                self.parse_steps.extend(read_n(n));
            }
            Count::Prefix { .. } => {
                let n = prefix.expect("a prefix count").read(coll, root);
                self.parse_steps.extend(read_n(n));
            }
            Count::Strided { by, cap, stride } => {
                let Step::Fixed { read, .. } = &step else {
                    return Err(Error::Refused(format!(
                        "{owner}: collection `{coll}` has a stride, but its elements are self-delimiting. \
                         Use a count without a stride"
                    )));
                };
                let elem_len = element_width(element, root);
                let n = self.env.size_n(owner, "#[count]", by, *cap)?;
                let sfield = blamed(stride, coll);
                let stride = self.env.size(owner, "#[stride]", stride)?;
                self.parse_steps.extend(quote! {
                    #n
                    let __stride = #stride;
                    if __stride < #elem_len {
                        return #err(#root::ParseError::Malformed { field: #sfield, at: __at });
                    }
                    let mut __v: #vec<#elem_ty> =
                        #vec::with_capacity(__n.min((__body.len() - __at) / __stride));
                    for _ in 0..__n {
                        let __step_end = __at
                            .checked_add(__stride)
                            .ok_or(#root::ParseError::Malformed { field: #sfield, at: __at })?;
                        let __s = __body.get(__at..__step_end).ok_or(
                            #root::ParseError::Short {
                                need_bytes: __step_end,
                                got_bytes: __body.len(),
                                at: __at,
                            },
                        )?;
                        let __s = &__s[..#elem_len];
                        let __e = #read;
                        __v.push(__e);
                        __at = __step_end;
                    }
                    let #coll_ident = __v;
                });
            }
            Count::Window(Len::Until { terminator }) => {
                let t = Literal::u8_unsuffixed(*terminator);
                let one = step.read_one(false, root);
                // Test for the terminator before each element. It is not an element.
                self.parse_steps.extend(quote! {
                    let mut __v: #vec<#elem_ty> = #vec::new();
                    loop {
                        match __body.get(__at) {
                            #some(&#t) => {
                                __at += 1;
                                break;
                            }
                            #some(_) => {}
                            #none => {
                                return #err(#root::ParseError::Short {
                                    need_bytes: __at + 1,
                                    got_bytes: __body.len(),
                                    at: __at,
                                });
                            }
                        }
                        #one
                        __v.push(__e);
                    }
                    let #coll_ident = __v;
                });
            }
            Count::Window(len) => {
                let bound = match prefix {
                    Some(p) => p.read(coll, root),
                    None => self.window_len(len, "collection", coll)?,
                };
                let one = step.read_one(false, root);
                let elements = quote! {
                    let mut __v: #vec<#elem_ty> = #vec::new();
                    while __at < __body.len() {
                        #one
                        __v.push(__e);
                    }
                    let __e = __v;
                };
                let stated = matches!(len, Len::Field { .. } | Len::Prefix { .. });
                let window = windowed(coll, stated, elements, root);
                self.parse_steps.extend(quote! {
                    #bound
                    #window
                    let #coll_ident = __e;
                });
            }
            Count::Terminated { mask } => {
                let mask_lit = Literal::u8_unsuffixed(*mask);
                let one = step.read_one(false, root);
                // Test the mask before the read moves the cursor past it.
                self.parse_steps.extend(quote! {
                    let mut __v: #vec<#elem_ty> = #vec::new();
                    loop {
                        let __flag = match __body.get(__at) {
                            #some(b) => (b & #mask_lit) == #mask_lit,
                            #none => {
                                return #err(#root::ParseError::Short {
                                    need_bytes: __at + 1,
                                    got_bytes: __body.len(),
                                    at: __at,
                                });
                            }
                        };
                        #one
                        __v.push(__e);
                        if __flag {
                            break;
                        }
                    }
                    let #coll_ident = __v;
                });
            }
            Count::Fill { cap } => match &step {
                Step::Fixed { len, .. } => {
                    let elem_len = Literal::usize_unsuffixed(*len);
                    let n = if *len == 1 {
                        quote! { let __n = __body.len() - __at; }
                    } else {
                        quote! {
                            let __rem = __body.len() - __at;
                            if !__rem.is_multiple_of(#elem_len) {
                                return #err(#root::ParseError::Malformed { field: #coll, at: __at });
                            }
                            let __n = __rem / #elem_len;
                        }
                    };
                    let bound = cap.map(|c| {
                        let c = Literal::usize_unsuffixed(c);
                        quote! {
                            if __n > #c {
                                return #err(#root::ParseError::Malformed { field: #coll, at: __at });
                            }
                        }
                    });
                    self.parse_steps.extend(read_n(quote! { #n #bound }));
                }
                Step::Discovered { .. } => {
                    let one = step.read_one(false, root);
                    let bound = cap.map(|c| {
                        let c = Literal::usize_unsuffixed(c);
                        quote! {
                            if __v.len() == #c {
                                return #err(#root::ParseError::Malformed { field: #coll, at: __at });
                            }
                        }
                    });
                    self.parse_steps.extend(quote! {
                        let mut __v: #vec<#elem_ty> = #vec::new();
                        while __at < __body.len() {
                            #bound
                            #one
                            __v.push(__e);
                        }
                        let #coll_ident = __v;
                    });
                }
            },
        }
        let held = own.by_ref(coll);
        let len = {
            let plain = own.get(coll);
            quote!(#plain.len())
        };
        let mut each = TokenStream::new();
        if matches!(count, Count::Window(_) | Count::Terminated { .. } | Count::Fill { .. })
            && matches!(step, Step::Discovered { .. })
        {
            each.extend(quote! {
                if out.len() == __elem {
                    return #err(#root::EncodeError::Empty { field: #coll });
                }
            });
        }
        let i64 = quote!(::core::primitive::i64);
        let (before, after) = match count {
            Count::Prefix { .. } => {
                let p = prefix.expect("a prefix count");
                (p.write(coll, len.clone(), root), quote!())
            }
            Count::Window(Len::Prefix { .. }) => {
                prefix.expect("a prefix length").reserve(coll, root)
            }
            Count::Window(Len::Field { by, cap, .. }) => (
                quote! { let __span_start = out.len(); },
                self.stamp_or_agree(
                    own,
                    coll,
                    (SlotOf::Length, false),
                    by,
                    quote!(out.len() - __span_start),
                    *cap,
                )?,
            ),
            Count::Field { by, .. } | Count::Strided { by, .. } => {
                let mut checks = TokenStream::new();
                let counts = |d: &Derived| matches!(d, Derived::Count { of, .. } if of == coll);
                if !self.writes(counts) {
                    checks.extend(self.agree(own, coll, len.clone(), by)?);
                }
                if let Count::Strided { stride, .. } = count {
                    let widths = |d: &Derived| matches!(d, Derived::Stride { elem, by } if elem == element && by == stride);
                    if !self.writes(widths) {
                        let width = element_width(element, root);
                        checks.extend(self.agree(own, coll, width, stride)?);
                    }
                }
                (checks, quote!())
            }
            Count::Window(Len::Bytes(n)) => {
                let lit = Literal::i64_unsuffixed(*n as i64);
                (
                    quote! { let __span_start = out.len(); },
                    quote! {
                        #root::EncodeError::agree(#coll, (out.len() - __span_start) as #i64, #lit)?;
                    },
                )
            }
            Count::Window(Len::Until { terminator }) => {
                let t = Literal::u8_unsuffixed(*terminator);
                each.extend(quote! {
                    if out.written()[__elem] == #t {
                        return #err(#root::EncodeError::Delimiter { field: #coll, byte: #t });
                    }
                });
                (quote!(), quote! { out.push(&[#t])?; })
            }
            // The last element sets the mask, and every element before it must leave the mask clear.
            Count::Terminated { mask } => {
                let m = Literal::u8_unsuffixed(*mask);
                each.extend(quote! {
                    if let #some(__prev) = __last
                        && out.written()[__prev] & #m == #m
                    {
                        return #err(#root::EncodeError::Delimiter { field: #coll, byte: #m });
                    }
                    __last = #some(__elem);
                });
                (
                    quote! {
                        if (#held).is_empty() {
                            return #err(#root::EncodeError::Empty { field: #coll });
                        }
                        let mut __last = #none;
                    },
                    quote! {
                        if let #some(__prev) = __last {
                            out.written_mut()[__prev] |= #m;
                        }
                    },
                )
            }
            Count::Fill { cap: Some(cap) } => {
                let lit = Literal::usize_unsuffixed(*cap);
                (quote! { #root::EncodeError::cap(#coll, #len, #lit)?; }, quote!())
            }
            _ => (quote!(), quote!()),
        };
        let write_one = step.write_one();
        self.byte_steps.extend(quote! {
            #stamp
            #before
            for __e in #held {
                let __elem = out.len();
                #write_one
                #each
            }
            #after
        });
        if let Some(when) = when {
            let (parse, bytes) = (
                core::mem::replace(&mut self.parse_steps, outer_parse),
                core::mem::replace(&mut self.byte_steps, outer_bytes),
            );
            let Prelude { some, none, .. } = root.prelude();
            let present = self.presence_read(when, coll)?;
            self.parse_steps.extend(quote! {
                let #coll_ident = if #present {
                    #parse
                    #some(#inner_val)
                } else {
                    #none
                };
            });
            self.optional_write(outer, when, (coll, quote!(#coll_ident)), bytes)?;
        }
        Ok(())
    }
}

impl Walk<'_> {
    /// A `&'a [u8]` or `&'a [u8; N]` run: a view of the input, written back in one piece.
    fn borrowed(&mut self, item: &mut Item<'_>, own: Own, seg: &Segment) -> Result<(), Error> {
        let Prelude { err, ok, u8, .. } = self.root.prelude();
        let (root, owner) = (self.root, self.owner);
        let Segment::Repeat { name: coll, count, at, collection, doc, .. } = seg else {
            unreachable!("just matched")
        };
        let Some(lifetime) = item.lifetime else {
            return Err(Error::Refused(format!(
                "{owner}: `{coll}` borrows the input, and only a message with a lifetime can"
            )));
        };
        let lifetime = syn::Lifetime::new(&format!("'{lifetime}"), proc_macro2::Span::call_site());
        let coll_ident = ident(coll);
        let coll_ty = match (collection, count) {
            (Collection::BorrowedArray, Count::Window(Len::Bytes(n))) => {
                let n = Literal::usize_unsuffixed(*n);
                quote!(&#lifetime [#u8; #n])
            }
            _ => quote!(&#lifetime [#u8]),
        };
        let d = doc_attr(doc);
        self.public_fields.extend(quote! { #d pub #coll_ident: #coll_ty, });
        self.binds.push(Bind {
            name: coll_ident.clone(),
            ty: coll_ty.clone(),
            value: quote!(#coll_ident),
        });

        let prefix = Prefix::of_count(count);
        if let Some(p) = prefix {
            let name = p.row_name(coll);
            self.rows.push(p.row(&name));
            self.rows.advance(Some(p.repr.bits()));
        }
        let mut row = Row::new(coll, span_of_count(count, Some(8), coll));
        if let Some(at) = at {
            row = row.seek(at);
        }
        self.rows.push(row);

        let n = match count {
            Count::Field { by, cap } => self.env.size_n(owner, "#[count]", by, *cap)?,
            Count::Prefix { .. } | Count::Window(Len::Prefix { .. }) => {
                prefix.expect("a prefix").read(coll, root)
            }
            Count::Window(len @ (Len::Field { .. } | Len::Bytes(_))) => {
                self.window_len(len, "collection", coll)?
            }
            Count::Fill { cap } => {
                let bound = cap.map(|c| {
                    let c = Literal::usize_unsuffixed(c);
                    quote! {
                        if __n > #c {
                            return #err(#root::ParseError::Malformed { field: #coll, at: __at });
                        }
                    }
                });
                quote! {
                    let __n = __body.len() - __at;
                    #bound
                }
            }
            other => {
                return Err(Error::Refused(format!(
                    "{owner}: `{coll}` borrows {other:?} bytes. \
                     Size it by a count, a length, a fixed number of bytes, or the rest of the body"
                )));
            }
        };
        let short = quote! {
            #root::ParseError::Short { need_bytes: __end, got_bytes: __body.len(), at: __at }
        };
        let view = match coll_ty_array(collection, count) {
            Some(len) => quote! {
                match <&[#u8; #len]>::try_from(__run) {
                    #ok(__array) => __array,
                    ::core::result::Result::Err(_) => return #err(#short),
                }
            },
            None => quote!(__run),
        };
        self.parse_steps.extend(quote! {
            #n
            let __end = __at
                .checked_add(__n)
                .ok_or(#root::ParseError::Malformed { field: #coll, at: __at })?;
            let __run = __body.get(__at..__end).ok_or(#short)?;
            let #coll_ident = #view;
            __at = __end;
        });

        let mut stamp = TokenStream::new();
        if let Some(at) = at {
            let here = self.here();
            stamp = self.stamp_or_agree(own, coll, (SlotOf::Position, false), at, here, None)?;
        }
        let held = own.get(coll);
        let (before, after) = match count {
            Count::Prefix { .. } | Count::Window(Len::Prefix { .. }) => {
                (prefix.expect("a prefix").write(coll, quote!(#held.len()), root), quote!())
            }
            Count::Window(Len::Field { by, cap, .. }) => (
                quote! { let __span_start = out.len(); },
                self.stamp_or_agree(
                    own,
                    coll,
                    (SlotOf::Length, false),
                    by,
                    quote!(out.len() - __span_start),
                    *cap,
                )?,
            ),
            Count::Field { by, .. }
                if !self.writes(|d| matches!(d, Derived::Count { of, .. } if of == coll)) =>
            {
                (self.agree(own, coll, quote!(#held.len()), by)?, quote!())
            }
            Count::Window(Len::Bytes(n)) if *collection == Collection::Borrowed => {
                let lit = Literal::i64_unsuffixed(*n as i64);
                let i64 = quote!(::core::primitive::i64);
                (quote!(), quote! { #root::EncodeError::agree(#coll, #held.len() as #i64, #lit)?; })
            }
            Count::Fill { cap: Some(cap) } => {
                let lit = Literal::usize_unsuffixed(*cap);
                (quote! { #root::EncodeError::cap(#coll, #held.len(), #lit)?; }, quote!())
            }
            _ => (quote!(), quote!()),
        };
        self.byte_steps.extend(quote! {
            #stamp
            #before
            out.push(#held)?;
            #after
        });
        Ok(())
    }
}

/// The `N` of a `&'a [u8; N]` run.
fn coll_ty_array(collection: &Collection, count: &Count) -> Option<Literal> {
    match (collection, count) {
        (Collection::BorrowedArray, Count::Window(Len::Bytes(n))) => {
            Some(Literal::usize_unsuffixed(*n))
        }
        _ => None,
    }
}
