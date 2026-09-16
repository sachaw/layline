//! Rendering the audit artifact from table rows.

use layline_core::{audit, table};

use crate::model::positioned;
use crate::walk::{collection_ty, field_rows, row};
use crate::{By, Kind, LayoutDef, Param, Scalar};

/// The records for a fixed layout: `L`, its `F` rows, the `V` verdict, and its `T` rows.
pub(crate) fn layout(l: &LayoutDef) -> String {
    let rows = field_rows(&l.fields, &l.container);
    let fields = field_views(&rows);
    let spellings: Vec<(&str, String)> =
        l.fields.iter().map(|f| (f.name.as_str(), collection_ty(&f.kind))).collect();
    let types: Vec<table::TypeDef<'_>> =
        spellings.iter().map(|(field, ty)| table::TypeDef::new(field, ty)).collect();
    let stated: Vec<table::StatedDef<'_>> = positioned(&l.fields, &l.container)
        .filter_map(|p| {
            let stated = p.field.stated?.published(p.physical);
            Some(table::StatedDef::new(&p.field.name, stated.unit, stated.pos))
        })
        .collect();
    let constants: Vec<table::ConstDef<'_>> = l
        .fields
        .iter()
        .filter_map(|f| f.magic.as_deref().map(|bytes| table::ConstDef::new(&f.name, bytes)))
        .collect();
    let ranges: Vec<table::RangeDef<'_>> = l
        .fields
        .iter()
        .filter_map(|f| {
            let r = f.range?;
            let (min, max) = primitive_bounds(&f.kind)?;
            Some(table::RangeDef::new(&f.name, r.lo.unwrap_or(min), r.hi.unwrap_or(max)))
        })
        .collect();
    let covers: Vec<(&str, row::CoverDef)> = l
        .fields
        .iter()
        .filter_map(|f| {
            let Kind::Checksum { over, .. } = &f.kind else { return None };
            let excludes_self =
                over.excises(&f.name, |row| l.fields.iter().position(|f| f.name == row));
            Some((f.name.as_str(), row::CoverDef::new(&f.name, over, excludes_self)))
        })
        .collect();
    let covered: Vec<table::CoverDef<'_>> =
        covers.iter().map(|(name, cover)| cover_view(name, cover)).collect();
    audit::layout(&l.name, l.container.bits(), &fields)
        .types(&types)
        .constants(&constants)
        .ranges(&ranges)
        .covering(&covered)
        .stated(&stated)
        .to_string()
}

fn primitive_bounds(kind: &Kind) -> Option<(i64, i64)> {
    let (signed, bits) = match kind {
        Kind::Scalar(Scalar::U(n)) => (false, *n),
        Kind::Scalar(Scalar::I(n)) => (true, *n),
        _ => return None,
    };
    if bits == 0 || bits > 64 {
        return None;
    }
    Some(if signed {
        let half = 1i128 << (bits - 1);
        (i64::try_from(-half).ok()?, i64::try_from(half - 1).ok()?)
    } else {
        (0, i64::try_from((1i128 << bits) - 1).ok()?)
    })
}

/// The records for a message: `M`, a `P` row per parameter, a `G` row per segment, and `E`.
///
/// A fixed-size run's field table follows its `G` row.
pub(crate) fn message(name: &str, needs: &[Param], rows: &[row::SegmentDef]) -> String {
    let tables = table_views(rows);
    let spans = span_views(rows);
    let segments = segment_views(rows, &spans, &tables);
    let covered = cover_views(rows);
    let params: Vec<table::ParamDef<'_>> =
        needs.iter().map(|p| table::ParamDef::new(&p.name, p.repr.primitive())).collect();
    audit::message(name, &segments).covering(&covered).params(&params).to_string()
}

/// The records for a choice: `C`, then an `A` row and segment table per arm.
pub(crate) fn choice(name: &str, arms: &[row::ArmDef]) -> String {
    let tables: Vec<Vec<Vec<table::FieldDef<'_>>>> =
        arms.iter().map(|arm| table_views(&arm.segments)).collect();
    let spans: Vec<Vec<table::Span<'_>>> =
        arms.iter().map(|arm| span_views(&arm.segments)).collect();
    let segments: Vec<Vec<table::SegmentDef<'_>>> = arms
        .iter()
        .enumerate()
        .map(|(i, arm)| segment_views(&arm.segments, &spans[i], &tables[i]))
        .collect();
    let arm_defs: Vec<table::ArmDef<'_>> = arms
        .iter()
        .enumerate()
        .map(|(i, arm)| table::ArmDef::new(&arm.name, arm.value, &segments[i]))
        .collect();
    let covers: Vec<Vec<table::CoverDef<'_>>> =
        arms.iter().map(|arm| cover_views(&arm.segments)).collect();
    let covered: Vec<table::ArmCover<'_>> = arms
        .iter()
        .enumerate()
        .filter(|(i, _)| !covers[*i].is_empty())
        .map(|(i, arm)| table::ArmCover::new(&arm.name, &covers[i]))
        .collect();
    audit::choice(name, &arm_defs).covering(&covered).to_string()
}

