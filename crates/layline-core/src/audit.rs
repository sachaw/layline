//! The audit format: every table of a type, one record per line.
//!
//! The `audit()` trait methods print a compiled type. The functions here take rows directly, for
//! a code generator printing a model it has not compiled.
use core::fmt;

use crate::LayoutError;
use crate::table::{
    ArmCover, ArmDef, By, ConstDef, CoverDef, CoverEdge, Discriminant, FieldDef, ParamDef,
    Presence, RangeDef, SegmentDef, Span, Start, StatedDef, TypeDef,
};
use crate::table::{check_layout, fixed_bits};

/// Audit records for a layout: `L`, an `F` per field, the `V` check result, then `T`, `X`, `R`, `K` and `S` rows.
#[derive(Debug, Clone, Copy)]
pub struct LayoutRecords<'a> {
    name: &'a str,
    bits: u64,
    fields: &'a [FieldDef<'a>],
    types: &'a [TypeDef<'a>],
    constants: &'a [ConstDef<'a>],
    ranges: &'a [RangeDef<'a>],
    covered: &'a [CoverDef<'a>],
    stated: &'a [StatedDef<'a>],
}

/// Audit records for a message: `M`, a `P` per parameter, a `G` per segment with its fields, then `E`.
#[derive(Debug, Clone, Copy)]
pub struct MessageRecords<'a> {
    name: &'a str,
    segments: &'a [SegmentDef<'a>],
    covered: &'a [CoverDef<'a>],
    params: &'a [ParamDef<'a>],
}

/// Audit records for a catalogue: `C`, then an `A` and a segment table per arm.
#[derive(Debug, Clone, Copy)]
pub struct ChoiceRecords<'a> {
    name: &'a str,
    arms: &'a [ArmDef<'a>],
    covered: &'a [ArmCover<'a>],
}

/// Records for a field table in a container of `bits` bits. `V` reports whether the fields cover it exactly.
#[must_use]
pub fn layout<'a>(name: &'a str, bits: u64, fields: &'a [FieldDef<'a>]) -> LayoutRecords<'a> {
    LayoutRecords {
        name,
        bits,
        fields,
        types: &[],
        constants: &[],
        ranges: &[],
        covered: &[],
        stated: &[],
    }
}

impl<'a> LayoutRecords<'a> {
    /// Add the `#[at(..)]` positions as `S` records.
    #[must_use]
    pub fn stated(self, stated: &'a [StatedDef<'a>]) -> Self {
        Self { stated, ..self }
    }

    /// Add each field's type as `T` records.
    #[must_use]
    pub fn types(self, types: &'a [TypeDef<'a>]) -> Self {
        Self { types, ..self }
    }

    /// Add the `#[magic(..)]` constants as `X` records.
    #[must_use]
    pub fn constants(self, constants: &'a [ConstDef<'a>]) -> Self {
        Self { constants, ..self }
    }

    /// Add the `#[range(..)]` bounds as `R` records.
    #[must_use]
    pub fn ranges(self, ranges: &'a [RangeDef<'a>]) -> Self {
        Self { ranges, ..self }
    }

    /// Add checksum coverage as `K` records.
    #[must_use]
    pub fn covering(self, covered: &'a [CoverDef<'a>]) -> Self {
        Self { covered, ..self }
    }
}

/// Records for a segment table.
#[must_use]
pub fn message<'a>(name: &'a str, segments: &'a [SegmentDef<'a>]) -> MessageRecords<'a> {
    MessageRecords { name, segments, covered: &[], params: &[] }
}

impl<'a> MessageRecords<'a> {
    /// Add checksum coverage as `K` records.
    #[must_use]
    pub fn covering(self, covered: &'a [CoverDef<'a>]) -> Self {
        Self { covered, ..self }
    }

    /// Add the parameters as `P` records.
    #[must_use]
    pub fn params(self, params: &'a [ParamDef<'a>]) -> Self {
        Self { params, ..self }
    }
}

/// Records for a catalogue's arms.
#[must_use]
pub fn choice<'a>(name: &'a str, arms: &'a [ArmDef<'a>]) -> ChoiceRecords<'a> {
    ChoiceRecords { name, arms, covered: &[] }
}

