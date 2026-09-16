//! Big-endian containers, MSB0 bit addressing and signed bit fields.
//! Each is pinned against a hand-computed vector from a well-known protocol.

#![cfg(feature = "derive")]

use layline::Layout;

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq)]
#[layout(bytes = 28, endian = be)]
pub struct LcmLogHead {
    pub magic: u32,
    pub number: i64,
    pub timestamp: i64,
    pub channel_len: u32,
    pub payload_len: u32,
}

#[test]
fn lcm_log_head_matches_the_wire_format() {
    let head = LcmLogHead {
        magic: 0xEDA1_DA01,
        number: 7,
        timestamp: 1_700_000_000_000_000,
        channel_len: 5,
        payload_len: 32,
    };
    let wire = head.encode();
    assert_eq!(&wire[..4], &[0xED, 0xA1, 0xDA, 0x01]);
    assert_eq!(&wire[4..12], &7i64.to_be_bytes());
    assert_eq!(&wire[24..28], &[0, 0, 0, 32]);
    assert_eq!(LcmLogHead::decode(&wire), head);
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(words = 3, endian = be)]
pub struct Registers {
    #[bits(16)]
    pub voltage: u16,
    #[bits(16)]
    pub current: u16,
    #[bits(1)]
    pub fault: bool,
    #[bits(15)]
    pub spare: u16,
}

#[test]
fn modbus_registers_are_big_endian_words() {
    let regs = Registers { voltage: 0x1234, current: 0x00FF, fault: true, spare: 0 };
    let wire = regs.encode();
    assert_eq!(&wire[..2], &[0x12, 0x34], "register high byte first");
    assert_eq!(&wire[2..4], &[0x00, 0xFF]);
    assert_eq!(Registers::decode(&wire), regs);
}

/// The first word of an IPv4 header, declared from the most significant bit downward as RFC 791 prints it.
#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 32, endian = be, order = msb, view)]
pub struct Ipv4Word0 {
    #[bits(4)]
    pub version: u8,
    #[bits(4)]
    pub ihl: u8,
    #[bits(6)]
    pub dscp: u8,
    #[bits(2)]
    pub ecn: u8,
    #[bits(16)]
    pub total_length: u16,
}

#[test]
fn ipv4_version_and_ihl_land_in_the_famous_first_byte() {
    let w = Ipv4Word0 { version: 4, ihl: 5, dscp: 0, ecn: 0, total_length: 1500 };
    let wire = w.encode();
    assert_eq!(wire[0], 0x45);
    assert_eq!(&wire[2..4], &1500u16.to_be_bytes());
    assert_eq!(Ipv4Word0::decode(&wire), w);

    assert_eq!(layline::table::check_layout(Ipv4Word0::FIELDS, 32), Ok(()));

    let view = Ipv4Word0View::from_wire(&wire);
    assert_eq!(view.version(), 4);
    assert_eq!(view.total_length(), 1500);
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 24, endian = be, order = msb)]
pub struct Beacon {
    #[bits(4)]
    pub version: u8,
    #[bits(12)]
    pub serial: u16,
    #[bits(3)]
    pub mode: u8,
    #[bits(5)]
    pub spare: u8,
}

#[test]
fn a_be_msb0_word_puts_its_named_spare_at_the_tail() {
    let b = Beacon { version: 0x9, serial: 0x2A5, mode: 0b101, spare: 0 };

    assert_eq!(b.encode(), [0x92, 0xA5, 0xA0]);
    assert_eq!(Beacon::decode(&[0x92, 0xA5, 0xA0]), b);

    let b = Beacon { spare: 0b11001, ..b };
    assert_eq!(b.encode(), [0x92, 0xA5, 0xB9]);
    assert_eq!(Beacon::decode(&[0x92, 0xA5, 0xB9]), b);

    // FIELDS is in physical (LSB0) order, the reverse of the declaration order under `order = msb`.
    let rows: Vec<(&str, u64, u32)> =
        Beacon::FIELDS.iter().map(|f| (f.name, f.extent.start(), f.extent.width())).collect();
    assert_eq!(rows, vec![("spare", 0, 5), ("mode", 5, 3), ("serial", 8, 12), ("version", 20, 4)]);
    assert_eq!(layline::table::check_layout(Beacon::FIELDS, 24), Ok(()));
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(bits = 32)]
pub struct Deltas {
    #[bits(14)]
    pub delta_lat: i16,
    #[bits(14)]
    pub delta_lon: i16,
    #[bits(1)]
    pub valid: bool,
    #[bits(3)]
    pub spare: u8,
}

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(words = 1)]
pub struct WordDelta {
    #[bits(12)]
    pub delta: i16,
    #[bits(4)]
    pub flags: u8,
}

#[test]
fn signed_fields_sign_extend_and_round_trip() {
    for v in [-8192i16, -1, 0, 1, 8191] {
        let d = Deltas { delta_lat: v, delta_lon: -v.saturating_add(1), valid: true, spare: 0 };
        assert_eq!(Deltas::decode(&d.encode()), d, "delta {v}");
    }
    let d = Deltas { delta_lat: -1, ..Default::default() };
    let wire = d.encode();
    assert_eq!(wire[0], 0xFF);
    assert_eq!(wire[1] & 0x3F, 0x3F);

    for v in [-2048i16, -1, 0, 2047] {
        let w = WordDelta { delta: v, flags: 5 };
        assert_eq!(WordDelta::decode(&w.encode()), w, "word delta {v}");
    }
}
