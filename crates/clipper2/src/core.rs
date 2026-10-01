//! A faithful port of `Clipper2Lib/include/clipper2/clipper.core.h`.
//!
//! Only the pieces the 64-bit integer engine (`Clipper64`), `ClipperOffset`
//! and `RectClip64` actually need are ported: `Point64` (with KiCad's
//! `USINGZ` build flag in effect -- see `thirdparty/clipper2/CMakeLists.txt`,
//! `target_compile_definitions(clipper2 PUBLIC USINGZ)` -- so `Point64`
//! carries a `z` field the engine threads through intersections via
//! `ZCallback64`), `Rect64`, `FillRule`, `PointInPolygonResult`, and the free
//! functions (`CrossProduct`, `Area`, `PointInPolygon`, `GetIntersectPoint`,
//! ...) used throughout `clipper.engine.cpp` / `clipper.offset.cpp` /
//! `clipper.rectclip.cpp`.
//!
//! The double-precision `PointD`/`ClipperD` side of the original library is
//! not ported: KiCad's `SHAPE_POLY_SET` only ever instantiates
//! `Clipper2Lib::Clipper64` / `Clipper2Lib::PolyTree64` (see
//! `booleanOp()`/`inflate2()`/`Simplify()` in `shape_poly_set.cpp`), so there
//! is no `ClipperD` call site to port faithfully from. Where the original
//! offset code needs floating point intermediates (unit normals etc.) a
//! small local `PointD` (`x`/`y` only, no `z`) is used instead -- see the
//! doc comment on `offset.rs` for how the `z` propagation that the real
//! `PointD` would carry is preserved exactly at each call site.

/// `Point<int64_t>` from `clipper.core.h`, with the `z` field that KiCad's
/// `USINGZ` build enables. `operator==`/`operator!=` in the original only
/// ever compare `x`/`y` (see clipper.core.h lines 195-203) -- `z` rides
/// along for `ZCallback64` bookkeeping only -- so [`PartialEq`] is
/// hand-written to match rather than derived.
#[derive(Debug, Clone, Copy, Default)]
pub struct Point64 {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

impl Point64 {
    #[inline]
    pub fn new(x: i64, y: i64) -> Self {
        Point64 { x, y, z: 0 }
    }

    #[inline]
    pub fn with_z(x: i64, y: i64, z: i64) -> Self {
        Point64 { x, y, z }
    }
}

impl PartialEq for Point64 {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.x == other.x && self.y == other.y
    }
}
impl Eq for Point64 {}

impl std::ops::Neg for Point64 {
    type Output = Point64;
    #[inline]
    fn neg(self) -> Point64 {
        Point64::with_z(-self.x, -self.y, self.z)
    }
}

impl std::ops::Add for Point64 {
    type Output = Point64;
    #[inline]
    fn add(self, b: Point64) -> Point64 {
        Point64::new(self.x + b.x, self.y + b.y)
    }
}

impl std::ops::Sub for Point64 {
    type Output = Point64;
    #[inline]
    fn sub(self, b: Point64) -> Point64 {
        Point64::new(self.x - b.x, self.y - b.y)
    }
}

pub type Path64 = Vec<Point64>;
pub type Paths64 = Vec<Path64>;

/// `Point<double>`, trimmed to just what the offset code needs (no `z` --
/// see the module doc comment on why that's faithful here).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PointD {
    pub x: f64,
    pub y: f64,
}

impl PointD {
    #[inline]
    pub fn new(x: f64, y: f64) -> Self {
        PointD { x, y }
    }
    #[inline]
    pub fn negate(&mut self) {
        self.x = -self.x;
        self.y = -self.y;
    }
}

impl From<Point64> for PointD {
    #[inline]
    fn from(p: Point64) -> PointD {
        PointD { x: p.x as f64, y: p.y as f64 }
    }
}

pub type PathD = Vec<PointD>;
pub type PathsD = Vec<PathD>;

/// By far the most widely used filling rules for polygons are `EvenOdd`
/// and `NonZero`, sometimes called Alternate and Winding respectively.
/// <https://en.wikipedia.org/wiki/Nonzero-rule>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    EvenOdd,
    NonZero,
    Positive,
    Negative,
}

pub const MAX_COORD: i64 = i64::MAX >> 2;
pub const MIN_COORD: i64 = -MAX_COORD;

