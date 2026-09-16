//! Dispatch over an id-keyed catalogue, with a borrowed and an owned `#[other]` body.

#![cfg(feature = "derive")]

#[path = "support/fixtures.rs"]
mod fixtures;

use fixtures::{NavState, Sealed};
use layline::{Dispatch, Fixed, Layout, Overflow};

#[derive(Layout, Debug, Clone, PartialEq, Eq)]
#[layout(bytes = 8)]
pub struct TimePulse {
    pub week: u32,
    pub time_of_week_ms: u32,
}

#[derive(Dispatch, Debug, Clone, PartialEq)]
#[dispatch(id = u16)]
pub enum Payload<'a> {
    #[value(4)]
    NavState(NavState),
    #[value(83)]
    TimePulse(TimePulse),
    #[other]
    Unknown { id: u16, body: &'a [u8] },
}

#[derive(Dispatch, Debug, Clone, PartialEq)]
#[dispatch(id = u16)]
pub enum OwnedPayload {
    #[value(4)]
    NavState(NavState),
    #[other]
    Unknown { id: u16, body: Vec<u8> },
}

fn nav_state_bytes() -> [u8; 80] {
    let ins = NavState {
        week: 2374,
        time_of_week: 345_601.8,
        nav_status: 0x0800_0000,
        hw_status: 0,
        theta: [0.01, -0.02, 1.57],
        uvw: [12.5, -0.3, 0.1],
        lla: [-33.8688, 151.2093, 58.0],
        ned: [1.0, 2.0, -3.0],
    };
    ins.encode()
}

#[test]
fn a_known_id_with_the_right_length_dispatches() {
    let wire = nav_state_bytes();
    let payload = Payload::decode(4, &wire);
    assert!(matches!(payload, Payload::NavState(ref v) if v.week == 2374));
    assert_eq!(payload.id(), 4);

    let pulse = TimePulse { week: 2374, time_of_week_ms: 1234 }.encode();
    assert!(matches!(
        Payload::decode(83, &pulse),
        Payload::TimePulse(ref v) if v.time_of_week_ms == 1234
    ));
}

#[test]
fn an_unlisted_id_keeps_its_bytes() {
    let body = [1u8, 2, 3, 4, 5];
    let payload = Payload::decode(999, &body);
    assert_eq!(payload, Payload::Unknown { id: 999, body: &body });
    assert_eq!(payload.id(), 999);
}

#[test]
fn a_known_id_with_the_wrong_length_is_unknown_not_garbage() {
    let body = [0u8; 84];
    let payload = Payload::decode(4, &body);
    assert!(matches!(payload, Payload::Unknown { id: 4, .. }));
}

#[test]
fn encode_round_trips_known_and_unknown_alike() {
    let wire = nav_state_bytes();
    let payload = Payload::decode(4, &wire);
    let mut out = Vec::new();
    payload.encode_into(&mut out).unwrap();
    assert_eq!(out, wire);

    let raw = [9u8, 8, 7];
    let unknown = Payload::decode(700, &raw);
    out.clear();
    unknown.encode_into(&mut out).unwrap();
    assert_eq!(out, raw);

    let mut tiny = [0u8; 4];
    assert_eq!(payload.encode_into(&mut Fixed::new(&mut tiny)), Err(Overflow));
}

#[test]
fn the_owned_shape_decodes_from_a_transient_buffer() {
    let payload = {
        let wire = nav_state_bytes().to_vec();
        OwnedPayload::decode(4, &wire)
    };
    assert!(matches!(payload, OwnedPayload::NavState(_)));

    let unknown = {
        let transient = vec![1, 2, 3];
        OwnedPayload::decode(55, &transient)
    };
    assert_eq!(unknown, OwnedPayload::Unknown { id: 55, body: vec![1, 2, 3] });
}

#[derive(Dispatch, Debug, PartialEq)]
#[dispatch(id = u16)]
enum Asserted {
    #[value(2)]
    Sealed(Sealed),
    #[other]
    Unknown { id: u16, body: Vec<u8> },
}

#[test]
fn a_refused_payload_is_kept_whole() {
    let good = Sealed { signature: *b"SEAL", file_size: 272 };
    let mut room = [0u8; 8];
    let mut buf = Fixed::new(&mut room);
    layline::Layout::encode_into(&good, &mut buf).expect("fits");
    let wire = *buf.into_written().first_chunk::<8>().expect("eight bytes");

    assert_eq!(
        Asserted::decode(2, &wire),
        Asserted::Sealed(good),
        "a good body decodes to its arm"
    );

    let mut bad = wire;
    bad[0] = b'X';
    let kept = Asserted::decode(2, &bad);
    assert_eq!(
        kept,
        Asserted::Unknown { id: 2, body: bad.to_vec() },
        "a body that fails its magic check is kept as `Unknown`"
    );

    let mut out = Vec::new();
    kept.encode_into(&mut out).expect("fits");
    assert_eq!(out, bad, "encode returns the kept bytes");
}
