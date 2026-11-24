//! What a message's constants depend on, as a graph a `const fn` searches.
//!
//! A message that holds itself, directly or through other types, would make its
//! `OPEN_ENDED` and `ENCODE_FALLIBLE` constants depend on themselves, which no constant can.
//! So each derived type publishes a [`Node`]: what it decides alone, and links to the types it
//! defers to. A node is a `static`, whose address exists before its value, so nodes may link in
//! cycles. A constant is then a search from its own node, which visits each node once: the
//! smallest answer consistent with every type's own, so a cycle that nothing on it breaks is
//! closed, and a cycle that only holds itself refuses nothing it does not.

/// One type's part of the graph: what it decides alone, and the types it defers to.
pub struct Node {
    /// Distinguishes this node from every other in a search. Empty for a type that links to
    /// nothing, which a search reads without visiting.
    pub id: &'static str,
    /// Whether the type's body runs to its end by itself.
    pub open_ended: bool,
    /// Whether the type's encode refuses a value by itself.
    pub refuses: bool,
    /// The types whose open end is this one's: what its last field holds.
    pub ends: &'static [Link],
    /// The types whose refusals are this one's: every message and catalogue it holds.
    pub holds: &'static [Link],
}

impl Node {
    /// A type that defers to nothing: a hand-written one, or a derived one that holds no message.
    #[must_use]
    pub const fn leaf(open_ended: bool, refuses: bool) -> Self {
        Self { id: "", open_ended, refuses, ends: &[], holds: &[] }
    }
}

/// A link to a [`Node`] in a `static`, which a constant may hold before the static has a value.
///
/// A reference would make the compiler check the static's value when it checks the constant,
/// and in a cycle that value is still being computed.
#[derive(Clone, Copy)]
pub struct Link(*const Node);

// SAFETY: a link is made only from a `&'static Node`, and a `Node` is never written after its
// `static` is initialized.
unsafe impl Sync for Link {}

impl Link {
    /// A link to `node`.
    #[must_use]
    pub const fn new(node: &'static Node) -> Self {
        Self(node)
    }

    const fn get(self) -> &'static Node {
        // SAFETY: `new` is the only constructor, so the pointer came from a `&'static Node`.
        unsafe { &*self.0 }
    }
}

/// The most derived types one search visits. A search that finds more fails the build.
const MOST: usize = 512;

/// Whether `node`, or a type its last field leads to, runs to the end of the body.
#[must_use]
pub const fn open_ended(node: Link) -> bool {
    search(node, false)
}

/// Whether `node`, or a type it holds at any depth, refuses a value.
#[must_use]
pub const fn refuses(node: Link) -> bool {
    search(node, true)
}

const fn decides(node: &Node, holds: bool) -> bool {
    if holds { node.refuses } else { node.open_ended }
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

/// Breadth-first, visiting each node with an id once, and reading each leaf where it is linked.
///
/// The nodes seen are the work list: each is pushed once, and read once its turn comes.
const fn search(start: Link, holds: bool) -> bool {
    if decides(start.get(), holds) {
        return true;
    }
    let mut seen: [Link; MOST] = [start; MOST];
    let (mut done, mut found) = (0, 1);
    while done < found {
        let out = edges(seen[done].get(), holds);
        done += 1;
        let mut i = 0;
        while i < out.len() {
            let next = out[i].get();
            if decides(next, holds) {
                return true;
            }
            let mut known = next.id.is_empty();
            let mut j = 0;
            while !known && j < found {
                known = same(seen[j].get().id, next.id);
                j += 1;
            }
            if !known {
                assert!(found < MOST, "layline: more than 512 message types reach one another");
                seen[found] = out[i];
                found += 1;
            }
            i += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    static A: Node = Node { id: "a", open_ended: false, refuses: false, ends: &[B_], holds: &[B_] };
    static B: Node =
        Node { id: "b", open_ended: false, refuses: false, ends: &[A_, C_], holds: &[A_] };
    static C: Node = Node::leaf(true, true);
    const A_: Link = Link::new(&A);
    const B_: Link = Link::new(&B);
    const C_: Link = Link::new(&C);

    static LONE: Node =
        Node { id: "lone", open_ended: false, refuses: false, ends: &[], holds: &[] };
    static SELF: Node =
        Node { id: "self", open_ended: false, refuses: false, ends: &[SELF_], holds: &[SELF_] };
    const SELF_: Link = Link::new(&SELF);

    #[test]
    fn a_cycle_takes_what_leads_out_of_it() {
        const { assert!(open_ended(A_)) };
        const { assert!(!refuses(A_)) };
        assert!(open_ended(B_) && !refuses(B_));
    }

    #[test]
    fn a_cycle_with_no_way_out_decides_nothing() {
        const { assert!(!open_ended(SELF_) && !refuses(SELF_)) };
        assert!(!open_ended(Link::new(&LONE)));
    }
}
