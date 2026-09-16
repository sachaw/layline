//! Rules for enumerations and dispatches.

use alloc::format;
use alloc::vec::Vec;

use super::{Invalid, ident, type_path};
use crate::{DispatchDef, EnumDef, Scalar};

/// Checks a dispatch: names, an 8, 16, 32 or 64-bit integer id, distinct ids that fit it, and a
/// fallback name no arm uses.
///
/// # Errors
///
/// An [`Invalid`] naming the dispatch.
pub(crate) fn validate_dispatch(d: &DispatchDef) -> Result<(), Invalid> {
    let name = &d.name;
    ident("dispatch", name)?;
    ident("fallback arm", &d.other)?;
    for a in &d.arms {
        ident("arm", &a.name)?;
        type_path(&a.name, "the arm's body", &a.ty)?;
    }
    let bits = match d.id {
        Scalar::U(b) | Scalar::I(b) => b,
        Scalar::Bool | Scalar::F32 | Scalar::F64 => {
            return Err(Invalid::Other(format!(
                "dispatch `{name}`: the id must be an integer, not a bool or a float"
            )));
        }
    };
    if !matches!(bits, 8 | 16 | 32 | 64) {
        return Err(Invalid::Other(format!(
            "dispatch `{name}`: the id must be 8, 16, 32 or 64 bits, not {bits}. \
             Declare a smaller id as a field of the enclosing layout"
        )));
    }

    distinct(d.arms.iter().map(|a| a.id))?;

    let (lo, hi) = bounds(matches!(d.id, Scalar::I(_)), bits);
    for a in &d.arms {
        if a.id < lo || a.id > hi {
            return Err(Invalid::Other(format!(
                "dispatch `{name}`: `{}` has the id {}, which does not fit in {bits} bits. \
                 Use an id in {lo}..={hi}",
                a.name, a.id,
            )));
        }
        if a.name == d.other {
            return Err(Invalid::Other(format!(
                "dispatch `{name}`: `{}` is both an arm and the fallback. Rename the fallback",
                a.name,
            )));
        }
    }
    Ok(())
}

/// Checks an enumeration: names, a 1 to 64-bit integer representation, distinct values that fit
/// it, and every value listed when there is no fallback.
///
/// # Errors
///
/// [`Invalid::Other`] or [`Invalid::DuplicateArm`], naming the enumeration.
pub(crate) fn validate_enum(e: &EnumDef) -> Result<(), Invalid> {
    let name = &e.name;
    ident("catalogue", name)?;
    for v in &e.variants {
        ident("variant", &v.name)?;
    }
    if let Some(other) = &e.other {
        ident("fallback variant", other)?;
    }
    if let Some(default) = &e.default {
        if Some(default) == e.other.as_ref() {
            return Err(Invalid::Other(format!(
                "catalogue `{name}`: the default variant `{default}` is the fallback, which \
                 carries a value. Name a listed variant"
            )));
        }
        if !e.variants.iter().any(|v| &v.name == default) {
            return Err(Invalid::Other(format!(
                "catalogue `{name}`: the default variant `{default}` is not one of its variants"
            )));
        }
    }
    let (bits, signed) = match e.repr {
        Scalar::U(b) => (b, false),
        Scalar::I(b) => (b, true),
        Scalar::Bool => (1, false),
        Scalar::F32 | Scalar::F64 => {
            return Err(Invalid::Other(format!(
                "catalogue `{name}`: the representation must be an integer, not a float"
            )));
        }
    };

    if bits == 0 || bits > 64 {
        return Err(Invalid::Other(format!(
            "catalogue `{name}`: the representation is {bits} bits. Use 1 to 64 bits"
        )));
    }
    let (lo, hi) = bounds(signed, bits);
    for v in &e.variants {
        if v.value < lo || v.value > hi {
            return Err(Invalid::Other(format!(
                "catalogue `{name}`: `{}` has the value {}, which does not fit in {bits} bits. \
                 Use a value in {lo}..={hi}",
                v.name, v.value,
            )));
        }
    }

    distinct(e.variants.iter().map(|v| v.value))?;

    let total = 1u128 << bits;
    if e.other.is_none() && (e.variants.len() as u128) != total {
        return Err(Invalid::Other(format!(
            "catalogue `{name}` is not total: {} of its {total} values have no variant. \
             List them all, or add a fallback variant",
            total - e.variants.len() as u128,
        )));
    }
    Ok(())
}

/// The range of an integer of `bits`, as `(lo, hi)`.
fn bounds(signed: bool, bits: u64) -> (i64, i64) {
    if signed {
        (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1)
    } else {
        (0, i64::try_from((1u128 << bits) - 1).unwrap_or(i64::MAX))
    }
}

/// Refuses a value listed twice.
fn distinct(values: impl Iterator<Item = i64>) -> Result<(), Invalid> {
    let mut seen: Vec<i64> = values.collect();
    seen.sort_unstable();
    match seen.windows(2).find(|w| w[0] == w[1]) {
        Some(w) => Err(Invalid::DuplicateArm(u64::try_from(w[0]).unwrap_or_default())),
        None => Ok(()),
    }
}
