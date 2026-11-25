//! Resolving a field reference against the names declared before it.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{Invalid, ident};
use crate::model::split_ref;
use crate::{Kind, Scalar};

/// A name a reference can resolve to: an earlier field or a parameter.
pub(super) struct Known<'a> {
    pub(super) name: &'a str,
    pub(super) kind: Kind,
    /// Whether the name is a parameter, which encode never writes.
    pub(super) param: bool,
}

/// The names declared so far, in wire order.
pub(super) type Seen<'a> = Vec<Known<'a>>;

impl<'a> Known<'a> {
    pub(super) fn field(name: &'a str, kind: &Kind) -> Self {
        Known { name, kind: kind.clone(), param: false }
    }
}

/// What a reference needs from the field it names.
#[derive(Clone, Copy)]
pub(super) enum Reads {
    /// A count, length, offset or discriminant.
    Number,
    /// The `bool` of a bare `#[when]`.
    Flag,
}

/// Resolves `field` to an earlier name of the right type.
pub(super) fn resolve(seen: &Seen<'_>, field: &str, reads: Reads) -> Result<(), Invalid> {
    let (outer, inner) = split_ref(field);
    let Some(Known { kind, .. }) = seen.iter().find(|k| k.name == outer) else {
        return Err(Invalid::UnknownReference(String::from(field)));
    };
    let (holds, name) = match reads {
        Reads::Number => (kind.is_integer(), "a field of this message"),
        Reads::Flag => (
            matches!(kind, Kind::Scalar(Scalar::Bool | Scalar::ByteBool)),
            "a `bool` in this message",
        ),
    };
    match inner {
        None if holds => Ok(()),
        None => Err(match reads {
            Reads::Number => Invalid::NonIntegerReference(String::from(field)),
            Reads::Flag => Invalid::NotAFlagReference(String::from(field)),
        }),
        Some(inner) if inner.contains('.') => Err(Invalid::Other(format!(
            "`{field}` reaches through more than one nested layout. \
             Name {name}, or a field of one nested layout"
        ))),
        Some(inner) if matches!(kind, Kind::Nested { .. }) => ident("field", inner),
        Some(_) => Err(Invalid::NotANestedLayout(String::from(field))),
    }
}

/// Refuses an offset that is a bare parameter, where encode would write the value back.
pub(super) fn read_only(seen: &Seen<'_>, at: &crate::Expr, what: &str) -> Result<(), Invalid> {
    let Some(field) = at.as_name() else { return Ok(()) };
    let (outer, _) = split_ref(field);
    if !seen.iter().any(|k| k.name == outer && k.param) {
        return Ok(());
    }
    Err(Invalid::Other(format!(
        "`{outer}` is a parameter, and encode cannot write {what} into it. \
         Name a field of this message"
    )))
}

/// Checks the `#[with(..)]` list of a nested message: one expression per parameter, over earlier
/// fields of this message, fields of a nested layout in it, and parameters.
pub(super) fn stated_with(seen: &Seen<'_>, kind: &Kind) -> Result<(), Invalid> {
    let Kind::Msg { with, .. } = kind else { return Ok(()) };
    stated_args(seen, with)
}

/// Whether `name` is an earlier real field or a real parameter.
fn is_real(seen: &Seen<'_>, name: &str) -> bool {
    seen.iter().any(|k| k.name == name && matches!(k.kind, Kind::Scalar(Scalar::F32 | Scalar::F64)))
}

/// Checks a `#[with(..)]` list, of a nested message or a switch's catalogue. A bare name is an
/// integer, a real or a `bool`; any other expression is typed by its operators.
pub(super) fn stated_args(seen: &Seen<'_>, with: &[crate::Expr]) -> Result<(), Invalid> {
    for arg in with {
        match arg.as_name() {
            Some(name) if is_real(seen, name) => {}
            Some(name) => {
                if resolve(seen, name, Reads::Flag).is_err() {
                    resolve(seen, name, Reads::Number)?;
                }
            }
            None => super::expr::argument(seen, arg)?,
        }
    }
    Ok(())
}
