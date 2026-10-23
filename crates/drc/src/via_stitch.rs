//! Where a via stitching puts its vias: `PCB_VIA_STITCH::buildPlacementCells` and the parts of `PCB_VIA_STITCH::Update` that come before it (the grid's anchor taken from
//! a via that was dragged off it), of KiCad 25ec2335 (`pcbnew/generators/pcb_via_stitch.cpp`; the 8303b2ad source the rest of the port reads predates via stitching), with
//! `POISSON_DISK::ToroidalUnitTile` (`libs/kimath/src/geometry/poisson_disk.cpp`), `SampleGuardEnvelope`, and Boost's `mt19937`, `uniform_real_distribution` and
//! `uniform_int_distribution` the two of them draw from.
//!
//! The region a via may go is where the stitch net's copper fills overlap on two of the via's layers or more, inside the outline, shrunk by half a via, less the obstacles:
//! every other item of copper on the via's layers grown by half the via and the clearance the rules give between it and a via of the stitch net (`DRC_ENGINE::EvalRules`,
//! [`crate::constraints::RuleEngine`]), every hole grown by half the drill and the hole-to-hole clearance (or the hole clearance, for another net), a rule area that keeps
//! vias out, and the fill of another net's zone near it outside that zone's own outline. A plain or staggered grid then samples the region at the pitch from the anchor; the
//! Poisson layout tiles a baked Poisson pattern; a guard walks the clearance envelope of the guarded net's tracks.
//!
//! Everything is in nanometres, as in KiCad and the design. Where this differs from KiCad: a pad whose copper is on a layer only when something is
//! connected there (`UNCONNECTED_LAYER_MODE`'s `IfConnected`) counts as flashed; text on copper is no obstacle; zone fills are the ones kicad-cli made last (the canvas's).

use std::collections::{BTreeSet, HashMap};

use eda_clipper2::Point64;
use eda_model::ir::{Design, Point, Nm};
use eda_model::via_stitch::{StitchLayout, StitchMode, ViaStitch};
use eda_model::ConstraintModel;
use eda_shape_poly_set::basic_shapes::{circle_polygon, oval_polygon, ErrorLoc};
use eda_shape_poly_set::{CornerStrategy, ShapePolySet};

use crate::constraints::CType;
use crate::kimath::Shape;

/// `pcbIUScale.mmToIU( 0.005 )`: the error the polygons are made to.
const POLY_APPROX_ERROR: i64 = 5_000;

/// `POISSON_TILE_PITCHES`: a Poisson tile is this many pitches wide.
pub const POISSON_TILE_PITCHES: i64 = 8;
/// `POISSON_TILE_SEED`: "This is an arbitrary value but must never be changed (it'll break the patterns used in designs)."
pub const POISSON_TILE_SEED: u32 = 0x542c_21fc;

fn p64(p: Point) -> Point64 {
    Point64::new(p.x, p.y)
}

/// The fills of the board's zones, nanometres, by `(zone id, layer)`: what kicad-cli made last (`ZONE::GetFill( layer )`).
pub type ZoneFillsNm = HashMap<(String, String), ShapePolySet>;

/// What the regeneration found: the stitching with its defaults filled in (`InitializeDefaults`) and its grid anchor as `Update` left it, and where its vias go, each with
/// the key that identifies it -- a grid cell `(column, row)` for the plain and staggered grids, the position itself for the Poisson layout and a guard (`cellForPosition`).
#[derive(Debug, Clone, PartialEq)]
pub struct StitchPlan {
    pub stitch: ViaStitch,
    pub vias: Vec<((i64, i64), Point)>,
}

// ------------------------------------------------------------------------------------------------------------------------- grid cells

/// `cellForPosition`: the cell of a board position (nanometres) on the stitching's grid, or the position itself where the layout has no grid.
pub fn cell_for_position(v: &ViaStitch, pitch: i64, pos: (i64, i64)) -> (i64, i64) {
    if !v.uses_grid_cells() {
        return pos;
    }
    let pitch = if pitch > 0 { pitch } else { 1 };
    // `roundDiv`: half away from zero, in integers.
    let round_div = |a: i64, b: i64| if a >= 0 { (a + b / 2) / b } else { -((-a + b / 2) / b) };
    let (ox, oy) = (v.origin_offset.x, v.origin_offset.y);
    let row = round_div(pos.1 - oy, pitch);
    let shift = if v.layout == StitchLayout::Staggered && (row % 2 + 2) % 2 != 0 { pitch / 2 } else { 0 };
    let col = round_div(pos.0 - ox - shift, pitch);
    (col, row)
}

/// `positionForCell`: the board position (nanometres) of a cell, the odd rows of a staggered grid shifted by half a pitch.
pub fn position_for_cell(v: &ViaStitch, pitch: i64, cell: (i64, i64)) -> (i64, i64) {
    if !v.uses_grid_cells() {
        return cell;
    }
    let shift = if v.layout == StitchLayout::Staggered && (cell.1 % 2 + 2) % 2 != 0 { pitch / 2 } else { 0 };
    (cell.0 * pitch + v.origin_offset.x + shift, cell.1 * pitch + v.origin_offset.y)
}

/// `Update`'s "Try to detect origin offset change due to moved via": on a grid the vias were made on (`childrenAreOnThisGrid`), the first member via off its cell defines
/// the new anchor -- the half-pitch shift of a staggered odd row taken out first. `members` are the member vias' positions in nanometres, in the generator's order.
pub fn detect_origin_offset(v: &ViaStitch, pitch: i64, members: &[(i64, i64)]) -> Option<Point> {
    if !v.uses_grid_cells() || pitch <= 0 {
        return None;
    }
    for &p in members {
        let expected = position_for_cell(v, pitch, cell_for_position(v, pitch, p));
        if expected != p {
            let offset_y = ((p.1 % pitch) + pitch) % pitch;
            let row = (p.1 - offset_y) / pitch;
            let shift = if v.layout == StitchLayout::Staggered && (row % 2 + 2) % 2 != 0 { pitch / 2 } else { 0 };
            let offset_x = (((p.0 - shift) % pitch) + pitch) % pitch;
            return Some(Point { x: offset_x, y: offset_y });
        }
    }
    None
}

