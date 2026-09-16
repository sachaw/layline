//! `prefix = K`: the low K bits belong to a dispatcher.
//! Encode leaves them zero for the parent to write, or writes `prefix_value = V`.

#![cfg(feature = "derive")]

use layline::Layout;

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 72, prefix = 7)]
pub struct C1Body {
    #[bits(7)]
    pub channel_original: u8,
    #[bits(7)]
    pub relay_delay_2: u8,
    #[bits(51)]
    pub spare: u64,
}

#[test]
fn skip_region_is_left_zero_and_ignored() {
    let body = C1Body { channel_original: 0x55, relay_delay_2: 0x2A, spare: 0 };
    let wire = body.encode();
    assert_eq!(wire.len(), 9);
    assert_eq!(wire[0] & 0x7F, 0, "low 7 bits are the dispatcher's, left zero");

    let mut stamped = wire;
    stamped[0] |= (1 << 2) | 0b01;
    let back = C1Body::decode_slice(&stamped).ok().unwrap();
    assert_eq!(back, body);

    let expect = (0x55u16) << 7 | ((1 << 2) | 0b01) as u16;
    assert_eq!(u16::from_le_bytes([stamped[0], stamped[1]]) & 0x3FFF, expect);
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bits = 16, prefix = 2, prefix_value = 0b10)]
pub struct Prefixed {
    #[bits(10)]
    pub value: u16,
    #[bits(4)]
    pub spare: u8,
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bits = 16, prefix = 2)]
pub struct Unstamped {
    #[bits(10)]
    pub value: u16,
    #[bits(4)]
    pub spare: u8,
}

#[test]
fn encode_writes_the_stated_stamp() {
    let s = Prefixed { value: 0x3FF, spare: 0 };
    let wire = s.encode();
    assert_eq!(wire[0] & 0b11, 0b10, "encode writes `prefix_value` into the low bits");

    let u = Unstamped { value: 0x3FF, spare: 0 };
    assert_eq!(u.encode()[0] & 0b11, 0, "without `prefix_value` the low bits stay zero");

    assert_eq!(wire[0] & !0b11, u.encode()[0] & !0b11);
    assert_eq!(wire[1], u.encode()[1]);
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bits = 16, prefix = 2, prefix_value = 0b10)]
struct Checked {
    #[bits(14)]
    value: u16,
}

#[test]
fn a_checked_stamp_refuses_the_wrong_prefix() {
    let good = [0b0000_0110u8, 0x3F];
    let v = Checked::decode_slice(&good).expect("the prefix matches `prefix_value`");
    assert_eq!(v.value, (0x3F06 >> 2), "the fields start after the prefix bits");

    let wrong = [0b0000_0101u8, 0x3F];
    assert_eq!(
        Checked::decode_slice(&wrong),
        Err(layline::ParseError::Prefix { expected: 0b10, got: 0b01 }),
    );
}

/// A container wider than one register: encode writes the prefix through a byte window.
#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bits = 192, endian = be, order = msb, prefix = 16, prefix_value = 0x1616)]
struct WideStamped {
    #[bits(4)]
    kind: u8,
    #[bits(12)]
    count: u16,
    #[bits(32)]
    a: u32,
    #[bits(64)]
    b: u64,
    #[bits(64)]
    c: u64,
}

#[test]
fn a_wide_layout_writes_its_stamp() {
    let v = WideStamped { kind: 1, count: 7, a: 0x0102_0304, b: 0, c: 0xFFFF };
    let wire = v.encode();
    assert_eq!(&wire[..2], &[0x16, 0x16], "the prefix bytes contain `prefix_value`");
    assert_eq!(wire[2], 0x10, "`kind` is at byte 2, after the prefix");
}

#[test]
fn a_wide_checked_stamp_refuses_the_wrong_prefix() {
    let v = WideStamped { kind: 2, count: 1, a: 5, b: 6, c: 7 };
    let mut wire = v.encode();
    let back = WideStamped::decode_slice(&wire).expect("the prefix matches `prefix_value`");
    assert_eq!(back, v);

    wire[1] = 0x17;
    assert_eq!(
        WideStamped::decode_slice(&wire),
        Err(layline::ParseError::Prefix { expected: 0x1616, got: 0x1617 }),
    );
}
