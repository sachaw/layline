//! Checks across the module's references: endless recursion, infinite-size types, open ends
//! that depend on a cycle, and fields after a switch that runs to the end of the body.

use super::Module;
use crate::walk::{Reference, Referent, defers_open_end, references};
use crate::{ChoiceDef, Error, Invalid, Item, MessageDef, Segment};

/// A message or choice, and the references each of its alternatives follows.
struct Node<'a> {
    name: &'a str,
    /// A message and a choice may share a name, so lookups match on name and kind.
    choice: bool,
    /// The references of each alternative.
    alternatives: Vec<Vec<Reference<'a>>>,
}

/// Refuses endless recursion, and a type that contains itself without a pointer.
pub(super) fn recursion(module: &Module) -> Result<(), Error> {
    let nodes: Vec<Node<'_>> = module
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Message(m) => Some(Node {
                name: &m.name,
                choice: false,
                alternatives: vec![references(&m.segments)],
            }),
            Item::Choice(c) => {
                let mut alternatives: Vec<Vec<_>> =
                    c.arms.iter().map(|a| references(&a.body)).collect();
                if c.other.is_some() || c.arms.is_empty() {
                    alternatives.push(Vec::new());
                }
                Some(Node { name: &c.name, choice: true, alternatives })
            }
            _ => None,
        })
        .collect();

    let find = |to: Referent<'_>| {
        let want = matches!(to, Referent::Choice(_));
        nodes.iter().position(|n| n.name == to.name() && n.choice == want)
    };

    let ends = least_fixpoint(nodes.len(), |i, ends| {
        nodes[i].alternatives.iter().any(|edges| {
            edges.iter().filter(|r| !r.avoidable).all(|r| find(r.to).is_none_or(|j| ends[j]))
        })
    });
    if let Some((i, node)) = nodes.iter().enumerate().find(|(i, _)| !ends[*i]) {
        let path = cycle_from(&nodes, &find, i, |n| {
            n.alternatives.iter().flatten().filter(|r| !r.avoidable).map(|r| r.to).collect()
        });
        return Err(Error::Refused(format!(
            "`{}`: {path} recurses on every path, so no wire can end. \
             Add a way out, such as an empty collection, a flag, or an arm that leads out",
            node.name,
        )));
    }

    let sized = least_fixpoint(nodes.len(), |i, sized| {
        nodes[i]
            .alternatives
            .iter()
            .flatten()
            .all(|r| r.indirect || find(r.to).is_none_or(|j| sized[j]))
    });
    if let Some((i, node)) = nodes.iter().enumerate().find(|(i, _)| !sized[*i]) {
        let path = cycle_from(&nodes, &find, i, |n| {
            n.alternatives.iter().flatten().filter(|r| !r.indirect).map(|r| r.to).collect()
        });
        return Err(Error::Refused(format!(
            "`{}`: {path} contains itself without indirection, so the Rust type has infinite size. \
             Use `Box<T>` or `Vec<T>` on one edge of the cycle",
            node.name,
        )));
    }

    Ok(())
}

/// Each item's name, and the members of its cycle with whether each is open-ended.
type OpenEndCycles = Vec<(String, Vec<(String, bool)>)>;

/// Whether a body is open-ended: known, or the same as another item.
enum Term<'a> {
    Known(bool),
    Defers(Referent<'a>),
}

