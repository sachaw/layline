//! Framing out of the type: a length, count or presence flag just before its value is no field,
//! and one away from it is a `Stated<T>`, which encode writes and equality ignores.

#![cfg(feature = "derive")]

use std::collections::HashSet;

use layline::table::{By, Presence, Span};
use layline::{EncodeError, Message, MessageRef, ParseError, Stated};

#[path = "support/rng.rs"]
mod rng;

/// JET's string8 pair: each string is a `u8` byte length, then the text.
#[derive(Message, Debug, Clone, PartialEq, Eq, Hash, Default)]
#[message(endian = be)]
pub struct ClientId {
    #[len(prefix = u8)]
    #[text]
    pub node_id: String,
    #[len(prefix = u8)]
    #[text]
    pub name: String,
}

/// JET's header: a string8 key and a value of at most 16 KiB under a `u16` length.
#[derive(Message, Debug, Clone, PartialEq, Eq, Hash, Default)]
#[message(endian = be)]
pub struct Header {
    #[len(prefix = u8)]
    #[text]
    pub key: String,
    #[len(prefix = u16, cap = 16384)]
    pub value: Vec<u8>,
}

/// JET's type 50: the sender, a `u8` count of headers, and a blob32.
#[derive(Message, Debug, Clone, PartialEq, Eq, Default)]
#[message(endian = be)]
pub struct ReceivedMessage {
    #[message]
    pub origin: ClientId,
    #[count(prefix = u8)]
    #[message]
    pub headers: Vec<Header>,
    #[len(prefix = u32, cap = 1048576)]
    pub data: Vec<u8>,
}

/// JET's type 17: one string8.
#[derive(Message, Debug, Clone, PartialEq, Eq, Default)]
#[message(endian = be)]
pub struct NodeReady {
    #[len(prefix = u8)]
    #[text]
    pub node_id: String,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub enum Body {
    #[value(17)]
    NodeReady(NodeReady),
    #[value(50)]
    ReceivedMessage(ReceivedMessage),
    #[other]
    Unknown(Vec<u8>),
}

/// JET's frame: the size counts the whole frame, so it is the body's length plus five. Both
/// numbers sit ahead of the body, away from it, so they are `Stated`.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Frame {
    pub size: Stated<u32>,
    pub kind: Stated<u8>,
    #[switch(kind)]
    #[len(size - 5, cap = 1048571)]
    pub body: Body,
}

fn received() -> ReceivedMessage {
    ReceivedMessage {
        origin: ClientId { node_id: "node-1".into(), name: "cli".into() },
        headers: vec![
            Header { key: "a".into(), value: vec![1, 2] },
            Header { key: "bb".into(), value: vec![] },
        ],
        data: vec![0xAA; 3],
    }
}

#[test]
fn a_prefix_sits_just_before_its_value() {
    let id = ClientId { node_id: "node-1".into(), name: "cli".into() };
    let wire = id.encode().unwrap();
    assert_eq!(wire, b"\x06node-1\x03cli");
    assert_eq!(ClientId::decode(&wire).unwrap(), (id, 11));

    let m = received();
    let wire = m.encode().unwrap();
    let mut want = b"\x06node-1\x03cli\x02".to_vec();
    want.extend_from_slice(b"\x01a\x00\x02\x01\x02");
    want.extend_from_slice(b"\x02bb\x00\x00");
    want.extend_from_slice(b"\x00\x00\x00\x03\xAA\xAA\xAA");
    assert_eq!(wire, want);
    let (back, used) = ReceivedMessage::decode(&wire).unwrap();
    assert_eq!((back, used), (m, want.len()));
}

#[test]
fn a_decoded_frame_equals_the_one_built() {
    let built = Frame {
        size: Stated::default(),
        kind: Stated::default(),
        body: Body::ReceivedMessage(received()),
    };
    let wire = built.encode().unwrap();
    let size = u32::from_be_bytes(wire[..4].try_into().unwrap());
    assert_eq!((size as usize, wire[4]), (wire.len(), 50));

    let (decoded, used) = Frame::decode(&wire).unwrap();
    assert_eq!(used, wire.len());
    assert_eq!((decoded.size.get(), decoded.kind.get()), (size, 50));
    assert_eq!(decoded, built, "the stated numbers carry nothing");
    assert_eq!(decoded.encode().unwrap(), wire);

    let ready = Frame {
        size: Stated::new(9),
        kind: Stated::new(99),
        body: Body::NodeReady(NodeReady { node_id: "n".into() }),
    };
    assert_eq!(ready.encode().unwrap(), [0, 0, 0, 7, 17, 1, b'n'], "encode never reads them");
}

