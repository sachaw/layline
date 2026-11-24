//! Wire fields that encode computes instead of taking from the caller.
//!
//! An attribute's expression derives a field when it is invertible in the one field it reads:
//! encode works the number back to that field. Any other expression derives nothing, and encode
//! checks it instead.

use crate::model::{Ty, split_ref};
use crate::{Absence, Count, Coverage, Discriminant, Expr, Kind, Len, Presence, Segment};

/// A wire field that encode computes from the value.
///
/// `by` is the attribute's expression, which encode works back from the number to the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Derived {
    /// `by = collection.len()`
    Count {
        /// The collection the field counts.
        of: String,
        by: Expr,
        /// The decode limit on elements, if any.
        cap: Option<usize>,
    },
    /// `by = encoded byte length of the text field`
    TextLen {
        /// The text field whose byte length this is.
        of: String,
        /// The text's `TextCodec`, which sets the byte count. `None` means UTF-8.
        codec: Option<String>,
        by: Expr,
        /// The decode limit on bytes, if any.
        cap: Option<usize>,
    },
    /// `by = the byte position the segment was written at`
    Offset {
        /// The segment whose position this is.
        of: String,
        /// Whether the segment is a single record. Only error messages use it.
        record: bool,
        by: Expr,
    },
    /// `by = one element's wire width`
    ///
    /// Encode packs elements at `WIRE_BYTES` apart, with no gaps.
    Stride {
        /// The element [`Kind`].
        elem: Kind,
        by: Expr,
    },
    /// `by = the byte length the collection wrote`
    ///
    /// Reserved, like [`Offset`](Derived::Offset).
    ByteLen {
        /// The segment whose byte length this is.
        of: String,
        /// Whether the segment is a single record. Only error messages use it.
        record: bool,
        by: Expr,
        /// The decode limit on bytes, if any.
        cap: Option<usize>,
    },
    /// `by = the arm's encoded byte length`
    ///
    /// Measured before the field is written, because a varint field has no fixed size to reserve.
    ArmLen {
        /// The switch whose byte length this is.
        of: String,
        by: Expr,
        /// The decode limit on bytes, if any.
        cap: Option<usize>,
    },
    /// `by = the value of the switch's arm`
    ///
    /// The open arm has no value. It keeps the field's own value instead.
    Discriminant {
        /// The switch whose arm the field selects.
        of: String,
        /// The field the discriminant is written to.
        field: String,
        by: Expr,
    },
    /// `field = field, with each mask's bits set when its optional field is Some`, or for a mask
    /// that marks absence, when it is None
    Flags {
        /// The flags field. Encode keeps the bits that no optional field uses.
        field: String,
        /// `(mask, optional field name, whether set bits mean present)`, in declaration order.
        /// A mask is a literal or a constant.
        bits: Vec<(Expr, String, bool)>,
    },
    /// `field = whether its optional field is Some`, or with `set` false, None
    Presence {
        /// The optional field the flag controls.
        of: String,
        /// Whether `true` means present.
        set: bool,
    },
    /// `field = the algorithm's check value over the bytes it covers`
    ///
    /// Reserved when the covered range extends past the field.
    Checksum {
        /// Rust path of the `layline::Checksum` type.
        algorithm: String,
        over: Coverage,
    },
}

impl Derived {
    /// The noun phrase an error message uses for this derivation.
    pub(crate) fn what(&self) -> &'static str {
        match self {
            Derived::Count { .. } => "length of a collection",
            Derived::TextLen { .. } => "encoded length of a string",
            Derived::Offset { record: false, .. } => "position of a collection",
            Derived::Offset { record: true, .. } => "position of a record",
            Derived::ByteLen { record: false, .. } => "byte length of a collection",
            Derived::ByteLen { record: true, .. } => "byte length of a record",
            Derived::Stride { .. } => "wire width of one element",
            Derived::ArmLen { .. } => "byte length of a switch",
            Derived::Discriminant { .. } => "discriminant of a switch",
            Derived::Flags { .. } => "presence bits of optional fields",
            Derived::Presence { .. } => "presence of an optional field",
            Derived::Checksum { .. } => "check value over the bytes it covers",
        }
    }

    /// The fix to suggest, for derivations that have one.
    pub(crate) fn instead(&self) -> Option<&'static str> {
        match self {
            Derived::Flags { .. } => Some("Give the presence bits a word of their own"),
            Derived::Presence { .. } => {
                Some("Write a flags word with a mask per field (`#[when(w & 0x01)]`)")
            }
            Derived::Checksum { .. } => Some("Give the checksum a field of its own"),
            Derived::Stride { .. } => Some("Give the stride a field of its own"),
            _ => None,
        }
    }

    /// The slot this derivation reserves, and the segment it describes.
    ///
    /// Encode writes zeros to a reserved field, then fills in the number after the segment.
    pub(crate) fn reserves(&self) -> Option<(SlotOf, &str)> {
        match self {
            Derived::Offset { of, .. } => Some((SlotOf::Position, of)),
            Derived::ByteLen { of, .. } => Some((SlotOf::Length, of)),
            _ => None,
        }
    }
}

/// What a reserved field holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotOf {
    /// The segment's byte position.
    Position,
    /// The segment's byte length.
    Length,
}

/// Whether `name` is a field of `segments`, or of a nested layout in one, and not a parameter.
pub(crate) fn declares(segments: &[Segment], name: &str) -> bool {
    let (outer, _) = split_ref(name);
    segments.iter().any(|seg| match seg {
        Segment::Block(fields) => fields.iter().any(|f| f.name == outer),
        Segment::Opt { field, .. } => field.name == outer,
        other => other.name() == Some(outer),
    })
}

