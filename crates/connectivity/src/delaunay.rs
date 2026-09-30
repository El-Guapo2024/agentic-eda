//! A Delaunay triangulation of a 2D point set, incremental hull insertion
//! with an angular hash for point location -- the "Delaunator" algorithm.
//!
//! This is a from-scratch Rust port of the algorithm (not a translation of
//! any specific source file we had on hand): [Delaunator][js] by Vladimir
//! Agafonkin (ISC-licensed, "a very fast library for Delaunay
//! triangulation of 2D points"), the same algorithm KiCad's own
//! `thirdparty/delaunator` (a C++11 port by Volodymyr Bilonenko,
//! MIT-licensed) implements and which `pcbnew/ratsnest/ratsnest_data.cpp`
//! calls to triangulate a net's anchors before running Kruskal's MST over
//! the result (see [`crate::ratsnest`]). We could not fetch either source
//! for this task, so the structure below (seed-triangle selection sorted
//! by distance from its circumcenter, a hash-accelerated walk to find the
//! hull edge a new point is visible from, and a stack-based Lawson-flip
//! `legalize` pass after every insertion) is reconstructed from how the
//! published algorithm works, using the same field/function names as the
//! reference implementations so the two stay easy to compare.
//!
//! [js]: https://github.com/mapbox/delaunator
//!
//! Credit: Delaunator, Copyright (c) 2017, Vladimir Agafonkin (ISC
//! license); delaunator-cpp, Copyright (c) 2018 Volodymyr Bilonenko (MIT
//! license). Neither source text is reproduced here -- this is an
//! independent re-implementation of the published algorithm.
//!
//! What callers need: [`Triangulation::new`] takes `[x, y, x, y, ...]`
//! point coordinates and returns `triangles` (point indices, 3 per
//! triangle, CCW) and `halfedges` (for each directed edge `e` of
//! `triangles`, the index of the opposite directed edge in the adjacent
//! triangle, or [`INVALID_INDEX`] on the hull boundary) -- exactly the two
//! arrays `RN_NET::TRIANGULATOR_STATE::Triangulate` reads off
//! `delaunator::Delaunator`.

/// Sentinel for "no such half-edge" / "empty hash slot", matching the
/// C++ port's `delaunator::INVALID_INDEX` (KiCad tests
/// `delaunator.halfedges[i] == delaunator::INVALID_INDEX`).
pub const INVALID_INDEX: usize = usize::MAX;

pub struct Triangulation {
    /// Point indices, 3 per triangle, counter-clockwise.
    pub triangles: Vec<usize>,
    /// Same length as `triangles`; halfedges[e] is the opposite half-edge
    /// of edge `e`, or `INVALID_INDEX` if `e` is on the convex hull.
    pub halfedges: Vec<usize>,
    /// The convex hull, point indices in CCW order.
    pub hull: Vec<usize>,
}

