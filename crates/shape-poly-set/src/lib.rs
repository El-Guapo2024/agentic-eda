//! A faithful port of the `SHAPE_POLY_SET` operations KiCad's `ZONE_FILLER`
//! needs, from `libs/kimath/src/geometry/shape_poly_set.cpp` /
//! `include/geometry/shape_poly_set.h` (KiCad vendored snapshot, commit
//! `8303b2ad`), built on this workspace's `eda_clipper2` port (stage 1).
//!
//! ## Scope
//!
//! `SHAPE_LINE_CHAIN`'s arc support (`SHAPE_ARC`, `CLIPPER_Z_VALUE`-tagged
//! intersections) is not ported: this workspace's IR stores zone outlines
//! as plain straight-segment polygons (`Vec<Point>` -- see
//! `eda_model::ir::Zone`), so there is nothing upstream's arc bookkeeping
//! would need to track here. `LineChain` is accordingly just `Vec<Point64>`
//! (always implicitly closed -- first vertex connects back to the last,
//! matching `SHAPE_LINE_CHAIN`'s closed-chain convention), not a full
//! `SHAPE_LINE_CHAIN` port.
//!
//! Also out of scope, each a targeted fix-up for a specific degenerate
//! input shape rather than part of the core fill pipeline: `Simplify()`'s
//! `splitCollinearOutlines()` pre-pass, `splitSelfTouchingOutlines()`,
//! `SimplifyOutlines()` (KiCad's own, non-Clipper2,
//! collinear-point-removal), `HasTouchingHoles()`, and
//! `NormalizeAreaOutlines()`. `Simplify()` here is exactly upstream's
//! `booleanOp(Union, {})` half (the actual degeneracy-removal engine, via
//! Clipper2's `FixSelfIntersects`), just without the pre-pass.
//!
//! `Fracture`/`Unfracture` are the cache-friendly, index-based algorithms
//! (`fractureSingleCacheFriendly`/`unfractureSingle`) -- see `fracture.rs`.

pub mod fracture;

use eda_clipper2::{self as cl, ClipType, Clipper64, ClipperOffset, EndType, FillRule, JoinType, Path64, Paths64, Point64, PolyTree64};

/// `SHAPE_LINE_CHAIN`, trimmed to what the filler needs: a closed polyline
/// (implicitly closed -- no repeated first/last point).
pub type LineChain = Vec<Point64>;

/// `SHAPE_POLY_SET::POLYGON`: `[0]` is the outline, `[1..]` are its holes.
pub type Polygon = Vec<LineChain>;

/// `CORNER_STRATEGY` (`geometry/corner_strategy.h`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerStrategy {
    AllowAcuteCorners,
    ChamferAcuteCorners,
    RoundAcuteCorners,
    ChamferAllCorners,
    RoundAllCorners,
}

/// `GetArcToSegmentCount` (`geometry_utils.cpp`), specialized to
/// `aArcAngle == FULL_CIRCLE` (360 degrees) since that's the only angle any
/// `SHAPE_POLY_SET` call site here ever passes.
pub fn get_arc_to_segment_count(radius: i32, error_max: i32) -> i32 {
    const MIN_SEGCOUNT_FOR_CIRCLE: f64 = 8.0;
    let radius = radius.max(1) as f64;
    let error_max = error_max.max(1) as f64;
    let rel_error = error_max / radius;
    let mut arc_increment = 180.0 / std::f64::consts::PI * (1.0 - rel_error).acos() * 2.0;
    arc_increment = (360.0 / MIN_SEGCOUNT_FOR_CIRCLE).min(arc_increment);
    let seg_count = (360.0 / arc_increment).round() as i32;
    seg_count.max(2)
}

