//! A byte order a holder picks at run time: `#[message(endian = needs(p))]`.
//!
//! JEL descriptor-word packets have a big-endian header whose flags byte says which order the
//! elements after it are in, and which fields each element carries.

#![cfg(feature = "derive")]

use layline::checksum::Crc;
use layline::{Layout, Message, ParseError};

type Crc16 = Crc<u16, 0x1021, 0xFFFF, 0x0000, false>;

#[path = "support/rng.rs"]
mod rng;

#[derive(Layout, Debug, Clone, Copy, PartialEq)]
#[layout(bits = 8, order = msb)]
pub struct DwFlags {
    #[bits(1)]
    pub big_endian: bool,
    #[bits(7)]
    pub spare: u8,
}

/// A fixed record nested in a held-order message keeps its own declared order.
#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 2, endian = be)]
pub struct Tag {
    pub code: u16,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(mask: u32, big_endian: bool), endian = needs(big_endian))]
pub struct Pdw {
    #[when(mask & 0x1)]
    pub toa: Option<i64>,
    #[when(mask & 0x2)]
    pub pf: Option<f64>,
    #[when(mask & 0x4)]
    #[bytes(2)]
    pub tag: Option<Tag>,
    pub n: u8,
    #[count(n)]
    pub pulses: Vec<u16>,
    #[checksum(Crc16, over = n..)]
    pub crc: u16,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct DwDataPacket {
    pub mask: u32,
    pub context_id: u8,
    #[bytes(1)]
    pub flags: DwFlags,
    pub num_elements: u16,
    #[count(num_elements)]
    #[with(mask, flags.big_endian)]
    #[message]
    pub elements: Vec<Pdw>,
}

fn pdw(mask: u32, i: u16) -> Pdw {
    Pdw {
        toa: (mask & 0x1 != 0).then_some(0x0102_0304_0506_0708 + i64::from(i)),
        pf: (mask & 0x2 != 0).then_some(1.5 + f64::from(i)),
        tag: (mask & 0x4 != 0).then_some(Tag { code: 0xA0B0 + i }),
        n: 0,
        pulses: vec![0x1122, 0x3344 + i],
        crc: 0,
    }
}

fn packet(mask: u32, big_endian: bool, n: u16) -> DwDataPacket {
    DwDataPacket {
        mask,
        context_id: 9,
        flags: DwFlags { big_endian, spare: 0 },
        num_elements: 0,
        elements: (0..n).map(|i| pdw(mask, i)).collect(),
    }
}

/// The wire of one element, by hand, in the order `big`, and its checksum.
fn element(mask: u32, i: u16, big: bool) -> (Vec<u8>, u16) {
    let mut out = Vec::new();
    let e = pdw(mask, i);
    if let Some(toa) = e.toa {
        out.extend(if big { toa.to_be_bytes() } else { toa.to_le_bytes() });
    }
    if let Some(pf) = e.pf {
        out.extend(if big { pf.to_be_bytes() } else { pf.to_le_bytes() });
    }
    if let Some(tag) = e.tag {
        out.extend(tag.code.to_be_bytes());
    }
    let covered_from = out.len();
    out.push(2);
    for p in &e.pulses {
        out.extend(if big { p.to_be_bytes() } else { p.to_le_bytes() });
    }
    let crc = <Crc16 as layline::Checksum>::compute(&out[covered_from..]);
    out.extend(if big { crc.to_be_bytes() } else { crc.to_le_bytes() });
    (out, crc)
}

fn element_wire(mask: u32, i: u16, big: bool) -> Vec<u8> {
    element(mask, i, big).0
}

fn packet_wire(mask: u32, big: bool, n: u16) -> Vec<u8> {
    let mut out = mask.to_be_bytes().to_vec();
    out.push(9);
    out.push(if big { 0x80 } else { 0x00 });
    out.extend(n.to_be_bytes());
    for i in 0..n {
        out.extend(element_wire(mask, i, big));
    }
    out
}

#[test]
fn each_order_round_trips_under_every_mask() {
    for big in [true, false] {
        for mask in 0..8 {
            let m = packet(mask, big, 3);
            let wire = m.encode();
            assert_eq!(wire, packet_wire(mask, big, 3), "mask {mask:#x}, big {big}");

            let (back, used) = DwDataPacket::decode(&wire).expect("decodes");
            assert_eq!(used, wire.len());
            let mut want = m.clone();
            want.num_elements = 3;
            for (i, e) in (0..).zip(&mut want.elements) {
                e.n = 2;
                e.crc = element(mask, i, big).1;
            }
            assert_eq!(back, want, "encode fills in the counts and checksums, mask {mask:#x}");
            assert_eq!(back.encode(), wire, "a decoded packet re-encodes to the same bytes");
        }
    }
}

#[test]
fn the_flag_alone_flips_the_elements() {
    let be = packet(0x3, true, 1).encode();
    let le = packet(0x3, false, 1).encode();
    assert_eq!(be[..5], le[..5], "the header is big-endian either way");
    assert_eq!(be[6..8], le[6..8]);
    assert_eq!(&be[8..16], &0x0102_0304_0506_0708i64.to_be_bytes());
    assert_eq!(&le[8..16], &0x0102_0304_0506_0708i64.to_le_bytes());

    // The same element bytes under the other flag fail the checksum, read in the other order.
    let mut flipped = le.clone();
    flipped[5] = 0x80;
    assert!(matches!(DwDataPacket::decode(&flipped), Err(ParseError::Checksum { .. })));
}

#[test]
fn a_nested_layout_keeps_its_own_order() {
    for big in [true, false] {
        let wire = packet(0x4, big, 1).encode();
        assert_eq!(&wire[8..10], &[0xA0, 0xB0], "`Tag` is big-endian, big {big}");
    }
}

#[test]
fn an_element_decodes_alone_under_either_context() {
    let ctx = |big| <Pdw as Message>::Ctx::new(0x1, big);
    for big in [true, false] {
        let wire = element_wire(0x1, 0, big);
        let (e, used) = Pdw::decode_with(&wire, ctx(big)).expect("decodes");
        assert_eq!(used, wire.len());
        assert_eq!(e.toa, Some(0x0102_0304_0506_0708));
        assert_eq!(e.pulses, [0x1122, 0x3344]);
        assert_eq!(e.encode_with(ctx(big)), wire);
    }
}

#[test]
fn a_checksum_read_in_the_wrong_order_is_refused() {
    let ctx = |big| <Pdw as Message>::Ctx::new(0x0, big);
    let wire = element_wire(0x0, 0, true);
    assert!(Pdw::decode_with(&wire, ctx(true)).is_ok());
    // The count byte reads the same, and the pulses and checksum swap their bytes.
    assert!(matches!(Pdw::decode_with(&wire, ctx(false)), Err(ParseError::Checksum { .. })));
}

#[test]
#[should_panic(
    expected = "optional field `pf`: whether it is `Some` is not what parameter `mask` states"
)]
fn an_element_that_disagrees_with_the_mask_is_refused() {
    let mut m = packet(0x1, true, 1);
    m.elements[0].pf = Some(2.0);
    let _ = m.encode();
}

