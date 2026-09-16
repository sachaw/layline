//! Two 72-bit layouts that share a container, tested against an independent bit-extraction oracle.

#![cfg(feature = "derive")]

#[path = "support/oracle.rs"]
mod oracle;
#[path = "support/rng.rs"]
mod rng;

use layline::{FieldCodec, Layout};
use oracle::WordBits;
use rng::Rng;

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(19)]
pub struct RecordId(pub u32);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(13)]
pub struct Altitude25Ft(pub u16);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(21)]
pub struct Latitude(pub u32);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(22)]
pub struct Longitude(pub u32);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(9)]
pub struct Course(pub u16);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(11)]
pub struct Speed(pub u16);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(3)]
pub struct ClassDetail(pub u8);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(3)]
pub enum Classification {
    #[value(0)]
    #[default]
    Unset,
    #[value(1)]
    Unknown,
    #[value(2)]
    Tentative,
    #[value(3)]
    Confirmed,
    #[value(4)]
    Reference,
    #[value(5)]
    Degraded,
    #[value(6)]
    Rejected,
    #[value(7)]
    Undefined7,
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(2)]
pub enum HeightSource {
    #[value(0)]
    #[default]
    NoStatement,
    #[value(1)]
    Sensor,
    #[value(2)]
    Reported,
    #[value(3)]
    Estimated,
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 72)]
pub struct TrackRecord {
    #[bits(2)]
    pub word_format: u8,
    #[bits(5)]
    pub label: u8,
    #[bits(3)]
    pub sublabel: u8,
    #[bits(3)]
    pub length_code: u8,
    #[bits(1)]
    pub test_mode: bool,
    #[bits(1)]
    pub id_valid: bool,
    #[bits(1)]
    pub priority_flag: bool,
    #[bits(1)]
    pub alert_flag: bool,
    #[bits(1)]
    pub reprocess_flag: bool,
    #[bits(1)]
    pub simulated: bool,
    #[bits(19)]
    #[at(bit = 19)]
    pub record_id: RecordId,
    #[bits(4)]
    pub strength: u8,
    #[bits(2)]
    pub height_source: HeightSource,
    #[bits(13)]
    #[at(bit = 44)]
    pub altitude: Altitude25Ft,
    #[bits(1)]
    pub class_conflict: bool,
    #[bits(4)]
    pub quality: u8,
    #[bits(4)]
    pub disused: u8,
    #[bits(3)]
    #[at(bit = 66)]
    #[overlay(class_detail: ClassDetail)]
    pub class: Classification,
    #[bits(1)]
    pub flagged: bool,
    #[bits(2)]
    #[at(bit = 70)]
    pub spare_70: u8,
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 72)]
pub struct TrackExtension {
    #[bits(2)]
    pub word_format: u8,
    #[bits(21)]
    pub latitude: Latitude,
    #[bits(1)]
    pub spare_23: bool,
    #[bits(1)]
    pub spare_24: bool,
    #[bits(22)]
    #[at(bit = 25)]
    pub longitude: Longitude,
    #[bits(1)]
    pub spare_47: bool,
    #[bits(9)]
    pub course: Course,
    #[bits(11)]
    #[at(bit = 57)]
    pub speed: Speed,
    #[bits(2)]
    pub spare_68: u8,
    #[bits(2)]
    #[at(bit = 70)]
    pub spare_70: u8,
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(20)]
pub struct LatitudeMedium(pub u32);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(21)]
pub struct LongitudeMedium(pub u32);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(10)]
pub struct Altitude100FtShort(pub u16);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(6)]
pub struct Minute(pub u8);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(5)]
pub struct Hour(pub u8);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(5)]
pub struct CodeA(pub u8);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(12)]
pub struct CodeB(pub u16);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(12)]
pub struct CodeC(pub u16);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(2)]
pub struct CodeDFlags(pub u8);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(2)]
pub struct CodeEFlags(pub u8);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(6)]
pub struct PlatformCode(pub u8);

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(7)]
pub struct ActivityCode(pub u8);

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 72)]
pub struct PointInitial {
    #[bits(2)]
    pub word_format: u8,
    #[bits(5)]
    pub label: u8,
    #[bits(3)]
    pub sublabel: u8,
    #[bits(3)]
    pub length_code: u8,
    #[bits(1)]
    pub test_mode: bool,
    #[bits(1)]
    pub spare_14: bool,
    #[bits(1)]
    pub priority_flag: bool,
    #[bits(1)]
    pub aux_flag: bool,
    #[bits(1)]
    pub reprocess_flag: bool,
    #[bits(1)]
    pub simulated: bool,
    #[bits(19)]
    #[at(bit = 19)]
    pub record_id: RecordId,
    #[bits(1)]
    pub derived_flag: bool,
    #[bits(1)]
    pub continued_flag: bool,
    #[bits(2)]
    pub spare_40: u8,
    #[bits(3)]
    #[at(bit = 42)]
    pub time_function: u8,
    #[bits(2)]
    pub extent_kind: u8,
    #[bits(3)]
    pub priority: u8,
    #[bits(1)]
    pub spare_50: bool,
    #[bits(4)]
    #[at(bit = 51)]
    pub point_kind: u8,
    #[bits(4)]
    pub point_detail: u8,
    #[bits(6)]
    #[at(bit = 59)]
    pub minute: Minute,
    #[bits(5)]
    #[at(bit = 65)]
    pub hour: Hour,
    #[bits(2)]
    #[at(bit = 70)]
    pub spare_70: u8,
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 72)]
pub struct PointExtension {
    #[bits(2)]
    pub word_format: u8,
    #[bits(3)]
    pub event_kind: u8,
    #[bits(20)]
    #[at(bit = 5)]
    pub latitude: LatitudeMedium,
    #[bits(2)]
    pub event_magnitude: u8,
    #[bits(1)]
    pub spare_27: bool,
    #[bits(21)]
    #[at(bit = 28)]
    pub longitude: LongitudeMedium,
    #[bits(10)]
    #[at(bit = 49)]
    pub altitude_1: Altitude100FtShort,
    #[bits(1)]
    pub spare_59: bool,
    #[bits(10)]
    #[at(bit = 60)]
    pub altitude_2: Altitude100FtShort,
    #[bits(2)]
    #[at(bit = 70)]
    pub spare_70: u8,
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 72)]
pub struct TrackInitial {
    #[bits(2)]
    pub word_format: u8,
    #[bits(5)]
    pub label: u8,
    #[bits(3)]
    pub sublabel: u8,
    #[bits(3)]
    pub length_code: u8,
    #[bits(1)]
    pub test_mode: bool,
    #[bits(1)]
    pub id_valid: bool,
    #[bits(1)]
    pub priority_flag: bool,
    #[bits(1)]
    pub alert_flag: bool,
    #[bits(1)]
    pub reprocess_flag: bool,
    #[bits(1)]
    pub simulated: bool,
    #[bits(19)]
    #[at(bit = 19)]
    pub record_id: RecordId,
    #[bits(4)]
    pub strength: u8,
    #[bits(12)]
    #[at(bit = 42)]
    pub subject_type: u16,
    #[bits(3)]
    pub spare_54: u8,
    #[bits(1)]
    #[at(bit = 57)]
    pub class_conflict: bool,
    #[bits(4)]
    pub quality: u8,
    #[bits(4)]
    pub spare_62: u8,
    #[bits(3)]
    #[at(bit = 66)]
    pub class: Classification,
    #[bits(1)]
    pub flagged: bool,
    #[bits(2)]
    #[at(bit = 70)]
    pub spare_70: u8,
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 72)]
pub struct TrackDetail {
    #[bits(2)]
    pub word_format: u8,
    #[bits(5)]
    pub detail_label: u8,
    #[bits(1)]
    pub spare_7: bool,
    #[bits(5)]
    #[at(bit = 8)]
    pub code_a: CodeA,
    #[bits(12)]
    pub code_b: CodeB,
    #[bits(12)]
    #[at(bit = 25)]
    pub code_c: CodeC,
    #[bits(2)]
    pub code_d_flags: CodeDFlags,
    #[bits(2)]
    pub spare_39: u8,
    #[bits(6)]
    #[at(bit = 41)]
    pub platform: PlatformCode,
    #[bits(7)]
    pub activity: ActivityCode,
    #[bits(2)]
    #[at(bit = 54)]
    pub code_e_flags: CodeEFlags,
    #[bits(3)]
    pub spare_56: u8,
    #[bits(6)]
    #[at(bit = 59)]
    pub minute: Minute,
    #[bits(5)]
    #[at(bit = 65)]
    pub hour: Hour,
    #[bits(2)]
    #[at(bit = 70)]
    pub spare_70: u8,
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 72)]
pub struct TrackCovariance {
    #[bits(2)]
    pub word_format: u8,
    #[bits(5)]
    pub detail_label: u8,
    #[bits(8)]
    pub variance_xx: u8,
    #[bits(8)]
    pub variance_yy: u8,
    #[bits(8)]
    pub variance_zz: u8,
    #[bits(9)]
    #[at(bit = 31)]
    pub variance_xy: u16,
    #[bits(9)]
    pub variance_xz: u16,
    #[bits(9)]
    #[at(bit = 49)]
    pub variance_yz: u16,
    #[bits(12)]
    pub spare_58: u16,
    #[bits(2)]
    #[at(bit = 70)]
    pub spare_70: u8,
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 72)]
pub struct TrackDetailAlt {
    #[bits(2)]
    pub word_format: u8,
    #[bits(5)]
    pub detail_label: u8,
    #[bits(1)]
    pub type_extended: bool,
    #[bits(5)]
    #[at(bit = 8)]
    pub code_a: CodeA,
    #[bits(12)]
    pub code_b: CodeB,
    #[bits(12)]
    pub code_c: CodeC,
    #[bits(2)]
    #[at(bit = 37)]
    pub code_d_flags: CodeDFlags,
    #[bits(2)]
    pub spare_39: u8,
    #[bits(6)]
    #[at(bit = 41)]
    pub platform_raw: u8,
    #[bits(7)]
    pub activity_raw: u8,
    #[bits(2)]
    #[at(bit = 54)]
    pub code_e_flags: CodeEFlags,
    #[bits(3)]
    pub spare_56: u8,
    #[bits(6)]
    pub minute: Minute,
    #[bits(5)]
    #[at(bit = 65)]
    pub hour: Hour,
    #[bits(2)]
    #[at(bit = 70)]
    pub spare_70: u8,
}

