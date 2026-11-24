//! Variable-length messages.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::Derive;

use super::{BitOrder, Coverage, Discriminant, Endian, Expr, Field, Kind, Len, Position, Scalar};

/// How many elements a collection has.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Count {
    /// Given by an [`Expr`] over earlier fields and parameters, such as `n`, `n * 2 - 1` or
    /// `order * order`.
    ///
    /// If the expression is invertible in the one field it reads, encode writes that field from
    /// the collection's length. Otherwise encode checks the length against it.
    Field {
        /// The number of elements.
        by: Expr,
        /// The largest count allowed.
        cap: Option<usize>,
    },
    /// An integer immediately before the elements: adjacent framing, which is no field.
    ///
    /// Decode reads it and refuses a count past `cap`. Encode writes the collection's length,
    /// refusing one the integer cannot hold with `EncodeError::Unfit` and one past `cap` with
    /// `EncodeError::OverCap`. The message's `SEGMENTS` give it a row of its own, named
    /// `<field>.count`. A byte length before the elements is [`Count::Window`] of
    /// [`Len::Prefix`].
    Prefix {
        /// The integer: [`Scalar::U`] or [`Scalar::I`] of 8, 16, 32 or 64 bits, in the message's
        /// byte order.
        repr: Scalar,
        /// The largest count allowed.
        cap: Option<usize>,
    },
    /// A count, and each element's byte size from a second expression.
    ///
    /// Decode reads each element from the start of its stride. A stride shorter than the element
    /// is [`ParseError::Malformed`](layline_core::ParseError).
    Strided {
        /// The number of elements.
        by: Expr,
        /// The largest count allowed.
        cap: Option<usize>,
        /// Each element's size in bytes.
        stride: Expr,
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Presence {
    /// When a condition over earlier fields and parameters holds.
    ///
    /// A test of one field, `f` for a `bool` or `f & MASK` for bits, with `!` or `== 0` for
    /// their absence, is one encode sets from whether the field is `Some`: it sets the masked
    /// bits and leaves the rest of the flags field alone. Encode checks any other condition
    /// against whether the field is `Some`.
    When(Expr),
    /// When a flag just before the field is set: one bit in a bit-addressed message, and in a
    /// byte-addressed one a byte that decode reads as 0 or 1, refusing any other with
    /// [`ParseError::Malformed`](layline_core::ParseError). Encode writes it from whether the
    /// field is `Some`. The flag is adjacent framing, which is no field: a byte-addressed
    /// message's `SEGMENTS` give it a row of its own, named `<field>.present`.
    Prefix,
    /// When the body has bytes left. Must be the last field.
    Remaining,
}

impl Presence {
    /// `When`, from an expression.
    #[must_use]
    pub fn when(condition: Expr) -> Self {
        Presence::When(condition)
    }

    /// For a test of one earlier field: its name, the bits tested, and whether set bits mean
    /// present. A `bool` is its single bit. A mask must be a literal.
    pub(crate) fn tests(&self) -> Option<(&str, u64, bool)> {
        let Presence::When(e) = self else { return None };
        let t = e.test()?;
        let mask = match t.mask {
            None => 1,
            Some(m) => m.fold()? as u64,
        };
        Some((t.field, mask, t.set))
    }

    /// Whether the test reads a `bool`, rather than bits of an integer.
    pub(crate) fn reads_flag(&self) -> bool {
        matches!(self, Presence::When(e) if e.test().is_some_and(|t| t.mask.is_none()))
    }
}

/// What makes an optional [`Placed`](Segment::Placed) record absent.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Absence {
    /// An offset of zero: [`Segment::Placed::at`] gives 0.
    ZeroOffset,
    /// A length of zero.
    ZeroLength {
        /// The earlier field holding the length in bytes.
        by: String,
        /// Padding that is not part of the length: `#[align(N)]`.
        ///
        /// The record lies in the stated length rounded up to a multiple of `N` bytes. It may end
        /// anywhere from the stated length to that bound, since a record that states its own size
        /// can count the padding, as an SDAT block does. Encode writes the record's length and
        /// then zeros up to the bound.
        align: Option<u64>,
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
    /// `&'a [u8]`, borrowed from the bytes the message decodes from. Needs a
    /// [`lifetime`](MessageDef::lifetime), and `u8` elements counted by a field, a byte length or
    /// the rest of the body.
    Borrowed,
    /// `&'a [u8; N]`, borrowed as [`Borrowed`](Self::Borrowed) is. Needs
    /// [`Count::Window`] of [`Len::Bytes`]`(N)`.
    BorrowedArray,
}

impl Collection {
    /// Whether the collection borrows from the input.
    #[must_use]
    pub fn borrows(self) -> bool {
        matches!(self, Collection::Borrowed | Collection::BorrowedArray)
    }
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
        position: Option<Position>,
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
        position: Option<Position>,
        /// The element type.
        element: Kind,
        /// How many elements there are.
        count: Count,
        /// The byte offset of the elements. Encode writes the field it reads, if it is invertible.
        at: Option<Expr>,
        /// The Rust type.
        collection: Collection,
        /// When an `Option` of the collection is present, or `None` for a collection that always
        /// is.
        ///
        /// An optional collection states its own size: a [`Count::Prefix`], or a
        /// [`Count::Window`] of [`Len::Prefix`], [`Len::Bytes`] or [`Len::Until`], or a
        /// [`Count::Terminated`] run. It has no offset.
        when: Option<Presence>,
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
        position: Option<Position>,
        /// The record: a [`Kind::Nested`] or a [`Kind::Msg`].
        kind: Kind,
        /// The byte offset. Encode writes the field it reads, if it is invertible.
        at: Expr,
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
        position: Option<Position>,
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
        position: Option<Position>,
        /// The integer type.
        repr: Scalar,
        /// The path of the `layline::Checksum` algorithm.
        algorithm: String,
        /// The bytes it covers.
        over: Coverage,
    },
    /// A condition the value must satisfy, over earlier fields and parameters: an [`Expr`] that
    /// encode cannot write back, so it checks it.
    ///
    /// It takes no bytes. Decode refuses a body that breaks it with
    /// [`ParseError::Malformed`](layline_core::ParseError), and encode refuses such a value with
    /// [`EncodeError::Mismatch`](layline_core::EncodeError), both naming `field`.
    Valid {
        /// The field that declares the condition, as errors name it.
        field: String,
        /// The condition.
        holds: Expr,
    },
}

