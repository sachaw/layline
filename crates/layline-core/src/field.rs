//! Field tables for layouts.
use crate::Extent;
use crate::row::row;

row! {
    /// One field of a layout.
    FieldDef<'a> {
        /// The field's name.
        name: &'a str,
        /// The field's position and width.
        extent: Extent,
    }
}

row! {
    /// A field's Rust type, joined to its [`FieldDef`] by name.
    TypeDef<'a> {
        /// The field's name.
        field: &'a str,
        /// The type as written in the source.
        ty: &'a str,
    }
}

row! {
    /// A parameter of a [`Message`](crate::Message) whose `Ctx` is not `()`.
    ParamDef<'a> {
        /// The name. Rows refer to it as they would to a field.
        name: &'a str,
        /// The type as written in the source.
        ty: &'a str,
    }
}

/// The unit of a [`StatedDef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StatedUnit {
    /// `#[at(bit = N)]`.
    Bit,
    /// `#[at(byte = N)]`.
    Byte,
}

impl StatedUnit {
    /// Bits per unit.
    #[must_use]
    pub const fn bits(self) -> u64 {
        match self {
            StatedUnit::Bit => 1,
            StatedUnit::Byte => 8,
        }
    }

    /// The unit's name in `#[at(..)]`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            StatedUnit::Bit => "bit",
            StatedUnit::Byte => "byte",
        }
    }
}

row! {
    /// A field position from `#[at(..)]`, as a standard numbers it.
    ///
    /// Counted from the container's first bit after `order = msb` mirroring, so it matches
    /// [`FieldDef`]. Stored in the declared unit if the position divides evenly, and in bits otherwise.
    StatedDef<'a> {
        /// The field's name.
        field: &'a str,
        /// The unit of [`pos`](Self::pos).
        unit: StatedUnit,
        /// The position, in [`unit`](Self::unit)s.
        pos: u64,
    }
}

row! {
    /// A `#[magic(..)]` field and its constant.
    ConstDef<'a> {
        /// The field's name.
        field: &'a str,
        /// The constant's bytes, in wire order.
        bytes: &'a [u8],
    }
}

row! {
    /// The values a `#[range(..)]` field allows.
    RangeDef<'a> {
        /// The field's name.
        field: &'a str,
        /// The lowest allowed value.
        lo: i64,
        /// The highest allowed value.
        hi: i64,
    }
}

impl StatedDef<'_> {
    /// The position in bits.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.pos * self.unit.bits()
    }
}

/// Why a field table does not exactly cover its container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LayoutError {
    /// A field does not start where the previous one ended.
    Gap {
        /// Index of the field.
        field: usize,
        /// Where the previous field ended.
        expected_start: u64,
        /// Where this field starts.
        actual_start: u64,
    },
    /// The fields do not end at the container's last bit.
    Short {
        /// Where the fields end.
        end: u64,
        /// The container's width in bits.
        total_bits: u64,
    },
}

impl core::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Gap { field, expected_start, actual_start } => write!(
                f,
                "field {field} starts at bit {actual_start}, but the previous field ends at bit \
                 {expected_start}"
            ),
            Self::Short { end, total_bits } => {
                write!(f, "the fields end at bit {end} of a {total_bits}-bit container")
            }
        }
    }
}

impl core::error::Error for LayoutError {}