/// A `bool` parameter is a flag a `#[when]` tests, and a `bool` field can be passed for one.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(big: bool), endian = needs(big))]
pub struct Note {
    pub a: u16,
    #[when(!big)]
    pub little_only: Option<u16>,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Notes {
    pub big: bool,
    #[with(big)]
    #[message]
    pub note: Note,
}

#[test]
fn a_flag_parameter_is_tested_and_passed() {
    let le = Notes { big: false, note: Note { a: 0x0102, little_only: Some(0x0304) } };
    assert_eq!(le.encode(), [0, 0x02, 0x01, 0x04, 0x03]);
    assert_eq!(Notes::decode(&le.encode()), Ok((le.clone(), 5)));

    let be = Notes { big: true, note: Note { a: 0x0102, little_only: None } };
    assert_eq!(be.encode(), [1, 0x01, 0x02]);
    assert_eq!(Notes::decode(&be.encode()), Ok((be.clone(), 3)));
}

#[test]
fn the_parameters_are_published() {
    use layline::table::ParamDef;
    assert_eq!(
        <Pdw as Message>::PARAMS,
        &[ParamDef::new("mask", "u32"), ParamDef::new("big_endian", "bool")]
    );
}

/// Random bytes either fail to decode, or decode to a packet that re-encodes and reads back.
#[test]
fn a_random_wire_either_errors_or_round_trips() {
    let mut r = rng::Rng(0x0DE5_C0DE);
    let mut decoded = 0;
    for i in 0..8192 {
        let len = (r.next() as usize) % 48;
        let mut wire = vec![0u8; len];
        r.fill(&mut wire);
        // Mostly short masks and counts, so some wires decode.
        if len >= 8 {
            wire[..4].copy_from_slice(&(r.next() as u32 & 0x3).to_be_bytes());
            wire[6] = 0;
            wire[7] %= 3;
        }
        let Ok((value, used)) = DwDataPacket::decode(&wire) else { continue };
        decoded += 1;
        assert!(used <= wire.len(), "wire {i}");
        let out = value.encode();
        assert_eq!(out, wire[..used], "wire {i}: the bytes read are the bytes written");
        assert_eq!(DwDataPacket::decode(&out), Ok((value, out.len())), "wire {i}");
    }
    assert!(decoded > 0, "no random wire decoded, so nothing was proven");
}
