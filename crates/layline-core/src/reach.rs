//! What a message's constants depend on, as a graph a `const fn` searches.
//!
//! A message that holds itself, directly or through other types, would make its
//! `OPEN_ENDED` and `ENCODE_FALLIBLE` constants depend on themselves, which no constant can.
//! So each derived type publishes a [`Node`]: what it decides alone, and links to the types it
//! defers to. A node is a `static`, whose address exists before its value, so nodes may link in
//! cycles.
//!
//! A node is [`Unfrozen`], and the language lets a constant hold a reference to such a static
//! but never read through it. So the compiler checks a link without the node behind it, which
//! in a cycle is still being computed. Only the initializer of a `static` may read a node, so
//! each answer is a search in a `static` of its own, which the type's constant copies. The
//! search visits each node once: the smallest answer consistent with every type's own, so a
//! cycle that nothing on it breaks is closed, and a cycle that only holds itself refuses nothing
//! it does not.

use core::sync::atomic::AtomicBool;

/// One type's part of the graph: what it decides alone, and the types it defers to.
pub struct Node {
    /// Distinguishes this node from every other in a search.
    pub id: &'static str,
    /// Whether the type's body runs to its end by itself.
    pub open_ended: bool,
    /// Whether the type's encode refuses a value by itself.
    pub refuses: bool,
    /// The types whose open end is this one's: what its last field holds.
    pub ends: &'static [Link],
    /// The types whose refusals are this one's: every message and catalogue it holds.
    pub holds: &'static [Link],
    /// What keeps a constant from reading this node.
    pub unfrozen: Unfrozen,
}

/// What makes a [`Node`] a `static` that a constant may link to and only a `static` may read.
///
/// Nothing writes it. It is here for its type, which the compiler does not freeze.
pub struct Unfrozen {
    _unwritten: AtomicBool,
}

impl Unfrozen {
    /// The only value.
    #[must_use]
    pub const fn new() -> Self {
        Self { _unwritten: AtomicBool::new(false) }
    }
}

impl Default for Unfrozen {
    fn default() -> Self {
        Self::new()
    }
}

/// What a type's constants are searched from, and what its holders link to.
#[derive(Clone, Copy)]
pub enum Link {
    /// A type that defers to nothing: a hand-written one, or a derived one that holds no
    /// message. A search reads it where it is linked.
    Leaf {
        /// Whether the type's body runs to its end.
        open_ended: bool,
        /// Whether the type's encode refuses a value.
        refuses: bool,
    },
    /// A type that defers to others, as its node in a `static`.
    Node(&'static Node),
}

impl Link {
    const fn decides(self, holds: bool) -> bool {
        let (open_ended, refuses) = match self {
            Self::Leaf { open_ended, refuses } => (open_ended, refuses),
            Self::Node(node) => (node.open_ended, node.refuses),
        };
        if holds { refuses } else { open_ended }
    }
}

/// The most derived types one search visits. A search that finds more fails the build.
const MOST: usize = 512;

/// Whether `link`, or a type its last field leads to, runs to the end of the body.
///
/// For the initializer of a `static`: a constant may not read a node.
#[must_use]
pub const fn open_ended(link: Link) -> bool {
    search(link, false)
}

/// Whether `link`, or a type it holds at any depth, refuses a value.
///
/// For the initializer of a `static`: a constant may not read a node.
#[must_use]
pub const fn refuses(link: Link) -> bool {
    search(link, true)
}

const fn edges(node: &Node, holds: bool) -> &'static [Link] {
    if holds { node.holds } else { node.ends }
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// Breadth-first, visiting each node once, and reading each leaf where it is linked.
///
/// The nodes seen are the work list: each is pushed once, and read once its turn comes.
const fn search(start: Link, holds: bool) -> bool {
    if start.decides(holds) {
        return true;
    }
    let Link::Node(start) = start else { return false };
    let mut seen: [&Node; MOST] = [start; MOST];
    let (mut done, mut found) = (0, 1);
    while done < found {
        let out = edges(seen[done], holds);
        done += 1;
        let mut i = 0;
        while i < out.len() {
            if out[i].decides(holds) {
                return true;
            }
            if let Link::Node(next) = out[i] {
                let mut known = false;
                let mut j = 0;
                while !known && j < found {
                    known = same(seen[j].id, next.id);
                    j += 1;
                }
                if !known {
                    assert!(found < MOST, "layline: more than 512 message types reach one another");
                    seen[found] = next;
                    found += 1;
                }
            }
            i += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn node(id: &'static str, ends: &'static [Link], holds: &'static [Link]) -> Node {
        Node { id, open_ended: false, refuses: false, ends, holds, unfrozen: Unfrozen::new() }
    }

    static A: Node = node("a", &[Link::Node(&B)], &[Link::Node(&B)]);
    static B: Node = node("b", &[Link::Node(&A), C], &[Link::Node(&A)]);
    const C: Link = Link::Leaf { open_ended: true, refuses: true };
    static LONE: Node = node("lone", &[], &[]);
    static SELF: Node = node("self", &[Link::Node(&SELF)], &[Link::Node(&SELF)]);

    #[test]
    fn a_cycle_takes_what_leads_out_of_it() {
        static OPEN_ENDED: [bool; 2] = [open_ended(Link::Node(&A)), open_ended(Link::Node(&B))];
        static REFUSES: [bool; 2] = [refuses(Link::Node(&A)), refuses(Link::Node(&B))];
        const { assert!(OPEN_ENDED[0] && OPEN_ENDED[1]) };
        const { assert!(!REFUSES[0] && !REFUSES[1]) };
        assert!(open_ended(Link::Node(&B)) && !refuses(Link::Node(&B)));
    }

    #[test]
    fn a_cycle_with_no_way_out_decides_nothing() {
        static ALONE: [bool; 3] = [
            open_ended(Link::Node(&SELF)),
            refuses(Link::Node(&SELF)),
            open_ended(Link::Node(&LONE)),
        ];
        const { assert!(!ALONE[0] && !ALONE[1] && !ALONE[2]) };
    }

    #[test]
    fn a_leaf_is_what_it_says() {
        const { assert!(open_ended(C) && refuses(C)) };
        const { assert!(!refuses(Link::Leaf { open_ended: true, refuses: false })) };
    }
}