/// Check that `fields`, in order, cover exactly `total` bits.
///
/// # Errors
///
/// [`LayoutError::Gap`] at the first field that does not start where the previous one ended.
/// [`LayoutError::Short`] if the fields do not end at `total`.
pub const fn check_layout(fields: &[FieldDef<'_>], total: u64) -> Result<(), LayoutError> {
    let mut at = 0u64;
    let mut i = 0;
    while i < fields.len() {
        let extent = fields[i].extent;
        if extent.start() != at {
            return Err(LayoutError::Gap {
                field: i,
                expected_start: at,
                actual_start: extent.start(),
            });
        }
        at = extent.end();
        i += 1;
    }
    if at != total {
        return Err(LayoutError::Short { end: at, total_bits: total });
    }
    Ok(())
}

/// The width of the field named `name`, or zero if there is none.
///
/// ```
/// use layline_core::{Extent, table::{FieldDef, field_width}};
///
/// const FIELDS: &[FieldDef<'_>] = &[
///     FieldDef::new("kind", Extent::new(0, 3)),
///     FieldDef::new("n_entries", Extent::new(3, 5)),
/// ];
/// const N: u32 = field_width(FIELDS, "n_entries");
/// assert_eq!(N, 5);
/// assert_eq!(field_width(FIELDS, "absent"), 0);
/// ```
#[must_use]
pub const fn field_width(fields: &[FieldDef<'_>], name: &str) -> u32 {
    let mut i = 0;
    while i < fields.len() {
        if str_eq(fields[i].name, name) {
            return fields[i].extent.width();
        }
        i += 1;
    }
    0
}

/// Whether the field named `name` has the type `ty`.
///
/// ```
/// use layline_core::table::{TypeDef, type_is};
///
/// const TYPES: &[TypeDef<'_>] = &[
///     TypeDef::new("seq_flag", "bool"),
///     TypeDef::new("n_entries", "u8"),
/// ];
/// const IS_FLAG: bool = type_is(TYPES, "seq_flag", "bool");
/// assert!(IS_FLAG);
/// assert!(!type_is(TYPES, "n_entries", "bool"));
/// assert!(!type_is(TYPES, "absent", "bool"));
/// ```
#[must_use]
pub const fn type_is(types: &[TypeDef<'_>], name: &str, ty: &str) -> bool {
    let mut i = 0;
    while i < types.len() {
        if str_eq(types[i].field, name) {
            return str_eq(types[i].ty, ty);
        }
        i += 1;
    }
    false
}

const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// [`check_layout`] as a panic, for a `const` block in generated code.
pub const fn assert_layout(fields: &[FieldDef<'_>], total: u64) {
    assert!(
        check_layout(fields, total).is_ok(),
        "layout fields do not exactly cover the container"
    );
}

/// A fixed-size type whose named fields cover every bit.
///
/// `FIELDS` passes [`check_layout`]: it ascends from bit zero to `WIRE_BYTES * 8`, with no gaps or overlaps.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a fixed-size layout",
    label = "not a `#[derive(Layout)]` type",
    note = "`#[bytes(N)]` fields and `Dispatch` payloads need a fixed size. \
            Add `#[derive(Layout)]`, or use `#[message]` for a `#[derive(Message)]` type"
)]
pub trait Layout: Sized {
    /// The type's name, as printed in the audit.
    const NAME: &'static str;

    /// Size on the wire, in bytes.
    const WIRE_BYTES: usize;

    /// Every field, in ascending order, covering `WIRE_BYTES * 8` bits.
    const FIELDS: &'static [crate::table::FieldDef<'static>];

    /// Whether the inherent `decode` returns a `Result`, as it does when the layout checks its own bytes.
    ///
    /// Read by the parent, since a derive cannot see a field's type.
    #[doc(hidden)]
    const DECODE_FALLIBLE: bool = false;

    /// Decode from a slice.
    ///
    /// # Errors
    ///
    /// [`Short`](crate::ParseError::Short) if the slice is not [`WIRE_BYTES`](Self::WIRE_BYTES)
    /// long. Otherwise a failed `#[magic]`, `#[checksum]` or `#[range]`.
    fn decode_slice(bytes: &[u8]) -> Result<Self, crate::ParseError>;

    /// Append the [`WIRE_BYTES`](Self::WIRE_BYTES) bytes to `out`.
    ///
    /// # Errors
    ///
    /// [`Overflow`](crate::Overflow) if there is no room.
    fn encode_into<B: crate::Buffer>(&self, out: &mut B) -> Result<(), crate::Overflow>;

    /// The low bits owned by an enclosing dispatcher, or zero.
    ///
    /// A `#[dispatch(.., prefix = K)]` catalogue checks that every arm agrees.
    const PREFIX_BITS: u32 = 0;

    /// The [`StatedDef`] rows, in declaration order.
    const STATED: &'static [crate::table::StatedDef<'static>] = &[];

    /// The type of each field, in declaration order.
    ///
    /// Empty when every field is a plain integer of its declared width.
    const TYPES: &'static [crate::table::TypeDef<'static>] = &[];

    /// The `#[magic(..)]` fields, in declaration order.
    const CONSTANTS: &'static [crate::table::ConstDef<'static>] = &[];

    /// The `#[range(..)]` bounds, in declaration order.
    const RANGES: &'static [crate::table::RangeDef<'static>] = &[];

    /// The bytes each `#[checksum(..)]` field covers, in declaration order.
    const COVERED: &'static [crate::table::CoverDef<'static>] = &[];

    /// The audit records for this layout.
    fn audit() -> crate::audit::LayoutRecords<'static> {
        crate::audit::layout(Self::NAME, Self::WIRE_BYTES as u64 * 8, Self::FIELDS)
            .stated(Self::STATED)
            .types(Self::TYPES)
            .constants(Self::CONSTANTS)
            .ranges(Self::RANGES)
            .covering(Self::COVERED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &[FieldDef<'_>] = &[
        FieldDef::new("a", Extent::new(0, 2)),
        FieldDef::new("b", Extent::new(2, 5)),
        FieldDef::new("c", Extent::new(7, 63)),
    ];

    #[test]
    fn a_tiled_layout_passes() {
        assert_eq!(check_layout(GOOD, 70), Ok(()));
    }

    #[test]
    fn a_gap_is_named() {
        let holey = &[FieldDef::new("a", Extent::new(0, 2)), FieldDef::new("b", Extent::new(3, 5))];
        assert_eq!(
            check_layout(holey, 8),
            Err(LayoutError::Gap { field: 1, expected_start: 2, actual_start: 3 })
        );
    }

    #[test]
    fn a_short_layout_is_named() {
        assert_eq!(check_layout(GOOD, 72), Err(LayoutError::Short { end: 70, total_bits: 72 }));
    }
}
