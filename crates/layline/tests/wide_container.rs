//! Bit-addressed containers wider than one register.
//! A field is read from the bytes its bit range touches; every wire vector is computed by hand.
//! Under `order = msb` a declared position mirrors over the whole container: `p = bits - start - width`.

#![cfg(feature = "derive")]

use layline::{Layout, View};

macro_rules! quad {
    ($name:ident, $view:ident, $($attr:tt)*) => {
        #[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
        #[layout(bits = 192, view $($attr)*)]
        pub struct $name {
            #[bits(3)] pub a: u8,
            #[bits(13)] pub b: u16,
            #[bits(5)] pub c: u8,
            #[bits(33)] pub d: u64,
            #[bits(7)] pub e: u8,
            #[bits(64)] pub f: u64,
            #[bits(2)] pub g: u8,
            #[bits(1)] pub h: bool,
            #[bits(64)] pub i: u64,
        }

        impl $name {
            fn sample() -> Self {
                Self {
                    a: 0b101,
                    b: 0x1ABC,
                    c: 0b10011,
                    d: 0x1_2345_6789,
                    e: 0b101_0101,
                    f: 0x0123_4567_89AB_CDEF,
                    g: 0b11,
                    h: true,
                    i: 0xFEDC_BA98_7654_3210,
                }
            }
        }
    };
}

quad!(QuadLe, QuadLeView,);
quad!(QuadBe, QuadBeView, , endian = be);
quad!(QuadLeMsb, QuadLeMsbView, , order = msb);
quad!(QuadBeMsb, QuadBeMsbView, , endian = be, order = msb);

const QUAD_LE: [u8; 24] = [
    0xE5, 0xD5, 0x33, 0xF1, 0xAC, 0x68, 0x64, 0xF5, 0xBD, 0x79, 0x35, 0xF1, 0xAC, 0x68, 0x24, 0xE0,
    0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE,
];

const QUAD_BE: [u8; 24] = [
    0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10, 0xE0, 0x24, 0x68, 0xAC, 0xF1, 0x35, 0x79, 0xBD,
    0xF5, 0x64, 0x68, 0xAC, 0xF1, 0x33, 0xD5, 0xE5,
];

const QUAD_LE_MSB: [u8; 24] = [
    0x10, 0x32, 0x54, 0x76, 0x98, 0xBA, 0xDC, 0xFE, 0x7F, 0x6F, 0x5E, 0x4D, 0x3C, 0x2B, 0x1A, 0x09,
    0xA8, 0x26, 0x9E, 0x15, 0x8D, 0x9C, 0xBC, 0xBA,
];

const QUAD_BE_MSB: [u8; 24] = [
    0xBA, 0xBC, 0x9C, 0x8D, 0x15, 0x9E, 0x26, 0xA8, 0x09, 0x1A, 0x2B, 0x3C, 0x4D, 0x5E, 0x6F, 0x7F,
    0xFE, 0xDC, 0xBA, 0x98, 0x76, 0x54, 0x32, 0x10,
];

#[test]
fn a_192_bit_container_matches_hand_computed_wire_bytes() {
    assert_eq!(QuadLe::sample().encode(), QUAD_LE);
    assert_eq!(QuadBe::sample().encode(), QUAD_BE);
    assert_eq!(QuadLeMsb::sample().encode(), QUAD_LE_MSB);
    assert_eq!(QuadBeMsb::sample().encode(), QUAD_BE_MSB);

    assert_eq!(QuadLe::decode(&QUAD_LE), QuadLe::sample());
    assert_eq!(QuadBe::decode(&QUAD_BE), QuadBe::sample());
    assert_eq!(QuadLeMsb::decode(&QUAD_LE_MSB), QuadLeMsb::sample());
    assert_eq!(QuadBeMsb::decode(&QUAD_BE_MSB), QuadBeMsb::sample());
}

