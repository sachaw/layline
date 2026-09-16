//! Words mode: 16-bit LE word sequences with bit fields inside words, nested layouts and text/word arrays.

#![cfg(feature = "derive")]

use layline::{FieldCodec, Layout};

#[derive(Layout, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[layout(words = 2)]
pub struct TwoWords {
    #[bits(16)]
    pub w1: u16,
    #[bits(16)]
    pub w2: u16,
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(words = 7)]
pub struct Sample {
    #[bytes(4)]
    pub time: TwoWords,
    #[bits(1)]
    pub reserved: bool,
    #[bits(2)]
    pub spare: u16,
    #[bits(12)]
    #[at(bit = 35)]
    pub status: u16,
    #[bits(1)]
    pub all_data: bool,
    #[bits(16)]
    pub count: u16,
    pub reserved_2: [u16; 1],
    pub name: [u8; 4],
}

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(2)]
pub struct ModeCode(pub u8);

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(words = 7, view)]
pub struct SampleV {
    #[bytes(4)]
    pub time: TwoWords,
    #[bits(1)]
    pub reserved: bool,
    #[bits(2)]
    #[overlay(mode: ModeCode)]
    pub spare: u16,
    #[bits(12)]
    pub status: u16,
    #[bits(1)]
    pub all_data: bool,
    #[bits(16)]
    pub count: u16,
    pub reserved_2: [u16; 1],
    pub name: [u8; 4],
}

#[test]
fn words_view_agrees_with_owned_and_patches_in_place() {
    let s = SampleV {
        time: TwoWords { w1: 0x4080, w2: 1 },
        status: 0x0ABC,
        all_data: true,
        count: 7,
        name: *b"NAME",
        ..Default::default()
    };
    let wire = s.encode();
    let view = SampleVView::from_wire(&wire);
    assert_eq!(view.status(), 0x0ABC);
    assert_eq!(view.time(), s.time);
    assert_eq!(view.count(), 7);
    assert_eq!(view.name(), *b"NAME");
    assert_eq!(view.decode(), s);

    fn decode_borrowed<V: layline::View>(v: &V) -> V::Owned {
        v.decode()
    }
    assert_eq!(decode_borrowed(view), s);
    assert_eq!(layline::View::as_wire(view), &wire[..]);
    assert!(<SampleVView as layline::View>::from_slice(&wire[..1]).is_none());

    let mut buf = wire;
    let v = SampleVView::from_wire_mut(&mut buf);
    v.set_status(0x123);
    assert_eq!(v.status(), 0x123);
    assert!(v.all_data(), "neighbouring bit untouched");
    assert_eq!(v.count(), 7);
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(words = 0)]
pub struct Empty {}

#[test]
fn fields_pack_lsb_first_within_le_words() {
    let s = Sample {
        time: TwoWords { w1: 0x4080, w2: 0 },
        reserved: false,
        spare: 0,
        status: 0x0ABC,
        all_data: true,
        count: 7,
        reserved_2: [0xFFFF],
        name: *b"NAME",
    };
    let wire = s.encode();
    assert_eq!(wire.len(), 14);
    assert_eq!(u16::from_le_bytes([wire[0], wire[1]]), 0x4080);
    let w3 = u16::from_le_bytes([wire[4], wire[5]]);
    assert_eq!((w3 >> 3) & 0xFFF, 0x0ABC);
    assert_eq!(w3 >> 15, 1);
    assert_eq!(&wire[10..14], b"NAME");

    assert_eq!(Sample::decode(&wire), s);
    assert_eq!(layline::table::check_layout(Sample::FIELDS, 7 * 16), Ok(()));
}

#[test]
fn a_zero_word_layout_works() {
    assert_eq!(Empty::WIRE_BYTES, 0);
    let e = Empty::decode(&[]);
    assert_eq!(e.encode(), [0u8; 0]);
    assert_eq!(Empty::decode_slice(&[]).ok(), Some(Empty {}));
    assert_eq!(Empty::decode_slice(&[0]).ok(), None);
}
