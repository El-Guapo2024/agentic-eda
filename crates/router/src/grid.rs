//! Board rasterization and occupancy tracking.
//!
//! Occupancy is stored as flat `Vec`s indexed by `(cx, cy, layer)` rather
//! than a `HashMap`, since the whole point of a maze router's hot loop is
//! calling `passable()` millions of times — hashing a tuple key that often
//! dwarfed the actual pathfinding cost before this was array-indexed.

use eda_model::ir::{Point, Um};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occ {
    Pad,
    Track,
    Via,
}

#[derive(Debug, Clone)]
struct Cell {
    net_id: u32,
    kind: Occ,
}

const EMPTY: u32 = u32::MAX;

pub struct Grid {
    pub grid_um: Um,
    pub clearance_um: Um,
    pub track_half_um: Um,
    pub via_half_um: Um,
    pub min_x: Um,
    pub min_y: Um,
    pub cells_x: i64,
    pub cells_y: i64,
    pub num_layers: usize,
    /// Clearance dilation radius, in grid cells (Chebyshev).
    pub clearance_cells: i64,
    occ: Vec<Option<Cell>>,
    /// Precomputed point-in-polygon per cell (`cy * cells_x + cx`), so the
    /// A* hot loop never re-runs the polygon test.
    inside: Vec<bool>,
    /// Net name interning: index <-> name.
    net_ids: std::collections::HashMap<String, u32>,
    net_names: Vec<String>,
}

impl Grid {
    pub fn new(outline: Vec<Point>, grid_um: Um, clearance_um: Um, num_layers: usize) -> Self {
        Self::with_widths(outline, grid_um, clearance_um, grid_um, grid_um, num_layers)
    }

    /// Full constructor: `track_width` and `via_diameter` in µm feed the
    /// kind-aware clearance in [`Grid::passable_as`].
    pub fn with_widths(outline: Vec<Point>, grid_um: Um, clearance_um: Um, track_width: Um, via_diameter: Um, num_layers: usize) -> Self {
        let min_x = outline.iter().map(|p| p.x).min().unwrap_or(0);
        let min_y = outline.iter().map(|p| p.y).min().unwrap_or(0);
        let max_x = outline.iter().map(|p| p.x).max().unwrap_or(0);
        let max_y = outline.iter().map(|p| p.y).max().unwrap_or(0);
        let cells_x = ((max_x - min_x) / grid_um).max(0) + 1;
        let cells_y = ((max_y - min_y) / grid_um).max(0) + 1;
        // Widest separation any pair can need: via-to-via.
        let clearance_cells = ((clearance_um + via_diameter + grid_um - 1) / grid_um).max(1) - 1;

        let mut g = Grid {
            grid_um,
            clearance_um,
            track_half_um: track_width / 2,
            via_half_um: via_diameter / 2,
            min_x,
            min_y,
            cells_x,
            cells_y,
            num_layers,
            clearance_cells,
            occ: Vec::new(),
            inside: Vec::new(),
            net_ids: std::collections::HashMap::new(),
            net_names: Vec::new(),
        };
        let mut inside = vec![false; (cells_x * cells_y) as usize];
        for cy in 0..cells_y {
            for cx in 0..cells_x {
                if point_in_polygon(g.to_point(cx, cy), &outline) {
                    inside[(cy * cells_x + cx) as usize] = true;
                }
            }
        }
        g.inside = inside;
        g.occ = (0..(cells_x * cells_y * num_layers as i64) as usize).map(|_| None).collect();
        g
    }

    fn net_id(&mut self, net: &str) -> u32 {
        if let Some(&id) = self.net_ids.get(net) {
            return id;
        }
        let id = self.net_names.len() as u32;
        self.net_names.push(net.to_string());
        self.net_ids.insert(net.to_string(), id);
        id
    }

    fn net_id_ro(&self, net: &str) -> Option<u32> {
        self.net_ids.get(net).copied()
    }

    #[inline]
    fn idx(&self, cx: i64, cy: i64, layer: u8) -> Option<usize> {
        if cx < 0 || cy < 0 || cx >= self.cells_x || cy >= self.cells_y || (layer as usize) >= self.num_layers {
            return None;
        }
        Some(((cy * self.cells_x + cx) as usize) * self.num_layers + layer as usize)
    }

    /// Nearest grid cell to a board-space point (may be out of bounds).
    pub fn to_cell(&self, p: Point) -> (i64, i64) {
        let cx = (p.x - self.min_x + self.grid_um / 2).div_euclid(self.grid_um);
        let cy = (p.y - self.min_y + self.grid_um / 2).div_euclid(self.grid_um);
        (cx, cy)
    }

