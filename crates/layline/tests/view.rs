//! The `repr(transparent)` view over the wire bytes: field for field the owned value, and a setter
//! touches only its own bits.

#![cfg(feature = "derive")]

#[path = "support/rng.rs"]
mod rng;

#[path = "support/fixtures.rs"]
mod fixtures;

use fixtures::{NavState, NavStateView};
use layline::{FieldCodec, Layout, View};
use rng::Rng;

#[derive(FieldCodec, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[bits(3)]
pub enum Mode {
    #[value(0)]
    #[default]
    Idle,
    #[value(1)]
    Acquire,
    #[value(2)]
    Track,
    #[other]
    Other(u8),
}

#[derive(Layout, Debug, Clone, PartialEq, Eq, Default)]
#[layout(bits = 16, view)]
pub struct StatusWord {
    #[bits(1)]
    pub ready: bool,
    #[bits(3)]
    pub mode: Mode,
    #[bits(11)]
    #[at(bit = 4)]
    pub count: u16,
    #[bits(1)]
    pub fault: bool,
}

#[test]
fn view_getters_agree_with_owned_decode_on_random_bytes() {
    let mut rng = Rng(0x51DE);
    for _ in 0..5_000 {
        let mut wire = [0u8; 80];
        rng.fill(&mut wire);
        let view = NavStateView::from_wire(&wire);
        let owned = NavState::decode(&wire);

        assert_eq!(view.week(), owned.week);
        assert_eq!(view.time_of_week().to_bits(), owned.time_of_week.to_bits());
        assert_eq!(view.nav_status(), owned.nav_status);
        for i in 0..3 {
            assert_eq!(view.theta()[i].to_bits(), owned.theta[i].to_bits());
            assert_eq!(view.lla()[i].to_bits(), owned.lla[i].to_bits());
        }
        // Wire-level equality, NaN included.
        assert_eq!(view.decode().encode(), owned.encode());

        let word: [u8; 2] = [wire[0], wire[1]];
        let wview = StatusWordView::from_wire(&word);
        let wowned = StatusWord::decode(&word);
        assert_eq!(wview.ready(), wowned.ready);
        assert_eq!(wview.mode(), wowned.mode);
        assert_eq!(wview.count(), wowned.count);
        assert_eq!(wview.fault(), wowned.fault);
    }
}

#[test]
fn a_setter_touches_its_own_bytes_and_nothing_else() {
    let mut rng = Rng(0x5E77E5);
    for _ in 0..2_000 {
        let mut wire = [0u8; 80];
        rng.fill(&mut wire);
        let before = wire;
        let view = NavStateView::from_wire_mut(&mut wire);
        view.set_nav_status(0xDEAD_BEEF);
        assert_eq!(view.nav_status(), 0xDEAD_BEEF);

        assert_eq!(&wire[..12], &before[..12]);
        assert_eq!(&wire[16..], &before[16..]);
    }
}

#[test]
fn a_word_setter_touches_its_own_bits_and_nothing_else() {
    let mut rng = Rng(0xB175);
    for _ in 0..5_000 {
        let word = [rng.next() as u8, (rng.next() >> 8) as u8];
        let mut wire = word;
        let view = StatusWordView::from_wire_mut(&mut wire);
        view.set_mode(Mode::Track);
        assert_eq!(view.mode(), Mode::Track);

        let mask = !(0b111u16 << 1);
        let before = u16::from_le_bytes(word);
        let after = u16::from_le_bytes(wire);
        assert_eq!(after & mask, before & mask);
        assert_eq!((after >> 1) & 0b111, 2);
    }
}

#[test]
fn construction_by_setters_equals_construction_by_mirror() {
    let owned = StatusWord { ready: true, mode: Mode::Acquire, count: 1234, fault: false };

    let mut built = StatusWordView::zeroed();
    built.set_ready(true);
    built.set_mode(Mode::Acquire);
    built.set_count(1234);
    built.set_fault(false);

    assert_eq!(built.as_wire(), &owned.encode());
    assert_eq!(StatusWordView::from(&owned), built);
    assert_eq!(built.decode(), owned);
}

#[test]
fn the_view_is_the_bytes() {
    let mut buffer = [0u8; 80];
    NavStateView::from_wire_mut(&mut buffer).set_week(2374);
    assert_eq!(u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]), 2374);
}

fn decode_borrowed<V: View>(view: &V) -> V::Owned {
    view.decode()
}

fn parse_borrowed<V: View>(bytes: &[u8]) -> Option<V::Owned> {
    Some(V::from_slice(bytes)?.decode())
}

#[test]
fn a_view_is_generic_over_its_layout() {
    let mut rng = Rng(0xC0DE);
    for _ in 0..1_000 {
        let mut wire = [0u8; 80];
        rng.fill(&mut wire);

        let view = NavStateView::from_wire(&wire);
        assert_eq!(decode_borrowed(view).encode(), NavState::decode(&wire).encode());
        assert_eq!(View::as_wire(view), &wire[..]);
        assert_eq!(parse_borrowed::<NavStateView>(&wire).unwrap().encode(), wire,);

        let word = [wire[0], wire[1]];
        let wview = StatusWordView::from_wire(&word);
        assert_eq!(decode_borrowed(wview), StatusWord::decode(&word));
        assert_eq!(View::as_wire(wview), &word[..]);
        assert_eq!(parse_borrowed::<StatusWordView>(&word), Some(StatusWord::decode(&word)));
    }
}

#[test]
fn the_trait_constructor_checks_the_length_the_inherent_one_proves() {
    let bytes = [0u8; 80];
    assert!(<NavStateView as View>::from_slice(&bytes).is_some());
    assert!(<NavStateView as View>::from_slice(&bytes[..79]).is_none());
    assert!(<StatusWordView as View>::from_slice(&bytes[..2]).is_some());
    assert!(<StatusWordView as View>::from_slice(&bytes[..3]).is_none());
}

#[test]
fn a_view_does_not_truncate_a_wide_carry() {
    #[derive(Debug, Clone, PartialEq, layline::Layout)]
    #[layout(bits = 72, view)]
    struct Carry {
        #[bits(2)]
        wf: u8,
        #[bits(68)]
        body: u128,
        #[bits(2)]
        spare: u8,
    }

    let all_ones = (1u128 << 68) - 1;
    for body in [0, 1, all_ones, all_ones >> 1, 0x1_DEAD_BEEF_CAFE_F00D] {
        let wire = Carry { wf: 0b10, body, spare: 0 }.encode();
        assert_eq!(Carry::decode(&wire).body, body, "decode lost bits");
        assert_eq!(
            CarryView::from_wire(&wire).body(),
            body,
            "view disagreed with decode on a 68-bit carry"
        );
    }
}

#[test]
fn a_mutable_view_writes_through_the_trait_into_the_caller_s_buffer() {
    let mut wire = [0u8; 2];
    let view = <StatusWordView as View>::from_slice_mut(&mut wire).expect("two bytes");
    view.set_ready(true);
    view.set_count(1000);
    let read = <StatusWordView as View>::from_slice(&wire).expect("two bytes");
    assert_eq!((read.ready(), read.count()), (true, 1000));
    assert!(<StatusWordView as View>::from_slice_mut(&mut [0u8; 3]).is_none(), "wrong length");
}
