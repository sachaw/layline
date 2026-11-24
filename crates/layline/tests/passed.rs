//! `#[with(..)]` takes an expression per parameter, in the holder's scope, as every attribute that
//! takes a value does. Encode reads it and writes nothing back: Kaitai's
//! `compound_ext((extension_len - 1) - len)` passes a computed length to a nested message, and an
//! arm takes a scaled length or a tested bit from its holder.

#![cfg(feature = "derive")]

use layline::{EncodeError, Message, ParseError};

#[path = "support/rng.rs"]
mod rng;

/// Bytes counted by its holder.
#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(n: u8))]
pub struct Compound {
    #[count(n)]
    pub items: Vec<u8>,
}

/// Kaitai's `compound_ext((extension_len - 1) - len)`.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Extension {
    pub extension_len: u8,
    pub len: u8,
    #[with(extension_len - 1 - len)]
    #[message]
    pub compound: Compound,
}

fn extension(extension_len: u8, len: u8, items: &[u8]) -> Extension {
    Extension { extension_len, len, compound: Compound { items: items.to_vec() } }
}

#[test]
fn a_computed_value_reaches_the_nested_message() {
    const _: () = assert!(<Extension as Message>::ENCODE_FALLIBLE);
    let ext = extension(5, 2, &[7, 8]);
    let wire = ext.encode().unwrap();
    assert_eq!(wire, [5, 2, 7, 8]);
    assert_eq!(Extension::decode(&wire), Ok((ext, 4)));
    assert_eq!(Extension::decode(&[1, 0, 9]).unwrap().0, extension(1, 0, &[]));
}

#[test]
fn encode_reads_the_value_and_the_nested_message_checks_it() {
    // The holder's fields are the caller's: the expression writes neither.
    assert_eq!(
        extension(5, 1, &[7, 8]).encode(),
        Err(EncodeError::Mismatch { field: "items", value: 2, stated: 3 })
    );
}

#[test]
fn a_value_the_parameter_cannot_hold_is_refused_both_ways() {
    assert_eq!(
        Extension::decode(&[1, 1]),
        Err(ParseError::Malformed { field: "extension_len", at: 2 })
    );
    assert_eq!(
        extension(1, 1, &[]).encode(),
        Err(EncodeError::Unfit { field: "compound", value: -1, bits: 0 })
    );
    assert_eq!(
        extension(0, 0, &[]).encode(),
        Err(EncodeError::Unfit { field: "compound", value: -1, bits: 0 })
    );
}

/// An arm sized in words by its holder, and a byte order from a tested bit.
#[derive(Message, Debug, Clone, PartialEq)]
pub struct Frame {
    pub kind: u8,
    pub flags: u8,
    pub words: u8,
    #[switch(kind)]
    #[with(words * 4, flags & 0x80 != 0)]
    pub body: Body,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(bytes: u16, big: bool), closed)]
pub enum Body {
    #[value(1)]
    #[with(bytes / 2, big)]
    Samples(Samples),
    #[value(2)]
    #[with(bytes, big)]
    Raw(Raw),
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(n: u16, big: bool))]
pub struct Raw {
    #[count(n)]
    pub data: Vec<u8>,
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(n: u16, big: bool), endian = needs(big))]
pub struct Samples {
    #[count(n)]
    pub values: Vec<u16>,
}

#[test]
fn an_arm_takes_expressions_and_passes_them_on() {
    let raw =
        Frame { kind: 2, flags: 0, words: 1, body: Body::Raw(Raw { data: vec![1, 2, 3, 4] }) };
    let wire = raw.encode().unwrap();
    assert_eq!(wire, [2, 0, 1, 1, 2, 3, 4]);
    assert_eq!(Frame::decode(&wire), Ok((raw, wire.len())));

    for (flags, two) in [(0x80, [0x00, 0x02]), (0x7F, [0x02, 0x00])] {
        let samples = Samples { values: vec![1, 2, 3, 4] };
        let frame = Frame { kind: 1, flags, words: 2, body: Body::Samples(samples.clone()) };
        let wire = frame.encode().unwrap();
        assert_eq!(wire[..3], [1, flags, 2]);
        assert_eq!(wire[5..7], two);
        assert_eq!(Frame::decode(&wire), Ok((frame, wire.len())));
    }

    let short =
        Frame { kind: 1, flags: 0, words: 1, body: Body::Samples(Samples { values: vec![] }) };
    assert_eq!(short.encode(), Err(EncodeError::Mismatch { field: "values", value: 0, stated: 2 }));
}

/// Random bytes: an extension decodes exactly when its arithmetic gives a count a `u8` holds
/// and the bytes are there, and encodes back to them.
#[test]
fn random_lengths_decode_or_are_refused() {
    let mut r = rng::Rng(0x0C0A_7E17);
    for _ in 0..10_000 {
        let mut wire = [0u8; 8];
        r.fill(&mut wire);
        wire[0] %= 8;
        wire[1] %= 8;
        let len = 2 + (r.next() % 7) as usize;
        let wire = &wire[..len];
        let n = i64::from(wire[0]) - 1 - i64::from(wire[1]);
        match Extension::decode(wire) {
            Ok((ext, used)) => {
                assert_eq!(used as i64, 2 + n, "{wire:?}");
                assert_eq!(ext.encode().unwrap(), wire[..used]);
            }
            Err(ParseError::Malformed { field, .. }) => {
                assert!(n < 0, "{wire:?}");
                assert_eq!(field, "extension_len");
            }
            Err(e) => {
                assert!(n >= 0 && 2 + n > len as i64, "{wire:?}: {e}");
            }
        }
    }
}

/// A header layout's fields, in an expression.
#[derive(layline::Layout, Debug, Clone, Copy, PartialEq)]
#[layout(bits = 8, order = msb)]
pub struct Head {
    #[bits(1)]
    pub little: bool,
    #[bits(7)]
    pub words: u8,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Headed {
    #[bytes(1)]
    pub head: Head,
    #[with(head.words * 2, !head.little)]
    #[message]
    pub samples: Samples,
}

#[test]
fn an_expression_reads_a_nested_layouts_fields() {
    let headed = |little| Headed {
        head: Head { little, words: 1 },
        samples: Samples { values: vec![0x0102, 0x0304] },
    };
    let wire = headed(false).encode().unwrap();
    assert_eq!(wire, [0x01, 1, 2, 3, 4]);
    assert_eq!(Headed::decode(&wire), Ok((headed(false), 5)));
    let wire = headed(true).encode().unwrap();
    assert_eq!(wire, [0x81, 2, 1, 4, 3]);
    assert_eq!(Headed::decode(&wire), Ok((headed(true), 5)));
}
