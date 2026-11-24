//! Names bound so far, which counts, lengths, offsets and switches read their numbers from.

use proc_macro2::TokenStream;
use quote::quote;

use super::cursor::cap_guard;
use super::eval::{blamed, condition, number};
use super::patch::nested_accessors;
use crate::model::{Ty, split_ref};
use crate::walk::tokens::ident;
use crate::{Error, Expr, Root};

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

    /// What `#[with(name)]` passes, as decode reads it: an integer or `bool` field at its own
    /// type, or a field of a nested layout. The flag is whether `name` is a `bool` field.
    pub(super) fn argument(&self, name: &str) -> Option<(TokenStream, bool)> {
        let (outer, inner) = split_ref(name);
        match (self.bound(outer)?, inner) {
            (Bound::Number(read), None) => Some((read.clone(), false)),
            (Bound::Flag(read), None) => Some((read.clone(), true)),
            (Bound::Nested(access), Some(inner)) => {
                let f = ident(inner);
                Some((quote!(#access.#f), false))
            }
            _ => None,
        }
    }

    /// The type `name` reads as, if it is bound: `None` for a field of a nested layout, whose type
    /// only the compiler knows.
    pub(super) fn ty(&self, name: &str) -> Option<Ty> {
        match (self.bound(split_ref(name).0)?, split_ref(name).1) {
            (Bound::Number(_), None) => Some(Ty::Int),
            (Bound::Flag(_), None) => Some(Ty::Bool),
            _ => None,
        }
    }

    /// How `name` reads in decode, for [`eval`](super::eval): an integer as an `i64`, a `bool`
    /// as itself. `what` is the attribute named in a refusal.
    pub(super) fn read(
        &self,
        msg: &str,
        what: &str,
        name: &str,
        want: Ty,
    ) -> Result<(TokenStream, Ty), Error> {
        match self.ty(name).unwrap_or(want) {
            Ty::Int => Ok((self.get(msg, what, name)?, Ty::Int)),
            Ty::Bool => Ok((self.get_flag(msg, what, name)?, Ty::Bool)),
        }
    }

    /// `e` as an `i64` read off the wire, or `Malformed` naming the first field it reads.
    pub(super) fn number(&self, msg: &str, what: &str, e: &Expr) -> Result<TokenStream, Error> {
        let root = &self.root;
        let n = number(e, &|name, want| self.read(msg, what, name, want), root)?;
        let field = blamed(e, msg);
        Ok(quote!(#n.ok_or(#root::ParseError::Malformed { field: #field, at: __at })?))
    }

    /// `e` as a `bool` read off the wire, or `Malformed` naming the first field it reads.
    pub(super) fn condition(&self, msg: &str, what: &str, e: &Expr) -> Result<TokenStream, Error> {
        let root = &self.root;
        let c = condition(e, &|name, want| self.read(msg, what, name, want), root)?;
        let field = blamed(e, msg);
        Ok(quote!(#c.ok_or(#root::ParseError::Malformed { field: #field, at: __at })?))
    }

    /// `e` as a `usize`: `Malformed` if it is undefined or negative.
    pub(super) fn size(&self, msg: &str, what: &str, e: &Expr) -> Result<TokenStream, Error> {
        let root = &self.root;
        let n = number(e, &|name, want| self.read(msg, what, name, want), root)?;
        let field = blamed(e, msg);
        Ok(quote! {
            #root::__private::value::size(#n)
                .ok_or(#root::ParseError::Malformed { field: #field, at: __at })?
        })
    }

    /// `let __n = ..;`: read a count or length, and check its cap.
    pub(super) fn size_n(
        &self,
        msg: &str,
        what: &str,
        e: &Expr,
        cap: Option<usize>,
    ) -> Result<TokenStream, Error> {
        let n = self.size(msg, what, e)?;
        let cap = cap_guard(cap, blamed(e, msg), &self.root);
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