fn field_views(rows: &[row::FieldDef]) -> Vec<table::FieldDef<'_>> {
    rows.iter().map(|row| table::FieldDef::new(&row.name, row.extent)).collect()
}

fn table_views(rows: &[row::SegmentDef]) -> Vec<Vec<table::FieldDef<'_>>> {
    rows.iter()
        .map(|row| row.table.as_ref().map(|t| field_views(&t.fields)).unwrap_or_default())
        .collect()
}

fn span_views(rows: &[row::SegmentDef]) -> Vec<table::Span<'_>> {
    rows.iter().map(|row| span_view(&row.span)).collect()
}

fn segment_views<'a>(
    rows: &'a [row::SegmentDef],
    spans: &'a [table::Span<'a>],
    tables: &'a [Vec<table::FieldDef<'a>>],
) -> Vec<table::SegmentDef<'a>> {
    rows.iter()
        .enumerate()
        .map(|(i, row)| {
            table::SegmentDef::new(
                &row.name,
                match &row.start {
                    row::Start::At(bit) => table::Start::At(*bit),
                    row::Start::After { segment, bits } => {
                        table::Start::After { segment, bits: *bits }
                    }
                    row::Start::Seek { by } => table::Start::Seek { by },
                },
                spans[i],
                row.when.as_ref().map(presence_view),
                row.decoded_by.as_deref(),
                &tables[i],
            )
        })
        .collect()
}

fn presence_view(when: &row::Presence) -> table::Presence<'_> {
    match when {
        row::Presence::Flag { field, mask } => table::Presence::Flag { field, mask: *mask },
        row::Presence::Offset => table::Presence::Offset,
        row::Presence::Bit => table::Presence::Bit,
        row::Presence::Remaining => table::Presence::Remaining,
        row::Presence::Length { by } => table::Presence::Length { by },
    }
}

fn cover_views(rows: &[row::SegmentDef]) -> Vec<table::CoverDef<'_>> {
    rows.iter().filter_map(|row| Some(cover_view(&row.name, row.covers.as_ref()?))).collect()
}

fn cover_view<'a>(field: &'a str, cover: &'a row::CoverDef) -> table::CoverDef<'a> {
    let row::CoverDef { from, to, excludes_self } = cover;
    table::CoverDef::new(field, edge_view(from), edge_view(to), *excludes_self)
}

fn edge_view(edge: &row::CoverEdge) -> table::CoverEdge<'_> {
    match edge {
        row::CoverEdge::Start => table::CoverEdge::Start,
        row::CoverEdge::Before(row) => table::CoverEdge::Before(row),
        row::CoverEdge::After(row) => table::CoverEdge::After(row),
    }
}

fn by_view(by: &By) -> table::By<'_> {
    table::By { field: &by.field, scale: by.scale, offset: by.offset }
}

fn span_view(span: &row::Span) -> table::Span<'_> {
    match span {
        row::Span::Fixed(bits) => table::Span::Fixed(*bits),
        row::Span::Terminated { mask } => table::Span::Terminated { mask: *mask },
        row::Span::Counted { by, each } => table::Span::Counted { by: by_view(by), each: *each },
        row::Span::Squared { by, each } => table::Span::Squared { by: by_view(by), each: *each },
        row::Span::Strided { by, stride } => {
            table::Span::Strided { by: by_view(by), stride: by_view(stride) }
        }
        row::Span::Window { by } => table::Span::Window { by: by_view(by) },
        row::Span::SelfDelimiting => table::Span::SelfDelimiting,
        row::Span::Until { terminator } => table::Span::Until { terminator: *terminator },
        row::Span::Fill { cap } => table::Span::Fill { cap: *cap },
        row::Span::Chosen { on } => table::Span::Chosen { on: discriminant_view(on) },
        row::Span::ChosenIn { on, bits } => {
            table::Span::ChosenIn { on: discriminant_view(on), bits: *bits }
        }
        row::Span::ChosenWithin { on, by } => {
            table::Span::ChosenWithin { on: discriminant_view(on), by: by_view(by) }
        }
    }
}

fn discriminant_view(on: &crate::Discriminant) -> table::Discriminant<'_> {
    match on {
        crate::Discriminant::Field(field) => table::Discriminant::Field(field),
        crate::Discriminant::BodyLength => table::Discriminant::BodyLength,
    }
}
