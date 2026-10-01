//! Port of the *structure* `PNS::INDEX` (`pcbnew/router/pns_index.h`) gives
//! `NODE`: a spatial index of item bounding boxes so a collision query
//! tests nearby candidates instead of every item on the board.
//!
//! Unlike `eda_drc::rtree::DrcRTree` (built once per DRC run, query-only),
//! this index has to support `remove`/re-`insert` as the interactive router
//! adds, shoves, and retires items on every mouse move -- so it is its own
//! small uniform-grid structure rather than a dependency on that one,
//! keeping `crates/drc` untouched. Same asymptotic idea as `DrcRTree`
//! (bucket by cell, inflate the query box by the caller's clearance): see
//! that module's doc comment for why a uniform grid stands in for KiCad's
//! real R-tree.

use crate::item::ItemId;
use eda_model::ir::Um;
use std::collections::{HashMap, HashSet};

type CellKey = (i64, i64);
pub type BBox = (Um, Um, Um, Um);

#[derive(Default)]
pub struct Index {
    cell_size: Um,
    cells: HashMap<CellKey, Vec<ItemId>>,
    boxes: HashMap<ItemId, BBox>,
}

impl Index {
    pub fn new(cell_size: Um) -> Self {
        Index { cell_size: cell_size.max(1), cells: HashMap::new(), boxes: HashMap::new() }
    }

    fn cell_of(&self, x: Um, y: Um) -> CellKey {
        (x.div_euclid(self.cell_size), y.div_euclid(self.cell_size))
    }

    fn cells_for(&self, bbox: BBox) -> Vec<CellKey> {
        let (x0, y0) = self.cell_of(bbox.0, bbox.1);
        let (x1, y1) = self.cell_of(bbox.2, bbox.3);
        (x0..=x1).flat_map(move |cx| (y0..=y1).map(move |cy| (cx, cy))).collect()
    }

    pub fn insert(&mut self, id: ItemId, bbox: BBox) {
        for key in self.cells_for(bbox) {
            self.cells.entry(key).or_default().push(id);
        }
        self.boxes.insert(id, bbox);
    }

    pub fn remove(&mut self, id: ItemId) {
        let Some(bbox) = self.boxes.remove(&id) else { return };
        for key in self.cells_for(bbox) {
            if let Some(v) = self.cells.get_mut(&key) {
                v.retain(|&x| x != id);
                if v.is_empty() {
                    self.cells.remove(&key);
                }
            }
        }
    }

    /// Every id whose stored bbox overlaps `query_bbox`. The caller
    /// inflates its own query box by whatever clearance it cares about --
    /// same contract as `INDEX::Query`/`DrcRTree::query`.
    pub fn query(&self, query_bbox: BBox) -> Vec<ItemId> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for key in self.cells_for(query_bbox) {
            let Some(ids) = self.cells.get(&key) else { continue };
            for &id in ids {
                let Some(&(x0, y0, x1, y1)) = self.boxes.get(&id) else { continue };
                if x1 < query_bbox.0 || query_bbox.2 < x0 || y1 < query_bbox.1 || query_bbox.3 < y0 {
                    continue;
                }
                if seen.insert(id) {
                    out.push(id);
                }
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }
}

impl Clone for Index {
    /// NODE branching clones the whole index (see `node.rs`'s doc comment
    /// on why this port branches by cloning rather than KiCad's
    /// copy-on-write overlay).
    fn clone(&self) -> Self {
        Index { cell_size: self.cell_size, cells: self.cells.clone(), boxes: self.boxes.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_query_remove() {
        let mut idx = Index::new(1000);
        idx.insert(1, (0, 0, 100, 100));
        idx.insert(2, (50_000, 0, 50_100, 100));
        assert_eq!(idx.query((-10, -10, 10, 10)), vec![1]);
        idx.remove(1);
        assert!(idx.query((-10, -10, 10, 10)).is_empty());
        assert_eq!(idx.query((49_000, -10, 51_000, 10)), vec![2]);
    }

    #[test]
    fn item_spanning_many_cells_returned_once() {
        let mut idx = Index::new(100);
        idx.insert(9, (0, 0, 10_000, 50));
        assert_eq!(idx.query((4_900, -200, 5_100, 200)), vec![9]);
    }
}
