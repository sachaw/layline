//! A self-delimiting value: a `VarCodec`, a nested message, or text.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use super::body::{Bind, Item, Own, Walk};
use super::cursor::{consumed_nothing, take};
use super::env::Env;
use super::patch::{Target, derived_value, fits, too_narrow};
use super::table::{Row, deferred_type, span_of_kind};
use super::text::{text_decode, text_unpad, text_write};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{Derived, doc_attr, kind_ty, path};
use crate::{Error, Kind, Len, Root, Segment};

/// The `Message::Ctx` a use site builds, for decode and for encode.
///
/// Decode takes values already read. Encode takes them from the value being written.
pub(super) struct WithCtx {
    pub(super) decode: TokenStream,
    pub(super) encode: TokenStream,
}

/// `let (__e, __used) = ..;`: read a self-delimiting value from the rest of the body.
///
/// `what` names the value in errors.
pub(super) fn discovered_read(
    kind: &Kind,
    what: &str,
    ctx: Option<&WithCtx>,
    root: &Root,
) -> Result<TokenStream, Error> {
    let Prelude { boxed, .. } = root.prelude();
    match kind {
        Kind::Msg { ty, boxed: b, .. } => {
            let p = path(ty);
            let carry = b.then(|| quote! { let __e = #boxed::new(__e); });
            let stated = ctx.map_or_else(|| quote!(()), |c| c.decode.clone());
            Ok(quote! {
                let (__e, __used) =
                    <#p as #root::Message>::decode_with_nested(&__body[__at..], __depth + 1, #stated)
                        .map_err(|__err| __err.rebased(__at))?;
                #carry
            })
        }
        Kind::Var { ty } => {
            let p = path(ty);
            Ok(quote! {
                let (__e, __used) = <#p as #root::VarCodec>::decode(&__body[__at..])
                    .map_err(|__e| match __e.rebased(__at) {
                        #root::ParseError::Malformed { at, .. } => {
                            #root::ParseError::Malformed { field: #what, at }
                        }
                        __other => __other,
                    })?;
            })
        }
        Kind::Text { len: Len::Until { terminator }, codec } => {
            let term = Literal::u8_unsuffixed(*terminator);
            let decode = text_decode(codec, what, root);
            Ok(quote! {
                let (__e, __used) = {
                    let __rest = &__body[__at..];
                    let __end = __rest
                        .iter()
                        .position(|__b| *__b == #term)
                        .ok_or(#root::ParseError::Malformed { field: #what, at: __at })?;
                    let __s = &__rest[..__end];
                    (#decode, __end + 1)
                };
            })
        }
        Kind::Text { len: Len::Bytes(n), codec } => {
            let lit = Literal::usize_unsuffixed(*n);
            let bytes = take(&quote!(#lit), root);
            let decode = text_decode(codec, what, root);
            let unpad = text_unpad();
            Ok(quote! {
                let (__e, __used) = {
                    let __s = #bytes;
                    #unpad
                    (#decode, #lit)
                };
            })
        }
        Kind::Text { len, .. } => Err(Error::Refused(format!(
            "text element with length {len:?} does not delimit itself. \
             Use a fixed byte count or a terminator"
        ))),
        other => Err(Error::Refused(format!("unsupported collection element {other:?}"))),
    }
}

/// Decode a value segment into a local named after the field.
pub(super) fn value_read(
    env: &Env,
    owner: &str,
    field: &str,
    kind: &Kind,
    ctx: Option<&WithCtx>,
    root: &Root,
) -> Result<TokenStream, Error> {
    let ident = ident(field);
    match kind {
        Kind::Text { len: Len::Field { by, cap }, codec } => {
            let decode = text_decode(codec, field, root);
            let n = env.stated_n(owner, "#[count]", by, *cap)?;
            let by = by.field.as_str();
            Ok(quote! {
                #n
                let __end = __at
                    .checked_add(__n)
                    .ok_or(#root::ParseError::Malformed { field: #by, at: __at })?;
                let #ident = {
                    let __s = __body.get(__at..__end).ok_or(#root::ParseError::Short {
                        need_bytes: __end,
                        got_bytes: __body.len(),
                        at: __at,
                    })?;
                    #decode
                };
                __at = __end;
            })
        }
        Kind::Text { len: Len::Fill, codec } => {
            let decode = text_decode(codec, field, root);
            Ok(quote! {
                let #ident = {
                    let __s = &__body[__at..];
                    #decode
                };
                __at = __body.len();
            })
        }
        Kind::Msg { .. } | Kind::Var { .. } | Kind::Text { .. } => {
            let read = discovered_read(kind, field, ctx, root)?;
            let empty = matches!(kind, Kind::Var { .. }).then(|| consumed_nothing(field, root));
            Ok(quote! {
                #read
                #empty
                let #ident = __e;
                __at += __used;
            })
        }
        other => Err(Error::Refused(format!(
            "`{field}`: a value segment must be self-delimiting, and {other:?} has a fixed width"
        ))),
    }
}

/// Encode a derived `VarCodec` number.
fn value_write_derived(
    field: &str,
    kind: &Kind,
    d: &Derived,
    own: Own,
    root: &Root,
) -> Result<TokenStream, Error> {
    let Kind::Var { ty } = kind else {
        return Err(Error::Refused(format!(
            "`{field}`: this field's value is derived, but {kind:?} is not an integer. \
             Declare an integer"
        )));
    };
    let p = path(ty);
    let n = derived_value(d, own, field, root);
    let fits = fits(Target::Field(&quote!(#p), 0), &n, &too_narrow(field, d.what()), root);
    Ok(quote! {
        {
            #fits
            let __derived = <#p as #root::WireInt>::from_i64(#n);
            #root::VarCodec::encode(&__derived, out)?;
        }
    })
}

/// Encode a self-delimiting value, given by value as `held` and by reference as `by_ref`.
pub(super) fn value_write(
    field: &str,
    kind: &Kind,
    held: TokenStream,
    by_ref: TokenStream,
    ctx: Option<&WithCtx>,
    root: &Root,
) -> Result<TokenStream, Error> {
    match kind {
        Kind::Var { .. } => Ok(quote! {
            #root::VarCodec::encode(#by_ref, out)?;
        }),
        Kind::Msg { ty, .. } => {
            // `&Box<T>` coerces to `&T`.
            let p = path(ty);
            let stated = match ctx {
                Some(WithCtx { encode, .. }) => quote!(#encode),
                None => quote!(()),
            };
            Ok(quote! {
                <#p as #root::Message>::encode_into_with(#by_ref, out, #stated)?;
            })
        }
        Kind::Text { len, codec } => Ok(text_write((field, held), len, codec, root)),
        other => Err(Error::Refused(format!(
            "`{field}`: a value segment must be self-delimiting, and {other:?} has a fixed width"
        ))),
    }
}

impl Walk<'_> {
    /// The context passed to a nested message, or `None` if it takes no parameters.
    ///
    /// Each `#[with]` name is a parameter of this body or an earlier field.
    pub(super) fn with_ctx(&self, kind: &Kind, own: Own) -> Result<Option<WithCtx>, Error> {
        let (root, owner) = (self.root, self.owner);
        let Kind::Msg { ty, with, .. } = kind else { return Ok(None) };
        if with.is_empty() {
            return Ok(None);
        }
        let p = path(ty);
        let mut read = Vec::new();
        let mut held = Vec::new();
        for name in with {
            if self.is_param(name) {
                let id = ident(name);
                read.push(quote!(__ctx.#id));
                held.push(quote!(__ctx.#id));
                continue;
            }
            let Some(access) = self.env.number(name) else {
                return Err(Error::Refused(format!(
                    "{owner}: `#[with({name})]`: `{name}` is not an earlier integer field or a parameter. \
                     Name one of those"
                )));
            };
            read.push(access.clone());
            held.push(own.get(name));
        }
        let ctx = quote!(<#p as #root::Message>::Ctx::new);
        Ok(Some(WithCtx { decode: quote!(#ctx(#(#read),*)), encode: quote!(#ctx(#(#held),*)) }))
    }

    pub(super) fn value(
        &mut self,
        item: &mut Item<'_>,
        own: Own,
        seg: &Segment,
        last: bool,
    ) -> Result<(), Error> {
        let Segment::Value { name: field, kind, doc, .. } = seg else {
            unreachable!("just matched")
        };
        let ident = ident(field);
        let ty = kind_ty(kind, self.root);
        let d = doc_attr(doc);
        self.public_fields.extend(quote! { #d pub #ident: #ty, });
        self.binds.push(Bind { name: ident.clone(), ty, value: quote!(#ident) });
        self.rows.push(Row::new(field, span_of_kind(kind)).decoded_by(deferred_type(kind)));
        let ctx = self.with_ctx(kind, own)?;
        self.parse_steps.extend(value_read(
            &self.env,
            self.owner,
            field,
            kind,
            ctx.as_ref(),
            self.root,
        )?);
        self.byte_steps.extend(match self.derived.iter().find(|(df, _)| df == field) {
            Some((_, d)) => value_write_derived(field, kind, d, own, self.root)?,
            None => value_write(
                field,
                kind,
                own.get(field),
                own.by_ref(field),
                ctx.as_ref(),
                self.root,
            )?,
        });
        self.held_message(item, field, kind, last)?;
        if kind.is_integer() {
            self.env.bind(field, quote!(#ident));
        }
        Ok(())
    }
}
