//! An optional last field with no presence flag on the wire.

#![cfg(feature = "derive")]

use layline::Message;

#[derive(Message, Debug, Clone, PartialEq, Eq)]
#[message(endian = be)]
pub struct Abort {
    pub file_index: u32,
    #[fill]
    pub reason: Option<u32>,
}

#[test]
fn it_is_there_when_the_body_has_room_for_it() {
    let with = [0, 0, 0, 7, 0, 0, 0, 9];
    let (got, used) = Abort::decode(&with).expect("parses");
    assert_eq!(used, 8);
    assert_eq!(got, Abort { file_index: 7, reason: Some(9) });
    assert_eq!(got.encode(), with);
}

#[test]
fn it_is_absent_when_the_message_stopped() {
    let without = [0, 0, 0, 7];
    let (got, used) = Abort::decode(&without).expect("parses");
    assert_eq!(used, 4);
    assert_eq!(got, Abort { file_index: 7, reason: None });
    assert_eq!(got.encode(), without);
}

#[test]
fn a_body_too_short_to_hold_it_reads_as_absent() {
    let ragged = [0, 0, 0, 7, 1, 2, 3];
    let (got, used) = Abort::decode(&ragged).expect("parses");
    assert_eq!(used, 4);
    assert_eq!(got.reason, None);
}
