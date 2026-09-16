//! A hand-written `impl Message` serves as a nested field, a switch arm and a fill element.

#![cfg(feature = "derive")]

use layline::table::{By, SegmentDef, Span, Start};
use layline::{Buffer, Choice, Message, Overflow, ParseError};

#[derive(Debug, Clone, PartialEq)]
struct Blob(Vec<u8>);

impl Message for Blob {
    const NAME: &'static str = "Blob";

    type Ctx = ();

    const SEGMENTS: &'static [SegmentDef<'static>] = &[
        SegmentDef::new("len", Start::At(0), Span::Fixed(8), None, None, &[]),
        SegmentDef::new(
            "data",
            Start::At(8),
            Span::Counted { by: By::field("len"), each: Some(8) },
            None,
            None,
            &[],
        ),
    ];

    fn decode_with_nested(
        bytes: &[u8],
        _depth: u32,
        _ctx: (),
    ) -> Result<(Self, usize), ParseError> {
        let short = |need_bytes| ParseError::Short { need_bytes, got_bytes: bytes.len(), at: 0 };
        let n = usize::from(*bytes.first().ok_or(short(1))?);
        let data = bytes.get(1..1 + n).ok_or(short(1 + n))?;
        Ok((Blob(data.to_vec()), 1 + n))
    }

    fn encode_into_with<B: Buffer>(&self, out: &mut B, _ctx: ()) -> Result<(), Overflow> {
        out.push(&[u8::try_from(self.0.len()).expect("a blob is at most 255 bytes")])?;
        out.push(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Packet {
    kind: u8,
    #[message]
    first: Blob,
    #[switch(kind)]
    body: Body,
    #[fill]
    #[message]
    rest: Vec<Blob>,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
enum Body {
    #[value(1)]
    Blob(Blob),
    #[value(2)]
    Byte(u8),
}

fn blob(s: &[u8]) -> Blob {
    Blob(s.to_vec())
}

#[test]
fn a_hand_written_message_nests_switches_and_fills() {
    let m = Packet {
        kind: 1,
        first: blob(b"ab"),
        body: Body::Blob(blob(b"xyz")),
        rest: vec![blob(b""), blob(b"q")],
    };
    let wire = m.encode();
    assert_eq!(wire, vec![1, 2, b'a', b'b', 3, b'x', b'y', b'z', 0, 1, b'q']);

    let (back, used) = Packet::decode(&wire).expect("decodes");
    assert_eq!(used, wire.len());
    assert_eq!(back, m);

    let m = Packet { kind: 0, first: blob(b""), body: Body::Byte(7), rest: vec![] };
    let wire = m.encode();
    assert_eq!(wire, vec![2, 0, 7]);
    assert_eq!(Packet::decode(&wire).expect("decodes").0, Packet { kind: 2, ..m });
}

#[test]
fn the_hand_written_refusal_is_the_parents_refusal() {
    assert_eq!(
        Packet::decode(&[1, 5, b'a']),
        Err(ParseError::Short { need_bytes: 6, got_bytes: 2, at: 1 })
    );
    assert_eq!(
        Body::decode_with(1, &[9]),
        Err(ParseError::Short { need_bytes: 10, got_bytes: 1, at: 0 }),
        "`decode_with` returns the arm's own error, with `at` counted from the arm's first byte"
    );
    assert_eq!(
        Packet::decode(&[2, 0, 7, 3, b'x']),
        Err(ParseError::Short { need_bytes: 4, got_bytes: 2, at: 3 })
    );
}

#[test]
fn the_hand_written_table_is_reachable_from_the_parent() {
    let rows = <Packet as Message>::SEGMENTS;
    let first = rows.iter().find(|r| r.name == "first").expect("a row per field");
    assert_eq!(first.decoded_by, Some("Blob"));
    assert_eq!(<Blob as Message>::SEGMENTS.len(), 2);
    assert_eq!(<Body as Choice>::ARMS[0].name, "Blob");
    const _: () = assert!(!<Blob as Message>::OPEN_ENDED);
}
