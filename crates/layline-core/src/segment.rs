//! Segment tables for messages.
use crate::field::FieldDef;
use crate::row::row;

/// Where a segment starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Start<'a> {
    /// At this bit of the body, known before decoding.
    At(u64),
    /// A fixed number of bits after a segment whose size is read from the wire.
    After {
        /// The closest earlier segment whose size is read from the wire.
        segment: &'a str,
        /// Bits between the end of `segment` and this segment.
        bits: u64,
    },
    /// At a byte offset read from an earlier field, counted from the start of the body.
    Seek {
        /// The field holding the offset.
        by: &'a str,
    },
}

impl Start<'_> {
    /// The bit position, if the start is [`Start::At`].
    #[must_use]
    pub const fn bit(self) -> Option<u64> {
        match self {
            Start::At(bit) => Some(bit),
            _ => None,
        }
    }
}

/// A field a size is read from, with a scale and offset applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct By<'a> {
    /// The field holding the number.
    pub field: &'a str,
    /// Multiplier.
    pub scale: u32,
    /// Added after scaling.
    pub offset: i32,
}

impl<'a> By<'a> {
    /// The field's value, unscaled.
    #[must_use]
    pub const fn field(field: &'a str) -> Self {
        Self { field, scale: 1, offset: 0 }
    }
}

/// What selects a switch arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Discriminant<'a> {
    /// An earlier field's value.
    Field(&'a str),
    /// The number of bytes given to the switch.
    BodyLength,
}

/// How wide a segment is.
///
/// Whether it is present at all is [`SegmentDef::when`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Span<'a> {
    /// Elements up to and including the first whose first byte has `mask` set.
    ///
    /// Encode sets `mask` on the last element.
    Terminated {
        /// The bits set on the last element and clear on the others.
        mask: u8,
    },
    /// Exactly this many bits, known before decoding.
    Fixed(u64),
    /// As many elements as a field's value.
    Counted {
        /// The count field.
        by: By<'a>,
        /// One element's width in bits, or `None` if elements are self-delimiting.
        each: Option<u64>,
    },
    /// A field's value squared, as for a square matrix.
    Squared {
        /// The matrix order, squared after scaling.
        by: By<'a>,
        /// One element's width in bits, or `None` if elements are self-delimiting.
        each: Option<u64>,
    },
    /// Element count from one field, and bytes per element from another.
    Strided {
        /// The count field.
        by: By<'a>,
        /// Bytes per element.
        stride: By<'a>,
    },
    /// As many whole elements as fit in a byte length.
    Window {
        /// The length, in bytes.
        by: By<'a>,
    },
    /// The value reports its own width, as a `VarCodec` or nested message does.
    SelfDelimiting,
    /// Bytes up to and including the first byte equal to `terminator`.
    Until {
        /// The end byte.
        terminator: u8,
    },
    /// As many whole elements as the body has left. An element wider than `cap` bits is refused.
    Fill {
        /// The cap, if one was declared.
        cap: Option<u64>,
    },
    /// The selected arm sets both the content and the size.
    Chosen {
        /// What selects the arm.
        on: Discriminant<'a>,
    },
    /// A C-style union: the arm sets the content, and the size is fixed.
    ///
    /// Each arm gets exactly `bits` bits and may not read past them.
    ChosenIn {
        /// What selects the arm.
        on: Discriminant<'a>,
        /// The size, in bits.
        bits: u64,
    },
    /// Tag-length-value: the arm sets the content, and an earlier field sets the size.
    ///
    /// The arm must fill the size. Decode advances by the size whichever arm is chosen.
    ChosenWithin {
        /// What selects the arm.
        on: Discriminant<'a>,
        /// The size, in bytes.
        by: By<'a>,
    },
}

impl Span<'_> {
    /// The width in bits, if the table fixes it.
    #[must_use]
    pub const fn bits(self) -> Option<u64> {
        match self {
            Span::Fixed(bits) | Span::ChosenIn { bits, .. } => Some(bits),
            _ => None,
        }
    }
}

