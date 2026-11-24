//! Fields and their types.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use layline_core::table::PositionUnit;

use super::{Coverage, Expr};

/// An `#[at]` position, checked against the position computed from field widths.
///
/// Counted from the start of the [`LayoutDef`](crate::LayoutDef) or
/// [`MessageDef`](crate::MessageDef) body. It catches a mistyped field width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Position {
    /// The unit of [`pos`](Self::pos).
    pub unit: PositionUnit,
    /// The position.
    pub pos: u64,
}

impl Position {
    /// `#[at(<unit> = pos)]`.
    #[must_use]
    pub fn new(unit: PositionUnit, pos: u64) -> Self {
        Self { unit, pos }
    }

    /// `#[at(bit = pos)]`.
    #[must_use]
    pub fn bit(pos: u64) -> Self {
        Self { unit: PositionUnit::Bit, pos }
    }

    /// `#[at(byte = pos)]`.
    #[must_use]
    pub fn byte(pos: u64) -> Self {
        Self { unit: PositionUnit::Byte, pos }
    }

    /// The position in bits.
    #[must_use]
    pub fn bits(self) -> u64 {
        self.pos * self.unit.bits()
    }

    /// The `POSITIONS` table row for the bit position `physical`.
    ///
    /// Uses this position's unit if it divides `physical`, and bits otherwise.
    #[must_use]
    pub fn published(self, physical: u64) -> Position {
        if physical.is_multiple_of(self.unit.bits()) {
            Position { unit: self.unit, pos: physical / self.unit.bits() }
        } else {
            Position::bit(physical)
        }
    }

    /// Checks the position against `at`, with no bit-order mirroring.
    ///
    /// # Errors
    ///
    /// The diagnostic text.
    pub(crate) fn check(self, field: &str, at: u64) -> Result<(), String> {
        if self.bits() == at {
            return Ok(());
        }
        let noun = self.unit.name();
        let landed = if self.unit == PositionUnit::Byte && !at.is_multiple_of(8) {
            format!("bit {at} — not a byte boundary")
        } else {
            format!("{noun} {}", at / self.unit.bits())
        };
        Err(format!(
            "field `{field}`: #[at({noun} = {})] but the solver placed it at {landed}",
            self.pos
        ))
    }
}

/// A scalar field type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Scalar {
    /// An unsigned integer of this many bits, at most 128: 8, 16, 32, 64 or 128 in byte mode.
    U(u64),
    /// A signed integer of this many bits, sign-extended.
    I(u64),
    /// `f32`.
    F32,
    /// `f64`.
    F64,
    /// `bool`, one bit wide.
    Bool,
    /// `bool` as a whole byte, the form a byte-mode layout or message takes.
    ///
    /// Decode reads 0 as `false` and 1 as `true`, and refuses any other byte with
    /// [`ParseError::Malformed`](layline_core::ParseError). Encode writes 0 or 1. For a flag the
    /// format sets with any nonzero value, use a [`Kind::Codec`] of `layline::Nonzero`.
    ByteBool,
}

impl Scalar {
    /// The width in bits.
    #[must_use]
    pub fn bits(self) -> u64 {
        match self {
            Scalar::U(n) | Scalar::I(n) => n,
            Scalar::F32 => 32,
            Scalar::F64 => 64,
            Scalar::Bool => 1,
            Scalar::ByteBool => 8,
        }
    }

    /// The smallest Rust primitive that holds this scalar, such as `u16` for 12 bits.
    #[must_use]
    pub fn primitive(self) -> &'static str {
        match self {
            Scalar::F32 => "f32",
            Scalar::F64 => "f64",
            Scalar::Bool | Scalar::ByteBool => "bool",
            Scalar::U(n) => match n {
                0..=8 => "u8",
                9..=16 => "u16",
                17..=32 => "u32",
                33..=64 => "u64",
                _ => "u128",
            },
            Scalar::I(n) => match n {
                0..=8 => "i8",
                9..=16 => "i16",
                17..=32 => "i32",
                33..=64 => "i64",
                _ => "i128",
            },
        }
    }
}