#[test]
fn a_wide_container_round_trips_on_arbitrary_bytes() {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    for _ in 0..2_000 {
        let mut wire = [0u8; 24];
        for chunk in wire.chunks_mut(8) {
            chunk.copy_from_slice(&next().to_le_bytes());
        }
        assert_eq!(QuadLe::decode(&wire).encode(), wire, "le/lsb on {wire:02X?}");
        assert_eq!(QuadBe::decode(&wire).encode(), wire, "be/lsb on {wire:02X?}");
        assert_eq!(QuadLeMsb::decode(&wire).encode(), wire, "le/msb on {wire:02X?}");
        assert_eq!(QuadBeMsb::decode(&wire).encode(), wire, "be/msb on {wire:02X?}");
    }
    assert_eq!(QuadBeMsb::decode(&[0xFF; 24]).encode(), [0xFF; 24]);
}

#[test]
fn a_wide_container_publishes_a_table_that_tiles() {
    for (name, fields) in [
        ("le/lsb", QuadLe::FIELDS),
        ("be/lsb", QuadBe::FIELDS),
        ("le/msb", QuadLeMsb::FIELDS),
        ("be/msb", QuadBeMsb::FIELDS),
    ] {
        assert_eq!(layline::table::check_layout(fields, 192), Ok(()), "{name}");
        assert_eq!(fields.len(), 9, "{name}");
    }
    let rows: Vec<(&str, u64, u32)> =
        QuadLe::FIELDS.iter().map(|f| (f.name, f.extent.start(), f.extent.width())).collect();
    assert_eq!(rows.last(), Some(&("i", 128, 64)));

    let msb: Vec<(&str, u64)> =
        QuadBeMsb::FIELDS.iter().map(|f| (f.name, f.extent.start())).collect();
    assert_eq!(msb.first(), Some(&("i", 0)));
    assert_eq!(msb.last(), Some(&("a", 189)));
}

#[test]
fn a_view_over_a_wide_container_reads_the_same_bits_as_decode() {
    let view = QuadBeMsbView::from_wire(&QUAD_BE_MSB);
    let owned = QuadBeMsb::sample();
    assert_eq!(view.a(), owned.a);
    assert_eq!(view.d(), owned.d);
    assert_eq!(view.f(), owned.f);
    assert_eq!(view.i(), owned.i);
    assert_eq!(view.decode(), owned);

    let mut wire = QUAD_BE_MSB;
    let view = QuadBeMsbView::from_wire_mut(&mut wire);
    view.set_f(0);
    assert_eq!(view.f(), 0);
    let patched = view.decode();
    assert_eq!(patched, QuadBeMsb { f: 0, ..owned });
    assert_eq!(
        wire[..8],
        QUAD_BE_MSB[..8]
            .iter()
            .enumerate()
            .map(|(k, b)| if k == 7 { b & 0xF8 } else { *b })
            .collect::<Vec<_>>()[..]
    );
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 304, endian = be, order = msb)]
pub struct NavSubframe {
    #[bits(8)]
    #[at(bit = 0)]
    pub preamble: u8,
    #[bits(14)]
    pub tlm_message: u16,
    #[bits(1)]
    pub integrity: bool,
    #[bits(1)]
    pub reserved: bool,
    #[bits(17)]
    pub tow_count: u32,
    #[bits(1)]
    pub alert: bool,
    #[bits(1)]
    pub anti_spoof: bool,
    #[bits(3)]
    pub subframe_id: u8,
    #[bits(64)]
    pub ephemeris: u64,
    #[bits(48)]
    pub clock_bias: u64,
    #[bits(37)]
    pub correction: u64,
    #[bits(8)]
    pub iode: u8,
    #[bits(16)]
    pub crs: u16,
    #[bits(16)]
    pub delta_n: u16,
    #[bits(32)]
    pub m0: u32,
    #[bits(16)]
    pub cuc: u16,
    #[bits(17)]
    pub eccentricity: u32,
    #[bits(4)]
    pub pad: u8,
}

