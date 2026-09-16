//! Fields and their types.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use layline_core::table::StatedUnit;

use super::Coverage;

/// An `#[at]` position, checked against the position computed from field widths.
///
/// Counted from the start of the [`LayoutDef`](crate::LayoutDef) or
/// [`MessageDef`](crate::MessageDef) body. It catches a mistyped field width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Stated {
    /// The unit of [`pos`](Self::pos).
    pub unit: StatedUnit,
    /// The position.
    pub pos: u64,
}

impl Stated {
    /// `#[at(<unit> = pos)]`.
    #[must_use]
    pub fn new(unit: StatedUnit, pos: u64) -> Self {
        Self { unit, pos }
    }

    /// `#[at(bit = pos)]`.
    #[must_use]
    pub fn bit(pos: u64) -> Self {
        Self { unit: StatedUnit::Bit, pos }
    }

    /// `#[at(byte = pos)]`.
    #[must_use]
    pub fn byte(pos: u64) -> Self {
        Self { unit: StatedUnit::Byte, pos }
    }

    /// The position in bits.
    #[must_use]
    pub fn bits(self) -> u64 {
        self.pos * self.unit.bits()
    }

    /// The `STATED` table row for the bit position `physical`.
    ///
    /// Uses this position's unit if it divides `physical`, and bits otherwise.
    #[must_use]
    pub fn published(self, physical: u64) -> Stated {
        if physical.is_multiple_of(self.unit.bits()) {
            Stated { unit: self.unit, pos: physical / self.unit.bits() }
        } else {
            Stated::bit(physical)
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
        let landed = if self.unit == StatedUnit::Byte && !at.is_multiple_of(8) {
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
    /// An unsigned integer of this many bits: 8, 16, 32 or 64 in byte mode, any width in bit modes.
    U(u64),
    /// A signed integer of this many bits, sign-extended.
    I(u64),
    /// `f32`.
    F32,
    /// `f64`.
    F64,
    /// `bool`, one bit wide.
    Bool,
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
        }
    }

    /// The smallest Rust primitive that holds this scalar, such as `u16` for 12 bits.
    #[must_use]
    pub fn primitive(self) -> &'static str {
        match self {
            Scalar::F32 => "f32",
            Scalar::F64 => "f64",
            Scalar::Bool => "bool",
            Scalar::U(n) => match n {
                0..=8 => "u8",
                9..=16 => "u16",
                17..=32 => "u32",
                _ => "u64",
            },
            Scalar::I(n) => match n {
                0..=8 => "i8",
                9..=16 => "i16",
                17..=32 => "i32",
                _ => "i64",
            },
        }
    }
}

/// A number taken from an earlier field: `field * scale + offset`.
///
/// The runtime equivalent is [`layline_core::table::By`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct By {
    /// The field to read. It must come before the field that uses it.
    pub field: String,
    /// The multiplier.
    pub scale: u32,
    /// The value added after scaling.
    pub offset: i32,
}

impl By {
    /// The field's value, unscaled.
    #[must_use]
    pub fn field(field: &str) -> Self {
        Self { field: String::from(field), scale: 1, offset: 0 }
    }

    /// `field * scale + offset`.
    #[must_use]
    pub fn new(field: &str, scale: u32, offset: i32) -> Self {
        Self { field: String::from(field), scale, offset }
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
    /// Read from an earlier field. Encode writes the field from the actual length.
    Field {
        /// The length field.
        by: By,
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
            Kind::Scalar(Scalar::F32 | Scalar::F64 | Scalar::Bool)
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
            Kind::Scalar(Scalar::Bool) => "a bool",
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
    pub stated: Option<Stated>,
    /// The `#[magic]` bytes the field must hold.
    pub magic: Option<Vec<u8>>,
    /// The `#[range]` of allowed values.
    pub range: Option<Range>,
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
            stated: None,
            magic: None,
            range: None,
            visibility: None,
        }
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
    pub fn with_stated(self, stated: Stated) -> Self {
        Self { stated: Some(stated), ..self }
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
