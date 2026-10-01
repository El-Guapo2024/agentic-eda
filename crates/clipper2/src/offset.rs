//! A faithful port of `Clipper2Lib/include/clipper2/clipper.offset.h` and
//! `Clipper2Lib/src/clipper.offset.cpp` (`ClipperOffset`), used by KiCad's
//! `SHAPE_POLY_SET::inflate2`/`inflateLine2` for zone-outline inflate/deflate
//! (with every join/end type and KiCad's `CORNER_STRATEGY` mapping -- see
//! `shape_poly_set.cpp` lines ~924-1140).
//!
//! `DeltaCallback64` (variable-width offsetting) is not ported: nothing in
//! `shape_poly_set.cpp` ever calls `ClipperOffset::SetDeltaCallback`, so
//! every `if (deltaCallback64_) {...}` branch in the original is dead code
//! for KiCad's zone filler and is omitted here (with `group_delta` simply a
//! constant for the whole group offset, as it always is in practice).
//!
//! ## On `z` and the lack of a `z`-carrying `PointD`
//!
//! As in `engine.rs`, KiCad always builds Clipper2 with `USINGZ`, so the
//! real `Point64`/`PointD` both carry a `z` field. This port's `PointD`
//! (see `core.rs`) deliberately does not, because tracing every
//! `#ifdef USINGZ` branch in `clipper.offset.cpp` shows `z` is *never*
//! computed from intermediate `PointD` arithmetic -- `IntersectPoint`
//! (used by `DoSquare`) always produces a fresh point with `z` reset to 0
//! regardless of its inputs, and every join function then either leaves
//! that 0 in place or explicitly overwrites it with one concrete source
//! vertex's `z`:
//! - `DoBevel` never touches `z` at all (not even under `USINGZ`), so its
//!   output points get `z == 0`, always -- this looks like an upstream
//!   oversight, but faithful means faithful, bugs included.
//! - `DoMiter`/`DoRound`/`DoSquare` all end up using exactly
//!   `path[j].z` (the vertex currently being offset) for every point they
//!   emit -- `DoSquare`'s `pt.z = ptQ.z` included, since `ptQ` is seeded
//!   from `path[j]` and only ever translated/reflected, operations that
//!   the real `PointD` would carry `z` through unchanged.
//! - `GetPerpendic`/`GetPerpendic` (not `*D`) and the plain "push `path[j]`
//!   itself" case in `OffsetPoint`'s concave branch all take a concrete
//!   `Point64` and preserve its real `z` directly -- no special-casing
//!   needed since this port's `get_perpendic` takes a real (z-carrying)
//!   `Point64` to begin with.
//!
//! So every function below takes the die-faithful `z` to attach as an
//! explicit parameter (always either `0` or a specific vertex's `.z`),
//! rather than threading a `z`-carrying `PointD` through arithmetic that
//! never actually uses it.

use crate::core::*;
use crate::engine::{ClipType, Clipper64, PolyTree64};

const DEFAULT_ARC_TOLERANCE: f64 = 0.25;
const FLOATING_POINT_TOLERANCE: f64 = 1e-12;

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

/// `IsClosedPath`: unused in upstream `clipper.offset.cpp` itself (no call
/// site there either) -- ported for completeness/fidelity.
#[allow(dead_code)]
#[inline]
fn is_closed_path(et: EndType) -> bool {
    et == EndType::Polygon || et == EndType::Joined
}

#[inline]
fn toggle_bool_if(val: bool, condition: bool) -> bool {
    if condition {
        !val
    } else {
        val
    }
}

fn get_multi_bounds(paths: &[Path64]) -> Vec<Rect64> {
    let mut rec_list = Vec::with_capacity(paths.len());
    for path in paths {
        if path.is_empty() {
            rec_list.push(Rect64::invalid());
            continue;
        }
        let (x, y) = (path[0].x, path[0].y);
        let mut r = Rect64::new(x, y, x, y);
        for pt in path {
            if pt.y > r.bottom {
                r.bottom = pt.y;
            } else if pt.y < r.top {
                r.top = pt.y;
            }
            if pt.x > r.right {
                r.right = pt.x;
            } else if pt.x < r.left {
                r.left = pt.x;
            }
        }
        rec_list.push(r);
    }
    rec_list
}