// ------------------------------------------------------------------------------------------------------------------------- random numbers

/// `boost::random::mt19937`: the 32-bit Mersenne twister (MT19937), seeded as `seed( value )` seeds it.
pub struct Mt19937 {
    mt: [u32; 624],
    index: usize,
}

impl Mt19937 {
    pub fn new(seed: u32) -> Mt19937 {
        let mut mt = [0u32; 624];
        mt[0] = seed;
        for i in 1..624 {
            mt[i] = 1_812_433_253u32.wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_add(i as u32);
        }
        Mt19937 { mt, index: 624 }
    }

    fn twist(&mut self) {
        const UPPER: u32 = 0x8000_0000;
        const LOWER: u32 = 0x7fff_ffff;
        for i in 0..624 {
            let y = (self.mt[i] & UPPER) | (self.mt[(i + 1) % 624] & LOWER);
            let mut v = self.mt[(i + 397) % 624] ^ (y >> 1);
            if y & 1 != 0 {
                v ^= 0x9908_b0df;
            }
            self.mt[i] = v;
        }
        self.index = 0;
    }

    pub fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            self.twist();
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// `uniform_real_distribution<double>( 0, 1 )`: `generate_uniform_real` for an integral engine, `(eng() - min) / (max - min + 1)`, drawn again in the (impossible) case it is 1.
    pub fn uniform01(&mut self) -> f64 {
        loop {
            let r = self.next_u32() as f64 / 4_294_967_296.0;
            if r < 1.0 {
                return r;
            }
        }
    }

    /// `uniform_int_distribution<size_t>( 0, max )`: `generate_uniform_int` with the engine's range wider than the target's -- buckets of `brange / (range + 1)` (one more when
    /// the remainder is the range), drawing again above the range; a range of 0 draws nothing.
    pub fn uniform_index(&mut self, max: usize) -> usize {
        if max == 0 {
            return 0;
        }
        let brange = u32::MAX;
        let range = max as u32;
        let mut bucket = brange / (range + 1);
        if brange % (range + 1) == range {
            bucket += 1;
        }
        loop {
            let r = self.next_u32() / bucket;
            if r <= range {
                return r as usize;
            }
        }
    }
}

/// `POISSON_DISK::ToroidalUnitTile( aMinDist, aSeed )`: Bridson's Poisson-disk sampling of the unit square wrapped into a torus, so the tile repeats without a seam.
pub fn toroidal_unit_tile(min_dist: f64, seed: u32) -> Vec<(f64, f64)> {
    let mut samples: Vec<(f64, f64)> = Vec::new();
    if min_dist <= 0.0 || min_dist > 0.5 {
        return samples;
    }
    let grid_n = 3.max((2f64.sqrt() / min_dist).ceil() as i64);
    let cell_size = 1.0 / grid_n as f64;
    let mut grid = vec![-1i64; (grid_n * grid_n) as usize];
    let mut active: Vec<i64> = Vec::new();
    let cell_of = |p: (f64, f64)| -> (i64, i64) {
        let gx = (p.0 / cell_size).floor() as i64;
        let gy = (p.1 / cell_size).floor() as i64;
        (((gx % grid_n) + grid_n) % grid_n, ((gy % grid_n) + grid_n) % grid_n)
    };
    let tor_dist_sq = |mut dx: f64, mut dy: f64| -> f64 {
        if dx > 0.5 {
            dx -= 1.0;
        }
        if dx < -0.5 {
            dx += 1.0;
        }
        if dy > 0.5 {
            dy -= 1.0;
        }
        if dy < -0.5 {
            dy += 1.0;
        }
        dx * dx + dy * dy
    };
    let mut rng = Mt19937::new(seed);
    let x = rng.uniform01();
    let y = rng.uniform01();
    let p = (x, y);
    let (gx, gy) = cell_of(p);
    samples.push(p);
    grid[(gy * grid_n + gx) as usize] = 0;
    active.push(0);
    const K: usize = 30;
    while !active.is_empty() {
        let idx = rng.uniform_index(active.len() - 1);
        let base = samples[active[idx] as usize];
        let mut accepted = false;
        for _ in 0..K {
            let theta = rng.uniform01() * 2.0 * std::f64::consts::PI;
            let rho = min_dist + rng.uniform01() * min_dist;
            let mut cand = (base.0 + rho * theta.cos(), base.1 + rho * theta.sin());
            cand.0 -= cand.0.floor();
            cand.1 -= cand.1.floor();
            let (cx, cy) = cell_of(cand);
            let mut too_close = false;
            'scan: for dy in -2..=2 {
                let ny = ((cy + dy) % grid_n + grid_n) % grid_n;
                for dx in -2..=2 {
                    let nx = ((cx + dx) % grid_n + grid_n) % grid_n;
                    let n = grid[(ny * grid_n + nx) as usize];
                    if n < 0 {
                        continue;
                    }
                    let s = samples[n as usize];
                    if tor_dist_sq(s.0 - cand.0, s.1 - cand.1) < min_dist * min_dist {
                        too_close = true;
                        break 'scan;
                    }
                }
            }
            if too_close {
                continue;
            }
            samples.push(cand);
            grid[(cy * grid_n + cx) as usize] = samples.len() as i64 - 1;
            active.push(samples.len() as i64 - 1);
            accepted = true;
            break;
        }
        if !accepted {
            let last = *active.last().expect("not empty");
            active[idx] = last;
            active.pop();
        }
    }
    samples
}

/// `bakedPoissonTile`: the one tile every Poisson stitching repeats.
pub fn baked_poisson_tile() -> &'static [(f64, f64)] {
    static TILE: std::sync::OnceLock<Vec<(f64, f64)>> = std::sync::OnceLock::new();
    TILE.get_or_init(|| toroidal_unit_tile(1.0 / POISSON_TILE_PITCHES as f64, POISSON_TILE_SEED))
}

