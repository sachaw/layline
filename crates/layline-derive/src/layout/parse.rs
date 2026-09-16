//! Parses `#[layout(..)]` and the struct into [`Declared`].

use proc_macro2::Span;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields};

use layline_codegen::{BitOrder, Container, Endian};

use super::declared::*;
use super::field::{byte_field, word_field, words_field};
use super::misplaced::{LAYOUT_ATTRS, walk_vocabulary};
use crate::attr::{endian_value, int_value, order_value};
use crate::claim::Range;

enum ContainerAttr {
    Word {
        bits: u32,
        prefix: u32,
        prefix_value: Option<u128>,
        view: bool,
        endian: Endian,
        order: BitOrder,
    },
    Bytes {
        wire_bytes: usize,
        view: bool,
        endian: Endian,
    },
    Words {
        words: usize,
        endian: Endian,
        order: BitOrder,
        view: bool,
    },
}

pub struct Parsed {
    pub declared: Declared,
    pub root: layline_codegen::Root,
    pub internal: Internal,
}

pub fn parse(input: &DeriveInput) -> syn::Result<Parsed> {
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new(
            input.generics.span(),
            format!("`{}`: #[derive(Layout)] does not support generics", input.ident),
        ));
    }

    let (container, root, internal) = container_attr(input)?;
    for attr in &input.attrs {
        if let Some(name) = crate::attr::registered(attr, LAYOUT_ATTRS)
            && name != "layout"
        {
            if walk_vocabulary(name) {
                return Err(syn::Error::new(
                    attr.meta.span(),
                    format!(
                        "`{}`: `#[{name}]` is not supported on a layout. \
                         Use #[derive(Message)]",
                        input.ident,
                    ),
                ));
            }
            let hint = if name == "bits" {
                ". Set the layout's width with `#[layout(bits = N)]`"
            } else {
                ""
            };
            return Err(syn::Error::new(
                attr.meta.span(),
                format!(
                    "`{}`: `#[{name}]` belongs on a field. \
                     Only `#[layout(..)]` goes above the struct{hint}",
                    input.ident,
                ),
            ));
        }
    }

    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            format!("`{}`: #[derive(Layout)] needs a struct", input.ident),
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new(
            input.ident.span(),
            format!("`{}`: #[derive(Layout)] needs named fields", input.ident),
        ));
    };
    let zero_extent = matches!(
        container,
        ContainerAttr::Words { words: 0, .. } | ContainerAttr::Bytes { wire_bytes: 0, .. }
    );
    if named.named.is_empty() && !zero_extent {
        return Err(syn::Error::new(
            input.ident.span(),
            format!("`{}`: a layout needs at least one field", input.ident),
        ));
    }

    let layout = match container {
        ContainerAttr::Word { bits, prefix, prefix_value, view, endian, order } => {
            let fields = named.named.iter().map(word_field).collect::<syn::Result<Vec<_>>>()?;
            viewless(
                &input.ident,
                view,
                fields.iter().map(|f| (&f.ident, range_asserted(&f.range))),
            )?;
            overlays_apart(&input.ident, fields.iter().flat_map(|f| &f.overlays))?;
            Declared::Word(WordLayout {
                ident: input.ident.clone(),
                vis: input.vis.clone(),
                endian,
                order,
                bits,
                prefix,
                prefix_value,
                view,
                fields,
            })
        }
        ContainerAttr::Bytes { wire_bytes, view, endian } => {
            let fields = named.named.iter().map(byte_field).collect::<syn::Result<Vec<_>>>()?;
            viewless(
                &input.ident,
                view,
                fields.iter().map(|f| {
                    let asserted = f.magic.as_ref().map(|m| ("#[magic]", m.span));
                    let asserted = asserted.or(f.check.as_ref().map(|c| ("#[checksum]", c.span)));
                    (&f.ident, asserted.or(range_asserted(&f.range)))
                }),
            )?;
            Declared::Bytes(ByteLayout {
                ident: input.ident.clone(),
                vis: input.vis.clone(),
                endian,
                wire_bytes,
                view,
                fields,
            })
        }
        ContainerAttr::Words { words, endian, order, view } => {
            let fields = named.named.iter().map(words_field).collect::<syn::Result<Vec<_>>>()?;
            viewless(
                &input.ident,
                view,
                fields.iter().map(|f| (&f.ident, range_asserted(&f.range))),
            )?;
            overlays_apart(&input.ident, fields.iter().flat_map(|f| &f.overlays))?;
            Declared::Words(WordsLayout {
                ident: input.ident.clone(),
                vis: input.vis.clone(),
                endian,
                order,
                view,
                words,
                fields,
            })
        }
    };
    Ok(Parsed { declared: layout, root, internal })
}

