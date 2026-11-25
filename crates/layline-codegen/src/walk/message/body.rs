//! Decode and encode for a list of segments, shared by messages and catalogue arms.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;

use super::Check;
use super::checksum::Marked;
use super::cursor::{seek_to, seek_to_stream, skip_padding, write_padding};
use super::env::Env;
use super::eval::{argument, condition, number};
use super::patch::{Slot, arm_ident, derived_flag, derived_value, nested_in_blocks, nested_probe};
use super::table::{Row, Rows};
use crate::model::{Ty, split_ref};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{
    Derived, arguments, callers, derivations, endian_arg, expressions, row, writers,
};
use crate::{
    Absence, Count, Endian, Error, Expr, Kind, Len, Origin, Param, Presence, Root, Segment,
};

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
    /// The lifetime borrowed fields hold, without the `'`.
    pub(super) lifetime: Option<&'a str>,
    /// The `ENCODE_FALLIBLE` of each layout and codec the bodies encode through.
    pub(super) via: Vec<TokenStream>,
    /// The `NODE` of each message and catalogue the bodies hold.
    pub(super) links: Vec<TokenStream>,
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
            lifetime: None,
            via: Vec::new(),
            links: Vec::new(),
        }
    }

    /// Join what encoding a held type adds to this item's refusals.
    pub(super) fn hold(&mut self, held: Option<super::refuse::Held>) {
        match held {
            Some(super::refuse::Held::Leaf(t)) => self.via.push(t),
            Some(super::refuse::Held::Link(t)) => self.links.push(t),
            None => {}
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
    /// The segments encode has written so far.
    pub(super) done: Vec<String>,
    /// Whether the segment being walked is in the tail, which decode reads before the body.
    pub(super) in_tail: bool,
}

/// Generate decode, encode and the table for `segments` in one pass. The segments from index
/// `tail` on are the body's tail, which decode reads first, from the end.
pub(super) fn walk_segments(
    item: &mut Item<'_>,
    segments: &[Segment],
    tail: usize,
    own: Own,
) -> Result<Walked, Error> {
    let (root, owner) = (item.root, item.name);
    let mut derived = derivations(segments);
    let rebased =
        derived.iter().any(|(_, d)| matches!(d, Derived::Offset { .. } | Derived::Checksum { .. }));
    // The holder owns a parameter's bytes, so encode never writes one.
    derived.retain(|(field, _)| !item.needs.iter().any(|p| p.name == *field));
    // Encode works back through integers: an expression that reads a real parameter is checked.
    let real = |name: &str| {
        item.needs
            .iter()
            .any(|p| p.name == name && matches!(p.repr, crate::Scalar::F32 | crate::Scalar::F64))
    };
    derived.retain(|(_, d)| !d.by().is_some_and(|by| by.names().into_iter().any(real)));
    let derived = writers(callers(segments, derived));

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
        done: Vec::new(),
        in_tail: false,
    };
    for p in item.needs {
        let id = ident(&p.name);
        match p.repr {
            crate::Scalar::Bool => w.env.bind_flag(&p.name, quote!(__ctx.#id)),
            crate::Scalar::F32 | crate::Scalar::F64 => w.env.bind_real(&p.name, quote!(__ctx.#id)),
            _ => w.env.bind(&p.name, quote!(__ctx.#id)),
        }
    }

    let measured: Vec<String> = w
        .derived
        .iter()
        .filter_map(|(_, d)| match d {
            Derived::ArmLen { of, .. } => Some(of.clone()),
            _ => None,
        })
        .collect();
    for of in &measured {
        {
            let arm = arm_ident(of);
            let held = own.get(of);
            let ctx = match segments.iter().find(|s| s.name() == Some(of.as_str())) {
                Some(Segment::Switch { choice, with, .. }) if !with.is_empty() => {
                    let cty = crate::walk::path(choice);
                    w.ctx_encode(&quote!(<#cty as #root::Choice>::Ctx), of, with, own)?
                }
                _ => quote!(()),
            };
            w.byte_steps.extend(quote! {
                let #arm = {
                    let __mark = out.len();
                    let __wrote = #root::Choice::encode_in_stream(&#held, out, __mark, #ctx);
                    let __n = out.len() - __mark;
                    out.rewind(__mark);
                    __wrote?;
                    __n
                };
            });
        }
    }

    // Every field of a nested layout an expression reads, and whether it reads it as a number.
    let nested: core::cell::RefCell<Vec<(String, bool)>> = core::cell::RefCell::default();
    // The real fields and parameters, which read as reals wherever they are named.
    let real = |kind: &Kind| matches!(kind, Kind::Scalar(crate::Scalar::F32 | crate::Scalar::F64));
    let reals: Vec<&str> = segments
        .iter()
        .flat_map(|s| match s {
            Segment::Block(fields) => fields.iter().collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .filter(|f| real(&f.kind))
        .map(|f| f.name.as_str())
        .chain(item.needs.iter().filter(|p| real(&Kind::Scalar(p.repr))).map(|p| p.name.as_str()))
        .collect();
    let record = |name: &str, ty: Ty| -> Result<(TokenStream, Ty), Error> {
        if reals.contains(&name) {
            return Ok((quote!(0.0), Ty::Real));
        }
        if split_ref(name).1.is_some() {
            let mut nested = nested.borrow_mut();
            match nested.iter_mut().find(|(n, _)| n == name) {
                Some((_, number)) => *number |= ty == Ty::Int,
                None => nested.push((String::from(name), ty == Ty::Int)),
            }
        }
        Ok((quote!(0), ty))
    };
    for (e, want) in expressions(segments) {
        match want {
            Ty::Int | Ty::Real => number(&e, &record, root)?,
            Ty::Bool => condition(&e, &record, root)?,
        };
    }
    // A bare `#[with]` name passes the field itself, and any other value reads as expressions do.
    for e in arguments(segments).into_iter().filter(|e| e.as_name().is_none()) {
        argument(e, &record, root)?;
    }
    for (field, number) in nested.into_inner() {
        let (outer, Some(inner)) = split_ref(&field) else { continue };
        let Some(ty) = nested_in_blocks(segments, outer) else {
            return Err(Error::Refused(format!(
                "{owner}: `{field}` reads a field inside `{outer}`, but `{outer}` is not a \
                 nested layout in a block. Name a nested layout field of this message"
            )));
        };
        item.checks.push(Check {
            tokens: nested_probe(owner, ty, outer, inner, number, root)?,
            field: field.clone(),
        });
    }

    let point = tail_point(segments, tail);
    let order: Vec<usize> =
        (0..point).chain((tail..segments.len()).rev()).chain(point..tail).collect();
    let n = segments.len();
    let (mut parse_of, mut bytes_of, mut fields_of) =
        (vec![TokenStream::new(); n], vec![TokenStream::new(); n], vec![TokenStream::new(); n]);
    let mut size_of = vec![TokenStream::new(); n];
    let mut rows_of = vec![0usize; n];
    // The body's rows while the tail's, which count from the end, are walked.
    let mut body_rows: Option<Rows> = None;
    let mut tail_rows: Option<Rows> = None;
    for &i in &order {
        let seg = &segments[i];
        // The last segment before the tail reads up to it. A condition takes no bytes, so one
        // after it does not count.
        let end = if i < tail { tail } else { n };
        let last = segments[i + 1..end].iter().all(|s| matches!(s, Segment::Valid { .. }));
        w.in_tail = i >= tail;
        w.done = segments[..i].iter().flat_map(Segment::places).map(String::from).collect();
        if i + 1 == n && i >= tail {
            body_rows = Some(core::mem::replace(&mut w.rows, Rows::new()));
        }
        if w.in_tail {
            size_of[i] = w.tail_size(seg)?;
        }
        let saved = (
            core::mem::take(&mut w.parse_steps),
            core::mem::take(&mut w.byte_steps),
            core::mem::take(&mut w.public_fields),
        );
        let rows_before = w.rows.rows.len();
        w.walk_one(item, segments, seg, own, last)?;
        rows_of[i] = w.rows.rows.len() - rows_before;
        parse_of[i] = core::mem::replace(&mut w.parse_steps, saved.0);
        bytes_of[i] = core::mem::replace(&mut w.byte_steps, saved.1);
        fields_of[i] = core::mem::replace(&mut w.public_fields, saved.2);
        if i == tail && i < n {
            let body = body_rows.take().expect("saved at the tail's last segment");
            tail_rows = Some(core::mem::replace(&mut w.rows, body));
        }
    }

    // Decode: the body up to the tail point, the tail from the end, then the rest of the body.
    for read in &mut parse_of[..point] {
        w.parse_steps.extend(core::mem::take(read));
    }
    if tail < n {
        let Prelude { err, usize, .. } = root.prelude();
        w.parse_steps.extend(quote! {
            let __head_at = __at;
            let mut __tail_end = __body.len();
        });
        for i in (tail..n).rev() {
            let name = tail_name(&segments[i]);
            let size = &size_of[i];
            let read = core::mem::take(&mut parse_of[i]);
            w.parse_steps.extend(quote! {
                let __n: #usize = #size;
                let __from = match __tail_end.checked_sub(__n) {
                    ::core::option::Option::Some(__from) if __from >= __head_at => __from,
                    _ => {
                        return #err(#root::ParseError::Short {
                            need_bytes: __head_at.saturating_add(__n),
                            got_bytes: __tail_end,
                            at: __head_at,
                        });
                    }
                };
                __at = __from;
                #read
                if __at != __tail_end {
                    return #err(#root::ParseError::Malformed { field: #name, at: __from });
                }
                __tail_end = __from;
            });
        }
        w.parse_steps.extend(quote! { __at = __head_at; });
        if point < tail {
            w.parse_steps.extend(quote! {
                let __whole = __body;
                let __body = &__whole[..__tail_end];
            });
        }
    }
    for read in &mut parse_of[point..tail] {
        w.parse_steps.extend(core::mem::take(read));
    }
    if tail < n {
        // The body must end where the tail starts: no byte between them is unaccounted for.
        let err = root.prelude().err;
        let name = tail_name(&segments[tail]);
        w.parse_steps.extend(quote! {
            if __at != __tail_end {
                return #err(#root::ParseError::Malformed { field: #name, at: __at });
            }
        });
    }

    // Encode, and the declared fields, in wire order.
    for (bytes, fields) in bytes_of.into_iter().zip(fields_of) {
        w.byte_steps.extend(bytes);
        w.public_fields.extend(fields);
    }

    // The tail's rows were pushed last segment first. Each ends a counted distance before the
    // body's end, or before the nearest later tail segment whose size is read from the wire.
    if let Some(mut walked) = tail_rows {
        let (mut before, mut bits) = (None::<String>, 0u64);
        let mut placed: Vec<Vec<row::SegmentDef>> = Vec::new();
        for i in (tail..n).rev() {
            let mut group: Vec<row::SegmentDef> = walked.rows.drain(..rows_of[i]).collect();
            for r in &mut group {
                r.start = row::Start::Tail { before: before.clone(), bits };
            }
            match segments[i].fixed_bits() {
                Some(b) => bits += b,
                None => {
                    before = Some(String::from(tail_name(&segments[i])));
                    bits = 0;
                }
            }
            placed.push(group);
        }
        w.rows.rows.extend(placed.into_iter().rev().flatten());
    }
    if tail < n && point < tail && !w.late_checks.is_empty() {
        // A check that waited reads the whole body, the tail included.
        w.parse_steps.extend(quote! { let __body = __whole; });
    }
    w.parse_steps.extend(w.late_checks);
    w.byte_steps.extend(w.late_stamps);

    let inits = w.binds.iter().map(|Bind { name, value, .. }| {
        if *name == value.to_string() { quote!(#name,) } else { quote!(#name: #value,) }
    });
    Ok(Walked {
        fields: w.public_fields,
        base: if rebased || super::mentions(&w.byte_steps, "__base") {
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

/// The index of the first segment before `tail` that decode cannot read until it knows where
/// the tail starts: one that reaches the end of the body, or reads a tail field. `tail` if none.
fn tail_point(segments: &[Segment], tail: usize) -> usize {
    if tail == segments.len() {
        return tail;
    }
    let names: Vec<&str> = segments[tail..].iter().flat_map(declared).collect();
    let reads = |seg: &Segment| {
        let one = core::slice::from_ref(seg);
        let exprs = crate::walk::expressions(one);
        let mut read: Vec<&Expr> = exprs.iter().map(|(e, _)| e).collect();
        read.extend(crate::walk::arguments(one));
        read.iter().flat_map(|e| e.names()).any(|r| names.contains(&split_ref(r).0))
    };
    let reaches_end = |seg: &Segment| {
        seg.open_ended()
            || matches!(
                seg,
                Segment::Switch { window: None, .. }
                    | Segment::Value { kind: Kind::Msg { .. }, .. }
                    | Segment::Placed { .. }
                    | Segment::Opt { when: Presence::Remaining, .. }
                    | Segment::Opt { field: crate::Field { kind: Kind::Msg { .. }, .. }, .. }
            )
    };
    (0..tail).find(|&i| reaches_end(&segments[i]) || reads(&segments[i])).unwrap_or(tail)
}

/// The field names a segment declares.
fn declared(seg: &Segment) -> Vec<&str> {
    match seg {
        Segment::Block(fields) => fields.iter().map(|f| f.name.as_str()).collect(),
        other => other.name().into_iter().collect(),
    }
}

/// The name a tail segment's refusals report: its field, or a block's first.
fn tail_name(seg: &Segment) -> &str {
    match seg {
        Segment::Block(fields) => fields.first().map_or("tail", |f| f.name.as_str()),
        Segment::Valid { field, .. } => field,
        other => other.name().unwrap_or("tail"),
    }
}

impl Walk<'_> {
    /// Decode, encode and rows for one segment.
    fn walk_one(
        &mut self,
        item: &mut Item<'_>,
        segments: &[Segment],
        seg: &Segment,
        own: Own,
        last: bool,
    ) -> Result<(), Error> {
        let (root, owner) = (self.root, self.owner);
        if let Some((at, from)) = seeks(seg) {
            let start = self.env.number(owner, "#[seek]", at)?;
            let field = super::eval::blamed(at, owner);
            self.parse_steps.extend(match from {
                Origin::Message => seek_to(&start, field, root),
                Origin::Stream => seek_to_stream(&start, field, root),
            });
        }
        if !matches!(seg, Segment::Block(_)) {
            for name in seg.places() {
                self.marked.start(name, &mut self.parse_steps, &mut self.byte_steps);
            }
        }
        // A placed record pads inside its own window.
        let padded = seg.align().filter(|_| !matches!(seg, Segment::Placed { .. }));
        if padded.is_some() {
            self.parse_steps.extend(quote! { let __pad_from = __at; });
            self.byte_steps.extend(quote! { let __pad_from = out.len(); });
        }
        match seg {
            Segment::Block(_) => self.block(item, own, seg)?,
            Segment::Value { .. } => self.value(item, own, seg, last)?,
            Segment::Repeat { .. } => self.repeat(item, own, seg)?,
            Segment::Placed { .. } => self.placed(item, own, seg, last)?,
            Segment::Opt { .. } => self.opt(item, own, seg, last)?,
            Segment::Switch { .. } => self.switch(item, own, seg, last)?,
            Segment::Checksum { .. } => self.checksum(item, segments, own, seg)?,
            Segment::Valid { field, holds } => self.valid(own, field, holds)?,
        }
        if !matches!(seg, Segment::Block(_)) {
            for name in seg.places() {
                self.marked.end(name, &mut self.parse_steps, &mut self.byte_steps);
            }
        }
        if let Some(align) = padded {
            self.parse_steps.extend(skip_padding(align, root));
            self.byte_steps.extend(write_padding(align));
        }
        if let (Some(align), Some(name)) = (seg.align(), seg.name()) {
            self.rows.advance(None);
            let pad = format!("{name}.pad");
            let mut row = Row::new(&pad, row::Span::Pad { align });
            if let Segment::Placed { absent: Some(Absence::ZeroLength { by, .. }), .. } = seg {
                row = row.when(row::Presence::Length { by: by.clone() });
            }
            self.rows.push(row);
        }
        self.rows.advance(seg.fixed_bits());
        self.parse_steps.extend(quote! {
            if __at > __hi {
                __hi = __at;
            }
        });
        Ok(())
    }

    /// The byte size of a tail segment, from what decode has read: its own width, or a count or
    /// length over later tail fields, earlier fields and parameters, and zero when it is absent.
    fn tail_size(&self, seg: &Segment) -> Result<TokenStream, Error> {
        let (root, owner) = (self.root, self.owner);
        let refuse = || {
            Error::Refused(format!(
                "{owner}: tail field `{}` reads its own size from the wire, and decode must know \
                 it before reading it, from the end. Give it a fixed width, a count or a length",
                tail_name(seg)
            ))
        };
        let bytes = |bits: u64| Literal::usize_unsuffixed((bits / 8) as usize);
        let gated = |when: Option<&Presence>, size: TokenStream| -> Result<TokenStream, Error> {
            Ok(match when {
                Some(Presence::When(e)) => {
                    let present = self.env.condition(owner, "#[when]", e)?;
                    quote!(if #present { #size } else { 0 })
                }
                Some(_) => return Err(refuse()),
                None => size,
            })
        };
        match seg {
            Segment::Valid { .. } => Ok(quote!(0)),
            Segment::Block(_) | Segment::Checksum { .. } => {
                let n = bytes(seg.fixed_bits().ok_or_else(refuse)?);
                Ok(quote!(#n))
            }
            Segment::Opt { when, field } => {
                let n = bytes(field.kind.bits().ok_or_else(refuse)?);
                gated(Some(when), quote!(#n))
            }
            Segment::Repeat { count, element, when, at: None, .. } => {
                let size = match count {
                    Count::Window(Len::Bytes(k)) => {
                        let k = Literal::usize_unsuffixed(*k);
                        quote!(#k)
                    }
                    Count::Window(Len::Field { by, .. }) => self.env.size(owner, "#[len]", by)?,
                    Count::Field { by, .. } => {
                        let each = bytes(element.bits().ok_or_else(refuse)?);
                        let count = self.env.size(owner, "#[count]", by)?;
                        let field = tail_name(seg);
                        quote! {
                            #count.checked_mul(#each).ok_or(
                                #root::ParseError::Malformed { field: #field, at: __tail_end },
                            )?
                        }
                    }
                    _ => return Err(refuse()),
                };
                gated(when.as_ref(), size)
            }
            Segment::Value { kind: Kind::Text { len, .. }, .. } => match len {
                Len::Bytes(k) => {
                    let k = Literal::usize_unsuffixed(*k);
                    Ok(quote!(#k))
                }
                Len::Field { by, .. } => self.env.size(owner, "#[len]", by),
                _ => Err(refuse()),
            },
            _ => Err(refuse()),
        }
    }
}

/// The offset to seek to before `seg`, if the seek is unconditional.
///
/// An absent record seeks inside its own presence test.
fn seeks(seg: &Segment) -> Option<(&Expr, Origin)> {
    match seg {
        Segment::Repeat { at: Some(at), from, .. }
        | Segment::Switch { at: Some(at), from, .. }
        | Segment::Placed { at, from, absent: None, .. } => Some((at, *from)),
        _ => None,
    }
}

/// Whether encode must clone a value into its hidden layout. Nested layouts are not `Copy`.
pub(super) fn needs_clone(kind: &Kind) -> bool {
    matches!(kind, Kind::NestedArray { .. } | Kind::Nested { .. })
}

impl Walk<'_> {
    /// The position of the next byte, counted from this body's first byte or the stream's.
    pub(super) fn here_from(&self, from: Origin) -> TokenStream {
        let i64 = self.root.prelude().i64;
        match from {
            Origin::Message => quote!((out.len() - __base) as #i64),
            Origin::Stream => quote!((out.len() - __stream) as #i64),
        }
    }

    /// Whether `name` is a parameter of this body.
    pub(super) fn is_param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p.name == name)
    }

    /// How `name` reads in encode, for [`eval`](super::eval): the value encode writes for it.
    ///
    /// A parameter from the context, a derived field as encode derives it, and any other field as
    /// the value holds it. Refuses a field encode fills in only after a later segment.
    pub(super) fn written(
        &self,
        own: Own,
        name: &str,
        want: Ty,
    ) -> Result<(TokenStream, Ty), Error> {
        let (root, owner) = (self.root, self.owner);
        let ty = self.env.ty(name).unwrap_or(want);
        let held = if self.is_param(name) {
            let id = ident(name);
            quote!(__ctx.#id)
        } else if let Some((_, d)) = self.derived.iter().find(|(f, _)| f == name) {
            if let Some((_, of)) = d.reserves() {
                // A collection's number is stamped by the time a later segment checks.
                let record = matches!(
                    d,
                    Derived::Offset { record: true, .. } | Derived::ByteLen { record: true, .. }
                );
                if !record && self.done.iter().any(|d| d == of) {
                    let wrote = super::patch::wrote_ident(name);
                    return Ok((quote!(#wrote), Ty::Int));
                }
                return Err(Error::Refused(format!(
                    "{owner}: an expression reads `{name}`, which encode fills in only after \
                     writing `{of}`, so nothing states it in time. Read a field that comes from \
                     the value or a parameter"
                )));
            }
            if matches!(d, Derived::Checksum { .. }) {
                return Err(Error::Refused(format!(
                    "{owner}: an expression reads the checksum `{name}`. Read a field the \
                     checksum covers instead"
                )));
            }
            if let Some(flag) = derived_flag(d, own) {
                return Ok((flag, Ty::Bool));
            }
            return Ok((derived_value(d, own, name, root)?, Ty::Int));
        } else {
            own.get(name)
        };
        Ok(match ty {
            Ty::Int => (quote!(#root::WireInt::to_i64(&#held)), Ty::Int),
            Ty::Bool => (quote!(#root::__private::WireFlag::to_flag(&#held)), Ty::Bool),
            Ty::Real => (quote!(#root::__private::value::real::Real::real(&#held)), Ty::Real),
        })
    }

    /// A condition the value must satisfy: decode refuses a body that breaks it, and encode a
    /// value that does.
    fn valid(&mut self, own: Own, field: &str, holds: &Expr) -> Result<(), Error> {
        let Prelude { err, some, .. } = self.root.prelude();
        let root = self.root;
        let read = self.env.condition(self.owner, "#[valid]", holds)?;
        self.parse_steps.extend(quote! {
            if !#read {
                return #err(#root::ParseError::Malformed { field: #field, at: __at });
            }
        });
        let stated = condition(holds, &|name, want| self.written(own, name, want), root)?;
        self.byte_steps.extend(quote! {
            match #stated {
                #some(__holds) => #root::EncodeError::agree(
                    #field,
                    ::core::primitive::i64::from(__holds),
                    1,
                )?,
                _ => return #err(#root::EncodeError::Inexact { field: #field, value: 0 }),
            }
        });
        Ok(())
    }

    /// Whether encode writes the field this segment's derivation `mine` works back to: the
    /// first derivation of that field is `mine`. Otherwise encode checks the expression.
    pub(super) fn writes(&self, mine: impl Fn(&Derived) -> bool) -> bool {
        let Some((field, _)) = self.derived.iter().find(|(_, d)| mine(d)) else { return false };
        self.derived.iter().find(|(f, _)| f == field).is_some_and(|(_, d)| mine(d))
    }

    /// Refuse a `value`, an `i64`, that differs from the number `e` states over the fields as
    /// encode writes them: `Mismatch` naming `field`, or `Inexact` where `e` has no number.
    pub(super) fn agree(
        &self,
        own: Own,
        field: &str,
        value: TokenStream,
        e: &Expr,
    ) -> Result<TokenStream, Error> {
        let Prelude { some, err, .. } = self.root.prelude();
        let root = self.root;
        let stated = number(e, &|name, want| self.written(own, name, want), root)?;
        Ok(quote! {
            {
                let __value = #root::__private::value::Wide::wide(#value);
                match #stated {
                    #some(__stated) => #root::EncodeError::agree(#field, __value, __stated)?,
                    _ => return #err(#root::EncodeError::Inexact { field: #field, value: __value }),
                }
            }
        })
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
            Len::Field { by, cap, .. } => self.env.size_n(owner, "#[len]", by, *cap),
            other => Err(Error::Refused(format!(
                "{owner}: {what} `{name}` has length {other:?}. \
                 Use a fixed byte count or an earlier field"
            ))),
        }
    }
}
