//! Rules for checksum fields and the bytes they cover.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::{Invalid, type_path};
use crate::{CoverFrom, CoverTo, Coverage, Scalar, Segment};

/// Field names in wire order.
pub(super) type Places<'a> = Vec<&'a str>;

/// Checks a checksum field's algorithm and integer type.
pub(super) fn check_value(name: &str, repr: &Scalar, algorithm: &str) -> Result<(), Invalid> {
    if algorithm.trim().is_empty() {
        return Err(Invalid::field(
            name,
            String::from(
                "a checksum requires the algorithm. \
                 Name a type that implements `layline::Checksum`",
            ),
        ));
    }
    type_path(name, "the checksum algorithm", algorithm)?;
    if !matches!(repr, Scalar::U(8 | 16 | 32 | 64) | Scalar::I(8 | 16 | 32 | 64)) {
        return Err(Invalid::field(
            name,
            format!(
                "a checksum must be an 8, 16, 32 or 64-bit integer, not {repr:?}. \
                 Use the algorithm's output width"
            ),
        ));
    }
    Ok(())
}

/// The name a range ends at, and whether the range includes that field.
fn end_of<'a>(to: &'a CoverTo, own: &'a str) -> (&'a str, bool) {
    match to {
        CoverTo::Here => (own, false),
        CoverTo::Before(f) => (f, false),
        CoverTo::After(f) => (f, true),
    }
}

/// The index of `field` in wire order. Refuses an optional or unknown field.
fn place_of(places: &Places<'_>, optional: &Places<'_>, field: &str) -> Result<usize, Invalid> {
    if let Some(i) = places.iter().position(|p| *p == field) {
        return Ok(i);
    }
    if optional.contains(&field) {
        return Err(Invalid::Other(format!(
            "a covered range cannot start or end at `{field}`, which is only sometimes present. \
             Use its flags field, or a field that is always present"
        )));
    }
    Err(Invalid::UnknownReference(String::from(field)))
}

/// Checks that a range resolves, is not empty, and does not include the checksum itself.
pub(super) fn coverage(
    places: &Places<'_>,
    optional: &Places<'_>,
    own: &str,
    over: &Coverage,
) -> Result<(), Invalid> {
    let from = match &over.from {
        CoverFrom::Start => None,
        CoverFrom::Field(f) => {
            if f == own {
                return Err(Invalid::field(
                    own,
                    format!(
                        "the covered range starts at the checksum `{own}` itself. \
                         Start it at a field before or after the checksum"
                    ),
                ));
            }
            Some(place_of(places, optional, f)?)
        }
    };
    let (end, through) = end_of(&over.to, own);
    if end == own && through {
        return Err(Invalid::field(
            own,
            format!(
                "the covered range runs through the checksum `{own}` itself. \
                 End it at `{own}` (`over = a..`), or after a neighbouring field"
            ),
        ));
    }
    let to = place_of(places, optional, end)?;
    let ck = place_of(places, optional, own)?;
    if matches!(over.to, CoverTo::Before(_)) && to == ck {
        return Err(Invalid::field(
            own,
            format!(
                "the covered range ends at `{own}`, where `over = a..` already ends it. \
                 Remove the end"
            ),
        ));
    }
    let last = if through {
        to
    } else {
        match to.checked_sub(1) {
            Some(last) => last,
            None => {
                return Err(Invalid::field(
                    own,
                    format!(
                        "the covered range ends at `{end}`, which is the first field, so it is \
                         empty. End it at a later field"
                    ),
                ));
            }
        }
    };
    if from.is_some_and(|from| from > last) {
        let start = match &over.from {
            CoverFrom::Field(f) => f.as_str(),
            CoverFrom::Start => "",
        };
        let ends = if through { "runs through" } else { "ends at" };
        return Err(Invalid::field(
            own,
            format!(
                "the covered range starts at `{start}` and {ends} `{end}`, which comes earlier. \
                 Write `over = {start}..`, or `over = {start}..=<field>`"
            ),
        ));
    }
    Ok(())
}

/// Refuses checksums whose ranges cover each other, since neither can be computed first.
pub(super) fn stampable(segments: &[Segment]) -> Result<(), Invalid> {
    let names: Vec<&str> = segments.iter().flat_map(Segment::places).collect();
    let order = |row: &str| names.iter().position(|p| *p == row);
    let checks: Vec<(&str, &Coverage)> = segments
        .iter()
        .filter_map(|seg| match seg {
            Segment::Checksum { name, over, .. } => Some((name.as_str(), over)),
            _ => None,
        })
        .collect();
    let covers = |over: &Coverage, own: &str, other: &str| {
        let (Some(o), Some(t)) = (order(own), order(other)) else { return false };
        let at = |n: &str| order(n).map(|i| (i as u64, i as u64 + 1));
        over.resolve((o as u64, o as u64 + 1), at)
            .is_some_and(|c| c.runs().iter().any(|&(a, b)| (t as u64) >= a && (t as u64) < b))
    };
    let waits_on: Vec<Vec<usize>> = checks
        .iter()
        .map(|(own, over)| {
            (0..checks.len())
                .filter(|&j| checks[j].0 != *own && covers(over, own, checks[j].0))
                .collect()
        })
        .collect();

    let mut stamped = vec![false; checks.len()];
    for _ in 0..checks.len() {
        let Some(next) =
            (0..checks.len()).find(|&i| !stamped[i] && waits_on[i].iter().all(|&j| stamped[j]))
        else {
            let stuck: Vec<&str> =
                (0..checks.len()).filter(|&i| !stamped[i]).map(|i| checks[i].0).collect();
            return Err(Invalid::Other(format!(
                "checksums {} cover each other. Narrow one range so it does not reach the other",
                stuck.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(" and "),
            )));
        };
        stamped[next] = true;
    }
    Ok(())
}