/// `static const double PI = 3.141592653589793238;` in `clipper.core.h` --
/// bit-for-bit the same value as `std::f64::consts::PI`, used directly to
/// satisfy `clippy::approx_constant`.
pub const PI: f64 = std::f64::consts::PI;

// Rect -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect64 {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
}

impl Default for Rect64 {
    /// `Rect(bool is_valid = true)` -- the all-zero "valid but empty" rect,
    /// matching `OutRec::bounds = {}` in `clipper.engine.h`.
    fn default() -> Self {
        Rect64 { left: 0, top: 0, right: 0, bottom: 0 }
    }
}

impl Rect64 {
    #[inline]
    pub fn new(l: i64, t: i64, r: i64, b: i64) -> Self {
        Rect64 { left: l, top: t, right: r, bottom: b }
    }

    /// `Rect(false)`: the "invalid" sentinel rect used as `invalid_rect` /
    /// `InvalidRect64` and as the starting point for `GetMultiBounds`.
    #[inline]
    pub fn invalid() -> Self {
        Rect64 { left: i64::MAX, top: i64::MAX, right: i64::MIN, bottom: i64::MIN }
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        self.left != i64::MAX
    }

    #[inline]
    pub fn width(&self) -> i64 {
        self.right - self.left
    }
    #[inline]
    pub fn height(&self) -> i64 {
        self.bottom - self.top
    }

    #[inline]
    pub fn mid_point(&self) -> Point64 {
        Point64::new((self.left + self.right) / 2, (self.top + self.bottom) / 2)
    }

    pub fn as_path(&self) -> Path64 {
        vec![
            Point64::new(self.left, self.top),
            Point64::new(self.right, self.top),
            Point64::new(self.right, self.bottom),
            Point64::new(self.left, self.bottom),
        ]
    }

    #[inline]
    pub fn contains_pt(&self, pt: Point64) -> bool {
        pt.x > self.left && pt.x < self.right && pt.y > self.top && pt.y < self.bottom
    }

    #[inline]
    pub fn contains_rect(&self, rec: &Rect64) -> bool {
        rec.left >= self.left && rec.right <= self.right && rec.top >= self.top && rec.bottom <= self.bottom
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bottom <= self.top || self.right <= self.left
    }

    #[inline]
    pub fn intersects(&self, rec: &Rect64) -> bool {
        self.left.max(rec.left) <= self.right.min(rec.right) && self.top.max(rec.top) <= self.bottom.min(rec.bottom)
    }
}

pub fn get_bounds(path: &[Point64]) -> Rect64 {
    let mut xmin = i64::MAX;
    let mut ymin = i64::MAX;
    let mut xmax = i64::MIN;
    let mut ymax = i64::MIN;
    for p in path {
        if p.x < xmin {
            xmin = p.x;
        }
        if p.x > xmax {
            xmax = p.x;
        }
        if p.y < ymin {
            ymin = p.y;
        }
        if p.y > ymax {
            ymax = p.y;
        }
    }
    Rect64::new(xmin, ymin, xmax, ymax)
}

pub fn get_bounds_paths(paths: &[Path64]) -> Rect64 {
    let mut xmin = i64::MAX;
    let mut ymin = i64::MAX;
    let mut xmax = i64::MIN;
    let mut ymax = i64::MIN;
    for path in paths {
        for p in path {
            if p.x < xmin {
                xmin = p.x;
            }
            if p.x > xmax {
                xmax = p.x;
            }
            if p.y < ymin {
                ymin = p.y;
            }
            if p.y > ymax {
                ymax = p.y;
            }
        }
    }
    Rect64::new(xmin, ymin, xmax, ymax)
}

// Miscellaneous ----------------------------------------------------------

#[inline]
pub fn sqr(v: f64) -> f64 {
    v * v
}

#[inline]
pub fn near_equal(p1: Point64, p2: Point64, max_dist_sqrd: f64) -> bool {
    sqr((p1.x - p2.x) as f64) + sqr((p1.y - p2.y) as f64) < max_dist_sqrd
}

/// `StripDuplicates(Path<T>&, bool)`.
pub fn strip_duplicates(path: &mut Path64, is_closed_path: bool) {
    path.dedup_by(|a, b| a == b);
    if is_closed_path {
        while path.len() > 1 && path.last() == path.first() {
            path.pop();
        }
    }
}