// ------------------------------------------------------------------------------------------------------------------------- geometry

fn rect_set(x0: i64, y0: i64, x1: i64, y1: i64) -> ShapePolySet {
    ShapePolySet::from_outline(vec![Point64::new(x0, y0), Point64::new(x1, y0), Point64::new(x1, y1), Point64::new(x0, y1)])
}

/// `PAD::FlashLayer` of a non-plated hole on copper: a round pad with a round drill as wide as the pad, or an oval one with a slot as long and as wide, has no copper -- only
/// its hole is an obstacle.
fn npth_without_copper(pad: &crate::board::DrcPad) -> bool {
    use eda_model::footprint::{PadKind, PadShape};
    if pad.kind != PadKind::NonPlatedHole {
        return false;
    }
    match (pad.shape, pad.drill_round, pad.drill_slot) {
        (PadShape::Circle, Some(d), None) => d >= pad.size.0,
        // The slot is held turned with the footprint, the pad's size not: their long and short sides compared.
        (PadShape::Oval, _, Some((w, h))) => w.max(h) >= pad.size.0.max(pad.size.1) && w.min(h) >= pad.size.0.min(pad.size.1),
        _ => false,
    }
}

/// `PCB_TEXT::TransformShapeToPolygon` with a clearance: `buildBoundingHull` -- the glyph strokes (each `r` wide either side) turned back by the draw rotation `rot_deg`
/// about `at`, their box grown by `margin` (nanometres; square corners), its corners turned forward again -- added to `out` when it is `near` the query.
fn add_text_hull(out: &mut ShapePolySet, segs: &[crate::kimath::Seg], r: Nm, at: Point, rot_deg: f64, margin: i64, near: &dyn Fn((i64, i64, i64, i64)) -> bool) {
    if segs.is_empty() {
        return;
    }
    let (sin, cos) = rot_deg.to_radians().sin_cos();
    // `RotatePoint( p, centre, angle )`, y down: x' = x cos + y sin, y' = -x sin + y cos, relative to the centre.
    let turn = |p: (f64, f64), s: f64| (p.0 * cos + p.1 * s, -p.0 * s + p.1 * cos);
    let (ax, ay) = (at.x as f64, at.y as f64);
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in segs.iter().flat_map(|s| [s.a, s.b]) {
        let q = turn((p.x as f64 - ax, p.y as f64 - ay), -sin);
        (x0, y0, x1, y1) = (x0.min(q.0), y0.min(q.1), x1.max(q.0), y1.max(q.1));
    }
    let grow = (r + margin) as f64;
    let corners = [(x0 - grow, y0 - grow), (x1 + grow, y0 - grow), (x1 + grow, y1 + grow), (x0 - grow, y1 + grow)];
    let pts: Vec<Point64> = corners.iter().map(|&c| turn(c, sin)).map(|(x, y)| Point64::new((x + ax).round() as i64, (y + ay).round() as i64)).collect();
    let (bx0, by0) = (pts.iter().map(|p| p.x).min().unwrap_or(0), pts.iter().map(|p| p.y).min().unwrap_or(0));
    let (bx1, by1) = (pts.iter().map(|p| p.x).max().unwrap_or(0), pts.iter().map(|p| p.y).max().unwrap_or(0));
    if near((bx0, by0, bx1, by1)) {
        out.add_outline(pts);
    }
}

fn ring_set(pts: &[Point]) -> ShapePolySet {
    ShapePolySet::from_outline(pts.iter().map(|p| p64(*p)).collect())
}

fn bbox_of(set: &ShapePolySet) -> Option<(i64, i64, i64, i64)> {
    let mut b: Option<(i64, i64, i64, i64)> = None;
    for poly in &set.polys {
        for p in poly.first().into_iter().flatten() {
            b = Some(match b {
                None => (p.x, p.y, p.x, p.y),
                Some((a, c, d, e)) => (a.min(p.x), c.min(p.y), d.max(p.x), e.max(p.y)),
            });
        }
    }
    b
}

fn boxes_overlap(a: (i64, i64, i64, i64), b: (i64, i64, i64, i64)) -> bool {
    a.0 <= b.2 && b.0 <= a.2 && a.1 <= b.3 && b.1 <= a.3
}

/// `TransformShapeToPolygon( aBuffer, aLayer, aClearance, aError, ERROR_OUTSIDE )` of an item's copper: the shape grown by `margin` (nanometres) into `out`.
fn shape_to_polygon(out: &mut ShapePolySet, s: &Shape, margin: i64) {
    match s {
        Shape::Circle { c, r } => {
            out.add_outline(circle_polygon(p64(*c), (*r) + margin, POLY_APPROX_ERROR, ErrorLoc::Outside, 0));
        }
        Shape::Stadium { a, b, r } => {
            out.add_outline(oval_polygon(p64(*a), p64(*b), 2 * ((*r) + margin), POLY_APPROX_ERROR, ErrorLoc::Outside, 0));
        }
        Shape::Rect { x0, y0, x1, y1 } => {
            let mut r = rect_set(*x0, *y0, *x1, *y1);
            if margin > 0 {
                r.inflate(margin, CornerStrategy::RoundAllCorners, POLY_APPROX_ERROR as i32, false);
            }
            out.polys.extend(r.polys);
        }
        Shape::RoundRect { x0, y0, x1, y1, r } => {
            let (rr, x0, y0, x1, y1) = ((*r), (*x0), (*y0), (*x1), (*y1));
            let mut core = rect_set(x0 + rr, y0 + rr, x1 - rr, y1 - rr);
            core.inflate(rr + margin, CornerStrategy::RoundAllCorners, POLY_APPROX_ERROR as i32, false);
            out.polys.extend(core.polys);
        }
        Shape::Polygon { pts } => {
            let mut p = ring_set(pts);
            if margin > 0 {
                p.inflate(margin, CornerStrategy::RoundAllCorners, POLY_APPROX_ERROR as i32, false);
            }
            out.polys.extend(p.polys);
        }
        Shape::Strokes { segs, r } => {
            for seg in segs {
                out.add_outline(oval_polygon(p64(seg.a), p64(seg.b), 2 * ((*r) + margin), POLY_APPROX_ERROR, ErrorLoc::Outside, 0));
            }
        }
        Shape::Compound { parts } => {
            for part in parts {
                shape_to_polygon(out, part, margin);
            }
        }
    }
}