/// Whether a segment is present on the wire.
///
/// A row whose `when` is `None` is always present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Presence<'a> {
    /// Present when `field & mask != 0`.
    Flag {
        /// The flag field.
        field: &'a str,
        /// The bits that mark presence.
        mask: u64,
    },
    /// Present when this row's [offset](Start::Seek) is non-zero.
    Offset,
    /// Present when the bit just before it is set.
    ///
    /// That bit belongs to no field. Encode writes it from `is_some`.
    Bit,
    /// Present when the body has bytes left. Must be the last row.
    Remaining,
    /// Present when its byte length is non-zero.
    Length {
        /// The byte length field.
        by: &'a str,
    },
}

row! {
    /// One row of a message's segment table.
    SegmentDef<'a> {
        /// The field's name, or the name of a generated block of fields.
        name: &'a str,
        /// Where it starts.
        start: Start<'a>,
        /// How wide it is when present.
        span: Span<'a>,
        /// What makes it present, or `None` if it always is.
        when: Option<Presence<'a>>,
        /// The type that decodes this row. A type may name itself.
        decoded_by: Option<&'a str>,
        /// A block's fields, with offsets relative to [`start`](Self::start).
        fields: &'a [FieldDef<'a>],
    }
}

impl SegmentDef<'_> {
    /// The bits this row occupies, if the table alone decides it.
    ///
    /// `None` for a row with a [`when`](Self::when).
    #[must_use]
    pub const fn extent(&self) -> Option<u64> {
        if self.when.is_some() { None } else { self.span.bits() }
    }

    /// Whether this row's position and width are both known before decoding.
    #[must_use]
    pub const fn is_fixed(&self) -> bool {
        matches!(self.start, Start::At(_)) && self.extent().is_some()
    }
}

/// One end of a checksum's byte range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CoverEdge<'a> {
    /// The start of the body. Inside a [`Choice`], the start of the arm's body.
    Start,
    /// The first bit of the named row.
    Before(&'a str),
    /// The bit after the named row.
    After(&'a str),
}

row! {
    /// The bytes one checksum field covers.
    CoverDef<'a> {
        /// The checksum field's name.
        field: &'a str,
        /// Where the range starts.
        from: CoverEdge<'a>,
        /// Where it ends.
        to: CoverEdge<'a>,
        /// Whether the checksum field's own bytes are left out. True when the range includes them.
        excludes_self: bool,
    }
}

row! {
    /// One arm of a catalogue: a discriminant value and the body it selects.
    ArmDef<'a> {
        /// The variant's name.
        name: &'a str,
        /// The discriminant value, or `None` for the `#[other]` arm.
        value: Option<i64>,
        /// The arm's segment table, counted from the arm's first bit.
        segments: &'a [SegmentDef<'a>],
    }
}

