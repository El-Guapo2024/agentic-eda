//! A uniform spatial grid standing in for `CN_RTREE`
//! (`pcbnew/connectivity/connectivity_rtree.h`), per the task's explicit
//! allowance to use "a grid if simpler, but keep the semantics."
//!
//! `CN_RTREE` is a 3D R-tree keyed on `(layer, x, y)`: `Insert` takes an
//! item's `(StartLayer, BBox)`/`(EndLayer, BBox)` corners, and `Query`
//! returns every item whose box intersects a given `(layerRange, bbox)`.
//! We keep the same *semantics* -- "give me every candidate whose bounding
//! box could plausibly touch mine" -- but split it into two cheaper
//! pieces: this grid indexes only `(x, y)`, and the caller
//! ([`crate::algo::build_graph`]) checks the layer range itself once it
//! already has a short candidate list, which is exactly as correct and
//! avoids a 3D structure for what is, on any board this crate sees, a few
//! hundred items at most.
//!
//! Indexing is exact, not approximate: an item is inserted into every
//! cell its bounding box overlaps (not just its center cell), so querying
//! by the cells a box overlaps is guaranteed to find every other item
//! whose box intersects it, regardless of cell size.

use eda_model::ir::Um;

type CellKey = (i64, i64);

pub struct Grid {
    cell_size: i64,
    cells: std::collections::HashMap<CellKey, Vec<usize>>,
}

impl Grid {
    /// Build an index over `boxes` (one axis-aligned `(x0, y0, x1, y1)`
    /// per item, indices matching the caller's item list).
    pub fn build(boxes: &[(Um, Um, Um, Um)]) -> Self {
        let cell_size = Self::pick_cell_size(boxes);
        let mut cells: std::collections::HashMap<CellKey, Vec<usize>> = std::collections::HashMap::new();
        for (idx, &b) in boxes.iter().enumerate() {
            for key in Self::cells_of(b, cell_size) {
                cells.entry(key).or_default().push(idx);
            }
        }
        Grid { cell_size, cells }
    }

    /// Every item index whose stored box overlaps `bbox` (deduplicated).
    pub fn query(&self, bbox: (Um, Um, Um, Um)) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for key in Self::cells_of(bbox, self.cell_size) {
            if let Some(v) = self.cells.get(&key) {
                out.extend(v.iter().copied());
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn cells_of(b: (Um, Um, Um, Um), cell_size: i64) -> impl Iterator<Item = CellKey> {
        let (x0, y0, x1, y1) = (b.0.div_euclid(cell_size), b.1.div_euclid(cell_size), b.2.div_euclid(cell_size), b.3.div_euclid(cell_size));
        (x0..=x1).flat_map(move |cx| (y0..=y1).map(move |cy| (cx, cy)))
    }

    /// Aim for a modest handful of items per cell on average: cell size is
    /// the overall bounding box's diagonal-ish scale divided by
    /// `sqrt(item count)`, clamped to a sane minimum (0.5 mm) so a board
    /// with one giant item and many tiny ones doesn't collapse to one cell.
    fn pick_cell_size(boxes: &[(Um, Um, Um, Um)]) -> i64 {
        const MIN_CELL_UM: i64 = 500;
        if boxes.is_empty() {
            return MIN_CELL_UM;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (Um::MAX, Um::MAX, Um::MIN, Um::MIN);
        for b in boxes {
            x0 = x0.min(b.0);
            y0 = y0.min(b.1);
            x1 = x1.max(b.2);
            y1 = y1.max(b.3);
        }
        let area = ((x1 - x0).max(1) as f64) * ((y1 - y0).max(1) as f64);
        let target = (area / boxes.len().max(1) as f64).sqrt();
        (target as i64).max(MIN_CELL_UM)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_overlapping_boxes_across_cell_boundaries() {
        let boxes = vec![(0, 0, 100, 100), (90, 90, 200, 200), (10_000, 10_000, 10_100, 10_100)];
        let grid = Grid::build(&boxes);
        let hits = grid.query((50, 50, 95, 95));
        assert!(hits.contains(&0));
        assert!(hits.contains(&1));
        assert!(!hits.contains(&2));
    }

    #[test]
    fn query_dedupes_items_spanning_many_cells() {
        let boxes = vec![(0, 0, 50_000, 50_000)];
        let grid = Grid::build(&boxes);
        let hits = grid.query((0, 0, 50_000, 50_000));
        assert_eq!(hits, vec![0]);
    }
}
