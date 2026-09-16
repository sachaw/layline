//! Decode and encode for a list of segments, shared by messages and catalogue arms.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;

use super::Check;
use super::checksum::Marked;
use super::cursor::seek_to;
use super::env::Env;
use super::patch::{Slot, arm_ident, nested_in_blocks, nested_probe};
use super::table::Rows;
use crate::model::split_ref;
use crate::walk::tokens::ident;
use crate::walk::{Derived, derivations, endian_arg, row};
use crate::{Endian, Error, Kind, Len, Param, Root, Segment};

/// Where encode reads a body's values from.
#[derive(Clone, Copy)]
pub(super) enum Own {
    /// `self.name`
    Struct,
    /// `(*name)`, from a variant pattern.
    Arm,
}

impl Own {
    /// The value `name` refers to. A dotted name reads a field of a nested layout.
    pub(super) fn get(self, name: &str) -> TokenStream {
        let (outer, inner) = split_ref(name);
        let id = ident(outer);
        let base = match self {
            Own::Struct => quote!(self.#id),
            Own::Arm => quote!((*#id)),
        };
        match inner {
            None => base,
            Some(inner) => {
                let f = ident(inner);
                quote!(#base.#f)
            }
        }
    }

    /// A reference to the value `name` refers to.
    pub(super) fn by_ref(self, name: &str) -> TokenStream {
        let id = ident(name);
        match self {
            Own::Struct => quote!(&self.#id),
            Own::Arm => quote!(#id),
        }
    }
}

/// The message or catalogue being generated, and the declarations its bodies add.
pub(super) struct Item<'a> {
    pub(super) name: &'a str,
    pub(super) endian: Endian,
    pub(super) root: &'a Root,
    /// Parameters the holder supplies. Empty for a message that decodes alone.
    pub(super) needs: &'a [Param],
    /// Shared across arms, so hidden layout names from different arms don't collide.
    pub(super) block_n: usize,
    pub(super) opt_n: usize,
    pub(super) ck_n: usize,
    /// Hidden layouts the bodies read through.
    pub(super) items: TokenStream,
    pub(super) checks: Vec<Check>,
    /// The attribute a spare field carries, from this item's serde derive.
    pub(super) skip: TokenStream,
}

impl<'a> Item<'a> {
    pub(super) fn new(
        name: &'a str,
        endian: Endian,
        root: &'a Root,
        needs: &'a [Param],
        skip: TokenStream,
    ) -> Self {
        Self {
            name,
            endian,
            root,
            needs,
            skip,
            block_n: 0,
            opt_n: 0,
            ck_n: 0,
            items: TokenStream::new(),
            checks: Vec::new(),
        }
    }
}

/// A value decode binds, and the expression the constructor uses for it.
pub(super) struct Bind {
    pub(super) name: Ident,
    pub(super) ty: TokenStream,
    pub(super) value: TokenStream,
}

/// A generated body: field declarations, decode, encode and table.
pub(super) struct Walked {
    pub(super) fields: TokenStream,
    /// Binds `__base` to the body's first byte, before anything is written.
    pub(super) base: TokenStream,
    pub(super) binds: Vec<Bind>,
    /// Decode statements.
    pub(super) parse: TokenStream,
    /// Constructor field initialisers.
    pub(super) build: TokenStream,
    /// Encode statements.
    pub(super) bytes: TokenStream,
    /// `SEGMENTS` rows, as literals.
    pub(super) table: TokenStream,
    pub(super) rows: Vec<row::SegmentDef>,
}

/// State while generating one body.
pub(super) struct Walk<'a> {
    pub(super) root: &'a Root,
    pub(super) owner: &'a str,
    /// Parameters read from the holder. Encode never writes them.
    pub(super) params: &'a [Param],
    pub(super) endian: TokenStream,
    pub(super) derived: Vec<(String, Derived)>,
    /// Positions this body's checksums need.
    pub(super) marked: Marked<'a>,
    /// Names this body puts on the wire, in wire order.
    pub(super) places: Vec<&'a str>,
    /// Checksum checks that must wait for bytes read later.
    pub(super) late_checks: TokenStream,
    /// Stamps that must wait for bytes written later.
    pub(super) late_stamps: TokenStream,
    pub(super) public_fields: TokenStream,
    pub(super) binds: Vec<Bind>,
    pub(super) parse_steps: TokenStream,
    pub(super) byte_steps: TokenStream,
    pub(super) env: Env,
    pub(super) slots: Vec<Slot>,
    pub(super) rows: Rows,
    /// The attribute a spare field carries.
    pub(super) skip: TokenStream,
}

/// Generate decode, encode and the table for `segments` in one pass.
pub(super) fn walk_segments(
    item: &mut Item<'_>,
    segments: &[Segment],
    own: Own,
) -> Result<Walked, Error> {
    let (root, owner) = (item.root, item.name);
    let mut derived = derivations(segments);
    let rebased =
        derived.iter().any(|(_, d)| matches!(d, Derived::Offset { .. } | Derived::Checksum { .. }));
    // The holder owns a parameter's bytes, so encode never writes one.
    derived.retain(|(field, _)| !item.needs.iter().any(|p| p.name == *field));
    clashes(owner, &derived)?;

    let mut w = Walk {
        root,
        owner,
        params: item.needs,
        endian: endian_arg(item.endian),
        derived,
        marked: Marked::of(segments),
        places: segments.iter().flat_map(Segment::places).collect(),
        late_checks: TokenStream::new(),
        late_stamps: TokenStream::new(),
        public_fields: TokenStream::new(),
        binds: Vec::new(),
        parse_steps: TokenStream::new(),
        byte_steps: TokenStream::new(),
        env: Env::new(root),
        slots: Vec::new(),
        rows: Rows::new(),
        skip: item.skip.clone(),
    };
    for p in item.needs {
        let id = ident(&p.name);
        w.env.bind(&p.name, quote!(__ctx.#id));
    }

    for (_, d) in &w.derived {
        if let Derived::ArmLen { of, .. } = d {
            let arm = arm_ident(of);
            let held = own.get(of);
            w.byte_steps.extend(quote! {
                let #arm = {
                    let __mark = out.len();
                    let __wrote = #root::Choice::encode_into(&#held, out);
                    let __n = out.len() - __mark;
                    out.rewind(__mark);
                    __wrote?;
                    __n
                };
            });
        }
    }

    for (field, d) in &w.derived {
        let (outer, Some(inner)) = split_ref(field) else {
            continue;
        };
        let Some(ty) = nested_in_blocks(segments, outer) else {
            return Err(Error::Refused(format!(
                "{owner}: `{field}` reads a field inside `{outer}`, but `{outer}` is not a \
                 nested layout in a block. Name a nested layout field of this message"
            )));
        };
        item.checks.push(Check {
            field: field.clone(),
            tokens: nested_probe(owner, ty, outer, inner, d, root)?,
        });
    }

    for (i, seg) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        if let Some(at) = seeks(seg) {
            let start = w.env.get(owner, "#[at]", at)?;
            w.parse_steps.extend(seek_to(&start, at, root));
        }
        if !matches!(seg, Segment::Block(_)) {
            for name in seg.places() {
                w.marked.start(name, &mut w.parse_steps, &mut w.byte_steps);
            }
        }
        match seg {
            Segment::Block(_) => w.block(item, own, seg)?,
            Segment::Value { .. } => w.value(item, own, seg, last)?,
            Segment::Repeat { .. } => w.repeat(item, own, seg)?,
            Segment::Placed { .. } => w.placed(item, own, seg, last)?,
            Segment::Opt { .. } => w.opt(item, own, seg, last)?,
            Segment::Switch { .. } => w.switch(item, own, seg, last)?,
            Segment::Checksum { .. } => w.checksum(item, segments, own, seg)?,
        }
        if !matches!(seg, Segment::Block(_)) {
            for name in seg.places() {
                w.marked.end(name, &mut w.parse_steps, &mut w.byte_steps);
            }
        }
        w.rows.advance(seg.fixed_bits());
        w.parse_steps.extend(quote! {
            if __at > __hi {
                __hi = __at;
            }
        });
    }
    w.parse_steps.extend(w.late_checks);
    w.byte_steps.extend(w.late_stamps);

    let inits = w.binds.iter().map(|Bind { name, value, .. }| {
        if *name == value.to_string() { quote!(#name,) } else { quote!(#name: #value,) }
    });
    Ok(Walked {
        fields: w.public_fields,
        base: if rebased {
            quote! { let __base = out.len(); }
        } else {
            TokenStream::new()
        },
        parse: w.parse_steps,
        build: quote!(#(#inits)*),
        bytes: w.byte_steps,
        binds: w.binds,
        table: w.rows.table(root),
        rows: w.rows.rows,
    })
}

/// Refuse a field that two derivations write.
fn clashes(owner: &str, derived: &[(String, Derived)]) -> Result<(), Error> {
    if let Some((field, a, b)) = derived.iter().enumerate().find_map(|(i, (f, d))| {
        let other = derived[i + 1..]
            .iter()
            .find(|(g, e)| g == f && core::mem::discriminant(d) != core::mem::discriminant(e))?;
        Some((f, d, &other.1))
    }) {
        return Err(Error::Refused(format!(
            "{owner}: `{field}` is the {} and also the {}. {}",
            a.what(),
            b.what(),
            a.instead().or_else(|| b.instead()).unwrap_or("Give the second a field of its own"),
        )));
    }

    if let Some((flag, of)) = derived.iter().enumerate().find_map(|(i, (f, d))| {
        let Derived::Presence { of } = d else { return None };
        derived[i + 1..]
            .iter()
            .find(|(g, e)| g == f && matches!(e, Derived::Presence { .. }))
            .map(|_| (f, of))
    }) {
        return Err(Error::Refused(format!(
            "{owner}: `{flag}` is the whole of `{of}`'s presence flag and another optional field's. \
             Use a flags word with one mask per field (`#[when(w & 0x01)]`)"
        )));
    }

    if let Some(field) = derived.iter().enumerate().find_map(|(i, (f, d))| {
        derived[i + 1..].iter().find(|(g, e)| g == f && stride_clash(d, e)).map(|_| f)
    }) {
        return Err(Error::Refused(format!(
            "{owner}: `{field}` is the element width of two runs with different elements. \
             Give each stride its own field"
        )));
    }
    Ok(())
}

/// Whether two strides on one field give different widths.
fn stride_clash(a: &Derived, b: &Derived) -> bool {
    match (a, b) {
        (
            Derived::Stride { elem: x, scale: sx, offset: ox },
            Derived::Stride { elem: y, scale: sy, offset: oy },
        ) => x != y || sx != sy || ox != oy,
        _ => false,
    }
}

/// The offset field to seek to before `seg`, if the seek is unconditional.
///
/// A absent record seeks inside its own presence test.
fn seeks(seg: &Segment) -> Option<&str> {
    match seg {
        Segment::Repeat { at: Some(at), .. } | Segment::Placed { at, absent: None, .. } => Some(at),
        _ => None,
    }
}

/// Whether encode must clone a value into its hidden layout. Nested layouts are not `Copy`.
pub(super) fn needs_clone(kind: &Kind) -> bool {
    matches!(kind, Kind::NestedArray { .. } | Kind::Nested { .. })
}

impl Walk<'_> {
    /// The position of the next byte, counted from this body's first byte.
    pub(super) fn here(&self) -> TokenStream {
        let i64 = self.root.prelude().i64;
        quote!((out.len() - __base) as #i64)
    }

    /// Whether `name` is a parameter of this body.
    pub(super) fn is_param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p.name == name)
    }

    /// The number `name` refers to, in encode: from the context for a parameter, else from the value.
    pub(super) fn held(&self, name: &str, own: Own) -> TokenStream {
        if self.is_param(name) {
            let id = ident(name);
            return quote!(__ctx.#id);
        }
        own.get(name)
    }

    /// `let __n = …;`: a window's byte length, from a fixed number or an earlier field.
    pub(super) fn window_len(
        &self,
        len: &Len,
        what: &str,
        name: &str,
    ) -> Result<TokenStream, Error> {
        let owner = self.owner;
        match len {
            Len::Bytes(n) => {
                let lit = Literal::usize_unsuffixed(*n);
                Ok(quote! { let __n = #lit; })
            }
            Len::Field { by, cap } => self.env.stated_n(owner, "#[len]", by, *cap),
            other => Err(Error::Refused(format!(
                "{owner}: {what} `{name}` has length {other:?}. \
                 Use a fixed byte count or an earlier field"
            ))),
        }
    }
}
