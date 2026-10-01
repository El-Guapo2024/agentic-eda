//! A faithful port of `fractureSingleCacheFriendly`/`processHole` and
//! `unfractureSingle` from `libs/kimath/src/geometry/shape_poly_set.cpp`.
//!
//! KiCad picks between two fracture implementations at runtime
//! (`ENABLECACHEFRIENDLYFRACTURE`, true by default outside of a debug
//! advanced-config override); this ports the cache-friendly one, since
//! that's what every normal KiCad build actually runs. It's already
//! index-based in the original (`FractureEdge::Index` into a
//! `std::vector<FractureEdge>`, not a pointer-linked list -- the index `0`
//! doubles as a "no next" sentinel because the outline's own closing edge
//! is always built first and always lands at index 0), so the port is a
//! close-to-mechanical translation.

use crate::Polygon;
use eda_clipper2::Point64;

type Index = usize;

#[derive(Clone, Copy)]
struct FractureEdge {
    p1: Point64,
    p2: Point64,
    next: Index,
}

impl FractureEdge {
    fn matches(&self, y: i64) -> bool {
        (y >= self.p1.y || y >= self.p2.y) && (y <= self.p1.y || y <= self.p2.y)
    }
}

/// `rescale<int64_t>` (the `__SIZEOF_INT128__` branch, which is what any
/// real 64-bit gcc/clang build takes): `aNumerator * aValue / aDenominator`,
/// rounded to nearest with overflow-safe `i128` intermediate arithmetic.
fn rescale(numerator: i64, value: i64, denominator: i64) -> i64 {
    let n = numerator as i128 * value as i128;
    let d = denominator as i128;
    if (n < 0) != (d < 0) {
        ((n - d / 2) / d) as i64
    } else {
        ((n + d / 2) / d) as i64
    }
}

/// `processHole`.
fn process_hole(edges: &mut [FractureEdge], provoking_index: Index, edge_index: Index, bridge_index: Index) -> bool {
    let edge_p1 = edges[edge_index].p1;
    let (x, y) = (edge_p1.x, edge_p1.y);
    let mut min_dist = i64::MAX;
    let mut x_nearest = 0i64;
    let mut e_nearest: Option<Index> = None;

    // Holes are processed left to right, so no edge beyond the provoking one
    // needs checking: it would always be further right, and unconnected to
    // the outline anyway.
    for (i, e) in edges.iter().enumerate().take(provoking_index) {
        if !e.matches(y) {
            continue;
        }

        let x_intersect = if e.p1.y == e.p2.y {
            e.p1.x.max(e.p2.x)
        } else {
            e.p1.x + rescale(e.p2.x - e.p1.x, y - e.p1.y, e.p2.y - e.p1.y)
        };

        let dist = x - x_intersect;
        if dist >= 0 && dist < min_dist {
            min_dist = dist;
            x_nearest = x_intersect;
            e_nearest = Some(i);
        }
    }

    match e_nearest {
        Some(e_nearest_idx) => {
            let outline2hole_index = bridge_index;
            let hole2outline_index = bridge_index + 1;
            let split_index = bridge_index + 2;
            let bridge_pt = Point64::new(x_nearest, y);
            let e_nearest_p2 = edges[e_nearest_idx].p2;
            let e_nearest_next = edges[e_nearest_idx].next;

            // an edge between the split outline edge and the hole ...
            edges[outline2hole_index] = FractureEdge { p1: bridge_pt, p2: edge_p1, next: edge_index };
            // ... between the hole and the edge ...
            edges[hole2outline_index] = FractureEdge { p1: edge_p1, p2: bridge_pt, next: split_index };
            // ... and between the split outline edge and the rest.
            edges[split_index] = FractureEdge { p1: bridge_pt, p2: e_nearest_p2, next: e_nearest_next };

            // perform the actual outline edge split
            edges[e_nearest_idx].p2 = bridge_pt;
            edges[e_nearest_idx].next = outline2hole_index;

            let mut last = edge_index;
            while edges[last].next != edge_index {
                last = edges[last].next;
            }
            edges[last].next = hole2outline_index;
            true
        }
        None => false,
    }
}