/// The bit up to which every row's position is known before decoding.
///
/// Zero if the first row's width is read from the wire. The full width if no row's is.
///
/// ```
/// use layline_core::table::fixed_bits;
/// use layline_core::table::{By, SegmentDef, Span, Start};
///
/// const T: &[SegmentDef<'_>] = &[
///     SegmentDef::new("__PacketBlock0", Start::At(0), Span::Fixed(64), None, None, &[]),
///     SegmentDef::new(
///         "items",
///         Start::At(64),
///         Span::Counted { by: By::field("count"), each: Some(32) },
///         None,
///         Some("Item"),
///         &[],
///     ),
/// ];
/// const _: () = assert!(fixed_bits(T) == 64);
/// ```
#[must_use]
pub const fn fixed_bits(segments: &[SegmentDef<'_>]) -> u64 {
    let mut at = 0;
    let mut i = 0;
    while i < segments.len() {
        match (segments[i].start, segments[i].extent()) {
            (Start::At(start), Some(bits)) => at = start + bits,
            (Start::At(start), None) => return start,
            _ => return at,
        }
        i += 1;
    }
    at
}

/// Whether every arm is exactly `bits` wide, as [`Span::ChosenIn`] requires.
///
/// Only arms whose width the table fixes are checked. A [`Fill`](Span::Fill) arm passes if it
/// fits. An arm whose width is read from the wire is checked during decode.
#[must_use]
pub const fn arms_fit(arms: &[ArmDef<'_>], bits: u64) -> bool {
    let mut i = 0;
    while i < arms.len() {
        if !arm_fits(arms[i].segments, bits) {
            return false;
        }
        i += 1;
    }
    true
}

const fn arm_fits(rows: &[SegmentDef<'_>], bits: u64) -> bool {
    let mut at = 0;
    let mut i = 0;
    while i < rows.len() {
        let start = match rows[i].start {
            Start::At(start) => start,
            _ => return true,
        };
        if rows[i].when.is_some() {
            // An optional row: the table cannot fix the arm's width.
            return true;
        }
        if matches!(rows[i].span, Span::Fill { .. }) {
            return start <= bits;
        }
        match rows[i].span.bits() {
            Some(width) => at = start + width,
            None => return true,
        }
        i += 1;
    }
    at == bits
}

/// A variable-length type, described by a segment table.
///
/// `decode` and `encode` are trait methods, so `use layline::Message;` imports them with the derive.
/// [`decode`](Self::decode) and [`encode`](Self::encode) need `Ctx = ()`. A
/// `#[message(needs(..))]` type uses the `_with` methods.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a message",
    label = "not a `#[derive(Message)]` type",
    note = "`#[message]` fields and enum variants without `#[bytes(N)]` need a `#[derive(Message)]` type. \
            Use `#[bytes(N)]` for a `Layout`, and `#[with(..)]` for a `#[message(needs(..))]` type"
)]
pub trait Message: Sized {
    /// The type's name, as printed in the audit.
    const NAME: &'static str;

    /// Values the containing message passes in, or `()` if there are none.
    ///
    /// The derive generates a `<Name>Ctx` struct with one field per `#[message(needs(..))]` parameter.
    type Ctx;

    /// The segment table, in wire order.
    const SEGMENTS: &'static [crate::table::SegmentDef<'static>];

    /// The parameters, in declaration order.
    const PARAMS: &'static [crate::table::ParamDef<'static>] = &[];

    /// The bytes each `#[checksum]` field covers, in wire order.
    const COVERED: &'static [crate::table::CoverDef<'static>] = &[];

    /// Whether decode consumes all the bytes it is given. Read by the parent, since a derive cannot see it.
    #[doc(hidden)]
    const OPEN_ENDED: bool = false;

    /// Decode from the start of `bytes`. Returns the value and the bytes consumed.
    ///
    /// # Errors
    ///
    /// A short body, a malformed count, a wrong checksum, a missing magic, or nesting past
    /// [`MAX_NESTING_DEPTH`](crate::MAX_NESTING_DEPTH).
    fn decode_with(bytes: &[u8], ctx: Self::Ctx) -> Result<(Self, usize), crate::ParseError> {
        Self::decode_with_nested(bytes, 0, ctx)
    }

    /// [`decode_with`](Self::decode_with) at nesting depth `depth`.
    ///
    /// # Errors
    ///
    /// As [`decode_with`](Self::decode_with), plus [`TooDeep`](crate::ParseError::TooDeep).
    #[doc(hidden)]
    fn decode_with_nested(
        bytes: &[u8],
        depth: u32,
        ctx: Self::Ctx,
    ) -> Result<(Self, usize), crate::ParseError>;

    /// Append the wire bytes to `out`.
    ///
    /// Encode computes counts, lengths, offsets, flags and discriminants from the value.
    /// Parameters belong to the containing message and are not written.
    ///
    /// # Errors
    ///
    /// [`Overflow`](crate::Overflow), leaving the bytes already written in place.
    fn encode_into_with<B: crate::Buffer>(
        &self,
        out: &mut B,
        ctx: Self::Ctx,
    ) -> Result<(), crate::Overflow>;

    /// [`encode_into_with`](Self::encode_into_with) for a message with no parameters.
    ///
    /// # Errors
    ///
    /// As [`encode_into_with`](Self::encode_into_with).
    fn encode_into<B: crate::Buffer>(&self, out: &mut B) -> Result<(), crate::Overflow>
    where
        Self: Message<Ctx = ()>,
    {
        self.encode_into_with(out, ())
    }

    /// [`encode_into_with`](Self::encode_into_with) into a new vector.
    #[cfg(feature = "alloc")]
    #[must_use]
    fn encode_with(&self, ctx: Self::Ctx) -> crate::__private::Vec<u8> {
        crate::buffer::to_vec(|out| self.encode_into_with(out, ctx))
    }

    /// [`decode_with`](Self::decode_with) for a message with no parameters.
    ///
    /// # Errors
    ///
    /// As [`decode_with`](Self::decode_with).
    fn decode(bytes: &[u8]) -> Result<(Self, usize), crate::ParseError>
    where
        Self: Message<Ctx = ()>,
    {
        Self::decode_with(bytes, ())
    }

    /// [`encode_with`](Self::encode_with) for a message with no parameters.
    #[cfg(feature = "alloc")]
    #[must_use]
    fn encode(&self) -> crate::__private::Vec<u8>
    where
        Self: Message<Ctx = ()>,
    {
        self.encode_with(())
    }

    /// The audit records for this message.
    fn audit() -> crate::audit::MessageRecords<'static> {
        crate::audit::message(Self::NAME, Self::SEGMENTS)
            .covering(Self::COVERED)
            .params(Self::PARAMS)
    }
}