const FUZZ_WORDS: usize = 10_000;

const FAMILY: &[(&str, &[layline::table::FieldDef<'static>])] = &[
    ("TrackRecord", TrackRecord::FIELDS),
    ("TrackExtension", TrackExtension::FIELDS),
    ("PointInitial", PointInitial::FIELDS),
    ("PointExtension", PointExtension::FIELDS),
    ("TrackInitial", TrackInitial::FIELDS),
    ("TrackDetail", TrackDetail::FIELDS),
    ("TrackCovariance", TrackCovariance::FIELDS),
    ("TrackDetailAlt", TrackDetailAlt::FIELDS),
];

#[test]
fn solved_offsets_match_the_hand_calculated_map() {
    let expected: &[(&str, u32, u32)] = &[
        ("word_format", 0, 2),
        ("label", 2, 5),
        ("sublabel", 7, 3),
        ("length_code", 10, 3),
        ("test_mode", 13, 1),
        ("id_valid", 14, 1),
        ("priority_flag", 15, 1),
        ("alert_flag", 16, 1),
        ("reprocess_flag", 17, 1),
        ("simulated", 18, 1),
        ("record_id", 19, 19),
        ("strength", 38, 4),
        ("height_source", 42, 2),
        ("altitude", 44, 13),
        ("class_conflict", 57, 1),
        ("quality", 58, 4),
        ("disused", 62, 4),
        ("class", 66, 3),
        ("flagged", 69, 1),
        ("spare_70", 70, 2),
    ];
    assert_eq!(TrackRecord::FIELDS.len(), expected.len());
    for (def, &(name, start, width)) in TrackRecord::FIELDS.iter().zip(expected) {
        assert_eq!(def.name, name);
        assert_eq!(
            (def.extent.start(), def.extent.width()),
            (start as u64, width),
            "field `{name}`"
        );
    }
    for (name, fields) in FAMILY {
        assert_eq!(layline::table::check_layout(fields, 72), Ok(()), "{name}");
    }
}

#[test]
fn every_field_lens_agrees_with_the_wordbits_oracle() {
    let mut rng = Rng(0x1EAF_1153);
    for _ in 0..FUZZ_WORDS {
        let mut wire = [0u8; 9];
        rng.fill(&mut wire);
        let oracle = WordBits::from_container(&wire);
        let word = layline::__private::BitWord::<72, 9>::from_wire(&wire);
        for (name, fields) in FAMILY {
            for def in *fields {
                assert_eq!(
                    def.extent.extract(word.raw()),
                    oracle.field(def.extent.start() as u8, def.extent.width() as u8),
                    "{name}.{} at {}..{} disagrees with the oracle on {wire:02X?}",
                    def.name,
                    def.extent.start(),
                    def.extent.end(),
                );
            }
        }
    }
}

#[test]
fn typed_decode_matches_the_oracle_field_by_field() {
    let mut rng = Rng(0x2FA3_9C41);
    for _ in 0..FUZZ_WORDS {
        let mut wire = [0u8; 9];
        rng.fill(&mut wire);
        let oracle = WordBits::from_container(&wire);
        let iw = TrackRecord::decode(&wire);

        assert_eq!(iw.word_format as u64, oracle.field(0, 2));
        assert_eq!(iw.label as u64, oracle.field(2, 5));
        assert_eq!(iw.sublabel as u64, oracle.field(7, 3));
        assert_eq!(iw.length_code as u64, oracle.field(10, 3));
        assert_eq!(iw.test_mode as u64, oracle.field(13, 1));
        assert_eq!(iw.record_id.to_raw(), oracle.field(19, 19));
        assert_eq!(iw.strength as u64, oracle.field(38, 4));
        assert_eq!(iw.height_source.to_raw(), oracle.field(42, 2));
        assert_eq!(iw.altitude.to_raw(), oracle.field(44, 13));
        assert_eq!(iw.quality as u64, oracle.field(58, 4));
        assert_eq!(iw.disused as u64, oracle.field(62, 4));
        assert_eq!(iw.class.to_raw(), oracle.field(66, 3));
        assert_eq!(iw.flagged as u64, oracle.field(69, 1));

        let e0 = TrackExtension::decode(&wire);
        assert_eq!(e0.latitude.to_raw(), oracle.field(2, 21));
        assert_eq!(e0.longitude.to_raw(), oracle.field(25, 22));
        assert_eq!(e0.course.to_raw(), oracle.field(48, 9));
        assert_eq!(e0.speed.to_raw(), oracle.field(57, 11));
    }
}

#[test]
fn decode_encode_round_trip_is_byte_identical() {
    let mut rng = Rng(0xB1F0);
    for _ in 0..FUZZ_WORDS {
        let mut wire = [0u8; 9];
        rng.fill(&mut wire);
        assert_eq!(TrackRecord::decode(&wire).encode(), wire);
        assert_eq!(TrackExtension::decode(&wire).encode(), wire);
        assert_eq!(PointInitial::decode(&wire).encode(), wire);
        assert_eq!(PointExtension::decode(&wire).encode(), wire);
        assert_eq!(TrackInitial::decode(&wire).encode(), wire);
        assert_eq!(TrackDetail::decode(&wire).encode(), wire);
        assert_eq!(TrackCovariance::decode(&wire).encode(), wire);
        assert_eq!(TrackDetailAlt::decode(&wire).encode(), wire);
    }
}

#[test]
fn hand_vector_label_and_sublabel_land_in_the_right_bytes() {
    let iw = TrackRecord { label: 0b00011, sublabel: 0b010, ..Default::default() };
    let wire = iw.encode();
    assert_eq!(wire[0], 0x0C);
    assert_eq!(wire[1], 0x01);
    assert_eq!(&wire[2..], &[0u8; 7]);

    let oracle = WordBits::from_container(&wire);
    assert_eq!(oracle.field(2, 5), 0b00011);
    assert_eq!(oracle.field(7, 3), 0b010);
}

#[test]
fn the_class_overlay_reads_and_writes_the_same_three_bits() {
    let mut iw = TrackRecord { class: Classification::Degraded, ..Default::default() };
    assert_eq!(iw.class_detail().to_raw(), 5);

    iw.set_class_detail(ClassDetail::from_raw(2));
    assert_eq!(iw.class, Classification::Tentative);

    let oracle = WordBits::from_container(&iw.encode());
    assert_eq!(oracle.field(66, 3), 2);
}