#[test]
fn an_open_arm_has_no_discriminant_for_a_stated_selector() {
    let (unknown, _) = Frame::decode(&[0, 0, 0, 7, 99, 1, 2]).unwrap();
    assert_eq!((unknown.kind.get(), &unknown.body), (99, &Body::Unknown(vec![1, 2])));
    assert_eq!(unknown.encode(), Err(EncodeError::Unstated { field: "kind" }));
}

#[test]
fn a_prefix_too_narrow_or_past_its_cap_is_refused() {
    let long = ClientId { node_id: "x".repeat(256), name: String::new() };
    assert_eq!(
        long.encode(),
        Err(EncodeError::Unfit { field: "node_id", value: 256, bits: 0 }),
        "a string8 holds at most 255 bytes",
    );
    let fits = ClientId { node_id: "x".repeat(255), name: String::new() };
    assert_eq!(fits.encode().unwrap().len(), 257);

    let capped = Header { key: "k".into(), value: vec![0; 16385] };
    assert_eq!(
        capped.encode(),
        Err(EncodeError::OverCap { field: "value", len: 16385, cap: 16384 })
    );

    let many = ReceivedMessage { headers: vec![Header::default(); 256], ..received() };
    assert_eq!(many.encode(), Err(EncodeError::Unfit { field: "headers", value: 256, bits: 0 }));
}

#[test]
fn decode_refuses_a_prefix_past_its_cap_or_the_bytes() {
    let mut wire = vec![1, b'k', 0x40, 0x01];
    wire.extend(vec![0; 16385]);
    assert_eq!(Header::decode(&wire), Err(ParseError::Malformed { field: "value", at: 4 }));
    assert!(matches!(Header::decode(&[1, b'k', 0, 4, 1]), Err(ParseError::Short { .. })));
    assert!(matches!(ClientId::decode(&[5, b'a']), Err(ParseError::Short { .. })));
}

/// OpenWire's loose encoding: a nullable value is a presence byte, then the value. A string is a
/// `u16` length and the text; bytes are an `i32` length; an array is an `i16` count.
#[derive(Message, Debug, Clone, PartialEq, Eq, Default)]
#[message(endian = be)]
pub struct Throwable {
    #[present]
    #[len(prefix = u16)]
    #[text]
    pub class: Option<String>,
    #[present]
    #[len(prefix = u16)]
    #[text]
    pub message: Option<String>,
}

#[derive(Message, Debug, Clone, PartialEq, Eq, Default)]
#[message(endian = be)]
pub struct ConnectionInfo {
    #[present]
    #[len(prefix = u16)]
    #[text]
    pub client_id: Option<String>,
    #[present]
    #[len(prefix = i32)]
    pub properties: Option<Vec<u8>>,
    #[present]
    #[count(prefix = i16)]
    #[message]
    pub broker_path: Option<Vec<Throwable>>,
    #[present]
    #[message]
    pub error: Option<Throwable>,
    pub manageable: bool,
}

#[test]
fn a_present_flag_precedes_the_prefix_and_the_value() {
    let info = ConnectionInfo {
        client_id: Some("ab".into()),
        properties: Some(vec![9]),
        broker_path: Some(vec![Throwable { class: None, message: Some("m".into()) }]),
        error: None,
        manageable: true,
    };
    let wire = info.encode().unwrap();
    assert_eq!(
        wire,
        [
            1, 0, 2, b'a', b'b', // client_id: present, length, text
            1, 0, 0, 0, 1, 9, // properties: present, i32 length, bytes
            1, 0, 1, 0, 1, 0, 1, b'm', // broker_path: present, i16 count, one throwable
            0,    // error: absent
            1,    // manageable
        ]
    );
    assert_eq!(ConnectionInfo::decode(&wire).unwrap(), (info, wire.len()));

    let null = ConnectionInfo::default();
    assert_eq!(null.encode().unwrap(), [0, 0, 0, 0, 0]);
    assert_eq!(ConnectionInfo::decode(&[0, 0, 0, 0, 0]).unwrap().0, null);
}

#[test]
fn a_present_flag_is_a_strict_byte() {
    assert_eq!(
        ConnectionInfo::decode(&[2, 0, 0, 0, 0]),
        Err(ParseError::Malformed { field: "client_id", at: 0 })
    );
}

#[test]
fn a_signed_prefix_refuses_a_negative_number() {
    let wire = [0, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0];
    assert_eq!(
        ConnectionInfo::decode(&wire),
        Err(ParseError::Malformed { field: "properties", at: 6 })
    );
}

