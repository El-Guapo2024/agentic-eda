//! Spatial index, ported from FreeRouting's `ShapeTree` + `MinAreaTree`.
//!
//! Answers "which stored shapes might touch this one" -- asked for every
//! room the maze search grows, so it is on the router's hot path.
//!
//! A binary tree of bounding octagons. A new shape descends toward the
//! child whose bounds would grow *least in area* if it were added, then
//! pairs up with the leaf it lands on under a fresh inner node. A query
//! walks every branch whose bounds intersect it. There is no rebalancing,
//! exactly as in the Java: insertion order shapes the tree.
//!
//! Java links nodes with object references and parent pointers. Here the
//! nodes live in one `Vec` and refer to each other by index -- the usual
//! Rust shape for a mutable tree with parent links, with no `Rc<RefCell>`
//! and no unsafe. Removed slots are recycled.
//!
//! In 45-degree mode FreeRouting bounds everything by octagons, so that is
//! the only bounding shape here. Inner-node bounds are unions and are not
//! normalized, also as in the Java: a loose bound only ever makes a query
//! look at more candidates, never miss one.

use crate::geometry::IntOctagon;

/// A handle to a stored shape, stable until that shape is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LeafId(usize);

#[derive(Debug, Clone, Copy)]
enum Kind {
    Inner { first: usize, second: usize },
    /// Its payload is in [`ShapeTree::payloads`] under the same index.
    Leaf,
    /// A recycled slot.
    Free,
}

/// A node, its payload kept apart: the walks of a room completion touch
/// only bounds and links, so nodes stay small.
#[derive(Debug, Clone)]
struct Node {
    bounds: IntOctagon,
    parent: Option<usize>,
    kind: Kind,
}

#[derive(Debug, Clone)]
pub struct ShapeTree<T> {
    nodes: Vec<Node>,
    /// By node index: a leaf's payload, `None` for any other node.
    payloads: Vec<Option<T>>,
    free: Vec<usize>,
    root: Option<usize>,
    leaf_count: usize,
}

impl<T> Default for ShapeTree<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ShapeTree<T> {
    pub fn new() -> Self {
        ShapeTree { nodes: Vec::new(), payloads: Vec::new(), free: Vec::new(), root: None, leaf_count: 0 }
    }

    pub fn len(&self) -> usize {
        self.leaf_count
    }

    pub fn is_empty(&self) -> bool {
        self.leaf_count == 0
    }

    fn alloc(&mut self, node: Node, payload: Option<T>) -> usize {
        if let Some(i) = self.free.pop() {
            self.nodes[i] = node;
            self.payloads[i] = payload;
            i
        } else {
            self.nodes.push(node);
            self.payloads.push(payload);
            self.nodes.len() - 1
        }
    }

    fn release(&mut self, i: usize) {
        self.nodes[i].kind = Kind::Free;
        self.nodes[i].parent = None;
        self.payloads[i] = None;
        self.free.push(i);
    }

    pub fn payload(&self, leaf: LeafId) -> &T {
        match (self.nodes[leaf.0].kind, &self.payloads[leaf.0]) {
            (Kind::Leaf, Some(payload)) => payload,
            _ => panic!("ShapeTree::payload: {leaf:?} is not a live leaf"),
        }
    }

    pub fn payload_mut(&mut self, leaf: LeafId) -> &mut T {
        match (self.nodes[leaf.0].kind, &mut self.payloads[leaf.0]) {
            (Kind::Leaf, Some(payload)) => payload,
            _ => panic!("ShapeTree::payload_mut: {leaf:?} is not a live leaf"),
        }
    }

    pub fn bounds(&self, leaf: LeafId) -> IntOctagon {
        self.nodes[leaf.0].bounds
    }

