//! Wire fields that encode computes instead of taking from the caller.

use crate::{Absence, Count, Coverage, Discriminant, Kind, Len, Presence, Segment};

/// A wire field that encode computes from the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Derived {
    /// `field = (collection.len() - offset) / scale`
    Count {
        /// The collection the field counts.
        of: String,
        scale: u32,
        offset: i32,
        /// The decode limit on elements, if any.
        cap: Option<usize>,
    },
    /// `field = (isqrt(collection.len()) - offset) / scale`
    ///
    /// Encode refuses a length that is not a square.
    Squared {
        /// The square collection whose order the field holds.
        of: String,
        scale: u32,
        offset: i32,
        /// The decode limit on `order * order`, if any.
        cap: Option<usize>,
    },
    /// `field = (encoded byte length of the text field - offset) / scale`
    TextLen {
        /// The text field whose byte length this is.
        of: String,
        /// The text's `TextCodec`, which sets the byte count. `None` means UTF-8.
        codec: Option<String>,
        scale: u32,
        offset: i32,
        /// The decode limit on bytes, if any.
        cap: Option<usize>,
    },
    /// `field = the byte position the segment was written at`
    Offset {
        /// The segment whose position this is.
        of: String,
        /// Whether the segment is a single record. Only error messages use it.
        record: bool,
    },
    /// `field = (one element's wire width - offset) / scale`
    ///
    /// Encode packs elements at `WIRE_BYTES` apart, with no gaps.
    Stride {
        /// The element [`Kind`].
        elem: Kind,
        scale: u32,
        offset: i32,
    },
    /// `field = (the byte length the collection wrote - offset) / scale`
    ///
    /// Reserved, like [`Offset`](Derived::Offset).
    ByteLen {
        /// The segment whose byte length this is.
        of: String,
        /// Whether the segment is a single record. Only error messages use it.
        record: bool,
    },
    /// `field = (the arm's encoded byte length - offset) / scale`
    ///
    /// Measured before the field is written, because a varint field has no fixed size to reserve.
    ArmLen {
        /// The switch whose byte length this is.
        of: String,
        scale: u32,
        offset: i32,
        /// The decode limit on bytes, if any.
        cap: Option<usize>,
    },
    /// `field = the value of the switch's arm`
    ///
    /// The open arm has no value. It keeps its raw bytes instead.
    Discriminant {
        /// The switch whose arm the field selects.
        of: String,
        /// The field the discriminant is written to.
        field: String,
    },
    /// `field = field, with each mask bit set when its optional field is Some`
    Flags {
        /// The flags field. Encode keeps the bits that no optional field uses.
        field: String,
        /// `(mask, optional field name)`, in declaration order.
        bits: Vec<(u64, String)>,
    },
    /// `field = whether its optional field is Some`
    Presence {
        /// The optional field the flag controls.
        of: String,
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
            Derived::Squared { .. } => "order of a square collection",
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

/// Every computed field in the segments, as `(field, derivation)`.
pub(crate) fn derivations(segments: &[Segment]) -> Vec<(String, Derived)> {
    let mut out = Vec::new();
    for seg in segments {
        match seg {
            Segment::Repeat { name: coll, element, count, at, .. } => {
                match count {
                    Count::Field { by, cap } | Count::Strided { by, cap, .. } => {
                        out.push((
                            by.field.clone(),
                            Derived::Count {
                                of: coll.clone(),
                                scale: by.scale,
                                offset: by.offset,
                                cap: *cap,
                            },
                        ));
                    }
                    Count::Squared { by, cap } => {
                        out.push((
                            by.field.clone(),
                            Derived::Squared {
                                of: coll.clone(),
                                scale: by.scale,
                                offset: by.offset,
                                cap: *cap,
                            },
                        ));
                    }
                    Count::Window(Len::Field { by, .. }) => {
                        out.push((
                            by.field.clone(),
                            Derived::ByteLen { of: coll.clone(), record: false },
                        ));
                    }
                    _ => {}
                }
                if let Count::Strided { stride, .. } = count {
                    out.push((
                        stride.field.clone(),
                        Derived::Stride {
                            elem: element.clone(),
                            scale: stride.scale,
                            offset: stride.offset,
                        },
                    ));
                }
                if let Some(field) = at {
                    out.push((field.clone(), Derived::Offset { of: coll.clone(), record: false }));
                }
            }
            Segment::Placed { name, at, absent, .. } => {
                out.push((at.clone(), Derived::Offset { of: name.clone(), record: true }));
                if let Some(Absence::ZeroLength { by }) = absent {
                    out.push((by.clone(), Derived::ByteLen { of: name.clone(), record: true }));
                }
            }
            Segment::Value {
                name: text,
                kind: Kind::Text { len: Len::Field { by, cap }, codec },
                ..
            } => {
                out.push((
                    by.field.clone(),
                    Derived::TextLen {
                        of: text.clone(),
                        codec: codec.clone(),
                        scale: by.scale,
                        offset: by.offset,
                        cap: *cap,
                    },
                ));
            }
            Segment::Switch { name: body, on, window, .. } => {
                if let Discriminant::Field(field) = on {
                    out.push((
                        field.clone(),
                        Derived::Discriminant { of: body.clone(), field: field.clone() },
                    ));
                }
                if let Some(Len::Field { by, cap }) = window {
                    out.push((
                        by.field.clone(),
                        Derived::ArmLen {
                            of: body.clone(),
                            scale: by.scale,
                            offset: by.offset,
                            cap: *cap,
                        },
                    ));
                }
            }
            Segment::Opt { when: Presence::Mask { field: flag, mask }, field } => {
                let bit = (*mask, field.name.clone());
                match out
                    .iter_mut()
                    .find(|(df, d)| df == flag && matches!(d, Derived::Flags { .. }))
                {
                    Some((_, Derived::Flags { bits, .. })) => bits.push(bit),
                    _ => out.push((
                        flag.clone(),
                        Derived::Flags { field: flag.clone(), bits: vec![bit] },
                    )),
                }
            }
            Segment::Opt { when: Presence::Flag { field: flag }, field } => {
                out.push((flag.clone(), Derived::Presence { of: field.name.clone() }));
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
