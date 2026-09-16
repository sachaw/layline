//! The segment table: rows collected during generation, and their literals.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;

use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{field_rows, row};
use crate::{By, Container, Count, Discriminant, Endian, Field, Kind, Len, Root};

/// The rows so far, and where the next one starts.
pub(super) struct Rows {
    pub(super) rows: Vec<row::SegmentDef>,
    /// Fixed bits since [`from`](Self::from), or since the body's start.
    at: u64,
    /// The nearest earlier segment whose size is known only at run time.
    from: Option<String>,
    /// The row just pushed. A later `Start::After` may name it.
    last: String,
}

impl Rows {
    pub(super) fn new() -> Self {
        Self { rows: Vec::new(), at: 0, from: None, last: String::new() }
    }

    fn start(&self) -> row::Start {
        match &self.from {
            None => row::Start::At(self.at),
            Some(segment) => row::Start::After { segment: segment.clone(), bits: self.at },
        }
    }

    pub(super) fn push(&mut self, row: Row<'_>) {
        let Row { name, start, span, when, decoded_by, table, covers } = row;
        self.rows.push(row::SegmentDef {
            name: String::from(name),
            start: start.unwrap_or_else(|| self.start()),
            span,
            when,
            decoded_by: decoded_by.map(|ty| String::from(ty.trim())),
            table,
            covers,
        });
        self.last = String::from(name);
    }

    /// Move past a segment: by its bits if fixed, else restart counting after it.
    pub(super) fn advance(&mut self, bits: Option<u64>) {
        match bits {
            Some(bits) => self.at += bits,
            None => {
                self.from = Some(self.last.clone());
                self.at = 0;
            }
        }
    }