    /// Store `payload` under bounding octagon `bounds`.
    /// MinAreaTree.insertUnlocked.
    pub fn insert(&mut self, bounds: IntOctagon, payload: T) -> LeafId {
        self.leaf_count += 1;
        let leaf = self.alloc(Node { bounds, parent: None, kind: Kind::Leaf }, Some(payload));
        let Some(root) = self.root else {
            self.root = Some(leaf);
            return LeafId(leaf);
        };
        let replace = self.position_locate(root, &bounds);
        let new_bounds = bounds.union(&self.nodes[replace].bounds);
        let parent = self.nodes[replace].parent;
        let inner = self.alloc(Node { bounds: new_bounds, parent, kind: Kind::Inner { first: replace, second: leaf } }, None);
        if let Some(p) = parent {
            if let Kind::Inner { first, second } = &mut self.nodes[p].kind {
                if *first == replace {
                    *first = inner;
                } else {
                    *second = inner;
                }
            }
        }
        self.nodes[replace].parent = Some(inner);
        self.nodes[leaf].parent = Some(inner);
        if self.root == Some(replace) {
            self.root = Some(inner);
        }
        LeafId(leaf)
    }

    /// Walk down to the leaf the new shape should pair with, widening each
    /// inner node's bounds on the way. MinAreaTree.positionLocate.
    fn position_locate(&mut self, start: usize, bounds: &IntOctagon) -> usize {
        let mut node = start;
        loop {
            let (first, second) = match self.nodes[node].kind {
                Kind::Inner { first, second } => (first, second),
                _ => return node,
            };
            self.nodes[node].bounds = bounds.union(&self.nodes[node].bounds);
            let f = self.nodes[first].bounds;
            let s = self.nodes[second].bounds;
            let grow_first = bounds.union(&f).area() - f.area();
            let grow_second = bounds.union(&s).area() - s.area();
            node = if grow_first <= grow_second { first } else { second };
        }
    }

    /// Remove a stored shape. MinAreaTree.removeLeafUnlocked.
    pub fn remove(&mut self, leaf: LeafId) {
        let l = leaf.0;
        if !matches!(self.nodes[l].kind, Kind::Leaf) {
            panic!("ShapeTree::remove: {leaf:?} is not a live leaf");
        }
        let parent = self.nodes[l].parent;
        self.leaf_count -= 1;
        let Some(p) = parent else {
            self.root = None;
            self.release(l);
            return;
        };
        let other = match self.nodes[p].kind {
            Kind::Inner { first, second } if second == l => first,
            Kind::Inner { first, second } if first == l => second,
            _ => panic!("ShapeTree::remove: parent of {leaf:?} does not hold it"),
        };
        let grand = self.nodes[p].parent;
        self.nodes[other].parent = grand;
        match grand {
            None => self.root = Some(other),
            Some(g) => {
                if let Kind::Inner { first, second } = &mut self.nodes[g].kind {
                    if *second == p {
                        *second = other;
                    } else if *first == p {
                        *first = other;
                    } else {
                        panic!("ShapeTree::remove: grandparent inconsistent");
                    }
                }
            }
        }
        self.release(p);
        self.release(l);
        // Shrink ancestor bounds until one no longer changes.
        let mut cur = grand;
        while let Some(c) = cur {
            let Kind::Inner { first, second } = self.nodes[c].kind else { break };
            let nb = self.nodes[second].bounds.union(&self.nodes[first].bounds);
            if self.nodes[c].bounds.is_contained_in(&nb) {
                break;
            }
            self.nodes[c].bounds = nb;
            cur = self.nodes[c].parent;
        }
    }