/// A byte length.
///
/// [`Count`](crate::Count) counts elements instead.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Len {
    /// A fixed number of bytes.
    Bytes(usize),
    /// Given by an [`Expr`] over earlier fields and parameters.
    ///
    /// If the expression is invertible in the one field it reads, encode writes that field from
    /// the actual length. Otherwise encode checks the length against it.
    Field {
        /// The length in bytes.
        by: Expr,
        /// The largest length allowed.
        cap: Option<usize>,
        /// Padding that is not part of the length: `#[align(N)]`.
        ///
        /// The value takes exactly the stated length, and zero bytes follow it up to the next
        /// multiple of `N` bytes, counted from the value's first byte. Decode skips them without
        /// reading them, and encode writes them. The message's `SEGMENTS` give them a row of
        /// their own, of [`Span::Pad`](layline_core::table::Span::Pad).
        align: Option<u64>,
    },
    /// An integer immediately before the value: adjacent framing, which is no field.
    ///
    /// Decode reads it and refuses a length past `cap`. Encode writes the value's length,
    /// refusing one the integer cannot hold with `EncodeError::Unfit` and one past `cap` with
    /// `EncodeError::OverCap`. The message's `SEGMENTS` give it a row of its own, named
    /// `<field>.len`.
    Prefix {
        /// The integer: [`Scalar::U`] or [`Scalar::I`] of 8, 16, 32 or 64 bits, in the message's
        /// byte order.
        repr: Scalar,
        /// The largest length allowed.
        cap: Option<usize>,
    },
    /// Up to and including the first `terminator` byte.
    Until {
        /// The byte that ends the value.
        terminator: u8,
    },
    /// To the end of the body.
    Fill,
}

/// A field's type and width.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Kind {
    /// A scalar.
    Scalar(Scalar),
    /// An array of scalars, outermost length first: `[[f64; 3]; 2]` is `[2, 3]`.
    Array(Scalar, Vec<usize>),
    /// A `FieldCodec` type.
    Codec {
        /// The type path.
        ty: String,
        /// The width in bits.
        bits: u64,
    },
    /// A nested layout.
    Nested {
        /// The type path.
        ty: String,
        /// The size in bytes.
        bytes: usize,
    },
    /// An array of nested layouts.
    NestedArray {
        /// The element type path.
        ty: String,
        /// One element's size in bytes.
        bytes: usize,
        /// The number of elements.
        len: usize,
    },
    /// A `VarCodec` type, whose size is read from the wire.
    Var {
        /// The type path.
        ty: String,
    },
    /// A nested [`MessageDef`](crate::MessageDef).
    Msg {
        /// The type path.
        ty: String,
        /// Whether the Rust field is a `Box`, as a type that contains itself needs.
        boxed: bool,
        /// The fields passed as the message's [`needs`](crate::MessageDef::needs), in order.
        with: Vec<String>,
    },
    /// A `#[checksum(..)]` field in a fixed layout.
    ///
    /// Messages use [`Segment::Checksum`](crate::Segment::Checksum).
    Checksum {
        /// The integer type.
        repr: Scalar,
        /// The path of the `layline::Checksum` algorithm.
        algorithm: String,
        /// The bytes it covers.
        over: Coverage,
    },
    /// A `String`.
    Text {
        /// The length in bytes.
        len: Len,
        /// The `TextCodec` type path. `None` is UTF-8.
        codec: Option<String>,
    },
}

impl Kind {
    /// `[u8; n]`.
    ///
    /// Same as `Array(Scalar::U(8), vec![n])`, but [`describe`](Kind::describe)s as raw bytes.
    #[must_use]
    pub fn bytes(n: usize) -> Self {
        Kind::array(Scalar::U(8), n)
    }

    /// `[s; len]`.
    #[must_use]
    pub fn array(s: Scalar, len: usize) -> Self {
        Kind::Array(s, vec![len])
    }

    /// `#[checksum(algorithm, over = ..)]`.
    #[must_use]
    pub fn checksum(repr: Scalar, algorithm: &str, over: Coverage) -> Self {
        Kind::Checksum { repr, algorithm: String::from(algorithm), over }
    }

    /// The width in bits, or `None` if the size is read from the wire.
    #[must_use]
    pub fn bits(&self) -> Option<u64> {
        Some(match self {
            Kind::Scalar(s) | Kind::Checksum { repr: s, .. } => s.bits(),
            Kind::Array(s, dims) => s.bits() * dims.iter().product::<usize>() as u64,
            Kind::Codec { bits, .. } => *bits,
            Kind::Nested { bytes, .. } => *bytes as u64 * 8,
            Kind::NestedArray { bytes, len, .. } => (*bytes * *len) as u64 * 8,
            Kind::Text { len: Len::Bytes(n), .. } => *n as u64 * 8,
            Kind::Var { .. } | Kind::Msg { .. } | Kind::Text { .. } => return None,
        })
    }

    /// [`bits`](Self::bits), or `0` if not fixed.
    pub(crate) fn width(&self) -> u64 {
        self.bits().unwrap_or(0)
    }