    /// The rows as `SegmentDef` literals.
    pub(super) fn table(&self, root: &Root) -> TokenStream {
        let rows = self.rows.iter().map(|row| row_tokens(row, root));
        quote!(#(#rows)*)
    }
}

/// The `CoverDef` literals for rows that cover bytes, or `None` if none do.
pub(super) fn covers_table(rows: &[row::SegmentDef], root: &Root) -> Option<TokenStream> {
    let covers: Vec<TokenStream> = rows
        .iter()
        .filter_map(|row| {
            let row::CoverDef { from, to, excludes_self } = row.covers.as_ref()?;
            let name = &row.name;
            let from = edge_tokens(from, root);
            let to = edge_tokens(to, root);
            Some(quote! {
                #root::table::CoverDef::new(#name, #from, #to, #excludes_self),
            })
        })
        .collect();
    (!covers.is_empty()).then(|| quote!(#(#covers)*))
}

fn edge_tokens(edge: &row::CoverEdge, root: &Root) -> TokenStream {
    match edge {
        row::CoverEdge::Start => quote!(#root::table::CoverEdge::Start),
        row::CoverEdge::Before(row) => quote!(#root::table::CoverEdge::Before(#row)),
        row::CoverEdge::After(row) => quote!(#root::table::CoverEdge::After(#row)),
    }
}

/// A row before it is placed.
pub(super) struct Row<'a> {
    name: &'a str,
    /// An explicit start for a segment decode seeks to. Otherwise [`Rows::start`].
    start: Option<row::Start>,
    span: row::Span,
    when: Option<row::Presence>,
    decoded_by: Option<&'a str>,
    table: Option<row::Table>,
    covers: Option<row::CoverDef>,
}

impl<'a> Row<'a> {
    pub(super) fn new(name: &'a str, span: row::Span) -> Self {
        Self { name, start: None, span, when: None, decoded_by: None, table: None, covers: None }
    }

    /// Start the row at the offset in field `by`.
    pub(super) fn seek(self, by: &str) -> Self {
        Self { start: Some(row::Start::Seek { by: String::from(by) }), ..self }
    }

    /// Make the row present only `when`.
    pub(super) fn when(self, when: row::Presence) -> Self {
        Self { when: Some(when), ..self }
    }

    pub(super) fn covers(self, covers: row::CoverDef) -> Self {
        Self { covers: Some(covers), ..self }
    }

    pub(super) fn decoded_by(self, decoded_by: Option<&'a str>) -> Self {
        Self { decoded_by, ..self }
    }

    /// Attach the `FIELDS` of hidden layout `ty`, which holds `fields` in `bits`.
    pub(super) fn table(self, ty: &Ident, fields: &[Field], bits: u64, endian: Endian) -> Self {
        let container = Container::Bytes { bytes: (bits / 8) as usize, endian };
        let table = row::Table { ty: ty.to_string(), fields: field_rows(fields, &container) };
        Self { table: Some(table), ..self }
    }
}

/// One `SegmentDef` literal.
pub(super) fn row_tokens(row: &row::SegmentDef, root: &Root) -> TokenStream {
    let Prelude { some, none, .. } = root.prelude();
    let name = &row.name;
    let start = start_tokens(&row.start, root);
    let span = span_tokens(&row.span, root);
    let when = match &row.when {
        None => quote!(#none),
        Some(when) => {
            let when = when_tokens(when, root);
            quote!(#some(#when))
        }
    };
    let decoded_by = match &row.decoded_by {
        Some(ty) => quote!(#some(#ty)),
        None => quote!(#none),
    };
    let fields = match &row.table {
        Some(row::Table { ty, .. }) => {
            let ty = ident(ty);
            quote!(<#ty as #root::Layout>::FIELDS)
        }
        None => quote!(&[]),
    };
    quote! {
        #root::table::SegmentDef::new(#name, #start, #span, #when, #decoded_by, #fields),
    }
}

fn when_tokens(when: &row::Presence, root: &Root) -> TokenStream {
    match when {
        row::Presence::Flag { field, mask } => {
            let mask = Literal::u64_unsuffixed(*mask);
            quote!(#root::table::Presence::Flag { field: #field, mask: #mask })
        }
        row::Presence::Offset => quote!(#root::table::Presence::Offset),
        row::Presence::Bit => quote!(#root::table::Presence::Bit),
        row::Presence::Remaining => quote!(#root::table::Presence::Remaining),
        row::Presence::Length { by } => quote!(#root::table::Presence::Length { by: #by }),
    }
}

fn start_tokens(start: &row::Start, root: &Root) -> TokenStream {
    match start {
        row::Start::At(at) => {
            let at = Literal::u64_unsuffixed(*at);
            quote!(#root::table::Start::At(#at))
        }
        row::Start::After { segment, bits } => {
            let bits = Literal::u64_unsuffixed(*bits);
            quote!(#root::table::Start::After { segment: #segment, bits: #bits })
        }
        row::Start::Seek { by } => quote!(#root::table::Start::Seek { by: #by }),
    }
}

fn span_tokens(span: &row::Span, root: &Root) -> TokenStream {
    match span {
        row::Span::Fixed(bits) => {
            let bits = Literal::u64_unsuffixed(*bits);
            quote!(#root::table::Span::Fixed(#bits))
        }
        row::Span::Counted { by, each } => {
            let by = by_tokens(by, root);
            let each = opt_u64(*each, root);
            quote!(#root::table::Span::Counted { by: #by, each: #each })
        }
        row::Span::Squared { by, each } => {
            let by = by_tokens(by, root);
            let each = opt_u64(*each, root);
            quote!(#root::table::Span::Squared { by: #by, each: #each })
        }
        row::Span::Strided { by, stride } => {
            let by = by_tokens(by, root);
            let stride = by_tokens(stride, root);
            quote!(#root::table::Span::Strided { by: #by, stride: #stride })
        }
        row::Span::Window { by } => {
            let by = by_tokens(by, root);
            quote!(#root::table::Span::Window { by: #by })
        }
        row::Span::SelfDelimiting => quote!(#root::table::Span::SelfDelimiting),
        row::Span::Terminated { mask } => {
            let mask = Literal::u8_unsuffixed(*mask);
            quote!(#root::table::Span::Terminated { mask: #mask })
        }
        row::Span::Until { terminator } => {
            let terminator = Literal::u8_unsuffixed(*terminator);
            quote!(#root::table::Span::Until { terminator: #terminator })
        }
        row::Span::Fill { cap } => {
            let cap = opt_u64(*cap, root);
            quote!(#root::table::Span::Fill { cap: #cap })
        }
        row::Span::Chosen { on } => {
            let on = discriminant_tokens(on, root);
            quote!(#root::table::Span::Chosen { on: #on })
        }
        row::Span::ChosenIn { on, bits } => {
            let on = discriminant_tokens(on, root);
            let bits = Literal::u64_unsuffixed(*bits);
            quote!(#root::table::Span::ChosenIn { on: #on, bits: #bits })
        }
        row::Span::ChosenWithin { on, by } => {
            let on = discriminant_tokens(on, root);
            let by = by_tokens(by, root);
            quote!(#root::table::Span::ChosenWithin { on: #on, by: #by })
        }
    }
}

fn discriminant_tokens(on: &Discriminant, root: &Root) -> TokenStream {
    match on {
        Discriminant::Field(f) => quote!(#root::table::Discriminant::Field(#f)),
        Discriminant::BodyLength => quote!(#root::table::Discriminant::BodyLength),
    }
}

fn by_tokens(by: &By, root: &Root) -> TokenStream {
    let By { field, scale, offset } = by;
    if *scale == 1 && *offset == 0 {
        return quote!(#root::table::By::field(#field));
    }
    let scale = Literal::u32_unsuffixed(*scale);
    let offset = Literal::i32_unsuffixed(*offset);
    quote!(#root::table::By { field: #field, scale: #scale, offset: #offset })
}

fn opt_u64(value: Option<u64>, root: &Root) -> TokenStream {
    let Prelude { some, none, .. } = root.prelude();
    match value {
        Some(n) => {
            let n = Literal::u64_unsuffixed(n);
            quote!(#some(#n))
        }
        None => quote!(#none),
    }
}

fn span_of_len(len: &Len) -> row::Span {
    match len {
        Len::Bytes(n) => row::Span::Fixed(*n as u64 * 8),
        Len::Field { by, .. } => row::Span::Window { by: by.clone() },
        Len::Until { terminator } => row::Span::Until { terminator: *terminator },
        Len::Fill => row::Span::Fill { cap: None },
    }
}

/// The span of one value of `kind`.
pub(super) fn span_of_kind(kind: &Kind) -> row::Span {
    match kind {
        Kind::Var { .. } | Kind::Msg { .. } => row::Span::SelfDelimiting,
        Kind::Text { len, .. } => span_of_len(len),
        other => row::Span::Fixed(other.width()),
    }
}

/// The span of a collection counted by `count`, with elements `each` bits wide if fixed.
pub(super) fn span_of_count(count: &Count, each: Option<u64>) -> row::Span {
    match count {
        Count::Field { by, .. } => row::Span::Counted { by: by.clone(), each },
        Count::Squared { by, .. } => row::Span::Squared { by: by.clone(), each },
        Count::Strided { by, stride, .. } => {
            row::Span::Strided { by: by.clone(), stride: stride.clone() }
        }
        Count::Window(len) => span_of_len(len),
        Count::Terminated { mask } => row::Span::Terminated { mask: *mask },
        Count::Fill { cap } => {
            row::Span::Fill { cap: cap.and_then(|c| each.map(|bits| c as u64 * bits)) }
        }
    }
}

/// The type a row of `kind` names, if any.
pub(super) fn deferred_type(kind: &Kind) -> Option<&str> {
    match kind {
        Kind::Nested { ty, .. }
        | Kind::NestedArray { ty, .. }
        | Kind::Codec { ty, .. }
        | Kind::Var { ty }
        | Kind::Msg { ty, .. } => Some(ty),
        _ => None,
    }
}
