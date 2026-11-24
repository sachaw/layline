//! The one evaluator: an [`Expr`] as Rust over its names' values, for decode and encode alike.
//!
//! An expression evaluates to an `Option`, through `__private::value`: `None` is a number the
//! expression does not have. Decode refuses it as `Malformed`, and encode as the declaration it
//! breaks.

use proc_macro2::{Literal, TokenStream};
use quote::quote;

use crate::model::{Inverse, Ty, Undo};
use crate::root::Prelude;
use crate::walk::path;
use crate::walk::tokens::ident;
use crate::{Error, Expr, Op, Root};

/// How a name reads where the expression is evaluated: the value as an `i64` or a `bool`, and
/// which. `Ty` is the type the place wants, which a field of a nested layout takes.
pub(super) type Lookup<'a> = dyn Fn(&str, Ty) -> Result<(TokenStream, Ty), Error> + 'a;

/// `e` as an `Option<i64>` or `Option<bool>`, and which.
fn eval(e: &Expr, want: Ty, look: &Lookup<'_>, root: &Root) -> Result<(TokenStream, Ty), Error> {
    let Prelude { some, .. } = root.prelude();
    let v = quote!(#root::__private::value);
    Ok(match e {
        Expr::Int(n) => {
            let n = Literal::i64_suffixed(*n);
            (quote!(#some(#n)), Ty::Int)
        }
        Expr::Const(c) => {
            let p = path(c);
            (quote!(#some(#root::WireInt::to_i64(&#p))), Ty::Int)
        }
        Expr::Name(name) => {
            let (read, ty) = look(name, want)?;
            (quote!(#some(#read)), ty)
        }
        Expr::Neg(x) => {
            let x = number(x, look, root)?;
            (quote!(#v::neg(#x)), Ty::Int)
        }
        Expr::Not(x) => {
            let x = condition(x, look, root)?;
            (quote!(#v::not(#x)), Ty::Bool)
        }
        Expr::If(c, a, b) => {
            let c = condition(c, look, root)?;
            let (a, ty) = eval(a, want, look, root)?;
            let (b, other) = eval(b, ty, look, root)?;
            if other != ty {
                return Err(Error::Refused(format!(
                    "`{e}` gives a `bool` one way and an integer the other"
                )));
            }
            (quote!(#v::pick(#c, || #a, || #b)), ty)
        }
        Expr::Binary(op, a, b) if op.joins() => {
            let (a, b) = (condition(a, look, root)?, condition(b, look, root)?);
            let f = if *op == Op::And { quote!(both) } else { quote!(either) };
            (quote!(#v::#f(#a, || #b)), Ty::Bool)
        }
        Expr::Binary(op, a, b) => {
            let (a, ty) = eval(a, Ty::Int, look, root)?;
            let (b, _) = eval(b, ty, look, root)?;
            let f = match op {
                Op::Mul => quote!(mul),
                Op::Div => quote!(div),
                Op::Rem => quote!(rem),
                Op::Add => quote!(add),
                Op::Sub => quote!(sub),
                Op::Shl => quote!(shl),
                Op::Shr => quote!(shr),
                Op::BitAnd => quote!(bit_and),
                Op::BitXor => quote!(bit_xor),
                Op::BitOr => quote!(bit_or),
                Op::Eq => quote!(eq),
                Op::Ne => quote!(ne),
                Op::Lt => quote!(lt),
                Op::Le => quote!(le),
                Op::Gt => quote!(gt),
                Op::Ge => quote!(ge),
                Op::And | Op::Or => unreachable!("joined above"),
            };
            let ty = if op.compares() { Ty::Bool } else { Ty::Int };
            (quote!(#v::#f(#a, #b)), ty)
        }
    })
}

/// `e` as an `Option<i64>` or `Option<bool>`, as its operators type it, and which: a value passed to
/// a parameter.
pub(super) fn argument(
    e: &Expr,
    look: &Lookup<'_>,
    root: &Root,
) -> Result<(TokenStream, Ty), Error> {
    eval(e, Ty::Int, look, root)
}

/// `e` as an `Option<i64>`.
pub(super) fn number(e: &Expr, look: &Lookup<'_>, root: &Root) -> Result<TokenStream, Error> {
    let (tokens, ty) = eval(e, Ty::Int, look, root)?;
    match ty {
        Ty::Int => Ok(tokens),
        Ty::Bool => Err(Error::Refused(format!("`{e}` is a condition where a number is needed"))),
    }
}

/// `e` as an `Option<bool>`. A bit test holds when any bit is set.
pub(super) fn condition(e: &Expr, look: &Lookup<'_>, root: &Root) -> Result<TokenStream, Error> {
    let (tokens, ty) = eval(e, Ty::Bool, look, root)?;
    Ok(match ty {
        Ty::Bool => tokens,
        Ty::Int => quote!(#root::__private::value::any_bit(#tokens)),
    })
}

/// The name a decode refusal of `e` reports: the first field it reads, or `fallback`.
pub(super) fn blamed<'e>(e: &'e Expr, fallback: &'e str) -> &'e str {
    e.names().first().copied().unwrap_or(fallback)
}

/// How a parameter reads in encode, for the side of an invertible expression that is not its
/// field: from the context the holder passed. A `bool` reads as 1 or 0, which a condition tests.
pub(super) fn params(name: &str, _: Ty, root: &Root) -> Result<(TokenStream, Ty), Error> {
    let id = ident(name);
    Ok((quote!(#root::__private::value::Number::number(&__ctx.#id)), Ty::Int))
}

/// The value of the field `inv` names, where `e` equals the `i64` `target`, as an `i64`.
///
/// Encode refuses with `Inexact`, naming `field`, where no value of the field gives `target`:
/// the steps back have no exact answer, or decode would read a different number from it.
pub(super) fn solve(
    e: &Expr,
    inv: &Inverse<'_>,
    target: TokenStream,
    field: &str,
    root: &Root,
) -> Result<TokenStream, Error> {
    let Prelude { i64, err, some, .. } = root.prelude();
    if inv.steps.is_empty() {
        return Ok(quote!(#root::__private::value::Wide::wide(#target)));
    }
    let look = |name: &str, ty: Ty| params(name, ty, root);
    let back = undo(&inv.steps, quote!(#some(__target)), &look, root)?;
    let solved = inv.field;
    let forward = number(
        e,
        &|name: &str, ty: Ty| {
            if name == solved { Ok((quote!(__solved), Ty::Int)) } else { params(name, ty, root) }
        },
        root,
    )?;
    Ok(quote! {{
        let __target: #i64 = #root::__private::value::Wide::wide(#target);
        match #back {
            #some(__solved) if #forward == #some(__target) => __solved,
            _ => {
                return #err(#root::EncodeError::Inexact { field: #field, value: __target });
            }
        }
    }})
}

/// `back`, an `Option<i64>`, worked back through `steps`, whose constant sides read through `look`.
fn undo(
    steps: &[Undo<'_>],
    mut back: TokenStream,
    look: &Lookup<'_>,
    root: &Root,
) -> Result<TokenStream, Error> {
    let v = quote!(#root::__private::value);
    for step in steps {
        let k = |k: &Expr| number(k, look, root);
        back = match step {
            Undo::Sub(k_) => {
                let k = k(k_)?;
                quote!(#v::sub(#back, #k))
            }
            Undo::Add(k_) => {
                let k = k(k_)?;
                quote!(#v::add(#back, #k))
            }
            Undo::SubFrom(k_) => {
                let k = k(k_)?;
                quote!(#v::sub(#k, #back))
            }
            Undo::Exactly(k_) => {
                let k = k(k_)?;
                quote!(#v::exactly(#back, #k))
            }
            Undo::Times(k_) => {
                let k = k(k_)?;
                quote!(#v::mul(#back, #k))
            }
            Undo::Unshift(k_) => {
                let k = k(k_)?;
                quote!(#v::unshifted(#back, #k))
            }
            Undo::Shift(k_) => {
                let k = k(k_)?;
                quote!(#v::shl(#back, #k))
            }
            Undo::Neg => quote!(#v::neg(#back)),
            Undo::Root => quote!(#v::root(#back)),
            Undo::Pick(c, then, otherwise) => {
                let c = condition(c, look, root)?;
                let a = undo(then, back.clone(), look, root)?;
                let b = undo(otherwise, back, look, root)?;
                quote!(#v::pick(#c, || #a, || #b))
            }
        };
    }
    Ok(back)
}
