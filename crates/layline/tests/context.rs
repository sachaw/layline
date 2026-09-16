//! A message that reads a value from its containing message: `#[message(needs(..))]` and `#[with(..)]`.

#![cfg(feature = "derive")]

use layline::table::{By, ParamDef, SegmentDef, Span, Start};
use layline::{Buffer, Message, Overflow, ParseError};

#[path = "support/rng.rs"]
mod rng;

/// A hand-written [`Message`] with its own `Ctx`.
#[derive(Debug, Clone, PartialEq)]
struct HandBlock(Vec<u8>);

#[derive(Debug, Clone, Copy, PartialEq)]
struct HandBlockCtx {
    stride: u8,
}

impl Message for HandBlock {
    const NAME: &'static str = "HandBlock";

    type Ctx = HandBlockCtx;

    const SEGMENTS: &'static [SegmentDef<'static>] = &[SegmentDef::new(
        "data",
        Start::At(0),
        Span::Counted { by: By::field("stride"), each: Some(8) },
        None,
        None,
        &[],
    )];

    const PARAMS: &'static [ParamDef<'static>] = &[ParamDef::new("stride", "u8")];

    fn decode_with_nested(
        bytes: &[u8],
        _depth: u32,
        ctx: Self::Ctx,
    ) -> Result<(Self, usize), ParseError> {
        let n = usize::from(ctx.stride);
        let data = bytes.get(..n).ok_or(ParseError::Short {
            need_bytes: n,
            got_bytes: bytes.len(),
            at: 0,
        })?;
        Ok((HandBlock(data.to_vec()), n))
    }

    fn encode_into_with<B: Buffer>(&self, out: &mut B, _ctx: Self::Ctx) -> Result<(), Overflow> {
        out.push(&self.0)
    }
}

#[test]
fn a_hand_written_context_message_decodes_under_the_value_it_is_given() {
    let ctx = HandBlockCtx { stride: 3 };
    let (block, used) = HandBlock::decode_with(b"abcdef", ctx).expect("decodes");
    assert_eq!(used, 3);
    assert_eq!(block, HandBlock(b"abc".to_vec()));
    assert_eq!(block.encode_with(ctx), b"abc".to_vec());

    assert_eq!(
        HandBlock::decode_with(b"ab", HandBlockCtx { stride: 4 }),
        Err(ParseError::Short { need_bytes: 4, got_bytes: 2, at: 0 })
    );
}

