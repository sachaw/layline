//! Expressions: every name resolves to an earlier field or a parameter, of the right type.

use alloc::format;
use alloc::string::String;

use super::Invalid;
use super::names::{Reads, Seen, resolve};
use crate::model::{Ty, split_ref};
use crate::{Expr, Kind, Op, Scalar};

/// Checks that `e` gives a number: a count, length, stride, offset or discriminant.
pub(super) fn number(seen: &Seen<'_>, e: &Expr) -> Result<(), Invalid> {
    if e.names().is_empty() {
        return Err(Invalid::Other(format!(
            "`{e}` reads no field or parameter, so it is a fixed number. State a fixed size instead"
        )));
    }
    match check(seen, e, Ty::Int)? {
        Ty::Int => Ok(()),
        Ty::Bool if e.as_name().is_some() => Err(Invalid::NonIntegerReference(format!("{e}"))),
        Ty::Bool => Err(Invalid::Other(format!(
            "`{e}` is a condition, and a number is needed here. Write an integer expression"
        ))),
    }
}

/// Checks that `e` is a condition: a `bool`, or a bit test that holds when any bit is set.
pub(super) fn condition(seen: &Seen<'_>, e: &Expr) -> Result<(), Invalid> {
    let ty = check(seen, e, Ty::Bool)?;
    if conditional(e, ty) {
        return Ok(());
    }
    Err(Invalid::Other(format!(
        "`{e}` is an integer, and a condition is needed here. \
         Compare it (`{e} != 0`), or test its bits (`{e} & 0x01`)"
    )))
}

/// Whether `e` can stand where a condition is needed: a `bool`, or the result of `&`, `|`, `^`.
fn conditional(e: &Expr, ty: Ty) -> bool {
    ty == Ty::Bool || matches!(e, Expr::Binary(op, ..) if op.on_bits())
}

/// The type of `e`, given the type its place wants. A field of a nested layout takes the wanted
/// type, since only the compiler knows its own.
fn check(seen: &Seen<'_>, e: &Expr, want: Ty) -> Result<Ty, Invalid> {
    let int = |x: &Expr| -> Result<(), Invalid> {
        match check(seen, x, Ty::Int)? {
            Ty::Int => Ok(()),
            Ty::Bool if x.as_name().is_some() => Err(Invalid::NonIntegerReference(format!("{x}"))),
            Ty::Bool => Err(Invalid::Other(format!(
                "`{x}` is a `bool`, and `{e}` needs an integer there. Compare it, or name an \
                 integer"
            ))),
        }
    };
    let cond = |x: &Expr| -> Result<(), Invalid> {
        let ty = check(seen, x, Ty::Bool)?;
        if conditional(x, ty) {
            Ok(())
        } else {
            Err(Invalid::Other(format!(
                "`{x}` is an integer, and `{e}` needs a condition there. \
                 Compare it (`{x} != 0`), or test its bits (`{x} & 0x01`)"
            )))
        }
    };
    match e {
        Expr::Int(_) | Expr::Const(_) => Ok(Ty::Int),
        Expr::Name(name) => name_ty(seen, name, want),
        Expr::Neg(x) => int(x).map(|()| Ty::Int),
        Expr::Not(x) => match check(seen, x, Ty::Bool)? {
            Ty::Bool => Ok(Ty::Bool),
            Ty::Int => Err(Invalid::Other(format!(
                "`!` on an integer flips its bits, so it does not test them. Write `{x} == 0`"
            ))),
        },
        Expr::Binary(op, a, b) if op.joins() => {
            cond(a)?;
            cond(b)?;
            Ok(Ty::Bool)
        }
        Expr::Binary(Op::Eq | Op::Ne, a, b) => {
            let left = check(seen, a, Ty::Int)?;
            let right = check(seen, b, left)?;
            if left != right {
                return Err(Invalid::Other(format!(
                    "`{e}` compares a `bool` with an integer. Compare like with like"
                )));
            }
            Ok(Ty::Bool)
        }
        Expr::Binary(op, a, b) if op.compares() => {
            int(a)?;
            int(b)?;
            Ok(Ty::Bool)
        }
        Expr::Binary(_, a, b) => {
            int(a)?;
            int(b)?;
            Ok(Ty::Int)
        }
    }
}

/// The type of the field or parameter `name`.
fn name_ty(seen: &Seen<'_>, name: &str, want: Ty) -> Result<Ty, Invalid> {
    let (outer, inner) = split_ref(name);
    if inner.is_some() {
        let reads = if want == Ty::Bool { Reads::Flag } else { Reads::Number };
        resolve(seen, name, reads)?;
        return Ok(want);
    }
    let Some(known) = seen.iter().find(|k| k.name == outer) else {
        return Err(Invalid::UnknownReference(String::from(name)));
    };
    if matches!(known.kind, Kind::Scalar(Scalar::Bool | Scalar::ByteBool)) {
        return Ok(Ty::Bool);
    }
    if known.kind.is_integer() {
        return Ok(Ty::Int);
    }
    Err(match want {
        Ty::Bool => Invalid::NotAFlagReference(String::from(name)),
        Ty::Int => Invalid::NonIntegerReference(String::from(name)),
    })
}

/// Checks a presence condition: a bare name must be a `bool`, since it reads as a flag.
pub(super) fn presence(seen: &Seen<'_>, e: &Expr) -> Result<(), Invalid> {
    let mut bare = e;
    while let Expr::Not(x) = bare {
        bare = x;
    }
    if let Expr::Name(name) = bare {
        resolve(seen, name, Reads::Flag)?;
    }
    if let Some(t) = e.test()
        && let Some(0) = t.mask.and_then(Expr::fold)
    {
        return Err(Invalid::Other(format!(
            "`{} & 0` masks no bits, so the field is never present. \
             Write the bit that marks it present",
            t.field
        )));
    }
    condition(seen, e)
}
