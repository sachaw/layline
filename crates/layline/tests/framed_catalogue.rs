//! `#[dispatch(prefix = N)]`: the id is the low N bits of every arm's layout.

#![cfg(feature = "derive")]

use layline::{Dispatch, Fixed, Layout};

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bits = 16, prefix = 3, prefix_value = 0b101)]
pub struct Ping {
    #[bits(13)]
    pub seq: u16,
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bits = 16, prefix = 3)]
pub struct Pong {
    #[bits(13)]
    pub ack: u16,
}

#[derive(Dispatch, Debug, Clone, PartialEq)]
#[dispatch(id = u8, prefix = 3)]
pub enum Frame {
    #[value(0b101)]
    Ping(Ping),
    #[value(0b011)]
    Pong(Pong),
    #[other]
    Unknown { id: u8, body: Vec<u8> },
}

/// The id is the low three bits of the frame.
fn id_of(frame: &[u8]) -> u8 {
    frame[0] & 0b111
}

#[test]
fn an_arm_that_states_its_prefix_writes_its_own_id() {
    let f = Frame::Ping(Ping { seq: 0x1FFF });
    let mut room = [0u8; 2];
    let mut wire = Fixed::new(&mut room);
    f.encode_into(&mut wire).expect("2 bytes");
    let wire = wire.into_written();
    assert_eq!(id_of(wire), 0b101, "`prefix_value` puts the id in the low three bits");

    let back = Frame::decode(id_of(wire), wire);
    assert_eq!(back, f);
    assert_eq!(back.unlisted(), None);
}

#[test]
fn a_stamped_payload_and_the_dispatcher_agree() {
    let f = Frame::Ping(Ping { seq: 7 });
    let mut room = [0u8; 2];
    let mut wire = Fixed::new(&mut room);
    f.encode_into(&mut wire).expect("2 bytes");
    assert_eq!(wire.into_written(), Ping { seq: 7 }.encode());
}

#[test]
fn an_unlisted_id_is_kept_and_named() {
    let wire = [0b111u8, 0x55];
    let back = Frame::decode(id_of(&wire), &wire);
    assert_eq!(back.unlisted(), Some(0b111));
    let mut room = [0u8; 2];
    let mut out = Fixed::new(&mut room);
    back.encode_into(&mut out).expect("2 bytes");
    assert_eq!(out.into_written(), wire, "an unlisted frame round-trips verbatim");
}

#[test]
fn a_payload_projects_back_out_of_its_placement() {
    use layline::Slot;
    let f = Frame::from(Pong { ack: 9 });
    assert_eq!(f.slot(), Some(&Pong { ack: 9 }));
    let none: Option<&Ping> = f.slot();
    assert_eq!(none, None, "`slot` returns `None` for the other variant's type");
}