impl Segment {
    /// A [`Segment::Valid`].
    #[must_use]
    pub fn valid(field: &str, holds: Expr) -> Self {
        Segment::Valid { field: String::from(field), holds }
    }

    /// A [`Segment::Block`].
    #[must_use]
    pub fn block(fields: Vec<Field>) -> Self {
        Segment::Block(fields)
    }

    /// A [`Segment::Value`] with no documentation or `#[at]`.
    #[must_use]
    pub fn value(name: &str, kind: Kind) -> Self {
        Segment::Value { name: String::from(name), doc: None, position: None, kind }
    }

    /// A [`Segment::Repeat`].
    ///
    /// `at` gives the byte offset, or is `None` to read the elements in place.
    #[must_use]
    pub fn repeat(
        name: &str,
        element: Kind,
        count: Count,
        at: Option<Expr>,
        collection: Collection,
    ) -> Self {
        Segment::Repeat {
            name: String::from(name),
            doc: None,
            position: None,
            element,
            count,
            at,
            collection,
            when: None,
        }
    }

    /// Makes a [`Segment::Repeat`] an `Option` of its collection, present `when`. Does nothing
    /// on any other segment.
    #[must_use]
    pub fn with_presence(mut self, presence: Presence) -> Self {
        if let Segment::Repeat { when, .. } = &mut self {
            *when = Some(presence);
        }
        self
    }

    /// A [`Segment::Placed`] at the byte offset `at`. Optional when `absent` is set.
    #[must_use]
    pub fn placed(name: &str, kind: Kind, at: Expr, absent: Option<Absence>) -> Self {
        Segment::Placed { name: String::from(name), doc: None, position: None, kind, at, absent }
    }

    /// A [`Segment::Switch`] on `on`, with arms from the choice type `choice`.
    #[must_use]
    pub fn switch(name: &str, on: Discriminant, choice: &str, window: Option<Len>) -> Self {
        Segment::Switch {
            name: String::from(name),
            doc: None,
            position: None,
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
            position: None,
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
            Segment::Valid { .. } => {}
        }
        self
    }

    /// Sets the `#[at]` position. Does nothing on a [`Block`](Segment::Block).
    #[must_use]
    pub fn with_position(mut self, at: Position) -> Self {
        match &mut self {
            Segment::Block(_) => {}
            Segment::Opt { field, .. } => field.position = Some(at),
            Segment::Value { position, .. }
            | Segment::Repeat { position, .. }
            | Segment::Placed { position, .. }
            | Segment::Switch { position, .. }
            | Segment::Checksum { position, .. } => *position = Some(at),
            Segment::Valid { .. } => {}
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
            Segment::Repeat { when: Some(_), .. } => Vec::new(),
            Segment::Value { name, .. }
            | Segment::Repeat { name, .. }
            | Segment::Switch { name, .. }
            | Segment::Checksum { name, .. } => vec![name.as_str()],
            Segment::Placed { name, absent: None, .. } => vec![name.as_str()],
            Segment::Placed { absent: Some(_), .. } => Vec::new(),
            Segment::Opt { .. } | Segment::Valid { .. } => Vec::new(),
        }
    }

