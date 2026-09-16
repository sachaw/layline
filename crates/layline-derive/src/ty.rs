//! Type shapes recognised by spelling, since a proc macro cannot resolve types.

use syn::Type;

use layline_codegen::{Collection, Scalar};

/// A single-segment type path with no arguments, as in `u8` or `Head`.
pub(crate) fn bare_ident(ty: &Type) -> Option<&syn::Ident> {
    match ty {
        Type::Path(p) if p.qself.is_none() && p.path.segments.len() == 1 => {
            let seg = &p.path.segments[0];
            seg.arguments.is_none().then_some(&seg.ident)
        }
        _ => None,
    }
}

/// A named type with no generic arguments and no `<T as Trait>` qualifier.
pub(crate) fn is_plain_path(ty: &Type) -> bool {
    matches!(ty, Type::Path(p)
        if p.qself.is_none() && p.path.segments.iter().all(|s| s.arguments.is_none()))
}

pub(crate) fn scalar_of(ty: &Type) -> Option<Scalar> {
    Some(match bare_ident(ty)?.to_string().as_str() {
        "u8" => Scalar::U(8),
        "u16" => Scalar::U(16),
        "u32" => Scalar::U(32),
        "u64" => Scalar::U(64),
        "i8" => Scalar::I(8),
        "i16" => Scalar::I(16),
        "i32" => Scalar::I(32),
        "i64" => Scalar::I(64),
        "f32" => Scalar::F32,
        "f64" => Scalar::F64,
        _ => return None,
    })
}

pub(crate) fn is_string(ty: &Type) -> bool {
    let Type::Path(p) = ty else { return false };
    p.qself.is_none()
        && p.path.segments.last().is_some_and(|s| s.ident == "String" && s.arguments.is_none())
}

/// Standard-library types whose size is only known at runtime.
pub(crate) fn variable_size(ty: &Type) -> bool {
    if matches!(ty, Type::Reference(_) | Type::Slice(_) | Type::TraitObject(_)) {
        return true;
    }
    let Type::Path(p) = ty else { return false };
    let Some(seg) = p.path.segments.last() else { return false };
    matches!(
        seg.ident.to_string().as_str(),
        "String"
            | "str"
            | "OsString"
            | "OsStr"
            | "CString"
            | "CStr"
            | "PathBuf"
            | "Path"
            | "Cow"
            | "Vec"
            | "VecDeque"
    )
}

/// `[T; N]` with a literal length.
pub(crate) fn array_of(ty: &Type) -> Option<(&Type, usize)> {
    let Type::Array(arr) = ty else { return None };
    let syn::Expr::Lit(lit) = &arr.len else { return None };
    let syn::Lit::Int(n) = &lit.lit else { return None };
    Some((&arr.elem, n.base10_parse().ok()?))
}

/// The innermost element type, and every dimension outermost first.
pub(crate) fn array_dims(ty: &Type) -> Option<(&Type, Vec<usize>)> {
    let mut dims = Vec::new();
    let mut elem = ty;
    while let Some((inner, len)) = array_of(elem) {
        dims.push(len);
        elem = inner;
    }
    (!dims.is_empty() && !matches!(elem, Type::Array(_))).then_some((elem, dims))
}

/// The `T` of `wrapper<T>`, by the last path segment.
fn wrapped<'a>(ty: &'a Type, wrapper: &str) -> Option<&'a Type> {
    let Type::Path(p) = ty else { return None };
    if p.qself.is_some() {
        return None;
    }
    let seg = p.path.segments.last()?;
    if seg.ident != wrapper {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    match args.args.first() {
        Some(syn::GenericArgument::Type(t)) if args.args.len() == 1 => Some(t),
        _ => None,
    }
}

pub(crate) fn option_element(ty: &Type) -> Option<&Type> {
    wrapped(ty, "Option")
}

pub(crate) fn vec_element(ty: &Type) -> Option<&Type> {
    wrapped(ty, "Vec")
}

pub(crate) fn box_element(ty: &Type) -> Option<&Type> {
    wrapped(ty, "Box")
}

/// The element of a `Vec<T>` or a `Box<[T]>`, and which of the two it is.
pub(crate) fn run_element(ty: &Type) -> Option<(&Type, Collection)> {
    if let Some(t) = vec_element(ty) {
        return Some((t, Collection::Vec));
    }
    let Type::Slice(slice) = box_element(ty)? else { return None };
    Some((&slice.elem, Collection::Boxed))
}

pub(crate) fn is_byte_vec(ty: &Type) -> bool {
    vec_element(ty).and_then(scalar_of) == Some(Scalar::U(8))
}
