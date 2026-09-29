//! The plain (default) search tree: every item's exact shapes, clearance
//! uncompensated, kept up to date as items come and go. What the via and
//! push checks, the contact queries and the clean-up search.
//! FreeRouting's default `ShapeSearchTree`.
//!
//! The Java's tree answers every query as a sorted set, so its layout never
//! shows; only the entries do.

use crate::autoroute::engine::TreeItem;
use crate::board::{tree_shapes, TreeKind};
use crate::cellgrid::{CellGrid, SlotId};
use crate::door::Entry;
use crate::geometry::{IntOctagon, TileShape};
use crate::model::{Board, ItemKind};
use crate::searchtree::{LeafId, ShapeTree};

pub struct DefaultTree {
    /// Each entry: item index, shape index, layer. A grid, not the Java's
    /// tree, as nothing sees its layout.
    tree: CellGrid<(usize, u32, i32)>,
    /// Per item index: its entries, `None` where it has no shape.
    leaves: Vec<Vec<Option<SlotId>>>,
    /// Per item index: its tree shapes, counting those FreeRouting has none
    /// for.
    shapes: Vec<Vec<Option<TileShape>>>,
}

impl DefaultTree {
    pub fn new(board: &Board) -> Self {
        let mut t = DefaultTree { tree: CellGrid::new(&board.bounds), leaves: Vec::new(), shapes: Vec::new() };
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
        self.sorted(board, bounds, |_| true)
    }

    /// The entries `keep` accepts of those whose bounding octagons meet
    /// `bounds`, in FreeRouting's order.
    fn sorted(&self, board: &Board, bounds: &IntOctagon, keep: impl FnMut(&(usize, u32, i32)) -> bool) -> Vec<(usize, u32, i32)> {
        let mut found = Vec::new();
        self.tree.overlaps(bounds, keep, &mut found);
        found.sort_by(|a, b| board.items[b.0].id.cmp(&board.items[a.0].id).then(a.1.cmp(&b.1)));
        found
    }