    /// The `#[align(N)]` of the length that sizes this segment, if any.
    #[must_use]
    pub fn align(&self) -> Option<u64> {
        match self {
            Segment::Repeat { count: Count::Window(Len::Field { align, .. }), .. }
            | Segment::Value { kind: Kind::Text { len: Len::Field { align, .. }, .. }, .. }
            | Segment::Switch { window: Some(Len::Field { align, .. }), .. }
            | Segment::Placed { absent: Some(Absence::ZeroLength { align, .. }), .. } => *align,
            _ => None,
        }
    }

    /// The name of the field, if it is optional.
    #[must_use]
    pub fn placed_optionally(&self) -> Option<&str> {
        match self {
            Segment::Opt { field, .. } => Some(field.name.as_str()),
            Segment::Placed { name, absent: Some(_), .. }
            | Segment::Repeat { name, when: Some(_), .. } => Some(name.as_str()),
            _ => None,
        }
    }

    /// The field name, or `None` for a [`Block`](Segment::Block) or [`Valid`](Segment::Valid).
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        match self {
            Segment::Block(_) | Segment::Valid { .. } => None,
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
    pub fn position(&self) -> Option<Position> {
        match self {
            Segment::Block(_) | Segment::Valid { .. } => None,
            Segment::Opt { field, .. } => field.position,
            Segment::Value { position, .. }
            | Segment::Repeat { position, .. }
            | Segment::Placed { position, .. }
            | Segment::Switch { position, .. }
            | Segment::Checksum { position, .. } => *position,
        }
    }

    /// The size in bits, or `None` if it depends on the wire.
    #[must_use]
    pub fn fixed_bits(&self) -> Option<u64> {
        match self {
            Segment::Block(fields) => fields.iter().map(|f| f.kind.bits()).sum(),
            Segment::Checksum { repr, .. } => Some(repr.bits()),
            Segment::Valid { .. } => Some(0),
            Segment::Value { kind, .. } => kind.bits(),
            Segment::Repeat { count, at, when, .. } => match (count, at, when) {
                (Count::Window(Len::Bytes(n)), None, None) => Some(*n as u64 * 8),
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
            | Segment::Valid { .. }
            | Segment::Checksum { .. }
            | Segment::Switch { .. }
            | Segment::Placed { .. } => false,
        }
    }
}

/// A read-only integer or `bool` the enclosing message passes in.
///
/// Fields can refer to it like an earlier field. Encode never writes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Param {
    /// The name fields refer to.
    pub name: String,
    /// The integer type, or [`Scalar::Bool`].
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
    ///
    /// The order of the hidden blocks' `Layout` impls when [`endian_from`](Self::endian_from)
    /// is set.
    pub endian: Endian,
    /// The `bool` [parameter](Self::needs) that picks the byte order: big-endian when `true`.
    ///
    /// Decode and encode are generic over [`ByteOrder`](layline_core::order::ByteOrder), and
    /// choose the order once per message. Fixed fields in this message's blocks follow it,
    /// including counts, lengths and checksums encode fills in. Nested layouts keep their own.
    pub endian_from: Option<String>,
    /// The bit order, or `None` for a byte-addressed message.
    ///
    /// A bit-addressed message holds bit fields and inline [presence bits](Presence::Prefix),
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
    /// The lifetime of [borrowed](Collection::Borrowed) fields, without the `'`: `Some("a")`
    /// declares `Name<'a>`.
    ///
    /// The message implements `layline::MessageRef<'a>` instead of `layline::Message`, so it
    /// takes no [`needs`](Self::needs) and no other message can hold it.
    pub lifetime: Option<String>,
}

impl MessageDef {
    /// A little-endian, byte-addressed message.
    #[must_use]
    pub fn new(name: &str, segments: Vec<Segment>) -> Self {
        Self {
            name: String::from(name),
            endian: Endian::Le,
            endian_from: None,
            bits: None,
            needs: Vec::new(),
            segments,
            doc: None,
            derives: Vec::new(),
            lifetime: None,
        }
    }

    /// Declares `Name<'lifetime>`, whose borrowed fields hold views of the input.
    #[must_use]
    pub fn with_lifetime(self, lifetime: &str) -> Self {
        Self { lifetime: Some(String::from(lifetime)), ..self }
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

    /// Picks the byte order from the `bool` parameter `param`: big-endian when `true`.
    #[must_use]
    pub fn with_endian_from(self, param: &str) -> Self {
        Self { endian_from: Some(String::from(param)), ..self }
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