fn validate_bounds(rec_list: &[Rect64], delta: f64) -> bool {
    let int_delta = delta as i64;
    let big = MAX_COORD - int_delta;
    let small = MIN_COORD + int_delta;
    for r in rec_list {
        if !r.is_valid() {
            continue;
        }
        if r.left < small || r.right > big || r.top < small || r.bottom > big {
            return false;
        }
    }
    true
}

fn get_lowest_closed_path_idx(bounds_list: &[Rect64]) -> Option<usize> {
    let mut result = None;
    let mut bot_pt = Point64::new(i64::MAX, i64::MIN);
    for (i, r) in bounds_list.iter().enumerate() {
        if !r.is_valid() {
            continue;
        }
        if r.bottom > bot_pt.y || (r.bottom == bot_pt.y && r.left < bot_pt.x) {
            bot_pt = Point64::new(r.left, r.bottom);
            result = Some(i);
        }
    }
    result
}

fn get_unit_normal(pt1: Point64, pt2: Point64) -> PointD {
    if pt1 == pt2 {
        return PointD::new(0.0, 0.0);
    }
    let dx = (pt2.x - pt1.x) as f64;
    let dy = (pt2.y - pt1.y) as f64;
    let inverse_hypot = 1.0 / dx.hypot(dy);
    PointD::new(dy * inverse_hypot, -(dx * inverse_hypot))
}

#[inline]
fn almost_zero(value: f64, epsilon: f64) -> bool {
    value.abs() < epsilon
}

#[inline]
fn normalize_vector(vec: PointD) -> PointD {
    let h = vec.x.hypot(vec.y);
    if almost_zero(h, 0.001) {
        return PointD::new(0.0, 0.0);
    }
    let inv = 1.0 / h;
    PointD::new(vec.x * inv, vec.y * inv)
}

#[inline]
fn get_avg_unit_vector(vec1: PointD, vec2: PointD) -> PointD {
    normalize_vector(PointD::new(vec1.x + vec2.x, vec1.y + vec2.y))
}

/// `GetPerpendic`: offsets `pt` by `delta` along `norm`, preserving `pt.z`.
#[inline]
fn get_perpendic(pt: Point64, norm: PointD, delta: f64) -> Point64 {
    Point64::with_z((pt.x as f64 + norm.x * delta).round() as i64, (pt.y as f64 + norm.y * delta).round() as i64, pt.z)
}

/// `GetPerpendicD` (no `z` -- see the module doc comment on why that's
/// faithful: nothing downstream reads it).
#[inline]
fn get_perpendic_d(pt: Point64, norm: PointD, delta: f64) -> PointD {
    PointD::new(pt.x as f64 + norm.x * delta, pt.y as f64 + norm.y * delta)
}

#[inline]
fn translate_point(pt: PointD, dx: f64, dy: f64) -> PointD {
    PointD::new(pt.x + dx, pt.y + dy)
}

#[inline]
fn reflect_point(pt: PointD, pivot: PointD) -> PointD {
    PointD::new(pivot.x + (pivot.x - pt.x), pivot.y + (pivot.y - pt.y))
}

fn intersect_point_d(pt1a: PointD, pt1b: PointD, pt2a: PointD, pt2b: PointD) -> PointD {
    if pt1a.x == pt1b.x {
        // vertical
        if pt2a.x == pt2b.x {
            return PointD::new(0.0, 0.0);
        }
        let m2 = (pt2b.y - pt2a.y) / (pt2b.x - pt2a.x);
        let b2 = pt2a.y - m2 * pt2a.x;
        PointD::new(pt1a.x, m2 * pt1a.x + b2)
    } else if pt2a.x == pt2b.x {
        // vertical
        let m1 = (pt1b.y - pt1a.y) / (pt1b.x - pt1a.x);
        let b1 = pt1a.y - m1 * pt1a.x;
        PointD::new(pt2a.x, m1 * pt2a.x + b1)
    } else {
        let m1 = (pt1b.y - pt1a.y) / (pt1b.x - pt1a.x);
        let b1 = pt1a.y - m1 * pt1a.x;
        let m2 = (pt2b.y - pt2a.y) / (pt2b.x - pt2a.x);
        let b2 = pt2a.y - m2 * pt2a.x;
        if m1 == m2 {
            return PointD::new(0.0, 0.0);
        }
        let x = (b2 - b1) / (m1 - m2);
        PointD::new(x, m1 * x + b1)
    }
}

