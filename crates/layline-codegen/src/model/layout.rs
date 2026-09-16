//! Fixed layouts and their containers.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use layline_core::table::StatedUnit;

use super::{Field, Kind, Scalar, Stated};
use crate::Invalid;

/// Byte order. Little-endian by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Endian {
    /// Little-endian.
    #[default]
    Le,
    /// Big-endian.
    Be,
}

/// Bit order within a bit-addressed container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BitOrder {
    /// The first field takes the least significant bits.
    Lsb,
    /// The first field takes the most significant bits.
    Msb,
}

/// How a layout addresses its fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Container {
    /// Word mode: bit fields in one container of whole bytes.
    ///
    /// The container can be any size, but a field is at most 64 bits.
    Word {
        /// The size in bits, a positive multiple of 8.
        bits: u64,
        /// Leading bits the enclosing dispatch reads, which this layout's fields do not cover.
        ///
        /// Encode writes zeros there unless [`prefix_value`](Container::Word::prefix_value) is
        /// set.
        prefix: u64,
        /// The prefix's value. Encode writes it and decode refuses any other.
        prefix_value: Option<u128>,
        /// The byte order.
        endian: Endian,
        /// The bit order.
        order: BitOrder,
    },
    /// Byte mode: byte-aligned scalars.
    Bytes {
        /// The size in bytes.
        bytes: usize,
        /// The byte order.
        endian: Endian,
    },
    /// Words mode: bit fields in a sequence of 16-bit words.
    Words {
        /// The number of words.
        words: usize,
        /// Each word's byte order.
        endian: Endian,
        /// The bit order within each word.
        order: BitOrder,
    },
}

/// A size unit, and its attribute keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unit {
    /// `#[layout(bits = N)]`, `#[bits(N)]`.
    Bits,
    /// `#[layout(bytes = N)]`, `#[bytes(N)]`.
    Bytes,
    /// `#[layout(words = N)]`.
    Words,
}

impl Unit {
    /// The attribute keyword.
    #[must_use]
    pub fn keyword(self) -> &'static str {
        match self {
            Unit::Bits => "bits",
            Unit::Bytes => "bytes",
            Unit::Words => "words",
        }
    }
}

impl Container {
    /// A [`Container::Word`] with no prefix.
    #[must_use]
    pub fn word(bits: u64, endian: Endian, order: BitOrder) -> Self {
        Container::Word { bits, prefix: 0, prefix_value: None, endian, order }
    }

    /// Checks that this container allows a field of `kind`.
    ///
    /// # Errors
    ///
    /// [`Invalid::Field`] naming the field.
    pub fn admits(&self, name: &str, kind: &Kind) -> Result<(), Invalid> {
        let no = |what: &str| Err(Invalid::field(name, String::from(what)));

        if let Kind::Array(_, dims) = kind
            && dims.is_empty()
        {
            return no("an array with no dimensions. Give it a length, or declare a scalar");
        }

        match self {
            Container::Bytes { .. } => match kind {
                Kind::Scalar(Scalar::Bool) => {
                    no("byte mode has no `bool`. Declare it in word mode")
                }
                Kind::Text { .. } => {
                    no("byte mode has no text. Declare it in a `#[derive(Message)]`")
                }
                _ => Ok(()),
            },
            Container::Words { .. } => match kind {
                Kind::Array(s, _) if !matches!(s, Scalar::U(8 | 16)) => {
                    no("words mode allows only `[u16; K]` and `[u8; K]` arrays")
                }
                _ => Ok(()),
            },
            Container::Word { .. } => match kind {
                Kind::Scalar(Scalar::F32 | Scalar::F64) => {
                    no("word mode has no floats. Declare it in byte mode")
                }
                Kind::Nested { .. } | Kind::NestedArray { .. } => {
                    no("word mode has no nested layouts. Declare it in byte mode")
                }
                Kind::Text { .. } | Kind::Var { .. } | Kind::Msg { .. } => no(
                    "word mode needs a fixed width, and this field's size is read from the wire. \
                     Declare it in a `#[derive(Message)]`",
                ),
                _ => Ok(()),
            },
        }
    }

    /// The unit of the size in `#[layout(..)]`.
    #[must_use]
    pub fn unit(&self) -> Unit {
        match self {
            Container::Word { .. } => Unit::Bits,
            Container::Bytes { .. } => Unit::Bytes,
            Container::Words { .. } => Unit::Words,
        }
    }

    /// The size in [`unit`](Self::unit)s: the `N` of `#[layout(<unit> = N)]`.
    #[must_use]
    pub fn units(&self) -> u64 {
        match self {
            Container::Word { bits, .. } => *bits,
            Container::Bytes { bytes, .. } => *bytes as u64,
            Container::Words { words, .. } => *words as u64,
        }
    }

    /// The byte order.
    #[must_use]
    pub fn endian(&self) -> Endian {
        match self {
            Container::Word { endian, .. }
            | Container::Bytes { endian, .. }
            | Container::Words { endian, .. } => *endian,
        }
    }

    /// The bit order, or `None` in byte mode.
    #[must_use]
    pub fn order(&self) -> Option<BitOrder> {
        match self {
            Container::Word { order, .. } | Container::Words { order, .. } => Some(*order),
            Container::Bytes { .. } => None,
        }
    }

    /// Whether fields are addressed in bits.
    #[must_use]
    pub fn is_bit_addressed(&self) -> bool {
        !matches!(self, Container::Bytes { .. })
    }

    /// The size in bits.
    #[must_use]
    pub fn bits(&self) -> u64 {
        match self {
            Container::Word { bits, .. } => *bits,
            Container::Bytes { bytes, .. } => *bytes as u64 * 8,
            Container::Words { words, .. } => *words as u64 * 16,
        }
    }

