//! A port of the *structure* KiCad's `DRC_RTREE` (`pcbnew/drc/drc_rtree.h`)
//! gives every test provider: a spatial index of item bounding boxes,
//! queried with a single worst-case clearance so a provider tests candidate
//! pairs instead of all pairs. KiCad builds one packed R-tree per copper
//! layer (`BOARD::m_CopperItemRTreeCache`), inserts every item's bbox
//! inflated by the run's worst-case clearance
//! (`drc_cache_generator.cpp::Insert(item, layer, m_DRCMaxClearance)`), and
//! queries it by inflating the *reference* item's own bbox by that same
//! clearance (`DRC_RTREE::QueryColliding`) -- a broad phase that only
//! narrows down candidates; the exact geometry test still runs on whatever
//! the broad phase returns, same as every provider in this crate already
//! does via [`crate::kimath::Shape::collides`].
//!
//! This is a uniform grid spatial hash, not a literal R-tree (no new
//! crates.io dependency pulls one in), but it gives the same asymptotic win
//! for the one operation every provider needs: "every item whose bounding
//! box could be within `clearance` of mine". Inflating only at query time
//! (rather than at both insert *and* query time, as KiCad's own
//! implementation does) is a one-line simplification that is still exactly
//! correct: two boxes are within `clearance` of each other iff either one,
//! inflated by `clearance`, overlaps the other's true box.
use eda_model::ir::Um;
use std::collections::{HashMap, HashSet};

type CellKey = (i64, i64);
type BBox = (Um, Um, Um, Um);

pub struct DrcRTree {
    cell_size: Um,
    cells: HashMap<CellKey, Vec<usize>>,
    boxes: Vec<BBox>,
}

impl DrcRTree {
    /// `cell_size` should track the run's worst-case clearance ([`crate::
    /// constraints::worst_case_clearance`]) -- large enough that a query
    /// (which always spans at least one clearance-width) touches only a
    /// handful of cells, small enough that a dense board doesn't pile every
    /// item into one bucket. Degenerate (<=0, e.g. a board with no
    /// clearance configured anywhere) floors at 1 so cell arithmetic never
    /// divides by zero.
    pub fn new(cell_size: Um) -> Self {
        DrcRTree { cell_size: cell_size.max(1), cells: HashMap::new(), boxes: Vec::new() }
    }

    fn cell_of(&self, x: Um, y: Um) -> CellKey {
        (x.div_euclid(self.cell_size), y.div_euclid(self.cell_size))
    }

    /// Index item `id`'s true (uninflated) bounding box. `id` is caller-
    /// assigned (typically a `Vec` index into the provider's own item
    /// list) and must be stable across `insert`/`query` for one `DrcRTree`.
    pub fn insert(&mut self, id: usize, bbox: BBox) {
        if self.boxes.len() <= id {
            self.boxes.resize(id + 1, (0, 0, 0, 0));
        }
        self.boxes[id] = bbox;
        let (x0, y0, x1, y1) = bbox;
        let (cx0, cy0) = self.cell_of(x0, y0);
        let (cx1, cy1) = self.cell_of(x1, y1);
        for cx in cx0..=cx1 {
            for cy in cy0..=cy1 {
                self.cells.entry((cx, cy)).or_default().push(id);
            }
        }
    }

    /// Every indexed id whose true bbox overlaps `query_bbox` -- the caller
    /// inflates its own reference item's box by whatever clearance it cares
    /// about before calling, same contract as `DRC_RTREE::QueryColliding`.
    /// Each id appears at most once, even if its bbox spans several cells.
    pub fn query(&self, query_bbox: BBox) -> Vec<usize> {
        let (qx0, qy0, qx1, qy1) = query_bbox;
        let (cx0, cy0) = self.cell_of(qx0, qy0);
        let (cx1, cy1) = self.cell_of(qx1, qy1);
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for cx in cx0..=cx1 {
            for cy in cy0..=cy1 {
                let Some(ids) = self.cells.get(&(cx, cy)) else { continue };
                for &id in ids {
                    let (x0, y0, x1, y1) = self.boxes[id];
                    if x1 < qx0 || qx1 < x0 || y1 < qy0 || qy1 < y0 {
                        continue; // shares a cell with the query but the item's own bbox doesn't overlap it
                    }
                    if seen.insert(id) {
                        out.push(id);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nearby_and_skips_far_items() {
        let mut idx = DrcRTree::new(1000);
        idx.insert(0, (0, 0, 100, 100));
        idx.insert(1, (50_000, 50_000, 50_100, 50_100));
        let hits = idx.query((-500, -500, 500, 500));
        assert_eq!(hits, vec![0]);
    }

    #[test]
    fn an_item_spanning_many_cells_is_returned_once() {
        let mut idx = DrcRTree::new(100);
        idx.insert(0, (0, 0, 10_000, 50)); // a long thin "track" spanning ~100 cells
        let hits = idx.query((4_900, -200, 5_100, 200));
        assert_eq!(hits, vec![0]);
    }

    #[test]
    fn empty_index_returns_nothing() {
        let idx = DrcRTree::new(500);
        assert!(idx.query((-1000, -1000, 1000, 1000)).is_empty());
    }

    #[test]
    fn query_matches_touching_bbox() {
        let mut idx = DrcRTree::new(500);
        idx.insert(7, (0, 0, 100, 100));
        // Query box touches at a single point (101,101 vs inclusive 100) --
        // bbox overlap test is inclusive, matching the all-pairs code this
        // replaces (which never skips a boundary-touching pair).
        assert_eq!(idx.query((100, 100, 200, 200)), vec![7]);
        assert!(idx.query((101, 101, 200, 200)).is_empty());
    }
}
