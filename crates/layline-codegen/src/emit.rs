//! Formatted Rust source generated from the [model](crate).
//!
//! [`Module::new`] uses [`Spelling::Bare`]: `Vec<Entry>`, `u32`, `Ok(..)`. Runtime paths stay
//! qualified. A module with a message, choice or dispatch imports `Box`, `String` and `Vec`
//! from `::layline::__private`, so it builds under `no_std` with `alloc`.

mod committed;
mod graph;
mod verbatim;

use std::fmt::Write as _;

use proc_macro2::TokenStream;
use quote::quote;

pub use committed::{Change, Committed, Drift, Drifted};

use crate::walk::{emit_choice, emit_dispatch, emit_enum, emit_layout, emit_message};
use crate::{Derive, Error, Invalid, Item, Root, Spelling};

/// A module to generate.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Module {
    /// Module doc lines, without `//!`, written after the `// @generated` marker.
    ///
    /// ```no_run
    /// # use layline_codegen::emit::Module;
    /// const COMMAND: &str = "cargo codegen";
    /// Module::new(vec![]).with_doc(vec![
    ///     "The block catalogue.".into(),
    ///     String::new(),
    ///     format!("Regenerate with `{COMMAND}`. Edit `codegen/src/spec.rs`."),
    ///     String::new(),
    ///     "Source: reference manual, revision 4, 2025-10-09.".into(),
    /// ]);
    /// ```
    pub doc: Vec<String>,
    /// `use` lines, each written as `use <line>;`.
    pub uses: Vec<String>,
    /// Extra derives for every struct and enum, after `Debug, Clone, PartialEq`.
    ///
    /// Derives that name a `cfg` follow in one `#[cfg_attr(..)]` per predicate.
    pub derives: Vec<Derive>,
    /// The items, in output order.
    pub items: Vec<Item>,
    /// The runtime path and prelude spelling.
    pub root: Root,
}

impl Module {
    /// A module of `items`, with root `::layline` in [`Spelling::Bare`] and nothing else set.
    #[must_use]
    pub fn new(items: Vec<Item>) -> Self {
        Self {
            doc: Vec::new(),
            uses: Vec::new(),
            derives: Vec::new(),
            items,
            root: Root::new(syn::parse_quote!(::layline), Spelling::Bare),
        }
    }

    /// Sets [`root`](Self::root).
    #[must_use]
    pub fn with_root(self, root: Root) -> Self {
        Self { root, ..self }
    }

    /// Sets [`doc`](Self::doc).
    #[must_use]
    pub fn with_doc(self, doc: Vec<String>) -> Self {
        Self { doc, ..self }
    }

    /// Sets [`uses`](Self::uses).
    #[must_use]
    pub fn with_uses(self, uses: Vec<String>) -> Self {
        Self { uses, ..self }
    }

    /// Sets [`derives`](Self::derives).
    #[must_use]
    pub fn with_derives(self, derives: Vec<Derive>) -> Self {
        Self { derives, ..self }
    }
}

/// A generated module and its audit artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Generated {
    /// The formatted Rust source.
    pub source: String,
    /// The field tables, one record per line, in `layline_core::audit`'s format.
    ///
    /// Empty when the module has only enums and verbatim items.
    pub audit: String,
}

/// Generates a module's source and audit artifact.
///
/// ```
/// use layline_codegen::emit::{Module, generate};
/// use layline_codegen::{Container, Endian, Field, Item, Kind, LayoutDef, Scalar};
///
/// let head = LayoutDef::new(
///     "Head",
///     Container::Bytes { bytes: 2, endian: Endian::Be },
///     vec![Field::new("len", Kind::Scalar(Scalar::U(16)))],
/// );
/// let out = generate(&Module::new(vec![Item::Layout(head)])).unwrap();
/// assert!(out.source.contains("pub struct Head"));
/// assert!(out.audit.starts_with("L Head"));
/// ```
///
/// # Errors
///
/// [`Error::Invalid`] when an item fails validation, or a `use` line, derive or `cfg` is not
/// one this crate can write.
/// [`Error::Refused`] when valid items do not work together, such as an endless recursion.
/// [`Error::Internal`] when the generated source does not parse.
pub fn generate(module: &Module) -> Result<Generated, Error> {
    let mut audit = String::new();
    let source = emit_source(module, &mut audit)?;
    Ok(Generated { source, audit })
}

