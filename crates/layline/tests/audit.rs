//! The audit dump, pinned byte for byte from a compiled program.
//! Every number here is computed by hand from the declared widths.

#![cfg(feature = "derive")]

// A `T` row contains the type as written in the declaration.
// The import style is part of the expected text.
use layline_core::U;
use layline_core::checksum::{Crc, Sum16Neg};
use layline_core::num::Reserved;

/// The bit pattern `height` reserves.
const ABSENT: u64 = (-1e9f32).to_bits() as u64;

type Crc16Ccitt = Crc<u16, 0x1021, 0xFFFF, 0x0000, false>;
type Sum16NegLe = Sum16Neg<false>;

#[path = "support/fixtures.rs"]
mod fixtures;

use fixtures::{Body, Ping, Switched};
use layline::{Layout, Message};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 16, order = msb, endian = be)]
struct Word {
    #[bits(5)]
    #[at(bit = 0)]
    label: u8,
    #[bits(3)]
    sublabel: u8,
    #[bits(8)]
    #[at(bit = 8)]
    spare: u8,
}

#[test]
fn a_bit_layout_publishes_its_spare_and_states_its_claims_apart() {
    assert_eq!(
        format!("{}", <Word as layline::Layout>::audit()),
        "\
L Word 16
F Word 0 8 spare
F Word 8 3 sublabel
F Word 11 5 label
V Word ok
T Word label u8
T Word sublabel u8
T Word spare u8
S Word label bit 11
S Word spare bit 0
"
    );
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 8)]
struct Header {
    #[at(byte = 0)]
    magic: u32,
    #[at(byte = 4)]
    count: u16,
    flags: u8,
    #[at(byte = 7)]
    spare: u8,
}

