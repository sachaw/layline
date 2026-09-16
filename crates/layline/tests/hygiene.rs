//! Every derive expands under items that shadow the names the expansion uses.
#![cfg(feature = "derive")]
extern crate alloc;

use layline::{Layout, Message};

/// A user's checksum, declared outside the shadowing modules and reached through `super::`.
pub struct Lrc8;

impl layline::Checksum for Lrc8 {
    type Output = u8;
    type State = u8;

    fn init() -> u8 {
        0
    }

    fn update(state: u8, bytes: &[u8]) -> u8 {
        bytes.iter().fold(state, |acc, &b| acc.wrapping_add(b))
    }

    fn finish(state: u8) -> u8 {
        state.wrapping_neg()
    }
}

pub trait Segments {}

pub type Result<T> = core::result::Result<T, ()>;

#[derive(Debug)]
pub struct Error;
pub trait Buffer {}
pub struct Overflow;
pub struct ParseError;

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
pub struct Head {
    pub n: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Body {
    #[bytes(2)]
    pub head: Head,
    #[count(head.n, cap = 8)]
    pub data: Vec<u8>,
}

/// A field type in the declaration resolves in the consumer's scope.
/// The field types here are paths that avoid the shadows (`i16`, `alloc::vec::Vec`,
/// `::core::option::Option`).
#[allow(dead_code, unused_imports, non_camel_case_types)]
mod shadowed {
    use layline::{Dispatch, Layout, Message};

    pub enum Shadow {
        Ok,
        Err,
        Some,
        None,
    }
    pub use Shadow::*;
    pub struct Vec;
    pub struct Box;
    pub struct String;
    pub struct Result;
    pub struct Option;
    pub struct usize;
    pub struct u8;
    pub struct u16;
    pub struct u32;
    pub struct u64;
    pub struct i64;
    pub struct bool;
    pub struct str;
    pub trait TryFrom {}
    pub trait Extend {}
    pub trait From {}
    pub trait Into {}
    pub trait Default {}
    pub trait Iterator {}

    #[derive(Debug, Clone, PartialEq, Layout)]
    #[layout(bytes = 4)]
    pub struct Head {
        pub n: i16,
        pub kind: i8,
        pub spare: i8,
    }

    #[derive(Debug, Clone, PartialEq, Layout)]
    #[layout(bits = 16)]
    pub struct Packed {
        #[bits(3)]
        pub a: i8,
        #[bits(13)]
        pub b: i16,
    }

