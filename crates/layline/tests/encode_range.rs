//! Encode refuses a value too wide for its field, in every mode.

#![cfg(feature = "derive")]

use layline::{EncodeError, Layout};

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 16)]
struct Word {
    #[bits(4)]
    small: u8,
    #[bits(4)]
    signed: i8,
    #[bits(8)]
    rest: u8,
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(words = 1)]
struct Words {
    #[bits(4)]
    small: u8,
    #[bits(12)]
    rest: u16,
}

#[test]
fn a_value_too_wide_for_its_field_is_refused_in_the_container_grid() {
    assert_eq!(
        Word { small: 200, signed: 0, rest: 0 }.encode(),
        Err(EncodeError::Unfit { field: "small", value: 200, bits: 4 })
    );
}

#[test]
fn a_value_too_wide_for_its_field_is_refused_in_the_fixed_grid() {
    assert_eq!(
        Words { small: 200, rest: 0 }.encode(),
        Err(EncodeError::Unfit { field: "small", value: 200, bits: 4 })
    );
}

#[test]
fn a_signed_field_still_accepts_its_whole_range() {
    for v in -8..=7i8 {
        let w = Word { small: 0, signed: v, rest: 0 };
        assert_eq!(Word::decode(&w.encode().unwrap()).signed, v, "signed {v}");
    }
}

#[test]
fn a_signed_value_past_its_width_is_refused() {
    assert_eq!(
        Word { small: 0, signed: 8, rest: 0 }.encode(),
        Err(EncodeError::Unfit { field: "signed", value: 8, bits: 4 })
    );
}

mod derived {
    use layline::{EncodeError, Message};

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Counted {
        n: u8,
        #[count(n)]
        items: Vec<u8>,
    }

    #[test]
    fn a_count_too_large_for_its_field_is_refused() {
        let ok = Counted { n: 0, items: vec![7; 255] };
        assert_eq!(ok.encode().unwrap()[0], 255, "255 is the largest count a `u8` field contains");

        assert_eq!(
            Counted { n: 0, items: vec![7; 256] }.encode(),
            Err(EncodeError::Unfit { field: "n", value: 256, bits: 0 })
        );
    }

    #[derive(Debug, Clone, PartialEq, layline::Layout)]
    #[layout(bytes = 2)]
    struct Entry {
        tag: u16,
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Directory {
        table_offset: u8,
        pad_len: u8,
        #[count(pad_len)]
        pad: Vec<u8>,
        table_len: u8,
        #[count(table_len)]
        #[seek(table_offset)]
        #[bytes(2)]
        entries: Vec<Entry>,
    }

    #[test]
    fn an_offset_too_large_for_its_field_is_refused() {
        assert_eq!(
            Directory {
                table_offset: 0,
                pad_len: 255,
                pad: vec![0xEE; 255],
                table_len: 1,
                entries: vec![Entry { tag: 0xBEEF }],
            }
            .encode(),
            Err(EncodeError::Unfit { field: "table_offset", value: 258, bits: 0 })
        );
    }
}

/// Encode refuses a computed count or length that decode would read back differently.
mod stated_exactly {
    use layline::{EncodeError, Message};

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Paired {
        n: u8,
        #[count(n * 2)]
        items: Vec<u8>,
    }

    #[test]
    fn a_count_that_is_not_a_whole_number_of_steps_is_refused() {
        assert_eq!(
            Paired { n: 0, items: vec![1, 2, 3] }.encode(),
            Err(EncodeError::Inexact { field: "n", value: 3 })
        );
    }

    #[test]
    fn a_count_that_is_a_whole_number_of_steps_round_trips() {
        let m = Paired { n: 0, items: vec![1, 2, 3, 4] };
        let wire = m.encode().unwrap();
        assert_eq!(wire, [2, 1, 2, 3, 4]);
        assert_eq!(Paired::decode(&wire), Ok((Paired { n: 2, ..m }, 5)));
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Words {
        words: u8,
        #[text]
        #[len(words * 2, cap = 4)]
        name: String,
    }

    #[test]
    fn a_string_length_that_is_not_a_whole_number_of_steps_is_refused() {
        assert_eq!(
            Words { words: 0, name: "ABC".into() }.encode(),
            Err(EncodeError::Inexact { field: "words", value: 3 })
        );
    }

    #[test]
    fn a_string_past_its_cap_is_refused() {
        assert_eq!(
            Words { words: 0, name: "ABCDEF".into() }.encode(),
            Err(EncodeError::OverCap { field: "name", len: 6, cap: 4 })
        );
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Spanned {
        words: u8,
        #[len(words * 2)]
        bytes: Vec<u8>,
    }

    #[test]
    fn a_byte_length_that_is_not_a_whole_number_of_steps_is_refused() {
        assert_eq!(
            Spanned { words: 0, bytes: vec![1, 2, 3] }.encode(),
            Err(EncodeError::Inexact { field: "words", value: 3 })
        );
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Counted {
        n: u8,
        #[count(n, cap = 2)]
        items: Vec<u8>,
    }

    #[test]
    fn a_count_past_its_cap_is_refused() {
        assert_eq!(
            Counted { n: 0, items: vec![1, 2, 3] }.encode(),
            Err(EncodeError::OverCap { field: "items", len: 3, cap: 2 })
        );
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Filled {
        #[fill(cap = 2)]
        items: Vec<u8>,
    }

    #[test]
    fn a_fill_past_its_cap_is_refused() {
        assert_eq!(
            Filled { items: vec![1, 2, 3] }.encode(),
            Err(EncodeError::OverCap { field: "items", len: 3, cap: 2 })
        );
    }
}
