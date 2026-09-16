//! Collections: one element type, repeated by a count, a window, a terminator or to the end.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use super::body::{Bind, Item, Own, Walk};
use super::cursor::{cap_guard, consumed_nothing, take, windowed};
use super::patch::{StampAt, affine_inverse, stamp_slot};
use super::table::{Row, deferred_type, span_of_count};
use super::value::{WithCtx, discovered_read, value_write};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{SlotOf, doc_attr, kind_ty, path, scalar_ty};
use crate::{Collection, Count, Endian, Error, Kind, Len, Root, Scalar, Segment};

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
    endian: Endian,
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
                write: quote! { out.push(&__e.encode())?; },
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
            let (from, to) = match endian {
                Endian::Be => (quote!(from_be_bytes), quote!(to_be_bytes)),
                Endian::Le => (quote!(from_le_bytes), quote!(to_le_bytes)),
            };
            let byte = matches!(s, Scalar::U(8));
            let read = match (width, byte) {
                (1, true) => quote! { __s[0] },
                (1, false) => quote! { __s[0] as #ty },
                _ => quote! { #ty::#from(__s.try_into().expect("sized")) },
            };
            let write = match (width, byte) {
                (1, true) => quote! { out.push(&[*__e])?; },
                (1, false) => quote! { out.push(&[*__e as #u8])?; },
                _ => quote! { out.push(&__e.#to())?; },
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
        let Segment::Repeat { name: coll, element, count, at, collection, doc, .. } = seg else {
            unreachable!("just matched")
        };
        let coll_ident = ident(coll);
        let elem_ty = kind_ty(element, root);
        let (coll_ty, coll_val) = match collection {
            Collection::Boxed => {
                (quote!(#boxed<[#elem_ty]>), quote!(#coll_ident.into_boxed_slice()))
            }
            _ => (quote!(#vec<#elem_ty>), quote!(#coll_ident)),
        };
        let d = doc_attr(doc);
        self.public_fields.extend(quote! { #d pub #coll_ident: #coll_ty, });
        self.binds.push(Bind { name: coll_ident.clone(), ty: coll_ty, value: coll_val });

        let ctx = self.with_ctx(element, own)?;
        let step = element_step(element, item.endian, coll, ctx.as_ref(), root)?;

        let each = match &step {
            Step::Fixed { len, .. } => Some(*len as u64 * 8),
            Step::Discovered { .. } => None,
        };
        let mut row = Row::new(coll, span_of_count(count, each)).decoded_by(deferred_type(element));
        if let Some(field) = at {
            row = row.seek(field);
        }
        self.rows.push(row);

        let mut stamp = TokenStream::new();
        if let Some(field) = at {
            let here = self.here();
            stamp = stamp_slot(
                &self.slots,
                StampAt {
                    owner,
                    coll,
                    of: SlotOf::Position,
                    field,
                    endian: item.endian,
                    record: false,
                },
                &here,
                root,
            )?;
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
                let n = self.env.stated_n(owner, "#[count]", by, *cap)?;
                self.parse_steps.extend(read_n(n));
            }
            Count::Squared { by, cap } => {
                let field = by.field.as_str();
                let order = self.env.stated(owner, "#[count]", by)?;
                let cap_check = cap_guard(*cap, field, root);
                self.parse_steps.extend(read_n(quote! {
                    let __side = #order;
                    let __n = __side
                        .checked_mul(__side)
                        .ok_or(#root::ParseError::Malformed { field: #field, at: __at })?;
                    #cap_check
                }));
            }
            Count::Strided { by, cap, stride } => {
                let Step::Fixed { read, .. } = &step else {
                    return Err(Error::Refused(format!(
                        "{owner}: collection `{coll}` has a stride, but its elements are self-delimiting. \
                         Use a count without a stride"
                    )));
                };
                let elem_len = element_width(element, root);
                let n = self.env.stated_n(owner, "#[count]", by, *cap)?;
                let sfield = stride.field.as_str();
                let stride = self.env.stated(owner, "#[stride]", stride)?;
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
                let bound = self.window_len(len, "collection", coll)?;
                let one = step.read_one(false, root);
                let elements = quote! {
                    let mut __v: #vec<#elem_ty> = #vec::new();
                    while __at < __body.len() {
                        #one
                        __v.push(__e);
                    }
                    let __e = __v;
                };
                let window = windowed(coll, matches!(len, Len::Field { .. }), elements, root);
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
        let mut each = TokenStream::new();
        if matches!(count, Count::Window(_) | Count::Terminated { .. } | Count::Fill { .. })
            && matches!(step, Step::Discovered { .. })
        {
            let msg = format!(
                "collection `{coll}`: an element encoded to zero bytes. \
                 Decode rejects that in a run without a count"
            );
            each.extend(quote! { ::core::assert!(out.len() > __elem, #msg); });
        }
        let (before, after) = match count {
            // The holder writes a parameter. Encode writes only the elements.
            Count::Window(Len::Field { by, .. }) if self.is_param(&by.field) => {
                (quote!(), quote!())
            }
            Count::Window(Len::Field { by, cap }) => (
                quote! { let __span_start = out.len(); },
                stamp_slot(
                    &self.slots,
                    StampAt {
                        owner,
                        coll,
                        of: SlotOf::Length,
                        field: &by.field,
                        endian: item.endian,
                        record: false,
                    },
                    &affine_inverse(
                        quote!(out.len() - __span_start),
                        (by.scale, by.offset),
                        *cap,
                        (&by.field, "byte length of a collection"),
                        root,
                    ),
                    root,
                )?,
            ),
            Count::Window(Len::Bytes(n)) => {
                let lit = Literal::usize_unsuffixed(*n);
                let msg = format!(
                    "collection `{coll}` is {n} bytes and its elements did not fill it exactly"
                );
                (
                    quote! { let __span_start = out.len(); },
                    quote! { ::core::assert!(out.len() - __span_start == #lit, #msg); },
                )
            }
            Count::Window(Len::Until { terminator }) => {
                let t = Literal::u8_unsuffixed(*terminator);
                let msg = format!(
                    "collection `{coll}`: an element starts with terminator byte {terminator:#04x}. \
                     Decode would end the run there"
                );
                each.extend(quote! { ::core::assert!(out.written()[__elem] != #t, #msg); });
                (quote!(), quote! { out.push(&[#t])?; })
            }
            // The last element sets the mask, and every element before it must leave the mask clear.
            Count::Terminated { mask } => {
                let m = Literal::u8_unsuffixed(*mask);
                let empty = format!(
                    "collection `{coll}` is empty, but a run ended by mask {mask:#04x} needs an element"
                );
                let early = format!(
                    "collection `{coll}`: an element before the last has mask {mask:#04x} set. \
                     Decode would end the run there"
                );
                each.extend(quote! {
                    if let #some(__prev) = __last {
                        ::core::assert!(out.written()[__prev] & #m != #m, #early);
                    }
                    __last = #some(__elem);
                });
                (
                    quote! {
                        ::core::assert!(!(#held).is_empty(), #empty);
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
                let msg = format!("collection `{coll}` passes its cap of {cap} elements");
                (quote! { ::core::assert!((#held).len() <= #lit, #msg); }, quote!())
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
        Ok(())
    }
}