#[inline]
fn dist2(ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

/// Twice the signed area of triangle `(p, q, r)`: positive when `p -> q ->
/// r` turns left (counter-clockwise), zero when collinear, negative when
/// clockwise -- every call site below checks its sign against zero.
#[inline]
fn cross(px: f64, py: f64, qx: f64, qy: f64, rx: f64, ry: f64) -> f64 {
    (qx - px) * (ry - py) - (qy - py) * (rx - px)
}

/// A monotonic stand-in for the angle of `(dx, dy)` around the origin, in
/// `[0, 1)` -- cheaper than `atan2` and all that matters for hashing
/// points into angular buckets around the seed circumcenter.
#[inline]
fn pseudo_angle(dx: f64, dy: f64) -> f64 {
    let p = dx / (dx.abs() + dy.abs());
    (if dy > 0.0 { 3.0 - p } else { 1.0 + p }) / 4.0
}

#[inline]
fn circumradius(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let ex = cx - ax;
    let ey = cy - ay;
    let bl = dx * dx + dy * dy;
    let cl = ex * ex + ey * ey;
    let d = 0.5 / (dx * ey - dy * ex);
    if !d.is_finite() {
        return f64::INFINITY;
    }
    let x = (ey * bl - dy * cl) * d;
    let y = (dx * cl - ex * bl) * d;
    x * x + y * y
}

#[inline]
fn circumcenter(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64) -> (f64, f64) {
    let dx = bx - ax;
    let dy = by - ay;
    let ex = cx - ax;
    let ey = cy - ay;
    let bl = dx * dx + dy * dy;
    let cl = ex * ex + ey * ey;
    let d = 0.5 / (dx * ey - dy * ex);
    (ax + (ey * bl - dy * cl) * d, ay + (dx * cl - ex * bl) * d)
}

/// True when `p` lies strictly inside the circumcircle of `a, b, c`
/// (given in CCW order) -- the Delaunay legality test.
#[inline]
#[allow(clippy::too_many_arguments)]
fn in_circle(ax: f64, ay: f64, bx: f64, by: f64, cx: f64, cy: f64, px: f64, py: f64) -> bool {
    let dx = ax - px;
    let dy = ay - py;
    let ex = bx - px;
    let ey = by - py;
    let fx = cx - px;
    let fy = cy - py;

    let ap = dx * dx + dy * dy;
    let bp = ex * ex + ey * ey;
    let cp = fx * fx + fy * fy;

    dx * (ey * cp - bp * fy) - dy * (ex * cp - bp * fx) + ap * (ex * fy - ey * fx) > 0.0
}

/// Insertion-sort-cutoff quicksort of `ids` by `dists[ids[i]]`, matching
/// delaunator's own sort (a plain comparison sort; which one does not
/// matter for correctness, only for the processing order of points at
/// exactly equal distance from the seed circumcenter -- and any
/// consistent order still yields a valid triangulation).
fn sort_by_dist(ids: &mut [usize], dists: &[f64]) {
    ids.sort_by(|&a, &b| dists[a].partial_cmp(&dists[b]).unwrap_or(std::cmp::Ordering::Equal));
}

impl Triangulation {
    /// `coords` is `[x0, y0, x1, y1, ...]`. Points that exactly coincide
    /// with an earlier point (once sorted) are skipped, same as upstream
    /// delaunator -- callers with genuinely coincident anchors should
    /// de-duplicate positions themselves first (see
    /// `crate::ratsnest::Triangulator`), since a triangulation cannot
    /// place two coincident points in distinct triangle corners anyway.
    pub fn new(coords: &[f64]) -> Self {
        let n = coords.len() / 2;
        let x = |i: usize| coords[2 * i];
        let y = |i: usize| coords[2 * i + 1];

        if n < 3 {
            // Degenerate: no triangle possible. The hull is whatever points
            // exist, in input order -- callers (RN_NET) special-case n<=2
            // and the colinear case before ever calling into here, so this
            // is purely a defensive fallback, never hit from `ratsnest.rs`.
            return Triangulation { triangles: Vec::new(), halfedges: Vec::new(), hull: (0..n).collect() };
        }

        let (mut min_x, mut min_y, mut max_x, mut max_y) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for i in 0..n {
            min_x = min_x.min(x(i));
            min_y = min_y.min(y(i));
            max_x = max_x.max(x(i));
            max_y = max_y.max(y(i));
        }
        let (cx0, cy0) = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);

        // Seed point i0: closest to the bbox center.
        let mut i0 = 0usize;
        let mut min_dist = f64::INFINITY;
        for i in 0..n {
            let d = dist2(cx0, cy0, x(i), y(i));
            if d < min_dist {
                min_dist = d;
                i0 = i;
            }
        }

        // i1: closest to i0.
        let mut i1 = 0usize;
        min_dist = f64::INFINITY;
        for i in 0..n {
            if i == i0 {
                continue;
            }
            let d = dist2(x(i0), y(i0), x(i), y(i));
            if d < min_dist && d > 0.0 {
                min_dist = d;
                i1 = i;
            }
        }

        // i2: minimises the circumradius of (i0, i1, i2).
        let mut i2 = 0usize;
        let mut min_radius = f64::INFINITY;
        for i in 0..n {
            if i == i0 || i == i1 {
                continue;
            }
            let r = circumradius(x(i0), y(i0), x(i1), y(i1), x(i), y(i));
            if r < min_radius {
                min_radius = r;
                i2 = i;
            }
        }

        if !min_radius.is_finite() {
            // Every point is collinear: no triangle exists. Order the
            // points along the line and report them as the hull.
            // `areNodesColinear`/`RN_NET::Triangulate` in KiCad special-cases
            // this before triangulating at all (see `ratsnest.rs`); this
            // branch only guards a caller that skips that check.
            let mut ids: Vec<usize> = (0..n).collect();
            let dx = x(1) - x(0);
            let key = |i: usize| if dx != 0.0 { x(i) } else { y(i) };
            ids.sort_by(|&a, &b| key(a).partial_cmp(&key(b)).unwrap_or(std::cmp::Ordering::Equal));
            return Triangulation { triangles: Vec::new(), halfedges: Vec::new(), hull: ids };
        }

        let (mut i1, mut i2) = (i1, i2);
        if cross(x(i0), y(i0), x(i1), y(i1), x(i2), y(i2)) < 0.0 {
            std::mem::swap(&mut i1, &mut i2);
        }

        let (ccx, ccy) = circumcenter(x(i0), y(i0), x(i1), y(i1), x(i2), y(i2));

        let dists: Vec<f64> = (0..n).map(|i| dist2(x(i), y(i), ccx, ccy)).collect();
        let mut ids: Vec<usize> = (0..n).collect();
        sort_by_dist(&mut ids, &dists);

        let hash_size = (n as f64).sqrt().ceil() as usize;
        let hash_size = hash_size.max(1);

        let mut hull_prev = vec![0usize; n];
        let mut hull_next = vec![0usize; n];
        let mut hull_tri = vec![0usize; n];
        let mut hull_hash = vec![INVALID_INDEX; hash_size];

        let hash_key = |px: f64, py: f64| -> usize { (pseudo_angle(px - ccx, py - ccy) * hash_size as f64).floor() as usize % hash_size };

        let hull_start = i0;
        hull_next[i0] = i1;
        hull_prev[i2] = i1;
        hull_next[i1] = i2;
        hull_prev[i0] = i2;
        hull_next[i2] = i0;
        hull_prev[i1] = i0;

        hull_tri[i0] = 0;
        hull_tri[i1] = 1;
        hull_tri[i2] = 2;

        hull_hash[hash_key(x(i0), y(i0))] = i0;
        hull_hash[hash_key(x(i1), y(i1))] = i1;
        hull_hash[hash_key(x(i2), y(i2))] = i2;

        // Every point can add at most two triangles as it's inserted, plus
        // the seed triangle; oversize slightly and truncate at the end.
        let max_triangles = 2 * n;
        let mut triangles: Vec<usize> = Vec::with_capacity(max_triangles * 3);
        let mut halfedges: Vec<usize> = Vec::with_capacity(max_triangles * 3);

        let add_triangle = |triangles: &mut Vec<usize>, halfedges: &mut Vec<usize>, i0: usize, i1: usize, i2: usize, a: usize, b: usize, c: usize| -> usize {
            let t = triangles.len();
            triangles.push(i0);
            triangles.push(i1);
            triangles.push(i2);
            halfedges.push(a);
            halfedges.push(b);
            halfedges.push(c);
            if a != INVALID_INDEX {
                halfedges[a] = t;
            }
            if b != INVALID_INDEX {
                halfedges[b] = t + 1;
            }
            if c != INVALID_INDEX {
                halfedges[c] = t + 2;
            }
            t
        };

        add_triangle(&mut triangles, &mut halfedges, i0, i1, i2, INVALID_INDEX, INVALID_INDEX, INVALID_INDEX);

        let mut xp = f64::NAN;
        let mut yp = f64::NAN;

        for (k, &i) in ids.iter().enumerate() {
            let (px, py) = (x(i), y(i));

            if k > 0 && (px - xp).abs() <= f64::EPSILON && (py - yp).abs() <= f64::EPSILON {
                continue;
            }
            xp = px;
            yp = py;

            if i == i0 || i == i1 || i == i2 {
                continue;
            }

            // Find a hull vertex visible from (px,py), starting the walk
            // from whatever the angular hash suggests is nearby.
            let mut start = INVALID_INDEX;
            let key0 = hash_key(px, py);
            for j in 0..hash_size {
                let candidate = hull_hash[(key0 + j) % hash_size];
                if candidate != INVALID_INDEX && candidate != hull_next[candidate] {
                    start = candidate;
                    break;
                }
            }
            if start == INVALID_INDEX {
                // Every hash slot pointed at a removed vertex (can only
                // happen when the hull has degenerated to nothing useful);
                // there's nothing sane to attach this point to.
                continue;
            }
            start = hull_prev[start];

            let mut e = start;
            loop {
                let q = hull_next[e];
                if cross(px, py, x(e), y(e), x(q), y(q)) >= 0.0 {
                    e = q;
                    if e == start {
                        e = INVALID_INDEX;
                        break;
                    }
                } else {
                    break;
                }
            }
            if e == INVALID_INDEX {
                // Near-duplicate of an existing point that slipped past the
                // epsilon check above; skip it exactly as upstream does.
                continue;
            }

            // First new triangle, from the visible edge (e, hull_next[e]).
            let mut t = add_triangle(&mut triangles, &mut halfedges, e, i, hull_next[e], INVALID_INDEX, INVALID_INDEX, hull_tri[e]);
            hull_tri[i] = legalize(t + 2, &mut triangles, &mut halfedges, coords, &mut hull_tri, &hull_prev, hull_start);
            hull_tri[e] = t;

            // Walk forward over now-interior hull vertices, fanning in
            // more triangles and legalizing each.
            let mut n2 = hull_next[e];
            loop {
                let q = hull_next[n2];
                if cross(px, py, x(n2), y(n2), x(q), y(q)) < 0.0 {
                    t = add_triangle(&mut triangles, &mut halfedges, n2, i, q, hull_tri[i], INVALID_INDEX, hull_tri[n2]);
                    hull_tri[i] = legalize(t + 2, &mut triangles, &mut halfedges, coords, &mut hull_tri, &hull_prev, hull_start);
                    hull_next[n2] = n2; // mark removed
                    n2 = q;
                } else {
                    break;
                }
            }

            // Walk backward too, only when the forward walk consumed the
            // whole hull back around to `start` (mirrors delaunator.js: the
            // backward walk only fires in that case, since otherwise the
            // predecessor side was never made visible).
            if e == start {
                loop {
                    let q = hull_prev[e];
                    if cross(px, py, x(q), y(q), x(e), y(e)) < 0.0 {
                        let t = add_triangle(&mut triangles, &mut halfedges, q, i, e, INVALID_INDEX, hull_tri[e], hull_tri[q]);
                        legalize(t + 2, &mut triangles, &mut halfedges, coords, &mut hull_tri, &hull_prev, hull_start);
                        hull_tri[q] = t;
                        hull_next[e] = e; // mark removed
                        e = q;
                    } else {
                        break;
                    }
                }
            }

            // Splice the new point into the hull between e and n2.
            hull_prev[i] = e;
            hull_next[e] = i;
            hull_prev[n2] = i;
            hull_next[i] = n2;

            hull_hash[hash_key(px, py)] = i;
            hull_hash[hash_key(x(e), y(e))] = e;
        }

        // Walk the surviving hull linked list from `hull_start`.
        let mut hull = Vec::new();
        let mut e = hull_start;
        loop {
            hull.push(e);
            e = hull_next[e];
            if e == hull_start {
                break;
            }
        }

        Triangulation { triangles, halfedges, hull }
    }
}