fn emit_source(module: &Module, audit: &mut String) -> Result<String, Error> {
    let root = &module.root;
    let mut tokens = TokenStream::new();

    // `use` lines bypass the pretty-printer, so parse them here.
    for u in &module.uses {
        syn::parse_str::<syn::ItemUse>(&format!("use {u};")).map_err(|_| {
            Error::Invalid(
                String::from("uses"),
                Invalid::Other(format!(
                    "`{u}` is not a valid `use` path. \
                     Write one path per entry, such as `crate::wire::Head`"
                )),
            )
        })?;
    }

    if module.items.iter().any(|i| matches!(i, Item::Message(_) | Item::Choice(_))) {
        tokens.extend(quote! {
            /// The error a message decode returns.
            #[allow(unused_imports)]
            pub use #root::ParseError;
        });
    }
    let allocates = |i: &Item| match i {
        Item::Message(_) | Item::Choice(_) => true,
        Item::Dispatch(d) => d.other_body.is_none(),
        _ => false,
    };
    if module.items.iter().any(allocates) {
        tokens.extend(quote! {
            #[allow(unused_imports)]
            use #root::__private::{Box, String, Vec};
        });
    }

    graph::recursion(module)?;
    let cycles = graph::open_end_cycles(module)?;
    let cyclic = |name: &str| {
        cycles.iter().find(|(n, _)| n == name).map_or(&[][..], |(_, members)| members.as_slice())
    };

    let derives = &module.derives;
    for item in &module.items {
        tokens.extend(match item {
            Item::Enum(e) => emit_enum(e, derives, root)?,
            Item::Layout(l) => {
                let code = emit_layout(l, derives, root)?;
                audit.push_str(&crate::audit::layout(l));
                code
            }
            Item::Dispatch(d) => emit_dispatch(d, derives, root)?,
            Item::Choice(c) => {
                let (code, arms) = emit_choice(c, derives, root, cyclic(&c.name))?;
                audit.push_str(&crate::audit::choice(&c.name, &arms));
                code
            }
            Item::Message(m) => {
                graph::terminal_switches(module, m)?;
                let (code, rows) = emit_message(m, derives, root, cyclic(&m.name))?;
                audit.push_str(&crate::audit::message(&m.name, &m.needs, &rows));
                code
            }
            Item::Verbatim(verbatim) => {
                verbatim::refuse_codec(verbatim)?;
                verbatim.clone()
            }
        });
    }

    let body = crate::source::render(tokens)?;

    let mut out = String::new();
    let _ = writeln!(out, "// @generated by layline-codegen — do not edit.");
    for line in &module.doc {
        let _ = writeln!(out, "//! {line}");
    }
    let _ = writeln!(out);
    for u in &module.uses {
        let _ = writeln!(out, "use {u};");
    }
    if !module.uses.is_empty() {
        let _ = writeln!(out);
    }
    out.push_str(&body);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walk::message_parts;
    use crate::{
        Arm, By, ChoiceDef, Collection, Count, CoverTo, Coverage, Discriminant, Endian, Field,
        Kind, MessageDef, Presence, Scalar, Segment,
    };

    fn emit_module(module: &Module) -> Result<String, Error> {
        generate(module).map(|g| g.source)
    }

    #[test]
    fn a_switch_may_discriminate_on_body_length() {
        let module = Module {
            items: vec![
                Item::Choice(ChoiceDef {
                    name: "VelocityForm".into(),
                    endian: Endian::Le,
                    arms: vec![
                        Arm::new(
                            16,
                            "F32",
                            vec![Segment::Block(vec![
                                Field::new("velocity", Kind::array(Scalar::F32, 3)),
                                Field::new("sigma", Kind::Scalar(Scalar::F32)),
                            ])],
                        ),
                        Arm::new(
                            24,
                            "F64",
                            vec![Segment::Block(vec![Field::new(
                                "velocity",
                                Kind::array(Scalar::F64, 3),
                            )])],
                        ),
                    ],
                    other: None,
                    other_bytes: None,
                    doc: None,
                    derives: Vec::new(),
                }),
                Item::Message(MessageDef {
                    name: "VelocityReport".into(),
                    endian: Endian::Le,
                    bits: None,
                    needs: Vec::new(),
                    segments: vec![Segment::Switch {
                        stated: None,
                        name: "form".into(),
                        on: Discriminant::BodyLength,
                        choice: "VelocityForm".into(),
                        window: None,
                        doc: None,
                    }],
                    doc: None,
                    derives: Vec::new(),
                }),
            ],
            ..Module::new(Vec::new())
        };
        let out = emit_module(&module).expect("specification emits");
        assert!(
            out.contains("((__body.len() - __at) as i64)"),
            "the discriminant is the body the switch can see:\n{out}"
        );
        assert!(out.contains("<VelocityForm as ::layline::Choice>::decode_with_nested"), "{out}");
        assert!(out.contains("fn decode_with_nested("), "{out}");
        assert!(out.contains(r#"field: "VelocityForm""#), "{out}");
    }

    #[test]
    fn a_checksum_may_live_inside_one_arm_of_a_choice() {
        let module = Module {
            items: vec![Item::Choice(ChoiceDef {
                name: "Body".into(),
                endian: Endian::Le,
                arms: vec![
                    Arm::new(
                        0,
                        "Bare",
                        vec![Segment::Block(vec![Field::new("seq", Kind::Scalar(Scalar::U(16)))])],
                    ),
                    Arm::new(
                        1,
                        "Checked",
                        vec![
                            Segment::Block(vec![Field::new("seq", Kind::Scalar(Scalar::U(16)))]),
                            Segment::Checksum {
                                stated: None,
                                name: "crc".into(),
                                repr: Scalar::U(16),
                                algorithm: "::wire::Crc16Ccitt".into(),
                                over: Coverage::whole(),
                                doc: None,
                            },
                        ],
                    ),
                    Arm::new(
                        2,
                        "Excised",
                        vec![
                            Segment::Checksum {
                                stated: None,
                                name: "sum".into(),
                                repr: Scalar::U(16),
                                algorithm: "::wire::Sum16NegLe".into(),
                                over: Coverage::whole().to(CoverTo::After("tail".into())),
                                doc: None,
                            },
                            Segment::Repeat {
                                stated: None,
                                collection: Collection::Vec,
                                name: "tail".into(),
                                doc: None,
                                element: Kind::Scalar(Scalar::U(8)),
                                count: Count::Fill { cap: None },
                                at: None,
                            },
                        ],
                    ),
                ],
                other: None,
                other_bytes: None,
                doc: None,
                derives: Vec::new(),
            })],
            ..Module::new(Vec::new())
        };
        let out = emit_module(&module).expect("specification emits");
        assert!(out.contains("__BodyCk0"), "the arm gets its own one-field layout:\n{out}");
        assert!(
            out.contains("&out.written()[__base..]") && out.contains(".get(0..__ck_to)"),
            "the arm's range starts at the arm's own first byte:\n{out}"
        );
        assert!(out.contains("ParseError::Checksum"), "{out}");

        assert!(
            out.contains("Checksum>::init()") && out.contains("Checksum>::finish("),
            "an excludes_self range is the fold:\n{out}"
        );
        assert!(out.contains("__BodyCk1 { sum: 0 }"), "the reserved bytes:\n{out}");
        assert!(
            out.contains("out.written_mut()[__out_sum..__out_sum + 2]"),
            "…stamped once the range has landed:\n{out}"
        );
        assert!(
            out.contains("const COVERED: &'static [::layline::table::ArmCover<'static>]"),
            "the checksum column, per arm:\n{out}"
        );
        let covered: Vec<&str> =
            out.split("ArmCover::new(").skip(1).filter_map(|s| s.split('"').nth(1)).collect();
        assert_eq!(covered, ["Checked", "Excised"], "an arm with no checksum has no row:\n{out}");
        assert!(out.contains("            true,\n"), "the hole is its own bytes:\n{out}");
    }

    #[test]
    fn a_checksum_refuses_a_range_it_cannot_resolve() {
        let msg = |over: Coverage| MessageDef {
            name: "Packet".into(),
            endian: Endian::Le,
            bits: None,
            needs: Vec::new(),
            segments: vec![
                Segment::Block(vec![Field::new("magic", Kind::Scalar(Scalar::U(16)))]),
                Segment::Checksum {
                    stated: None,
                    name: "crc".into(),
                    repr: Scalar::U(16),
                    algorithm: "::wire::Crc16Ccitt".into(),
                    over,
                    doc: None,
                },
            ],
            doc: None,
            derives: Vec::new(),
        };
        assert!(matches!(
            message_parts(&msg(Coverage::from_field("nope")), &[], &Root::default()),
            Err(Error::Invalid(..)),
        ));
        assert!(message_parts(&msg(Coverage::from_field("magic")), &[], &Root::default()).is_ok());
        assert!(message_parts(&msg(Coverage::whole()), &[], &Root::default()).is_ok());
    }

    #[test]
    fn a_checksum_field_may_not_also_be_a_count() {
        let out = message_parts(
            &MessageDef {
                name: "Packet".into(),
                endian: Endian::Le,
                bits: None,
                needs: Vec::new(),
                segments: vec![
                    Segment::Checksum {
                        stated: None,
                        name: "crc".into(),
                        repr: Scalar::U(16),
                        algorithm: "::wire::Crc16Ccitt".into(),
                        over: Coverage::whole(),
                        doc: None,
                    },
                    Segment::Repeat {
                        stated: None,
                        collection: Collection::Vec,
                        name: "items".into(),
                        element: Kind::Scalar(Scalar::U(8)),
                        count: Count::Field { by: By::field("crc"), cap: None },
                        at: None,
                        doc: None,
                    },
                ],
                doc: None,
                derives: Vec::new(),
            },
            &[],
            &Root::default(),
        );
        let Err(err) = out else {
            panic!("a doubly-derived field must be refused");
        };
        let why = err.to_string();
        assert!(why.contains("crc"), "{why}");
    }

    #[test]
    fn a_bool_presence_flag_governs_exactly_one_field() {
        let opt = |name: &str, kind| Segment::Opt {
            when: Presence::Flag { field: "head.on".into() },
            field: Field::new(name, kind),
        };
        let msg = |extra: Segment| MessageDef {
            name: "Packet".into(),
            endian: Endian::Le,
            bits: None,
            needs: Vec::new(),
            segments: vec![
                Segment::Block(vec![Field::new(
                    "head",
                    Kind::Nested { ty: "Head".into(), bytes: 1 },
                )]),
                opt("a", Kind::Scalar(Scalar::U(8))),
                extra,
            ],
            doc: None,
            derives: Vec::new(),
        };

        let out = message_parts(&msg(opt("b", Kind::Scalar(Scalar::U(8)))), &[], &Root::default());
        let Err(Error::Refused(why)) = out else {
            panic!("two presence writes over one `bool` must be refused");
        };
        assert!(why.contains("is the whole of `a`'s presence flag"), "{why}");

        let out = message_parts(
            &msg(Segment::Repeat {
                stated: None,
                collection: Collection::Vec,
                name: "items".into(),
                element: Kind::Scalar(Scalar::U(8)),
                count: Count::Field { by: By::field("head.on"), cap: None },
                at: None,
                doc: None,
            }),
            &[],
            &Root::default(),
        );
        assert!(matches!(out, Err(Error::Refused(_))), "a count over a presence flag");
    }

    #[test]
    fn a_field_may_not_follow_an_open_ended_switch() {
        let module = Module {
            items: vec![
                Item::Choice(ChoiceDef {
                    name: "Body".into(),
                    endian: Endian::Le,
                    arms: vec![Arm::new(
                        0,
                        "Ping",
                        vec![Segment::Block(vec![Field::new("seq", Kind::Scalar(Scalar::U(16)))])],
                    )],
                    other: Some("Unknown".into()),
                    other_bytes: None,
                    doc: None,
                    derives: Vec::new(),
                }),
                Item::Message(MessageDef {
                    name: "Packet".into(),
                    endian: Endian::Le,
                    bits: None,
                    needs: Vec::new(),
                    segments: vec![
                        Segment::Block(vec![Field::new("kind", Kind::Scalar(Scalar::U(8)))]),
                        Segment::Switch {
                            stated: None,
                            name: "body".into(),
                            on: Discriminant::Field("kind".into()),
                            choice: "Body".into(),
                            window: None,
                            doc: None,
                        },
                        Segment::Block(vec![Field::new("trailer", Kind::Scalar(Scalar::U(8)))]),
                    ],
                    doc: None,
                    derives: Vec::new(),
                }),
            ],
            ..Module::new(Vec::new())
        };
        assert!(matches!(emit_module(&module), Err(Error::Invalid(_, Invalid::AfterOpenEnd(_)))));
    }
}