#[inline]
fn to_point64(p: PointD) -> Point64 {
    // `Point64(const PointD&)`: integer construction from a double point
    // rounds (see `Point::Init`'s `std::round` branch for integer `T` from
    // non-integer `T2`).
    Point64::new(p.x.round() as i64, p.y.round() as i64)
}

struct Group {
    paths_in: Paths64,
    is_hole_list: Vec<bool>,
    bounds_list: Vec<Rect64>,
    lowest_path_idx: Option<usize>,
    is_reversed: bool,
    join_type: JoinType,
    end_type: EndType,
}

impl Group {
    fn new(paths: Paths64, join_type: JoinType, end_type: EndType) -> Self {
        let is_joined = end_type == EndType::Polygon || end_type == EndType::Joined;
        let mut paths_in = paths;
        for p in paths_in.iter_mut() {
            strip_duplicates(p, is_joined);
        }
        let bounds_list = get_multi_bounds(&paths_in);

        let (is_hole_list, lowest_path_idx, is_reversed);
        if end_type == EndType::Polygon {
            let mut holes: Vec<bool> = paths_in.iter().map(|p| area(p) < 0.0).collect();
            let lpi = get_lowest_closed_path_idx(&bounds_list);
            // the lowermost path must be an outer path, so if its
            // orientation is negative, flag the whole group 'reversed'
            // (negating delta etc.) -- cheaper than reversing every path.
            let rev = matches!(lpi, Some(i) if holes[i]);
            if rev {
                for h in holes.iter_mut() {
                    *h = !*h;
                }
            }
            is_hole_list = holes;
            lowest_path_idx = lpi;
            is_reversed = rev;
        } else {
            is_hole_list = vec![false; paths_in.len()];
            lowest_path_idx = None;
            is_reversed = false;
        }

        Group { paths_in, is_hole_list, bounds_list, lowest_path_idx, is_reversed, join_type, end_type }
    }
}

pub struct ClipperOffset {
    error_code: i32,
    delta: f64,
    group_delta: f64,
    temp_lim: f64,
    steps_per_rad: f64,
    step_sin: f64,
    step_cos: f64,
    norms: PathD,
    path_out: Path64,
    solution: Paths64,
    groups: Vec<Group>,
    join_type: JoinType,
    end_type: EndType,

    miter_limit: f64,
    arc_tolerance: f64,
    preserve_collinear: bool,
    reverse_solution: bool,
}

impl ClipperOffset {
    pub fn new(miter_limit: f64, arc_tolerance: f64, preserve_collinear: bool, reverse_solution: bool) -> Self {
        ClipperOffset {
            error_code: 0,
            delta: 0.0,
            group_delta: 0.0,
            temp_lim: 0.0,
            steps_per_rad: 0.0,
            step_sin: 0.0,
            step_cos: 0.0,
            norms: Vec::new(),
            path_out: Vec::new(),
            solution: Vec::new(),
            groups: Vec::new(),
            join_type: JoinType::Bevel,
            end_type: EndType::Polygon,
            miter_limit,
            arc_tolerance,
            preserve_collinear,
            reverse_solution,
        }
    }

    pub fn error_code(&self) -> i32 {
        self.error_code
    }
    pub fn miter_limit(&self) -> f64 {
        self.miter_limit
    }
    pub fn set_miter_limit(&mut self, v: f64) {
        self.miter_limit = v;
    }
    pub fn arc_tolerance(&self) -> f64 {
        self.arc_tolerance
    }
    pub fn set_arc_tolerance(&mut self, v: f64) {
        self.arc_tolerance = v;
    }
    pub fn preserve_collinear(&self) -> bool {
        self.preserve_collinear
    }
    pub fn set_preserve_collinear(&mut self, v: bool) {
        self.preserve_collinear = v;
    }
    pub fn reverse_solution(&self) -> bool {
        self.reverse_solution
    }
    pub fn set_reverse_solution(&mut self, v: bool) {
        self.reverse_solution = v;
    }

    pub fn clear(&mut self) {
        self.groups.clear();
        self.norms.clear();
    }

    pub fn add_path(&mut self, path: &Path64, jt: JoinType, et: EndType) {
        self.add_paths(std::slice::from_ref(path), jt, et);
    }