/// Stack-based Lawson-flip legalisation starting from half-edge `a0`
/// (recursion turned into an explicit stack, same as delaunator's
/// `_legalize`, so a long flip chain on a large net can't blow the Rust
/// call stack). `hull_prev`/`hull_start` are only used for the rare case
/// where a flip's far edge sits on the convex hull (its `hull_tri` entry,
/// not a `halfedges` entry, must be retargeted).
///
/// The control flow is easy to get subtly wrong porting this from
/// recursion: after a flip at edge `a`, `a` itself must be re-examined
/// immediately (the flip just gave it a new opposite, `hbl`, which may
/// itself now be illegal against it) -- only an edge found *already
/// legal* (or sitting on the hull) is done, at which point the next edge
/// to check is popped from the stack. So the stack only ever holds
/// edges deferred for *later*, never the edge to look at right now.
#[allow(clippy::too_many_arguments)]
fn legalize(a0: usize, triangles: &mut [usize], halfedges: &mut [usize], coords: &[f64], hull_tri: &mut [usize], hull_prev: &[usize], hull_start: usize) -> usize {
    let x = |i: usize| coords[2 * i];
    let y = |i: usize| coords[2 * i + 1];

    let mut stack: Vec<usize> = Vec::new();
    let mut a = a0;
    let mut ar;

    loop {
        let b = halfedges[a];
        let tri_a0 = a - a % 3;
        ar = tri_a0 + (a + 2) % 3;

        if b == INVALID_INDEX {
            // `a` is a hull edge; nothing to flip against here.
            match stack.pop() {
                Some(next) => {
                    a = next;
                    continue;
                }
                None => break,
            }
        }

        let b0 = b - b % 3;
        let al = tri_a0 + (a + 1) % 3;
        let bl = b0 + (b + 2) % 3;

        let p0 = triangles[ar];
        let pr = triangles[a];
        let pl = triangles[al];
        let p1 = triangles[bl];

        let illegal = in_circle(x(p0), y(p0), x(pr), y(pr), x(pl), y(pl), x(p1), y(p1));

        if illegal {
            triangles[a] = p1;
            triangles[b] = p0;

            let hbl = halfedges[bl];

            // `bl`'s opposite lies beyond a triangle we've already
            // repointed via `hull_tri` rather than `halfedges` (only
            // possible when `bl` was itself a hull edge); retarget that
            // hull-side bookkeeping to the new triangle at `a`.
            if hbl == INVALID_INDEX {
                let mut e = hull_start;
                loop {
                    if hull_tri[e] == bl {
                        hull_tri[e] = a;
                        break;
                    }
                    e = hull_prev[e];
                    if e == hull_start {
                        break;
                    }
                }
            }

            link(a, hbl, halfedges);
            let har = halfedges[ar];
            link(b, har, halfedges);
            link(ar, bl, halfedges);

            let br = b0 + (b + 1) % 3;
            stack.push(br);
            // Re-examine `a` itself: the flip gave it a new opposite
            // (`hbl`), which may in turn be illegal against it.
        } else {
            match stack.pop() {
                Some(next) => a = next,
                None => break,
            }
        }
    }

    ar
}

