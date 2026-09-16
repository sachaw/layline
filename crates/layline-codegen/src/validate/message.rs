//! Rules for messages and choices.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::bits::validate_bits;
use super::coverage::{Places, check_value, coverage, stampable};
use super::names::{Known, Reads, Seen, read_only, resolve, stated_with};
use super::{Invalid, field_names, ident, kind_paths, type_path};
use crate::{
    Absence, ChoiceDef, Count, Discriminant, Kind, Len, MessageDef, Param, Presence, Segment,
    Stated,
};

/// Checks a message: every reference names an earlier field, and nothing follows a segment that
/// reads to the end of the body.
pub(crate) fn validate_message(msg: &MessageDef) -> Result<(), Invalid> {
    ident("message", &msg.name)?;
    let mut seen = needs(msg)?;
    if msg.bits.is_some() {
        return validate_bits(msg);
    }
    validate_segments(&msg.segments, &mut seen)
}

/// Checks a choice: distinct arm values, and each arm a valid body.
///
/// An arm cannot refer to fields outside itself.
pub(crate) fn validate_choice(choice: &ChoiceDef) -> Result<(), Invalid> {
    ident("choice", &choice.name)?;
    if let Some(other) = &choice.other {
        ident("fallback arm", other)?;
    }
    let mut values: Vec<u64> = Vec::new();
    for arm in &choice.arms {
        ident("arm", &arm.name)?;
        if values.contains(&arm.value) {
            return Err(Invalid::DuplicateArm(arm.value));
        }
        values.push(arm.value);
        validate_segments(&arm.body, &mut Vec::new())?;
    }
    if choice.arms.is_empty() && choice.other.is_none() {
        return Err(Invalid::Other(format!(
            "choice `{}` has no arms and no fallback. Declare an arm, or add a fallback",
            choice.name
        )));
    }
    Ok(())
}

/// Checks the parameters and returns them as the first names a body can refer to.
fn needs(msg: &MessageDef) -> Result<Seen<'_>, Invalid> {
    if !msg.needs.is_empty() && msg.bits.is_some() {
        return Err(Invalid::Other(format!(
            "message `{}`: a bit-addressed message cannot take parameters. \
             Remove the bit addressing, or remove the parameters",
            msg.name
        )));
    }
    let mut seen: Seen<'_> = Vec::new();
    let fields = declared(&msg.segments);
    for Param { name, repr } in &msg.needs {
        ident("parameter", name)?;
        if seen.iter().any(|k| k.name == name.as_str()) {
            return Err(Invalid::Other(format!(
                "message `{}` declares the parameter `{name}` twice. Rename one",
                msg.name
            )));
        }
        if fields.contains(&name.as_str()) {
            return Err(Invalid::Other(format!(
                "message `{}`: `{name}` is both a parameter and a field. Rename one",
                msg.name
            )));
        }
        if !Kind::Scalar(*repr).is_integer() {
            return Err(Invalid::Other(format!(
                "message `{}`: parameter `{name}` is `{}`. A parameter must be an integer",
                msg.name,
                repr.primitive(),
            )));
        }
        seen.push(Known { name, kind: Kind::Scalar(*repr), param: true });
    }
    Ok(seen)
}

/// Every field name the body declares.
fn declared(segments: &[Segment]) -> Vec<&str> {
    let mut out = Vec::new();
    for seg in segments {
        match seg {
            Segment::Block(fields) => out.extend(fields.iter().map(|f| f.name.as_str())),
            Segment::Opt { field, .. } => out.push(field.name.as_str()),
            other => out.extend(other.name()),
        }
    }
    out
}

/// The current bit position, or `None` after a variable-length segment.
type Cursor = Option<u64>;

fn advance(pos: Cursor, bits: Option<u64>) -> Cursor {
    Some(pos? + bits?)
}

/// Checks an `#[at]` position against the cursor.
fn stated_here(pos: Cursor, name: &str, stated: Stated) -> Result<(), Invalid> {
    let Some(pos) = pos else {
        return Err(Invalid::field(
            name,
            String::from(
                "`#[at]` after a variable-length segment, whose offset is only known at run time. \
                 Put `#[at]` on an earlier field",
            ),
        ));
    };
    stated.check(name, pos).map_err(Invalid::Stated)
}