#[test]
fn a_304_bit_subframe_puts_its_preamble_in_the_first_byte() {
    let sf = NavSubframe {
        preamble: 0x8B,
        tlm_message: 0x2A5B,
        subframe_id: 5,
        tow_count: 0x1_2345,
        ephemeris: 0xDEAD_BEEF_CAFE_F00D,
        pad: 0,
        ..Default::default()
    };
    let wire = sf.encode();
    assert_eq!(wire[0], 0x8B);
    assert_eq!(wire[1], 0b1010_1001);
    assert_eq!(wire[2] >> 2, 0b011011);
    assert_eq!(NavSubframe::decode(&wire), sf);
    assert_eq!(NavSubframe::WIRE_BYTES * 8, 304);
    assert_eq!(NavSubframe::WIRE_BYTES, 38);
    assert_eq!(layline::table::check_layout(NavSubframe::FIELDS, 304), Ok(()));
}

#[test]
fn a_304_bit_subframe_round_trips_every_field_independently() {
    let base = NavSubframe::default();
    let cases: [(&str, NavSubframe); 6] = [
        ("ephemeris", NavSubframe { ephemeris: u64::MAX, ..base }),
        ("clock_bias", NavSubframe { clock_bias: (1 << 48) - 1, ..base }),
        ("correction", NavSubframe { correction: (1 << 37) - 1, ..base }),
        ("eccentricity", NavSubframe { eccentricity: (1 << 17) - 1, ..base }),
        ("pad", NavSubframe { pad: 0xF, ..base }),
        ("preamble", NavSubframe { preamble: 0xFF, ..base }),
    ];
    for (name, value) in cases {
        let wire = value.encode();
        assert_eq!(NavSubframe::decode(&wire), value, "{name} round trip");
        let ones: u32 = wire.iter().map(|b| b.count_ones()).sum();
        let want = NavSubframe::FIELDS
            .iter()
            .find(|f| f.name == name)
            .expect("named field")
            .extent
            .width();
        assert_eq!(ones, want, "{name} set bits outside its own bit range");
    }
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 304)]
pub struct NavSubframeLe {
    #[bits(8)]
    pub preamble: u8,
    #[bits(14)]
    pub tlm_message: u16,
    #[bits(64)]
    pub ephemeris: u64,
    #[bits(48)]
    pub clock_bias: u64,
    #[bits(37)]
    pub correction: u64,
    #[bits(128)]
    pub opaque: u128,
    #[bits(5)]
    pub pad: u8,
}

#[test]
fn a_304_bit_le_container_carries_a_128_bit_opaque_field() {
    let sf = NavSubframeLe {
        preamble: 0x8B,
        tlm_message: 0x2A5B,
        ephemeris: 0x0123_4567_89AB_CDEF,
        clock_bias: 0xAAAA_5555_AAAA,
        correction: 0x1F_FFFF_FFFF,
        opaque: 0xFEDC_BA98_7654_3210_0123_4567_89AB_CDEF,
        pad: 0b10101,
    };
    let wire = sf.encode();
    assert_eq!(NavSubframeLe::decode(&wire), sf);
    assert_eq!(wire[0], 0x8B);
    assert_eq!(wire[21] >> 3, (0xEF & 0x1F) as u8);
    assert_eq!(layline::table::check_layout(NavSubframeLe::FIELDS, 304), Ok(()));
    assert_eq!(NavSubframeLe::FIELDS.iter().map(|f| f.extent.width()).sum::<u32>(), 304,);
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 512, endian = be, order = msb, view)]
pub struct CanFdPayload {
    #[bits(4)]
    pub version: u8,
    #[bits(12)]
    pub source: u16,
    #[bits(20)]
    pub timestamp: u32,
    #[bits(1)]
    pub valid: bool,
    #[bits(11)]
    pub counter: u16,
    #[bits(64)]
    pub position_x: u64,
    #[bits(64)]
    pub position_y: u64,
    #[bits(64)]
    pub position_z: u64,
    #[bits(48)]
    pub velocity: u64,
    #[bits(24)]
    pub attitude: u32,
    #[bits(128)]
    pub opaque: u128,
    #[bits(64)]
    pub trailer: u64,
    #[bits(7)]
    pub spare: u8,
    #[bits(1)]
    #[at(bit = 511)]
    pub last: bool,
}