/// The arms of a `#[switch]` field. `#[derive(Message)]` on an enum implements it.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a catalogue",
    label = "not a `#[derive(Message)]` enum",
    note = "`#[switch(..)]` needs a `#[derive(Message)]` enum. \
            Give each variant `#[value(N)]`, or `#[other]` for the last"
)]
pub trait Choice: Sized {
    /// The type's name, as printed in the audit.
    const NAME: &'static str;

    /// The arms, in declaration order, with the `#[other]` arm last.
    const ARMS: &'static [crate::table::ArmDef<'static>];

    /// The bytes each arm's `#[checksum]` fields cover.
    const COVERED: &'static [crate::table::ArmCover<'static>] = &[];

    /// Whether the chosen arm can consume all the bytes it is given.
    #[doc(hidden)]
    const OPEN_ENDED: bool = false;

    /// Decode the arm `discriminant` selects. Returns the value and the bytes consumed.
    ///
    /// # Errors
    ///
    /// Any error from the arm, or [`Malformed`](crate::ParseError::Malformed) for an unlisted
    /// discriminant when there is no `#[other]` arm.
    fn decode_with(discriminant: i64, bytes: &[u8]) -> Result<(Self, usize), crate::ParseError> {
        Self::decode_with_nested(discriminant, bytes, 0)
    }

    /// [`decode_with`](Self::decode_with) at nesting depth `depth`.
    ///
    /// # Errors
    ///
    /// As [`decode_with`](Self::decode_with).
    #[doc(hidden)]
    fn decode_with_nested(
        discriminant: i64,
        bytes: &[u8],
        depth: u32,
    ) -> Result<(Self, usize), crate::ParseError>;

    /// This arm's discriminant, or `None` for the `#[other]` arm. Encode writes it into the parent.
    #[doc(hidden)]
    fn discriminant(&self) -> Option<i64>;

    /// Whether a listed arm has `discriminant`.
    #[doc(hidden)]
    #[must_use]
    fn defines(discriminant: i64) -> bool {
        Self::ARMS.iter().any(|arm| arm.value == Some(discriminant))
    }

    /// Append the arm's body to `out`.
    ///
    /// # Errors
    ///
    /// [`Overflow`](crate::Overflow), leaving the bytes already written in place.
    fn encode_into<B: crate::Buffer>(&self, out: &mut B) -> Result<(), crate::Overflow>;

    /// [`encode_into`](Self::encode_into) into a new vector.
    #[cfg(feature = "alloc")]
    #[must_use]
    fn encode(&self) -> crate::__private::Vec<u8> {
        crate::buffer::to_vec(|out| self.encode_into(out))
    }

    /// The audit records for every arm.
    fn audit() -> crate::audit::ChoiceRecords<'static> {
        crate::audit::choice(Self::NAME, Self::ARMS).covering(Self::COVERED)
    }
}

