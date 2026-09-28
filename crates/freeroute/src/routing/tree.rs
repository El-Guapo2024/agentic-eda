//! The plain (default) search tree: every item's exact shapes, clearance
//! uncompensated, kept up to date as items come and go. What the via and
//! push checks, the contact queries and the clean-up search.
//! FreeRouting's default `ShapeSearchTree`.
//!
//! The Java's tree answers every query as a sorted set, so its layout never
//! shows; only the entries do.

use crate::board::{tree_shapes, TreeKind};
use crate::geometry::{IntOctagon, TileShape};
use crate::model::Board;
use crate::searchtree::{LeafId, ShapeTree};

pub struct DefaultTree {
    /// Each entry: item index, shape index, layer.
    tree: ShapeTree<(usize, u32, i32)>,
    /// Per item index: its leaves, `None` where it has no shape.
    leaves: Vec<Vec<Option<LeafId>>>,
    /// Per item index: its tree shapes, counting those FreeRouting has none
    /// for.
    shapes: Vec<Vec<Option<TileShape>>>,
}

impl DefaultTree {
    pub fn new(board: &Board) -> Self {
        let mut t = DefaultTree { tree: ShapeTree::new(), leaves: Vec::new(), shapes: Vec::new() };
        for i in 0..board.items.len() {
            t.insert(board, i);
        }
        t
    }

    /// Enter item `item`'s shapes. `SearchTreeManager.insert`.
    pub fn insert(&mut self, board: &Board, item: usize) {
        if self.shapes.len() <= item {
            self.shapes.resize(item + 1, Vec::new());
            self.leaves.resize(item + 1, Vec::new());
        }
        let it = &board.items[item];
        let all = tree_shapes(board, it, TreeKind::Plain, 0);
        let count = all.len();
        let mut leaves = Vec::with_capacity(count);
        for (k, shape) in all.iter().enumerate() {
            let leaf = shape.as_ref().and_then(|s| s.bounding_octagon()).map(|bounds| self.tree.insert(bounds, (item, k as u32, it.shape_layer(k, board.layer_count(), count))));
            leaves.push(leaf);
        }
        self.leaves[item] = leaves;
        self.shapes[item] = all;
    }

    /// Take item `item`'s shapes out. `SearchTreeManager.remove`. Its
    /// shapes are kept, as a removed Java item keeps its own, and answer
    /// [`get_shape`](Self::get_shape) still.
    pub fn remove(&mut self, item: usize) {
        for leaf in std::mem::take(&mut self.leaves[item]).into_iter().flatten() {
            self.tree.remove(leaf);
        }
    }

    /// `Item.tree_shape_count` in this tree.
    pub fn shape_count(&self, item: usize) -> usize {
        self.shapes.get(item).map_or(0, |v| v.len())
    }

    pub fn shape(&self, item: usize, index: u32) -> &TileShape {
        self.get_shape(item, index).expect("an item shape in the plain tree")
    }

    pub fn get_shape(&self, item: usize, index: u32) -> Option<&TileShape> {
        self.shapes.get(item)?.get(index as usize)?.as_ref()
    }

    /// The entries whose bounding octagons meet `bounds`, in FreeRouting's
    /// order: by item number descending, then shape index; each with its
    /// layer. `MinAreaTree.overlaps`.
    pub fn candidates(&self, board: &Board, bounds: &IntOctagon) -> Vec<(usize, u32, i32)> {
        let mut found: Vec<(usize, u32, i32)> = self.tree.overlaps(bounds).into_iter().map(|l| *self.tree.payload(l)).collect();
        found.sort_by(|a, b| board.items[b.0].id.cmp(&board.items[a.0].id).then(a.1.cmp(&b.1)));
        found
    }

    /// The entries touching `shape` on `layer` (every layer if negative),
    /// skipping items no obstacle to one of `ignore_nets`, in FreeRouting's
    /// order: by item number descending, then shape index; each with its
    /// layer. `ShapeSearchTree.overlapping_tree_entries`.
    pub fn overlapping_entries(&self, board: &Board, shape: &TileShape, layer: i32, ignore_nets: &[i32]) -> Vec<(usize, u32, i32)> {
        let Some(bounds) = shape.bounding_octagon() else { return Vec::new() };
        let found = self.candidates(board, &bounds);
        let is_45_degree = matches!(shape, TileShape::Octagon(_));
        found
            .into_iter()
            .filter(|&(i, k, l)| {
                if layer >= 0 && l != layer {
                    return false;
                }
                let item = &board.items[i];
                if ignore_nets.iter().any(|&net| !item.is_obstacle(net)) {
                    return false;
                }
                let s = self.shape(i, k);
                (is_45_degree && matches!(s, TileShape::Octagon(_))) || s.intersects(shape)
            })
            .collect()
    }
}