    /// Whether a count, length or offset can read its number from this field.
    #[must_use]
    pub fn is_integer(&self) -> bool {
        match self {
            Kind::Scalar(Scalar::U(_) | Scalar::I(_)) | Kind::Var { .. } | Kind::Codec { .. } => {
                true
            }
            Kind::Scalar(Scalar::F32 | Scalar::F64 | Scalar::Bool | Scalar::ByteBool)
            | Kind::Array(..)
            | Kind::Nested { .. }
            | Kind::NestedArray { .. }
            | Kind::Msg { .. }
            | Kind::Text { .. }
            | Kind::Checksum { .. } => false,
        }
    }

    /// A short description for error messages, such as "an integer".
    #[must_use]
    pub fn describe(&self) -> &'static str {
        match self {
            Kind::Scalar(Scalar::U(_) | Scalar::I(_)) => "an integer",
            Kind::Scalar(Scalar::F32 | Scalar::F64) => "a float",
            Kind::Scalar(Scalar::Bool | Scalar::ByteBool) => "a bool",
            Kind::Array(Scalar::U(8), _) => "a run of raw bytes",
            Kind::Array(..) => "an array",
            Kind::Codec { .. } => "a `FieldCodec` value",
            Kind::Nested { .. } => "a nested layout",
            Kind::NestedArray { .. } => "an array of nested layouts",
            Kind::Var { .. } => "a self-delimiting value",
            Kind::Msg { .. } => "a nested message",
            Kind::Checksum { .. } => "a check value over the bytes it covers",
            Kind::Text { .. } => "text",
        }
    }
}

/// A named field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Field {
    /// The field name.
    pub name: String,
    /// The type and width.
    pub kind: Kind,
    /// Documentation.
    pub doc: Option<String>,
    /// The `#[at]` position.
    pub position: Option<Position>,
    /// The `#[magic]` bytes the field must hold.
    pub magic: Option<Vec<u8>>,
    /// The `#[range]` of allowed values.
    pub range: Option<Range>,
    /// Whether the field is spare: bits the format reserves that carry no information.
    ///
    /// The bits round-trip like any other field. Serde leaves a spare field out, under the same
    /// `cfg` as the module's serde derive.
    pub spare: bool,
    /// Whether the field is a `Stated<T>`: framing, a number the wire states about other fields,
    /// away from them, that carries no information of its own. Its Rust type is
    /// `layline::Stated<T>`.
    ///
    /// Encode writes it, so an attribute must: a [`Count`](crate::Count), [`Len`], offset,
    /// stride, switch or presence test that works back to it. Decode reads it, and equality
    /// ignores it. Only a byte-addressed message's integer or `bool` field in a block can be one.
    pub stated: bool,
    /// The field's visibility, such as `pub(crate)`. `None` is `pub`.
    ///
    /// A field the generated type keeps in step with something else, such as a label that a
    /// catalogue arm already decided, is not the caller's to set.
    pub visibility: Option<String>,
}

impl Field {
    /// A field with no documentation or attributes.
    #[must_use]
    pub fn new(name: &str, kind: Kind) -> Self {
        Self {
            name: String::from(name),
            kind,
            doc: None,
            position: None,
            magic: None,
            range: None,
            spare: false,
            stated: false,
            visibility: None,
        }
    }

    /// Makes the field a `layline::Stated<T>`: framing, which encode writes.
    #[must_use]
    pub fn with_stated(self) -> Self {
        Self { stated: true, ..self }
    }

    /// Marks the field spare: its bits carry no information.
    #[must_use]
    pub fn with_spare(self) -> Self {
        Self { spare: true, ..self }
    }

    /// Sets the visibility, such as `pub(crate)` or `""` for private.
    #[must_use]
    pub fn with_visibility(self, visibility: &str) -> Self {
        Self { visibility: Some(String::from(visibility)), ..self }
    }

    /// Sets the documentation.
    #[must_use]
    pub fn with_doc(self, doc: &str) -> Self {
        Self { doc: Some(String::from(doc)), ..self }
    }

    /// Sets the `#[at]` position.
    #[must_use]
    pub fn with_position(self, stated: Position) -> Self {
        Self { position: Some(stated), ..self }
    }

    /// Sets the `#[magic]` bytes.
    #[must_use]
    pub fn with_magic(self, bytes: &[u8]) -> Self {
        Self { magic: Some(bytes.to_vec()), ..self }
    }

    /// Sets the `#[range]`.
    #[must_use]
    pub fn with_range(self, range: Range) -> Self {
        Self { range: Some(range), ..self }
    }
}

/// The values a `#[range]` allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct Range {
    /// The lowest allowed value. `None` is the type's minimum.
    pub lo: Option<i64>,
    /// The highest allowed value. `None` is the type's maximum.
    pub hi: Option<i64>,
}

impl Range {
    /// `lo..=hi`. A `None` end is unbounded.
    #[must_use]
    pub fn new(lo: Option<i64>, hi: Option<i64>) -> Self {
        Self { lo, hi }
    }
}