    pub fn add_paths(&mut self, paths: &[Path64], jt: JoinType, et: EndType) {
        if paths.is_empty() {
            return;
        }
        self.groups.push(Group::new(paths.to_vec(), jt, et));
    }

    fn build_normals(&mut self, path: &[Point64]) {
        self.norms.clear();
        self.norms.reserve(path.len());
        if path.is_empty() {
            return;
        }
        for w in path.windows(2) {
            self.norms.push(get_unit_normal(w[0], w[1]));
        }
        self.norms.push(get_unit_normal(path[path.len() - 1], path[0]));
    }

    fn do_bevel(&mut self, path: &[Point64], j: usize, k: usize) {
        // Faithful quirk: the original never touches `z` here (not even
        // under `USINGZ`), so these two points always come out with `z == 0`.
        let (pt1, pt2);
        if j == k {
            let abs_delta = self.group_delta.abs();
            pt1 = PointD::new(path[j].x as f64 - abs_delta * self.norms[j].x, path[j].y as f64 - abs_delta * self.norms[j].y);
            pt2 = PointD::new(path[j].x as f64 + abs_delta * self.norms[j].x, path[j].y as f64 + abs_delta * self.norms[j].y);
        } else {
            pt1 = PointD::new(path[j].x as f64 + self.group_delta * self.norms[k].x, path[j].y as f64 + self.group_delta * self.norms[k].y);
            pt2 = PointD::new(path[j].x as f64 + self.group_delta * self.norms[j].x, path[j].y as f64 + self.group_delta * self.norms[j].y);
        }
        self.path_out.push(to_point64(pt1));
        self.path_out.push(to_point64(pt2));
    }

    fn do_square(&mut self, path: &[Point64], j: usize, k: usize) {
        let vec = if j == k {
            PointD::new(self.norms[j].y, -self.norms[j].x)
        } else {
            get_avg_unit_vector(PointD::new(-self.norms[k].y, self.norms[k].x), PointD::new(self.norms[j].y, -self.norms[j].x))
        };

        let abs_delta = self.group_delta.abs();

        // offset the original vertex delta units along the unit vector ...
        let pt_q = translate_point(PointD::from(path[j]), abs_delta * vec.x, abs_delta * vec.y);
        // get perpendicular vertices
        let pt1 = translate_point(pt_q, self.group_delta * vec.y, self.group_delta * -vec.x);
        let pt2 = translate_point(pt_q, self.group_delta * -vec.y, self.group_delta * vec.x);
        // get 2 vertices along one edge offset
        let pt3 = get_perpendic_d(path[k], self.norms[k], self.group_delta);
        let z = path[j].z; // == pt_q's conceptual z (see module doc comment)
        if j == k {
            let pt4 = PointD::new(pt3.x + vec.x * self.group_delta, pt3.y + vec.y * self.group_delta);
            let pt = intersect_point_d(pt1, pt2, pt3, pt4);
            self.path_out.push(Point64::with_z(to_point64(reflect_point(pt, pt_q)).x, to_point64(reflect_point(pt, pt_q)).y, z));
            self.path_out.push(Point64::with_z(to_point64(pt).x, to_point64(pt).y, z));
        } else {
            let pt4 = get_perpendic_d(path[j], self.norms[k], self.group_delta);
            let pt = intersect_point_d(pt1, pt2, pt3, pt4);
            self.path_out.push(Point64::with_z(to_point64(pt).x, to_point64(pt).y, z));
            self.path_out.push(Point64::with_z(to_point64(reflect_point(pt, pt_q)).x, to_point64(reflect_point(pt, pt_q)).y, z));
        }
    }

    fn do_miter(&mut self, path: &[Point64], j: usize, k: usize, cos_a: f64) {
        let q = self.group_delta / (cos_a + 1.0);
        self.path_out.push(Point64::with_z(
            (path[j].x as f64 + (self.norms[k].x + self.norms[j].x) * q).round() as i64,
            (path[j].y as f64 + (self.norms[k].y + self.norms[j].y) * q).round() as i64,
            path[j].z,
        ));
    }

