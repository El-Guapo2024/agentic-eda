//! The types both Clipper2 back ends share: the enums of `clipper.engine.h`
//! and `clipper.offset.h`, and `PolyTree64` as a flat arena (see
//! `port/engine.rs` for why it is flattened). The C++ binding (`cpp.rs`) fills
//! the same `PolyTree64` from Clipper2's `PolyPath64` in pre-order, so callers
//! read either back end's trees the same way.

use crate::core::*;

/// Note: all clipping operations except for Difference are commutative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipType {
    None,
    Intersection,
    Union,
    Difference,
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathType {
    Subject,
    Clip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinType {
    Square,
    Bevel,
    Round,
    Miter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndType {
    Polygon,
    Joined,
    Butt,
    Square,
    Round,
}

pub type ZCallback64 = Box<dyn FnMut(Point64, Point64, Point64, Point64, &mut Point64)>;

// PolyPath / PolyTree ---------------------------------------------------

/// A faithful (if flattened -- see the module doc comment on why an arena
/// is used throughout this port) rendition of `PolyPath64`/`PolyTree64`.
/// `level` is a precomputed `PolyPath::Level()` (0 would be the invisible
/// tree root; every real node here is level >= 1), so `is_hole` is exactly
/// `PolyPath::IsHole()`: `lvl != 0 && lvl % 2 == 0`.
#[derive(Debug, Clone)]
pub struct PolyPathNode {
    pub polygon: Path64,
    pub children: Vec<usize>,
    pub level: u32,
}

impl PolyPathNode {
    pub fn is_hole(&self) -> bool {
        self.level != 0 && self.level.is_multiple_of(2)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PolyTree64 {
    pub nodes: Vec<PolyPathNode>,
    pub roots: Vec<usize>,
}

impl PolyTree64 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.nodes.clear();
        self.roots.clear();
    }

    pub fn count(&self) -> usize {
        self.roots.len()
    }

    /// `PolyPath64::AddChild` called on the tree root.
    pub fn add_child(&mut self, path: Path64) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(PolyPathNode { polygon: path, children: Vec::new(), level: 1 });
        self.roots.push(idx);
        idx
    }

    /// `PolyPath64::AddChild` called on an existing node.
    pub fn add_child_to(&mut self, parent: usize, path: Path64) -> usize {
        let idx = self.nodes.len();
        let level = self.nodes[parent].level + 1;
        self.nodes.push(PolyPathNode { polygon: path, children: Vec::new(), level });
        self.nodes[parent].children.push(idx);
        idx
    }
}

/// `PolyTreeToPaths64`: flattens every node (outlines and holes alike) into
/// a flat `Paths64`, discarding the nesting.
pub fn poly_tree_to_paths64(tree: &PolyTree64) -> Paths64 {
    fn walk(tree: &PolyTree64, idx: usize, out: &mut Paths64) {
        out.push(tree.nodes[idx].polygon.clone());
        for &c in &tree.nodes[idx].children {
            walk(tree, c, out);
        }
    }
    let mut out = Vec::new();
    for &r in &tree.roots {
        walk(tree, r, &mut out);
    }
    out
}