/// `CrossProduct(pt1,pt2,pt3)`: the 3-point "turn" cross product.
#[inline]
pub fn cross_product(pt1: Point64, pt2: Point64, pt3: Point64) -> f64 {
    (pt2.x - pt1.x) as f64 * (pt3.y - pt2.y) as f64 - (pt2.y - pt1.y) as f64 * (pt3.x - pt2.x) as f64
}

/// `CrossProduct(vec1,vec2)`: the 2-vector cross product (note the operand
/// order in the original: `vec1.y*vec2.x - vec2.y*vec1.x`).
#[inline]
pub fn cross_product_vec(vec1: PointD, vec2: PointD) -> f64 {
    vec1.y * vec2.x - vec2.y * vec1.x
}

/// `DotProduct(pt1,pt2,pt3)`.
#[inline]
pub fn dot_product(pt1: Point64, pt2: Point64, pt3: Point64) -> f64 {
    (pt2.x - pt1.x) as f64 * (pt3.x - pt2.x) as f64 + (pt2.y - pt1.y) as f64 * (pt3.y - pt2.y) as f64
}

/// `DotProduct(vec1,vec2)`.
#[inline]
pub fn dot_product_vec(vec1: PointD, vec2: PointD) -> f64 {
    vec1.x * vec2.x + vec1.y * vec2.y
}

#[inline]
pub fn distance_sqr(pt1: Point64, pt2: Point64) -> f64 {
    sqr((pt1.x - pt2.x) as f64) + sqr((pt1.y - pt2.y) as f64)
}

#[inline]
pub fn distance_from_line_sqrd(pt: Point64, ln1: Point64, ln2: Point64) -> f64 {
    let a = (ln1.y - ln2.y) as f64;
    let b = (ln2.x - ln1.x) as f64;
    let c0 = a * ln1.x as f64 + b * ln1.y as f64;
    let c = a * pt.x as f64 + b * pt.y as f64 - c0;
    (c * c) / (a * a + b * b)
}

/// `Area(const Path<T>&)`: the shoelace formula. The original walks the
/// path with a paired-iterator trick (processing edges two at a time,
/// starting from the wraparound edge) purely as a micro-optimisation; since
/// summation order doesn't change the result (modulo last-bit float
/// reassociation, which nothing here depends on), this computes the exact
/// same per-edge term `(y_prev + y_curr) * (x_prev - x_curr)` in plain
/// traversal order.
pub fn area(path: &[Point64]) -> f64 {
    let cnt = path.len();
    if cnt < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    let mut prev = path[cnt - 1];
    for &curr in path {
        a += (prev.y + curr.y) as f64 * (prev.x - curr.x) as f64;
        prev = curr;
    }
    a * 0.5
}

pub fn area_paths(paths: &[Path64]) -> f64 {
    paths.iter().map(|p| area(p)).sum()
}

#[inline]
pub fn is_positive(poly: &[Point64]) -> bool {
    area(poly) >= 0.0
}

/// `GetIntersectPoint`: infinite-line intersection (not segment-bounded),
/// clamped to the two endpoints of line 1 when the parametric `t` falls
/// outside `[0,1]` -- exactly as the original does (with its "??check
/// further" comments preserved as-is since that's what upstream ships).
pub fn get_intersect_point(ln1a: Point64, ln1b: Point64, ln2a: Point64, ln2b: Point64) -> Option<Point64> {
    let dx1 = (ln1b.x - ln1a.x) as f64;
    let dy1 = (ln1b.y - ln1a.y) as f64;
    let dx2 = (ln2b.x - ln2a.x) as f64;
    let dy2 = (ln2b.y - ln2a.y) as f64;

    let det = dy1 * dx2 - dy2 * dx1;
    if det == 0.0 {
        return None;
    }
    let t = ((ln1a.x - ln2a.x) as f64 * dy2 - (ln1a.y - ln2a.y) as f64 * dx2) / det;
    let ip = if t <= 0.0 {
        ln1a
    } else if t >= 1.0 {
        ln1b
    } else {
        Point64::new((ln1a.x as f64 + t * dx1) as i64, (ln1a.y as f64 + t * dy1) as i64)
    };
    Some(ip)
}

