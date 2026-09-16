//! Raw identifiers name fields, references, overlays and arms as the plain names would.

#![cfg(feature = "derive")]
#![allow(non_camel_case_types)]

use layline::{FieldCodec, Layout};

#[derive(Debug, Clone, Copy, PartialEq, FieldCodec)]
#[bits(4)]
pub struct Nib(u8);

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
pub struct Word {
    #[bits(4)]
    #[overlay(r#type: Nib)]
    pub r#loop: u8,
    #[bits(4)]
    pub r#match: u8,
}

#[test]
fn a_layout_with_raw_names_round_trips_through_its_overlay() {
    let mut word = Word { r#loop: 0x3, r#match: 0xA };
    assert_eq!(word.encode(), [0xA3]);
    assert_eq!(Word::decode(&[0xA3]), word);
    assert_eq!(word.r#type(), Nib(3));
    word.set_type(Nib(5));
    assert_eq!(word.r#loop, 5);
}

mod message {
    use layline::Message;

    #[derive(Debug, Clone, PartialEq, Message)]
    #[message(closed)]
    pub enum Body {
        #[value(1)]
        r#struct(u8),
        #[value(2)]
        r#enum(u16),
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    pub struct Packet {
        pub r#type: u8,
        #[switch(r#type)]
        pub r#match: Body,
        pub r#in: u8,
        #[count(r#in)]
        pub r#loop: Vec<u8>,
    }

    #[test]
    fn a_message_with_raw_names_round_trips() {
        let packet =
            Packet { r#type: 2, r#match: Body::r#enum(0x0102), r#in: 3, r#loop: vec![7, 8, 9] };
        let wire = packet.encode();
        assert_eq!(wire, [2, 0x02, 0x01, 3, 7, 8, 9]);
        assert_eq!(Packet::decode(&wire).expect("decodes"), (packet, wire.len()));
    }
}