impl<'a> ChoiceRecords<'a> {
    /// Add each arm's checksum coverage as `K` records.
    #[must_use]
    pub fn covering(self, covered: &'a [ArmCover<'a>]) -> Self {
        Self { covered, ..self }
    }
}

fn absent<W: fmt::Write>(w: &mut W, value: Option<u64>) -> fmt::Result {
    match value {
        Some(n) => write!(w, "{n}"),
        None => w.write_str("-"),
    }
}

fn scope<W: fmt::Write>(w: &mut W, ty: &str, arm: Option<&str>) -> fmt::Result {
    w.write_str(ty)?;
    if let Some(arm) = arm {
        w.write_str("::")?;
        w.write_str(arm)?;
    }
    Ok(())
}

fn by_token<W: fmt::Write>(w: &mut W, by: By<'_>) -> fmt::Result {
    write!(w, "{},{},{}", by.field, by.scale, by.offset)
}

fn start_token<W: fmt::Write>(w: &mut W, start: Start<'_>) -> fmt::Result {
    match start {
        Start::At(bit) => write!(w, "at,{bit}"),
        Start::After { segment, bits } => write!(w, "after,{segment},{bits}"),
        Start::Seek { by } => write!(w, "seek,{by}"),
    }
}

fn when_token<W: fmt::Write>(w: &mut W, when: Option<Presence<'_>>) -> fmt::Result {
    match when {
        None => w.write_str("-"),
        Some(Presence::Flag { field, mask }) => write!(w, "flag,{field},{mask:#x}"),
        Some(Presence::Offset) => w.write_str("offset"),
        Some(Presence::Bit) => w.write_str("bit"),
        Some(Presence::Remaining) => w.write_str("remaining"),
        Some(Presence::Length { by }) => write!(w, "length,{by}"),
    }
}

fn span_token<W: fmt::Write>(w: &mut W, span: &Span<'_>) -> fmt::Result {
    match span {
        Span::Fixed(bits) => write!(w, "fixed,{bits}"),
        Span::Counted { by, each } => {
            w.write_str("counted,")?;
            by_token(w, *by)?;
            w.write_str(",")?;
            absent(w, *each)
        }
        Span::Squared { by, each } => {
            w.write_str("squared,")?;
            by_token(w, *by)?;
            w.write_str(",")?;
            absent(w, *each)
        }
        Span::Strided { by, stride } => {
            w.write_str("strided,")?;
            by_token(w, *by)?;
            w.write_str(",")?;
            by_token(w, *stride)
        }
        Span::Window { by } => {
            w.write_str("window,")?;
            by_token(w, *by)
        }
        Span::SelfDelimiting => w.write_str("selfdelimiting"),
        Span::Until { terminator } => write!(w, "until,{terminator:#04x}"),
        Span::Terminated { mask } => write!(w, "terminated,{mask:#04x}"),
        Span::Fill { cap: None } => w.write_str("fill"),
        Span::Fill { cap } => {
            w.write_str("fill,")?;
            absent(w, *cap)
        }
        Span::Chosen { on: Discriminant::Field(field) } => write!(w, "chosen,field,{field}"),
        Span::Chosen { on: Discriminant::BodyLength } => w.write_str("chosen,bodylength"),
        Span::ChosenIn { on: Discriminant::Field(field), bits } => {
            write!(w, "chosenin,field,{field},{bits}")
        }
        Span::ChosenIn { on: Discriminant::BodyLength, bits } => {
            write!(w, "chosenin,bodylength,{bits}")
        }
        Span::ChosenWithin { on: Discriminant::Field(field), by } => {
            write!(w, "chosenwithin,field,{field},")?;
            by_token(w, *by)
        }
        Span::ChosenWithin { on: Discriminant::BodyLength, by } => {
            w.write_str("chosenwithin,bodylength,")?;
            by_token(w, *by)
        }
    }
}

fn edge_token<W: fmt::Write>(w: &mut W, edge: CoverEdge<'_>) -> fmt::Result {
    match edge {
        CoverEdge::Start => w.write_str("start"),
        CoverEdge::Before(row) => write!(w, "before,{row}"),
        CoverEdge::After(row) => write!(w, "after,{row}"),
    }
}

