//! Expressions: every name resolves to an earlier field or a parameter, of the right type.

use alloc::format;
use alloc::string::String;

use super::Invalid;
use super::names::{Reads, Seen, resolve};
use crate::model::{Ty, split_ref};
use crate::{Cast, Expr, Kind, Op, Scalar};

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
        Ty::Real if e.as_name().is_some() => Err(Invalid::NonIntegerReference(format!("{e}"))),
        Ty::Real => Err(Invalid::Other(format!(
            "`{e}` is a real, and an integer is needed here. Write `{}`",
            as_int(e)
        ))),
    }
}

/// `e as i64`, as it is spelled.
fn as_int(e: &Expr) -> Expr {
    Expr::cast(e.clone(), Cast::Int)
}

/// Checks that `e` gives a value to pass to a parameter: a number, a real or a condition.
pub(super) fn argument(seen: &Seen<'_>, e: &Expr) -> Result<(), Invalid> {
    check(seen, e, Ty::Int).map(|_| ())
}

/// Checks that `e` is a condition: a `bool`, or a bit test that holds when any bit is set.
pub(super) fn condition(seen: &Seen<'_>, e: &Expr) -> Result<(), Invalid> {
    let ty = check(seen, e, Ty::Bool)?;
    if conditional(e, ty) {
        return Ok(());
    }
    Err(not_a_condition(e, ty))
}

/// Why `e`, of type `ty`, is no condition.
fn not_a_condition(e: &Expr, ty: Ty) -> Invalid {
    match ty {
        Ty::Real => Invalid::Other(format!(
            "`{e}` is a real, and a condition is needed here. Compare it (`{e} > 0.0`)"
        )),
        _ => Invalid::Other(format!(
            "`{e}` is an integer, and a condition is needed here. \
             Compare it (`{e} != 0`), or test its bits (`{e} & 0x01`)"
        )),
    }
}

/// Whether `e` can stand where a condition is needed: a `bool`, or the result of `&`, `|`, `^`.
fn conditional(e: &Expr, ty: Ty) -> bool {
    ty == Ty::Bool || matches!(e, Expr::Binary(op, ..) if op.on_bits())
}