#[inline]
fn link(a: usize, b: usize, halfedges: &mut [usize]) {
    halfedges[a] = b;
    if b != INVALID_INDEX {
        halfedges[b] = a;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    fn triangle_area2(coords: &[f64], a: usize, b: usize, c: usize) -> f64 {
        cross(coords[2 * a], coords[2 * a + 1], coords[2 * b], coords[2 * b + 1], coords[2 * c], coords[2 * c + 1])
    }

    /// Every triangle's circumcircle must contain no other input point --
    /// the defining property of a Delaunay triangulation, checked directly
    /// rather than trusting the incremental construction got it right.
    fn assert_delaunay(coords: &[f64], tri: &Triangulation) {
        let n = coords.len() / 2;
        for t in 0..tri.triangles.len() / 3 {
            let (a, b, c) = (tri.triangles[3 * t], tri.triangles[3 * t + 1], tri.triangles[3 * t + 2]);
            // Every triangle must be non-degenerate and CCW.
            assert!(triangle_area2(coords, a, b, c) > 0.0, "triangle {t} ({a},{b},{c}) is not CCW/degenerate");
            for p in 0..n {
                if p == a || p == b || p == c {
                    continue;
                }
                assert!(
                    !in_circle(coords[2 * a], coords[2 * a + 1], coords[2 * b], coords[2 * b + 1], coords[2 * c], coords[2 * c + 1], coords[2 * p], coords[2 * p + 1]),
                    "point {p} lies inside the circumcircle of triangle {t} ({a},{b},{c}) -- not Delaunay"
                );
            }
        }
    }

    /// halfedges must pair up: if `halfedges[e] == f` and `f` is valid,
    /// then `halfedges[f] == e`.
    fn assert_halfedges_consistent(tri: &Triangulation) {
        for (e, &f) in tri.halfedges.iter().enumerate() {
            if f != INVALID_INDEX {
                assert_eq!(tri.halfedges[f], e, "halfedge {e} <-> {f} not reciprocal");
            }
        }
    }

    #[test]
    fn three_points_make_one_triangle() {
        let coords = [0.0, 0.0, 10.0, 0.0, 5.0, 10.0];
        let tri = Triangulation::new(&coords);
        assert_eq!(tri.triangles.len(), 3);
        assert_delaunay(&coords, &tri);
        assert_halfedges_consistent(&tri);
    }

    #[test]
    fn square_splits_into_two_triangles_on_a_valid_diagonal() {
        let coords = [0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0];
        let tri = Triangulation::new(&coords);
        assert_eq!(tri.triangles.len(), 6, "a square triangulates into exactly 2 triangles");
        assert_delaunay(&coords, &tri);
        assert_halfedges_consistent(&tri);
        assert_eq!(tri.hull.len(), 4);
    }

    #[test]
    fn collinear_points_produce_no_triangles() {
        let coords = [0.0, 0.0, 10.0, 0.0, 20.0, 0.0, 30.0, 0.0];
        let tri = Triangulation::new(&coords);
        assert!(tri.triangles.is_empty());
    }

    #[test]
    fn grid_of_points_is_delaunay() {
        let mut coords = Vec::new();
        for gy in 0..6 {
            for gx in 0..6 {
                coords.push(gx as f64 * 37.0);
                coords.push(gy as f64 * 53.0);
            }
        }
        let tri = Triangulation::new(&coords);
        assert!(!tri.triangles.is_empty());
        assert_delaunay(&coords, &tri);
        assert_halfedges_consistent(&tri);
    }

    /// A small deterministic PRNG (xorshift64) -- no external `rand`
    /// dependency for a workspace crate that has none.
    fn xorshift(state: &mut u64) -> f64 {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (*state >> 11) as f64 / (1u64 << 53) as f64
    }

    #[test]
    fn random_points_are_delaunay() {
        for seed in [0x1234_5678_9abc_def1u64, 0xdead_beef_1234_5678, 0x0f0f_0f0f_f0f0_f0f0, 1, 42] {
            let mut state = seed;
            for trial in 0..12 {
                let n = 5 + trial * 7;
                let mut coords = Vec::with_capacity(n * 2);
                for _ in 0..n {
                    coords.push(xorshift(&mut state) * 100_000.0);
                    coords.push(xorshift(&mut state) * 100_000.0);
                }
                let tri = Triangulation::new(&coords);
                assert_delaunay(&coords, &tri);
                assert_halfedges_consistent(&tri);
            }
        }
    }

    /// Points confined to a coarse integer grid produce lots of exactly
    /// co-circular quadruples (every unit square's 4 corners share a
    /// circumcircle) -- a stress case for `in_circle`'s strict `<`/`>`
    /// comparison, which is expected to pick *a* legal triangulation of
    /// the tie, not any specific one.
    #[test]
    fn coarse_grid_with_many_cocircular_points_is_still_delaunay() {
        let mut state = 7u64;
        for _ in 0..3 {
            let mut coords = Vec::new();
            for gy in 0..8 {
                for gx in 0..8 {
                    // A little jitter, still snapped to a coarse grid, so
                    // exact and near-exact cocircularity both show up.
                    let jitter = (xorshift(&mut state) * 3.0).round();
                    coords.push(gx as f64 * 10.0 + jitter);
                    coords.push(gy as f64 * 10.0);
                }
            }
            let tri = Triangulation::new(&coords);
            assert_delaunay(&coords, &tri);
            assert_halfedges_consistent(&tri);
        }
    }

    #[test]
    fn duplicate_point_is_skipped_without_panicking() {
        let coords = [0.0, 0.0, 10.0, 0.0, 5.0, 10.0, 5.0, 10.0, 2.0, 3.0];
        let tri = Triangulation::new(&coords);
        assert_delaunay(&coords, &tri);
        assert_halfedges_consistent(&tri);
    }
}
