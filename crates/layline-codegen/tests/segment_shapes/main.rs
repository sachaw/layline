#[path = "../support/mod.rs"]
mod support;

use layline_codegen::{
    Absence, Arm, By, ChoiceDef, Collection, Container, Count, Discriminant, Endian, Field,
    Invalid, Kind, LayoutDef, Len, MessageDef, Presence, Scalar, Segment, Stated,
};

use support::{field, msg, validate_choice, validate_layout, validate_message};

mod bits;
mod discovered;
mod names;
mod optional;
mod rules;
mod shapes;

fn choice(name: &str, open: bool, arms: Vec<Arm>) -> ChoiceDef {
    let c = ChoiceDef::new(name, arms);
    if open { c.with_other("Unknown") } else { c }
}

fn opt(flag: &str, mask: u64, field: Field) -> Segment {
    Segment::Opt { when: Presence::Mask { field: flag.into(), mask }, field }
}

fn switch(name: &str, on: Discriminant, ty: &str) -> Segment {
    Segment::Switch {
        name: name.into(),
        on,
        choice: ty.into(),
        window: None,
        doc: None,
        stated: None,
    }
}

fn switch_in(name: &str, on: Discriminant, ty: &str, n: usize) -> Segment {
    Segment::Switch {
        name: name.into(),
        on,
        choice: ty.into(),
        window: Some(Len::Bytes(n)),
        doc: None,
        stated: None,
    }
}

fn as_message(layout: &LayoutDef) -> MessageDef {
    let endian = match layout.container {
        Container::Word { endian, .. }
        | Container::Bytes { endian, .. }
        | Container::Words { endian, .. } => endian,
        _ => unreachable!("there are three container kinds"),
    };
    MessageDef::new(&layout.name, vec![Segment::Block(layout.fields.clone())]).with_endian(endian)
}