    /// Leading bits the enclosing dispatch reads.
    #[must_use]
    pub fn prefix(&self) -> u64 {
        match self {
            Container::Word { prefix, .. } => *prefix,
            Container::Bytes { .. } | Container::Words { .. } => 0,
        }
    }

    /// The bits the fields must cover: [`bits`](Self::bits) minus [`prefix`](Self::prefix).
    #[must_use]
    pub fn field_bits(&self) -> u64 {
        match self {
            Container::Word { bits, prefix, .. } => bits.saturating_sub(*prefix),
            other => other.bits(),
        }
    }

    /// Bits in the unit a byte order applies to: the container, one word, or the field.
    fn region_bits(&self, width: u64) -> u64 {
        match self {
            Container::Word { bits, .. } => *bits,
            Container::Words { .. } => 16,
            Container::Bytes { .. } => width,
        }
    }

    /// First bit of the unit that holds the bit `start`.
    fn region_start(&self, start: u64) -> u64 {
        match self {
            Container::Word { .. } => 0,
            Container::Words { .. } => start - start % 16,
            Container::Bytes { .. } => start,
        }
    }

    /// The word size in words mode (`Some(16)`), and `None` otherwise.
    #[must_use]
    pub fn interior_boundary(&self) -> Option<u64> {
        match self {
            Container::Words { .. } => Some(16),
            Container::Word { .. } | Container::Bytes { .. } => None,
        }
    }

    /// The wire position of a field declared at bit `start`, after applying [`BitOrder`].
    ///
    /// Under `order = msb`, scalar and codec fields are mirrored within their word. `FIELDS` and
    /// `STATED` use this position. [`check_stated`](Self::check_stated) uses declared positions.
    #[must_use]
    pub fn physical(&self, kind: &Kind, start: u64) -> u64 {
        let (region, order) = match self {
            Container::Word { bits, order, .. } => (*bits, *order),
            Container::Words { order, .. } => (16, *order),
            Container::Bytes { .. } => return start,
        };
        let packed = matches!(kind, Kind::Scalar(_) | Kind::Codec { .. });
        if order == BitOrder::Lsb || !packed || region == 0 {
            return start;
        }
        let base = start - start % region;
        base + region.saturating_sub(start - base + kind.width())
    }

    /// Checks an `#[at]` position against the field's declared position.
    ///
    /// Under `msb`, `#[at]` counts in declared order, as a big-endian spec's table does.
    ///
    /// # Errors
    ///
    /// The diagnostic text. Under `msb` it gives both positions.
    pub fn check_stated(
        &self,
        field: &str,
        kind: &Kind,
        stated: Stated,
        declared: u64,
    ) -> Result<(), String> {
        let msb = matches!(
            self,
            Container::Word { order: BitOrder::Msb, .. }
                | Container::Words { order: BitOrder::Msb, .. }
        );
        if !msb {
            return stated.check(field, declared);
        }
        let claimed = stated.bits();
        if claimed == declared {
            return Ok(());
        }
        let width = kind.width();
        let region = self.region_bits(width);
        let place = |bits| place(stated.unit, bits);
        let lands = if claimed + width <= self.region_start(claimed) + region {
            format!("is physical {}", place(self.physical(kind, claimed)))
        } else {
            let unit = match self {
                Container::Words { .. } => "word",
                _ => "container",
            };
            format!("does not fit the {region}-bit {unit}")
        };
        Err(format!(
            "field `{field}`: #[at({} = {})] (declared numbering, msb) {lands}, but the solver \
             placed it at physical {} — declared {}",
            stated.unit.name(),
            stated.pos,
            place(self.physical(kind, declared)),
            place(declared),
        ))
    }
}

/// A bit position in `unit`, such as `byte 3`, or `bit 27 (not a byte boundary)`.
fn place(unit: StatedUnit, bits: u64) -> String {
    if bits.is_multiple_of(unit.bits()) {
        format!("{} {}", unit.name(), bits / unit.bits())
    } else {
        format!("bit {bits} (not a byte boundary)")
    }
}

/// A fixed-size layout whose fields cover every bit of its container.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LayoutDef {
    /// The type name.
    pub name: String,
    /// The container.
    pub container: Container,
    /// The fields, in wire order.
    pub fields: Vec<Field>,
    /// Whether to generate a `<Name>View` that reads fields in place: `#[layout(.., view)]`.
    pub view: bool,
}

impl LayoutDef {
    /// A layout with no view.
    #[must_use]
    pub fn new(name: &str, container: Container, fields: Vec<Field>) -> Self {
        Self { name: String::from(name), container, fields, view: false }
    }

    /// Adds a `<Name>View`.
    #[must_use]
    pub fn with_view(self) -> Self {
        Self { view: true, ..self }
    }
}

/// A field and its position in the container.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Positioned<'a> {
    pub(crate) field: &'a Field,
    /// The sum of the earlier fields' widths.
    pub(crate) declared: u64,
    /// [`declared`](Self::declared), after [`Container::physical`].
    pub(crate) physical: u64,
}

/// Each field with its declared and physical position.
///
/// The fields of a [`Segment::Block`](crate::Segment::Block) use this too.
pub(crate) fn positioned<'a>(
    fields: &'a [Field],
    container: &'a Container,
) -> impl Iterator<Item = Positioned<'a>> {
    fields.iter().scan(container.prefix(), move |at, field| {
        let declared = *at;
        *at += field.kind.width();
        Some(Positioned { field, declared, physical: container.physical(&field.kind, declared) })
    })
}
