//! Encode refuses a value too wide for its field, in every mode.

#![cfg(feature = "derive")]

use layline::Layout;

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
#[should_panic(expected = "field `small`: value does not fit #[bits(4)]")]
fn a_value_too_wide_for_its_field_is_refused_in_the_container_grid() {
    let _ = Word { small: 200, signed: 0, rest: 0 }.encode();
}

#[test]
#[should_panic(expected = "field `small`: value does not fit #[bits(4)]")]
fn a_value_too_wide_for_its_field_is_refused_in_the_fixed_grid() {
    let _ = Words { small: 200, rest: 0 }.encode();
}

#[test]
fn a_signed_field_still_accepts_its_whole_range() {
    for v in -8..=7i8 {
        let w = Word { small: 0, signed: v, rest: 0 };
        assert_eq!(Word::decode(&w.encode()).signed, v, "signed {v}");
    }
}

#[test]
#[should_panic(expected = "field `signed`: value does not fit #[bits(4)]")]
fn a_signed_value_past_its_width_is_refused() {
    let _ = Word { small: 0, signed: 8, rest: 0 }.encode();
}

mod derived {
    use layline::Message;

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Counted {
        n: u8,
        #[count(n)]
        items: Vec<u8>,
    }

    #[test]
    #[should_panic(
        expected = "field `n` is too narrow for the length of a collection the wire derives it from"
    )]
    fn a_count_too_large_for_its_field_is_refused() {
        let ok = Counted { n: 0, items: vec![7; 255] };
        assert_eq!(ok.encode()[0], 255, "255 is the largest count a `u8` field contains");

        let _ = Counted { n: 0, items: vec![7; 256] }.encode();
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
    #[should_panic(
        expected = "field `table_offset` is too narrow for the position of a collection the wire \
                    derives it from"
    )]
    fn an_offset_too_large_for_its_field_is_refused() {
        let _ = Directory {
            table_offset: 0,
            pad_len: 255,
            pad: vec![0xEE; 255],
            table_len: 1,
            entries: vec![Entry { tag: 0xBEEF }],
        }
        .encode();
    }
}

/// Encode refuses a computed count or length that decode would read back differently.
mod stated_exactly {
    use layline::Message;

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Paired {
        n: u8,
        #[count(n, scale = 2)]
        items: Vec<u8>,
    }

    #[test]
    #[should_panic(expected = "field `n` cannot state the length of a collection exactly")]
    fn a_count_that_is_not_a_whole_number_of_steps_is_refused() {
        let _ = Paired { n: 0, items: vec![1, 2, 3] }.encode();
    }

    #[test]
    fn a_count_that_is_a_whole_number_of_steps_round_trips() {
        let m = Paired { n: 0, items: vec![1, 2, 3, 4] };
        let wire = m.encode();
        assert_eq!(wire, [2, 1, 2, 3, 4]);
        assert_eq!(Paired::decode(&wire), Ok((Paired { n: 2, ..m }, 5)));
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Words {
        words: u8,
        #[text]
        #[len(words, scale = 2, cap = 4)]
        name: String,
    }

    #[test]
    #[should_panic(expected = "field `words` cannot state the encoded length of a string exactly")]
    fn a_string_length_that_is_not_a_whole_number_of_steps_is_refused() {
        let _ = Words { words: 0, name: "ABC".into() }.encode();
    }

    #[test]
    #[should_panic(expected = "field `words`: the encoded length of a string passes its cap of 4")]
    fn a_string_past_its_cap_is_refused() {
        let _ = Words { words: 0, name: "ABCDEF".into() }.encode();
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Spanned {
        words: u8,
        #[len(words, scale = 2)]
        bytes: Vec<u8>,
    }

    #[test]
    #[should_panic(expected = "field `words` cannot state the byte length of a collection exactly")]
    fn a_byte_length_that_is_not_a_whole_number_of_steps_is_refused() {
        let _ = Spanned { words: 0, bytes: vec![1, 2, 3] }.encode();
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Counted {
        n: u8,
        #[count(n, cap = 2)]
        items: Vec<u8>,
    }

    #[test]
    #[should_panic(expected = "field `n`: the length of a collection passes its cap of 2")]
    fn a_count_past_its_cap_is_refused() {
        let _ = Counted { n: 0, items: vec![1, 2, 3] }.encode();
    }

    #[derive(Debug, Clone, PartialEq, Message)]
    struct Filled {
        #[fill(cap = 2)]
        items: Vec<u8>,
    }

    #[test]
    #[should_panic(expected = "collection `items` passes its cap of 2 elements")]
    fn a_fill_past_its_cap_is_refused() {
        let _ = Filled { items: vec![1, 2, 3] }.encode();
    }
}