fn container_attr(
    input: &DeriveInput,
) -> syn::Result<(ContainerAttr, layline_codegen::Root, Internal)> {
    let ident = &input.ident;
    let attr = crate::attr::single(&input.attrs, "layout", format_args!("`{ident}`"))?.ok_or_else(|| {
        syn::Error::new(
            ident.span(),
            format!(
                "`{ident}`: missing #[layout(bits = N)], #[layout(bytes = N)], or #[layout(words = N)]"
            ),
        )
    })?;

    let (mut bits, mut bytes, mut words): (Option<u32>, Option<usize>, Option<usize>) =
        (None, None, None);
    let (mut prefix, mut prefix_value): (Option<u32>, Option<u128>) = (None, None);
    let (mut view, mut internal, mut endian, mut order, mut root) = (None, None, None, None, None);
    attr.parse_nested_meta(|meta| {
        let is = |key: &str| meta.path.is_ident(key);
        if is("crate") {
            crate::attr::once_key(&mut root, &meta, ident, "layout", || {
                crate::root::parse_arg(&meta)
            })
        } else if is("bits") {
            crate::attr::once_key(&mut bits, &meta, ident, "layout", || int_value(&meta))
        } else if is("prefix") {
            crate::attr::once_key(&mut prefix, &meta, ident, "layout", || int_value(&meta))
        } else if is("prefix_value") {
            crate::attr::once_key(&mut prefix_value, &meta, ident, "layout", || int_value(&meta))
        } else if is("bytes") {
            crate::attr::once_key(&mut bytes, &meta, ident, "layout", || int_value(&meta))
        } else if is("words") {
            crate::attr::once_key(&mut words, &meta, ident, "layout", || int_value(&meta))
        } else if is("view") {
            crate::attr::once_key(&mut view, &meta, ident, "layout", || Ok(()))
        } else if is("internal") {
            crate::attr::once_key(&mut internal, &meta, ident, "layout", || Ok(()))
        } else if is("endian") {
            crate::attr::once_key(&mut endian, &meta, ident, "layout", || {
                endian_value(&meta, ident)
            })
        } else if is("order") {
            crate::attr::once_key(&mut order, &meta, ident, "layout", || order_value(&meta, ident))
        } else {
            Err(meta.error(format!(
                "`{ident}`: expected `bits`, `bytes`, `words`, `endian`, `order`, `prefix`, \
                 `prefix_value`, `view`, `internal`, or `crate`"
            )))
        }
    })?;

    let span = attr.meta.span();
    let (view, internal) = (view.is_some(), internal.is_some());
    let root = root.unwrap_or_default();
    let endian = endian.unwrap_or(Endian::Le);
    let (order_set, order) = (order.is_some(), order.unwrap_or(BitOrder::Lsb));
    let prefix_outside_word_mode = || {
        let key = if prefix.is_some() { "prefix" } else { "prefix_value" };
        (prefix.is_some() || prefix_value.is_some()).then(|| {
            syn::Error::new(span, format!("`{ident}`: `{key}` only applies to a `bits = N` layout"))
        })
    };
    let container = match (bits, bytes, words) {
        (None, None, Some(words)) => {
            if let Some(err) = prefix_outside_word_mode() {
                return Err(err);
            }
            ContainerAttr::Words { words, endian, order, view }
        }
        (None, Some(wire_bytes), None) => {
            if order_set {
                return Err(syn::Error::new(
                    span,
                    format!("`{ident}`: a `bytes = N` layout has no bit order. Remove `order`"),
                ));
            }
            if let Some(err) = prefix_outside_word_mode() {
                return Err(err);
            }
            ContainerAttr::Bytes { wire_bytes, view, endian }
        }
        (Some(bits), None, None) => {
            let prefix = prefix.unwrap_or(0);
            if bits == 0 {
                return Err(syn::Error::new(
                    span,
                    format!("`{ident}`: `bits = 0` is empty. A layout needs at least one byte"),
                ));
            }
            if !bits.is_multiple_of(8) {
                return Err(syn::Error::new(span, not_whole_bytes(ident, bits)));
            }
            prefix_admitted(ident, bits, prefix, prefix_value, span)?;
            ContainerAttr::Word { bits, prefix, prefix_value, view, endian, order }
        }
        (None, None, None) => {
            return Err(syn::Error::new(
                span,
                format!(
                    "`{ident}`: #[layout] needs a size. \
                     Write `bits = N`, `bytes = N`, or `words = N`"
                ),
            ));
        }
        _ => {
            return Err(syn::Error::new(
                span,
                format!("`{ident}`: #[layout] takes only one of `bits`, `bytes`, or `words`"),
            ));
        }
    };
    Ok((container, root, Internal(internal)))
}