pub fn segments_intersect(seg1a: Point64, seg1b: Point64, seg2a: Point64, seg2b: Point64, inclusive: bool) -> bool {
    if inclusive {
        let res1 = cross_product(seg1a, seg2a, seg2b);
        let res2 = cross_product(seg1b, seg2a, seg2b);
        if res1 * res2 > 0.0 {
            return false;
        }
        let res3 = cross_product(seg2a, seg1a, seg1b);
        let res4 = cross_product(seg2b, seg1a, seg1b);
        if res3 * res4 > 0.0 {
            return false;
        }
        res1 != 0.0 || res2 != 0.0 || res3 != 0.0 || res4 != 0.0
    } else {
        (cross_product(seg1a, seg2a, seg2b) * cross_product(seg1b, seg2a, seg2b) < 0.0)
            && (cross_product(seg2a, seg1a, seg1b) * cross_product(seg2b, seg1a, seg1b) < 0.0)
    }
}

pub fn get_closest_point_on_segment(off_pt: Point64, seg1: Point64, seg2: Point64) -> Point64 {
    if seg1.x == seg2.x && seg1.y == seg2.y {
        return seg1;
    }
    let dx = (seg2.x - seg1.x) as f64;
    let dy = (seg2.y - seg1.y) as f64;
    let q = (((off_pt.x - seg1.x) as f64 * dx + (off_pt.y - seg1.y) as f64 * dy) / (sqr(dx) + sqr(dy))).clamp(0.0, 1.0);
    Point64::new(seg1.x + (q * dx).round() as i64, seg1.y + (q * dy).round() as i64)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointInPolygonResult {
    IsOn,
    IsInside,
    IsOutside,
}

/// `PointInPolygon`: a faithful port of the horizontal-ray-crossing test in
/// `clipper.core.h`, including its careful handling of vertices that sit
/// exactly on the test point's `y` (the "is_above"/wrap-around dance).
pub fn point_in_polygon(pt: Point64, polygon: &[Point64]) -> PointInPolygonResult {
    let n = polygon.len();
    if n < 3 {
        return PointInPolygonResult::IsOutside;
    }

    let mut val = 0i32;

    // `first` walks forward from index 0 skipping any vertex exactly on
    // pt.y; cend == n means "not a proper polygon".
    let mut first = 0usize;
    while first < n && polygon[first].y == pt.y {
        first += 1;
    }
    if first == n {
        return PointInPolygonResult::IsOutside;
    }

    let mut is_above = polygon[first].y < pt.y;
    let starting_above = is_above;
    let mut curr = first + 1; // may be == cend(), handled like C++'s wraparound below
    let mut cend = n; // the "logical end" shrinks to `first` once we wrap

    loop {
        if curr == cend {
            if cend == first || first == 0 {
                break;
            }
            cend = first;
            curr = 0;
        }

        if is_above {
            while curr != cend && polygon[curr].y < pt.y {
                curr += 1;
            }
            if curr == cend {
                continue;
            }
        } else {
            while curr != cend && polygon[curr].y > pt.y {
                curr += 1;
            }
            if curr == cend {
                continue;
            }
        }

        let prev = if curr == 0 { n - 1 } else { curr - 1 };

        if polygon[curr].y == pt.y {
            if polygon[curr].x == pt.x
                || (polygon[curr].y == polygon[prev].y && (pt.x < polygon[prev].x) != (pt.x < polygon[curr].x))
            {
                return PointInPolygonResult::IsOn;
            }
            curr += 1;
            if curr == first {
                break;
            }
            continue;
        }

        if pt.x < polygon[curr].x && pt.x < polygon[prev].x {
            // only interested in edges crossing on the left: do nothing
        } else if pt.x > polygon[prev].x && pt.x > polygon[curr].x {
            val = 1 - val;
        } else {
            let d = cross_product(polygon[prev], polygon[curr], pt);
            if d == 0.0 {
                return PointInPolygonResult::IsOn;
            }
            if (d < 0.0) == is_above {
                val = 1 - val;
            }
        }
        is_above = !is_above;
        curr += 1;
    }

    if is_above != starting_above {
        cend = n;
        if curr == cend {
            curr = 0;
        }
        let prev = if curr == 0 { cend - 1 } else { curr - 1 };
        let d = cross_product(polygon[prev], polygon[curr], pt);
        if d == 0.0 {
            return PointInPolygonResult::IsOn;
        }
        if (d < 0.0) == is_above {
            val = 1 - val;
        }
    }

    if val == 0 {
        PointInPolygonResult::IsOutside
    } else {
        PointInPolygonResult::IsInside
    }
}
