//! Rules for fixed layouts.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::coverage::{Places, check_value, coverage};
use super::{Invalid, field_names, ident};
use crate::model::positioned;
use crate::{Container, Coverage, Field, Kind, LayoutDef, Scalar};

/// Checks a fixed layout: size, field coverage, `#[at]` positions and word boundaries.
pub(crate) fn validate_layout(layout: &LayoutDef) -> Result<(), Invalid> {
    ident("layout", &layout.name)?;
    for f in &layout.fields {
        field_names(f)?;
    }
    validate_container(&layout.container)?;
    let declared = layout.container.field_bits();
    let covered: u64 = layout.fields.iter().map(|f| f.kind.width()).sum();
    if covered != declared {
        return Err(Invalid::Tiling { covered_bits: covered, declared_bits: declared });
    }

    let words_mode = matches!(layout.container, Container::Words { .. });
    for p in positioned(&layout.fields, &layout.container) {
        let f = p.field;
        let bits = f.kind.width();
        if words_mode {
            let in_word = matches!(f.kind, Kind::Scalar(_) | Kind::Codec { .. });
            if in_word && (bits > 16 || (p.declared % 16) + bits > 16) {
                return Err(Invalid::CrossesWord(f.name.clone()));
            }
        }
        if let Some(stated) = f.stated {
            layout
                .container
                .check_stated(&f.name, &f.kind, stated, p.declared)
                .map_err(Invalid::Stated)?;
        }
    }
    for f in &layout.fields {
        layout.container.admits(&f.name, &f.kind)?;
    }
    assertions(layout)
}

/// Checks a word container's width and prefix.
pub fn validate_container(container: &Container) -> Result<(), Invalid> {
    if let Container::Word { bits, .. } = container
        && (*bits == 0 || !bits.is_multiple_of(8))
    {
        return Err(Invalid::Other(format!(
            "a word container of {bits} bits. Use a positive multiple of 8, \
             and declare spare bits as fields"
        )));
    }
    if let Container::Word { bits, prefix, .. } = container
        && *prefix >= *bits
    {
        return Err(Invalid::Other(format!(
            "`prefix = {prefix}` leaves no bits for fields in a {bits}-bit container. \
             Widen the container, or narrow the prefix"
        )));
    }
    if let Container::Word { prefix, prefix_value: Some(v), .. } = container {
        if *prefix == 0 {
            return Err(Invalid::Other("`prefix_value` needs a prefix. Add `prefix = K`".into()));
        }
        if *prefix < 128 && *v >= (1u128 << *prefix) {
            return Err(Invalid::Other(format!(
                "`prefix_value = {v:#b}` does not fit the {prefix}-bit prefix"
            )));
        }
    }
    Ok(())
}

/// Checks `#[range]`, `#[magic]` and `#[checksum]` fields.
fn assertions(layout: &LayoutDef) -> Result<(), Invalid> {
    let asserts = |f: &Field| f.magic.is_some() || matches!(f.kind, Kind::Checksum { .. });

    for f in &layout.fields {
        if f.range.is_some() && !matches!(f.kind, Kind::Scalar(Scalar::U(_) | Scalar::I(_))) {
            return Err(Invalid::field(
                &f.name,
                format!(
                    "`#[range]` needs an integer field, and this field is {}",
                    f.kind.describe(),
                ),
            ));
        }
    }
    if layout.container.is_bit_addressed()
        && let Some(f) = layout.fields.iter().find(|f| asserts(f))
    {
        return Err(Invalid::field(
            &f.name,
            String::from(
                "a magic or checksum field needs whole bytes, and this container is \
                 bit-addressed. Declare the record in byte mode",
            ),
        ));
    }

    let mut extents: Vec<(&str, u64, u64)> = Vec::new();
    for p in positioned(&layout.fields, &layout.container) {
        let bits = p.field.kind.width();
        if asserts(p.field) && (p.physical % 8 != 0 || bits % 8 != 0) {
            return Err(Invalid::Other(format!(
                "field `{}`: a magic or checksum field needs whole bytes, and this one occupies \
                 bits {}..{}. Align it to byte boundaries",
                p.field.name,
                p.physical,
                p.physical + bits,
            )));
        }
        extents.push((&p.field.name, p.physical / 8, (p.physical + bits) / 8));
    }
    let places: Places<'_> = layout.fields.iter().map(|f| f.name.as_str()).collect();

    for f in &layout.fields {
        if let Some(bytes) = &f.magic {
            let ok = matches!(
                f.kind,
                Kind::Scalar(Scalar::U(_) | Scalar::I(_)) | Kind::Array(Scalar::U(8), _)
            );
            if !ok {
                return Err(Invalid::field(
                    &f.name,
                    format!(
                        "a magic constant needs an integer or `[u8; N]` field, not {}",
                        f.kind.describe(),
                    ),
                ));
            }
            if bytes.is_empty() || (bytes.len() as u64) * 8 != f.kind.width() {
                return Err(Invalid::field(
                    &f.name,
                    format!(
                        "the constant is {} byte(s) and the field is {}. Make them the same size",
                        bytes.len(),
                        f.kind.width() / 8,
                    ),
                ));
            }
        }
        if let Kind::Checksum { repr, algorithm, over } = &f.kind {
            check_value(&f.name, repr, algorithm)?;
            coverage(&places, &Vec::new(), &f.name, over)?;
            later_checksum(layout, &extents, &f.name, over)?;
        }
    }
    Ok(())
}

/// Refuses a range that covers a checksum declared after this one.
fn later_checksum(
    layout: &LayoutDef,
    extents: &[(&str, u64, u64)],
    own: &str,
    over: &Coverage,
) -> Result<(), Invalid> {
    let at = |name: &str| extents.iter().find(|(n, ..)| *n == name).map(|&(_, a, b)| (a, b));
    let Some(own_at) = at(own) else { return Ok(()) };
    let Some(covered) = over.resolve(own_at, at) else { return Ok(()) };
    let runs = covered.runs();
    let order = |name: &str| extents.iter().position(|(n, ..)| *n == name);
    for f in &layout.fields {
        if f.name == own || !matches!(f.kind, Kind::Checksum { .. }) {
            continue;
        }
        let Some((other_at, other_end)) = at(&f.name) else { continue };
        let covered = runs.iter().any(|&(a, b)| other_at >= a && other_end <= b);
        if covered && order(&f.name) > order(own) {
            return Err(Invalid::field(
                own,
                format!(
                    "the covered range includes `{}`, a checksum declared after it. \
                     Narrow the range",
                    f.name,
                ),
            ));
        }
    }
    Ok(())
}
