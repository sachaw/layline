//! A message that contains another of its own type: BER with short-form lengths.
//! Every octet is computed by hand from X.690.

#![cfg(feature = "derive")]

use layline::{Layout, MAX_NESTING_DEPTH, Message, ParseError};

/// The BER identifier octet (X.690 §8.1.2): tag in bits 0..5, constructed in bit 5, class in bits 6..8.
#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bits = 8)]
pub struct Identifier {
    #[bits(5)]
    pub tag: u8,
    #[bits(1)]
    pub constructed: u8,
    #[bits(2)]
    pub class: u8,
}

/// The length octet is in each arm.
/// A primitive's length counts bytes of value; a constructed's length counts bytes of child elements.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Element {
    #[bytes(1)]
    pub id: Identifier,
    #[switch(id.constructed)]
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(closed)]
pub enum Content {
    #[value(0)]
    Primitive(Primitive),
    #[value(1)]
    Constructed(Constructed),
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Primitive {
    pub len: u8,
    #[count(len)]
    pub value: Vec<u8>,
}

/// `#[len]`: a BER length counts bytes, and an element is several bytes wide.
#[derive(Debug, Clone, PartialEq, Message)]
pub struct Constructed {
    pub len: u8,
    #[len(len)]
    #[message]
    pub children: Vec<Element>,
}

fn prim(tag: u8, value: &[u8]) -> Element {
    Element {
        id: Identifier { tag, constructed: 0, class: 0 },
        content: Content::Primitive(Primitive { len: 0xFF, value: value.to_vec() }),
    }
}

fn ctor(tag: u8, children: Vec<Element>) -> Element {
    Element {
        id: Identifier { tag, constructed: 0, class: 0 },
        content: Content::Constructed(Constructed { len: 0xFF, children }),
    }
}

#[test]
fn a_three_deep_constructed_element_round_trips_to_the_octets_ber_prints() {
    let tree =
        ctor(16, vec![ctor(16, vec![prim(2, &[0x2A]), prim(4, &[0xDE, 0xAD])]), prim(1, &[0xFF])]);

    #[rustfmt::skip]
    let wire = vec![
        0x30, 0x0C,
        0x30, 0x07,
        0x02, 0x01, 0x2A,
        0x04, 0x02, 0xDE, 0xAD,
        0x01, 0x01, 0xFF,
    ];
    assert_eq!(tree.encode(), wire, "the octets X.690 prints");
    assert_eq!(wire.len(), 14);

    let (back, used) = Element::decode(&wire).expect("parses");
    assert_eq!(used, wire.len(), "the whole element");

    assert_eq!(back.id.constructed, 1, "the discriminant on the wire");
    assert_eq!(back.id.tag, 16);
    let Content::Constructed(outer) = &back.content else {
        panic!("the outer element is constructed")
    };
    assert_eq!(outer.len, 12, "the byte length on the wire");
    assert_eq!(outer.children.len(), 2);

    let Content::Constructed(inner) = &outer.children[0].content else {
        panic!("the first child is constructed")
    };
    assert_eq!(inner.len, 7);
    assert_eq!(inner.children.len(), 2);

    let Content::Primitive(int) = &inner.children[0].content else { panic!("primitive") };
    assert_eq!(int.len, 1);
    assert_eq!(int.value, vec![0x2A]);
    assert_eq!(inner.children[0].id.tag, 2);

    let Content::Primitive(oct) = &inner.children[1].content else { panic!("primitive") };
    assert_eq!(oct.value, vec![0xDE, 0xAD]);

    let Content::Primitive(bool_) = &outer.children[1].content else { panic!("primitive") };
    assert_eq!(bool_.value, vec![0xFF]);

    assert_eq!(back.encode(), wire);
}

#[test]
fn a_lie_at_every_level_does_not_reach_the_wire() {
    let mut tree =
        ctor(16, vec![ctor(16, vec![prim(2, &[0x2A]), prim(4, &[0xDE, 0xAD])]), prim(1, &[0xFF])]);

    fn lie(e: &mut Element) {
        e.id.constructed = match &e.content {
            Content::Primitive(_) => 1,
            Content::Constructed(_) => 0,
        };
        match &mut e.content {
            Content::Primitive(p) => p.len = 0,
            Content::Constructed(c) => {
                c.len = 0;
                for child in &mut c.children {
                    lie(child);
                }
            }
        }
    }
    lie(&mut tree);

    #[rustfmt::skip]
    let wire = vec![
        0x30, 0x0C,
        0x30, 0x07,
        0x02, 0x01, 0x2A,
        0x04, 0x02, 0xDE, 0xAD,
        0x01, 0x01, 0xFF,
    ];
    assert_eq!(tree.encode(), wire, "all eight derived numbers recomputed");
}

#[test]
fn an_empty_constructed_element_is_the_base_case() {
    let empty = ctor(16, vec![]);
    assert_eq!(empty.encode(), vec![0x30, 0x00]);
    let (back, used) = Element::decode(&[0x30, 0x00]).expect("parses");
    assert_eq!(used, 2);
    let Content::Constructed(c) = &back.content else { panic!("constructed") };
    assert_eq!(c.len, 0);
    assert!(c.children.is_empty());
}

#[test]
fn a_child_may_not_read_past_the_length_its_parent_declared() {
    #[rustfmt::skip]
    let wire = vec![
        0x30, 0x0C,
        0x30, 0x07,
        0x02, 0x01, 0x2A,
        0x04, 0x02, 0xDE, 0xAD,
        0x01, 0x01, 0xFF,
    ];
    let (back, _) = Element::decode(&wire).expect("parses");
    let Content::Constructed(outer) = &back.content else { panic!() };
    assert_eq!(outer.children.len(), 2, "the boolean is the outer sequence's");
    let Content::Constructed(inner) = &outer.children[0].content else { panic!() };
    assert_eq!(inner.children.len(), 2, "the inner sequence has its own two");

    let ragged = vec![0x30, 0x04, 0x02, 0x01, 0x2A, 0x02];
    assert!(
        matches!(
            Element::decode(&ragged),
            Err(ParseError::Short { .. } | ParseError::Malformed { .. })
        ),
        "got {:?}",
        Element::decode(&ragged),
    );

    assert!(matches!(
        Element::decode(&[0x30, 0x40, 0x02, 0x01, 0x2A]),
        Err(ParseError::Short { .. })
    ));
}

fn deep_chain(levels: usize) -> Vec<u8> {
    let mut wire = Vec::new();
    for i in 0..levels {
        let below = 2 * (levels - 1 - i);
        assert!(below <= 0xFF, "short-form lengths only");
        wire.push(0x30);
        wire.push(below as u8);
    }
    wire
}

/// One BER level costs two nested messages (the element, then the arm's body), so the deepest chain
/// that parses is half the limit.
#[test]
fn a_deep_but_legal_chain_still_parses() {
    let levels = (MAX_NESTING_DEPTH / 2) as usize;
    let wire = deep_chain(levels);
    let (back, used) = Element::decode(&wire).expect("within the bound");
    assert_eq!(used, wire.len());

    let mut depth = 0;
    let mut here = &back;
    while let Content::Constructed(c) = &here.content {
        depth += 1;
        let Some(child) = c.children.first() else { break };
        here = child;
    }
    assert_eq!(depth, levels);
    assert_eq!(back.encode(), wire, "encode returns the input bytes");
}

#[test]
fn a_chain_past_the_bound_is_a_clean_error() {
    let wire = deep_chain(70);
    assert_eq!(wire.len(), 140);
    assert_eq!(Element::decode(&wire), Err(ParseError::TooDeep { limit: MAX_NESTING_DEPTH }),);
}

#[test]
fn breadth_is_not_depth() {
    let children: Vec<Element> = (0..80).map(|_| prim(2, &[0x2A])).collect();
    let wide = Constructed { len: 0, children };
    let bytes = wide.encode();
    assert_eq!(bytes.len(), 1 + 3 * 80, "one length octet, then eighty elements");
    assert_eq!(bytes[0], 240);
    let (back, _) =
        Constructed::decode(&bytes).expect("breadth does not count toward the depth limit");
    assert_eq!(back.children.len(), 80);
}

#[derive(Debug, Clone, PartialEq, Message)]
pub struct Link {
    pub flags: u8,
    #[when(flags & 0x80)]
    #[message]
    pub next: Option<Box<Link>>,
}

#[test]
fn a_message_may_hold_exactly_one_of_itself_behind_a_box() {
    let chain = Link {
        flags: 0x01,
        next: Some(Box::new(Link {
            flags: 0x82,
            next: Some(Box::new(Link { flags: 0x83, next: None })),
        })),
    };
    assert_eq!(chain.encode(), vec![0x81, 0x82, 0x03]);

    let (back, used) = Link::decode(&[0x81, 0x82, 0x03]).expect("parses");
    assert_eq!(used, 3);
    assert_eq!(back.flags, 0x81);
    let second = back.next.as_ref().expect("a second link");
    assert_eq!(second.flags, 0x82);
    let third = second.next.as_ref().expect("a third link");
    assert_eq!(third.flags, 0x03);
    assert!(third.next.is_none());

    const _: () = assert!(!<Link as Message>::OPEN_ENDED);
}

#[test]
fn a_boxed_self_reference_is_bounded_too() {
    let deep = vec![0x80u8; 400];
    assert_eq!(Link::decode(&deep), Err(ParseError::TooDeep { limit: MAX_NESTING_DEPTH }),);
    let mut ok = vec![0x80u8; MAX_NESTING_DEPTH as usize];
    ok.push(0x00);
    assert!(Link::decode(&ok).is_ok());
}

#[test]
fn the_grammar_that_contains_itself_publishes_a_finite_table() {
    use layline::table::{By, Discriminant, Span, Start};
    use layline::{Choice, table::fixed_bits};

    let element = <Element as Message>::SEGMENTS;
    assert_eq!(element.len(), 2);
    assert_eq!(element[0].name, "__ElementBlock0");
    assert_eq!(element[0].start, Start::At(0));
    assert_eq!(element[0].span, Span::Fixed(8), "the identifier octet");
    assert_eq!(element[0].fields.len(), 1);
    assert_eq!(element[0].fields[0].name, "id");
    assert_eq!((element[0].fields[0].extent.start(), element[0].fields[0].extent.end()), (0, 8));

    assert_eq!(element[1].name, "content");
    assert_eq!(element[1].start, Start::At(8), "the content begins at the second byte");
    assert_eq!(
        element[1].span,
        Span::Chosen { on: Discriminant::Field("id.constructed") },
        "the row contains the flag bit inside the identifier octet"
    );
    assert_eq!(element[1].decoded_by, Some("Content"));
    assert_eq!(fixed_bits(element), 8, "the computed offsets cover one octet");

    let arms = <Content as Choice>::ARMS;
    assert_eq!(arms.len(), 2, "closed: `constructed` is one bit and both values have arms");
    assert_eq!((arms[0].name, arms[0].value), ("Primitive", Some(0)));
    assert_eq!(arms[0].segments.len(), 1);
    assert_eq!(arms[0].segments[0].span, Span::SelfDelimiting);
    assert_eq!(arms[0].segments[0].decoded_by, Some("Primitive"));
    assert_eq!((arms[1].name, arms[1].value), ("Constructed", Some(1)));
    assert_eq!(arms[1].segments[0].decoded_by, Some("Constructed"));

    let primitive = <Primitive as Message>::SEGMENTS;
    assert_eq!(primitive[0].span, Span::Fixed(8));
    assert_eq!(primitive[1].name, "value");
    assert_eq!(primitive[1].start, Start::At(8));
    assert_eq!(primitive[1].span, Span::Counted { by: By::field("len"), each: Some(8) });
    assert_eq!(fixed_bits(primitive), 8);

    let constructed = <Constructed as Message>::SEGMENTS;
    assert_eq!(constructed[0].span, Span::Fixed(8), "the length octet");
    assert_eq!(constructed[1].name, "children");
    assert_eq!(constructed[1].start, Start::At(8));
    assert_eq!(
        constructed[1].span,
        Span::Window { by: By::field("len") },
        "`#[len]` is a byte window"
    );
    assert_eq!(
        constructed[1].decoded_by,
        Some("Element"),
        "`decoded_by` is a type name, and the table ends there"
    );
    assert_eq!(fixed_bits(constructed), 8);

    assert_eq!(<Element as Message>::SEGMENTS[1].decoded_by, Some("Content"));
    assert_eq!(<Content as Choice>::ARMS[1].segments[0].decoded_by, Some("Constructed"));
    assert_eq!(<Constructed as Message>::SEGMENTS[1].decoded_by, Some("Element"));
}

/// `at` indexes the caller's buffer at any depth.
/// The short segment is the second value byte, at byte 5 of the caller's buffer.
#[test]
fn a_refusal_inside_a_nested_element_is_placed_in_the_callers_buffer() {
    assert_eq!(
        Element::decode(&[0x20, 0x03, 0x04, 0x05, 0xAA]),
        Err(ParseError::Malformed { field: "children", at: 2 }),
    );

    assert_eq!(
        Element::decode(&[0x20, 0x05, 0x20, 0x03, 0x04, 0x05, 0xAA]),
        Err(ParseError::Malformed { field: "children", at: 4 }),
    );

    assert_eq!(
        Element::decode(&[0x20, 0x09, 0x04, 0x00]),
        Err(ParseError::Short { need_bytes: 10, got_bytes: 3, at: 2 }),
    );
}