    pub fn to_point(&self, cx: i64, cy: i64) -> Point {
        Point { x: self.min_x + cx * self.grid_um, y: self.min_y + cy * self.grid_um }
    }

    pub fn in_bounds(&self, cx: i64, cy: i64) -> bool {
        cx >= 0 && cy >= 0 && cx < self.cells_x && cy < self.cells_y
    }

    #[inline]
    pub fn in_outline(&self, cx: i64, cy: i64) -> bool {
        if !self.in_bounds(cx, cy) {
            return false;
        }
        self.inside[(cy * self.cells_x + cx) as usize]
    }

    pub fn set(&mut self, cx: i64, cy: i64, layer: u8, net: &str, kind: Occ) {
        let net_id = self.net_id(net);
        if let Some(i) = self.idx(cx, cy, layer) {
            self.occ[i] = Some(Cell { net_id, kind });
        }
    }

    pub fn clear_net(&mut self, net: &str) {
        let Some(net_id) = self.net_id_ro(net) else { return };
        for c in self.occ.iter_mut() {
            if let Some(cell) = c {
                if cell.net_id == net_id && cell.kind != Occ::Pad {
                    *c = None;
                }
            }
        }
    }

    /// Copper half-extent (µm) beyond a cell centre for an occupant kind.
    /// Pads are rasterised over their area, so their edge lies at most half
    /// a cell past the outermost occupied cell centre.
    #[inline]
    fn half_extent(&self, kind: Occ) -> Um {
        match kind {
            Occ::Track => self.track_half_um,
            Occ::Via => self.via_half_um,
            Occ::Pad => self.grid_um / 2,
        }
    }

    /// Minimum centre-to-centre separation, in cells, between copper of
    /// kinds `a` and `b` on different nets.
    #[inline]
    fn min_sep_cells(&self, a: Occ, b: Occ) -> i64 {
        let um = self.clearance_um + self.half_extent(a) + self.half_extent(b);
        (um + self.grid_um - 1) / self.grid_um
    }

    /// True if (cx,cy,layer) is usable for a track of `net`.
    #[inline]
    pub fn passable(&self, cx: i64, cy: i64, layer: u8, net: &str) -> bool {
        self.passable_as(cx, cy, layer, net, Occ::Track)
    }

    /// True if (cx,cy,layer) is usable for copper of kind `me` on `net`:
    /// in the outline, and every different-net occupant within reach sits
    /// at least the kind-pair's required separation away (Chebyshev).
    #[inline]
    pub fn passable_as(&self, cx: i64, cy: i64, layer: u8, net: &str, me: Occ) -> bool {
        if !self.in_outline(cx, cy) {
            return false;
        }
        let net_id = self.net_id_ro(net).unwrap_or(EMPTY);
        let r = self.clearance_cells;
        for dx in -r..=r {
            for dy in -r..=r {
                if let Some(i) = self.idx(cx + dx, cy + dy, layer) {
                    if let Some(c) = &self.occ[i] {
                        if c.net_id != net_id {
                            let d = dx.abs().max(dy.abs());
                            if d < self.min_sep_cells(me, c.kind) {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        true
    }

    /// Nets whose occupied cells (any layer) fall within the bounding box
    /// of `a`..`b`, dilated by `margin` cells. Used to pick rip-up victims.
    pub fn nets_in_region(&self, a: (i64, i64), b: (i64, i64), margin: i64) -> std::collections::HashSet<String> {
        let (x0, x1) = (a.0.min(b.0) - margin, a.0.max(b.0) + margin);
        let (y0, y1) = (a.1.min(b.1) - margin, a.1.max(b.1) + margin);
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.cells_x - 1);
        let y1 = y1.min(self.cells_y - 1);
        let mut set = std::collections::HashSet::new();
        if x0 > x1 || y0 > y1 {
            return set;
        }
        for cy in y0..=y1 {
            for cx in x0..=x1 {
                for l in 0..self.num_layers as u8 {
                    if let Some(i) = self.idx(cx, cy, l) {
                        if let Some(c) = &self.occ[i] {
                            if c.kind != Occ::Pad {
                                set.insert(self.net_names[c.net_id as usize].clone());
                            }
                        }
                    }
                }
            }
        }
        set
    }
}

fn point_in_polygon(p: Point, poly: &[Point]) -> bool {
    if poly.len() < 3 {
        // No real outline given: don't constrain.
        return true;
    }
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (poly[i].x, poly[i].y);
        let (xj, yj) = (poly[j].x, poly[j].y);
        if (yi > p.y) != (yj > p.y) {
            let x_int = xi as f64 + ((p.y - yi) as f64) * ((xj - xi) as f64) / ((yj - yi) as f64);
            if (p.x as f64) < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}
