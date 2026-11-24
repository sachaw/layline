//! Decode and encode for a list of segments, shared by messages and catalogue arms.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::quote;

use super::Check;
use super::checksum::Marked;
use super::cursor::{seek_to, skip_padding, write_padding};
use super::env::Env;
use super::eval::{condition, number};
use super::patch::{Slot, arm_ident, derived_flag, derived_value, nested_in_blocks, nested_probe};
use super::table::{Row, Rows};
use crate::model::{Ty, split_ref};
use crate::root::Prelude;
use crate::walk::tokens::ident;
use crate::walk::{Derived, derivations, endian_arg, expressions, row};
use crate::{Absence, Endian, Error, Expr, Kind, Len, Param, Root, Segment};

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
        done: Vec::new(),
    };
    for p in item.needs {
        let id = ident(&p.name);
        match p.repr {
            crate::Scalar::Bool => w.env.bind_flag(&p.name, quote!(__ctx.#id)),
            _ => w.env.bind(&p.name, quote!(__ctx.#id)),
        }
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

    // Every field of a nested layout an expression reads, and whether it reads it as a number.
    let nested: core::cell::RefCell<Vec<(String, bool)>> = core::cell::RefCell::default();
    for (e, want) in expressions(segments) {
        let record = |name: &str, ty: Ty| -> Result<(TokenStream, Ty), Error> {
            if split_ref(name).1.is_some() {
                let mut nested = nested.borrow_mut();
                match nested.iter_mut().find(|(n, _)| n == name) {
                    Some((_, number)) => *number |= ty == Ty::Int,
                    None => nested.push((String::from(name), ty == Ty::Int)),
                }
            }
            Ok((quote!(0), ty))
        };
        match want {
            Ty::Int => number(&e, &record, root)?,
            Ty::Bool => condition(&e, &record, root)?,
        };
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

    for (i, seg) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        if let Some(at) = seeks(seg) {
            let start = w.env.number(owner, "#[seek]", at)?;
            w.parse_steps.extend(seek_to(&start, super::eval::blamed(at, owner), root));
        }
        if !matches!(seg, Segment::Block(_)) {
            for name in seg.places() {
                w.marked.start(name, &mut w.parse_steps, &mut w.byte_steps);
            }
        }
        // A placed record pads inside its own window.
        let padded = seg.align().filter(|_| !matches!(seg, Segment::Placed { .. }));
        if padded.is_some() {
            w.parse_steps.extend(quote! { let __pad_from = __at; });
            w.byte_steps.extend(quote! { let __pad_from = out.len(); });
        }
        match seg {
            Segment::Block(_) => w.block(item, own, seg)?,
            Segment::Value { .. } => w.value(item, own, seg, last)?,
            Segment::Repeat { .. } => w.repeat(item, own, seg)?,
            Segment::Placed { .. } => w.placed(item, own, seg, last)?,
            Segment::Opt { .. } => w.opt(item, own, seg, last)?,
            Segment::Switch { .. } => w.switch(item, own, seg, last)?,
            Segment::Checksum { .. } => w.checksum(item, segments, own, seg)?,
            Segment::Valid { field, holds } => w.valid(own, field, holds)?,
        }
        if !matches!(seg, Segment::Block(_)) {
            for name in seg.places() {
                w.marked.end(name, &mut w.parse_steps, &mut w.byte_steps);
            }
        }
        w.done.extend(seg.places().into_iter().map(String::from));
        if let Some(align) = padded {
            w.parse_steps.extend(skip_padding(align, root));
            w.byte_steps.extend(write_padding(align));
        }
        if let (Some(align), Some(name)) = (seg.align(), seg.name()) {
            w.rows.advance(None);
            let pad = format!("{name}.pad");
            let mut row = Row::new(&pad, row::Span::Pad { align });
            if let Segment::Placed { absent: Some(Absence::ZeroLength { by, .. }), .. } = seg {
                row = row.when(row::Presence::Length { by: by.clone() });
            }
            w.rows.push(row);
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
        let Derived::Presence { of, .. } = d else { return None };
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
        (Derived::Stride { elem: x, by: a }, Derived::Stride { elem: y, by: b }) => {
            x != y || a != b
        }
        _ => false,
    }
}

/// The offset to seek to before `seg`, if the seek is unconditional.
///
/// An absent record seeks inside its own presence test.
fn seeks(seg: &Segment) -> Option<&Expr> {
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
        let Prelude { i64, some, err, .. } = self.root.prelude();
        let root = self.root;
        let stated = number(e, &|name, want| self.written(own, name, want), root)?;
        Ok(quote! {
            {
                let __value = (#value) as #i64;
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