/// `SHAPE_POLY_SET::Collide( SEG )`: whether the segment touches the set -- an end inside it, or a crossing of an edge.
fn set_collides_seg(set: &ShapePolySet, a: Point64, b: Point64) -> bool {
    if set.contains_point(a) || set.contains_point(b) {
        return true;
    }
    let cross = |o: Point64, p: Point64, q: Point64| -> i128 { (p.x - o.x) as i128 * (q.y - o.y) as i128 - (p.y - o.y) as i128 * (q.x - o.x) as i128 };
    let on = |p: Point64, q: Point64, r: Point64| q.x.min(p.x) <= r.x && r.x <= q.x.max(p.x) && q.y.min(p.y) <= r.y && r.y <= q.y.max(p.y);
    let intersects = |p1: Point64, p2: Point64, p3: Point64, p4: Point64| {
        let (d1, d2, d3, d4) = (cross(p3, p4, p1), cross(p3, p4, p2), cross(p1, p2, p3), cross(p1, p2, p4));
        if ((d1 > 0 && d2 < 0) || (d1 < 0 && d2 > 0)) && ((d3 > 0 && d4 < 0) || (d3 < 0 && d4 > 0)) {
            return true;
        }
        (d1 == 0 && on(p3, p4, p1)) || (d2 == 0 && on(p3, p4, p2)) || (d3 == 0 && on(p1, p2, p3)) || (d4 == 0 && on(p1, p2, p4))
    };
    for poly in &set.polys {
        for ring in poly {
            let n = ring.len();
            for i in 0..n {
                if intersects(a, b, ring[i], ring[(i + 1) % n]) {
                    return true;
                }
            }
        }
    }
    false
}

/// `SampleGuardEnvelope`: points along the outlines of the envelope every `pitch` of arc length (from half a pitch in), each kept when `valid` and not within 0.7 pitch of a
/// point already kept -- unless the guarded copper lies between the two (a via facing it across a track is not a double).
pub fn sample_guard_envelope(envelope: &ShapePolySet, guarded: &ShapePolySet, pitch: i64, valid: &dyn Fn(Point64) -> bool) -> Vec<Point64> {
    let mut accepted: Vec<Point64> = Vec::new();
    if pitch <= 0 {
        return accepted;
    }
    let min = (pitch as f64 * 0.7) as i64;
    let min_dist_sq = min as i128 * min as i128;
    for poly in &envelope.polys {
        let Some(chain) = poly.first() else { continue };
        let mut cursor = 0.0f64;
        let mut next_sample = pitch as f64 / 2.0;
        let n = chain.len();
        // `SegmentCount()` of a closed chain: one more than its points less one.
        for i in 0..n {
            let (a, b) = (chain[i], chain[(i + 1) % n]);
            let seg_len = ((b.x - a.x) as f64).hypot((b.y - a.y) as f64);
            if seg_len < 1.0 {
                continue;
            }
            while next_sample <= cursor + seg_len {
                let t = (next_sample - cursor) / seg_len;
                let pt = Point64::new((a.x as f64 + t * (b.x - a.x) as f64).round() as i64, (a.y as f64 + t * (b.y - a.y) as f64).round() as i64);
                next_sample += pitch as f64;
                if valid(pt) {
                    let far_enough = accepted.iter().all(|o| {
                        let (dx, dy) = ((o.x - pt.x) as i128, (o.y - pt.y) as i128);
                        dx * dx + dy * dy >= min_dist_sq || set_collides_seg(guarded, pt, *o)
                    });
                    if far_enough {
                        accepted.push(pt);
                    }
                }
            }
            cursor += seg_len;
        }
    }
    accepted
}

// ------------------------------------------------------------------------------------------------------------------------- the plan

/// What the planner needs of the board besides the stitching: the board's current via size and drill (`GetCurrentViaSize`, `GetCurrentViaDrill`), the fills of its zones.
pub struct PlanInputs<'a> {
    pub design: &'a Design,
    pub model: &'a ConstraintModel,
    pub fills: &'a ZoneFillsNm,
    pub current_via_size_nm: Nm,
    pub current_via_drill_nm: Nm,
}

