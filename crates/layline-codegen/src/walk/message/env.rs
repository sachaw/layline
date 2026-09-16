//! Names bound so far, which counts, lengths, offsets and switches read their numbers from.

use proc_macro2::TokenStream;
use quote::quote;

use super::cursor::cap_guard;
use super::patch::nested_accessors;
use crate::model::split_ref;
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::{By, Error, Root};

/// Every name bound so far, in wire order.
pub(super) struct Env {
    values: Vec<(String, Bound)>,
    root: Root,
}

/// What a name reads as.
enum Bound {
    /// An integer field: the expression that reads it, at its own type.
    Number(TokenStream),
    /// A `bool` field: the expression that reads it. Number lookups skip it.
    Flag(TokenStream),
    /// A nested layout: the expression that reads it. Dotted references read its fields.
    Nested(TokenStream),
}

impl Env {
    pub(super) fn new(root: &Root) -> Self {
        Self { values: Vec::new(), root: root.clone() }
    }

    fn bound(&self, name: &str) -> Option<&Bound> {
        self.values.iter().find(|(n, _)| n == name).map(|(_, b)| b)
    }

    pub(super) fn bind(&mut self, name: &str, access: TokenStream) {
        self.values.push((String::from(name), Bound::Number(access)));
    }

    pub(super) fn bind_flag(&mut self, name: &str, access: TokenStream) {
        let root = &self.root;
        self.values.push((
            String::from(name),
            Bound::Flag(quote!(#root::__private::WireFlag::to_flag(&#access))),
        ));
    }

    pub(super) fn bind_nested(&mut self, name: &str, access: TokenStream) {
        self.values.push((String::from(name), Bound::Nested(access)));
    }

    /// The expression that reads integer `name`, at its declared type.
    ///
    /// [`get`](Self::get) widens it to `i64`.
    pub(super) fn number(&self, name: &str) -> Option<&TokenStream> {
        match self.bound(name)? {
            Bound::Number(read) => Some(read),
            _ => None,
        }
    }

    /// `usize::try_from(field * scale + offset)?`: `Malformed` if it does not fit a `usize`.
    pub(super) fn stated(&self, msg: &str, what: &str, by: &By) -> Result<TokenStream, Error> {
        let (root, field) = (&self.root, by.field.as_str());
        let Prelude { usize, .. } = root.prelude();
        let raw = affine(self.get(msg, what, field)?, by, root);
        Ok(quote! {
            #usize::try_from(#raw)
                .map_err(|_| #root::ParseError::Malformed { field: #field, at: __at })?
        })
    }

    /// `let __n = ..;`: read a count or length from a field, and check its cap.
    pub(super) fn stated_n(
        &self,
        msg: &str,
        what: &str,
        by: &By,
        cap: Option<usize>,
    ) -> Result<TokenStream, Error> {
        let n = self.stated(msg, what, by)?;
        let cap = cap_guard(cap, &by.field, &self.root);
        Ok(quote! {
            let __n = #n;
            #cap
        })
    }

    /// The integer `field` refers to, as an `i64`. `what` is the attribute named in a refusal.
    pub(super) fn get(&self, msg: &str, what: &str, field: &str) -> Result<TokenStream, Error> {
        let root = &self.root;
        let (outer, inner) = split_ref(field);
        match (self.bound(outer), inner) {
            (Some(Bound::Number(read)), None) => Ok(quote!(#root::WireInt::to_i64(&#read))),
            (Some(Bound::Nested(access)), Some(inner)) => {
                let (get, _) = nested_accessors(msg, outer, inner);
                Ok(quote!(#root::WireInt::to_i64(#get(&#access))))
            }
            (Some(Bound::Nested(_)), None) => Err(Error::Refused(format!(
                "{msg}: {what} refers to `{field}`, which is a nested layout. \
                 Name the integer field inside it: `{field}.<field>`"
            ))),
            (Some(Bound::Flag(_)), None) => Err(Error::Refused(format!(
                "{msg}: {what} refers to `{field}`, which is a `bool`. Name an integer field"
            ))),
            (Some(Bound::Number(_) | Bound::Flag(_)), Some(_)) => Err(Error::Refused(format!(
                "{msg}: {what} refers to `{field}`, but `{outer}` is not a nested layout. \
                 Name `{outer}` itself"
            ))),
            (None, None) => Err(Error::Refused(format!(
                "{msg}: {what} refers to `{field}`, which is not an earlier integer field. \
                 Name an earlier integer field of this message"
            ))),
            (None, Some(_)) => Err(Error::Refused(format!(
                "{msg}: {what} refers to `{field}`, but `{outer}` is not an earlier nested layout. \
                 Name an earlier field of this message"
            ))),
        }
    }

    /// The flag `field` refers to, as a `bool`. `what` is the attribute named in a refusal.
    pub(super) fn get_flag(
        &self,
        msg: &str,
        what: &str,
        field: &str,
    ) -> Result<TokenStream, Error> {
        let root = &self.root;
        let (outer, inner) = split_ref(field);
        match (self.bound(outer), inner) {
            (Some(Bound::Flag(read)), None) => Ok(read.clone()),
            (Some(Bound::Nested(access)), Some(inner)) => {
                let f = ident(inner);
                Ok(quote!(#root::__private::WireFlag::to_flag(&#access.#f)))
            }
            (Some(Bound::Number(_)), None) => Err(Error::Refused(format!(
                "{msg}: {what} tests `{field}` as a `bool`, but `{field}` is an integer. \
                 Write `{field} & 0x01`"
            ))),
            (Some(Bound::Nested(_)), None) => Err(Error::Refused(format!(
                "{msg}: {what} refers to `{field}`, which is a nested layout. \
                 Name the `bool` inside it: `{field}.<field>`"
            ))),
            (Some(Bound::Number(_) | Bound::Flag(_)), Some(_)) => Err(Error::Refused(format!(
                "{msg}: {what} refers to `{field}`, but `{outer}` is not a nested layout. \
                 Name `{outer}` itself"
            ))),
            (None, None) => Err(Error::Refused(format!(
                "{msg}: {what} refers to `{field}`, which is not an earlier `bool` field. \
                 Name an earlier `bool` field of this message"
            ))),
            (None, Some(_)) => Err(Error::Refused(format!(
                "{msg}: {what} refers to `{field}`, but `{outer}` is not an earlier nested layout. \
                 Name an earlier field of this message"
            ))),
        }
    }
}

/// `value * scale + offset` in checked `i64`, because the value comes from untrusted input.
///
/// The expression ends in `?`, so it is a plain `i64` where used.
fn affine(value: TokenStream, by: &By, root: &Root) -> TokenStream {
    let (s, o) = (i64::from(by.scale), i64::from(by.offset));
    let field = by.field.as_str();
    let malformed = quote!(.ok_or(#root::ParseError::Malformed { field: #field, at: __at })?);
    match (by.scale, by.offset) {
        (1, 0) => value,
        (1, _) => quote!((#value).checked_add(#o)#malformed),
        (_, 0) => quote!((#value).checked_mul(#s)#malformed),
        _ => quote!((#value).checked_mul(#s).and_then(|__v| __v.checked_add(#o))#malformed),
    }
}
