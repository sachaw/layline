//! `U<N>` / `I<N>`: the type sets the width in bits, and a `#[bits]` on the field must agree.

#![cfg(feature = "derive")]

use layline::{FieldCodec, I, Layout, Message, U};

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 8, order = msb)]
struct VersionIhl {
    version: U<4>,
    ihl: U<4>,
}

#[test]
fn an_attribute_free_field_lands_where_the_standard_says() {
    let table: Vec<(&str, u64, u32)> =
        VersionIhl::FIELDS.iter().map(|f| (f.name, f.extent.start(), f.extent.width())).collect();
    // FIELDS is in physical order, and under `order = msb` the first declared field is the top nibble.
    assert_eq!(table, vec![("ihl", 0, 4), ("version", 4, 4)]);
    assert_eq!(layline::table::check_layout(<VersionIhl as Layout>::FIELDS, 8), Ok(()));

    let msg = VersionIhl { version: U::new(4).unwrap(), ihl: U::new(5).unwrap() };
    assert_eq!(msg.encode(), [0x45]);
    assert_eq!(VersionIhl::decode(&[0x45]), msg);
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 8, order = msb)]
struct VersionIhlDoubleEntry {
    #[bits(4)]
    version: U<4>,
    #[bits(4)]
    #[at(bit = 4)]
    ihl: U<4>,
}

#[test]
fn a_double_entry_width_is_accepted_when_it_agrees() {
    let a = VersionIhl { version: U::new(4).unwrap(), ihl: U::new(5).unwrap() };
    let b = VersionIhlDoubleEntry { version: U::new(4).unwrap(), ihl: U::new(5).unwrap() };
    assert_eq!(a.encode(), b.encode());
    let rows = |f: &[layline::table::FieldDef<'static>]| -> Vec<(&str, u64, u32)> {
        f.iter().map(|f| (f.name, f.extent.start(), f.extent.width())).collect()
    };
    assert_eq!(rows(VersionIhl::FIELDS), rows(VersionIhlDoubleEntry::FIELDS));
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(2)]
enum Quality {
    #[value(0)]
    #[default]
    Bad,
    #[value(1)]
    Fair,
    #[value(2)]
    Good,
    #[other]
    Other(u8),
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bits = 16)]
struct Sample {
    #[bits(1)]
    flag: bool,
    #[bits(2)]
    quality: Quality,
    level: U<5>,
    trim: I<6>,
    #[bits(2)]
    spare: u8,
}

#[test]
fn carried_widths_interleave_with_declared_ones() {
    assert_eq!(
        Sample::FIELDS
            .iter()
            .map(|f| (f.name, f.extent.start(), f.extent.width()))
            .collect::<Vec<_>>(),
        vec![("flag", 0, 1), ("quality", 1, 2), ("level", 3, 5), ("trim", 8, 6), ("spare", 14, 2),],
    );
}

#[test]
fn a_signed_carried_width_sign_extends_through_the_wire() {
    for raw in 0..64u64 {
        let trim = I::<6>::from_raw(raw);
        let msg = Sample {
            flag: true,
            quality: Quality::Good,
            level: U::new(31).unwrap(),
            trim,
            spare: 0,
        };
        let wire = msg.encode();
        assert_eq!(Sample::decode(&wire), msg, "round trip at raw {raw}");
        let want = if raw < 32 { raw as i64 } else { raw as i64 - 64 };
        assert_eq!(trim.get(), want, "sign extension at raw {raw}");
        assert_eq!((u16::from_le_bytes(wire) >> 8) & 0b11_1111, raw as u16);
    }
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(words = 1)]
struct Header {
    kind: U<4>,
    seq: U<12>,
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bytes = 3)]
struct Record {
    tag: U<8>,
    count: U<16>,
}

#[test]
fn every_container_mode_reads_the_width_off_the_type() {
    let h = Header { kind: U::new(9).unwrap(), seq: U::new(0x321).unwrap() };
    assert_eq!(h.encode(), [0x19, 0x32]);
    assert_eq!(Header::decode(&h.encode()), h);

    let r = Record { tag: U::new(0xAB).unwrap(), count: U::new(0x1234).unwrap() };
    assert_eq!(r.encode(), [0xAB, 0x34, 0x12]);
    assert_eq!(Record::decode(&r.encode()), r);
    assert_eq!(
        Record::FIELDS
            .iter()
            .map(|f| (f.name, f.extent.start(), f.extent.width()))
            .collect::<Vec<_>>(),
        vec![("tag", 0, 8), ("count", 8, 16)],
    );
}

#[test]
fn an_out_of_range_value_has_no_constructor() {
    assert_eq!(U::<4>::new(16), None);
    assert_eq!(I::<6>::new(32), None);
    assert_eq!(I::<6>::new(-33), None);

    for v in 0..16u64 {
        let msg = VersionIhl { version: U::new(v).unwrap(), ihl: U::new(15 - v).unwrap() };
        assert_eq!(VersionIhl::decode(&msg.encode()), msg);
    }
}

#[test]
fn a_block_field_takes_the_width_its_type_states() {
    #[derive(layline::Message, Debug, PartialEq)]
    struct Stated {
        n: layline::U<8>,
        v: u16,
    }

    #[derive(layline::Message, Debug, PartialEq)]
    struct Restated {
        #[codec(8)]
        n: layline::U<8>,
        v: u16,
    }

    let m = Stated { n: layline::U::new(7).expect("7 fits in 8 bits"), v: 0x0102 };
    let wire = m.encode();
    assert_eq!(wire.len(), 3, "one byte for the codec, two for the scalar");
    assert_eq!(Stated::decode(&wire).expect("round trip"), (m, wire.len()));

    let same = Restated { n: layline::U::new(7).expect("7 fits in 8 bits"), v: 0x0102 };
    assert_eq!(same.encode(), wire, "the double entry changes no byte");
}
