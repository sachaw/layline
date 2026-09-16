//! Rules for bit-addressed messages.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{Invalid, field_names};
use crate::{Endian, Field, Kind, MessageDef, Presence, Scalar, Segment};

/// Checks a bit-addressed message.
///
/// Only bit fields, `#[var]` values and optional fields are allowed. The other segment kinds need
/// byte positions.
pub(super) fn validate_bits(msg: &MessageDef) -> Result<(), Invalid> {
    if msg.endian == Endian::Be {
        return Err(Invalid::Other(format!(
            "message `{}`: a bit-addressed message has no byte order. Remove the byte order",
            msg.name
        )));
    }
    if msg.segments.iter().all(|seg| matches!(seg, Segment::Block(fields) if fields.is_empty())) {
        return Err(Invalid::Other(format!(
            "message `{}`: a bit-addressed message needs at least one field. \
             Declare a field, or remove the bit addressing",
            msg.name
        )));
    }
    let mut seen: Vec<BitSeen<'_>> = Vec::new();
    let mut governed: Vec<(&str, u64, &str)> = Vec::new();
    for seg in &msg.segments {
        match seg {
            Segment::Block(fields) => {
                for f in fields {
                    bit_field(f, false)?;
                    seen.push(BitSeen { name: &f.name, kind: &f.kind, optional: false });
                }
            }
            Segment::Opt { when, field } => {
                bit_field(field, true)?;
                match when {
                    Presence::Bit => {}
                    Presence::Mask { field: flag, mask } => {
                        bit_flag(&seen, &field.name, flag)?;
                        bit_mask(&seen, &field.name, flag, *mask)?;
                        bit_alone(&governed, &field.name, flag, *mask)?;
                        governed.push((flag, *mask, &field.name));
                    }
                    Presence::Flag { field: flag } => {
                        bit_flag(&seen, &field.name, flag)?;
                        bit_bool(&seen, &field.name, flag)?;
                        bit_alone(&governed, &field.name, flag, 1)?;
                        governed.push((flag, 1, &field.name));
                    }
                    Presence::Remaining => {
                        return Err(Invalid::field(
                            &field.name,
                            String::from(
                                "a bit-addressed message has no trailing bytes to test. \
                                 Write `#[present]`, or name an earlier bit field",
                            ),
                        ));
                    }
                }
                seen.push(BitSeen { name: &field.name, kind: &field.kind, optional: true });
            }
            Segment::Value { name, kind: kind @ Kind::Var { .. }, .. } => {
                seen.push(BitSeen { name, kind, optional: false });
            }
            other => {
                let what = segment_word(other);
                let name = other.name().unwrap_or("?");
                return Err(Invalid::field(
                    name,
                    format!(
                        "{what} cannot appear in a bit-addressed message. \
                         Declare it in the message that holds this one"
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn segment_word(seg: &Segment) -> &'static str {
    match seg {
        Segment::Block(_) | Segment::Opt { .. } => "a field",
        Segment::Value { .. } => "a variable-length value",
        Segment::Repeat { .. } => "a collection",
        Segment::Placed { .. } => "a record at an offset",
        Segment::Switch { .. } => "a switch",
        Segment::Checksum { .. } => "a checksum",
    }
}

/// Checks one bit field: a readable width of 1 to 64 bits.
///
/// `alone` is true for a segment of its own, which may be a `#[var]` value.
fn bit_field(f: &Field, alone: bool) -> Result<(), Invalid> {
    field_names(f)?;
    if alone && matches!(f.kind, Kind::Var { .. }) {
        return Ok(());
    }
    let bits = match &f.kind {
        Kind::Scalar(s) => s.bits(),
        Kind::Codec { bits, .. } => *bits,
        Kind::Var { .. } => {
            return Err(Invalid::field(
                &f.name,
                String::from(
                    "a variable-length value in a block of fields at constant offsets. \
                     Make it a `Segment::Value` of its own",
                ),
            ));
        }
        other => {
            return Err(Invalid::field(
                &f.name,
                format!(
                    "{} has no bit width. \
                     Declare a scalar, a `U<N>`, or a `FieldCodec` with a fixed width",
                    other.describe()
                ),
            ));
        }
    };
    if bits == 0 {
        return Err(Invalid::field(
            &f.name,
            String::from("a field of 0 bits. Declare a width of 1 bit or more"),
        ));
    }
    if bits > 64 {
        return Err(Invalid::field(
            &f.name,
            format!("a field of {bits} bits, and a bit field holds at most 64. Split it in two"),
        ));
    }
    Ok(())
}

/// A field read before the current one.
struct BitSeen<'a> {
    name: &'a str,
    kind: &'a Kind,
    optional: bool,
}

/// Checks the flag a `#[when]` reads: an earlier, always-present field of fixed width.
fn bit_flag(seen: &[BitSeen<'_>], field: &str, flag: &str) -> Result<(), Invalid> {
    let Some(known) = seen.iter().find(|k| k.name == flag) else {
        return Err(Invalid::field(
            field,
            format!(
                "`{flag}` is not a field this message reads before `{field}`. \
                 Declare `{flag}` as an earlier bit field, or write `#[present]`"
            ),
        ));
    };
    if known.optional {
        return Err(Invalid::field(
            field,
            format!(
                "the flag `{flag}` is itself sometimes absent. \
                 Name a field that is always present, or write `#[present]`"
            ),
        ));
    }
    if known.kind.bits().is_none() {
        return Err(Invalid::field(
            field,
            format!(
                "the flag `{flag}` reads its width off the wire, so encode cannot write the \
                 presence bits into it. Name a `#[bits(N)]` field, or write `#[present]`"
            ),
        ));
    }
    Ok(())
}

/// Checks the mask of `#[when(f & MASK)]`: nonzero, and within the flag's width.
fn bit_mask(seen: &[BitSeen<'_>], field: &str, flag: &str, mask: u64) -> Result<(), Invalid> {
    if mask == 0 {
        return Err(Invalid::field(
            field,
            format!(
                "`{flag} & 0` masks no bits, so the field is never present. \
                 Write the bit that marks it present"
            ),
        ));
    }
    let Some(known) = seen.iter().find(|k| k.name == flag) else { return Ok(()) };
    let Some(bits) = known.kind.bits() else { return Ok(()) };
    if !known.kind.is_integer() {
        return Err(Invalid::field(
            field,
            format!(
                "`{flag}` is {}, and a mask tests the bits of an integer. \
                 Write `#[when({flag})]`, or declare `{flag}` as a `#[bits(N)]` integer",
                known.kind.describe(),
            ),
        ));
    }
    if bits < 64 && mask >> bits != 0 {
        return Err(Invalid::field(
            field,
            format!(
                "the mask reaches bit {} of `{flag}`, which is {bits} bits wide. \
                 Write a bit of 0..={}, or widen `{flag}`",
                63 - mask.leading_zeros(),
                bits - 1,
            ),
        ));
    }
    Ok(())
}

/// Refuses a presence bit that an earlier optional field already uses.
fn bit_alone(
    governed: &[(&str, u64, &str)],
    field: &str,
    flag: &str,
    mask: u64,
) -> Result<(), Invalid> {
    let Some((_, taken, other)) = governed.iter().find(|(f, m, _)| *f == flag && m & mask != 0)
    else {
        return Ok(());
    };
    Err(Invalid::field(
        field,
        format!(
            "bit {} of `{flag}` already marks `{other}` present. \
             Use a bit no other field uses, or widen `{flag}`",
            63 - (taken & mask).leading_zeros(),
        ),
    ))
}

/// Checks the flag of a bare `#[when(f)]`: a `bool`.
fn bit_bool(seen: &[BitSeen<'_>], field: &str, flag: &str) -> Result<(), Invalid> {
    let Some(known) = seen.iter().find(|k| k.name == flag) else {
        return Ok(());
    };
    if !matches!(known.kind, Kind::Scalar(Scalar::Bool)) {
        return Err(Invalid::field(
            field,
            format!(
                "`{flag}` is {} and a bare test reads a `bool`. \
                 Write `#[when({flag} & 0x01)]`, or declare `{flag}` as a `bool`",
                known.kind.describe(),
            ),
        ));
    }
    Ok(())
}
