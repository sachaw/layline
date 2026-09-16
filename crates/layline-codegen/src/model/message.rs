//! Variable-length messages.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::Derive;

use super::{BitOrder, By, Coverage, Discriminant, Endian, Field, Kind, Len, Scalar, Stated};

/// How many elements a collection has.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Count {
    /// Read from an earlier integer field: `count = field * scale + offset`.
    Field {
        /// The count field.
        by: By,
        /// The largest count allowed.
        cap: Option<usize>,
    },
    /// The square of an earlier field's value. Encode derives the field from the length.
    Squared {
        /// The field holding the square root.
        by: By,
        /// The largest count allowed, after squaring.
        cap: Option<usize>,
    },
    /// A count from one field and each element's byte size from another.
    ///
    /// Decode reads each element from the start of its stride. A stride shorter than the element
    /// is [`ParseError::Malformed`](layline_core::ParseError).
    Strided {
        /// The count field.
        by: By,
        /// The largest count allowed.
        cap: Option<usize>,
        /// The field holding each element's size in bytes.
        stride: By,
    },
    /// As many elements as fit a byte length.
    ///
    /// The elements must fill it exactly, or decode returns
    /// [`ParseError::Malformed`](layline_core::ParseError). With [`Len::Until`], one-byte
    /// elements run up to a terminator, which is not included. [`Len::Fill`] is not allowed:
    /// use [`Count::Fill`].
    Window(Len),
    /// Elements up to and including the first with a `mask` bit set, as in HDLC addresses.
    ///
    /// The element type declares that bit as a `prefix`, and encode sets it. Fields can follow.
    Terminated {
        /// The bits that mark the last element.
        mask: u8,
    },
    /// As many elements as fill the rest of the body exactly.
    Fill {
        /// The largest count allowed.
        cap: Option<usize>,
    },
}

/// When an optional field is present.
///
/// Encode sets the presence from whether the field is `Some`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Presence {
    /// When `field & mask != 0`.
    Mask {
        /// The flags field.
        field: String,
        /// The bits that mark the field present.
        mask: u64,
    },
    /// When the `bool` `field` is `true`.
    Flag {
        /// The `bool` field.
        field: String,
    },
    /// When an unnamed bit just before the field is set. Bit-addressed messages only.
    Bit,
    /// When the body has bytes left. Must be the last field.
    Remaining,
}

/// What makes an optional [`Placed`](Segment::Placed) record absent.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Absence {
    /// An offset of zero, in the field [`Segment::Placed::at`].
    ZeroOffset,
    /// A length of zero.
    ZeroLength {
        /// The earlier field holding the length in bytes.
        by: String,
    },
}

/// The Rust type of a collection. The wire format is the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Collection {
    /// `Vec<T>`.
    #[default]
    Vec,
    /// `Box<[T]>`.
    Boxed,
}

/// One part of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Segment {
    /// Fixed-width fields.
    Block(Vec<Field>),
    /// A [`Kind::Var`], [`Kind::Msg`] or [`Kind::Text`] value, whose size is read from the wire.
    Value {
        /// The field name.
        name: String,
        /// Documentation.
        doc: Option<String>,
        /// The `#[at]` position. Only allowed before any variable-length segment.
        stated: Option<Stated>,
        /// The type.
        kind: Kind,
    },
    /// A collection of elements.
    Repeat {
        /// The field name.
        name: String,
        /// Documentation.
        doc: Option<String>,
        /// The `#[at]` position. Only allowed before any variable-length segment.
        stated: Option<Stated>,
        /// The element type.
        element: Kind,
        /// How many elements there are.
        count: Count,
        /// The earlier field holding the byte offset of the elements. Encode writes it.
        at: Option<String>,
        /// The Rust type.
        collection: Collection,
    },
    /// One record at a byte offset read from an earlier field.
    ///
    /// The offset counts from the start of the body, which inside a [`Kind::Msg`] is that message.
    Placed {
        /// The field name.
        name: String,
        /// Documentation.
        doc: Option<String>,
        /// The `#[at]` position. Only allowed before any variable-length segment.
        stated: Option<Stated>,
        /// The record: a [`Kind::Nested`] or a [`Kind::Msg`].
        kind: Kind,
        /// The earlier field holding the offset. Encode writes it.
        at: String,
        /// What makes the record absent, for an `Option<T>` field.
        absent: Option<Absence>,
    },
    /// A field whose arm an earlier value selects.
    ///
    /// The arms and their values are in a [`ChoiceDef`](crate::ChoiceDef). Nothing can follow
    /// a switch that has no [`window`](Segment::Switch::window) and whose choice is
    /// [open-ended](crate::ChoiceDef::open_ended).
    Switch {
        /// The field name.
        name: String,
        /// Documentation.
        doc: Option<String>,
        /// The `#[at]` position. Only allowed before any variable-length segment.
        stated: Option<Stated>,
        /// What selects the arm.
        on: Discriminant,
        /// The [`ChoiceDef`](crate::ChoiceDef) type.
        choice: String,
        /// A fixed size for the switch, independent of the arm.
        ///
        /// `None` takes the arm's size. [`Len::Bytes`] makes a union, checked by
        /// [`arms_fit`](layline_core::table::arms_fit). [`Len::Field`] reads the size from an
        /// earlier field, and encode writes it. With a window, fields can follow the switch.
        window: Option<Len>,
    },
    /// An optional field.
    ///
    /// Encode sets the masked bits and leaves the rest of the flags field alone. Other fields
    /// cannot refer to an optional field.
    Opt {
        /// When the field is present.
        when: Presence,
        /// The field.
        field: Field,
    },
    /// A checksum over bytes of the same body.
    ///
    /// Encode computes it once the covered bytes are written. Decode verifies it and returns an
    /// error on mismatch.
    Checksum {
        /// The field name.
        name: String,
        /// Documentation.
        doc: Option<String>,
        /// The `#[at]` position. Only allowed before any variable-length segment.
        stated: Option<Stated>,
        /// The integer type.
        repr: Scalar,
        /// The path of the `layline::Checksum` algorithm.
        algorithm: String,
        /// The bytes it covers.
        over: Coverage,
    },
}