/// Checks the prefix rules through codegen's validation.
fn prefix_admitted(
    ident: &syn::Ident,
    bits: u32,
    prefix: u32,
    prefix_value: Option<u128>,
    span: Span,
) -> syn::Result<()> {
    let container = Container::Word {
        bits: u64::from(bits),
        prefix: u64::from(prefix),
        prefix_value,
        endian: Endian::Le,
        order: BitOrder::Lsb,
    };
    layline_codegen::__derive::validate_container(&container)
        .map_err(|why| syn::Error::new(span, format!("`{ident}`: {why}")))
}

fn range_asserted(range: &Option<Range>) -> Option<(&'static str, Span)> {
    range.as_ref().map(|r| ("#[range]", r.span))
}

/// A view skips decode, so it cannot check `#[magic]`, `#[checksum]` or `#[range]`.
fn viewless<'a>(
    container: &syn::Ident,
    view: bool,
    fields: impl IntoIterator<Item = (&'a syn::Ident, Option<(&'static str, Span)>)>,
) -> syn::Result<()> {
    let asserting = fields.into_iter().find_map(|(field, asserted)| Some((field, asserted?)));
    let (Some((field, (what, span))), true) = (asserting, view) else {
        return Ok(());
    };
    Err(syn::Error::new(
        span,
        format!(
            "`{container}`: `view` cannot check the {what} on `{field}`. \
             Remove `view` or the {what}"
        ),
    ))
}

/// Overlay getters and `set_` setters must not clash with each other, `decode` or `encode`.
fn overlays_apart<'a>(
    container: &syn::Ident,
    overlays: impl IntoIterator<Item = &'a Overlay>,
) -> syn::Result<()> {
    let mut methods = vec![String::from("decode"), String::from("encode")];
    for ov in overlays {
        let name = crate::attr::name(&ov.name);
        for method in [format!("set_{name}"), name.clone()] {
            if methods.contains(&method) {
                return Err(syn::Error::new(
                    ov.span,
                    format!(
                        "overlay `{name}`: writes `fn {method}`, which `{container}` already has. \
                         Rename the overlay"
                    ),
                ));
            }
            methods.push(method);
        }
    }
    Ok(())
}

fn not_whole_bytes(ident: &syn::Ident, bits: u32) -> String {
    let whole = bits.next_multiple_of(8);
    let spare = whole - bits;
    format!(
        "`{ident}`: `bits = {bits}` is not a whole number of bytes. \
         Write `bits = {whole}` and end with `#[bits({spare})] spare: u8`"
    )
}