#[test]
fn a_hand_written_context_message_publishes_its_parameters_beside_its_rows() {
    assert_eq!(<HandBlock as Message>::PARAMS, &[ParamDef::new("stride", "u8")]);
    assert_eq!(<HandBlock as Message>::SEGMENTS.len(), 1);
    assert!(format!("{}", <HandBlock as Message>::audit()).contains("G HandBlock data"));
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
#[message(needs(stride: u8))]
struct Block {
    id: u16,
    #[len(stride)]
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
struct Epoch {
    n: u8,
    stride: u8,
    #[count(n)]
    #[with(stride)]
    #[message]
    blocks: Vec<Block>,
}

fn epoch() -> Epoch {
    Epoch {
        n: 2,
        stride: 3,
        blocks: vec![Block { id: 1, data: vec![1, 2, 3] }, Block { id: 2, data: vec![4, 5, 6] }],
    }
}

#[test]
fn the_holder_states_the_width_each_element_reads() {
    let m = epoch();
    let wire = m.encode();
    assert_eq!(wire, vec![2, 3, 1, 0, 1, 2, 3, 2, 0, 4, 5, 6]);
    let (back, used) = Epoch::decode(&wire).expect("decodes");
    assert_eq!(used, wire.len());
    assert_eq!(back, m);
}

#[test]
fn a_parameter_is_written_as_the_writer_set_it() {
    let mut m = epoch();
    m.stride = 9;
    let wire = m.encode();
    assert_eq!(
        wire[1], 9,
        "a parameter is not derived: encode writes the number the writer put in the field"
    );
    assert_eq!(
        Epoch::decode(&wire),
        Err(ParseError::Short { need_bytes: 11, got_bytes: 10, at: 4 }),
        "and the wire the writer asked for is the wire that comes back"
    );
}

#[test]
fn a_context_message_decodes_on_its_own_through_the_context() {
    let ctx = BlockCtx::new(3);
    let (block, used) = Block::decode_with(&[7, 0, 1, 2, 3, 99], ctx).expect("decodes");
    assert_eq!(used, 5);
    assert_eq!(block, Block { id: 7, data: vec![1, 2, 3] });
    assert_eq!(block.encode_with(ctx), vec![7, 0, 1, 2, 3]);
}

#[test]
fn a_childs_refusal_is_rebased_into_the_holders_buffer() {
    assert_eq!(
        Block::decode_with(&[7, 0, 1], BlockCtx::new(3)),
        Err(ParseError::Short { need_bytes: 5, got_bytes: 3, at: 2 }),
        "the child counts from its own first byte"
    );
    assert_eq!(
        Epoch::decode(&[2, 3, 1, 0, 1, 2, 3, 2, 0, 4]),
        Err(ParseError::Short { need_bytes: 5, got_bytes: 3, at: 9 }),
        "and the holder's walk moves the same refusal into the buffer it was handed: `at` by the 7 bytes in front of it, and the two lengths unchanged"
    );
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
#[message(closed)]
enum Tail {
    #[value(1)]
    Small(u8),
    #[value(2)]
    Wide(u32),
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
#[message(needs(items: u8, kind: u8))]
struct Body {
    #[count(items)]
    values: Vec<u16>,
    #[switch(kind)]
    tail: Tail,
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
struct Frame {
    items: u8,
    kind: u8,
    #[with(items, kind)]
    #[message]
    body: Body,
}

#[test]
fn two_parameters_count_a_run_and_choose_an_arm() {
    let m = Frame {
        items: 2,
        kind: 2,
        body: Body { values: vec![0x0201, 0x0403], tail: Tail::Wide(9) },
    };
    let wire = m.encode();
    assert_eq!(wire, vec![2, 2, 1, 2, 3, 4, 9, 0, 0, 0]);
    assert_eq!(Frame::decode(&wire).expect("decodes"), (m, wire.len()));

    let m = Frame { items: 0, kind: 1, body: Body { values: vec![], tail: Tail::Small(5) } };
    assert_eq!(m.encode(), vec![0, 1, 5]);
    assert_eq!(Frame::decode(&[0, 1, 5]).expect("decodes").0, m);
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
#[message(needs(width: u8))]
struct Leaf {
    #[len(width)]
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
#[message(needs(tail_len: u8))]
struct Middle {
    leaf_width: u8,
    #[with(leaf_width)]
    #[message]
    leaf: Leaf,
    #[len(tail_len)]
    tail: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
struct Outer {
    tail_len: u8,
    #[with(tail_len)]
    #[message]
    middle: Middle,
}

#[test]
fn a_context_message_passes_one_of_its_own_fields_down() {
    let m = Outer {
        tail_len: 2,
        middle: Middle { leaf_width: 3, leaf: Leaf { bytes: vec![1, 2, 3] }, tail: vec![8, 9] },
    };
    let wire = m.encode();
    assert_eq!(wire, vec![2, 3, 1, 2, 3, 8, 9]);
    assert_eq!(Outer::decode(&wire).expect("decodes"), (m, wire.len()));
}

#[derive(Debug, Clone, PartialEq, layline::Message)]
struct Held {
    width: u8,
    flags: u8,
    off: u8,
    #[when(flags & 0x01)]
    #[with(width)]
    #[message]
    maybe: Option<Leaf>,
    #[seek(off)]
    #[with(width)]
    #[message]
    placed: Leaf,
}

#[test]
fn an_optional_and_a_placed_record_read_the_holders_value_too() {
    let m = Held {
        width: 2,
        flags: 1,
        off: 5,
        maybe: Some(Leaf { bytes: vec![1, 2] }),
        placed: Leaf { bytes: vec![3, 4] },
    };
    let wire = m.encode();
    assert_eq!(wire, vec![2, 1, 5, 1, 2, 3, 4]);
    assert_eq!(Held::decode(&wire).expect("decodes").0, m);

    let m = Held { flags: 0, off: 3, maybe: None, ..m };
    assert_eq!(m.encode(), vec![2, 0, 3, 3, 4]);
    assert_eq!(Held::decode(&[2, 0, 3, 3, 4]).expect("decodes").0, m);
}

#[test]
fn the_table_names_a_parameter_the_way_it_names_a_field() {
    let rows = <Block as Message>::SEGMENTS;
    let data = rows.iter().find(|r| r.name == "data").expect("a row per field");
    assert_eq!(data.span, Span::Window { by: By::field("stride") });
    assert_eq!(<Block as Message>::PARAMS, &[ParamDef::new("stride", "u8")]);

    let held = <Epoch as Message>::SEGMENTS.iter().find(|r| r.name == "blocks").expect("a row");
    assert_eq!(
        held.decoded_by,
        Some("Block"),
        "the holder's row still names the type it defers to"
    );
}

#[test]
fn the_audit_dump_has_a_record_per_parameter() {
    let dump = <Block as layline::Message>::audit().to_string();
    assert_eq!(
        dump.lines().nth(1),
        Some("P Block stride u8"),
        "the parameter list is one record, in front of the rows"
    );
    assert!(dump.contains("G Block data"));
}

#[test]
fn random_wires_through_a_holder_that_states_a_value() {
    let mut r = rng::Rng(0x00c0_ffee_0000_0001);
    let mut decoded = 0usize;
    for i in 0..4096 {
        let len = (r.next() as usize) % 65;
        let mut wire = vec![0u8; len];
        r.fill(&mut wire);
        let Ok((value, used)) = Epoch::decode(&wire) else { continue };
        decoded += 1;
        assert!(used <= wire.len(), "wire {i}: consumed {used} of {}", wire.len());
        let out = value.encode();
        let (again, used_again) = Epoch::decode(&out)
            .unwrap_or_else(|e| panic!("wire {i}: re-encoded to bytes it refuses: {e:?}"));
        assert_eq!(value, again, "wire {i}: a round trip changed the value");
        assert_eq!(used_again, out.len(), "wire {i}: re-encode left bytes over");
    }
    assert!(decoded > 0, "no random wire decoded, so nothing was proven");
}
