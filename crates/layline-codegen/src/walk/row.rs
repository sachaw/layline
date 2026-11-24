//! Owned versions of `layline_core::table` rows, before they become literals.

use layline_core::Extent;

use crate::model::positioned;
use crate::{Container, CoverFrom, CoverTo, Coverage, Discriminant, Expr, Field, Kind, Op, Scalar};

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
    Pad { align: u64 },
    Until { terminator: u8 },
    Terminated { mask: u8 },
    Fill { cap: Option<u64> },
    Chosen { on: On },
    ChosenIn { on: On, bits: u64 },
    ChosenWithin { on: On, by: By },
}

/// What selects a switch's arm, as `layline_core::table::Discriminant`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum On {
    /// A field's name, or an expression's compact spelling.
    Field(String),
    BodyLength,
}

impl On {
    pub(crate) fn of(on: &Discriminant) -> Self {
        match on {
            Discriminant::Field(e) => On::Field(spelled(e)),
            Discriminant::BodyLength => On::BodyLength,
        }
    }
}

/// What makes an optional row present.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Presence {
    Flag { field: String, mask: u64 },
    Clear { field: String, mask: u64 },
    Offset,
    Bit,
    Remaining,
    Length { by: String },
    When { expr: String },
}

impl Presence {
    /// The row of a presence condition: one field's bits where it tests them, else the condition.
    pub(crate) fn of(when: &Expr) -> Self {
        let test = when.test().and_then(|t| {
            let mask = match t.mask {
                None => 1,
                Some(m) => m.fold()? as u64,
            };
            Some((String::from(t.field), mask, t.set))
        });
        match test {
            Some((field, mask, true)) => Presence::Flag { field, mask },
            Some((field, mask, false)) => Presence::Clear { field, mask },
            None => Presence::When { expr: format!("{when:#}") },
        }
    }
}

/// A number a row's size is read from: `field * scale + offset`, as `layline_core::table::By`.
///
/// Any other expression is held in `field` in its compact spelling, unscaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct By {
    pub field: String,
    pub scale: u32,
    pub offset: i32,
}

impl By {
    pub(crate) fn of(e: &Expr) -> Self {
        affine(e).unwrap_or_else(|| By { field: spelled(e), scale: 1, offset: 0 })
    }

    /// The number in the row `name`, unscaled.
    pub(crate) fn named(name: &str) -> Self {
        By { field: String::from(name), scale: 1, offset: 0 }
    }
}

/// `e` as `field * scale + offset`, with a positive scale, if it is one.
fn affine(e: &Expr) -> Option<By> {
    let lit = |e: &Expr| match e {
        Expr::Int(n) => Some(*n),
        _ => None,
    };
    let scaled = |e: &Expr| -> Option<(String, u32)> {
        match e {
            Expr::Name(n) => Some((n.clone(), 1)),
            Expr::Binary(Op::Mul, a, b) => {
                let (n, k) = match (&**a, &**b) {
                    (Expr::Name(n), k) | (k, Expr::Name(n)) => (n, lit(k)?),
                    _ => return None,
                };
                Some((n.clone(), u32::try_from(k).ok().filter(|k| *k > 0)?))
            }
            _ => None,
        }
    };
    let (base, offset) = match e {
        Expr::Binary(Op::Add, a, b) => match (lit(a), lit(b)) {
            (_, Some(o)) => (&**a, o),
            (Some(o), _) => (&**b, o),
            _ => return None,
        },
        Expr::Binary(Op::Sub, a, b) => (&**a, lit(b)?.checked_neg()?),
        other => (other, 0),
    };
    let (field, scale) = scaled(base)?;
    Some(By { field, scale, offset: i32::try_from(offset).ok()? })
}

/// A name as itself, and any other expression in its compact spelling.
pub(crate) fn spelled(e: &Expr) -> String {
    match e {
        Expr::Name(n) => n.clone(),
        other => format!("{other:#}"),
    }
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