fn corner_strategy_to_join(cs: CornerStrategy) -> (JoinType, f64) {
    match cs {
        CornerStrategy::AllowAcuteCorners => (JoinType::Miter, 10.0), // allows large spikes
        CornerStrategy::ChamferAcuteCorners => (JoinType::Miter, 2.0),
        CornerStrategy::RoundAcuteCorners => (JoinType::Miter, 2.0),
        CornerStrategy::ChamferAllCorners => (JoinType::Square, 2.0),
        CornerStrategy::RoundAllCorners => (JoinType::Round, 2.0),
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShapePolySet {
    pub polys: Vec<Polygon>,
}

impl ShapePolySet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_outline(outline: LineChain) -> Self {
        ShapePolySet { polys: vec![vec![outline]] }
    }

    // ---- construction ----

    pub fn new_outline(&mut self) -> usize {
        self.polys.push(vec![Vec::new()]);
        self.polys.len() - 1
    }

    /// `AddOutline`.
    pub fn add_outline(&mut self, outline: LineChain) -> usize {
        self.polys.push(vec![outline]);
        self.polys.len() - 1
    }

    /// `AddHole`. `outline` defaults (as in the original's `aOutline = -1`)
    /// to the most recently added polygon.
    pub fn add_hole(&mut self, hole: LineChain, outline: Option<usize>) -> usize {
        let idx = outline.unwrap_or(self.polys.len() - 1);
        self.polys[idx].push(hole);
        self.polys[idx].len() - 2
    }

    /// `AddPolygon`.
    pub fn add_polygon(&mut self, poly: Polygon) -> usize {
        self.polys.push(poly);
        self.polys.len() - 1
    }

    // ---- accessors ----

    pub fn outline_count(&self) -> usize {
        self.polys.len()
    }
    pub fn hole_count(&self, outline: usize) -> usize {
        self.polys[outline].len() - 1
    }
    pub fn outline(&self, i: usize) -> &LineChain {
        &self.polys[i][0]
    }
    pub fn hole(&self, i: usize, j: usize) -> &LineChain {
        &self.polys[i][j + 1]
    }
    pub fn polygon(&self, i: usize) -> &Polygon {
        &self.polys[i]
    }

    /// `IsEmpty`.
    pub fn is_empty(&self) -> bool {
        self.polys.is_empty() || self.polys.iter().all(|p| p[0].is_empty())
    }

    /// `Area`.
    pub fn area(&self) -> f64 {
        let mut a = 0.0;
        for poly in &self.polys {
            a += cl::area(&poly[0]).abs();
            for hole in &poly[1..] {
                a -= cl::area(hole).abs();
            }
        }
        a
    }

    /// `HasHoles`.
    pub fn has_holes(&self) -> bool {
        self.polys.iter().any(|p| p.len() > 1)
    }

    pub fn remove_all_contours(&mut self) {
        self.polys.clear();
    }

    // ---- Clipper2 plumbing ----

    /// `SHAPE_LINE_CHAIN::convertToClipper2`'s orientation normalization
    /// (the arc/Z-value bookkeeping is dropped -- see the module doc
    /// comment): outlines (`required_positive == true`) must wind
    /// positive-area, holes (`false`) negative-area, matching whatever
    /// Clipper2's `NonZero` fill rule expects on the way in.
    fn convert_to_clipper2(chain: &[Point64], required_positive: bool) -> Path64 {
        let is_positive = cl::area(chain) >= 0.0;
        if is_positive != required_positive {
            let mut rev = chain.to_vec();
            rev.reverse();
            rev
        } else {
            chain.to_vec()
        }
    }

    fn to_clipper_paths(&self) -> Paths64 {
        let mut paths = Vec::new();
        for poly in &self.polys {
            for (i, chain) in poly.iter().enumerate() {
                paths.push(Self::convert_to_clipper2(chain, i == 0));
            }
        }
        paths
    }

    /// `importPolyPath`.
    fn import_poly_path(tree: &PolyTree64, node: usize, out: &mut Vec<Polygon>) {
        if tree.nodes[node].is_hole() {
            return;
        }
        let mut poly: Polygon = Vec::with_capacity(tree.nodes[node].children.len() + 1);
        poly.push(tree.nodes[node].polygon.clone());
        for &child in &tree.nodes[node].children {
            poly.push(tree.nodes[child].polygon.clone());
            for &grandchild in &tree.nodes[child].children {
                Self::import_poly_path(tree, grandchild, out);
            }
        }
        out.push(poly);
    }

    /// `importTree`.
    pub fn from_poly_tree(tree: &PolyTree64) -> Self {
        let mut polys = Vec::new();
        for &root in &tree.roots {
            Self::import_poly_path(tree, root, &mut polys);
        }
        ShapePolySet { polys }
    }

    /// `importPaths`: a positive-area path starts a new polygon (outline);
    /// a non-positive-area path is a hole of the most recently started one
    /// (dropped, matching `wxCHECK2_MSG(..., continue, ...)`, if none is
    /// open yet).
    pub fn from_paths(paths: &Paths64) -> Self {
        let mut polys: Vec<Polygon> = Vec::new();
        let mut current: Polygon = Vec::new();
        for p in paths {
            if cl::area(p) > 0.0 {
                if !current.is_empty() {
                    polys.push(std::mem::take(&mut current));
                }
                current.push(p.clone());
            } else if !current.is_empty() {
                current.push(p.clone());
            }
        }
        if !current.is_empty() {
            polys.push(current);
        }
        ShapePolySet { polys }
    }

    /// Flattens every outline and hole into one `Paths64` (e.g. for a
    /// post-`Fracture` single-outline-per-polygon dump into `filled_polygon`).
    pub fn to_paths64(&self) -> Paths64 {
        self.polys.iter().flat_map(|p| p.iter().cloned()).collect()
    }

    // ---- boolean ops ----

    fn boolean_op(&mut self, ct: ClipType, other: &ShapePolySet) {
        let a = self.clone();
        self.boolean_op_of(ct, &a, other);
    }

    fn boolean_op_of(&mut self, ct: ClipType, a: &ShapePolySet, b: &ShapePolySet) {
        let subjects = a.to_clipper_paths();
        let clips = b.to_clipper_paths();
        let mut c = Clipper64::new();
        c.add_subject(&subjects);
        c.add_clip(&clips);
        let mut tree = PolyTree64::new();
        let mut open = Vec::new();
        c.execute_tree(ct, FillRule::NonZero, &mut tree, &mut open);
        *self = Self::from_poly_tree(&tree);
    }

    pub fn boolean_add(&mut self, b: &ShapePolySet) {
        self.boolean_op(ClipType::Union, b);
    }
    pub fn boolean_subtract(&mut self, b: &ShapePolySet) {
        self.boolean_op(ClipType::Difference, b);
    }
    pub fn boolean_intersection(&mut self, b: &ShapePolySet) {
        self.boolean_op(ClipType::Intersection, b);
    }
    pub fn boolean_xor(&mut self, b: &ShapePolySet) {
        self.boolean_op(ClipType::Xor, b);
    }

    pub fn boolean_add_of(&mut self, a: &ShapePolySet, b: &ShapePolySet) {
        self.boolean_op_of(ClipType::Union, a, b);
    }
    pub fn boolean_subtract_of(&mut self, a: &ShapePolySet, b: &ShapePolySet) {
        self.boolean_op_of(ClipType::Difference, a, b);
    }
    pub fn boolean_intersection_of(&mut self, a: &ShapePolySet, b: &ShapePolySet) {
        self.boolean_op_of(ClipType::Intersection, a, b);
    }
    pub fn boolean_xor_of(&mut self, a: &ShapePolySet, b: &ShapePolySet) {
        self.boolean_op_of(ClipType::Xor, a, b);
    }

    /// `Simplify` (minus the `splitCollinearOutlines` pre-pass -- see the
    /// module doc comment): self-union, which is where Clipper2's own
    /// `FixSelfIntersects`/`CleanCollinear` do the actual degeneracy and
    /// overlap removal.
    pub fn simplify(&mut self) {
        let empty = ShapePolySet::new();
        self.boolean_op(ClipType::Union, &empty);
    }

    // ---- inflate / deflate ----

    /// `inflate2`. `arc_tolerance_factor`'s memoization table is dropped
    /// (pure micro-optimisation of `1 - cos(pi/n)`; same value either way).
    pub fn inflate2(&mut self, amount: i64, circle_seg_count: i32, corner_strategy: CornerStrategy, simplify: bool) {
        let (join_type, miter_limit) = corner_strategy_to_join(corner_strategy);
        let mut c = ClipperOffset::new(miter_limit, 0.0, false, false);

        for poly in &self.polys {
            let paths: Paths64 = poly.iter().enumerate().map(|(i, chain)| Self::convert_to_clipper2(chain, i == 0)).collect();
            c.add_paths(&paths, join_type, EndType::Polygon);
        }

        let circle_seg_count = circle_seg_count.max(6);
        let coeff = 1.0 - (std::f64::consts::PI / circle_seg_count as f64).cos();
        c.set_arc_tolerance((amount as f64).abs() * coeff);
        c.set_miter_limit(miter_limit);

        let mut tree = PolyTree64::new();
        if simplify {
            let mut paths = Vec::new();
            c.execute(amount as f64, &mut paths);
            let paths = cl::simplify_paths(&paths, (amount as f64).abs() * coeff, true);

            let mut c2 = Clipper64::new();
            c2.set_preserve_collinear(false);
            c2.set_reverse_solution(false);
            c2.add_subject(&paths);
            let mut open = Vec::new();
            c2.execute_tree(ClipType::Union, FillRule::Positive, &mut tree, &mut open);
        } else {
            c.execute_tree(amount as f64, &mut tree);
        }

        *self = Self::from_poly_tree(&tree);
    }

    /// `inflateLine2`.
    pub fn inflate_line2(&mut self, line: &LineChain, amount: i64, circle_seg_count: i32, corner_strategy: CornerStrategy, simplify: bool) {
        let (join_type, miter_limit) = corner_strategy_to_join(corner_strategy);
        let mut c = ClipperOffset::new(miter_limit, 0.0, false, false);

        let path = Self::convert_to_clipper2(line, true);
        c.add_path(&path, join_type, EndType::Butt);

        let circle_seg_count = circle_seg_count.max(6);
        let coeff = 1.0 - (std::f64::consts::PI / circle_seg_count as f64).cos();
        c.set_arc_tolerance((amount as f64).abs() * coeff);
        c.set_miter_limit(miter_limit);

        let mut tree = PolyTree64::new();
        if simplify {
            let mut paths2 = Vec::new();
            c.execute(amount as f64, &mut paths2);
            let paths2 = cl::simplify_paths(&paths2, (amount as f64).abs() * coeff, false);

            let mut c2 = Clipper64::new();
            c2.set_preserve_collinear(false);
            c2.set_reverse_solution(false);
            c2.add_subject(&paths2);
            let mut open = Vec::new();
            c2.execute_tree(ClipType::Union, FillRule::Positive, &mut tree, &mut open);
        } else {
            c.execute_tree(amount as f64, &mut tree);
        }

        *self = Self::from_poly_tree(&tree);
    }

    /// `Inflate`.
    pub fn inflate(&mut self, amount: i64, corner_strategy: CornerStrategy, max_error: i32, simplify: bool) {
        let seg_count = get_arc_to_segment_count(amount.unsigned_abs() as i32, max_error);
        self.inflate2(amount, seg_count, corner_strategy, simplify);
    }

    /// `Deflate`.
    pub fn deflate(&mut self, amount: i64, corner_strategy: CornerStrategy, max_error: i32) {
        self.inflate(-amount, corner_strategy, max_error, false);
    }

    /// `OffsetLineChain`.
    pub fn offset_line_chain(&mut self, line: &LineChain, amount: i64, corner_strategy: CornerStrategy, max_error: i32, simplify: bool) {
        let seg_count = get_arc_to_segment_count(amount.unsigned_abs() as i32, max_error);
        self.inflate_line2(line, amount, seg_count, corner_strategy, simplify);
    }

    /// `InflateWithLinkedHoles`.
    pub fn inflate_with_linked_holes(&mut self, factor: i64, corner_strategy: CornerStrategy, max_error: i32) {
        self.unfracture();
        self.inflate(factor, corner_strategy, max_error, false);
        self.fracture(true);
    }

    // ---- fracture / unfracture ----

    /// `Fracture`.
    pub fn fracture(&mut self, simplify: bool) {
        if simplify {
            self.simplify();
        }
        for poly in &mut self.polys {
            fracture::fracture_single(poly);
        }
    }

    /// `Unfracture`.
    pub fn unfracture(&mut self) {
        for poly in &mut self.polys {
            fracture::unfracture_single(poly);
        }
        self.simplify();
    }
}