    fn do_round(&mut self, path: &[Point64], j: usize, k: usize, angle: f64) {
        let pt = path[j];
        let mut offset_vec = PointD::new(self.norms[k].x * self.group_delta, self.norms[k].y * self.group_delta);

        if j == k {
            offset_vec.negate();
        }
        self.path_out.push(Point64::with_z((pt.x as f64 + offset_vec.x).round() as i64, (pt.y as f64 + offset_vec.y).round() as i64, pt.z));
        let steps = (self.steps_per_rad * angle.abs()).ceil() as i32; // #448, #456
        for _ in 1..steps {
            offset_vec = PointD::new(offset_vec.x * self.step_cos - self.step_sin * offset_vec.y, offset_vec.x * self.step_sin + offset_vec.y * self.step_cos);
            self.path_out.push(Point64::with_z((pt.x as f64 + offset_vec.x).round() as i64, (pt.y as f64 + offset_vec.y).round() as i64, pt.z));
        }
        self.path_out.push(get_perpendic(path[j], self.norms[j], self.group_delta));
    }

    /// `OffsetPoint`. The original's `Group&` parameter is dropped here: it
    /// exists solely to read `group.is_reversed` inside the
    /// `deltaCallback64_` branch, and `DeltaCallback64` isn't ported (see
    /// the module doc comment) since KiCad never uses it.
    fn offset_point(&mut self, path: &[Point64], j: usize, k: usize) {
        // Let A = change in angle where edges join: A==0 no change (flat),
        // A==PI edges 'spike'; sin(A)<0 right turning; cos(A)<0 change in
        // angle is more than 90 degrees.
        if path[j] == path[k] {
            return;
        }

        let sin_a = cross_product_vec(self.norms[j], self.norms[k]).clamp(-1.0, 1.0);
        let cos_a = dot_product_vec(self.norms[j], self.norms[k]);

        if self.group_delta.abs() <= FLOATING_POINT_TOLERANCE {
            self.path_out.push(path[j]);
            return;
        }

        if cos_a > -0.99 && (sin_a * self.group_delta < 0.0) {
            // concave; test for concavity first (#593)
            self.path_out.push(get_perpendic(path[j], self.norms[k], self.group_delta));
            // this extra point is the only (simple) way to ensure path
            // reversals are fully cleaned with the trailing clipper
            self.path_out.push(path[j]); // (#405)
            self.path_out.push(get_perpendic(path[j], self.norms[j], self.group_delta));
        } else if cos_a > 0.999 && self.join_type != JoinType::Round {
            // almost straight - less than 2.5 degrees (#424, #482, #526, #724)
            self.do_miter(path, j, k, cos_a);
        } else if self.join_type == JoinType::Miter {
            // miter unless the angle is sufficiently acute to exceed ML
            if cos_a > self.temp_lim - 1.0 {
                self.do_miter(path, j, k, cos_a);
            } else {
                self.do_square(path, j, k);
            }
        } else if self.join_type == JoinType::Round {
            self.do_round(path, j, k, sin_a.atan2(cos_a));
        } else if self.join_type == JoinType::Bevel {
            self.do_bevel(path, j, k);
        } else {
            self.do_square(path, j, k);
        }
    }

    fn offset_polygon(&mut self, path: &[Point64]) {
        self.path_out.clear();
        let n = path.len();
        let mut k = n - 1;
        for j in 0..n {
            self.offset_point(path, j, k);
            k = j;
        }
        self.solution.push(std::mem::take(&mut self.path_out));
    }

    fn offset_open_joined(&mut self, path: &[Point64]) {
        self.offset_polygon(path);
        let mut reverse_path = path.to_vec();
        reverse_path.reverse();

        // rebuild normals (`// BuildNormals(path);` in the original -- it's
        // commented out there too, replaced by this rotate+negate)
        self.norms.reverse();
        let first = self.norms[0];
        self.norms.push(first);
        self.norms.remove(0);
        for p in self.norms.iter_mut() {
            p.negate();
        }

        self.offset_polygon(&reverse_path);
    }