#[test]
fn a_512_bit_payload_is_one_container() {
    assert_eq!(CanFdPayload::WIRE_BYTES * 8, 512);
    assert_eq!(CanFdPayload::WIRE_BYTES, 64);
    assert_eq!(layline::table::check_layout(CanFdPayload::FIELDS, 512), Ok(()));

    let p = CanFdPayload {
        version: 0xD,
        source: 0x2A5,
        timestamp: 0x0F_1E2D,
        valid: true,
        counter: 0x7FF,
        position_x: 0x1122_3344_5566_7788,
        position_y: u64::MAX,
        position_z: 0,
        velocity: 0x0000_DEAD_BEEF,
        attitude: 0xABCDEF,
        opaque: 0x0F0E_0D0C_0B0A_0908_0706_0504_0302_0100,
        trailer: 0xCAFE_F00D_DEAD_BEEF,
        spare: 0,
        last: true,
    };
    let wire = p.encode();
    assert_eq!(&wire[..2], &[0xD2, 0xA5]);
    assert_eq!(wire[63] & 1, 1);
    assert_eq!(CanFdPayload::decode(&wire), p);

    let view = CanFdPayloadView::from_wire(&wire);
    assert_eq!(view.position_x(), p.position_x);
    assert_eq!(view.opaque(), p.opaque);
    assert!(view.last());
    let borrowed = <CanFdPayloadView as View>::from_slice(&wire[..]).expect("64 bytes");
    assert_eq!(borrowed.decode(), p);
    assert_eq!(View::as_wire(borrowed), &wire[..]);
}

#[test]
fn a_field_at_the_very_top_of_a_512_bit_container_is_one_bit_of_one_byte() {
    let only_last = CanFdPayload { last: true, ..Default::default() };
    let wire = only_last.encode();
    assert_eq!(wire[63], 0x01);
    assert_eq!(&wire[..63], &[0u8; 63]);

    let only_version = CanFdPayload { version: 0xF, ..Default::default() };
    let wire = only_version.encode();
    assert_eq!(wire[0], 0xF0);
    assert_eq!(&wire[1..], &[0u8; 63]);
}

#[test]
fn a_512_bit_payload_round_trips_arbitrary_bytes() {
    let mut state = 0x243F_6A88_85A3_08D3u64;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    for _ in 0..500 {
        let mut wire = [0u8; 64];
        for chunk in wire.chunks_mut(8) {
            chunk.copy_from_slice(&next().to_le_bytes());
        }
        assert_eq!(CanFdPayload::decode(&wire).encode(), wire, "on {wire:02X?}");
    }
}

macro_rules! span17 {
    ($name:ident, $($attr:tt)*) => {
        #[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
        #[layout(bits = 200 $($attr)*)]
        pub struct $name {
            #[bits(3)] pub head: u8,
            #[bits(128)] pub blob: u128,
            #[bits(64)] pub tail_a: u64,
            #[bits(5)] pub tail_b: u8,
        }
    };
}

span17!(Span17Le,);
span17!(Span17Be, , endian = be);

const SPAN17_LE: [u8; 25] = [
    0xC5, 0x4B, 0xD3, 0x5A, 0xE2, 0x69, 0xF1, 0x78, 0x80, 0x90, 0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6,
    0x7F, 0x6F, 0x5E, 0x4D, 0x3C, 0x2B, 0x1A, 0x09, 0xB0,
];