    /// The entries touching `shape` on `layer` (every layer if negative),
    /// skipping items no obstacle to one of `ignore_nets`, in FreeRouting's
    /// order: by item number descending, then shape index; each with its
    /// layer. `ShapeSearchTree.overlapping_tree_entries`.
    pub fn overlapping_entries(&self, board: &Board, shape: &TileShape, layer: i32, ignore_nets: &[i32]) -> Vec<(usize, u32, i32)> {
        let Some(bounds) = shape.bounding_octagon() else { return Vec::new() };
        let is_45_degree = matches!(shape, TileShape::Octagon(_));
        // Filtered before sorting, which gives what sorting first would.
        self.sorted(board, &bounds, |&(i, k, l)| {
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
    }
}

/// A tree the maze search grows its rooms in: every item's shapes, grown
/// by the clearance to one class, and, during a search, the rooms. Made
/// the first time a search for the class asks for it and then kept, as
/// FreeRouting keeps it, updated as items come, go and change -- with the
/// Java's own leaf operations, in its order, as room completion walks the
/// tree's nodes and so sees its layout, which only the order of the
/// updates decides. A compensated tree of `SearchTreeManager`.
#[derive(Debug, Clone)]
pub struct AutorouteTree {
    pub class: i32,
    pub tree: ShapeTree<Entry<TreeItem>>,
    /// Per item index: its leaves, `None` where it has no shape.
    pub leaves: Vec<Vec<Option<LeafId>>>,
    /// Per item index: its tree shapes, counting those FreeRouting has none
    /// for; kept when the item goes, as for the default tree.
    pub shapes: Vec<Vec<Option<TileShape>>>,
}

impl AutorouteTree {
    /// The tree of `items`, inserted in that order: the board's.
    /// `SearchTreeManager.get_autoroute_tree`. `Err` with the items before
    /// the one whose shapes FreeRouting throws working out: its tree, made
    /// and kept before it is filled, stays so.
    #[allow(clippy::result_large_err)]
    pub fn build(board: &Board, items: &[usize], class: i32) -> Result<Self, Self> {
        let mut t = AutorouteTree { class, tree: ShapeTree::new(), leaves: Vec::new(), shapes: Vec::new() };
        for &i in items {
            if shapes_throw(board, i) {
                return Err(t);
            }
            t.insert(board, i);
        }
        Ok(t)
    }

    fn grow(&mut self, item: usize) {
        if self.shapes.len() <= item {
            self.shapes.resize(item + 1, Vec::new());
            self.leaves.resize(item + 1, Vec::new());
        }
    }

    fn calculate(&self, board: &Board, item: usize) -> Vec<Option<TileShape>> {
        tree_shapes(board, &board.items[item], TreeKind::FortyFive, self.class)
    }

    /// Store shape `k` of `count` of the item, if it has one.
    /// `ShapeTree.insert(Storable, int)`.
    fn insert_leaf(&mut self, board: &Board, item: usize, k: usize, count: usize, shape: Option<&TileShape>) -> Option<LeafId> {
        let shape = shape?;
        let bounds = shape.bounding_octagon()?;
        Some(self.tree.insert(bounds, Entry::Item(TreeItem::of(board, item, k, count, shape))))
    }

    fn remove_leaf(&mut self, leaf: Option<LeafId>) {
        if let Some(leaf) = leaf {
            self.tree.remove(leaf);
        }
    }

    /// Renew the entries of the item's leaves from the board and its
    /// shapes: leaves moved over from another trace, or to another index,
    /// say so, as the Java's entries hold the item itself.
    fn refresh(&mut self, board: &Board, item: usize) {
        let count = self.shapes[item].len();
        for k in 0..self.leaves[item].len() {
            if let Some(leaf) = self.leaves[item][k] {
                let shape = self.shapes[item][k].as_ref().expect("a leaf has a shape");
                *self.tree.payload_mut(leaf) = Entry::Item(TreeItem::of(board, item, k, count, shape));
            }
        }
    }

    /// See [`ShapeTree::fingerprint`]: a leaf is named by its item's number
    /// and its shape index, a room as `R 0`.
    pub fn fingerprint(&self, full: bool) -> (usize, u64, Option<String>) {
        self.tree.fingerprint(entry_name, full)
    }

    /// Enter the item's shapes, in order. `ShapeTree.insert(Storable)`.
    pub fn insert(&mut self, board: &Board, item: usize) {
        assert!(!shapes_throw(board, item), "FreeRouting throws entering item {} into an autoroute tree: a pad beyond the critical bound", board.items[item].id);
        self.grow(item);
        let shapes = self.calculate(board, item);
        let count = shapes.len();
        let leaves = (0..count).map(|k| self.insert_leaf(board, item, k, count, shapes[k].as_ref())).collect();
        self.leaves[item] = leaves;
        self.shapes[item] = shapes;
    }

    /// Take the item's leaves out, in order. `ShapeTree.remove(Leaf[])`.
    pub fn remove(&mut self, item: usize) {
        if item < self.leaves.len() {
            for leaf in std::mem::take(&mut self.leaves[item]) {
                self.remove_leaf(leaf);
            }
        }
    }

    /// The trace's polyline, on the board, has changed but for its first
    /// `keep_start` and last `keep_end` shapes: those keep their leaves
    /// (the last renumbered), the rest are renewed.
    /// `ShapeSearchTree.change_entries`.
    pub fn change_entries(&mut self, board: &Board, item: usize, keep_start: usize, keep_end: usize) {
        let calculated = self.calculate(board, item);
        let old_leaves = std::mem::take(&mut self.leaves[item]);
        let old_shapes = std::mem::take(&mut self.shapes[item]);
        let (old_count, new_count) = (old_leaves.len(), calculated.len());
        assert!(keep_start + keep_end <= new_count.min(old_count), "change_entries: keeping {keep_start} + {keep_end} of {old_count} -> {new_count} shapes");
        for &leaf in &old_leaves[keep_start..old_count - keep_end] {
            self.remove_leaf(leaf);
        }
        let mut leaves = vec![None; new_count];
        let mut shapes = calculated;
        leaves[..keep_start].copy_from_slice(&old_leaves[..keep_start]);
        shapes[..keep_start].clone_from_slice(&old_shapes[..keep_start]);
        for j in 0..keep_end {
            let (new_index, old_index) = (new_count - keep_end + j, old_count - keep_end + j);
            leaves[new_index] = old_leaves[old_index];
            shapes[new_index] = old_shapes[old_index].clone();
        }
        for (i, leaf) in leaves.iter_mut().enumerate().take(new_count - keep_end).skip(keep_start) {
            *leaf = self.insert_leaf(board, item, i, new_count, shapes[i].as_ref());
        }
        self.leaves[item] = leaves;
        self.shapes[item] = shapes;
        self.refresh(board, item);
    }

    /// Trace `from` has been joined in front of trace `to`, whose polyline
    /// on the board is now the joined one; `change_order` if `from` ran
    /// the other way. `from`'s leaves but the one at the join move over, in
    /// front of `to`'s but its first, and the shapes where they meet are
    /// new. `ShapeSearchTree.merge_entries_in_front`.
    pub fn merge_in_front(&mut self, board: &Board, from: usize, to: usize, change_order: bool) {
        let calculated = self.calculate(board, to);
        let from_leaves = std::mem::take(&mut self.leaves[from]);
        let from_shapes = self.shapes[from].clone();
        let to_leaves = std::mem::take(&mut self.leaves[to]);
        let to_shapes = std::mem::take(&mut self.shapes[to]);
        let from_count_minus_1 = from_leaves.len() - 1;
        self.remove_leaf(from_leaves[if change_order { 0 } else { from_count_minus_1 }]);
        self.remove_leaf(to_leaves[0]);
        let new_count = calculated.len();
        let link_count = new_count + 2 - from_leaves.len() - to_leaves.len();
        let mut leaves = vec![None; new_count];
        let mut shapes = calculated;
        for i in 0..from_count_minus_1 {
            let from_no = if change_order { from_count_minus_1 - i } else { i };
            leaves[i] = from_leaves[from_no];
            shapes[i] = from_shapes[from_no].clone();
        }
        for i in 1..to_leaves.len() {
            let k = from_count_minus_1 + link_count + i - 1;
            leaves[k] = to_leaves[i];
            shapes[k] = to_shapes[i].clone();
        }
        for k in from_count_minus_1..from_count_minus_1 + link_count {
            leaves[k] = self.insert_leaf(board, to, k, new_count, shapes[k].as_ref());
        }
        self.leaves[to] = leaves;
        self.shapes[to] = shapes;
        self.refresh(board, to);
    }

    /// A middle piece of trace `from` has been cut out, leaving traces
    /// `start` and `end`, on the board: they take over `from`'s leaves
    /// but the one where each was cut, which is new; the leaves of the
    /// middle stay with `from`, to go with it. The pieces' shapes are their
    /// own. `ShapeSearchTree.reuse_entries_after_cutout`.
    pub fn reuse_after_cutout(&mut self, board: &Board, from: usize, start: usize, end: usize) {
        self.grow(start.max(end));
        let start_shapes = self.calculate(board, start);
        let end_shapes = self.calculate(board, end);
        let (start_count, end_count) = (start_shapes.len(), end_shapes.len());
        let mut start_leaves = vec![None; start_count];
        for (i, leaf) in start_leaves.iter_mut().enumerate().take(start_count - 1) {
            *leaf = self.leaves[from][i].take();
        }
        start_leaves[start_count - 1] = self.insert_leaf(board, start, start_count - 1, start_count, start_shapes[start_count - 1].as_ref());
        let mut end_leaves = vec![None; end_count];
        end_leaves[0] = self.insert_leaf(board, end, 0, end_count, end_shapes[0].as_ref());
        let from_count = self.leaves[from].len();
        for (i, leaf) in end_leaves.iter_mut().enumerate().skip(1) {
            *leaf = self.leaves[from][from_count - end_count + i].take();
        }
        self.leaves[start] = start_leaves;
        self.shapes[start] = start_shapes;
        self.leaves[end] = end_leaves;
        self.shapes[end] = end_shapes;
        self.refresh(board, start);
        self.refresh(board, end);
    }

    /// Give the item's shape `shape_no` a new shape, entered anew.
    /// `ShapeSearchTree.change_item_shape`.
    pub fn change_item_shape(&mut self, board: &Board, item: usize, shape_no: usize, shape: TileShape) {
        let old = self.leaves[item][shape_no].take();
        self.remove_leaf(old);
        let count = self.shapes[item].len();
        let leaf = self.insert_leaf(board, item, shape_no, count, Some(&shape));
        self.shapes[item][shape_no] = Some(shape);
        self.leaves[item][shape_no] = leaf;
    }

    /// Trace `from` has been joined at the end of trace `to`, whose
    /// polyline on the board is now the joined one; `change_order` if
    /// `from` ran the other way. `to`'s leaves but its last stay, `from`'s
    /// but the one at the join move over behind them, and the shapes where
    /// they meet are new. `ShapeSearchTree.merge_entries_at_end`.
    pub fn merge_at_end(&mut self, board: &Board, from: usize, to: usize, change_order: bool) {
        let calculated = self.calculate(board, to);
        let from_leaves = std::mem::take(&mut self.leaves[from]);
        let from_shapes = self.shapes[from].clone();
        let to_leaves = std::mem::take(&mut self.leaves[to]);
        let to_shapes = std::mem::take(&mut self.shapes[to]);
        let to_count_minus_1 = to_leaves.len() - 1;
        self.remove_leaf(to_leaves[to_count_minus_1]);
        self.remove_leaf(from_leaves[if change_order { from_leaves.len() - 1 } else { 0 }]);
        let new_count = calculated.len();
        let link_count = new_count + 2 - from_leaves.len() - to_leaves.len();
        let mut leaves = vec![None; new_count];
        let mut shapes = calculated;
        leaves[..to_count_minus_1].copy_from_slice(&to_leaves[..to_count_minus_1]);
        shapes[..to_count_minus_1].clone_from_slice(&to_shapes[..to_count_minus_1]);
        for i in 1..from_leaves.len() {
            let k = to_count_minus_1 + link_count + i - 1;
            let from_no = if change_order { from_leaves.len() - i - 1 } else { i };
            leaves[k] = from_leaves[from_no];
            shapes[k] = from_shapes[from_no].clone();
        }
        for k in to_count_minus_1..to_count_minus_1 + link_count {
            leaves[k] = self.insert_leaf(board, to, k, new_count, shapes[k].as_ref());
        }
        self.leaves[to] = leaves;
        self.shapes[to] = shapes;
        self.refresh(board, to);
    }
}

/// Whether FreeRouting throws working out the item's shapes in a
/// 45-degree tree: a pad with no bounding octagon, beyond the critical
/// bound. `ShapeSearchTree45Degree.calculate_tree_shapes(DrillItem)`.
fn shapes_throw(board: &Board, item: usize) -> bool {
    match &board.items[item].kind {
        ItemKind::Pin { pads, .. } | ItemKind::Via { pads, .. } => pads.iter().flatten().any(|p| p.bounding_octagon().is_none()),
        _ => false,
    }
}

/// How [`AutorouteTree::fingerprint`] names a leaf.
pub fn entry_name(entry: &Entry<TreeItem>) -> String {
    match entry {
        Entry::Item(t) => format!("{} {}", t.id, t.shape_index),
        Entry::Room { .. } => "R 0".to_string(),
    }
}