    /// Every stored shape whose bounds intersect `shape`, in no particular
    /// order. MinAreaTree.overlapsUnlocked. Java then sorts by the stored
    /// object's own ordering; callers here sort their payloads by it.
    pub fn overlaps(&self, shape: &IntOctagon) -> Vec<LeafId> {
        let mut found = Vec::new();
        let Some(root) = self.root else { return found };
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            if !self.nodes[n].bounds.intersects(shape) {
                continue;
            }
            match self.nodes[n].kind {
                Kind::Leaf => found.push(LeafId(n)),
                Kind::Inner { first, second } => {
                    stack.push(first);
                    stack.push(second);
                }
                Kind::Free => unreachable!("a free slot is linked into the tree"),
            }
        }
        found
    }

    // Raw traversal, for algorithms that walk the tree with a query shape
    // that changes as they go (room completion shrinks its bound after
    // every obstacle), which `overlaps` cannot express.

    pub(crate) fn root_node(&self) -> Option<usize> {
        self.root
    }

    pub(crate) fn node_bounds(&self, n: usize) -> IntOctagon {
        self.nodes[n].bounds
    }

    /// `Ok(leaf)` for a leaf, `Err((first, second))` for an inner node.
    pub(crate) fn node_step(&self, n: usize) -> Result<LeafId, (usize, usize)> {
        match self.nodes[n].kind {
            Kind::Leaf => Ok(LeafId(n)),
            Kind::Inner { first, second } => Err((first, second)),
            Kind::Free => panic!("ShapeTree: free slot {n} reached by traversal"),
        }
    }

    /// The tree's layout as the parity harness hashes it: its nodes in
    /// pre-order, first child before second -- an inner node as `I` and
    /// its bounds, a leaf as `L`, what `name` makes of its payload, and its
    /// bounds -- each token followed by a space, through 64-bit FNV-1a.
    /// The leaf count and the hash, and the tokens a node a line if
    /// `full`.
    pub fn fingerprint(&self, name: impl Fn(&T) -> String, full: bool) -> (usize, u64, Option<String>) {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut leaves = 0;
        let mut text = full.then(|| String::from("tree\n"));
        let mut stack: Vec<usize> = self.root.into_iter().collect();
        while let Some(n) = stack.pop() {
            let mut tokens = match self.nodes[n].kind {
                Kind::Leaf => {
                    leaves += 1;
                    format!("L {} ", name(self.payloads[n].as_ref().expect("a leaf's payload")))
                }
                Kind::Inner { first, second } => {
                    stack.push(second);
                    stack.push(first);
                    String::from("I ")
                }
                Kind::Free => unreachable!("a free slot is linked into the tree"),
            };
            let b = self.nodes[n].bounds;
            for v in [b.left_x, b.bottom_y, b.right_x, b.top_y, b.upper_left_diag_x, b.lower_right_diag_x, b.lower_left_diag_x, b.upper_right_diag_x] {
                tokens.push_str(&v.to_string());
                tokens.push(' ');
            }
            for c in tokens.bytes() {
                hash ^= c as u64;
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
            if let Some(t) = &mut text {
                t.push_str(&tokens);
                t.push('\n');
            }
        }
        (leaves, hash, text)
    }

    /// Checks the structural invariants: every inner node's bounds contain
    /// both children's, parent links are symmetric, and the leaf count
    /// matches. For tests.
    pub fn check_invariants(&self) -> Result<(), String> {
        let Some(root) = self.root else {
            return if self.leaf_count == 0 { Ok(()) } else { Err(format!("empty tree but leaf_count {}", self.leaf_count)) };
        };
        if self.nodes[root].parent.is_some() {
            return Err("root has a parent".into());
        }
        let mut leaves = 0;
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            match self.nodes[n].kind {
                Kind::Leaf => leaves += 1,
                Kind::Inner { first, second } => {
                    for c in [first, second] {
                        if self.nodes[c].parent != Some(n) {
                            return Err(format!("child {c} does not point back to parent {n}"));
                        }
                        let (cb, pb) = (self.nodes[c].bounds, self.nodes[n].bounds);
                        if !cb.is_contained_in(&pb) {
                            return Err(format!("child {c} bounds {cb:?} escape parent {n} bounds {pb:?}"));
                        }
                        stack.push(c);
                    }
                }
                Kind::Free => return Err(format!("free slot {n} is linked into the tree")),
            }
        }
        if leaves != self.leaf_count {
            return Err(format!("found {leaves} leaves, leaf_count says {}", self.leaf_count));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::IntBox;
    use std::collections::BTreeMap;

    fn rng(mut seed: u64) -> impl FnMut(i64, i64) -> i64 {
        move |lo, hi| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            lo + (seed % ((hi - lo + 1) as u64)) as i64
        }
    }

    fn shape(next: &mut impl FnMut(i64, i64) -> i64) -> IntOctagon {
        let x = next(-500, 500);
        let y = next(-500, 500);
        let w = next(1, 60);
        let h = next(1, 60);
        let o = IntBox::new(x, y, x + w, y + h).to_octagon();
        // Shave a corner off some of them so they are true octagons.
        let cut = next(0, w.min(h) / 2);
        IntOctagon::new(o.left_x, o.bottom_y, o.right_x, o.top_y, o.upper_left_diag_x, o.lower_right_diag_x, o.lower_left_diag_x + cut, o.upper_right_diag_x)
            .normalize()
    }

    /// The oracle: a flat list, queried by checking every entry.
    #[test]
    fn overlaps_finds_exactly_what_a_linear_scan_finds() {
        let mut next = rng(7);
        let mut tree = ShapeTree::new();
        let mut flat: BTreeMap<LeafId, IntOctagon> = BTreeMap::new();
        for i in 0..400 {
            let s = shape(&mut next);
            let id = tree.insert(s, i);
            flat.insert(id, s);
        }
        tree.check_invariants().unwrap();
        for _ in 0..300 {
            let q = shape(&mut next).offset(next(0, 80) as f64);
            let mut want: Vec<LeafId> = flat.iter().filter(|(_, s)| s.intersects(&q)).map(|(id, _)| *id).collect();
            want.sort();
            let mut got = tree.overlaps(&q);
            got.sort();
            assert_eq!(got, want);
        }
    }

    #[test]
    fn interleaved_removal_keeps_the_tree_correct() {
        let mut next = rng(99);
        let mut tree = ShapeTree::new();
        let mut flat: BTreeMap<LeafId, IntOctagon> = BTreeMap::new();
        for round in 0..2_000 {
            if flat.is_empty() || next(0, 2) > 0 {
                let s = shape(&mut next);
                flat.insert(tree.insert(s, round), s);
            } else {
                let k = next(0, flat.len() as i64 - 1) as usize;
                let id = *flat.keys().nth(k).unwrap();
                flat.remove(&id);
                tree.remove(id);
            }
            if round % 97 == 0 {
                tree.check_invariants().unwrap_or_else(|e| panic!("round {round}: {e}"));
                let q = shape(&mut next).offset(40.0);
                let want: Vec<LeafId> = flat.iter().filter(|(_, s)| s.intersects(&q)).map(|(id, _)| *id).collect();
                let mut got = tree.overlaps(&q);
                got.sort();
                assert_eq!(got, want, "round {round}");
            }
        }
        assert_eq!(tree.len(), flat.len());
        tree.check_invariants().unwrap();
    }

    #[test]
    fn removing_everything_empties_the_tree() {
        let mut next = rng(3);
        let mut tree = ShapeTree::new();
        let ids: Vec<_> = (0..50).map(|i| tree.insert(shape(&mut next), i)).collect();
        for id in ids {
            tree.remove(id);
        }
        assert!(tree.is_empty());
        tree.check_invariants().unwrap();
        assert!(tree.overlaps(&IntBox::new(-1000, -1000, 1000, 1000).to_octagon()).is_empty());
    }

    #[test]
    fn payloads_survive_restructuring() {
        let mut next = rng(11);
        let mut tree = ShapeTree::new();
        let ids: Vec<_> = (0..100).map(|i| (tree.insert(shape(&mut next), i * 10), i * 10)).collect();
        for (id, p) in ids.iter().step_by(3) {
            assert_eq!(tree.payload(*id), p);
            tree.remove(*id);
        }
        for (id, p) in ids.iter().enumerate().filter(|(i, _)| i % 3 != 0).map(|(_, x)| x) {
            assert_eq!(tree.payload(*id), p);
        }
    }
}
