//! `#[derive(FieldCodec)]`: `from_raw` accepts every value of the width, and `to_raw` returns its bits.

#![cfg(feature = "derive")]

use layline::{FieldCodec, Layout, Message};

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(15)]
pub struct TrackNumber(pub u16);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(8)]
pub struct Strength(pub u8);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(64)]
pub struct Opaque(pub u64);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(3)]
pub enum Identity {
    #[value(0)]
    #[default]
    Pending,
    #[value(1)]
    Unknown,
    #[value(2)]
    Friend,
    #[other]
    Undefined(u8),
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(2)]
pub enum Quadrant {
    #[value(0)]
    #[default]
    N,
    #[value(1)]
    E,
    #[value(2)]
    S,
    #[value(3)]
    W,
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(2)]
pub enum Shuffled {
    #[value(3)]
    D,
    #[value(0)]
    A,
    #[value(2)]
    C,
    #[value(1)]
    B,
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(12)]
pub enum Vendor {
    #[value(0x000)]
    Unassigned,
    #[value(0x2A7)]
    Reference,
    #[value(0xFFF)]
    Broadcast,
    #[other]
    Other(u16),
}

#[test]
fn a_newtype_round_trips_every_value_of_its_width() {
    for raw in 0..(1u64 << 15) {
        let v = TrackNumber::from_raw(raw);
        assert_eq!(v.to_raw(), raw, "TrackNumber lost {raw}");
        assert_eq!(v.0, raw as u16, "the inner value is the raw value");
    }
}

#[test]
fn a_newtype_narrower_than_its_primitive_does_not_carry_stray_high_bits() {
    assert_eq!(TrackNumber::from_raw(0x8000).0, 0, "bit 15 is outside the width");
    assert_eq!(TrackNumber::from_raw(0xFFFF).0, 0x7FFF);
    assert_eq!(TrackNumber::from_raw(0xFFFF).to_raw() >> TrackNumber::BITS, 0);
}

#[test]
fn a_full_width_newtype_round_trips_its_whole_primitive() {
    for raw in 0..=0xFFu64 {
        assert_eq!(Strength::from_raw(raw).to_raw(), raw);
    }
    for raw in [0, 1, u64::MAX / 3, u64::MAX - 1, u64::MAX] {
        assert_eq!(Opaque::from_raw(raw).to_raw(), raw);
        assert_eq!(Opaque::from_raw(raw).0, raw);
    }
}

#[test]
fn an_open_enum_names_the_values_it_knows() {
    assert_eq!(Identity::from_raw(0), Identity::Pending);
    assert_eq!(Identity::from_raw(1), Identity::Unknown);
    assert_eq!(Identity::from_raw(2), Identity::Friend);
    assert_eq!(Identity::Friend.to_raw(), 2);
}

#[test]
fn an_open_enum_agrees_with_itself_over_its_whole_range() {
    for raw in 0..(1u64 << Identity::BITS) {
        let v = Identity::from_raw(raw);
        assert_eq!(v.to_raw(), raw, "Identity lost {raw}");
        assert_eq!(
            matches!(v, Identity::Undefined(u) if u64::from(u) == raw),
            raw >= 3,
            "raw {raw}"
        );
    }
    for raw in 0..(1u64 << Vendor::BITS) {
        let v = Vendor::from_raw(raw);
        assert_eq!(v.to_raw(), raw, "Vendor lost {raw:#05X}");
        let named = matches!(raw, 0x000 | 0x2A7 | 0xFFF);
        assert_eq!(matches!(v, Vendor::Other(_)), !named, "raw {raw:#05X}");
    }
}

#[test]
fn an_open_enum_ignores_bits_above_its_width() {
    assert_eq!(Identity::from_raw(0b1_010), Identity::Friend);
    assert_eq!(Vendor::from_raw(0x1_2A7), Vendor::Reference);
}

#[test]
fn a_closed_enum_is_total_over_its_width() {
    let all = [Quadrant::N, Quadrant::E, Quadrant::S, Quadrant::W];
    for raw in 0..4u64 {
        let v = Quadrant::from_raw(raw);
        assert_eq!(v, all[raw as usize], "raw {raw}");
        assert_eq!(v.to_raw(), raw);
    }
    for (raw, expected) in [(0, Shuffled::A), (1, Shuffled::B), (2, Shuffled::C), (3, Shuffled::D)]
    {
        assert_eq!(Shuffled::from_raw(raw), expected, "raw {raw}");
        assert_eq!(expected.to_raw(), raw);
    }
}

#[test]
fn a_closed_enum_ignores_bits_above_its_width() {
    assert_eq!(Quadrant::from_raw(0b1_11), Quadrant::W);
    assert_eq!(Quadrant::from_raw(u64::MAX), Quadrant::W);
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 24)]
pub struct Contact {
    #[bits(15)]
    pub track: TrackNumber,
    #[bits(3)]
    pub identity: Identity,
    #[bits(2)]
    pub quadrant: Quadrant,
    #[bits(1)]
    pub urgent: bool,
    #[bits(3)]
    pub spare_21: u8,
}

#[test]
fn derived_codecs_carry_their_fields_through_a_layout() {
    let c = Contact {
        track: TrackNumber::from_raw(0x5A5A),
        identity: Identity::Undefined(6),
        quadrant: Quadrant::S,
        urgent: true,
        spare_21: 0b101,
    };
    assert_eq!(Contact::decode(&c.encode()), c);

    for hi in 0..=0xFFu8 {
        for lo in [0x00u8, 0x5A, 0xA5, 0xFF] {
            let wire = [lo, hi, lo ^ hi];
            assert_eq!(Contact::decode(&wire).encode(), wire, "{wire:02X?}");
        }
    }

    let fields: Vec<_> =
        Contact::FIELDS.iter().map(|f| (f.name, f.extent.start(), f.extent.width())).collect();
    assert_eq!(
        fields,
        vec![
            ("track", 0, 15),
            ("identity", 15, 3),
            ("quadrant", 18, 2),
            ("urgent", 20, 1),
            ("spare_21", 21, 3),
        ]
    );
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(8)]
pub enum Kind {
    #[value(0)]
    Ping,
    #[value(1)]
    Data,
    #[other]
    Other(u8),
}