/// `InitializeDefaults`, then `Update` as far as the positions of the vias: the anchor taken from a dragged via, and `buildPlacementCells` less the exclusions.
pub fn plan(inputs: &PlanInputs<'_>, stitch: &ViaStitch) -> StitchPlan {
    use eda_model::via_stitch::{default_pitch, default_via_drill, default_via_size};
    let mut v = stitch.clone();
    // `InitializeDefaults`
    if v.via.size_nm <= 0 {
        v.via.size_nm = default_via_size(inputs.current_via_size_nm);
    }
    if v.via.drill_nm <= 0 {
        v.via.drill_nm = default_via_drill(inputs.current_via_drill_nm);
    }
    if v.pitch_nm <= 0 {
        v.pitch_nm = default_pitch(v.via.size_nm);
    }
    let pitch = v.pitch_nm;
    // `childrenAreOnThisGrid = m_childGridConfig == currentConfig`: the vias were made on the layout, mode and pitch the board's stitching of this id has (the settings of
    // its last `Update`; a Properties edit has not reached the board yet). On a new grid every via is off it, and the first one is no dragged via.
    let effective_pitch = |s: &ViaStitch| -> Nm {
        if s.pitch_nm > 0 {
            s.pitch_nm
        } else {
            let size = if s.via.size_nm > 0 { s.via.size_nm } else { default_via_size(inputs.current_via_size_nm) };
            default_pitch(size)
        }
    };
    let on_this_grid = inputs
        .design
        .drawings
        .iter()
        .flat_map(|d| d.board_items.via_stitches.iter())
        .find(|s| s.id == v.id)
        .is_some_and(|s| (s.layout, s.mode, effective_pitch(s)) == (v.layout, v.mode, v.pitch_nm));
    // The anchor a moved via defines (the vias are on the grid of this layout and pitch: their cells are what the stitching's own settings make them).
    let members: Vec<(i64, i64)> = if !on_this_grid {
        Vec::new()
    } else {
        inputs
        .design
        .routing
        .iter()
        .flat_map(|rt| v.member_ids.iter().filter_map(move |id| rt.vias.iter().find(|x| &x.id == id)))
        .map(|x| (x.at.x, x.at.y))
        .collect()
    };
    if let Some(offset) = detect_origin_offset(&v, pitch, &members) {
        v.origin_offset = offset;
    }
    if v.outline.len() < 3 {
        return StitchPlan { stitch: v, vias: Vec::new() };
    }
    let cells = placement_cells(inputs, &v);
    // `isExcluded`: a grid cell the user took out, a position (near one, for a guard: within a quarter pitch).
    let guard_exclude_sq = (pitch / 4) as i128 * (pitch / 4) as i128;
    let excluded = |cell: (i64, i64)| -> bool {
        if v.mode == StitchMode::Guard {
            return v.excluded_positions.iter().any(|e| {
                let (dx, dy) = ((e.x - cell.0) as i128, (e.y - cell.1) as i128);
                dx * dx + dy * dy < guard_exclude_sq
            });
        }
        if v.uses_grid_cells() {
            v.excluded_cells.contains(&cell)
        } else {
            // The design keeps nanometres: a position is matched as the via was put there, to the nanometre.
            v.excluded_positions.iter().any(|e| (e.x, e.y) == (cell.0, cell.1))
        }
    };
    let vias = cells
        .into_iter()
        .filter(|c| !excluded(*c))
        .map(|c| {
            let (x, y) = position_for_cell(&v, pitch, c);
            // The design keeps nanometres: the position rounded to one -- and where the layout has no grid, the position is the via's key, as rounded.
            let at = Point { x: x, y: y };
            (if v.uses_grid_cells() { c } else { (at.x, at.y) }, at)
        })
        .collect();
    StitchPlan { stitch: v, vias }
}

