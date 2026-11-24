//! Text whose encoding has fewer characters than Unicode: encode refuses a character the encoding
//! has no bytes for, as decode refuses bytes it reads no character from, and a type holding such
//! text returns a `Result` from encode, found through its holders as any other refusal is.

#![cfg(feature = "derive")]

#[path = "support/text.rs"]
mod codecs;
#[path = "support/rng.rs"]
mod rng;

use layline::{EncodeError, Message, ParseError};

/// UTF-8 has bytes for every character.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Note {
    #[fill]
    #[text]
    pub text: String,
}

/// ASCII has bytes for 128.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Callsign {
    #[len(prefix = u8)]
    #[text(codecs::Ascii)]
    pub name: String,
}

/// Holds ASCII through a nested message, an option and a repeat.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Roster {
    #[message]
    pub lead: Callsign,
    #[present]
    #[text(codecs::Ascii)]
    #[until(0)]
    pub alias: Option<String>,
    #[count(prefix = u8)]
    #[text(codecs::Latin1)]
    #[until(0)]
    pub others: Vec<String>,
}

/// Holds only UTF-8, through a nested message.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Tagged {
    pub tag: u8,
    #[message]
    pub note: Note,
}

#[test]
fn text_of_every_character_keeps_an_infallible_encode() {
    const _: () = assert!(!<Note as Message>::ENCODE_FALLIBLE);
    const _: () = assert!(!<Tagged as Message>::ENCODE_FALLIBLE);
    let wire: Vec<u8> = Tagged { tag: 1, note: Note { text: String::from("été") } }.encode();
    assert_eq!(wire, [1, 0xC3, 0xA9, b't', 0xC3, 0xA9]);
}

#[test]
fn a_character_the_encoding_lacks_is_refused_naming_the_field() {
    const _: () = assert!(<Callsign as Message>::ENCODE_FALLIBLE);
    const _: () = assert!(<Roster as Message>::ENCODE_FALLIBLE);

    let ok: Result<Vec<u8>, EncodeError> = Callsign { name: String::from("K1ABC") }.encode();
    assert_eq!(ok, Ok(b"\x05K1ABC".to_vec()));
    assert_eq!(
        Callsign { name: String::from("Zoë") }.encode(),
        Err(EncodeError::Unencodable { field: "name", character: 'ë' })
    );
    assert_eq!(
        Callsign::decode(b"\x03Zo\xEB"),
        Err(ParseError::Malformed { field: "name", at: 1 })
    );

    let roster = |alias: &str, other: &str| Roster {
        lead: Callsign { name: String::from("K1") },
        alias: Some(String::from(alias)),
        others: vec![String::from(other)],
    };
    let wire = roster("al", "Zoë").encode().unwrap();
    assert_eq!(wire, b"\x02K1\x01al\x00\x01Zo\xEB\x00");
    assert_eq!(Roster::decode(&wire).unwrap(), (roster("al", "Zoë"), wire.len()));
    assert_eq!(
        roster("Zoë", "al").encode(),
        Err(EncodeError::Unencodable { field: "alias", character: 'ë' })
    );
    assert_eq!(
        roster("al", "→").encode(),
        Err(EncodeError::Unencodable { field: "others", character: '→' })
    );
    let lead = Roster { lead: Callsign { name: String::from("→") }, ..roster("a", "b") };
    assert_eq!(lead.encode(), Err(EncodeError::Unencodable { field: "name", character: '→' }));
}

/// Random text: encode refuses exactly the text with a character past ASCII, and decode reads back
/// all it writes.
#[test]
fn random_text_is_refused_exactly_when_ascii_has_no_bytes_for_it() {
    const ALPHABET: [char; 6] = ['a', 'Z', '0', '\u{7F}', '\u{80}', 'é'];
    let mut r = rng::Rng(0xA5C1_1000);
    for _ in 0..5_000 {
        let len = (r.next() % 8) as usize;
        let name: String = (0..len).map(|_| ALPHABET[(r.next() % 6) as usize]).collect();
        let first_wide = name.chars().find(|c| !c.is_ascii());
        match (Callsign { name: name.clone() }).encode() {
            Ok(wire) => {
                assert_eq!(first_wide, None, "{name:?} encoded");
                assert_eq!(Callsign::decode(&wire), Ok((Callsign { name }, wire.len())));
            }
            Err(e) => assert_eq!(
                Some(e),
                first_wide.map(|character| EncodeError::Unencodable { field: "name", character })
            ),
        }
    }
}

/// Random bytes: decode refuses any byte past ASCII, and encode writes back what it reads.
#[test]
fn random_bytes_decode_only_as_ascii() {
    let mut r = rng::Rng(0xA5C1_1001);
    for _ in 0..5_000 {
        let mut wire = [0u8; 6];
        r.fill(&mut wire);
        wire[0] %= 6;
        let body = &wire[1..=usize::from(wire[0])];
        match Callsign::decode(&wire) {
            Ok((back, used)) => {
                assert!(body.is_ascii());
                assert_eq!(back.encode().unwrap(), wire[..used]);
            }
            Err(e) => {
                assert!(!body.is_ascii(), "{wire:?}: {e}");
                assert_eq!(e, ParseError::Malformed { field: "name", at: 1 });
            }
        }
    }
}