#[test]
fn the_table_gives_each_prefix_and_flag_a_row() {
    let rows = <ConnectionInfo as Message>::SEGMENTS;
    let names: Vec<&str> = rows.iter().map(|r| r.name).collect();
    assert_eq!(
        names[..6],
        [
            "client_id.present",
            "client_id.len",
            "client_id",
            "properties.present",
            "properties.len",
            "properties"
        ]
    );
    assert_eq!(rows[0].span, Span::Fixed(8));
    assert_eq!(rows[1].span, Span::Fixed(16));
    assert_eq!(rows[1].when, Some(Presence::Flag { field: "client_id.present", mask: 1 }));
    assert_eq!(rows[2].span, Span::Window { by: By::field("client_id.len") });
    assert_eq!(rows[5].when, Some(Presence::Flag { field: "properties.present", mask: 1 }));

    let rows = <ReceivedMessage as Message>::SEGMENTS;
    let headers = rows.iter().position(|r| r.name == "headers").unwrap();
    assert_eq!(rows[headers - 1].name, "headers.count");
    assert!(
        matches!(rows[headers].span, Span::Counted { by, .. } if by == By::field("headers.count"))
    );
}

/// A count in a header, ahead of other fields: it is `Stated`, and the count reads it.
#[derive(Message, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Batch {
    pub n: Stated<u8>,
    pub tag: u16,
    #[count(n)]
    pub items: Vec<u16>,
}

/// A length in 4-byte words, a flag and a flags word, all stated away from what they describe.
#[derive(Message, Debug, Clone, PartialEq, Eq, Default)]
pub struct Words {
    pub words: Stated<u8>,
    pub has_note: Stated<bool>,
    pub flags: Stated<u8>,
    #[len(words * 4)]
    pub body: Vec<u8>,
    #[when(has_note)]
    pub note: Option<u16>,
    #[when(flags & 0x01)]
    pub a: Option<u8>,
    #[when(flags & 0x04 == 0)]
    pub b: Option<u8>,
}

#[test]
fn a_stated_field_is_written_by_what_reads_it() {
    let batch = Batch { tag: 7, items: vec![1, 2], ..Batch::default() };
    let wire = batch.encode().unwrap();
    assert_eq!(wire, [2, 7, 0, 1, 0, 2, 0]);
    let (back, _) = Batch::decode(&wire).unwrap();
    assert_eq!((back.n.get(), &back), (2, &batch));
    let set: HashSet<Batch> = [back, batch].into_iter().collect();
    assert_eq!(set.len(), 1, "a stated field hashes to nothing");

    let w = Words { body: vec![1; 8], note: Some(3), a: None, b: Some(5), ..Words::default() };
    let wire = w.encode().unwrap();
    assert_eq!(wire, [2, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 3, 0, 5]);
    let (back, _) = Words::decode(&wire).unwrap();
    assert_eq!(back, w);
    assert_eq!((back.words.get(), back.has_note.get(), back.flags.get()), (2, true, 0));

    let ragged = Words { body: vec![1; 3], ..Words::default() };
    assert_eq!(ragged.encode(), Err(EncodeError::Inexact { field: "words", value: 3 }));
}

#[test]
fn a_stated_flags_word_holds_only_its_presence_bits() {
    // flags 0x04 clears `b`; 0x02 is no field's.
    let (w, _) = Words::decode(&[0, 0, 0x04]).unwrap();
    assert_eq!((w.a, w.b), (None, None));
    assert_eq!(
        Words::decode(&[0, 0, 0x02, 0]),
        Err(ParseError::Malformed { field: "flags", at: 3 })
    );
}

/// An expression reads a stated field as encode writes it.
#[derive(Message, Debug, Clone, PartialEq, Default)]
pub struct Bounded {
    pub n: Stated<u8>,
    #[count(n)]
    #[valid(n <= 3)]
    pub items: Vec<u8>,
}

#[test]
fn an_expression_reads_a_stated_field_as_encode_writes_it() {
    let ok = Bounded { items: vec![1, 2, 3], ..Bounded::default() };
    assert_eq!(Bounded::decode(&ok.encode().unwrap()).unwrap().0, ok);
    let over = Bounded { items: vec![0; 4], ..Bounded::default() };
    assert_eq!(over.encode(), Err(EncodeError::Mismatch { field: "items", value: 0, stated: 1 }));
    assert_eq!(
        Bounded::decode(&[4, 0, 0, 0, 0]),
        Err(ParseError::Malformed { field: "items", at: 5 })
    );
}

#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(needs(n: u8))]
pub struct Sized {
    #[count(n)]
    pub items: Vec<u8>,
}

/// A stated field passed to a nested message is the number encode writes.
#[derive(Message, Debug, Clone, PartialEq, Default)]
pub struct Holder {
    pub n: Stated<u8>,
    #[len(n)]
    pub head: Vec<u8>,
    #[message]
    #[with(n)]
    pub tail: Sized,
}

