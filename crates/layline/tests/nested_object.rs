//! A message that holds itself through other types, as OpenWire's nested object does: an object
//! is a type byte and the fields it selects, and a field may be another object, or an array of
//! them. Each one's constants are searched across the cycle, and `depth` bounds the nesting.
//! Bytes follow ActiveMQ's loose encoding: big-endian, a presence byte before each nullable value.

#![cfg(feature = "derive")]

use layline::{EncodeError, MAX_NESTING_DEPTH, Message, ParseError, Stated};

#[path = "support/rng.rs"]
mod rng;

/// An object: its type byte, then its fields.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be, depth = 16)]
pub struct Object {
    pub kind: Stated<u8>,
    #[switch(kind)]
    pub body: Body,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be, closed)]
pub enum Body {
    #[value(110)]
    MessageId(Box<MessageId>),
    #[value(120)]
    ConnectionId(ConnectionId),
    #[value(21)]
    Dispatch(Box<Dispatch>),
}

/// A nullable nested object: a presence byte, then the object.
#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(endian = be)]
pub struct Nested {
    #[present]
    #[message]
    pub object: Option<Box<Object>>,
}

/// A nullable array of nested objects: a presence byte, an `i16` count, then each.
#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(endian = be)]
pub struct NullArray {
    #[present]
    #[count(prefix = i16)]
    #[message]
    pub items: Option<Vec<Nested>>,
}

#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(endian = be)]
pub struct ConnectionId {
    #[present]
    #[len(prefix = u16)]
    #[text]
    pub value: Option<String>,
}

/// Ends with a nested object, so its open end is the object's, which is its own in turn.
#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(endian = be)]
pub struct MessageId {
    pub producer_sequence_id: i64,
    #[message]
    pub producer_id: Nested,
}

#[derive(Message, Debug, Clone, PartialEq, Default)]
#[message(endian = be)]
pub struct Dispatch {
    pub command_id: i32,
    #[message]
    pub consumer_id: Nested,
    #[message]
    pub broker_path: NullArray,
    #[message]
    pub message: Nested,
}

/// A cycle that holds a refusal: `Rated` refuses a value past its range, and every type on the
/// cycle refuses with it, wherever the search starts.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Tree {
    pub kind: Stated<u8>,
    #[switch(kind)]
    pub node: Node,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be, closed)]