#[derive(Layout, Debug, Clone, PartialEq, Eq)]
#[layout(bytes = 2, endian = be)]
pub struct Payload {
    pub word: u16,
}

#[derive(Message, Debug, Clone, PartialEq, Eq)]
pub enum Body {
    #[value(0)]
    #[bytes(2)]
    Ping(Payload),
    #[value(1)]
    #[bytes(2)]
    Data(Payload),
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Message, Debug, Clone, PartialEq, Eq)]
#[message(endian = be)]
pub struct Framed {
    #[codec(8)]
    pub kind: Kind,
    #[switch(kind)]
    pub body: Body,
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(8)]
pub struct Count(u8);

#[derive(Message, Debug, Clone, PartialEq, Eq)]
#[message(endian = be)]
pub struct Counted {
    #[codec(8)]
    pub how_many: Count,
    #[count(how_many)]
    #[bytes(2)]
    pub items: Vec<Payload>,
}

#[test]
fn a_catalogue_can_be_a_discriminant() {
    let wire = [0x01, 0xBE, 0xEF];
    let (got, used) = Framed::decode(&wire).expect("parses");
    assert_eq!(used, 3);
    assert_eq!(got.kind, Kind::Data);
    assert_eq!(got.body, Body::Data(Payload { word: 0xBEEF }));
    assert_eq!(got.encode(), wire);
}

#[test]
fn an_unlisted_discriminant_reaches_the_unknown_arm() {
    let wire = [0x07, 0xAA];
    let (got, _) = Framed::decode(&wire).expect("parses");
    assert_eq!(got.kind, Kind::Other(7));
    assert_eq!(got.body, Body::Unknown(vec![0xAA]));
    assert_eq!(got.encode(), wire);
}

#[test]
fn the_discriminant_is_written_from_the_arm_held_not_from_the_field() {
    let wrong = Framed { kind: Kind::Ping, body: Body::Data(Payload { word: 1 }) };
    assert_eq!(wrong.encode()[0], 1);
}

#[test]
fn a_catalogue_can_be_a_count() {
    let wire = [0x02, 0x00, 0x01, 0x00, 0x02];
    let (got, used) = Counted::decode(&wire).expect("parses");
    assert_eq!(used, 5);
    assert_eq!(got.how_many, Count(2));
    assert_eq!(got.items, vec![Payload { word: 1 }, Payload { word: 2 }]);
    assert_eq!(got.encode(), wire);

    let mut m = Counted { how_many: Count(9), items: vec![Payload { word: 7 }] };
    assert_eq!(m.encode()[0], 1);
    m.items.clear();
    assert_eq!(m.encode()[0], 0);
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(16)]
pub struct Millivolt(pub i16);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq)]
#[bits(12)]
pub struct Trim(pub i16);

#[derive(Layout, Debug, Clone, PartialEq, Eq)]
#[layout(bits = 32, order = msb)]
pub struct Reading {
    #[bits(16)]
    pub millivolts: Millivolt,
    #[bits(12)]
    pub trim: Trim,
    #[bits(4)]
    pub spare: u8,
}

#[test]
fn a_narrow_signed_codec_extends_from_its_declared_width() {
    assert_eq!(<Trim as FieldCodec>::from_raw(0x800), Trim(-2048));
    assert_eq!(<Trim as FieldCodec>::from_raw(0x7FF), Trim(2047));
    assert_eq!(<Trim as FieldCodec>::from_raw(0xFFF), Trim(-1));
    assert_eq!(FieldCodec::to_raw(&Trim(-1)), 0xFFF);
    assert_eq!(FieldCodec::to_raw(&Trim(-2048)), 0x800);
}

#[test]
fn a_signed_codec_sits_in_a_bit_container() {
    for raw in [0i16, 1, -1, i16::MAX, i16::MIN, -1234, 4095] {
        let there = Reading { millivolts: Millivolt(raw), trim: Trim(-5), spare: 0 };
        assert_eq!(Reading::decode(&there.encode()), there, "{raw}");
    }
}

#[derive(Message, Debug, Clone, PartialEq, Eq)]
#[message(closed)]
pub enum Train {
    #[value(0)]
    #[bytes(2)]
    Simple(Payload),
    #[value(1)]
    None,
}

#[derive(Message, Debug, Clone, PartialEq, Eq)]
#[message(endian = be)]
pub struct Pulse {
    #[codec(8)]
    pub kind: Kind,
    #[switch(kind)]
    pub train: Train,
    pub tail: u8,
}

#[test]
fn an_arm_can_carry_nothing() {
    let with = [0x00, 0x12, 0x34, 0x99];
    let (got, used) = Pulse::decode(&with).expect("parses");
    assert_eq!(used, 4);
    assert_eq!(got.train, Train::Simple(Payload { word: 0x1234 }));
    assert_eq!(got.tail, 0x99);
    assert_eq!(got.encode(), with);

    let without = [0x01, 0x99];
    let (got, used) = Pulse::decode(&without).expect("parses");
    assert_eq!(used, 2);
    assert_eq!(got.train, Train::None);
    assert_eq!(got.tail, 0x99);
    assert_eq!(got.encode(), without);
}
