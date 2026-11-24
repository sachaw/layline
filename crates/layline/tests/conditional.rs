//! `if c { a } else { b }` in the one expression language: a switch may select on two fields, as
//! a DW packet's marker bit or payload type 127 selects its described bytes, and a conditional
//! that works back through either branch to one field is written, while any other is checked.

#![cfg(feature = "derive")]

use layline::{EncodeError, Layout, Message, ParseError, Stated};

#[path = "support/rng.rs"]
mod rng;

/// The arm a marked packet, or payload type 127, selects: past any 7-bit payload type.
const DESCRIBED: u8 = 128;

/// RTP's second byte: the marker bit and the payload type.
#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bits = 8, order = msb)]
pub struct Head {
    #[bits(1)]
    pub marker: bool,
    #[bits(7)]
    pub pt: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
pub enum Payload {
    /// Payload type 1: PDWs.
    #[value(1)]
    Pdw(Pdws),
    /// Payload type 2: SDWs.
    #[value(2)]
    Sdw(Sdws),
    /// Marked, or payload type 127: bytes a schema read at run time describes.
    #[value(128)]
    Described(Described),
    /// Any other payload type.
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
pub struct Pdws {
    #[fill]
    pub toas: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(endian = be)]
pub struct Sdws {
    #[fill]
    pub prfs: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Described {
    #[fill]
    pub bytes: Vec<u8>,
}

/// The DW packet's shape: `marker || pt == 127 ? DESCRIBED : pt`, as one derived enum.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct DwPacket {
    #[bytes(1)]
    pub head: Head,
    #[switch(if head.marker || head.pt == 127 { DESCRIBED } else { head.pt })]
    pub payload: Payload,
}

fn packet(marker: bool, pt: u8, payload: Payload) -> DwPacket {
    DwPacket { head: Head { marker, pt }, payload }
}

fn described(bytes: &[u8]) -> Payload {
    Payload::Described(Described { bytes: bytes.to_vec() })
}

#[test]
fn a_selector_over_two_fields_picks_the_arm() {
    let pdws = Payload::Pdw(Pdws { toas: vec![0x0102_0304] });
    for (p, wire) in [
        (packet(false, 1, pdws.clone()), vec![0x01, 1, 2, 3, 4]),
        (packet(false, 2, Payload::Sdw(Sdws { prfs: vec![0x0A0B] })), vec![0x02, 0x0A, 0x0B]),
        (packet(true, 1, described(&[1, 2, 3, 4])), vec![0x81, 1, 2, 3, 4]),
        (packet(false, 127, described(&[9])), vec![0x7F, 9]),
        (packet(true, 127, described(&[])), vec![0xFF]),
        (packet(false, 5, Payload::Unknown(vec![9, 9])), vec![0x05, 9, 9]),
    ] {
        assert_eq!(p.encode().unwrap(), wire, "{p:?}");
        assert_eq!(DwPacket::decode(&wire), Ok((p, wire.len())));
    }
}

#[test]
fn encode_checks_the_header_against_the_arm() {
    // The selector reads two fields, so it writes neither: the header must already select the arm.
    let marked_pdws = packet(true, 1, Payload::Pdw(Pdws { toas: vec![] }));
    assert_eq!(
        marked_pdws.encode(),
        Err(EncodeError::Mismatch { field: "payload", value: 1, stated: 128 })
    );
    let unmarked = packet(false, 3, described(&[]));
    assert_eq!(
        unmarked.encode(),
        Err(EncodeError::Mismatch { field: "payload", value: 128, stated: 3 })
    );
    let claimed = packet(false, 2, Payload::Unknown(vec![]));
    assert_eq!(claimed.encode(), Err(EncodeError::Claimed { field: "payload", value: 2 }));
    let marked_unknown = packet(true, 5, Payload::Unknown(vec![]));
    assert_eq!(marked_unknown.encode(), Err(EncodeError::Claimed { field: "payload", value: 128 }));
}

/// A count that is words or bytes as the holder says: either branch works back to `n`.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(needs(wide: bool))]
pub struct Run {
    pub n: Stated<u8>,
    #[count(if wide { n * 2 } else { n })]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Runs {
    pub wide: bool,
    #[with(wide)]
    #[message]
    pub run: Run,
}

fn run(wide: bool, bytes: &[u8]) -> Runs {
    Runs { wide, run: Run { n: Stated::default(), bytes: bytes.to_vec() } }
}

#[test]
fn a_conditional_invertible_in_one_field_writes_it_through_either_branch() {
    assert_eq!(run(false, &[7, 8, 9]).encode().unwrap(), [0, 3, 7, 8, 9]);
    assert_eq!(run(true, &[7, 8, 9, 10]).encode().unwrap(), [1, 2, 7, 8, 9, 10]);
    assert_eq!(Runs::decode(&[1, 2, 7, 8, 9, 10]).unwrap().0, run(true, &[7, 8, 9, 10]));
    assert_eq!(run(true, &[7, 8, 9]).encode(), Err(EncodeError::Inexact { field: "n", value: 3 }));
    assert_eq!(
        run(false, &[0; 256]).encode(),
        Err(EncodeError::Unfit { field: "n", value: 256, bits: 0 })
    );
}

/// A length chosen by a field of the same message: the test reads a field, so encode checks it.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Either {
    pub long: bool,
    pub short_len: u8,
    pub long_len: u16,
    #[count(if long { long_len } else { short_len })]
    pub bytes: Vec<u8>,
}

#[test]
fn a_conditional_over_several_fields_is_checked() {
    let short = Either { long: false, short_len: 2, long_len: 0, bytes: vec![1, 2] };
    let wire = short.encode().unwrap();
    assert_eq!(wire, [0, 2, 0, 0, 1, 2]);
    assert_eq!(Either::decode(&wire), Ok((short.clone(), wire.len())));
    let long = Either { long: true, ..short.clone() };
    assert_eq!(long.encode(), Err(EncodeError::Mismatch { field: "bytes", value: 2, stated: 0 }));
    assert_eq!(Either::decode(&[1, 9, 1, 0, 7]).unwrap().0.bytes, [7]);
}

/// Random bytes: every packet decodes to the arm its header selects, or is refused, and encodes
/// back to the same bytes.
#[test]
fn random_headers_select_the_arm_they_state() {
    let mut r = rng::Rng(0xD3_5C41_BED0);
    for _ in 0..10_000 {
        let mut wire = [0u8; 7];
        r.fill(&mut wire);
        let len = 1 + (r.next() % 7) as usize;
        let wire = &wire[..len];
        let (marker, pt) = (wire[0] & 0x80 != 0, wire[0] & 0x7F);
        match DwPacket::decode(wire) {
            Ok((p, used)) => {
                assert_eq!(used, len);
                let arm = match p.payload {
                    Payload::Pdw(_) => 1,
                    Payload::Sdw(_) => 2,
                    Payload::Described(_) => 128,
                    Payload::Unknown(_) => pt,
                };
                assert_eq!(arm, if marker || pt == 127 { DESCRIBED } else { pt });
                assert_eq!(p.encode().unwrap(), wire);
            }
            Err(e) => {
                let ragged = (pt == 1 && !(len - 1).is_multiple_of(4))
                    || (pt == 2 && !(len - 1).is_multiple_of(2));
                assert!(!marker && ragged, "{wire:?}: {e}");
                assert!(matches!(e, ParseError::Malformed { .. } | ParseError::Short { .. }));
            }
        }
    }
}
