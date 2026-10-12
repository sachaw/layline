//! Numbers encode derives or checks: several stamps into one nested record, a `#[range]` on a
//! stamped length, and parameters a holder writes. Expected bytes are laid out by hand.

#![cfg(feature = "derive")]

use layline::{Layout, Message};

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 12)]
pub struct Info {
    pub n_a: u16,
    pub n_b: u16,
    pub off_a: u32,
    pub off_b: u32,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Two {
    #[bytes(12)]
    pub info: Info,
    #[seek(info.off_a)]
    #[count(info.n_a)]
    pub a: Vec<u8>,
    #[seek(info.off_b)]
    #[count(info.n_b)]
    pub b: Vec<u8>,
}

#[test]
fn two_offsets_stamped_into_one_nested_record_both_land() {
    let two = Two { info: Info { n_a: 0, n_b: 0, off_a: 0, off_b: 0 }, a: vec![1], b: vec![2, 2] };
    // n_a 1, n_b 2, off_a 12 (right after the 12-byte record), off_b 13 (after `a`'s one byte).
    let wire = [1, 0, 2, 0, 12, 0, 0, 0, 13, 0, 0, 0, 1, 2, 2];
    assert_eq!(two.encode(), wire);
    let (back, used) = Two::decode(&wire).expect("decodes");
    assert_eq!(used, wire.len());
    assert_eq!(back, Two { info: Info { n_a: 1, n_b: 2, off_a: 12, off_b: 13 }, ..two });
}

#[derive(Layout, Debug, Clone, PartialEq)]
#[layout(bytes = 8)]
pub struct Span {
    pub off: u32,
    pub size: u32,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Windowed {
    #[bytes(8)]
    pub at: Span,
    #[seek(at.off)]
    #[len(at.size)]
    pub data: Vec<u8>,
}

#[test]
fn an_offset_and_a_size_stamped_into_one_nested_record_both_land() {
    let w = Windowed { at: Span { off: 0, size: 0 }, data: vec![7, 8, 9] };
    // off 8, size 3, then the three bytes.
    assert_eq!(w.encode(), [8, 0, 0, 0, 3, 0, 0, 0, 7, 8, 9]);
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Rec {
    pub tag: u16,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Located {
    #[bytes(8)]
    pub at: Span,
    #[seek(at.off)]
    #[when(at.size > 0)]
    #[message]
    pub rec: Option<Rec>,
}

#[test]
fn a_present_records_offset_and_size_both_land() {
    let l = Located { at: Span { off: 0, size: 0 }, rec: Some(Rec { tag: 0xBEEF }) };
    // off 8, size 2, then the record.
    let wire = [8, 0, 0, 0, 2, 0, 0, 0, 0xEF, 0xBE];
    assert_eq!(l.encode(), wire);
    assert_eq!(Located::decode(&wire).expect("decodes").0.rec, Some(Rec { tag: 0xBEEF }));
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Ranged {
    #[range(1..=100)]
    pub total: u16,
    #[len(total)]
    pub records: Vec<u8>,
}

#[test]
fn a_ranged_length_is_checked_once_it_is_known() {
    let r = Ranged { total: 0, records: vec![1, 2, 3] };
    assert_eq!(r.encode(), [3, 0, 1, 2, 3]);
    assert_eq!(Ranged::decode(&[3, 0, 1, 2, 3]).expect("decodes").0.total, 3);
}

#[test]
#[should_panic(expected = "field `total`: value is outside `#[range(1..=100)]`")]
fn a_ranged_length_outside_its_range_is_refused() {
    let _ = Ranged { total: 0, records: vec![] }.encode();
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(with_bounds: u16))]
pub struct Cell {
    pub obj: u16,
    #[count(with_bounds, scale = 4)]
    pub bounds: Vec<i16>,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Cells {
    pub with_bounds: u16,
    pub n: u8,
    #[count(n)]
    #[with(with_bounds)]
    #[message]
    pub cells: Vec<Cell>,
}

#[test]
fn elements_that_agree_with_the_parameter_encode() {
    let c =
        Cells { with_bounds: 1, n: 0, cells: vec![Cell { obj: 7, bounds: vec![-1, 2, -3, 4] }] };
    // with_bounds 1, n 1, obj 7, four i16.
    let wire = [1, 0, 1, 7, 0, 0xFF, 0xFF, 2, 0, 0xFD, 0xFF, 4, 0];
    assert_eq!(c.encode(), wire);
    assert_eq!(Cells::decode(&wire).expect("decodes").0, Cells { n: 1, ..c });
}

#[test]
#[should_panic(
    expected = "collection `bounds`: its length is not what parameter `with_bounds` states"
)]
fn a_count_the_parameter_does_not_state_is_refused() {
    let _ = Cells { with_bounds: 1, n: 0, cells: vec![Cell { obj: 7, bounds: vec![] }] }.encode();
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(len: u8))]
pub struct Named {
    #[text]
    #[len(len)]
    pub name: String,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Names {
    pub len: u8,
    #[with(len)]
    #[message]
    pub first: Named,
}

#[test]
#[should_panic(
    expected = "field `name`: the text's byte length is not what parameter `len` states"
)]
fn a_text_the_parameter_does_not_size_is_refused() {
    let _ = Names { len: 2, first: Named { name: String::from("abc") } }.encode();
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(closed)]
pub enum Body {
    #[value(1)]
    One(u8),
    #[value(2)]
    Two(u16),
}

#[derive(Message, Debug, Clone, PartialEq)]
#[message(needs(kind: u8))]
pub struct Keyed {
    #[switch(kind)]
    pub body: Body,
}

#[derive(Message, Debug, Clone, PartialEq)]
pub struct Holder {
    pub kind: u8,
    #[with(kind)]
    #[message]
    pub keyed: Keyed,
}

#[test]
fn an_arm_the_parameter_selects_encodes() {
    let h = Holder { kind: 2, keyed: Keyed { body: Body::Two(0x0102) } };
    assert_eq!(h.encode(), [2, 2, 1]);
}

#[test]
#[should_panic(expected = "field `body`: the arm's discriminant is not parameter `kind`")]
fn an_arm_the_parameter_does_not_select_is_refused() {
    let _ = Holder { kind: 1, keyed: Keyed { body: Body::Two(0x0102) } }.encode();
}
