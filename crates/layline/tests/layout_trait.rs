//! A generic function reads a layout's tables through the `Layout` trait alone.

#![cfg(feature = "derive")]

use layline::{Fixed, Layout, Overflow};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 16)]
struct Status {
    #[bits(4)]
    version: u8,
    #[bits(4)]
    kind: u8,
    #[bits(8)]
    seq: u8,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 6)]
struct Record {
    id: u16,
    value: u32,
}

fn audit<T: Layout>() -> (u32, usize, u64) {
    let covered: u64 = T::FIELDS.iter().map(|f| f.extent.width() as u64).sum();
    (T::WIRE_BYTES as u32 * 8, T::FIELDS.len(), covered)
}

#[test]
fn any_layout_can_be_walked_without_naming_it() {
    assert_eq!(audit::<Status>(), (16, 3, 16));
    assert_eq!(audit::<Record>(), (48, 2, 48));
}

#[test]
fn a_layout_encodes_and_decodes_through_the_trait_alone() {
    fn round_trip<T: Layout + PartialEq + core::fmt::Debug + Clone>(v: T) {
        let mut buf = Vec::new();
        assert_eq!(v.encode_into(&mut buf), Ok(()));
        assert_eq!(buf.len(), T::WIRE_BYTES);
        assert_eq!(T::decode_slice(&buf).ok().unwrap(), v);

        let mut room = vec![0u8; T::WIRE_BYTES - 1];
        assert_eq!(v.encode_into(&mut Fixed::new(&mut room)), Err(Overflow));
    }
    round_trip(Status { version: 1, kind: 2, seq: 3 });
    round_trip(Record { id: 7, value: 9 });
}