pub enum Node {
    #[value(0)]
    Leaf(Rated),
    #[value(1)]
    Branch(Box<Branch>),
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Rated {
    #[range(0..=9)]
    pub score: u8,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Branch {
    #[count(prefix = u8)]
    #[message]
    pub children: Vec<Tree>,
}

/// Two types that hold each other and declare nothing a value can break: neither refuses, so
/// encode returns bare bytes, as it would for a type that holds no message.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Ping {
    pub seq: u16,
    #[present]
    #[message]
    pub pong: Option<Box<Pong>>,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(endian = be)]
pub struct Pong {
    pub seq: u16,
    #[present]
    #[message]
    pub ping: Option<Box<Ping>>,
}

fn object(body: Body) -> Nested {
    Nested { object: Some(Box::new(Object { kind: Stated::default(), body })) }
}

fn connection(id: &str) -> Nested {
    object(Body::ConnectionId(ConnectionId { value: Some(id.into()) }))
}

/// A message id whose producer is a message id, `depth` levels down to a connection id.
fn chain(depth: usize) -> Nested {
    (0..depth).fold(connection("c"), |below, i| {
        object(Body::MessageId(Box::new(MessageId {
            producer_sequence_id: i as i64,
            producer_id: below,
        })))
    })
}

#[test]
fn the_constants_of_a_cycle_compile_and_are_exact() {
    const _: () = assert!(!<Object as Message>::OPEN_ENDED);
    const _: () = assert!(!<Nested as Message>::OPEN_ENDED);
    const _: () = assert!(!<MessageId as Message>::OPEN_ENDED);
    const _: () = assert!(!<Body as layline::Choice>::OPEN_ENDED);
    // Nothing on the OpenWire cycle declares a refusal but the object's `Stated` type byte and
    // the array's count, which a value can outgrow.
    const _: () = assert!(<Object as Message>::ENCODE_FALLIBLE);
    // A connection id holds no message and refuses only a string past its `u16` length.
    const _: () = assert!(<ConnectionId as Message>::ENCODE_FALLIBLE);
    const _: () = assert!(<Tree as Message>::ENCODE_FALLIBLE);
    const _: () = assert!(<Branch as Message>::ENCODE_FALLIBLE);
    assert_eq!(<Object as Message>::MAX_DEPTH, 16);
    assert_eq!(<Nested as Message>::MAX_DEPTH, MAX_NESTING_DEPTH);
}

#[test]
fn a_cycle_that_refuses_nothing_encodes_infallibly() {
    const _: () =
        assert!(!<Ping as Message>::ENCODE_FALLIBLE && !<Pong as Message>::ENCODE_FALLIBLE);
    let ping = Ping {
        seq: 1,
        pong: Some(Box::new(Pong { seq: 2, ping: Some(Box::new(Ping { seq: 3, pong: None })) })),
    };
    let wire: Vec<u8> = ping.encode();
    assert_eq!(wire, [0, 1, 1, 0, 2, 1, 0, 3, 0]);
    assert_eq!(Ping::decode(&wire).unwrap(), (ping, wire.len()));
}

#[test]
fn a_dispatch_round_trips_to_the_loose_encoding() {
    let dispatch = object(Body::Dispatch(Box::new(Dispatch {
        command_id: 7,
        consumer_id: connection("ab"),
        broker_path: NullArray { items: Some(vec![Nested::default(), connection("z")]) },
        message: Nested::default(),
    })));
    #[rustfmt::skip]
    let wire = vec![
        1, 21,                          // present, a MessageDispatch
        0, 0, 0, 7,                     // command id
        1, 120, 1, 0, 2, b'a', b'b',    // consumer id: present, ConnectionId, present, "ab"
        1, 0, 2,                        // broker path: present, two
        0,                              //   null
        1, 120, 1, 0, 1, b'z',          //   ConnectionId "z"
        0,                              // message: null
    ];
    assert_eq!(dispatch.encode().unwrap(), wire);
    assert_eq!(Nested::decode(&wire).unwrap(), (dispatch, wire.len()));
}

#[test]
fn a_chain_within_the_bound_round_trips_and_one_past_it_is_refused() {
    // A level is three messages, a nullable, its object and the arm's body, so the objects sit
    // at depths 1, 4, 7, .. and the sixth, at 16, is the last the bound allows.
    let deepest = 5;
    let ok = chain(deepest);
    let wire = ok.encode().unwrap();
    assert_eq!(Nested::decode(&wire).unwrap(), (ok, wire.len()));

    let wire = chain(deepest + 1).encode().unwrap();
    assert_eq!(Nested::decode(&wire), Err(ParseError::TooDeep { limit: 16 }));
}

#[test]
fn a_hostile_chain_of_presence_bytes_stops_at_the_bound() {
    // Present, MessageId, a sequence id, present, MessageId, ...: never ends, and never grows
    // past the bound.
    let mut wire = Vec::new();
    for _ in 0..10_000 {
        wire.extend_from_slice(&[1, 110, 0, 0, 0, 0, 0, 0, 0, 0]);
    }
    assert_eq!(Nested::decode(&wire), Err(ParseError::TooDeep { limit: 16 }));
}

#[test]
fn a_refusal_anywhere_on_the_cycle_reaches_the_top() {
    let tree = Tree {
        kind: Stated::default(),
        node: Node::Branch(Box::new(Branch {
            children: vec![Tree { kind: Stated::default(), node: Node::Leaf(Rated { score: 12 }) }],
        })),
    };
    assert!(matches!(
        tree.encode(),
        Err(EncodeError::OutOfRange { field: "score", value: 12, .. })
    ));
}

#[test]
fn random_bytes_decode_to_what_encodes_back() {
    let mut r = rng::Rng(0x0B1E_C700_0000_0001);
    let mut whole = 0;
    for _ in 0..20_000 {
        let mut buf = vec![0u8; (r.next() % 40) as usize];
        r.fill(&mut buf);
        // Mostly presence bytes and the catalogue's type bytes, so bodies often parse.
        for b in buf.iter_mut() {
            match r.next() % 6 {
                0 | 1 => *b = (r.next() % 2) as u8,
                2 => *b = [110, 120, 21][(r.next() % 3) as usize],
                3 => *b = 0,
                _ => {}
            }
        }
        if let Ok((nested, used)) = Nested::decode(&buf) {
            assert_eq!(nested.encode().unwrap(), &buf[..used]);
            whole += 1;
        }
    }
    assert!(whole > 1000, "the corpus reaches valid bodies: {whole}");
}