    #[derive(Debug, Clone, PartialEq, Layout)]
    #[layout(words = 1)]
    pub struct Word {
        #[bits(4)]
        pub lo: i8,
        #[bits(12)]
        pub hi: i16,
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    pub struct Item {
        pub n: i8,
        #[count(n)]
        pub v: alloc::vec::Vec<i8>,
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    #[message(closed)]
    pub enum Arm {
        #[value(0)]
        Item(Item),
        #[value(1)]
        Word(i16),
        #[value(2)]
        #[bytes(4)]
        Head(Head),
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    pub struct Tail {
        pub kind: i8,
        #[fill]
        pub v: alloc::vec::Vec<i16>,
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    pub struct Body {
        #[bytes(4)]
        pub head: Head,
        #[count(head.n)]
        pub data: alloc::vec::Vec<i8>,
        pub len: i16,
        #[len(len)]
        #[message]
        pub items: alloc::vec::Vec<Item>,
        #[var]
        pub id: layline_core::num::Sleb128,
        #[text]
        #[bytes(4)]
        pub tag: alloc::string::String,
        pub flags: i16,
        #[when(flags & 0x01)]
        pub stamp: ::core::option::Option<i32>,
        #[when(flags & 0x02)]
        #[text]
        #[until(0)]
        pub name: ::core::option::Option<alloc::string::String>,
        pub off: i16,
        #[seek(off)]
        #[message]
        pub placed: Item,
        pub kind: i8,
        #[switch(kind)]
        pub arm: Arm,
        #[message]
        pub nested: alloc::boxed::Box<Item>,
        #[message]
        pub tail: Tail,
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    #[message(bits, order = msb)]
    pub struct Bits {
        #[bits(3)]
        pub kind: i8,
        #[bits(1)]
        pub urgent: i8,
        #[present]
        #[bits(13)]
        pub seq: ::core::option::Option<i16>,
        #[present]
        pub level: ::core::option::Option<layline::U<4>>,
        #[when(kind & 0x1)]
        #[bits(5)]
        pub extra: ::core::option::Option<i8>,
        #[var]
        pub run: layline_core::num::ExpGolomb,
        #[bits(32)]
        pub scale: f32,
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    pub struct Rest {
        pub kind: i8,
        #[fill(cap = 4)]
        pub rest: alloc::vec::Vec<i8>,
    }

    #[derive(Debug, Clone, PartialEq, Layout)]
    #[layout(bits = 16, prefix = 3, prefix_value = 0b101)]
    pub struct Ping {
        #[bits(13)]
        pub seq: i16,
    }

    #[derive(Debug, Clone, PartialEq, Layout)]
    #[layout(bits = 16, prefix = 3, prefix_value = 0b011)]
    pub struct Pong {
        #[bits(13)]
        pub ack: i16,
    }

    #[derive(Debug, Clone, PartialEq, Dispatch)]
    #[dispatch(id = ::core::primitive::u8, prefix = 3)]
    pub enum Frame {
        #[value(0b101)]
        Ping(Ping),
        #[value(0b011)]
        Pong(Pong),
        #[other]
        Unknown { id: ::core::primitive::u8, body: alloc::vec::Vec<::core::primitive::u8> },
    }

    #[test]
    fn every_derive_expands_under_the_shadows() {
        let m = Body {
            head: Head { n: 0, kind: 3, spare: -1 },
            data: alloc::vec![1, 2, 3],
            len: 0,
            items: alloc::vec![Item { n: 1, v: alloc::vec![9] }, Item { n: 0, v: alloc::vec![] }],
            id: layline_core::num::Sleb128(-300),
            tag: alloc::string::String::from("ab"),
            flags: 0,
            stamp: ::core::option::Option::Some(0x0102_0304),
            name: ::core::option::Option::None,
            off: 0,
            placed: Item { n: 2, v: alloc::vec![7, 7] },
            kind: 2,
            arm: Arm::Head(Head { n: 1, kind: 2, spare: 3 }),
            nested: alloc::boxed::Box::new(Item { n: 1, v: alloc::vec![-1] }),
            tail: Tail { kind: 5, v: alloc::vec![256, -2] },
        };
        let bytes = m.encode();
        let (back, used) = Body::decode(&bytes).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(back.head.n, 3);
        assert_eq!(back.items, m.items);
        assert_eq!(back.id, m.id);
        assert_eq!(back.tag, m.tag);
        assert_eq!(back.flags, 1);
        assert_eq!(back.stamp, m.stamp);
        assert_eq!(back.placed, m.placed);
        assert_eq!(back.arm, m.arm);
        assert_eq!(back.nested, m.nested);
        assert_eq!(back.tail, m.tail);
        assert_eq!(back.encode(), bytes);

        let (r, used) = Rest::decode(&[1, 2, 3, 4, 5]).unwrap();
        assert_eq!((r.rest, used), (alloc::vec![2, 3, 4, 5], 5));

        let frame = Frame::Ping(Ping { seq: -5 });
        let mut room = [0u8; 2];
        let mut out = layline::Fixed::new(&mut room);
        assert_eq!(frame.encode_into(&mut out), ::core::result::Result::Ok(()));
        let wire = out.into_written();
        assert_eq!(<Frame as layline::Dispatch>::decode(wire[0] & 0b111, wire), frame);
        assert_eq!(Packed::decode(&Packed { a: -1, b: 100 }.encode()), Packed { a: -1, b: 100 });
        assert_eq!(Word::decode(&Word { lo: 7, hi: -9 }.encode()), Word { lo: 7, hi: -9 });
    }
}

/// `u8` and `u16` stay unshadowed: a `#[checksum]` field and a `FieldCodec` newtype are of those types.
/// `u32`, `u64` and `i64` are shadowed; only the expansion writes them.
#[allow(dead_code, unused_imports, non_camel_case_types)]
mod types {
    use layline::{FieldCodec, Message};

    pub enum Shadow {
        Ok,
        Err,
        Some,
        None,
    }
    pub use Shadow::*;
    pub struct Vec;
    pub struct Box;
    pub struct String;
    pub struct Result;
    pub struct Option;
    pub struct usize;
    pub struct u32;
    pub struct u64;
    pub struct i64;
    pub struct bool;
    pub struct str;
    pub trait TryFrom {}
    pub trait Extend {}
    pub trait From {}
    pub trait Into {}
    pub trait Default {}
    pub trait Iterator {}

    #[derive(Debug, Clone, PartialEq, Message)]
    #[message(endian = be)]
    pub struct Checked {
        pub magic: u16,
        pub len: u16,
        #[count(len)]
        pub payload: alloc::vec::Vec<u8>,
        #[checksum(layline::checksum::Crc<u16, 0x1021, 0xFFFF, 0x0000, false>, over = ..)]
        pub crc: u16,
        #[checksum(super::Lrc8, over = magic..=len)]
        pub lrc: u8,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, FieldCodec)]
    #[bits(15)]
    pub struct Track(pub u16);

    #[derive(Debug, Clone, Copy, PartialEq, Eq, FieldCodec)]
    #[bits(2)]
    pub enum Quadrant {
        #[value(0)]
        N,
        #[value(1)]
        E,
        #[value(2)]
        S,
        #[value(3)]
        W,
    }

    #[test]
    fn a_checksum_and_a_codec_expand_under_the_shadows() {
        let m = Checked { magic: 0xABCD, len: 0, payload: alloc::vec![1, 2, 3], crc: 0, lrc: 0 };
        let bytes = m.encode();
        let (back, used) = Checked::decode(&bytes).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(back.payload, m.payload);
        assert_eq!(back.encode(), bytes);
        assert_eq!(<Track as layline::FieldCodec>::to_raw(&Track(0x7FFF)), 0x7FFF);
        assert_eq!(<Quadrant as layline::FieldCodec>::from_raw(2), Quadrant::S);
    }
}

#[test]
fn a_field_may_be_named_body() {
    #[derive(Debug, Clone, PartialEq, Message)]
    struct Frame {
        n: u8,
        #[count(n)]
        body: Vec<u8>,
        m: u8,
        #[count(m)]
        tags: Vec<u16>,
    }

    let wire = vec![2, 0xAA, 0xBB, 2, 0x22, 0x11, 0x44, 0x33];
    let frame = Frame { n: 2, body: vec![0xAA, 0xBB], m: 2, tags: vec![0x1122, 0x3344] };
    assert_eq!(Frame::decode(&wire), Ok((frame.clone(), wire.len())));
    assert_eq!(frame.encode(), wire);
}
