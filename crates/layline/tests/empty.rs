//! A message with no fields: `struct Ready {}` and a unit struct, alone, nested, and as an arm.

#![cfg(feature = "derive")]

use layline::{Message, ParseError};

#[path = "support/rng.rs"]
mod rng;

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Ready {}

#[derive(Debug, Clone, Copy, PartialEq, Message)]
pub struct Closed;

#[derive(Debug, Clone, PartialEq, Message)]
#[message(needs(n: u8))]
pub struct Acked {}

/// A JET-like frame: the type byte selects the body, and some bodies are empty.
#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
pub enum Body {
    #[value(1)]
    Hello(Hello),
    #[value(17)]
    Ready(Ready),
    #[value(18)]
    Closed(Closed),
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Hello {
    pub id: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Frame {
    pub kind: u8,
    #[switch(kind)]
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Holder {
    pub n: u8,
    #[message]
    pub ready: Ready,
    #[message]
    pub closed: Box<Closed>,
    #[message]
    #[with(n)]
    pub acked: Acked,
    #[count(n)]
    #[message]
    pub marks: Vec<Closed>,
    pub tail: u8,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Filled {
    #[fill]
    #[message]
    pub marks: Vec<Ready>,
}

#[test]
fn an_empty_message_is_no_bytes() {
    assert!(Ready {}.encode().is_empty());
    assert!(Closed.encode().is_empty());
    assert_eq!(Ready::decode(&[]), Ok((Ready {}, 0)));
    assert_eq!(Closed::decode(&[]), Ok((Closed, 0)));
}

#[test]
fn an_empty_message_reads_none_of_what_follows() {
    assert_eq!(Ready::decode(&[0xAA, 0xBB]), Ok((Ready {}, 0)));
    assert_eq!(Closed::decode(&[0xAA]), Ok((Closed, 0)));
}

#[test]
fn an_empty_arm_is_its_selector_alone() {
    for (body, wire) in [
        (Body::Ready(Ready {}), vec![17]),
        (Body::Closed(Closed), vec![18]),
        (Body::Hello(Hello { id: 0x0102 }), vec![1, 0x02, 0x01]),
    ] {
        let frame = Frame { kind: 0, body };
        assert_eq!(frame.encode(), wire, "{frame:?}");
        let (back, used) = Frame::decode(&wire).expect("decodes");
        assert_eq!(used, wire.len());
        assert_eq!(back.body, frame.body);
        assert_eq!(back.kind, wire[0]);
    }
}

#[test]
fn nested_empty_messages_take_no_room() {
    let holder = Holder {
        n: 0,
        ready: Ready {},
        closed: Box::new(Closed),
        acked: Acked {},
        marks: vec![Closed; 3],
        tail: 0x5A,
    };
    let wire = holder.encode();
    assert_eq!(wire, [3, 0x5A]);
    assert_eq!(Holder::decode(&wire), Ok((Holder { n: 3, ..holder }, 2)));
}

#[test]
fn a_fill_of_empty_messages_refuses_a_byte_it_cannot_consume() {
    assert_eq!(Filled::decode(&[]), Ok((Filled { marks: Vec::new() }, 0)));
    assert!(matches!(Filled::decode(&[0]), Err(ParseError::Malformed { at: 0, .. })));
}

#[test]
fn random_wires_under_an_empty_message_decode_to_it_and_consume_nothing() {
    let mut r = rng::Rng(0x0E77);
    for _ in 0..1024 {
        let mut wire = vec![0u8; (r.next() % 32) as usize];
        r.fill(&mut wire);
        assert_eq!(Ready::decode(&wire), Ok((Ready {}, 0)));
        assert_eq!(Closed::decode(&wire), Ok((Closed, 0)));
    }
}