/// Whether each item on a cycle of open-end dependencies is open-ended.
///
/// A generated constant cannot answer this, because the cycle would never resolve.
///
/// # Errors
///
/// [`Error::Refused`] when a cycle depends on an item this module does not declare.
pub(super) fn open_end_cycles(module: &Module) -> Result<OpenEndCycles, Error> {
    let nodes: Vec<(&str, bool, Vec<Term<'_>>)> = module
        .items
        .iter()
        .filter_map(|it| {
            fn term(segments: &[Segment]) -> Term<'_> {
                match defers_open_end(segments) {
                    Some(r) => Term::Defers(r.to),
                    None => Term::Known(segments.last().is_some_and(Segment::open_ended)),
                }
            }
            match it {
                Item::Message(m) => Some((m.name.as_str(), false, vec![term(&m.segments)])),
                Item::Choice(c) if c.other.is_some() && c.other_bytes.is_none() => {
                    Some((c.name.as_str(), true, vec![Term::Known(true)]))
                }
                Item::Choice(c) => {
                    Some((c.name.as_str(), true, c.arms.iter().map(|a| term(&a.body)).collect()))
                }
                _ => None,
            }
        })
        .collect();
    let find = |to: Referent<'_>| {
        let want = matches!(to, Referent::Choice(_));
        nodes.iter().position(|(n, c, _)| *n == to.name() && *c == want)
    };

    let n = nodes.len();
    let mut reaches = vec![false; n * n];
    for (i, (.., terms)) in nodes.iter().enumerate() {
        for t in terms {
            if let Term::Defers(to) = t
                && let Some(j) = find(*to)
            {
                reaches[i * n + j] = true;
            }
        }
    }
    for k in 0..n {
        for i in 0..n {
            if reaches[i * n + k] {
                for j in 0..n {
                    reaches[i * n + j] |= reaches[k * n + j];
                }
            }
        }
    }
    let on_a_cycle = |i: usize| reaches[i * n + i];
    if !(0..n).any(on_a_cycle) {
        return Ok(Vec::new());
    }

    for i in (0..n).filter(|&i| on_a_cycle(i)) {
        for j in (0..n).filter(|&j| j == i || reaches[i * n + j]) {
            for t in &nodes[j].2 {
                if let Term::Defers(to) = t
                    && find(*to).is_none()
                {
                    return Err(Error::Refused(format!(
                        "`{owner}`: its end depends on `{to}`, which this module does not declare. \
                         Declare `{to}` in this module, or reach it through a collection",
                        owner = nodes[i].0,
                        to = to.name(),
                    )));
                }
            }
        }
    }

    let value = least_fixpoint(n, |i, value| {
        nodes[i].2.iter().any(|t| match t {
            Term::Known(b) => *b,
            Term::Defers(to) => find(*to).is_some_and(|j| value[j]),
        })
    });

    Ok((0..n)
        .map(|i| {
            let members = (0..n)
                .filter(|&j| reaches[i * n + j] && reaches[j * n + i])
                .map(|j| (String::from(nodes[j].0), value[j]))
                .collect();
            (String::from(nodes[i].0), members)
        })
        .collect())
}

/// The smallest set of nodes closed under `holds`, found by adding nodes until nothing changes.
fn least_fixpoint(n: usize, holds: impl Fn(usize, &[bool]) -> bool) -> Vec<bool> {
    let mut set = vec![false; n];
    loop {
        let mut changed = false;
        for i in 0..n {
            if !set[i] && holds(i, &set) {
                set[i] = true;
                changed = true;
            }
        }
        if !changed {
            return set;
        }
    }
}

/// The cycle reached from `start` along `out`'s edges, as `` `a` -> `b` -> `a` ``.
fn cycle_from<'a>(
    nodes: &[Node<'a>],
    find: &impl Fn(Referent<'_>) -> Option<usize>,
    start: usize,
    out: impl Fn(&Node<'a>) -> Vec<Referent<'a>>,
) -> String {
    let mut path = vec![start];
    let mut at = start;
    for _ in 0..=nodes.len() {
        let Some(next) = out(&nodes[at]).into_iter().find_map(find) else {
            break;
        };
        if let Some(seen) = path.iter().position(|&p| p == next) {
            let names = path[seen..]
                .iter()
                .chain(core::iter::once(&next))
                .map(|&p| format!("`{}`", nodes[p].name))
                .collect::<Vec<_>>();
            return names.join(" -> ");
        }
        path.push(next);
        at = next;
    }
    format!("`{}`", nodes[start].name)
}

/// Refuses a segment after a switch whose choice runs to the end of the body.
pub(super) fn terminal_switches(module: &Module, m: &MessageDef) -> Result<(), Error> {
    for (i, seg) in m.segments.iter().enumerate() {
        let Segment::Switch { name, choice, window: None, .. } = seg else {
            continue;
        };
        if i + 1 == m.segments.len() {
            continue;
        }
        let found = module.items.iter().find_map(|it| match it {
            Item::Choice(c) if c.name == *choice => Some(c),
            _ => None,
        });
        if found.is_some_and(ChoiceDef::open_ended) {
            return Err(Error::Invalid(
                m.name.clone(),
                Invalid::AfterOpenEnd(format!(
                    "`{name}` must be last, because choice `{choice}` runs to the end of the body. \
                     Bound every arm as `[u8; N]`, or close the catalogue"
                )),
            ));
        }
    }
    Ok(())
}