fn validate_segments<'a>(segments: &'a [Segment], seen: &mut Seen<'a>) -> Result<(), Invalid> {
    let mut open_ended: Option<&str> = None;
    let places: Places<'a> = segments.iter().flat_map(Segment::places).collect();
    let optional: Places<'a> = segments.iter().filter_map(Segment::placed_optionally).collect();
    let mut pos: Cursor = Some(0);
    for seg in segments {
        if let Some(who) = open_ended {
            return Err(Invalid::AfterOpenEnd(format!(
                "`{who}` reads to the end of the body, so nothing can follow it"
            )));
        }
        if let Some(name) = seg.name() {
            ident("field", name)?;
        }
        if let Some(stated) = seg.stated() {
            stated_here(pos, seg.name().unwrap_or("?"), stated)?;
        }
        match seg {
            Segment::Block(fields) => {
                let mut at = pos;
                for f in fields {
                    field_names(f)?;
                    if f.kind.bits().is_none() {
                        return Err(Invalid::Other(format!(
                            "field `{}`: a variable-length field cannot be in a block. \
                             Make it a `Segment::Value` of its own",
                            f.name
                        )));
                    }
                    if matches!(f.kind, Kind::Checksum { .. }) {
                        return Err(Invalid::Other(format!(
                            "field `{}`: a checksum cannot be in a block. \
                             Make it a `Segment::Checksum` of its own",
                            f.name,
                        )));
                    }
                    if let Some(stated) = f.stated {
                        stated_here(at, &f.name, stated)?;
                    }
                    at = advance(at, f.kind.bits());
                    seen.push(Known::field(&f.name, &f.kind));
                }
            }
            Segment::Value { name, kind, .. } => {
                kind_paths(name, kind)?;
                match kind {
                    Kind::Var { .. } | Kind::Msg { .. } => {}
                    Kind::Text { len, .. } => match len {
                        Len::Field { by, .. } => resolve(seen, &by.field, Reads::Number)?,
                        Len::Bytes(_) | Len::Until { .. } => {}
                        Len::Fill => open_ended = Some("text(fill)"),
                    },
                    other => {
                        return Err(Invalid::field(
                            name,
                            format!(
                                "`Segment::Value` takes a `Kind::Var`, `Kind::Msg` or \
                                 `Kind::Text`, and {other:?} has a fixed width. Put it in a block"
                            ),
                        ));
                    }
                }
                stated_with(seen, name, kind)?;
                seen.push(Known::field(name, kind));
            }
            Segment::Repeat { name, element, count, at, .. } => {
                kind_paths(name, element)?;
                variable_element(name, element)?;
                match count {
                    Count::Field { by, .. } | Count::Squared { by, .. } => {
                        resolve(seen, &by.field, Reads::Number)?;
                    }
                    Count::Window(Len::Until { .. }) => {
                        if element.bits() != Some(8) {
                            return Err(Invalid::Other(format!(
                                "collection `{name}`: a run ended by a terminator byte needs \
                                 one-byte elements, not {element:?}. \
                                 Use `Count::Terminated` for wider elements"
                            )));
                        }
                    }
                    Count::Terminated { mask } => {
                        if *mask == 0 {
                            return Err(Invalid::Other(format!(
                                "collection `{name}`: a mask of zero never marks the last \
                                 element. Write the bits the last element sets"
                            )));
                        }
                        if matches!(element, Kind::Var { .. } | Kind::Text { .. }) {
                            return Err(Invalid::Other(format!(
                                "collection `{name}`: a terminated run needs fixed-width \
                                 elements, and this element is variable-length. Give it a count"
                            )));
                        }
                    }
                    Count::Strided { by, stride, .. } => {
                        resolve(seen, &by.field, Reads::Number)?;
                        resolve(seen, &stride.field, Reads::Number)?;
                        if stride.field == by.field {
                            let field = &by.field;
                            return Err(Invalid::Other(format!(
                                "collection `{name}`: `{field}` is both the count and the \
                                 stride. Use a separate field for the stride"
                            )));
                        }
                        if matches!(
                            element,
                            Kind::Var { .. } | Kind::Msg { .. } | Kind::Text { .. }
                        ) {
                            return Err(Invalid::Other(format!(
                                "collection `{name}`: a variable-length element cannot have a \
                                 stride. Give it a count (`Count::Field`)"
                            )));
                        }
                    }
                    Count::Window(len) => match len {
                        Len::Field { by, .. } => resolve(seen, &by.field, Reads::Number)?,
                        Len::Bytes(n) if at.is_some() => {
                            return Err(Invalid::Other(format!(
                                "collection `{name}`: an offset and a fixed {n} bytes give no \
                                 element count. \
                                 Use `Segment::Placed`, `Len::Field` or `Count::Field`"
                            )));
                        }
                        Len::Bytes(_) => {}
                        other => {
                            return Err(Invalid::Other(format!(
                                "collection `{name}`: a byte window of {other:?} is not \
                                 supported. Use a fixed number of bytes, or a length field"
                            )));
                        }
                    },
                    Count::Fill { .. } => open_ended = Some("repeat(fill)"),
                }
                if let Some(field) = at {
                    resolve(seen, field, Reads::Number)?;
                    read_only(seen, field, "a collection offset")?;
                }
                stated_with(seen, name, element)?;
            }
            Segment::Placed { name, kind, at, absent, .. } => {
                kind_paths(name, kind)?;
                resolve(seen, at, Reads::Number)?;
                read_only(seen, at, "a record offset")?;
                stated_with(seen, name, kind)?;
                if let Some(Absence::ZeroLength { by }) = absent {
                    resolve(seen, by, Reads::Number)?;
                    read_only(seen, by, "a record length")?;
                    if by == at {
                        return Err(Invalid::field(
                            name,
                            format!(
                                "`{by}` is both the record's offset and its length. \
                                 Use a separate field for the length"
                            ),
                        ));
                    }
                }
                match kind {
                    Kind::Nested { bytes: 0, ty } => {
                        return Err(Invalid::field(
                            name,
                            format!("`{ty}` at an offset has a size of zero bytes. Give it a size"),
                        ));
                    }
                    Kind::Nested { .. } | Kind::Msg { .. } => {}
                    other => {
                        return Err(Invalid::field(
                            name,
                            format!(
                                "a record at an offset must be a nested layout or a message, \
                                 not {other:?}. Declare a `#[derive(Layout)]` struct"
                            ),
                        ));
                    }
                }
            }
            Segment::Switch { name, on, choice, window, .. } => {
                type_path(name, "the catalogue", choice)?;
                if let Discriminant::Field(field) = on {
                    resolve(seen, field, Reads::Number)?;
                }
                match window {
                    None => {}
                    Some(Len::Bytes(0)) => {
                        return Err(Invalid::field(
                            name,
                            String::from(
                                "a union of zero bytes. \
                                 Remove the window, or give the size the format defines",
                            ),
                        ));
                    }
                    Some(Len::Bytes(n)) => {
                        if matches!(on, Discriminant::BodyLength) {
                            return Err(Invalid::Other(format!(
                                "field `{name}`: a switch on the body length cannot also have a \
                                 fixed size of {n} bytes. \
                                 Switch on a field, or remove the window"
                            )));
                        }
                    }
                    Some(Len::Field { by, .. }) => {
                        if matches!(on, Discriminant::BodyLength) {
                            return Err(Invalid::field(
                                name,
                                format!(
                                    "a switch on the body length cannot also take its size \
                                     from `{}`. Switch on a field, or remove the window",
                                    by.field,
                                ),
                            ));
                        }
                        resolve(seen, &by.field, Reads::Number)?;
                    }
                    Some(other) => {
                        return Err(Invalid::field(
                            name,
                            format!(
                                "a switch window of {other:?} is not supported. \
                                 Use a fixed number of bytes, or a length field"
                            ),
                        ));
                    }
                }
            }
            Segment::Opt { when, field } => {
                field_names(field)?;
                stated_with(seen, &field.name, &field.kind)?;
                match when {
                    Presence::Remaining => {}
                    Presence::Mask { field: flag, mask } => {
                        resolve(seen, flag, Reads::Number)?;
                        read_only(seen, flag, "presence bits")?;
                        if *mask == 0 {
                            return Err(Invalid::field(
                                &field.name,
                                format!(
                                    "the mask tests no bits of `{flag}`, so the field is never \
                                     present. Write the bit that marks it present",
                                ),
                            ));
                        }
                    }
                    Presence::Flag { field: flag } => {
                        resolve(seen, flag, Reads::Flag)?;
                        read_only(seen, flag, "presence bits")?;
                    }
                    Presence::Bit => {
                        return Err(Invalid::field(
                            &field.name,
                            String::from(
                                "a byte-addressed message has no inline presence bits. \
                                 Test a flag field, or make this message bit-addressed",
                            ),
                        ));
                    }
                }
                match &field.kind {
                    Kind::Text { len: Len::Field { by, .. }, .. } => {
                        let src = &by.field;
                        return Err(Invalid::field(
                            &field.name,
                            format!(
                                "an optional string cannot take its length from `{src}`, which \
                                 is always on the wire. Give it a terminator or a fixed length",
                            ),
                        ));
                    }
                    Kind::Text { len: Len::Fill, .. } => open_ended = Some("optional text(fill)"),
                    Kind::Text { len, .. } if !matches!(len, Len::Bytes(_) | Len::Until { .. }) => {
                        return Err(Invalid::field(
                            &field.name,
                            format!(
                                "optional text of {len:?} length. \
                                 Give it a terminator or a fixed length",
                            ),
                        ));
                    }
                    _ => {}
                }
            }
            Segment::Checksum { name, repr, algorithm, over, .. } => {
                check_value(name, repr, algorithm)?;
                coverage(&places, &optional, name, over)?;
            }
        }
        pos = advance(pos, seg.fixed_bits());
    }
    stampable(segments)?;
    Ok(())
}

/// Refuses a text element that does not end itself.
fn variable_element(name: &str, element: &Kind) -> Result<(), Invalid> {
    let Kind::Text { len, .. } = element else {
        return Ok(());
    };
    match len {
        Len::Bytes(_) | Len::Until { .. } => Ok(()),
        other => Err(Invalid::Other(format!(
            "collection `{name}`: text elements of {other:?} length have no end. \
             Give each a terminator or a fixed length",
        ))),
    }
}