    fn offset_open_path(&mut self, path: &[Point64]) {
        // do the line start cap
        if self.group_delta.abs() <= FLOATING_POINT_TOLERANCE {
            self.path_out.push(path[0]);
        } else {
            match self.end_type {
                EndType::Butt => self.do_bevel(path, 0, 0),
                EndType::Round => self.do_round(path, 0, 0, PI),
                _ => self.do_square(path, 0, 0),
            }
        }

        let high_i = path.len() - 1;
        // offset the left side going forward
        let mut k = 0;
        for j in 1..high_i {
            self.offset_point(path, j, k);
            k = j;
        }

        // reverse normals
        for i in (1..=high_i).rev() {
            self.norms[i] = PointD::new(-self.norms[i - 1].x, -self.norms[i - 1].y);
        }
        self.norms[0] = self.norms[high_i];

        // do the line end cap
        if self.group_delta.abs() <= FLOATING_POINT_TOLERANCE {
            self.path_out.push(path[high_i]);
        } else {
            match self.end_type {
                EndType::Butt => self.do_bevel(path, high_i, high_i),
                EndType::Round => self.do_round(path, high_i, high_i, PI),
                _ => self.do_square(path, high_i, high_i),
            }
        }

        let mut j = high_i;
        let mut k = 0usize;
        while j > 0 {
            self.offset_point(path, j, k);
            k = j;
            j -= 1;
        }
        self.solution.push(std::mem::take(&mut self.path_out));
    }

    fn do_group_offset(&mut self, group_idx: usize) {
        {
            let group = &self.groups[group_idx];
            if group.end_type == EndType::Polygon {
                // a straight 2-point path can now also be 'polygon'
                // offset, treating the ends as (180deg) joins
                if group.lowest_path_idx.is_none() {
                    self.delta = self.delta.abs();
                }
                self.group_delta = if group.is_reversed { -self.delta } else { self.delta };
            } else {
                self.group_delta = self.delta.abs();
            }
        }

        let abs_delta = self.group_delta.abs();
        if !validate_bounds(&self.groups[group_idx].bounds_list, abs_delta) {
            self.error_code |= 64; // range_error_i
            return;
        }

        self.join_type = self.groups[group_idx].join_type;
        self.end_type = self.groups[group_idx].end_type;

        if self.groups[group_idx].join_type == JoinType::Round || self.groups[group_idx].end_type == EndType::Round {
            // a sensible number of steps for 360deg at this offset: when
            // arc_tolerance_ is undefined (0), the imprecision allowed is
            // based on the offset's size.
            let arc_tol =
                if self.arc_tolerance > FLOATING_POINT_TOLERANCE { abs_delta.min(self.arc_tolerance) } else { (2.0 + abs_delta).log10() * DEFAULT_ARC_TOLERANCE };

            let steps_per_360 = (PI / (1.0 - arc_tol / abs_delta).acos()).min(abs_delta * PI);
            self.step_sin = (2.0 * PI / steps_per_360).sin();
            self.step_cos = (2.0 * PI / steps_per_360).cos();
            if self.group_delta < 0.0 {
                self.step_sin = -self.step_sin;
            }
            self.steps_per_rad = steps_per_360 / (2.0 * PI);
        }

        let n = self.groups[group_idx].paths_in.len();
        for pi in 0..n {
            let path_rect = self.groups[group_idx].bounds_list[pi];
            if !path_rect.is_valid() {
                continue;
            }
            let path_len = self.groups[group_idx].paths_in[pi].len();
            self.path_out.clear();

            if path_len == 1 {
                // single point: build a circle or square
                if self.group_delta < 1.0 {
                    continue;
                }
                let pt = self.groups[group_idx].paths_in[pi][0];
                let mut path_out;
                if self.groups[group_idx].join_type == JoinType::Round {
                    let radius = abs_delta;
                    let steps = (self.steps_per_rad * 2.0 * PI).ceil() as i32; // #617
                    path_out = ellipse(pt, radius, radius, steps);
                } else {
                    let d = abs_delta.ceil() as i64;
                    path_out = Rect64::new(pt.x - d, pt.y - d, pt.x + d, pt.y + d).as_path();
                }
                for p in path_out.iter_mut() {
                    p.z = pt.z;
                }
                self.solution.push(path_out);
                continue;
            }

            let is_hole = self.groups[group_idx].is_hole_list[pi];
            let is_reversed = self.groups[group_idx].is_reversed;
            // when shrinking outer paths (or holes), make sure they can
            // shrink this far (#593, #715)
            if (self.group_delta > 0.0) == toggle_bool_if(is_hole, is_reversed) && path_rect.width().min(path_rect.height()) as f64 <= -self.group_delta * 2.0
            {
                continue;
            }

            if path_len == 2 && self.groups[group_idx].end_type == EndType::Joined {
                self.end_type = if self.groups[group_idx].join_type == JoinType::Round { EndType::Round } else { EndType::Square };
            }

            let path_in = self.groups[group_idx].paths_in[pi].clone();
            self.build_normals(&path_in);
            match self.end_type {
                EndType::Polygon => self.offset_polygon(&path_in),
                EndType::Joined => self.offset_open_joined(&path_in),
                _ => self.offset_open_path(&path_in),
            }
        }
    }