row! {
    /// The [`CoverDef`] rows of one arm.
    ArmCover<'a> {
        /// The variant's name, as in [`ArmDef::name`].
        arm: &'a str,
        /// The bytes the arm's checksums cover.
        covered: &'a [CoverDef<'a>],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Extent;

    const FIELDS: &[FieldDef<'_>] =
        &[FieldDef::new("magic", Extent::new(0, 32)), FieldDef::new("count", Extent::new(32, 16))];

    const BLOCK: SegmentDef<'_> =
        SegmentDef::new("__PacketBlock0", Start::At(0), Span::Fixed(48), None, None, FIELDS);

    #[test]
    fn a_table_of_solved_runs_is_solved_throughout() {
        const T: &[SegmentDef<'_>] = &[BLOCK];
        assert_eq!(fixed_bits(T), 48);
        assert!(T[0].is_fixed());
    }

    #[test]
    fn solving_stops_at_the_first_joint_and_reports_its_position() {
        const T: &[SegmentDef<'_>] = &[
            BLOCK,
            SegmentDef::new(
                "items",
                Start::At(48),
                Span::Counted { by: By::field("count"), each: Some(32) },
                None,
                Some("Item"),
                &[],
            ),
            SegmentDef::new(
                "crc",
                Start::After { segment: "items", bits: 0 },
                Span::Fixed(16),
                None,
                None,
                &[],
            ),
        ];
        assert_eq!(fixed_bits(T), 48);
        assert!(!T[1].is_fixed());
        assert!(!T[2].is_fixed(), "a row after a variable one has no known position");
        assert_eq!(T[2].start.bit(), None);
    }

    #[test]
    fn a_table_that_begins_with_a_joint_is_solved_to_nothing() {
        const T: &[SegmentDef<'_>] = &[
            SegmentDef::new("tag", Start::At(0), Span::SelfDelimiting, None, Some("Uleb128"), &[]),
            SegmentDef::new(
                "value",
                Start::After { segment: "tag", bits: 0 },
                Span::Fill { cap: None },
                None,
                None,
                &[],
            ),
        ];
        assert_eq!(fixed_bits(T), 0);
        assert!(!T[0].is_fixed());
    }

    #[test]
    fn a_directed_row_stops_solving_where_the_walk_last_knew() {
        const T: &[SegmentDef<'_>] = &[
            BLOCK,
            SegmentDef::new(
                "table",
                Start::Seek { by: "offset" },
                Span::Counted { by: By::field("count"), each: Some(32) },
                None,
                Some("Entry"),
                &[],
            ),
        ];
        assert_eq!(fixed_bits(T), 48);
        assert_eq!(T[1].start.bit(), None);
    }

    #[test]
    fn a_fixed_byte_window_keeps_solving() {
        const T: &[SegmentDef<'_>] = &[
            BLOCK,
            SegmentDef::new("children", Start::At(48), Span::Fixed(64), None, Some("Element"), &[]),
            SegmentDef::new("crc", Start::At(112), Span::Fixed(16), None, None, &[]),
        ];
        assert_eq!(fixed_bits(T), 128);
    }

    #[test]
    fn an_empty_body_is_solved_to_nothing() {
        assert_eq!(fixed_bits(&[]), 0);
    }

    #[test]
    fn a_strided_run_names_both_the_count_and_the_step() {
        const T: &[SegmentDef<'_>] = &[
            BLOCK,
            SegmentDef::new(
                "sub_blocks",
                Start::At(48),
                Span::Strided { by: By::field("n"), stride: By::field("sb_length") },
                None,
                Some("SubBlock"),
                &[],
            ),
        ];
        assert_eq!(fixed_bits(T), 48);
        assert!(!T[1].is_fixed());
        assert_eq!(T[1].span.bits(), None, "both numbers come from the wire");
    }

    #[test]
    fn a_fixed_footprint_union_keeps_solving() {
        const T: &[SegmentDef<'_>] = &[
            BLOCK,
            SegmentDef::new(
                "args",
                Start::At(48),
                Span::ChosenIn { on: Discriminant::Field("cmd"), bits: 64 },
                None,
                Some("Args"),
                &[],
            ),
            SegmentDef::new("payload", Start::At(112), Span::Fixed(32), None, None, &[]),
        ];
        assert_eq!(fixed_bits(T), 144);
        assert!(T[1].is_fixed(), "a union's size is fixed");
        assert_eq!(T[1].span.bits(), Some(64));
        assert_eq!(T[2].start.bit(), Some(112), "so the next row's position is known");
    }

    #[test]
    fn an_arm_that_decides_its_own_extent_is_still_a_joint() {
        const T: &[SegmentDef<'_>] = &[
            BLOCK,
            SegmentDef::new(
                "body",
                Start::At(48),
                Span::Chosen { on: Discriminant::Field("kind") },
                None,
                Some("Body"),
                &[],
            ),
        ];
        assert_eq!(fixed_bits(T), 48);
        assert!(!T[1].is_fixed());
        assert_eq!(T[1].span.bits(), None);
    }

    #[test]
    fn a_catalogue_tiles_a_footprint_or_it_does_not() {
        const EXACT: &[SegmentDef<'_>] =
            &[SegmentDef::new("__ArgsBlock0", Start::At(0), Span::Fixed(64), None, None, &[])];
        const SHORT: &[SegmentDef<'_>] =
            &[SegmentDef::new("__ArgsBlock1", Start::At(0), Span::Fixed(32), None, None, &[])];
        const OPEN: &[SegmentDef<'_>] =
            &[SegmentDef::new("Unknown", Start::At(0), Span::Fill { cap: None }, None, None, &[])];
        const WIRE: &[SegmentDef<'_>] = &[
            SegmentDef::new("__ArgsBlock2", Start::At(0), Span::Fixed(8), None, None, &[]),
            SegmentDef::new(
                "items",
                Start::At(8),
                Span::Counted { by: By::field("n"), each: Some(8) },
                None,
                Some("Item"),
                &[],
            ),
        ];

        let exact = ArmDef::new("Exact", Some(0), EXACT);
        let short = ArmDef::new("Short", Some(1), SHORT);
        let open = ArmDef::new("Unknown", None, OPEN);
        let wire = ArmDef::new("Wire", Some(2), WIRE);

        assert!(arms_fit(&[exact], 64));
        assert!(!arms_fit(&[exact, short], 64), "a short arm fails");
        assert!(arms_fit(&[exact, open], 64), "a fill arm passes");
        assert!(arms_fit(&[exact, wire], 64), "a wire-sized arm is checked during decode");
        assert!(!arms_fit(&[short], 64));

        let empty = ArmDef::new("Nop", Some(3), &[]);
        assert!(!arms_fit(&[empty], 64));
        assert!(arms_fit(&[empty], 0), "an empty arm is zero bits wide");
    }
}