fn verdict_token<W: fmt::Write>(w: &mut W, verdict: Result<(), LayoutError>) -> fmt::Result {
    match verdict {
        Ok(()) => w.write_str("ok"),
        Err(LayoutError::Gap { field, expected_start, actual_start }) => {
            write!(w, "gap,{field},{expected_start},{actual_start}")
        }
        Err(LayoutError::Short { end, total_bits }) => write!(w, "short,{end},{total_bits}"),
    }
}

fn field_records<W: fmt::Write>(
    w: &mut W,
    ty: &str,
    arm: Option<&str>,
    fields: &[FieldDef<'_>],
    total: Option<u64>,
) -> fmt::Result {
    for f in fields {
        w.write_str("F ")?;
        scope(w, ty, arm)?;
        writeln!(w, " {} {} {}", f.extent.start(), f.extent.width(), f.name)?;
    }
    if let Some(total) = total {
        w.write_str("V ")?;
        scope(w, ty, arm)?;
        w.write_str(" ")?;
        verdict_token(w, check_layout(fields, total))?;
        w.write_str("\n")?;
    }
    Ok(())
}

/// One `<tag> <type> <…>` line per row. `tail` writes the rest of the line.
fn records<W: fmt::Write, T>(
    w: &mut W,
    tag: char,
    ty: &str,
    rows: &[T],
    mut tail: impl FnMut(&mut W, &T) -> fmt::Result,
) -> fmt::Result {
    for row in rows {
        write!(w, "{tag} {ty} ")?;
        tail(w, row)?;
        w.write_str("\n")?;
    }
    Ok(())
}

fn cover_records<W: fmt::Write>(
    w: &mut W,
    ty: &str,
    arm: Option<&str>,
    covered: &[CoverDef<'_>],
) -> fmt::Result {
    for c in covered {
        w.write_str("K ")?;
        scope(w, ty, arm)?;
        write!(w, " {} ", c.field)?;
        edge_token(w, c.from)?;
        w.write_str(" ")?;
        edge_token(w, c.to)?;
        w.write_str(if c.excludes_self { " excludes_self\n" } else { " -\n" })?;
    }
    Ok(())
}

fn segment_records<W: fmt::Write>(
    w: &mut W,
    ty: &str,
    arm: Option<&str>,
    segments: &[SegmentDef<'_>],
    covered: &[CoverDef<'_>],
) -> fmt::Result {
    for d in segments {
        w.write_str("G ")?;
        scope(w, ty, arm)?;
        w.write_str(" ")?;
        w.write_str(d.name)?;
        w.write_str(" ")?;
        start_token(w, d.start)?;
        w.write_str(" ")?;
        span_token(w, &d.span)?;
        w.write_str(" ")?;
        when_token(w, d.when)?;
        w.write_str(" ")?;
        match d.decoded_by {
            Some(ty) => w.write_str(ty)?,
            None => w.write_str("-")?,
        }
        w.write_str("\n")?;
        if !d.fields.is_empty() {
            field_records(w, d.name, None, d.fields, d.extent())?;
        }
    }
    cover_records(w, ty, arm, covered)?;
    w.write_str("E ")?;
    scope(w, ty, arm)?;
    writeln!(w, " {}", fixed_bits(segments))
}

impl fmt::Display for LayoutRecords<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "L {} {}", self.name, self.bits)?;
        field_records(f, self.name, None, self.fields, Some(self.bits))?;
        records(f, 'T', self.name, self.types, |w, c| write!(w, "{} {}", c.field, c.ty))?;
        records(f, 'X', self.name, self.constants, |w, c| {
            write!(w, "{} ", c.field)?;
            for byte in c.bytes {
                write!(w, "{byte:02x}")?;
            }
            Ok(())
        })?;
        records(f, 'R', self.name, self.ranges, |w, r| write!(w, "{} {} {}", r.field, r.lo, r.hi))?;
        cover_records(f, self.name, None, self.covered)?;
        records(f, 'S', self.name, self.stated, |w, s| {
            write!(w, "{} {} {}", s.field, s.unit.name(), s.pos)
        })
    }
}

impl fmt::Display for MessageRecords<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "M {}", self.name)?;
        records(f, 'P', self.name, self.params, |w, p| write!(w, "{} {}", p.name, p.ty))?;
        segment_records(f, self.name, None, self.segments, self.covered)
    }
}