impl Segment {
    /// A [`Segment::Block`].
    #[must_use]
    pub fn block(fields: Vec<Field>) -> Self {
        Segment::Block(fields)
    }

    /// A [`Segment::Value`] with no documentation or `#[at]`.
    #[must_use]
    pub fn value(name: &str, kind: Kind) -> Self {
        Segment::Value { name: String::from(name), doc: None, stated: None, kind }
    }

    /// A [`Segment::Repeat`].
    ///
    /// `at` names the offset field, or is `None` to read the elements in place.
    #[must_use]
    pub fn repeat(
        name: &str,
        element: Kind,
        count: Count,
        at: Option<&str>,
        collection: Collection,
    ) -> Self {
        Segment::Repeat {
            name: String::from(name),
            doc: None,
            stated: None,
            element,
            count,
            at: at.map(String::from),
            collection,
        }
    }

    /// A [`Segment::Placed`] at the offset in field `at`. Optional when `absent` is set.
    #[must_use]
    pub fn placed(name: &str, kind: Kind, at: &str, absent: Option<Absence>) -> Self {
        Segment::Placed {
            name: String::from(name),
            doc: None,
            stated: None,
            kind,
            at: String::from(at),
            absent,
        }
    }

    /// A [`Segment::Switch`] on `on`, with arms from the choice type `choice`.
    #[must_use]
    pub fn switch(name: &str, on: Discriminant, choice: &str, window: Option<Len>) -> Self {
        Segment::Switch {
            name: String::from(name),
            doc: None,
            stated: None,
            on,
            choice: String::from(choice),
            window,
        }
    }

    /// A [`Segment::Opt`].
    #[must_use]
    pub fn opt(when: Presence, field: Field) -> Self {
        Segment::Opt { when, field }
    }

    /// A [`Segment::Checksum`].
    #[must_use]
    pub fn checksum(name: &str, repr: Scalar, algorithm: &str, over: Coverage) -> Self {
        Segment::Checksum {
            name: String::from(name),
            doc: None,
            stated: None,
            repr,
            algorithm: String::from(algorithm),
            over,
        }
    }

    /// Sets the documentation. Does nothing on a [`Block`](Segment::Block).
    #[must_use]
    pub fn with_doc(mut self, text: &str) -> Self {
        match &mut self {
            Segment::Block(_) => {}
            Segment::Opt { field, .. } => field.doc = Some(String::from(text)),
            Segment::Value { doc, .. }
            | Segment::Repeat { doc, .. }
            | Segment::Placed { doc, .. }
            | Segment::Switch { doc, .. }
            | Segment::Checksum { doc, .. } => *doc = Some(String::from(text)),
        }
        self
    }

    /// Sets the `#[at]` position. Does nothing on a [`Block`](Segment::Block).
    #[must_use]
    pub fn with_stated(mut self, at: Stated) -> Self {
        match &mut self {
            Segment::Block(_) => {}
            Segment::Opt { field, .. } => field.stated = Some(at),
            Segment::Value { stated, .. }
            | Segment::Repeat { stated, .. }
            | Segment::Placed { stated, .. }
            | Segment::Switch { stated, .. }
            | Segment::Checksum { stated, .. } => *stated = Some(at),
        }
        self
    }

    /// The names of the fields this segment always writes, in wire order.
    ///
    /// Optional fields are not included.
    #[must_use]
    pub fn places(&self) -> Vec<&str> {
        match self {
            Segment::Block(fields) => fields.iter().map(|f| f.name.as_str()).collect(),
            Segment::Value { name, .. }
            | Segment::Repeat { name, .. }
            | Segment::Switch { name, .. }
            | Segment::Checksum { name, .. } => vec![name.as_str()],
            Segment::Placed { name, absent: None, .. } => vec![name.as_str()],
            Segment::Placed { absent: Some(_), .. } => Vec::new(),
            Segment::Opt { .. } => Vec::new(),
        }
    }

    /// The name of the field, if it is optional.
    #[must_use]
    pub fn placed_optionally(&self) -> Option<&str> {
        match self {
            Segment::Opt { field, .. } => Some(field.name.as_str()),
            Segment::Placed { name, absent: Some(_), .. } => Some(name.as_str()),
            _ => None,
        }
    }

