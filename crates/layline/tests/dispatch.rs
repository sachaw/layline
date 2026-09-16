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

/// The fallback body is any type that is `From<&[u8]>` and `Deref<Target = [u8]>`,
/// so a catalogue can keep an unknown body without allocating.
mod inline_body {
    use layline::{Dispatch, Layout};

    #[derive(Layout, Debug, Clone, PartialEq)]
    #[layout(bytes = 4)]
    pub struct Ping {
        pub seq: u32,
    }

    /// A body of exactly four bytes, truncating or zero-padding so decode stays total.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Raw([u8; 4]);

    impl From<&[u8]> for Raw {
        fn from(bytes: &[u8]) -> Self {
            let mut out = [0u8; 4];
            let take = bytes.len().min(4);
            out[..take].copy_from_slice(&bytes[..take]);
            Self(out)
        }
    }

    impl core::ops::Deref for Raw {
        type Target = [u8];

        fn deref(&self) -> &[u8] {
            &self.0
        }
    }

    #[derive(Dispatch, Debug, Clone, PartialEq)]
    #[dispatch(id = u8)]
    pub enum Frame {
        #[value(1)]
        Ping(Ping),
        #[other]
        Unknown { id: u8, body: Raw },
    }

    #[test]
    fn an_unlisted_id_keeps_its_body_without_allocating() {
        let wire = [9u8, 8, 7, 6];
        let frame = Frame::decode(0xAB, &wire);
        assert_eq!(frame, Frame::Unknown { id: 0xAB, body: Raw(wire) });
        assert_eq!(frame.unlisted(), Some(0xAB));

        let mut room = [0u8; 4];
        let mut out = layline::Fixed::new(&mut room);
        frame.encode_into(&mut out).expect("four bytes");
        assert_eq!(out.into_written(), wire, "the bytes go back out as they came in");
    }

    #[test]
    fn a_listed_id_still_decodes_its_payload() {
        assert_eq!(Frame::decode(1, &1u32.to_le_bytes()), Frame::Ping(Ping { seq: 1 }));
    }
}