#[test]
fn a_128_bit_field_at_a_bit_offset_spans_seventeen_bytes() {
    let v = Span17Le {
        head: 0b101,
        blob: 0xFEDC_BA98_7654_3210_0F1E_2D3C_4B5A_6978,
        tail_a: 0x0123_4567_89AB_CDEF,
        tail_b: 0b10110,
    };
    assert_eq!(v.encode(), SPAN17_LE);
    assert_eq!(Span17Le::decode(&SPAN17_LE), v);

    let mut reversed = SPAN17_LE;
    reversed.reverse();
    let be = Span17Be { head: v.head, blob: v.blob, tail_a: v.tail_a, tail_b: v.tail_b };
    assert_eq!(be.encode(), reversed);
    assert_eq!(Span17Be::decode(&reversed), be);

    let cleared = Span17Le { blob: 0, ..v };
    let wire = cleared.encode();
    assert_eq!(wire[0], 0b101);
    assert_eq!(&wire[1..16], &[0u8; 15]);
    assert_eq!(wire[16], SPAN17_LE[16] & 0xF8);
    assert_eq!(Span17Le::decode(&wire), cleared);

    let full = Span17Le { head: 0, blob: u128::MAX, tail_a: 0, tail_b: 0 };
    let ones: u32 = full.encode().iter().map(|b| b.count_ones()).sum();
    assert_eq!(ones, 128);
    assert_eq!(Span17Le::decode(&full.encode()), full);

    assert_eq!(layline::table::check_layout(Span17Le::FIELDS, 200), Ok(()));
    assert_eq!(layline::table::check_layout(Span17Be::FIELDS, 200), Ok(()));
}

#[test]
fn a_seventeen_byte_field_round_trips_arbitrary_bytes() {
    let mut state = 0xB504_F333_F9DE_6484u64;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    for _ in 0..1_000 {
        let mut wire = [0u8; 25];
        for chunk in wire.chunks_mut(8) {
            let n = chunk.len();
            chunk.copy_from_slice(&next().to_le_bytes()[..n]);
        }
        assert_eq!(Span17Le::decode(&wire).encode(), wire, "le on {wire:02X?}");
        assert_eq!(Span17Be::decode(&wire).encode(), wire, "be on {wire:02X?}");
    }
}

/// 128 bits: the widest container that fits in one `u128`.
#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 128, endian = be, order = msb)]
pub struct Hoisted {
    #[bits(13)]
    pub a: u16,
    #[bits(51)]
    pub b: u64,
    #[bits(3)]
    pub c: u8,
    #[bits(61)]
    pub d: u64,
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 136, endian = be, order = msb)]
pub struct Windowed {
    #[bits(13)]
    pub a: u16,
    #[bits(51)]
    pub b: u64,
    #[bits(3)]
    pub c: u8,
    #[bits(61)]
    pub d: u64,
    #[bits(8)]
    pub extra: u8,
}

#[test]
fn the_hoist_threshold_changes_no_answer() {
    let h = Hoisted { a: 0x1ABC, b: 0x7_1234_5678_9ABC, c: 0b101, d: 0x1FED_CBA9_8765_4321 };
    let w = Windowed { a: h.a, b: h.b, c: h.c, d: h.d, extra: 0x5A };
    assert_eq!(&w.encode()[..16], &h.encode()[..]);
    assert_eq!(w.encode()[16], 0x5A);
    assert_eq!(Hoisted::decode(&h.encode()), h);
    assert_eq!(Windowed::decode(&w.encode()), w);

    assert_eq!(layline::table::check_layout(Hoisted::FIELDS, 128), Ok(()));
    assert_eq!(layline::table::check_layout(Windowed::FIELDS, 136), Ok(()));
}

#[test]
fn a_wide_container_still_refuses_a_value_that_does_not_fit() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = std::panic::catch_unwind(|| {
        let _ = QuadLe { c: 200, ..QuadLe::default() }.encode();
    });
    std::panic::set_hook(hook);
    assert!(caught.is_err(), "a 192-bit container accepted 200 in a 5-bit field");
}