/// `fractureSingleCacheFriendly`.
pub fn fracture_single(paths: &mut Polygon) {
    if paths.len() == 1 {
        return;
    }

    struct PathInfo {
        path_index: usize,
        leftmost: usize,
        x: i64,
        y: i64,
        // repurposed (matching the original) once building edges: becomes
        // (provoking_edge, bridge_start_index) for every hole.
        provoking_edge: Index,
        bridge_index: Index,
    }

    let paths_count = paths.len();
    let mut path_infos: Vec<PathInfo> = Vec::with_capacity(paths_count);
    for (path_index, path) in paths.iter().enumerate() {
        let mut x_min = i64::MAX;
        let mut y_min = i64::MAX;
        let mut leftmost = 0usize;
        for (point_index, p) in path.iter().enumerate() {
            if p.x < x_min {
                x_min = p.x;
                leftmost = point_index;
            }
            if p.y < y_min {
                y_min = p.y;
            }
        }
        path_infos.push(PathInfo { path_index, leftmost, x: x_min, y: y_min, provoking_edge: 0, bridge_index: 0 });
    }

    // sort every path *except the outline* (index 0) by (x, then y)
    path_infos[1..].sort_by(|a, b| if a.x == b.x { a.y.cmp(&b.y) } else { a.x.cmp(&b.x) });
    let mut sorted_paths: Vec<&mut PathInfo> = path_infos.iter_mut().collect();

    let mut edges: Vec<FractureEdge> = Vec::new();
    let mut edge_index: Index = 0;
    let mut is_outline = true;

    for path_info in sorted_paths.iter_mut() {
        let path = &paths[path_info.path_index];
        let point_count = path.len();
        let provoking_edge = edge_index;

        for i in 0..point_count - 1 {
            edges.push(FractureEdge { p1: path[i], p2: path[i + 1], next: edge_index + 1 });
            edge_index += 1;
        }
        // last edge loops back to the provoking one
        edges.push(FractureEdge { p1: path[point_count - 1], p2: path[0], next: provoking_edge });
        edge_index += 1;

        if !is_outline {
            path_info.provoking_edge = provoking_edge;
            path_info.bridge_index = edge_index;
            // reserve 3 additional edges to bridge with the outline
            edge_index += 3;
            edges.resize(edge_index, FractureEdge { p1: Point64::default(), p2: Point64::default(), next: 0 });
        }
        is_outline = false;
    }

    for path_info in sorted_paths.iter().skip(1) {
        let ok = process_hole(&mut edges, path_info.provoking_edge, path_info.provoking_edge + path_info.leftmost, path_info.bridge_index);
        if !ok {
            // broken polygon: drop this path (matches upstream's "return"
            // which abandons the whole fracture -- the shape is left
            // un-fractured).
            return;
        }
    }

    let mut new_path = Vec::new();
    let mut e = 0usize;
    loop {
        new_path.push(edges[e].p1);
        if edges[e].next == 0 {
            break;
        }
        e = edges[e].next;
    }

    paths.clear();
    paths.push(new_path);
}

/// `unfractureSingle`: recovers outline+holes from a single slitted path by
/// pairing up coincident (anti-parallel, `A==B' && B==A'`) "bridge" edges
/// and walking the resulting disjoint cycles back out.
pub fn unfracture_single(poly: &mut Polygon) {
    // `assert(aPoly.size() == 1)` in the original -- compiled out under
    // NDEBUG in a release KiCad build, so a caller that (mis-)calls this on
    // already-unfractured (outline+holes) data silently gets back whatever
    // `unfractureSingle` makes of just its first contour there too; this
    // only upgrades the empty-input case from C++ UB to a no-op.
    debug_assert_eq!(poly.len(), 1);
    if poly.is_empty() {
        return;
    }

    let lc = &poly[0];
    let seg_count = lc.len(); // closed chain: segment i is (lc[i], lc[(i+1)%n])
    if seg_count == 0 {
        return;
    }

    let seg = |i: usize| -> (Point64, Point64) { (lc[i], lc[(i + 1) % seg_count]) };

    // Group segment indices by an order-independent key of their endpoints
    // so we can find, for each segment, the other segment that runs the
    // exact reverse direction over the same two points (`compareSegs`).
    use std::collections::HashMap;
    let mut by_key: HashMap<(Point64, Point64), Vec<usize>> = HashMap::new();
    let canon = |a: Point64, b: Point64| -> (Point64, Point64) {
        if (a.x, a.y) <= (b.x, b.y) {
            (a, b)
        } else {
            (b, a)
        }
    };
    for i in 0..seg_count {
        let (a, b) = seg(i);
        by_key.entry(canon(a, b)).or_default().push(i);
    }

    // `next[i]` mirrors the original's circular `EDGE_LIST_ENTRY` chain,
    // initially just "the next segment around the outline"; `None` marks a
    // severed link (the original's `nullptr`).
    let mut next: Vec<Option<usize>> = (0..seg_count).map(|i| Some(if i != seg_count - 1 { i + 1 } else { 0 })).collect();

    for i in 0..seg_count {
        let (a, b) = seg(i);
        let key = canon(a, b);
        if let Some(candidates) = by_key.get(&key) {
            // `uniqueEdges.find(e)` returns *some* matching bucket member;
            // the original relies on unordered_set returning a consistent
            // single match, which in practice is the single geometric
            // anti-parallel twin for well-formed fractured output (at most
            // one other segment shares both endpoints, reversed).
            if let Some(&other) = candidates.iter().find(|&&j: &&usize| j != i && seg(j) == (b, a)) {
                let (mut e1, mut e2) = (other, i);
                if e1 > e2 {
                    std::mem::swap(&mut e1, &mut e2);
                }
                let e1_prev = if e1 == 0 { seg_count - 1 } else { e1 - 1 };
                let e2_prev = if e2 == 0 { seg_count - 1 } else { e2 - 1 };
                let e1_next = if e1 + 1 == seg_count { 0 } else { e1 + 1 };
                let e2_next = if e2 + 1 == seg_count { 0 } else { e2 + 1 };

                next[e1_prev] = Some(e2_next);
                next[e2_prev] = Some(e1_next);
                next[i] = None;
                next[other] = None;
            }
        }
    }

    let mut queued: Vec<bool> = (0..seg_count).map(|i| next[i].is_some()).collect();
    let mut result: Polygon = Vec::new();
    let mut max_area = 0.0f64;
    let mut outline_idx: Option<usize> = None;

    while let Some(start) = queued.iter().position(|&q| q) {
        let mut outl = Vec::new();
        let mut e = start;
        loop {
            outl.push(lc[e]);
            queued[e] = false;
            match next[e] {
                Some(n) if n != start => e = n,
                _ => break,
            }
        }

        let area = eda_clipper2::area(&outl).abs();
        if area > max_area {
            outline_idx = Some(result.len());
            max_area = area;
        }
        result.push(outl);
    }

    if let Some(idx) = outline_idx {
        if idx > 0 {
            result.swap(0, idx);
        }
    }

    *poly = result;
}