    /// The field name, or `None` for a [`Block`](Segment::Block).
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        match self {
            Segment::Block(_) => None,
            Segment::Opt { field, .. } => Some(&field.name),
            Segment::Value { name, .. }
            | Segment::Repeat { name, .. }
            | Segment::Placed { name, .. }
            | Segment::Switch { name, .. }
            | Segment::Checksum { name, .. } => Some(name),
        }
    }

    /// The `#[at]` position, if any.
    #[must_use]
    pub fn stated(&self) -> Option<Stated> {
        match self {
            Segment::Block(_) => None,
            Segment::Opt { field, .. } => field.stated,
            Segment::Value { stated, .. }
            | Segment::Repeat { stated, .. }
            | Segment::Placed { stated, .. }
            | Segment::Switch { stated, .. }
            | Segment::Checksum { stated, .. } => *stated,
        }
    }

    /// The size in bits, or `None` if it depends on the wire.
    #[must_use]
    pub fn fixed_bits(&self) -> Option<u64> {
        match self {
            Segment::Block(fields) => fields.iter().map(|f| f.kind.bits()).sum(),
            Segment::Checksum { repr, .. } => Some(repr.bits()),
            Segment::Value { kind, .. } => kind.bits(),
            Segment::Repeat { count, at, .. } => match (count, at) {
                (Count::Window(Len::Bytes(n)), None) => Some(*n as u64 * 8),
                _ => None,
            },
            Segment::Switch { window: Some(Len::Bytes(n)), .. } => Some(*n as u64 * 8),
            Segment::Switch { .. } | Segment::Placed { .. } | Segment::Opt { .. } => None,
        }
    }

    /// Whether this segment reads to the end of the body.
    ///
    /// Always `false` for a [`Switch`](Segment::Switch) or [`Kind::Msg`]. Generated code checks
    /// those against the other type.
    #[must_use]
    pub fn open_ended(&self) -> bool {
        match self {
            Segment::Repeat { count, .. } => matches!(count, Count::Fill { .. }),
            Segment::Value { kind, .. } | Segment::Opt { field: Field { kind, .. }, .. } => {
                matches!(kind, Kind::Text { len: Len::Fill, .. })
            }
            Segment::Block(_)
            | Segment::Checksum { .. }
            | Segment::Switch { .. }
            | Segment::Placed { .. } => false,
        }
    }
}

/// A read-only integer the enclosing message passes in.
///
/// Fields can refer to it like an earlier field. Encode never writes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Param {
    /// The name fields refer to.
    pub name: String,
    /// The integer type.
    pub repr: Scalar,
}

impl Param {
    /// A parameter.
    #[must_use]
    pub fn new(name: &str, repr: Scalar) -> Self {
        Self { name: String::from(name), repr }
    }
}

/// A variable-length message.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MessageDef {
    /// The type name.
    pub name: String,
    /// The byte order.
    pub endian: Endian,
    /// The bit order, or `None` for a byte-addressed message.
    ///
    /// A bit-addressed message holds bit fields and inline [presence bits](Presence::Bit),
    /// padded with zeros to a whole byte.
    pub bits: Option<BitOrder>,
    /// The parameters, which generate a `<Name>Ctx` struct used as `Message::Ctx`.
    pub needs: Vec<Param>,
    /// The segments, in wire order.
    pub segments: Vec<Segment>,
    /// Documentation.
    pub doc: Option<String>,
    /// Derives for this message, written after the module's.
    pub derives: Vec<Derive>,
}

impl MessageDef {
    /// A little-endian, byte-addressed message.
    #[must_use]
    pub fn new(name: &str, segments: Vec<Segment>) -> Self {
        Self {
            name: String::from(name),
            endian: Endian::Le,
            bits: None,
            needs: Vec::new(),
            segments,
            doc: None,
            derives: Vec::new(),
        }
    }

    /// Sets the documentation.
    #[must_use]
    pub fn with_doc(self, doc: &str) -> Self {
        Self { doc: Some(String::from(doc)), ..self }
    }

    /// Sets the derives written on this message, after the module's.
    #[must_use]
    pub fn with_derives(self, derives: Vec<Derive>) -> Self {
        Self { derives, ..self }
    }

    /// Sets the parameters.
    #[must_use]
    pub fn with_needs(self, needs: Vec<Param>) -> Self {
        Self { needs, ..self }
    }

    /// Sets the byte order.
    #[must_use]
    pub fn with_endian(self, endian: Endian) -> Self {
        Self { endian, ..self }
    }

    /// Makes the message bit-addressed, in this bit order.
    #[must_use]
    pub fn with_bits(self, order: BitOrder) -> Self {
        Self { bits: Some(order), ..self }
    }
}

/// Splits a reference at its first dot: `head.n` is `("head", Some("n"))`.
pub fn split_ref(name: &str) -> (&str, Option<&str>) {
    match name.split_once('.') {
        Some((outer, inner)) => (outer, Some(inner)),
        None => (name, None),
    }
}
