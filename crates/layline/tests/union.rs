//! `#[switch(f)] #[bytes(N)]`: a fixed-size union. The field after it keeps its offset.
//! Every vector is written out from the declared widths.

#![cfg(feature = "derive")]

use layline::table::{Discriminant, Span, Start};
use layline::{Layout, Message, ParseError, table::fixed_bits};

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 8)]
struct GetBlock {
    id: u32,
    index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bytes = 8)]
struct GetBlockDone {
    id: u32,
    spare: [u8; 4],
}

#[derive(Debug, Clone, PartialEq, Message)]
struct UserData {
    length: u8,
    #[count(length)]
    buffer: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
enum Args {
    #[value(0x05)]
    #[bytes(8)]
    GetBlock(GetBlock),
    #[value(0x06)]
    #[bytes(8)]
    GetBlockDone(GetBlockDone),
    #[value(0x07)]
    UserData(UserData),
    #[other]
    Unknown(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Packet {
    command: u8,
    bitmap: u16,
    counter: u8,
    #[switch(command)]
    #[bytes(8)]
    args: Args,
    payload: u32,
}

const GET_BLOCK: [u8; 16] = [
    0x05, 0xFE, 0xFF, 0x03, 0xD2, 0x04, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0xEF, 0xBE, 0xAD, 0xDE,
];

const GET_BLOCK_DONE: [u8; 16] = [
    0x06, 0xFE, 0xFF, 0x03, 0xD2, 0x04, 0x00, 0x00, 0xAA, 0xBB, 0xCC, 0xDD, 0xEF, 0xBE, 0xAD, 0xDE,
];

const UNKNOWN: [u8; 16] = [
    0x99, 0xFE, 0xFF, 0x03, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0xEF, 0xBE, 0xAD, 0xDE,
];

const USER_DATA: [u8; 16] = [
    0x07, 0xFE, 0xFF, 0x03, 0x07, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0xEF, 0xBE, 0xAD, 0xDE,
];

fn get_block() -> Packet {
    Packet {
        command: 0x05,
        bitmap: 0xFFFE,
        counter: 3,
        args: Args::GetBlock(GetBlock { id: 1234, index: 7 }),
        payload: 0xDEAD_BEEF,
    }
}

fn get_block_done() -> Packet {
    Packet {
        command: 0x06,
        bitmap: 0xFFFE,
        counter: 3,
        args: Args::GetBlockDone(GetBlockDone { id: 1234, spare: [0xAA, 0xBB, 0xCC, 0xDD] }),
        payload: 0xDEAD_BEEF,
    }
}

#[test]
fn the_payload_lands_at_twelve_whichever_arm_the_command_chose() {
    for (wire, packet) in [(GET_BLOCK, get_block()), (GET_BLOCK_DONE, get_block_done())] {
        let (back, used) = Packet::decode(&wire).expect("parses");
        assert_eq!(back, packet);
        assert_eq!(used, 16, "4 header + 8 union + 4 payload, by hand");
        assert_eq!(packet.encode(), wire.to_vec(), "hand-computed, byte for byte");
        assert_eq!(
            u32::from_le_bytes([wire[12], wire[13], wire[14], wire[15]]),
            0xDEAD_BEEF,
            "the payload is at 12 in the vector as well as in the decode"
        );
    }
}

#[test]
fn the_bytes_a_narrow_arm_does_not_use_are_a_field_and_survive() {
    let (back, _) = Packet::decode(&GET_BLOCK_DONE).expect("parses");
    let Args::GetBlockDone(done) = back.args else { panic!("the 0x06 arm") };
    assert_eq!(done.spare, [0xAA, 0xBB, 0xCC, 0xDD], "the writer's bytes");
    assert_eq!(
        <GetBlockDone as Layout>::FIELDS.iter().map(|f| f.name).collect::<Vec<_>>(),
        ["id", "spare[0]", "spare[1]", "spare[2]", "spare[3]"],
        "and every byte of the spare is named in the arm's own table, at a position a \
         reviewer can audit — which is what the union's other four bytes are"
    );
    assert_eq!(back.encode(), GET_BLOCK_DONE.to_vec(), "encode writes the same bytes");
}

#[test]
fn an_arm_the_wire_sizes_fills_the_union_and_moves_nothing() {
    let (back, used) = Packet::decode(&USER_DATA).expect("parses");
    assert_eq!(
        back.args,
        Args::UserData(UserData { length: 7, buffer: vec![1, 2, 3, 4, 5, 6, 7] }),
        "one byte of length and seven of buffer is what fills eight"
    );
    assert_eq!(back.payload, 0xDEAD_BEEF);
    assert_eq!(used, 16);
    assert_eq!(back.encode(), USER_DATA.to_vec(), "hand-computed");
}

#[test]
fn an_arm_that_overruns_the_footprint_is_refused() {
    let mut wire = USER_DATA;
    wire[4] = 9;
    assert_eq!(
        Packet::decode(&wire),
        Err(ParseError::Short { need_bytes: 9, got_bytes: 8, at: 12 }),
        "the window is eight bytes, and the arm requires nine"
    );

    assert_eq!(&wire[12..], &[0xEF, 0xBE, 0xAD, 0xDE]);
}

#[test]
fn an_arm_that_stops_short_of_the_footprint_is_refused_by_name() {
    let mut wire = USER_DATA;
    wire[4] = 3;
    assert_eq!(Packet::decode(&wire), Err(ParseError::Malformed { field: "args", at: 4 }));

    let mut six = USER_DATA;
    six[4] = 6;
    assert_eq!(Packet::decode(&six), Err(ParseError::Malformed { field: "args", at: 4 }));
    assert!(Packet::decode(&USER_DATA).is_ok(), "seven is the width that tiles it");
}

#[test]
#[should_panic(expected = "did not fill it exactly")]
fn encoding_an_arm_that_did_not_fill_the_union_is_refused_rather_than_padded() {
    let short = Packet {
        command: 0x07,
        bitmap: 0xFFFE,
        counter: 3,
        args: Args::UserData(UserData { length: 99, buffer: vec![1, 2, 3] }),
        payload: 0xDEAD_BEEF,
    };
    let _ = short.encode();
}

#[test]
fn the_open_arm_keeps_the_footprint_and_re_encodes_byte_identically() {
    let (back, used) = Packet::decode(&UNKNOWN).expect("parses");
    assert_eq!(
        back,
        Packet {
            command: 0x99,
            bitmap: 0xFFFE,
            counter: 3,
            args: Args::Unknown(vec![0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]),
            payload: 0xDEAD_BEEF,
        },
        "eight bytes of union, and the payload read as a payload"
    );
    assert_eq!(used, 16);
    assert_eq!(back.encode(), UNKNOWN.to_vec(), "byte-identical, discriminant and all");
}

#[test]
fn the_segment_table_says_the_footprint_is_the_unions() {
    let table = <Packet as Message>::SEGMENTS;
    assert_eq!(table.len(), 3, "header block, union, payload block");

    assert_eq!(table[1].name, "args");
    assert_eq!(table[1].start, Start::At(32));
    assert_eq!(table[1].span, Span::ChosenIn { on: Discriminant::Field("command"), bits: 64 });
    assert_eq!(table[1].decoded_by, Some("Args"));
    assert!(table[1].is_fixed(), "the union has a fixed size, and the offset after it is computed");
    assert_eq!(table[1].span.bits(), Some(64));

    assert_eq!(table[2].start, Start::At(96), "byte 12, `PAYLOAD_OFF`");
    assert_eq!(
        fixed_bits(table),
        128,
        "the computed offsets run past the union to the end; a `Chosen` row would have stopped them"
    );
}

#[test]
fn the_audit_artifact_names_the_footprint_and_the_field_behind_it() {
    let text = <Packet as layline::Message>::audit().to_string();
    assert!(text.contains("\nG Packet args at,32 chosenin,field,command,64 - Args\n"), "{text}");
    assert!(text.contains("\nG Packet __PacketBlock1 at,96 fixed,32 - -\n"), "{text}");
    assert!(text.contains("\nE Packet 128\n"), "{text}");
    assert!(!text.contains(" chosen,"), "no `chosen,` row: the union has a fixed size: {text}");
}

#[test]
fn every_arm_accounts_for_the_whole_footprint() {
    let arms = <Args as layline::Choice>::ARMS;
    let by = |name: &str| arms.iter().find(|a| a.name == name).expect("an arm");

    assert_eq!(fixed_bits(by("GetBlock").segments), 64);
    assert_eq!(fixed_bits(by("GetBlockDone").segments), 64, "id plus its own named spare");
    assert_eq!(
        by("UserData").segments[0].span,
        Span::SelfDelimiting,
        "the arm with a length field, checked against the window on decode"
    );
    assert_eq!(
        by("Unknown").segments[0].span,
        Span::Fill { cap: None },
        "and the open arm fills it"
    );

    assert!(layline::table::arms_fit(arms, 64), "`Packet` pins 64");
    assert!(!layline::table::arms_fit(arms, 32), "the check is for 64 exactly");
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Loose {
    command: u8,
    #[switch(command)]
    args: Args,
}

#[test]
fn a_switch_without_a_footprint_steps_by_the_arm() {
    let table = <Loose as Message>::SEGMENTS;
    assert_eq!(table[1].span, Span::Chosen { on: Discriminant::Field("command") });
    assert!(!table[1].is_fixed());
    assert_eq!(fixed_bits(table), 8, "the computed offsets stop at the switch");

    let (back, used) = Loose::decode(&UNKNOWN).expect("parses");
    assert_eq!(used, 16);
    assert_eq!(back.args, Args::Unknown(UNKNOWN[1..].to_vec()));
}