/// The name of a type, as refusals spell it.
fn noun(ty: Ty) -> &'static str {
    match ty {
        Ty::Int => "an integer",
        Ty::Bool => "a `bool`",
        Ty::Real => "a real",
    }
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
            Ty::Real if x.as_name().is_some() => Err(Invalid::NonIntegerReference(format!("{x}"))),
            Ty::Real => Err(Invalid::Other(format!(
                "`{x}` is a real, and `{e}` needs an integer there. Write `{}`",
                as_int(x)
            ))),
        }
    };
    let cond = |x: &Expr| -> Result<(), Invalid> {
        let ty = check(seen, x, Ty::Bool)?;
        if conditional(x, ty) {
            return Ok(());
        }
        Err(match ty {
            Ty::Real => Invalid::Other(format!(
                "`{x}` is a real, and `{e}` needs a condition there. Compare it (`{x} > 0.0`)"
            )),
            _ => Invalid::Other(format!(
                "`{x}` is an integer, and `{e}` needs a condition there. \
                 Compare it (`{x} != 0`), or test its bits (`{x} & 0x01`)"
            )),
        })
    };
    // Two operands of one numeric kind: two integers or two reals, as Rust has no mixed
    // arithmetic.
    let numbers = |a: &Expr, b: &Expr| -> Result<Ty, Invalid> {
        let left = check(seen, a, want_number(want))?;
        let right = check(seen, b, left)?;
        match (left, right) {
            (Ty::Int, Ty::Int) => Ok(Ty::Int),
            (Ty::Real, Ty::Real) => Ok(Ty::Real),
            (Ty::Bool, _) | (_, Ty::Bool) => {
                let x = if left == Ty::Bool { a } else { b };
                if x.as_name().is_some() {
                    Err(Invalid::NonIntegerReference(format!("{x}")))
                } else {
                    Err(Invalid::Other(format!(
                        "`{x}` is a `bool`, and `{e}` needs a number there. Compare it, or name a \
                         number"
                    )))
                }
            }
            _ => Err(Invalid::Other(format!(
                "`{e}` mixes {} and {}. Convert one: `{} as f64`, or `{}`",
                noun(left),
                noun(right),
                if left == Ty::Int { a } else { b },
                as_int(if left == Ty::Real { a } else { b }),
            ))),
        }
    };
    match e {
        Expr::Int(_) | Expr::Const(_) => Ok(Ty::Int),
        Expr::Real(_) => Ok(Ty::Real),
        Expr::Name(name) => name_ty(seen, name, want),
        Expr::Neg(x) => match check(seen, x, want_number(want))? {
            Ty::Bool => {
                int(x)?;
                Ok(Ty::Int)
            }
            ty => Ok(ty),
        },
        Expr::Cast(x, to) => {
            let from = check(seen, x, if *to == Cast::Int { Ty::Real } else { Ty::Int })?;
            if from == Ty::Bool {
                return Err(Invalid::Other(format!(
                    "`{e}` converts a `bool`, which has no number. Write `if {x} {{ 1 }} else \
                     {{ 0 }}`"
                )));
            }
            Ok(match to {
                Cast::Int => Ty::Int,
                Cast::Real => Ty::Real,
            })
        }
        Expr::Not(x) => match check(seen, x, Ty::Bool)? {
            Ty::Bool => Ok(Ty::Bool),
            Ty::Int => Err(Invalid::Other(format!(
                "`!` on an integer flips its bits, so it does not test them. Write `{x} == 0`"
            ))),
            Ty::Real => Err(Invalid::Other(format!(
                "`!` takes a `bool`, and `{x}` is a real. Compare it: `{x} == 0.0`"
            ))),
        },
        Expr::Binary(op, a, b) if op.joins() => {
            cond(a)?;
            cond(b)?;
            Ok(Ty::Bool)
        }
        Expr::Binary(Op::Eq | Op::Ne, a, b) => {
            let left = check(seen, a, want_number(want))?;
            let right = check(seen, b, left)?;
            if left != right {
                return Err(Invalid::Other(format!(
                    "`{e}` compares {} with {}. Compare like with like",
                    noun(left),
                    noun(right)
                )));
            }
            Ok(Ty::Bool)
        }
        Expr::Binary(op, a, b) if op.compares() => {
            numbers(a, b)?;
            Ok(Ty::Bool)
        }
        Expr::Binary(op, a, b) if op.on_bits() || matches!(op, Op::Shl | Op::Shr) => {
            int(a)?;
            int(b)?;
            Ok(Ty::Int)
        }
        Expr::Binary(_, a, b) => numbers(a, b),
        Expr::If(c, a, b) => {
            cond(c)?;
            let then = check(seen, a, want)?;
            let otherwise = check(seen, b, then)?;
            if otherwise != then {
                return Err(Invalid::Other(format!(
                    "`{e}` gives {} one way and {} the other. Give both branches one kind",
                    noun(then),
                    noun(otherwise)
                )));
            }
            Ok(then)
        }
    }
}

/// The type a numeric operand wants: a real where a real is wanted, else an integer.
fn want_number(want: Ty) -> Ty {
    if want == Ty::Real { Ty::Real } else { Ty::Int }
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
    if matches!(known.kind, Kind::Scalar(Scalar::F32 | Scalar::F64)) {
        return Ok(Ty::Real);
    }
    if known.kind.is_integer() {
        return Ok(Ty::Int);
    }
    Err(match want {
        Ty::Bool => Invalid::NotAFlagReference(String::from(name)),
        Ty::Int | Ty::Real => Invalid::NonIntegerReference(String::from(name)),
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
