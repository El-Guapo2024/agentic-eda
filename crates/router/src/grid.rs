//! Board rasterization and occupancy tracking.
//!
//! Occupancy is stored as flat `Vec`s indexed by `(cx, cy, layer)` rather
//! than a `HashMap`, since the whole point of a maze router's hot loop is
//! calling `passable()` millions of times — hashing a tuple key that often
//! dwarfed the actual pathfinding cost before this was array-indexed.

use eda_model::footprint::PlacedPad;
use eda_model::ir::{Point, Um};
use eda_model::RoutingTuning;

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
    /// Half-extent (µm) of the *widest* copper this cell actually holds.
    ///
    /// Not derivable from `kind`: a net's escape track legitimately runs
    /// over its own pad, and `kind` deliberately stays `Pad` there (rip-up
    /// must not erase the pad obstacle). The track's copper is wider than
    /// the pad rasterisation's half-cell extent, though, so deriving the
    /// required separation from `kind` alone under-reported the copper by
    /// `track_half - grid/2` and let a foreign track sit one cell too
    /// close. Every write raises this to the widest kind seen.
    half_um: Um,
}

const EMPTY: u32 = u32::MAX;
/// Summary marker: copper of two or more different nets is within
/// separation of this cell, so it is impassable for every net.
const MULTI: u32 = u32::MAX - 1;

/// KiCad's default board-setup copper-to-edge clearance, enforced by
/// `kicad-cli pcb drc` against Edge.Cuts.
/// Default copper-to-edge clearance (see `RoutingTuning::edge_clearance_um`).
pub const EDGE_CLEARANCE_UM: Um = 500;

fn seg_point_dist(a: Point, b: Point, p: Point) -> f64 {
    let (ax, ay, bx, by, px, py) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64, p.x as f64, p.y as f64);
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0) };
    ((px - (ax + t * dx)).powi(2) + (py - (ay + t * dy)).powi(2)).sqrt()
}

