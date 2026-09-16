//! References between items, used by the recursion, size and open-end checks.

use crate::{Count, Field, Kind, Len, Segment};

/// The kind of item a [`Reference`] refers to, with its type name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Referent<'a> {
    /// A [`MessageDef`](crate::MessageDef): a [`Kind::Msg`].
    Message(&'a str),
    /// A [`ChoiceDef`](crate::ChoiceDef): the `choice` of a [`Segment::Switch`].
    Choice(&'a str),
}

impl<'a> Referent<'a> {
    pub(crate) fn name(self) -> &'a str {
        match self {
            Referent::Message(n) | Referent::Choice(n) => n,
        }
    }
}

/// A reference from one item to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reference<'a> {
    pub(crate) to: Referent<'a>,
    /// The field that holds the reference.
    pub(crate) field: &'a str,
    /// Whether some valid wire skips this reference, giving a recursion a way to end.
    pub(crate) avoidable: bool,
    /// Whether the Rust value holds the target behind a pointer.
    pub(crate) indirect: bool,
}

impl Segment {
    /// The item this segment refers to, if any.
    fn references(&self) -> Option<Reference<'_>> {
        let edge = |to, field, avoidable, indirect| Reference { to, field, avoidable, indirect };
        Some(match self {
            Segment::Value { name, kind: Kind::Msg { ty, boxed, .. }, .. } => {
                edge(Referent::Message(ty), name, false, *boxed)
            }
            Segment::Opt {
                field: Field { name, kind: Kind::Msg { ty, boxed, .. }, .. }, ..
            } => edge(Referent::Message(ty), name, true, *boxed),
            Segment::Repeat { name, element: Kind::Msg { ty, .. }, count, .. } => edge(
                Referent::Message(ty),
                name,
                !matches!(count, Count::Window(Len::Bytes(n)) if *n > 0),
                true,
            ),
            Segment::Placed { name, kind: Kind::Msg { ty, boxed, .. }, absent, .. } => {
                edge(Referent::Message(ty), name, absent.is_some(), *boxed)
            }
            Segment::Switch { name, choice, .. } => {
                edge(Referent::Choice(choice), name, false, false)
            }
            _ => return None,
        })
    }
}

/// Every item the segments refer to, in order.
pub(crate) fn references(segments: &[Segment]) -> Vec<Reference<'_>> {
    segments.iter().filter_map(Segment::references).collect()
}

/// The reference that decides whether this body is open-ended, if the last segment has one.
///
/// A repeat never has one. Its [`Count`] decides, whatever its elements are.
#[cfg(feature = "emit")]
pub(crate) fn defers_open_end(segments: &[Segment]) -> Option<Reference<'_>> {
    match segments.last()? {
        Segment::Switch { window: Some(_), .. } => None,
        seg @ (Segment::Switch { .. }
        | Segment::Value { kind: Kind::Msg { .. }, .. }
        | Segment::Opt { field: Field { kind: Kind::Msg { .. }, .. }, .. }) => seg.references(),
        _ => None,
    }
}

#[cfg(all(test, feature = "emit"))]
mod tests {
    use super::*;
    use crate::{By, Scalar};

    #[test]
    fn a_windowed_repeat_defers_to_nothing() {
        let segments = [
            Segment::Block(vec![Field::new("len", Kind::Scalar(Scalar::U(16)))]),
            Segment::repeat(
                "items",
                Kind::Scalar(Scalar::U(8)),
                Count::Window(Len::Field { by: By::field("len"), cap: None }),
                None,
                crate::Collection::Vec,
            ),
        ];
        assert!(defers_open_end(&segments).is_none());
    }
}
