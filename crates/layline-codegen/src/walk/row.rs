//! Owned versions of `layline_core::table` rows, before they become literals.

use layline_core::Extent;

use crate::model::positioned;
use crate::{By, Container, CoverFrom, CoverTo, Coverage, Discriminant, Field, Kind, Scalar};

/// One row of a layout's `FIELDS` table.
///
/// An array has one row per element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDef {
    /// The field name, or `field[i]` for an element, or `field[wN]` for one word.
    pub name: String,
    /// Bit position and width, counted from the start of the container.
    pub extent: Extent,
}

/// One row of a message's `SEGMENTS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SegmentDef {
    /// The field, collection or hidden block this row describes.
    pub name: String,
    pub start: Start,
    pub span: Span,
    pub when: Option<Presence>,
    /// The type this row refers to: a collection element, message, choice or checksum algorithm.
    pub decoded_by: Option<String>,
    /// The hidden layout whose `FIELDS` this row exposes, if any.
    pub table: Option<Table>,
    /// The bytes a checksum row covers.
    pub covers: Option<CoverDef>,
}

/// A hidden `#[derive(Layout)]` type and its `FIELDS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Table {
    pub ty: String,
    pub fields: Vec<FieldDef>,
}

/// Where a row begins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Start {
    At(u64),
    After { segment: String, bits: u64 },
    Seek { by: String },
}

/// A row's size, and what determines it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Span {
    Fixed(u64),
    Counted { by: By, each: Option<u64> },
    Squared { by: By, each: Option<u64> },
    Strided { by: By, stride: By },
    Window { by: By },
    SelfDelimiting,
    Until { terminator: u8 },
    Terminated { mask: u8 },
    Fill { cap: Option<u64> },
    Chosen { on: Discriminant },
    ChosenIn { on: Discriminant, bits: u64 },
    ChosenWithin { on: Discriminant, by: By },
}

/// What makes an optional row present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Presence {
    Flag { field: String, mask: u64 },
    Offset,
    Bit,
    Remaining,
    Length { by: String },
}

/// The bytes a checksum row covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CoverDef {
    pub from: CoverEdge,
    pub to: CoverEdge,
    /// Whether the checksum's own bytes are excluded from the range.
    pub excludes_self: bool,
}

impl CoverDef {
    /// The cover for checksum field `own` over `over`.
    pub(crate) fn new(own: &str, over: &Coverage, excludes_self: bool) -> Self {
        let from = match &over.from {
            CoverFrom::Start => CoverEdge::Start,
            CoverFrom::Field(row) => CoverEdge::Before(row.clone()),
        };
        let to = match &over.to {
            CoverTo::Here => CoverEdge::Before(String::from(own)),
            CoverTo::Before(row) => CoverEdge::Before(row.clone()),
            CoverTo::After(row) => CoverEdge::After(row.clone()),
        };
        Self { from, to, excludes_self }
    }
}

/// One end of a covered range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CoverEdge {
    Start,
    Before(String),
    After(String),
}

/// One arm of a choice, with its body's rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArmDef {
    pub name: String,
    /// `None` for the open arm.
    pub value: Option<i64>,
    pub segments: Vec<SegmentDef>,
}

/// The `FIELDS` table the `Layout` derive writes, sorted by position.
///
/// An array gets one row per element, `field[i]`. A nested layout gets one row per word,
/// `field[wN]`, when the container has an [interior boundary](Container::interior_boundary).
/// Under `order = msb`, position order differs from declaration order.
pub fn field_rows(fields: &[Field], container: &Container) -> Vec<FieldDef> {
    let region = container.interior_boundary();
    let mut rows = Vec::new();
    let prefix = container.prefix();
    if prefix > 0 {
        let start = container.physical(&Kind::Scalar(Scalar::U(prefix)), 0);
        push_row(String::from("<prefix>"), start, prefix, &mut rows);
    }
    for p in positioned(fields, container) {
        rows_of(&p.field.kind, &p.field.name, p.physical, region, &mut rows);
    }
    rows.sort_by_key(|row| row.extent.start());
    rows
}

fn push_row(name: String, start: u64, width: u64, out: &mut Vec<FieldDef>) {
    out.push(FieldDef { name, extent: Extent::new(start, width as u32) });
}

fn rows_of(kind: &Kind, name: &str, at: u64, region: Option<u64>, out: &mut Vec<FieldDef>) {
    match kind {
        Kind::Array(s, dims) => {
            let each = s.bits();
            let mut strides: Vec<usize> = Vec::new();
            let mut acc = 1;
            for d in dims.iter().rev() {
                strides.push(acc);
                acc *= d;
            }
            strides.reverse();
            for i in 0..dims.iter().product::<usize>() {
                let mut path = String::from(name);
                for (d, stride) in dims.iter().zip(&strides) {
                    path.push_str(&format!("[{}]", (i / stride) % d));
                }
                push_row(path, at + i as u64 * each, each, out);
            }
        }
        Kind::NestedArray { ty, bytes, len } => {
            let each = *bytes as u64 * 8;
            for i in 0..*len {
                let elem = Kind::Nested { ty: ty.clone(), bytes: *bytes };
                rows_of(&elem, &format!("{name}[{i}]"), at + i as u64 * each, region, out);
            }
        }
        Kind::Nested { bytes, .. } => match region {
            Some(region) => {
                for k in 0..(*bytes as u64 * 8 / region) {
                    push_row(format!("{name}[w{k}]"), at + k * region, region, out);
                }
            }
            None => push_row(String::from(name), at, *bytes as u64 * 8, out),
        },
        other => push_row(String::from(name), at, other.width(), out),
    }
}

#[cfg(test)]
mod tests {
    use super::field_rows;
    use crate::{Container, Endian, Field, Kind, Scalar};

    #[test]
    fn an_array_has_a_row_per_element() {
        let container = Container::Bytes { bytes: 72, endian: Endian::Be };
        let rows = field_rows(&[Field::new("m", Kind::Array(Scalar::F64, vec![3, 3]))], &container);
        assert_eq!(rows.len(), 9);
        assert_eq!(rows[0].name, "m[0][0]");
        assert_eq!(rows[5].name, "m[1][2]");
        assert_eq!(rows[8].name, "m[2][2]");
        assert_eq!(rows[5].extent.start(), 5 * 64, "the flat run's offsets, unchanged");
    }
}