pub struct Grid {
    /// Router tuning for this board (from the intent's `board.tuning`).
    pub tuning: RoutingTuning,
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
    /// Track half-width per net id, for nets whose class differs from the
    /// board default. `set` stamps a net's own copper at its own width;
    /// the *querying* side (`half_extent`) stays at the widest class on the
    /// board, because the clearance summaries are precomputed for one
    /// querying width and must never under-state what a net owes a cell.
    net_track_half: std::collections::HashMap<u32, Um>,
    occ: Vec<Option<Cell>>,
    /// Hard blocks per (cell, layer) with no clearance dilation: gap strips
    /// between SMD pads of one footprint, and (per edge) own-net pads a
    /// track must not use as a stepping stone.
    blocked: Vec<bool>,
    /// Extra A* step cost per (cell, layer): soft keep-out for silkscreen
    /// refdes boxes.
    penalty: Vec<u8>,
    /// Soft keep-out per (cell, layer) for track copper: impassable while
    /// `soft_active`, merely penalised otherwise. An edge is first tried
    /// with the keep-outs enforced and falls back to the penalty-only pass
    /// when that leaves no path at all. Dilated by the track's own
    /// half-width, matching the gate's exact geometric test — not by the
    /// (much larger) via radius, which used to seal off narrow gaps a
    /// track alone could still thread.
    soft: Vec<bool>,
    /// Soft keep-out per (cell, layer) for via copper: same idea, dilated
    /// by the via radius (a via's copper is wider than a track's).
    soft_via: Vec<bool>,
    /// Track keep-out enforced (strict pass).
    pub soft_active: bool,
    /// Via keep-out enforced. Relaxed separately, and later, than the
    /// track one: a via's copper is what most often ends up under a label
    /// on the penalty-only pass, and a via can nearly always sit one cell
    /// over, so the first fallback tier keeps vias out of labels while
    /// letting tracks pay the penalty.
    pub soft_via_active: bool,
    /// Per (cell, layer): which net's copper lies within *track*
    /// separation of the cell — `EMPTY`, one net id (the cell is passable
    /// only for that net), or `MULTI`. Maintained incrementally by
    /// [`Grid::set`] (copper only ever grows between rip-ups) and rebuilt
    /// wholesale by [`Grid::clear_net`], so the A* hot loop answers
    /// [`Grid::passable_as`] with one lookup instead of a radius scan.
    /// Clearance summary for tracks, one array per *querying* width.
    /// A summary answers "would copper of width W here violate clearance
    /// against what is already stamped", and the answer depends on W, so a
    /// board with net classes needs one per class width. Bucket 0 is the
    /// board default. Building a single summary at the widest class instead
    /// looked tempting and sealed every fine-pitch pad on L4.
    near_track: Vec<Vec<u32>>,
    /// Querying half-widths, indexed by bucket. Bucket 0 is the default.
    track_query_halves: Vec<Um>,
    /// Which bucket a net queries. Absent = bucket 0.
    net_bucket: std::collections::HashMap<u32, usize>,
    /// Same for *via* separation (a via's copper is wider, so its radius
    /// is larger).
    near_via: Vec<u32>,
    /// Per (cell, layer): pad copper within the via-in-pad radius, i.e. a
    /// via may not sit here (see [`Grid::via_pad_radius_cells`]).
    via_near_pad: Vec<bool>,
    /// PathFinder-style history cost per (cell, layer): bumped around the
    /// pocket a net could not escape each time its fence is ripped up, so
    /// the victims' reroutes stop walling the same pad in again. Never
    /// decays within a run.
    hist: Vec<u8>,
    /// Every pad's rectangle, per layer, for exact separation stamping:
    /// a track/via centre must keep `clearance + its own half-width` from
    /// the pad rect (the gate's measure, see `PlacedPad::rect_distance`). Cell-quantised pad blobs (half a cell of copper at
    /// every rasterised cell centre, Chebyshev distance) read a 0.65 mm
    /// pitch TSSOP's straight-out escape — 500 µm to the neighbour's
    /// copper, 300 needed — as blocked; real geometry does not.
    pads_exact: Vec<(u32, u8, PlacedPad)>,
    /// `EDA_ROUTE_CHECK_NEAR` set: cross-check every summary lookup
    /// against the reference radius scan (slow; for debugging only).
    check_near: bool,
    /// Precomputed point-in-polygon per cell (`cy * cells_x + cx`), so the
    /// A* hot loop never re-runs the polygon test.
    inside: Vec<bool>,
    /// Precomputed distance (µm) from each cell centre to the nearest
    /// outline edge, for the copper-to-edge clearance.
    edge_dist: Vec<Um>,
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
            tuning: RoutingTuning::default(),
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
            net_track_half: std::collections::HashMap::new(),
            occ: Vec::new(),
            blocked: Vec::new(),
            penalty: Vec::new(),
            soft: Vec::new(),
            soft_via: Vec::new(),
            soft_active: false,
            soft_via_active: false,
            near_track: Vec::new(),
            track_query_halves: Vec::new(),
            net_bucket: std::collections::HashMap::new(),
            near_via: Vec::new(),
            via_near_pad: Vec::new(),
            hist: Vec::new(),
            pads_exact: Vec::new(),
            check_near: std::env::var_os("EDA_ROUTE_CHECK_NEAR").is_some(),
            inside: Vec::new(),
            edge_dist: Vec::new(),
            net_ids: std::collections::HashMap::new(),
            net_names: Vec::new(),
        };
        let mut inside = vec![false; (cells_x * cells_y) as usize];
        let mut edge_dist = vec![0; (cells_x * cells_y) as usize];
        let n = outline.len();
        for cy in 0..cells_y {
            for cx in 0..cells_x {
                let p = g.to_point(cx, cy);
                if point_in_polygon(p, &outline) {
                    inside[(cy * cells_x + cx) as usize] = true;
                    let d = (0..n).map(|i| seg_point_dist(outline[i], outline[(i + 1) % n], p)).fold(f64::MAX, f64::min);
                    edge_dist[(cy * cells_x + cx) as usize] = d as Um;
                }
            }
        }
        g.inside = inside;
        g.edge_dist = edge_dist;
        let n = (cells_x * cells_y * num_layers as i64) as usize;
        g.occ = (0..n).map(|_| None).collect();
        g.blocked = vec![false; n];
        g.penalty = vec![0; n];
        g.soft = vec![false; n];
        g.soft_via = vec![false; n];
        g.track_query_halves = vec![track_width / 2];
        g.near_track = vec![vec![EMPTY; n]];
        g.near_via = vec![EMPTY; n];
        g.via_near_pad = vec![false; n];
        g.hist = vec![0; n];
        g
    }

    /// Raise the history cost by `amount` on every cell within Chebyshev
    /// radius `r` of `cells` (each on its own layer).
    pub fn bump_history(&mut self, cells: &[(i64, i64, u8)], r: i64, amount: u8) {
        for &(cx, cy, l) in cells {
            for dx in -r..=r {
                for dy in -r..=r {
                    if let Some(i) = self.idx(cx + dx, cy + dy, l) {
                        self.hist[i] = self.hist[i].saturating_add(amount);
                    }
                }
            }
        }
    }

    /// Record copper of `net_id` with half-extent `half_um` at (cx, cy,
    /// layer) in the separation summaries: every cell closer than the
    /// kind-aware minimum separation learns that this net is nearby.
    fn stamp(&mut self, cx: i64, cy: i64, layer: u8, net_id: u32, half_um: Um, kind: Occ) {
        if kind == Occ::Pad {
            let r = self.via_pad_radius_cells();
            for dx in -r..=r {
                for dy in -r..=r {
                    if let Some(j) = self.idx(cx + dx, cy + dy, layer) {
                        self.via_near_pad[j] = true;
                    }
                }
            }
            // Pad copper itself is stamped exactly from its rectangle (see
            // `pads_exact`); only an own-net track/via laid over the pad
            // cell adds cell-centred copper here.
            if half_um == 0 {
                return;
            }
        }
        for b in 0..self.track_query_halves.len() {
            let r = self.sep_cells_for(self.track_query_halves[b], half_um) - 1;
            for dx in -r..=r {
                for dy in -r..=r {
                    if let Some(j) = self.idx(cx + dx, cy + dy, layer) {
                        let slot = &mut self.near_track[b][j];
                        *slot = match *slot {
                            EMPTY => net_id,
                            x if x == net_id => x,
                            _ => MULTI,
                        };
                    }
                }
            }
        }
        let r = self.min_sep_cells_half(Occ::Via, half_um) - 1;
        for dx in -r..=r {
            for dy in -r..=r {
                if let Some(j) = self.idx(cx + dx, cy + dy, layer) {
                    let slot = &mut self.near_via[j];
                    *slot = match *slot {
                        EMPTY => net_id,
                        x if x == net_id => x,
                        _ => MULTI,
                    };
                }
            }
        }
    }

    /// Public form of [`Grid::sep_cells_for`], for the negotiation
    /// bookkeeping, which has to size a net's claim to its own copper.
    pub fn sep_cells_for_pub(&self, query_half_um: Um, b_half_um: Um) -> i64 {
        self.sep_cells_for(query_half_um, b_half_um)
    }

    /// Cells of separation a querying half-width owes copper of half-width
    /// `b_half_um`.
    fn sep_cells_for(&self, query_half_um: Um, b_half_um: Um) -> i64 {
        let um = self.clearance_um + query_half_um + b_half_um;
        (um + self.grid_um - 1) / self.grid_um
    }

    /// Exact separation stamping for one pad's copper on `layer`: every
    /// cell whose centre is closer than `clearance + half-width(kind)` to
    /// the pad edge learns that `net_id` is nearby (track and via
    /// summaries separately).
    fn stamp_pad_exact(&mut self, net_id: u32, layer: u8, pad: &PlacedPad) {
        // One pass per track-width bucket plus one for vias. Each bucket
        // asks its own question -- "how close may a track of *this* half
        // width come to this pad" -- so each needs its own radius.
        //
        // This used to stamp `near_track[0]` only, at the default track
        // half width. Every bucket a net class created was therefore born
        // empty of pads and stayed that way, so a 0.45 mm power net saw no
        // pad separation at all and routed straight into pad copper: on L4
        // at net-class widths that was 251 routing_clearance failures the
        // negotiator scored as zero conflicts, against pads like J4.2.
        let vias = self.track_query_halves.len();
        for b in 0..=vias {
            let is_via = b == vias;
            let need = self.clearance_um + if is_via { self.half_extent(Occ::Via) } else { self.track_query_halves[b] };
            let (hw, hh) = (pad.size.0 / 2 + need + self.grid_um, pad.size.1 / 2 + need + self.grid_um);
            let (x0, y0) = self.to_cell(Point { x: pad.center.x - hw, y: pad.center.y - hh });
            let (x1, y1) = self.to_cell(Point { x: pad.center.x + hw, y: pad.center.y + hh });
            for cx in x0..=x1 {
                for cy in y0..=y1 {
                    let Some(j) = self.idx(cx, cy, layer) else { continue };
                    if pad.rect_distance(self.to_point(cx, cy)) < need as f64 {
                        let slot = if is_via { &mut self.near_via[j] } else { &mut self.near_track[b][j] };
                        *slot = match *slot {
                            EMPTY => net_id,
                            x if x == net_id => x,
                            _ => MULTI,
                        };
                    }
                }
            }
        }
    }

    /// Register a pad: its rasterised `cells` become `Occ::Pad` copper of
    /// `net` on `layer` (own-net passable, via-in-pad keep-out), and its
    /// real rectangle is stamped exactly into the separation summaries.
    pub fn add_pad(&mut self, net: &str, layer: u8, pad: &PlacedPad, cells: &[(i64, i64)]) {
        let net_id = self.net_id(net);
        for &(cx, cy) in cells {
            self.set_occ_pad(cx, cy, layer, net_id);
        }
        self.pads_exact.push((net_id, layer, pad.clone()));
        self.stamp_pad_exact(net_id, layer, pad);
    }

    /// Pad occupancy for one cell without any exact copper registration.
    fn set_occ_pad(&mut self, cx: i64, cy: i64, layer: u8, net_id: u32) {
        let half = 0;
        if let Some(i) = self.idx(cx, cy, layer) {
            if let Some(c) = &mut self.occ[i] {
                if c.net_id == net_id {
                    c.half_um = c.half_um.max(half);
                    return;
                }
            }
            self.occ[i] = Some(Cell { net_id, kind: Occ::Pad, half_um: half });
            self.stamp(cx, cy, layer, net_id, half, Occ::Pad);
        }
    }

    /// Rebuild every separation summary from the occupancy map (after a
    /// rip-up removed copper — the summaries only ever grow otherwise).
    fn rebuild_near(&mut self) {
        for b in self.near_track.iter_mut() {
            for v in b.iter_mut() {
                *v = EMPTY;
            }
        }
        for v in self.near_via.iter_mut() {
            *v = EMPTY;
        }
        for v in self.via_near_pad.iter_mut() {
            *v = false;
        }
        let cells: Vec<(usize, u32, Um, Occ)> =
            self.occ.iter().enumerate().filter_map(|(i, c)| c.as_ref().map(|c| (i, c.net_id, c.half_um, c.kind))).collect();
        for (i, net_id, half_um, kind) in cells {
            // Inverse of `idx`: cell-major, layer innermost.
            let layer = (i % self.num_layers) as u8;
            let cell = (i / self.num_layers) as i64;
            let (cx, cy) = (cell % self.cells_x, cell / self.cells_x);
            self.stamp(cx, cy, layer, net_id, half_um, kind);
        }
        let pads = std::mem::take(&mut self.pads_exact);
        for (net_id, layer, pad) in &pads {
            self.stamp_pad_exact(*net_id, *layer, pad);
        }
        self.pads_exact = pads;
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

    /// Declare that `net` routes at `width_um`. Call once per net before
    /// routing; nets never declared use the board default.
    /// Call before any copper is stamped: it allocates a clearance summary
    /// for the new width, and summaries only grow correctly from empty.
    pub fn set_net_width(&mut self, net: &str, width_um: Um) {
        let id = self.net_id(net);
        let half = width_um / 2;
        self.net_track_half.insert(id, half);
        let bucket = match self.track_query_halves.iter().position(|&h| h == half) {
            Some(b) => b,
            None => {
                let n = self.near_track[0].len();
                self.track_query_halves.push(half);
                self.near_track.push(vec![EMPTY; n]);
                // A bucket is born empty, so any pad already stamped is
                // invisible in it. Callers are meant to declare every class
                // width before laying copper, but a bucket that silently
                // lacks the board's pads is the kind of hole that shows up
                // later as unexplained clearance failures -- so re-stamp
                // rather than trust the call order.
                let pads = std::mem::take(&mut self.pads_exact);
                for (pid, layer, pad) in &pads {
                    self.stamp_pad_exact(*pid, *layer, pad);
                }
                self.pads_exact = pads;
                self.track_query_halves.len() - 1
            }
        };
        self.net_bucket.insert(id, bucket);
    }

    /// Track half-width `net` lays copper at.
    pub fn net_track_half_of(&self, net: &str) -> Um {
        self.net_ids.get(net).and_then(|id| self.net_track_half.get(id)).copied().unwrap_or(self.track_half_um)
    }

    #[inline]
    fn bucket_of(&self, net_id: u32) -> usize {
        self.net_bucket.get(&net_id).copied().unwrap_or(0)
    }

    /// The single choke point through which all copper enters the grid.
    /// Whatever `kind` survives for obstacle bookkeeping, `half_um` always
    /// ends up at the widest copper the cell holds, so `passable_as`
    /// cannot under-state the separation another net owes this cell.
    pub fn set(&mut self, cx: i64, cy: i64, layer: u8, net: &str, kind: Occ) {
        let net_id = self.net_id(net);
        // `half_um` is the cell-centred copper *laid* here (track/via);
        // pad copper is stamped exactly from its rectangle, so a pad-only
        // cell carries 0 — a rasterised pad cell may lie up to half a cell
        // outside the real copper, and a track running over it must stamp
        // its own width from there (ldo: GND over its tab's edge cell,
        // VOUT then laid adjacent with a 54 µm gap).
        let half = match kind {
            Occ::Pad => 0,
            Occ::Track => self.net_track_half.get(&net_id).copied().unwrap_or(self.track_half_um),
            _ => self.half_extent(kind),
        };
        if let Some(i) = self.idx(cx, cy, layer) {
            // A pad never downgrades to track/via copper of its own net:
            // rip-up must keep the pad as an obstacle. Likewise a via
            // never downgrades to a track when a later edge of the same
            // net runs through its cell — the via copper is still there,
            // and other nets must keep via-sized clearance from it.
            if let Some(c) = &mut self.occ[i] {
                if c.net_id == net_id {
                    c.half_um = c.half_um.max(half);
                    if !(c.kind == Occ::Pad || (c.kind == Occ::Via && kind == Occ::Track)) {
                        c.kind = kind;
                    }
                    let (h, k) = (c.half_um, c.kind);
                    self.stamp(cx, cy, layer, net_id, h, k);
                    return;
                }
            }
            self.occ[i] = Some(Cell { net_id, kind, half_um: half });
            self.stamp(cx, cy, layer, net_id, half, kind);
            if kind == Occ::Pad {
                // No real geometry given: the copper is one grid cell.
                let pad = PlacedPad {
                    number: String::new(),
                    center: self.to_point(cx, cy),
                    size: (self.grid_um, self.grid_um),
                    through_hole: false,
                    shape: eda_model::footprint::PadShape::Rect,
                };
                self.pads_exact.push((net_id, layer, pad.clone()));
                self.stamp_pad_exact(net_id, layer, &pad);
            }
        }
    }

    /// True if the cell holds pad copper of `net` on `layer`.
    pub fn is_pad_of(&self, cx: i64, cy: i64, layer: u8, net: &str) -> bool {
        let Some(net_id) = self.net_id_ro(net) else { return false };
        self.idx(cx, cy, layer).and_then(|i| self.occ[i].as_ref()).map(|c| c.kind == Occ::Pad && c.net_id == net_id).unwrap_or(false)
    }

    /// True if pad copper lies within the via-in-pad radius of the cell.
    #[inline]
    pub fn via_near_pad(&self, cx: i64, cy: i64, layer: u8) -> bool {
        self.idx(cx, cy, layer).map(|i| self.via_near_pad[i]).unwrap_or(true)
    }

    /// Hard-block (or unblock) a cell for every net, without clearance
    /// dilation. Out-of-range cells are ignored.
    pub fn set_blocked(&mut self, cx: i64, cy: i64, layer: u8, blocked: bool) {
        if let Some(i) = self.idx(cx, cy, layer) {
            self.blocked[i] = blocked;
        }
    }

    #[inline]
    pub fn is_blocked(&self, cx: i64, cy: i64, layer: u8) -> bool {
        self.is_blocked_as(cx, cy, layer, Occ::Track)
    }

    /// Kind-aware version of [`Grid::is_blocked`]: a via checks the (wider)
    /// via soft keep-out, a track the (narrower) track one.
    #[inline]
    pub fn is_blocked_as(&self, cx: i64, cy: i64, layer: u8, me: Occ) -> bool {
        self.idx(cx, cy, layer)
            .map(|i| {
                self.blocked[i]
                    || if me == Occ::Via { self.soft_via_active && self.soft_via[i] } else { self.soft_active && self.soft[i] }
            })
            .unwrap_or(true)
    }

    /// Mark a cell as a soft keep-out for track copper (see `soft_active`).
    pub fn set_soft(&mut self, cx: i64, cy: i64, layer: u8) {
        if let Some(i) = self.idx(cx, cy, layer) {
            self.soft[i] = true;
        }
    }

    /// Mark a cell as a soft keep-out for via copper (see `soft_active`).
    pub fn set_soft_via(&mut self, cx: i64, cy: i64, layer: u8) {
        if let Some(i) = self.idx(cx, cy, layer) {
            self.soft_via[i] = true;
        }
    }

    /// Add `p` to the per-step cost of entering a cell (saturating).
    pub fn add_penalty(&mut self, cx: i64, cy: i64, layer: u8, p: u8) {
        if let Some(i) = self.idx(cx, cy, layer) {
            self.penalty[i] = self.penalty[i].saturating_add(p);
        }
    }

    #[inline]
    pub fn penalty(&self, cx: i64, cy: i64, layer: u8) -> i64 {
        self.idx(cx, cy, layer).map(|i| self.penalty[i] as i64 + self.hist[i] as i64).unwrap_or(0)
    }

    /// True if any pad (any net, own included) occupies a cell within
    /// Chebyshev radius `r` of (cx, cy) on `layer`. Used to keep vias out
    /// of pad copper: via-in-pad is a fab/assembly defect even on the
    /// via's own net.
    pub fn pad_within(&self, cx: i64, cy: i64, layer: u8, r: i64) -> bool {
        for dx in -r..=r {
            for dy in -r..=r {
                if let Some(i) = self.idx(cx + dx, cy + dy, layer) {
                    if matches!(&self.occ[i], Some(c) if c.kind == Occ::Pad) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Cells whose radius-`r` (Chebyshev) neighbourhood is pad-free are
    /// legal via sites; `r` is the via copper half-extent plus half a
    /// cell (the pad edge lies up to half a cell past its outermost cell
    /// centre), in cells.
    pub fn via_pad_radius_cells(&self) -> i64 {
        (self.via_half_um + self.grid_um / 2 + self.grid_um - 1) / self.grid_um
    }

    /// Debug: why a cell is impassable for a track of `net`: ` ` outside,
    /// `B` hard-blocked, `S` soft (refdes) keep-out, `E` board-edge
    /// clearance, `N` foreign copper within separation, `.` passable.
    pub fn why_blocked(&self, cx: i64, cy: i64, layer: u8, net: &str) -> char {
        let Some(i) = self.idx(cx, cy, layer) else { return ' ' };
        if !self.in_outline(cx, cy) {
            return ' ';
        }
        if self.blocked[i] {
            return 'B';
        }
        let net_id = self.net_id_ro(net).unwrap_or(EMPTY);
        let own_pad = self.occ[i].as_ref().map(|c| c.kind == Occ::Pad && c.net_id == net_id).unwrap_or(false);
        if !own_pad && self.edge_dist[(cy * self.cells_x + cx) as usize] < self.tuning.edge_clearance_um + self.half_extent(Occ::Track) {
            return 'E';
        }
        let near = self.near_track[0][i];
        if !(near == EMPTY || near == net_id) {
            return 'N';
        }
        if self.soft[i] {
            return 'S';
        }
        '.'
    }

    /// Debug: everything the grid knows about one cell for `net`: the
    /// why-blocked verdict, the separation summaries, and every registered
    /// pad within 1.5 mm with its exact distance to the cell centre.
    pub fn probe(&self, cx: i64, cy: i64, layer: u8, net: &str) -> String {
        let mut out = format!("probe ({cx},{cy},{layer}) {:?} net={net}: why={} ", self.to_point(cx, cy), self.why_blocked(cx, cy, layer, net));
        if let Some(i) = self.idx(cx, cy, layer) {
            let name = |id: u32| match id {
                EMPTY => "EMPTY".to_string(),
                MULTI => "MULTI".to_string(),
                x => self.net_names.get(x as usize).cloned().unwrap_or_else(|| format!("#{x}")),
            };
            out += &format!("near_track={} near_via={} occ={:?}\n", name(self.near_track[0][i]), name(self.near_via[i]), self.occ[i].as_ref().map(|c| (name(c.net_id), c.kind, c.half_um)));
        }
        let p = self.to_point(cx, cy);
        for (nid, l, pad) in &self.pads_exact {
            if *l != layer {
                continue;
            }
            let d = pad.rect_distance(p);
            if d < 1500.0 {
                out += &format!("  pad net={} {:?} center={:?} size={:?} dist={d:.0}\n", self.net_names.get(*nid as usize).cloned().unwrap_or_default(), pad.number, pad.center, pad.size);
            }
        }
        out
    }

    /// Debug: `why_blocked` map around a cell.
    pub fn dump_why(&self, cx: i64, cy: i64, layer: u8, net: &str, r: i64) -> String {
        let mut out = String::new();
        for dy in -r..=r {
            for dx in -r..=r {
                out.push(if dx == 0 && dy == 0 { '@' } else { self.why_blocked(cx + dx, cy + dy, layer, net) });
            }
            out.push('\n');
        }
        out
    }

    /// Debug: one character per cell around (cx, cy) on `layer`:
    /// `.` free, `#` other-net pad, `=` other-net track, `o` other-net via,
    /// lowercase for own-net copper, ` ` outside the outline.
    pub fn dump_around(&self, cx: i64, cy: i64, layer: u8, net: &str, r: i64) -> String {
        let own = self.net_id_ro(net);
        let mut out = String::new();
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (cx + dx, cy + dy);
                let ch = if !self.in_outline(x, y) {
                    ' '
                } else if self.is_blocked(x, y, layer) {
                    'x'
                } else {
                    match self.idx(x, y, layer).and_then(|i| self.occ[i].as_ref()) {
                        None => '.',
                        Some(c) => {
                            let mine = Some(c.net_id) == own;
                            match (c.kind, mine) {
                                (Occ::Pad, true) => 'p',
                                (Occ::Pad, false) => '#',
                                (Occ::Track, true) => 't',
                                (Occ::Track, false) => '=',
                                (Occ::Via, true) => 'v',
                                (Occ::Via, false) => 'o',
                            }
                        }
                    }
                };
                out.push(if dx == 0 && dy == 0 { '@' } else { ch });
            }
            out.push('\n');
        }
        out
    }

    /// Every cell currently holding routed copper (track or via) of `net`.
    pub fn routed_cells_of(&self, net: &str) -> Vec<(i64, i64, u8)> {
        let Some(net_id) = self.net_id_ro(net) else { return Vec::new() };
        let mut out = Vec::new();
        for (i, c) in self.occ.iter().enumerate() {
            if let Some(c) = c {
                if c.net_id == net_id && c.kind != Occ::Pad {
                    let layer = (i % self.num_layers) as u8;
                    let cell = (i / self.num_layers) as i64;
                    out.push((cell % self.cells_x, cell / self.cells_x, layer));
                }
            }
        }
        out
    }

    pub fn clear_net(&mut self, net: &str) {
        let Some(net_id) = self.net_id_ro(net) else { return };
        for c in self.occ.iter_mut() {
            if let Some(cell) = c {
                if cell.net_id == net_id {
                    if cell.kind == Occ::Pad {
                        // The pad survives rip-up, but the ripped track
                        // that ran over it does not: drop the laid copper
                        // record, or the inflation would outlive the
                        // copper that caused it and permanently
                        // over-block the neighbourhood.
                        cell.half_um = 0;
                    } else {
                        *c = None;
                    }
                }
            }
        }
        self.rebuild_near();
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
    /// kind `a` and copper whose recorded half-extent is `b_half_um`, on
    /// different nets.
    #[inline]
    /// Public wrapper of `min_sep_cells_half` for the negotiated router's
    /// claim stamping.
    pub fn sep_cells(&self, a: Occ, b_half_um: Um) -> i64 {
        self.min_sep_cells_half(a, b_half_um)
    }

    fn min_sep_cells_half(&self, a: Occ, b_half_um: Um) -> i64 {
        let um = self.clearance_um + self.half_extent(a) + b_half_um;
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
        self.passable_as_id(cx, cy, layer, self.net_id_of(net), me)
    }

    /// The net's grid id, or `EMPTY` if the grid has never seen the name.
    /// Resolve this once per search and use [`Grid::passable_as_id`]: the
    /// string form hashes the net name on every cell test, and A* tests
    /// ~10 cells per node expansion.
    pub fn net_id_of(&self, net: &str) -> u32 {
        self.net_ids.get(net).copied().unwrap_or(EMPTY)
    }

    /// [`Grid::passable_as`] with the net already resolved to an id.
    pub fn passable_as_id(&self, cx: i64, cy: i64, layer: u8, net_id: u32, me: Occ) -> bool {
        if !self.in_outline(cx, cy) || self.is_blocked_as(cx, cy, layer, me) {
            return false;
        }
        // Copper-to-edge clearance (KiCad's 0.5 mm default), except on
        // the net's own pad copper: a pad legitimately placed at the
        // board edge must stay reachable.
        let own_pad = self.idx(cx, cy, layer).and_then(|i| self.occ[i].as_ref()).map(|c| c.kind == Occ::Pad && c.net_id == net_id).unwrap_or(false);
        if !own_pad && self.edge_dist[(cy * self.cells_x + cx) as usize] < self.tuning.edge_clearance_um + self.half_extent(me) {
            return false;
        }
        // Summary lookup (see `near_track`): equivalent to scanning the
        // clearance radius for foreign copper closer than the kind-aware
        // minimum separation, which is what `stamp` precomputed.
        let Some(i) = self.idx(cx, cy, layer) else { return false };
        let near = if me == Occ::Via { self.near_via[i] } else { self.near_track[self.bucket_of(net_id)][i] };
        let fast = near == EMPTY || near == net_id;
        if self.check_near {
            let slow = self.passable_scan(cx, cy, layer, net_id, me);
            if fast != slow {
                let name = self.net_ids.iter().find(|(_, &v)| v == net_id).map(|(k, _)| k.as_str()).unwrap_or("?");
                panic!("near-summary mismatch at ({cx},{cy},{layer}) net={name} me={me:?}: fast={fast} slow={slow} near={near}\n{}", self.dump_around(cx, cy, layer, name, 4));
            }
        }
        fast
    }

    /// Reference implementation of the separation test: scan the
    /// clearance radius for foreign copper closer than the kind-aware
    /// minimum separation. Only used to cross-check the summaries.
    fn passable_scan(&self, cx: i64, cy: i64, layer: u8, net_id: u32, me: Occ) -> bool {
        let r = self.clearance_cells;
        for dx in -r..=r {
            for dy in -r..=r {
                if let Some(i) = self.idx(cx + dx, cy + dy, layer) {
                    if let Some(c) = &self.occ[i] {
                        if c.net_id != net_id && !(c.kind == Occ::Pad && c.half_um == 0) {
                            let d = dx.abs().max(dy.abs());
                            if d < self.min_sep_cells_half(me, c.half_um) {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        let need = (self.clearance_um + self.half_extent(me)) as f64;
        let p = self.to_point(cx, cy);
        for (nid, l, pad) in &self.pads_exact {
            if *l == layer && *nid != net_id && pad.rect_distance(p) < need {
                return false;
            }
        }
        true
    }

    /// Nets whose routed copper (track/via, any layer) lies within
    /// Chebyshev radius `r` of any of `cells` on that cell's layer: the
    /// fence around a pocket the search could not leave.
    pub fn nets_bordering(&self, cells: &[(i64, i64, u8)], r: i64) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        for &(cx, cy, _l) in cells {
            // Every layer, not just the pocket's own: when a pocket's only
            // way out is a via, the copper sealing it is on the *other*
            // layer (l2 BOOT0: the pocket had via sites in preflight, all
            // taken by bottom-side tracks at routing time, none ever
            // ripped up because the fence only looked at the top).
            for l in 0..self.num_layers as u8 {
            for dx in -r..=r {
                for dy in -r..=r {
                    if let Some(i) = self.idx(cx + dx, cy + dy, l) {
                        if let Some(c) = &self.occ[i] {
                            if c.kind != Occ::Pad {
                                set.insert(self.net_names[c.net_id as usize].clone());
                            }
                        }
                    }
                }
            }
        }
        }
        set
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