impl fmt::Display for ChoiceRecords<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "C {}", self.name)?;
        for arm in self.arms {
            match arm.value {
                Some(value) => writeln!(f, "A {} {} {value}", self.name, arm.name)?,
                None => writeln!(f, "A {} {} -", self.name, arm.name)?,
            }
            let covered =
                self.covered.iter().find(|c| c.arm == arm.name).map_or(&[][..], |c| c.covered);
            segment_records(f, self.name, Some(arm.name), arm.segments, covered)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buf::Buf;
    use crate::table::StatedUnit;
    use crate::{Extent, Layout};

    const HEADER: &[FieldDef<'_>] =
        &[FieldDef::new("magic", Extent::new(0, 32)), FieldDef::new("count", Extent::new(32, 8))];

    #[test]
    fn a_layout_renders_its_fields_its_claims_and_its_verdict() {
        const STATED: &[StatedDef<'_>] = &[
            StatedDef::new("magic", StatedUnit::Byte, 0),
            StatedDef::new("count", StatedUnit::Bit, 32),
        ];
        let b = Buf::of(layout("Header", 40, HEADER).stated(STATED));
        assert_eq!(
            b.text(),
            "\
L Header 40
F Header 0 32 magic
F Header 32 8 count
V Header ok
S Header magic byte 0
S Header count bit 32
"
        );
    }

    #[test]
    fn the_types_a_field_is_read_through_are_published_with_the_proven_records() {
        const TYPES: &[TypeDef<'_>] =
            &[TypeDef::new("magic", "u32"), TypeDef::new("count", "Reserved<U<8>,255>")];
        const STATED: &[StatedDef<'_>] = &[StatedDef::new("magic", StatedUnit::Byte, 0)];
        let b = Buf::of(layout("Header", 40, HEADER).types(TYPES).stated(STATED));
        assert_eq!(
            b.text(),
            "\
L Header 40
F Header 0 32 magic
F Header 32 8 count
V Header ok
T Header magic u32
T Header count Reserved<U<8>,255>
S Header magic byte 0
"
        );

        let b = Buf::of(layout("Header", 40, HEADER));
        assert!(!b.text().contains("T "), "{}", b.text());
    }

    #[test]
    fn the_verdict_reports_a_hole_it_was_handed() {
        const HOLEY: &[FieldDef<'_>] =
            &[FieldDef::new("a", Extent::new(0, 2)), FieldDef::new("b", Extent::new(3, 5))];
        let b = Buf::of(layout("Holey", 8, HOLEY));
        assert!(b.text().ends_with("V Holey gap,1,2,3\n"), "{}", b.text());

        let b = Buf::of(layout("Short", 48, HEADER));
        assert!(b.text().ends_with("V Short short,40,48\n"), "{}", b.text());
    }

    const BLOCK: &[SegmentDef<'_>] = &[
        SegmentDef::new("__PacketBlock0", Start::At(0), Span::Fixed(40), None, None, HEADER),
        SegmentDef::new(
            "items",
            Start::At(40),
            Span::Counted { by: By::field("count"), each: Some(16) },
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

    #[test]
    fn a_message_renders_its_rows_its_blocks_and_where_solving_stops() {
        let b = Buf::of(message("Packet", BLOCK));
        assert_eq!(
            b.text(),
            "\
M Packet
G Packet __PacketBlock0 at,0 fixed,40 - -
F __PacketBlock0 0 32 magic
F __PacketBlock0 32 8 count
V __PacketBlock0 ok
G Packet items at,40 counted,count,1,0,16 - Item
G Packet crc after,items,0 fixed,16 - -
E Packet 40
"
        );
    }

    #[test]
    fn the_rows_a_wire_decides_say_what_decides_them() {
        const T: &[SegmentDef<'_>] = &[
            SegmentDef::new(
                "label",
                Start::At(0),
                Span::Until { terminator: 0 },
                Some(Presence::Flag { field: "flags", mask: 0x04 }),
                None,
                &[FieldDef::new("label", Extent::new(0, 8))],
            ),
            SegmentDef::new(
                "table",
                Start::Seek { by: "offset" },
                Span::Counted { by: By { field: "n", scale: 4, offset: -1 }, each: None },
                None,
                Some("Entry"),
                &[],
            ),
            SegmentDef::new(
                "blob",
                Start::After { segment: "table", bits: 32 },
                Span::Fill { cap: Some(64) },
                None,
                None,
                &[],
            ),
            SegmentDef::new(
                "body",
                Start::After { segment: "blob", bits: 0 },
                Span::Chosen { on: Discriminant::BodyLength },
                None,
                Some("Body"),
                &[],
            ),
        ];
        let b = Buf::of(message("Odd", T));
        assert_eq!(
            b.text(),
            "\
M Odd
G Odd label at,0 until,0x00 flag,flags,0x4 -
F label 0 8 label
G Odd table seek,offset counted,n,4,-1,- - Entry
G Odd blob after,table,32 fill,64 - -
G Odd body after,blob,0 chosen,bodylength - Body
E Odd 0
"
        );
    }

    #[test]
    fn one_record_at_a_stated_offset_names_no_counting_field() {
        const T: &[SegmentDef<'_>] = &[
            SegmentDef::new(
                "table",
                Start::Seek { by: "table_off" },
                Span::Fixed(64),
                None,
                Some("Table"),
                &[],
            ),
            SegmentDef::new(
                "symb",
                Start::Seek { by: "symb_off" },
                Span::SelfDelimiting,
                None,
                Some("Symb"),
                &[],
            ),
            SegmentDef::new(
                "info",
                Start::Seek { by: "info_off" },
                Span::Fixed(64),
                Some(Presence::Offset),
                Some("Info"),
                &[],
            ),
            SegmentDef::new(
                "wave",
                Start::Seek { by: "wave_off" },
                Span::Fixed(64),
                Some(Presence::Length { by: "wave_size" }),
                Some("Wave"),
                &[],
            ),
        ];
        let b = Buf::of(message("Container", T));
        assert_eq!(
            b.text(),
            "M Container
G Container table seek,table_off fixed,64 - Table
G Container symb seek,symb_off selfdelimiting - Symb
G Container info seek,info_off fixed,64 offset Info
G Container wave seek,wave_off fixed,64 length,wave_size Wave
E Container 0
"
        );
        assert!(!b.text().contains("counted"), "nothing counts one record");
    }

    #[test]
    fn a_union_with_a_fixed_footprint_says_the_footprint_is_its_own() {
        const T: &[SegmentDef<'_>] = &[
            SegmentDef::new(
                "__CommandBlock0",
                Start::At(0),
                Span::Fixed(32),
                None,
                None,
                &[FieldDef::new("cmd", Extent::new(0, 32))],
            ),
            SegmentDef::new(
                "args",
                Start::At(32),
                Span::ChosenIn { on: Discriminant::Field("cmd"), bits: 64 },
                None,
                Some("Args"),
                &[],
            ),
            SegmentDef::new(
                "payload",
                Start::At(96),
                Span::Fixed(32),
                None,
                None,
                &[FieldDef::new("payload", Extent::new(0, 32))],
            ),
        ];
        let b = Buf::of(message("Command", T));
        assert_eq!(
            b.text(),
            "\
M Command
G Command __CommandBlock0 at,0 fixed,32 - -
F __CommandBlock0 0 32 cmd
V __CommandBlock0 ok
G Command args at,32 chosenin,field,cmd,64 - Args
G Command payload at,96 fixed,32 - -
F payload 0 32 payload
V payload ok
E Command 128
"
        );
    }

    #[test]
    fn a_window_the_wire_states_is_its_own_token() {
        const T: &[SegmentDef<'_>] = &[SegmentDef::new(
            "value",
            Start::At(24),
            Span::ChosenWithin {
                on: Discriminant::Field("tag"),
                by: By { field: "len", scale: 2, offset: 0 },
            },
            None,
            Some("Value"),
            &[],
        )];
        let b = Buf::of(message("Tlv", T));
        assert_eq!(
            b.text(),
            "\
M Tlv
G Tlv value at,24 chosenwithin,field,tag,len,2,0 - Value
E Tlv 24
"
        );
    }

    #[test]
    fn a_footprint_chosen_by_body_length_names_no_field() {
        const T: &[SegmentDef<'_>] = &[SegmentDef::new(
            "args",
            Start::At(0),
            Span::ChosenIn { on: Discriminant::BodyLength, bits: 16 },
            None,
            Some("Args"),
            &[],
        )];
        let b = Buf::of(message("ByLength", T));
        assert_eq!(
            b.text(),
            "\
M ByLength
G ByLength args at,0 chosenin,bodylength,16 - Args
E ByLength 16
"
        );
    }

    #[test]
    fn every_arm_is_scoped_to_the_variant_it_is() {
        const PRIMITIVE: &[SegmentDef<'_>] = &[SegmentDef::new(
            "__PrimitiveBlock0",
            Start::At(0),
            Span::Fixed(8),
            None,
            None,
            &[FieldDef::new("len", Extent::new(0, 8))],
        )];
        const UNKNOWN: &[SegmentDef<'_>] =
            &[SegmentDef::new("raw", Start::At(0), Span::Fill { cap: None }, None, None, &[])];
        const ARMS: &[ArmDef<'_>] =
            &[ArmDef::new("Primitive", Some(0), PRIMITIVE), ArmDef::new("Unknown", None, UNKNOWN)];
        let b = Buf::of(choice("Element", ARMS));
        assert_eq!(
            b.text(),
            "\
C Element
A Element Primitive 0
G Element::Primitive __PrimitiveBlock0 at,0 fixed,8 - -
F __PrimitiveBlock0 0 8 len
V __PrimitiveBlock0 ok
E Element::Primitive 8
A Element Unknown -
G Element::Unknown raw at,0 fill - -
E Element::Unknown 0
"
        );
    }

    #[test]
    fn a_checksum_says_which_bytes_it_covered() {
        const COVERED: &[CoverDef<'_>] = &[
            CoverDef::new("crc", CoverEdge::Start, CoverEdge::Before("crc"), false),
            CoverDef::new("set_sum", CoverEdge::Before("count"), CoverEdge::After("items"), true),
        ];
        let b = Buf::of(message("Packet", BLOCK).covering(COVERED));
        assert!(b.text().contains("K Packet crc start before,crc -\n"), "{}", b.text());
        assert!(
            b.text().contains("K Packet set_sum before,count after,items excludes_self\n"),
            "{}",
            b.text(),
        );
        let mut tags = [0u8; 16];
        let mut n = 0;
        for line in b.text().lines().filter(|l| !l.starts_with(['F', 'V'])) {
            tags[n] = line.as_bytes()[0];
            n += 1;
        }
        assert_eq!(&tags[..n], b"MGGGKKE", "{}", b.text());

        assert!(!Buf::of(message("Packet", BLOCK)).text().contains("K "));
    }

    #[test]
    fn a_type_supplies_its_own_width_table_and_claims() {
        struct Hand;
        impl Layout for Hand {
            const NAME: &'static str = "Hand";
            const WIRE_BYTES: usize = 5;
            fn decode_slice(_: &[u8]) -> Result<Self, crate::ParseError> {
                Err(crate::ParseError::Short { need_bytes: 5, got_bytes: 0, at: 0 })
            }
            fn encode_into<B: crate::Buffer>(&self, _: &mut B) -> Result<(), crate::Overflow> {
                Ok(())
            }
            const FIELDS: &'static [crate::table::FieldDef<'static>] = HEADER;
            const TYPES: &'static [crate::table::TypeDef<'static>] =
                &[TypeDef::new("magic", "u32")];
            const STATED: &'static [crate::table::StatedDef<'static>] =
                &[StatedDef::new("count", StatedUnit::Byte, 4)];
        }
        let b = Buf::of(<Hand as Layout>::audit());
        assert_eq!(
            b.text(),
            "\
L Hand 40
F Hand 0 32 magic
F Hand 32 8 count
V Hand ok
T Hand magic u32
S Hand count byte 4
"
        );
        struct Bare;
        impl Layout for Bare {
            const NAME: &'static str = "Bare";
            const WIRE_BYTES: usize = 5;
            fn decode_slice(_: &[u8]) -> Result<Self, crate::ParseError> {
                Err(crate::ParseError::Short { need_bytes: 5, got_bytes: 0, at: 0 })
            }
            fn encode_into<B: crate::Buffer>(&self, _: &mut B) -> Result<(), crate::Overflow> {
                Ok(())
            }
            const FIELDS: &'static [crate::table::FieldDef<'static>] = HEADER;
        }
        let b = Buf::of(<Bare as Layout>::audit());
        assert!(!b.text().contains("S "), "{}", b.text());
        assert!(!b.text().contains("T "), "{}", b.text());
    }
}