/// `buildPlacementCells`: the cells (or positions, nanometres) where a via of the stitching fits.
pub fn placement_cells(inputs: &PlanInputs<'_>, v: &ViaStitch) -> BTreeSet<(i64, i64)> {
    let mut cells = BTreeSet::new();
    let design = inputs.design;
    let rules = &inputs.model.board;
    let pitch = if v.pitch_nm > 0 { v.pitch_nm } else { eda_model::via_stitch::default_pitch(v.via.size_nm) };
    let via_size = v.via.size_nm;
    let via_drill = v.via.drill_nm;
    // `GetNetCode() == 0`: a stitching of no net, or of a net the board does not have, makes nothing.
    if v.net.is_empty() || !inputs.model.nets.iter().any(|n| n.name == v.net) {
        return cells;
    }
    let outline = ring_set(&v.outline);
    let Some(stitch_box) = bbox_of(&outline) else { return cells };
    let stitch_clip = rect_set(stitch_box.0, stitch_box.1, stitch_box.2, stitch_box.3);
    let zones: Vec<&eda_model::ir::Zone> = design.routing.iter().flat_map(|rt| rt.zones.iter()).collect();
    let zone_box = |z: &eda_model::ir::Zone| bbox_of(&ring_set(&z.outline));

    // The stitch net's copper on each layer, inside the outline's box (a guard takes the zones' outlines).
    let mut per_layer: HashMap<String, ShapePolySet> = HashMap::new();
    for z in &zones {
        if z.is_rule_area || z.net != v.net {
            continue;
        }
        if !zone_box(z).is_some_and(|b| boxes_overlap(b, stitch_box)) {
            continue;
        }
        for layer in z.layer_set() {
            let mut clipped = if v.mode == StitchMode::Guard {
                ring_set(&z.outline)
            } else {
                match inputs.fills.get(&(z.id.clone(), layer.to_string())) {
                    Some(f) if !f.is_empty() => f.clone(),
                    _ => continue,
                }
            };
            clipped.boolean_intersection(&stitch_clip);
            if clipped.outline_count() == 0 {
                continue;
            }
            per_layer.entry(layer.to_string()).or_default().boolean_add(&clipped);
        }
    }

    // The via's copper layers that have stitch-net copper.
    let copper = &rules.layers;
    let span = |a: &str, b: &str| -> Vec<String> {
        let (i0, i1) = (copper.iter().position(|l| l == a), copper.iter().position(|l| l == b));
        match (i0, i1) {
            (Some(x), Some(y)) => copper[x.min(y)..=x.max(y)].to_vec(),
            _ => copper.clone(),
        }
    };
    let via_layers = span(&v.via.from_layer, &v.via.to_layer);
    let fills: Vec<&ShapePolySet> = via_layers.iter().filter_map(|l| per_layer.get(l)).filter(|f| !f.is_empty()).collect();
    if fills.len() < 2 {
        return cells;
    }
    // Where at least two of them overlap.
    let mut allowed = ShapePolySet::default();
    let mut covered = fills[0].clone();
    for (i, f) in fills.iter().enumerate().skip(1) {
        let mut overlap = (*f).clone();
        overlap.boolean_intersection(&covered);
        if !overlap.is_empty() {
            allowed.boolean_add(&overlap);
        }
        if i + 1 < fills.len() {
            covered.boolean_add(f);
        }
    }
    if allowed.outline_count() == 0 {
        return cells;
    }
    allowed.boolean_intersection(&outline);
    if allowed.outline_count() == 0 {
        return cells;
    }
    // "Require the whole via, not just its center, to land on same-net copper inside the outline."
    allowed.deflate(via_size / 2, CornerStrategy::RoundAllCorners, POLY_APPROX_ERROR as i32);
    if allowed.outline_count() == 0 {
        return cells;
    }

    // The rules, and the probe via they judge the obstacles against.
    let board = crate::board::build(design, inputs.model);
    let rb = crate::rule_board::RuleBoard::new(design, inputs.model, &board, crate::constraints::Units::Mm, rules.custom_rules_text.as_deref());
    let probe = rb.probe_via(&v.net, Point { x: 0, y: 0 }, v.via.size_nm, v.via.drill_nm, &v.via.from_layer, &v.via.to_layer);
    let eval = |t: CType, other: &std::rc::Rc<crate::rule_item::RuleItem>, layer: Option<&str>| -> i64 { rb.engine().eval_rules(t, Some(&probe), Some(other), layer, None).value.min() };
    let worst = crate::constraints::worst_case_clearance(rules);
    let query_box = (stitch_box.0 - worst - via_size, stitch_box.1 - worst - via_size, stitch_box.2 + worst + via_size, stitch_box.3 + worst + via_size);
    let query_clip = rect_set(query_box.0, query_box.1, query_box.2, query_box.3);
    let near = |b: (i64, i64, i64, i64)| boxes_overlap((b.0 - worst, b.1 - worst, b.2 + worst, b.3 + worst), query_box);
    let shape_box = |s: &Shape| {
        let (x0, y0, x1, y1) = s.bbox(0);
        (x0, y0, x1, y1)
    };

    let mut obstacles = ShapePolySet::default();
    let fp_copper = crate::fp_art::footprint_copper(design, inputs.model);
    let same_net = |net: &Option<String>| net.as_deref() == Some(v.net.as_str());
    // Holes, once each: half the drill and the hole-to-hole clearance, or half the via and the hole clearance for another net.
    let mut hole_seen: BTreeSet<String> = BTreeSet::new();
    let mut add_hole = |obstacles: &mut ShapePolySet, id: &str, hole: &Shape, net: &Option<String>, item: &std::rc::Rc<crate::rule_item::RuleItem>, layer: &str| {
        if !hole_seen.insert(id.to_string()) {
            return;
        }
        let mut margin = via_drill / 2 + eval(CType::HoleToHole, item, None);
        if !same_net(net) {
            margin = margin.max(via_size / 2 + eval(CType::HoleClearance, item, Some(layer)));
        }
        shape_to_polygon(obstacles, hole, margin);
    };
    let (first, last) = (copper.first().cloned().unwrap_or_default(), copper.last().cloned().unwrap_or_default());
    for layer in &via_layers {
        // Pads: "A copper-zone can be placed with thermal reliefs turned off ... process the PAD as a obstacle we need to clear"
        for (i, pad) in board.pads.iter().enumerate() {
            let on_layer = pad.layers.iter().any(|l| l == layer) || pad.hole.is_some();
            if !on_layer || !near(shape_box(&pad.copper)) {
                continue;
            }
            let Some(item) = rb.pad_item_at(i) else { continue };
            if let Some(hole) = &pad.hole {
                add_hole(&mut obstacles, &pad.id, hole, &pad.net, &item, layer);
            }
            let Some(copper_shape) = pad.copper_on(layer) else { continue };
            if pad.flash(layer, &first, &last) == eda_model::padstack::Flash::Never || npth_without_copper(pad) {
                continue;
            }
            let mut margin = via_size / 2 + eval(CType::Clearance, &item, Some(layer));
            if !same_net(&pad.net) {
                margin = margin.max(via_drill / 2 + eval(CType::HoleClearance, &item, Some(layer)));
            }
            shape_to_polygon(&mut obstacles, copper_shape, margin);
        }
        for (i, t) in board.tracks.iter().enumerate() {
            if t.layer != *layer || same_net(&t.net) || !near(shape_box(&t.shape())) {
                continue;
            }
            let item = rb.segment_item_at(i);
            let margin = (via_size / 2 + eval(CType::Clearance, &item, Some(layer))).max(via_drill / 2 + eval(CType::HoleClearance, &item, Some(layer)));
            shape_to_polygon(&mut obstacles, &t.shape(), margin);
        }
        for (i, via) in board.vias.iter().enumerate() {
            // "Our own children are regenerated along with us and must not block their own cells."
            if v.member_ids.contains(&via.id) || !span(&via.from_layer, &via.to_layer).contains(layer) || !near(shape_box(&via.shape())) {
                continue;
            }
            let item = rb.via_item_at(i);
            add_hole(&mut obstacles, &via.id, &via.hole(), &via.net, &item, layer);
            let mut margin = via_size / 2 + eval(CType::Clearance, &item, Some(layer));
            if !same_net(&via.net) {
                margin = margin.max(via_drill / 2 + eval(CType::HoleClearance, &item, Some(layer)));
            }
            shape_to_polygon(&mut obstacles, &via.shape(), margin);
        }
        // A footprint's own graphics and texts on copper (`PCB_SHAPE`, `PCB_TEXT` of no net), and the board's texts on copper: `TransformShapeToPolygon` of each, a text's
        // bounding hull (`PCB_TEXT::buildBoundingHull`) grown by the margin.
        for piece in &fp_copper {
            match piece {
                crate::fp_art::FpCopper::Shape(s) if s.layer() == layer => {
                    let item = rb.shape_item_of(s);
                    let margin = (via_size / 2 + eval(CType::Clearance, &item, Some(layer))).max(via_drill / 2 + eval(CType::HoleClearance, &item, Some(layer)));
                    for part in crate::board::effective_shapes(s) {
                        if near(shape_box(&part)) {
                            shape_to_polygon(&mut obstacles, &part, margin);
                        }
                    }
                }
                crate::fp_art::FpCopper::Text(t) if t.layer == *layer => {
                    let item = rb.text_item_of("fp_text", t);
                    let margin = (via_size / 2 + eval(CType::Clearance, &item, Some(layer))).max(via_drill / 2 + eval(CType::HoleClearance, &item, Some(layer)));
                    if let Some(Shape::Strokes { segs, r }) = crate::board::footprint_text_shape(t) {
                        add_text_hull(&mut obstacles, &segs, r, t.at, crate::board::footprint_text_draw_rotation(t), margin, &near);
                    }
                }
                _ => {}
            }
        }
        for t in design.drawings.iter().flat_map(|d| d.copper_texts.iter()).filter(|t| t.layer == *layer) {
            let item = rb.text_item_of("copper_text", t);
            let margin = (via_size / 2 + eval(CType::Clearance, &item, Some(layer))).max(via_drill / 2 + eval(CType::HoleClearance, &item, Some(layer)));
            if let Some(Shape::Strokes { segs, r }) = crate::board::footprint_text_shape(t) {
                add_text_hull(&mut obstacles, &segs, r, t.at, crate::board::footprint_text_draw_rotation(t), margin, &near);
            }
        }
        for t in design.drawings.iter().flat_map(|d| d.texts.iter()).filter(|t| t.layer == *layer) {
            let item = rb.board_text_item_of(t);
            let margin = (via_size / 2 + eval(CType::Clearance, &item, Some(layer))).max(via_drill / 2 + eval(CType::HoleClearance, &item, Some(layer)));
            // Laid out as the DRC lays out a plain text (`crate::board::build`): square glyphs, vertically centred.
            let size = (t.size_nm, t.size_nm);
            let pen = crate::stroke_font::effective_pen_width(t.stroke_width, size, false);
            let halign = match t.justify {
                eda_model::ir::TextJustify::Left => -1,
                eda_model::ir::TextJustify::Center => 0,
                eda_model::ir::TextJustify::Right => 1,
            };
            let rot = (t.angle as f64 / 1000.0).rem_euclid(360.0);
            let segs = crate::stroke_font::kicad_text_segments(&crate::stroke_font::TextLayout { text: &t.content, pos: t.at, size, thickness: t.stroke_width, angle_deg: rot, mirror: t.mirror, halign, valign: 0 });
            add_text_hull(&mut obstacles, &segs, pen / 2, t.at, rot, margin, &near);
        }
        // Graphics on copper (`PCB_SHAPE`), another net's or none.
        for s in design.drawings.iter().flat_map(|d| d.shapes.iter()) {
            if s.layer() != layer {
                continue;
            }
            let net = design.drawings.as_ref().and_then(|d| d.board_items.net_of(s.id())).map(str::to_string);
            if same_net(&net) {
                continue;
            }
            let item = rb.shape_item_of(s);
            let margin = (via_size / 2 + eval(CType::Clearance, &item, Some(layer))).max(via_drill / 2 + eval(CType::HoleClearance, &item, Some(layer)));
            for part in crate::board::effective_shapes(s) {
                if near(shape_box(&part)) {
                    shape_to_polygon(&mut obstacles, &part, margin);
                }
            }
        }
    }
    obstacles.simplify();

    for z in &zones {
        if !zone_box(z).is_some_and(|b| boxes_overlap(b, query_box)) {
            continue;
        }
        if z.is_rule_area {
            // "Honor via keepouts (any net)."
            if !z.keepout_vias || !z.layer_set().iter().any(|l| via_layers.iter().any(|x| x == l)) {
                continue;
            }
            let mut area = ring_set(&z.outline);
            area.boolean_intersection(&query_clip);
            if !area.is_empty() {
                area.inflate(via_size / 2, CornerStrategy::RoundAllCorners, POLY_APPROX_ERROR as i32, false);
                obstacles.boolean_add(&area);
            }
            continue;
        }
        if z.net == v.net {
            continue;
        }
        // "A different-net zone only blocks near its fill outside its own outline."
        let item = rb.zone_item_of(z);
        for layer in z.layer_set().into_iter().filter(|l| via_layers.iter().any(|x| x == l)) {
            let Some(fill) = inputs.fills.get(&(z.id.clone(), layer.to_string())).filter(|f| !f.is_empty()) else { continue };
            let mut near_fill = fill.clone();
            near_fill.boolean_intersection(&query_clip);
            if near_fill.is_empty() {
                continue;
            }
            let margin = (via_size / 2 + eval(CType::Clearance, &item, Some(layer))).max(via_drill / 2 + eval(CType::HoleClearance, &item, Some(layer)));
            near_fill.inflate(margin, CornerStrategy::RoundAllCorners, POLY_APPROX_ERROR as i32, false);
            let mut interior = ring_set(&z.outline);
            interior.boolean_intersection(&query_clip);
            near_fill.boolean_subtract(&interior);
            if !near_fill.is_empty() {
                obstacles.boolean_add(&near_fill);
            }
        }
    }

    allowed.boolean_subtract(&obstacles);
    if allowed.outline_count() == 0 {
        return cells;
    }
    let Some(bbox) = bbox_of(&allowed) else { return cells };
    let valid = |p: Point64| allowed.contains_point(p);

    if v.mode == StitchMode::Guard {
        // Walk the clearance envelope of each guarded-net track.
        if v.guarded_net.is_empty() || !inputs.model.nets.iter().any(|n| n.name == v.guarded_net) {
            return cells;
        }
        let mut envelope = ShapePolySet::default();
        let mut guarded = ShapePolySet::default();
        let safety = POLY_APPROX_ERROR + 2_000;
        for (i, t) in board.tracks.iter().enumerate() {
            if t.net.as_deref() != Some(v.guarded_net.as_str()) || !boxes_overlap(shape_box(&t.shape()), stitch_box) {
                continue;
            }
            let item = rb.segment_item_at(i);
            let margin = via_size / 2 + eval(CType::Clearance, &item, Some(&t.layer)) + safety;
            let mut env = ShapePolySet::default();
            shape_to_polygon(&mut env, &t.shape(), margin);
            envelope.boolean_add(&env);
            shape_to_polygon(&mut guarded, &t.shape(), 0);
        }
        for (i, via) in board.vias.iter().enumerate() {
            if via.net.as_deref() != Some(v.guarded_net.as_str()) || !boxes_overlap(shape_box(&via.shape()), stitch_box) {
                continue;
            }
            let item = rb.via_item_at(i);
            // "arbitrary copper layer for clearance evaluation"
            let margin = via_size / 2 + eval(CType::Clearance, &item, Some(&first)) + safety;
            let mut env = ShapePolySet::default();
            shape_to_polygon(&mut env, &via.shape(), margin);
            envelope.boolean_add(&env);
            shape_to_polygon(&mut guarded, &via.shape(), 0);
        }
        envelope.simplify();
        guarded.simplify();
        for pt in sample_guard_envelope(&envelope, &guarded, pitch, &valid) {
            cells.insert((pt.x, pt.y));
        }
        return cells;
    }

    let contains_box = |p: Point64| p.x >= bbox.0 && p.x <= bbox.2 && p.y >= bbox.1 && p.y <= bbox.3;
    if v.layout == StitchLayout::Poisson {
        let tile = baked_poisson_tile();
        let tile_size = 1.max(pitch * POISSON_TILE_PITCHES);
        let mut rng = Mt19937::new(v.seed);
        let offset_x = rng.uniform01() * tile_size as f64;
        let offset_y = rng.uniform01() * tile_size as f64;
        let ts = tile_size as f64;
        let first_x = ((bbox.0 as f64 - offset_x) / ts).floor() as i64;
        let first_y = ((bbox.1 as f64 - offset_y) / ts).floor() as i64;
        let last_x = ((bbox.2 as f64 - offset_x) / ts).floor() as i64;
        let last_y = ((bbox.3 as f64 - offset_y) / ts).floor() as i64;
        for ty in first_y..=last_y {
            for tx in first_x..=last_x {
                for s in tile {
                    let pt = Point64::new((offset_x + (tx as f64 + s.0) * ts).round() as i64, (offset_y + (ty as f64 + s.1) * ts).round() as i64);
                    if contains_box(pt) && valid(pt) {
                        cells.insert((pt.x, pt.y));
                    }
                }
            }
        }
        return cells;
    }

    // The plain and staggered grids, anchored to (0, 0) plus the offset.
    let (ox, oy) = (v.origin_offset.x, v.origin_offset.y);
    let start_row = ((bbox.1 - oy) as f64 / pitch as f64).floor() as i64;
    let start_col = ((bbox.0 - ox) as f64 / pitch as f64).floor() as i64;
    let origin = (start_col * pitch + ox, start_row * pitch + oy);
    let mut row = start_row;
    let mut y = origin.1;
    while y <= bbox.3 {
        let x_offset = if v.layout == StitchLayout::Staggered && (row % 2 + 2) % 2 != 0 { pitch / 2 } else { 0 };
        let mut col = start_col;
        let mut x = origin.0 + x_offset;
        while x <= bbox.2 {
            if valid(Point64::new(x, y)) {
                cells.insert((col, row));
            }
            x += pitch;
            col += 1;
        }
        y += pitch;
        row += 1;
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_twister_is_mt19937() {
        // The 10000th output of a default-seeded (5489) MT19937 is 4123659995 (the C++ standard's own check).
        let mut mt = Mt19937::new(5489);
        let mut last = 0;
        for _ in 0..10_000 {
            last = mt.next_u32();
        }
        assert_eq!(last, 4_123_659_995);
    }

    #[test]
    fn the_uniform_draws_are_boosts() {
        let mut a = Mt19937::new(1);
        let mut b = Mt19937::new(1);
        assert_eq!(a.uniform01(), b.next_u32() as f64 / 4_294_967_296.0);
        // A range of 0 draws nothing.
        let mut c = Mt19937::new(7);
        assert_eq!(c.uniform_index(0), 0);
        let mut d = Mt19937::new(7);
        assert_eq!(c.next_u32(), d.next_u32());
        // A range of 2: buckets of 4294967295 / 3 (the remainder 0 is not the range).
        let mut e = Mt19937::new(9);
        let mut f = Mt19937::new(9);
        assert_eq!(e.uniform_index(2), (f.next_u32() / 1_431_655_765) as usize);
    }

    #[test]
    fn the_poisson_tile_keeps_its_distance_round_the_torus() {
        let tile = baked_poisson_tile();
        // The studio's own port (web/studio/src/kicad-port/viaStitch.ts) bakes the same tile: 42 samples, from (0.404105.., 0.559086..).
        assert_eq!(tile.len(), 42);
        assert_eq!(tile[0], (0.404105210211128, 0.5590860228985548));
        let min = 1.0 / POISSON_TILE_PITCHES as f64;
        for (i, a) in tile.iter().enumerate() {
            for b in &tile[i + 1..] {
                let wrap = |d: f64| if d > 0.5 { d - 1.0 } else if d < -0.5 { d + 1.0 } else { d };
                let (dx, dy) = (wrap(a.0 - b.0), wrap(a.1 - b.1));
                assert!(dx * dx + dy * dy >= min * min - 1e-12);
            }
        }
    }

    #[test]
    fn cells_and_positions_follow_the_anchor_and_the_stagger() {
        let mut v = ViaStitch::new(vec![]);
        v.origin_offset = Point { x: 250_000, y: 0 };
        let pitch = 1_000_000;
        assert_eq!(position_for_cell(&v, pitch, (3, 2)), (3_250_000, 2_000_000));
        assert_eq!(cell_for_position(&v, pitch, (3_250_000, 2_000_000)), (3, 2));
        v.layout = StitchLayout::Staggered;
        assert_eq!(position_for_cell(&v, pitch, (3, 1)), (3_750_000, 1_000_000), "an odd row is half a pitch over");
        assert_eq!(cell_for_position(&v, pitch, (3_750_000, 1_000_000)), (3, 1));
        // A via dragged to (10.1, 5.3) mm on a plain grid anchors it at (0.1, 0.3).
        let mut plain = ViaStitch::new(vec![]);
        plain.member_ids = vec!["a".into()];
        assert_eq!(detect_origin_offset(&plain, pitch, &[(10_100_000, 5_300_000)]), Some(Point { x: 100_000, y: 300_000 }));
        assert_eq!(detect_origin_offset(&plain, pitch, &[(10_000_000, 5_000_000)]), None, "on its cell");
    }
}