#[test]
fn a_byte_layout_renders_in_bits_and_states_in_bytes() {
    assert_eq!(
        format!("{}", <Header as layline::Layout>::audit()),
        "\
L Header 64
F Header 0 32 magic
F Header 32 16 count
F Header 48 8 flags
F Header 56 8 spare
V Header ok
T Header magic u32
T Header count u16
T Header flags u8
T Header spare u8
S Header magic byte 0
S Header count byte 4
S Header spare byte 7
"
    );
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
struct CommonFlags {
    #[bits(1)]
    multipath: bool,
    #[bits(1)]
    smoothing: bool,
    #[bits(6)]
    spare: u8,
}

/// `height` is a type with one reserved bit pattern.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 16)]
struct Measurement {
    tow: u32,
    #[codec(32)]
    height: Reserved<f32, ABSENT>,
    #[bytes(1)]
    flags: CommonFlags,
    quality: [u8; 3],
    reserved: u32,
}

#[test]
fn a_layout_publishes_the_type_each_field_is_read_through() {
    assert_eq!(
        format!("{}", <Measurement as layline::Layout>::audit()),
        "\
L Measurement 128
F Measurement 0 32 tow
F Measurement 32 32 height
F Measurement 64 8 flags
F Measurement 72 8 quality[0]
F Measurement 80 8 quality[1]
F Measurement 88 8 quality[2]
F Measurement 96 32 reserved
V Measurement ok
T Measurement tow u32
T Measurement height Reserved<f32,ABSENT>
T Measurement flags CommonFlags
T Measurement quality [u8;3]
T Measurement reserved u32
"
    );

    assert_eq!(
        format!("{}", <CommonFlags as layline::Layout>::audit()),
        "\
L CommonFlags 8
F CommonFlags 0 1 multipath
F CommonFlags 1 1 smoothing
F CommonFlags 2 6 spare
V CommonFlags ok
T CommonFlags multipath bool
T CommonFlags smoothing bool
T CommonFlags spare u8
"
    );
}

#[test]
fn two_layouts_with_the_same_field_table_are_told_apart_by_what_reads_it() {
    #[derive(Debug, Clone, PartialEq, Layout)]
    #[layout(bits = 16)]
    struct Plain {
        #[bits(11)]
        speed: u16,
        #[bits(5)]
        spare: u8,
    }

    struct Knots;

    #[derive(Debug, Clone, PartialEq, Layout)]
    #[layout(bits = 16)]
    struct Coded {
        #[bits(11)]
        speed: Reserved<U<11>, 2047, Knots>,
        #[bits(5)]
        spare: u8,
    }

    let plain = format!("{}", <Plain as layline::Layout>::audit());
    let coded = format!("{}", <Coded as layline::Layout>::audit());
    // The records with `tag`, minus the column naming the type they belong to.
    let rows = |text: &str, tag: &str| -> Vec<String> {
        text.lines()
            .filter(|l| l.starts_with(tag))
            .map(|l| {
                let mut column = l.split(' ');
                let tag = column.next().expect("a tag");
                column.next();
                format!("{tag} {}", column.collect::<Vec<_>>().join(" "))
            })
            .collect()
    };
    assert_eq!(rows(&plain, "F "), rows(&coded, "F "), "the same rows, to the character");
    assert_eq!(rows(&plain, "T "), ["T speed u16", "T spare u8"]);
    assert_eq!(rows(&coded, "T "), ["T speed Reserved<U<11>,2047,Knots>", "T spare u8"]);

    let wire = [0xFFu8, 0x07];
    assert_eq!(Plain::decode(&wire).speed, 2047);
    assert_eq!(Coded::decode(&wire).speed.get(), None);
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4, endian = be)]
struct Entry {
    value: u32,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
struct Packet {
    magic: u32,
    count: u16,
    #[count(count)]
    #[bytes(4)]
    entries: Vec<Entry>,
    crc: u16,
}

#[test]
fn a_message_says_where_each_row_starts_what_decides_it_and_where_solving_stops() {
    assert_eq!(
        format!("{}", <Packet as layline::Message>::audit()),
        "\
M Packet
G Packet __PacketBlock0 at,0 fixed,48 - -
F __PacketBlock0 0 32 magic
F __PacketBlock0 32 16 count
V __PacketBlock0 ok
G Packet entries at,48 counted,count,1,0,32 - Entry
G Packet __PacketBlock1 after,entries,0 fixed,16 - -
F __PacketBlock1 0 16 crc
V __PacketBlock1 ok
E Packet 48
"
    );
}

/// `header_crc` is after `set_sum` in the record.
/// Two checksums that cover each other have no valid write order.
#[derive(Debug, Clone, PartialEq, Message)]
struct Covered {
    kind: u8,
    #[checksum(Sum16NegLe, over = ..=body)]
    set_sum: u16,
    len: u8,
    #[checksum(Crc16Ccitt, over = len..)]
    header_crc: u16,
    #[count(len)]
    body: Vec<u8>,
}

#[test]
fn a_checksum_row_says_which_bytes_it_covered_and_what_is_cut_out_of_them() {
    assert_eq!(
        format!("{}", <Covered as layline::Message>::audit()),
        "\
M Covered
G Covered __CoveredBlock0 at,0 fixed,8 - -
F __CoveredBlock0 0 8 kind
V __CoveredBlock0 ok
G Covered set_sum at,8 fixed,16 - Sum16NegLe
F set_sum 0 16 set_sum
V set_sum ok
G Covered __CoveredBlock1 at,24 fixed,8 - -
F __CoveredBlock1 0 8 len
V __CoveredBlock1 ok
G Covered header_crc at,32 fixed,16 - Crc16Ccitt
F header_crc 0 16 header_crc
V header_crc ok
G Covered body at,48 counted,len,1,0,8 - -
K Covered set_sum start after,body excludes_self
K Covered header_crc before,len before,header_crc -
E Covered 48
"
    );
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
enum Loose {
    #[value(0)]
    #[bytes(2)]
    Known(Ping),
    #[other]
    Unrecognised(Vec<u8>),
}

#[test]
fn a_switch_names_the_catalogue_and_the_catalogue_scopes_its_arms() {
    assert_eq!(
        format!("{}", <Switched as layline::Message>::audit()),
        "\
M Switched
G Switched __SwitchedBlock0 at,0 fixed,8 - -
F __SwitchedBlock0 0 8 kind
V __SwitchedBlock0 ok
G Switched body at,8 chosen,field,kind - Body
G Switched __SwitchedBlock1 after,body,0 fixed,16 - -
F __SwitchedBlock1 0 16 crc
V __SwitchedBlock1 ok
E Switched 8
"
    );

    assert_eq!(
        format!("{}", <Body as layline::Choice>::audit()),
        "\
C Body
A Body Ping 0
G Body::Ping __BodyBlock0 at,0 fixed,16 - -
F __BodyBlock0 0 16 inner
V __BodyBlock0 ok
E Body::Ping 16
A Body Pong 1
G Body::Pong __BodyBlock1 at,0 fixed,32 - -
F __BodyBlock1 0 32 inner
V __BodyBlock1 ok
E Body::Pong 32
"
    );
}

#[test]
fn the_open_arm_has_no_discriminant_and_solves_to_nothing() {
    assert_eq!(
        format!("{}", <Loose as layline::Choice>::audit()),
        "\
C Loose
A Loose Known 0
G Loose::Known __LooseBlock0 at,0 fixed,16 - -
F __LooseBlock0 0 16 inner
V __LooseBlock0 ok
E Loose::Known 16
A Loose Unrecognised -
G Loose::Unrecognised Unrecognised at,0 fill - -
E Loose::Unrecognised 0
"
    );
}
