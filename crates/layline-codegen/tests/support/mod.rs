#![allow(dead_code)]

use layline_codegen::{
    ChoiceDef, Container, DispatchDef, Endian, Field, Invalid, Item, Kind, LayoutDef, MessageDef,
    Scalar, Segment, validate,
};

pub fn field(name: &str, kind: Kind) -> Field {
    Field::new(name, kind)
}

pub fn u(bits: u64) -> Kind {
    Kind::Scalar(Scalar::U(bits))
}

/// A little-endian byte-mode layout of `len` bytes.
pub fn bytes(len: usize, fields: Vec<Field>) -> LayoutDef {
    LayoutDef::new("L", Container::Bytes { bytes: len, endian: Endian::Le }, fields)
}

pub fn msg(name: &str, segments: Vec<Segment>) -> MessageDef {
    MessageDef::new(name, segments)
}

pub fn validate_layout(l: &LayoutDef) -> Result<(), Invalid> {
    validate(&Item::Layout(l.clone()))
}

pub fn validate_message(m: &MessageDef) -> Result<(), Invalid> {
    validate(&Item::Message(m.clone()))
}

pub fn validate_choice(c: &ChoiceDef) -> Result<(), Invalid> {
    validate(&Item::Choice(c.clone()))
}

pub fn validate_dispatch(d: &DispatchDef) -> Result<(), Invalid> {
    validate(&Item::Dispatch(d.clone()))
}
