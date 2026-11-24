//! A field wider than 64 bits refuses a value its width cannot hold, as a narrow one does.

#![cfg(feature = "derive")]

use layline::{EncodeError, Layout};

#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bits = 128)]
struct Wide {
    #[bits(100)]
    a: u128,
    #[bits(28)]
    b: u32,
}

#[test]
fn the_widest_value_round_trips() {
    let wide = Wide { a: (1 << 100) - 1, b: (1 << 28) - 1 };
    assert_eq!(Wide::decode(&wide.encode().unwrap()), wide);
}

#[test]
fn a_value_past_a_wide_width_is_refused() {
    assert_eq!(
        Wide { a: 1 << 110, b: 0 }.encode(),
        Err(EncodeError::Unfit {
            field: "a",
            value: 1298074214633706907132624082305024,
            bits: 100
        })
    );
}

/// A cryptovariable half is 68 bits: wider than a `u64`, and the wire keeps every bit.
#[derive(Debug, Clone, Copy, PartialEq, Layout)]
#[layout(bits = 72, endian = be, order = msb)]
struct Crypto {
    #[bits(68)]
    part: u128,
    #[bits(4)]
    spare: u8,
}

#[test]
fn a_field_wider_than_a_u64_round_trips() {
    for part in [0, 1, (1u128 << 68) - 1, 0x000D_EADB_EEFC_AFEF_00D5] {
        let word = Crypto { part, spare: 0xF };
        assert_eq!(Crypto::decode(&word.encode().unwrap()), word, "{part:#x}");
    }
    assert_eq!(Crypto { part: 0, spare: 0 }.encode().unwrap(), [0u8; 9]);
}