#[test]
fn a_stated_field_passes_the_number_encode_writes() {
    let h = Holder { head: vec![1, 2], tail: Sized { items: vec![3, 4] }, ..Holder::default() };
    let wire = h.encode().unwrap();
    assert_eq!(wire, [2, 1, 2, 3, 4]);
    assert_eq!(Holder::decode(&wire).unwrap().0, h);
    let short = Holder { head: vec![1, 2], tail: Sized { items: vec![3] }, ..Holder::default() };
    assert_eq!(short.encode(), Err(EncodeError::Mismatch { field: "items", value: 1, stated: 2 }));
}

/// A switch window and a borrowed run, each sized by a prefix.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Tlv {
    pub kind: Stated<u8>,
    #[switch(kind)]
    #[len(prefix = u16)]
    pub body: Body,
}

#[derive(Message, Debug, PartialEq)]
pub struct Chunk<'a> {
    #[len(prefix = u16)]
    pub data: &'a [u8],
    #[count(prefix = u8)]
    pub more: &'a [u8],
}

#[test]
fn a_switch_and_a_borrowed_run_take_a_prefix() {
    let tlv =
        Tlv { kind: Stated::default(), body: Body::NodeReady(NodeReady { node_id: "ab".into() }) };
    let wire = tlv.encode().unwrap();
    assert_eq!(wire, [17, 3, 0, 2, b'a', b'b']);
    assert_eq!(Tlv::decode(&wire).unwrap(), (tlv, 6));
    assert_eq!(
        Tlv::decode(&[17, 4, 0, 2, b'a', b'b', 0]),
        Err(ParseError::Malformed { field: "body", at: 3 })
    );

    let chunk = Chunk { data: &[1, 2, 3], more: &[4] };
    let wire = chunk.encode().unwrap();
    assert_eq!(wire, [3, 0, 1, 2, 3, 1, 4]);
    assert_eq!(<Chunk<'_> as MessageRef<'_>>::decode(&wire).unwrap(), (chunk, 7));
}

fn text(r: &mut rng::Rng, most: u64) -> String {
    (0..r.next() % most).map(|_| char::from(b'a' + (r.next() % 26) as u8)).collect()
}

#[test]
fn random_frames_round_trip_and_compare_equal() {
    let mut r = rng::Rng(0xF4A3_1E5D_0000_0006);
    for _ in 0..2048 {
        let headers = (0..r.next() % 4)
            .map(|_| Header {
                key: text(&mut r, 6),
                value: vec![r.next() as u8; (r.next() % 5) as usize],
            })
            .collect();
        let m = ReceivedMessage {
            origin: ClientId { node_id: text(&mut r, 8), name: text(&mut r, 8) },
            headers,
            data: vec![r.next() as u8; (r.next() % 9) as usize],
        };
        let frame = Frame {
            size: Stated::default(),
            kind: Stated::default(),
            body: Body::ReceivedMessage(m),
        };
        let wire = frame.encode().unwrap();
        let (back, used) = Frame::decode(&wire).unwrap();
        assert_eq!((&back, used), (&frame, wire.len()));
        assert_eq!(back.encode().unwrap(), wire);

        let opt = |r: &mut rng::Rng| r.next().is_multiple_of(2);
        let info = ConnectionInfo {
            client_id: opt(&mut r).then(|| text(&mut r, 5)),
            properties: opt(&mut r).then(|| vec![7; (r.next() % 4) as usize]),
            broker_path: opt(&mut r).then(|| {
                (0..r.next() % 3)
                    .map(|_| Throwable {
                        class: opt(&mut r).then(|| text(&mut r, 4)),
                        message: None,
                    })
                    .collect()
            }),
            error: opt(&mut r).then(Throwable::default),
            manageable: opt(&mut r),
        };
        let wire = info.encode().unwrap();
        assert_eq!(ConnectionInfo::decode(&wire).unwrap(), (info, wire.len()));
    }
}

#[test]
fn random_bytes_decode_to_what_encodes_back() {
    let mut r = rng::Rng(0xF4A3_1E5D_0000_0007);
    let mut whole = 0;
    for _ in 0..20_000 {
        let mut buf = vec![0u8; (r.next() % 24) as usize];
        r.fill(&mut buf);
        // Mostly small bytes, so flags and lengths are often ones decode accepts.
        for b in buf.iter_mut() {
            if !r.next().is_multiple_of(4) {
                *b = (r.next() % 3) as u8;
            }
        }
        if let Ok((info, used)) = ConnectionInfo::decode(&buf) {
            assert_eq!(info.encode().unwrap(), &buf[..used]);
            whole += 1;
        }
        if let Ok((w, used)) = Words::decode(&buf) {
            assert_eq!(w.encode().unwrap(), &buf[..used]);
            whole += 1;
        }
        if let Ok((m, used)) = ReceivedMessage::decode(&buf) {
            assert_eq!(m.encode().unwrap(), &buf[..used]);
            whole += 1;
        }
    }
    assert!(whole > 100, "the corpus reaches valid bodies: {whole}");
}