    fn calc_solution_capacity(&self) -> usize {
        self.groups.iter().map(|g| if g.end_type == EndType::Joined { g.paths_in.len() * 2 } else { g.paths_in.len() }).sum()
    }

    fn check_reverse_orientation(&self) -> bool {
        // nb: assumes consistency of orientation between groups
        for g in &self.groups {
            if g.end_type == EndType::Polygon {
                return g.is_reversed;
            }
        }
        false
    }

    fn execute_internal(&mut self, delta: f64) {
        self.error_code = 0;
        self.solution.clear();
        if self.groups.is_empty() {
            return;
        }
        self.solution.reserve(self.calc_solution_capacity());

        if delta.abs() < 0.5 {
            // offset is insignificant
            for group in &self.groups {
                self.solution.extend(group.paths_in.iter().cloned());
            }
            return;
        }

        self.temp_lim = if self.miter_limit <= 1.0 { 2.0 } else { 2.0 / (self.miter_limit * self.miter_limit) };

        self.delta = delta;
        for gi in 0..self.groups.len() {
            self.do_group_offset(gi);
            if self.error_code != 0 {
                self.solution.clear();
                break;
            }
        }
    }

    pub fn execute(&mut self, delta: f64, paths: &mut Paths64) {
        paths.clear();
        self.execute_internal(delta);
        if self.solution.is_empty() {
            return;
        }

        let paths_reversed = self.check_reverse_orientation();
        // clean up self-intersections ...
        let mut c = Clipper64::new();
        c.set_preserve_collinear(false);
        // the solution should retain the orientation of the input
        c.set_reverse_solution(self.reverse_solution != paths_reversed);
        c.add_subject(&self.solution);
        let mut open_dummy = Vec::new();
        if paths_reversed {
            c.execute(ClipType::Union, FillRule::Negative, paths, &mut open_dummy);
        } else {
            c.execute(ClipType::Union, FillRule::Positive, paths, &mut open_dummy);
        }
    }

    pub fn execute_tree(&mut self, delta: f64, polytree: &mut PolyTree64) {
        polytree.clear();
        self.execute_internal(delta);
        if self.solution.is_empty() {
            return;
        }

        let paths_reversed = self.check_reverse_orientation();
        let mut c = Clipper64::new();
        c.set_preserve_collinear(false);
        c.set_reverse_solution(self.reverse_solution != paths_reversed);
        c.add_subject(&self.solution);

        let mut open_paths = Vec::new();
        if paths_reversed {
            c.execute_tree(ClipType::Union, FillRule::Negative, polytree, &mut open_paths);
        } else {
            c.execute_tree(ClipType::Union, FillRule::Positive, polytree, &mut open_paths);
        }
    }
}

/// `Ellipse` (as used by `DoGroupOffset`'s single-point round-join case).
fn ellipse(center: Point64, radius_x: f64, radius_y: f64, steps_in: i32) -> Path64 {
    if radius_x <= 0.0 {
        return Vec::new();
    }
    let radius_y = if radius_y <= 0.0 { radius_x } else { radius_y };
    let steps = if steps_in <= 2 { (PI * ((radius_x + radius_y) / 2.0).sqrt()) as i32 } else { steps_in };

    let si = (2.0 * PI / steps as f64).sin();
    let co = (2.0 * PI / steps as f64).cos();
    let (mut dx, mut dy) = (co, si);
    let mut result = Vec::with_capacity(steps as usize);
    result.push(Point64::new((center.x as f64 + radius_x).round() as i64, center.y));
    for _ in 1..steps {
        result.push(Point64::new((center.x as f64 + radius_x * dx).round() as i64, (center.y as f64 + radius_y * dy).round() as i64));
        let x = dx * co - dy * si;
        dy = dy * co + dx * si;
        dx = x;
    }
    result
}
