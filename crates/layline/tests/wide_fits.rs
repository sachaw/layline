//! A field wider than 64 bits refuses a value its width cannot hold, as a narrow one does.

#![cfg(feature = "derive")]

use layline::Layout;

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
    assert_eq!(Wide::decode(&wide.encode()), wide);
}

#[test]
#[should_panic(expected = "field `a`: value does not fit #[bits(100)]")]
fn a_value_past_a_wide_width_is_refused() {
    let _ = Wide { a: 1 << 110, b: 0 }.encode();
}