/// Every computed field in the segments, as `(field, derivation)`.
pub(crate) fn derivations(segments: &[Segment]) -> Vec<(String, Derived)> {
    let is_field = |name: &str| declares(segments, name);
    let mut out: Vec<(String, Derived)> = Vec::new();
    let derive =
        |out: &mut Vec<(String, Derived)>, by: &Expr, d: &dyn Fn(&str, Expr) -> Derived| {
            if let Some(inv) = by.inverse(&is_field) {
                out.push((String::from(inv.field), d(inv.field, by.clone())));
            }
        };
    for seg in segments {
        match seg {
            Segment::Repeat { name: coll, element, count, at, .. } => {
                match count {
                    Count::Field { by, cap } | Count::Strided { by, cap, .. } => {
                        derive(&mut out, by, &|_, by| Derived::Count {
                            of: coll.clone(),
                            by,
                            cap: *cap,
                        });
                    }
                    Count::Window(Len::Field { by, cap, .. }) => {
                        derive(&mut out, by, &|_, by| Derived::ByteLen {
                            of: coll.clone(),
                            record: false,
                            by,
                            cap: *cap,
                        });
                    }
                    _ => {}
                }
                if let Count::Strided { stride, .. } = count {
                    derive(&mut out, stride, &|_, by| Derived::Stride {
                        elem: element.clone(),
                        by,
                    });
                }
                if let Some(at) = at {
                    derive(&mut out, at, &|_, by| Derived::Offset {
                        of: coll.clone(),
                        record: false,
                        by,
                    });
                }
            }
            Segment::Placed { name, at, absent, .. } => {
                derive(&mut out, at, &|_, by| Derived::Offset {
                    of: name.clone(),
                    record: true,
                    by,
                });
                if let Some(Absence::ZeroLength { by, .. }) = absent {
                    derive(&mut out, &Expr::name(by), &|_, by| Derived::ByteLen {
                        of: name.clone(),
                        record: true,
                        by,
                        cap: None,
                    });
                }
            }
            Segment::Value {
                name: text,
                kind: Kind::Text { len: Len::Field { by, cap, .. }, codec },
                ..
            } => {
                derive(&mut out, by, &|_, by| Derived::TextLen {
                    of: text.clone(),
                    codec: codec.clone(),
                    by,
                    cap: *cap,
                });
            }
            Segment::Switch { name: body, on, window, .. } => {
                if let Discriminant::Field(on) = on {
                    derive(&mut out, on, &|field, by| Derived::Discriminant {
                        of: body.clone(),
                        field: String::from(field),
                        by,
                    });
                }
                if let Some(Len::Field { by, cap, .. }) = window {
                    derive(&mut out, by, &|_, by| Derived::ArmLen {
                        of: body.clone(),
                        by,
                        cap: *cap,
                    });
                }
            }
            Segment::Opt { when: Presence::When(e), field } => {
                let Some(t) = e.test().filter(|t| is_field(t.field)) else { continue };
                let flag = String::from(t.field);
                match t.mask {
                    Some(mask) => {
                        let bit = (mask.clone(), field.name.clone(), t.set);
                        match out
                            .iter_mut()
                            .find(|(df, d)| *df == flag && matches!(d, Derived::Flags { .. }))
                        {
                            Some((_, Derived::Flags { bits, .. })) => bits.push(bit),
                            _ => out.push((
                                flag.clone(),
                                Derived::Flags { field: flag, bits: vec![bit] },
                            )),
                        }
                    }
                    None => {
                        out.push((flag, Derived::Presence { of: field.name.clone(), set: t.set }))
                    }
                }
            }
            Segment::Checksum { name, algorithm, over, .. } => {
                out.push((
                    name.clone(),
                    Derived::Checksum { algorithm: algorithm.clone(), over: over.clone() },
                ));
            }
            _ => {}
        }
    }
    out
}

/// Reserved fields filled in after a later segment, as `(field, segment)`.
pub(crate) fn back_patched(segments: &[Segment]) -> Vec<(String, String)> {
    derivations(segments)
        .into_iter()
        .filter_map(|(field, d)| d.reserves().map(|(_, of)| (field, String::from(of))))
        .collect()
}

/// Every expression in the segments, and whether it gives a number or a condition.
pub(crate) fn expressions(segments: &[Segment]) -> Vec<(Expr, Ty)> {
    let mut out: Vec<(Expr, Ty)> = Vec::new();
    let mut conditions: Vec<(Expr, Ty)> = Vec::new();
    let mut number = |e: &Expr| out.push((e.clone(), Ty::Int));
    for seg in segments {
        match seg {
            Segment::Repeat { count, at, .. } => {
                match count {
                    Count::Field { by, .. } | Count::Window(Len::Field { by, .. }) => number(by),
                    Count::Strided { by, stride, .. } => {
                        number(by);
                        number(stride);
                    }
                    _ => {}
                }
                at.iter().for_each(&mut number);
            }
            Segment::Placed { at, absent, .. } => {
                number(at);
                if let Some(Absence::ZeroLength { by, .. }) = absent {
                    number(&Expr::name(by));
                }
            }
            Segment::Value { kind: Kind::Text { len: Len::Field { by, .. }, .. }, .. } => {
                number(by)
            }
            Segment::Switch { on, window, .. } => {
                if let Discriminant::Field(on) = on {
                    number(on);
                }
                if let Some(Len::Field { by, .. }) = window {
                    number(by);
                }
            }
            Segment::Opt { when: Presence::When(e), .. } | Segment::Valid { holds: e, .. } => {
                conditions.push((e.clone(), Ty::Bool));
            }
            _ => {}
        }
    }
    out.extend(conditions);
    out
}
