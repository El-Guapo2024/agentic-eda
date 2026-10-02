//! Port of `LINE::Walkaround( const SHAPE_LINE_CHAIN& aObstacle,
//! SHAPE_LINE_CHAIN& aPath, bool aCw )` (`pcbnew/router/pns_line.cpp`) and
//! the `SHAPE_LINE_CHAIN` / `HullIntersection` pieces it is built on: the
//! path and the hull are split at every valid crossing, merged into one
//! vertex graph, and walked from the path's start -- following the path
//! while it is outside the hull, the hull (in `aCw` order) while it isn't.
//!
//! Hulls must wind clockwise in KiCad's sense (`crate::hull::make_clockwise`).

use eda_drc::kimath::Seg;
use eda_model::ir::Point;

fn cross(o: Point, a: Point, b: Point) -> i128 {
    (a.x - o.x) as i128 * (b.y - o.y) as i128 - (a.y - o.y) as i128 * (b.x - o.x) as i128
}

/// `SEG::Side`.
fn side(a: Point, b: Point, p: Point) -> i32 {
    cross(a, b, p).signum() as i32
}

/// `SEG::Contains( p )`: within 1 unit of the segment.
fn seg_contains(a: Point, b: Point, p: Point) -> bool {
    Seg::new(a, b).sq_distance_to_point(p) <= 1
}

fn collinear(a: Point, b: Point, c: Point, d: Point) -> bool {
    cross(a, b, c) == 0 && cross(a, b, d) == 0
}

#[derive(Clone, Copy, Debug)]
struct Isect {
    p: Point,
    index_our: usize,
    index_their: usize,
    corner_our: bool,
    corner_their: bool,
}

/// `SHAPE_LINE_CHAIN::Intersect( aChain, aIp )` with `this` = the closed
/// hull and `aChain` = the open line.
fn intersect(hull: &[Point], line: &[Point]) -> Vec<Isect> {
    let mut out = Vec::new();
    let nh = hull.len();
    for s1 in 0..nh {
        let (a1, b1) = (hull[s1], hull[(s1 + 1) % nh]);
        for s2 in 0..line.len().saturating_sub(1) {
            let (a2, b2) = (line[s2], line[s2 + 1]);
            let base = Isect { p: a1, index_our: s1, index_their: s2, corner_our: false, corner_their: false };
            if collinear(a1, b1, a2, b2) {
                if seg_contains(a1, b1, a2) {
                    out.push(Isect { p: a2, corner_their: true, ..base });
                }
                if seg_contains(a1, b1, b2) {
                    out.push(Isect { p: b2, index_their: s2 + 1, corner_their: true, ..base });
                }
                if seg_contains(a2, b2, a1) {
                    out.push(Isect { p: a1, corner_our: true, ..base });
                }
                if seg_contains(a2, b2, b1) {
                    out.push(Isect { p: b1, index_our: s1 + 1, corner_our: true, ..base });
                }
            } else if let Some(p) = Seg::new(a1, b1).intersect(&Seg::new(a2, b2)) {
                let mut is = Isect { p, ..base };
                if p == a1 {
                    is.corner_our = true;
                }
                if p == b1 {
                    is.corner_our = true;
                    is.index_our += 1;
                }
                if p == a2 {
                    is.corner_their = true;
                }
                if p == b2 {
                    is.corner_their = true;
                    is.index_their += 1;
                }
                out.push(is);
            }
        }
    }
    out
}

/// `HullIntersection`: keep the crossings that really pass through the
/// hull (a corner touch counts only if some neighbouring line point lies on
/// the hull's inner side).
fn hull_intersection(hull: &[Point], line: &[Point]) -> Vec<Point> {
    if line.len() < 2 {
        return Vec::new();
    }
    let nh = hull.len();
    let hseg = |i: usize| (hull[i % nh], hull[(i + 1) % nh]);
    let mut ips = Vec::new();
    for mut p in intersect(hull, line) {
        if !p.corner_our && !p.corner_their {
            ips.push(p.p);
            continue;
        }
        if p.index_our >= nh {
            p.index_our -= nh;
        }
        let mut d1: Vec<(Point, Point)> = vec![hseg(p.index_our)];
        if p.corner_our {
            d1.push(hseg(p.index_our + nh - 1));
        }
        let mut d2: Vec<Point> = Vec::new();
        if p.corner_their {
            if p.index_their > 0 {
                d2.push(line[p.index_their - 1]);
            }
            if p.index_their < line.len() - 1 {
                d2.push(line[p.index_their + 1]);
            }
        } else {
            d2.push(line[p.index_their]);
            d2.push(line[p.index_their + 1]);
        }
        if d1.iter().any(|&(a, b)| d2.iter().any(|&q| side(a, b, q) > 0)) {
            ips.push(p.p);
        }
    }
    ips
}

/// `SHAPE_LINE_CHAIN::Find( p, aThreshold )`.
fn find(chain: &[Point], p: Point, threshold: i64) -> Option<usize> {
    chain.iter().position(|q| ((q.x - p.x) as i128).pow(2) + ((q.y - p.y) as i128).pow(2) <= (threshold as i128).pow(2))
}

/// `SHAPE_LINE_CHAIN::Split( p )`: insert `p` into the segment containing it.
fn split(chain: &mut Vec<Point>, closed: bool, p: Point) {
    if chain.contains(&p) {
        return;
    }
    let n = chain.len();
    let segs = if closed { n } else { n.saturating_sub(1) };
    let mut best: Option<(i128, usize)> = None;
    for i in 0..segs {
        let d = Seg::new(chain[i], chain[(i + 1) % n]).sq_distance_to_point(p);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    if let Some((d, i)) = best {
        // KiCad `Split`: `Distance < 2` with `Distance = isqrt( sqdist )`
        // (floor), i.e. squared distance <= 3 (shape_line_chain.cpp:1203-1210).
        if d <= 3 {
            chain.insert(i + 1, p);
        }
    }
}

/// `PointOnEdge` (accuracy 0: within 1 unit).
fn on_edge(hull: &[Point], p: Point) -> bool {
    let n = hull.len();
    (0..n).any(|i| {
        let (a, b) = (hull[i], hull[(i + 1) % n]);
        a == p || b == p || Seg::new(a, b).sq_distance_to_point(p) <= 1
    })
}

/// `SHAPE_LINE_CHAIN_BASE::PointInside` (even-odd, half-open in y).
fn inside(hull: &[Point], p: Point) -> bool {
    let n = hull.len();
    if n < 3 {
        return false;
    }
    let mut ins = false;
    for i in 0..n {
        let (p1, p2) = (hull[i], hull[(i + 1) % n]);
        let (dx, dy) = (p2.x - p1.x, p2.y - p1.y);
        if dy == 0 {
            continue;
        }
        let d = ((dx as i128 * (p.y - p1.y) as i128) as f64 / dy as f64).round() as i128;
        if ((p1.y >= p.y) != (p2.y >= p.y)) && ((p.x - p1.x) as i128) < d {
            ins = !ins;
        }
    }
    ins
}

fn are_neighbours(x: i64, y: i64, max: i64) -> bool {
    (x > 0 && x - 1 == y) || (x < max - 1 && x + 1 == y)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VType {
    Inside,
    Outside,
    OnEdge,
}

struct Vertex {
    ty: VType,
    is_hull: bool,
    pos: Point,
    neighbours: Vec<usize>,
    indexp: i64,
    indexh: i64,
    visited: bool,
}

/// `LINE::Walkaround( aObstacle, aPath, aCw )`. `None` = KiCad's `false`.
pub fn walkaround(line: &[Point], obstacle: &[Point], cw: bool) -> Option<Vec<Point>> {
    if line.len() < 2 || obstacle.len() < 3 {
        return None;
    }
    let p_first = line[0];
    if inside(obstacle, p_first) && !on_edge(obstacle, p_first) {
        return None;
    }

    let ips = hull_intersection(obstacle, line);
    let mut pnew = line.to_vec();
    let mut hnew = obstacle.to_vec();

    // Corner case for loopy tracks: split at the self-intersection point.
    'self_x: for i in 0..pnew.len().saturating_sub(1) {
        for j in (i + 2)..pnew.len().saturating_sub(1) {
            if let Some(p) = Seg::new(pnew[i], pnew[i + 1]).intersect(&Seg::new(pnew[j], pnew[j + 1])) {
                if Some(&p) != pnew.last() {
                    split(&mut pnew, false, p);
                }
                break 'self_x;
            }
        }
    }

    for &ip in &ips {
        if find(&pnew, ip, 1).is_none() {
            split(&mut pnew, false, ip);
        }
        if find(&hnew, ip, 1).is_none() {
            split(&mut hnew, true, ip);
        }
    }
    for i in 0..pnew.len() {
        let p = pnew[i];
        if on_edge(&hnew, p) && find(&hnew, p, 0).is_none() {
            split(&mut hnew, true, p);
        }
    }

    // "we assume the default orientation of the hulls is clockwise"
    if !cw {
        hnew.reverse();
    }

    let np = pnew.len() as i64;
    let nh = hnew.len() as i64;
    let mut vts: Vec<Vertex> = Vec::with_capacity(2 * (pnew.len() + hnew.len()));
    for (i, &p) in pnew.iter().enumerate() {
        let oe = on_edge(&hnew, p);
        let ins = inside(&hnew, p);
        let ty = if ins && !oe {
            VType::Inside
        } else if oe {
            VType::OnEdge
        } else {
            VType::Outside
        };
        vts.push(Vertex { ty, is_hull: false, pos: p, neighbours: Vec::new(), indexp: i as i64, indexh: -1, visited: false });
    }
    for i in 0..pnew.len().saturating_sub(1) {
        vts[i].neighbours.push(i + 1);
    }
    for i in 1..pnew.len() {
        vts[i].neighbours.push(i - 1);
    }
    let find_vertex = |vts: &Vec<Vertex>, pos: Point| vts.iter().position(|v| v.pos == pos);
    for (i, &hp) in hnew.iter().enumerate() {
        match find_vertex(&vts, hp) {
            Some(k) => {
                vts[k].is_hull = true;
                vts[k].indexh = i as i64;
            }
            None => vts.push(Vertex { ty: VType::OnEdge, is_hull: true, pos: hp, neighbours: Vec::new(), indexp: -1, indexh: i as i64, visited: false }),
        }
    }
    for i in 0..hnew.len() {
        // `hnew.CPoint( i+1 )` wraps on a closed chain.
        let (vc, vn) = (find_vertex(&vts, hnew[i]), find_vertex(&vts, hnew[(i + 1) % hnew.len()]));
        if let (Some(c), Some(n)) = (vc, vn) {
            vts[c].neighbours.push(n);
        }
    }

    let last = *line.last().unwrap();
    let in_last = inside(obstacle, last) && !on_edge(obstacle, last);
    let mut append_v = true;
    let mut last_dst = i128::MAX;
    let mut v = 0usize;
    let mut v_prev: Option<usize> = None;
    let mut out: Vec<Point> = Vec::new();
    let mut iter_limit = 1000;

    while vts[v].indexp != np - 1 {
        iter_limit -= 1;
        if iter_limit == 0 {
            return None;
        }
        if vts[v].visited {
            break; // loop found
        }
        out.push(vts[v].pos);
        let mut v_next: Option<usize> = None;

        match vts[v].ty {
            VType::Outside => {
                out.push(vts[v].pos);
                let mut fallback = None;
                for &vn in &vts[v].neighbours {
                    if are_neighbours(vts[vn].indexp, vts[v].indexp, np) && vts[vn].ty != VType::Inside {
                        if !vts[vn].visited {
                            v_next = Some(vn);
                            break;
                        } else if Some(vn) != v_prev {
                            fallback = Some(vn);
                        }
                    }
                }
                if v_next.is_none() {
                    v_next = fallback;
                }
                v_next?;
            }
            VType::OnEdge => {
                for &vn in &vts[v].neighbours {
                    if vts[vn].ty == VType::Outside && !vts[vn].visited {
                        v_next = Some(vn);
                        break;
                    }
                }
                if v_next.is_none() {
                    for &vn in &vts[v].neighbours {
                        if vts[vn].ty == VType::OnEdge && !vts[vn].is_hull && are_neighbours(vts[vn].indexp, vts[v].indexp, np) && vts[vn].indexh == (vts[v].indexh + 1) % nh {
                            v_next = Some(vn);
                            break;
                        }
                    }
                }
                if v_next.is_none() {
                    for &vn in &vts[v].neighbours {
                        if vts[vn].ty == VType::OnEdge && vts[vn].indexh == (vts[v].indexh + 1) % nh {
                            v_next = Some(vn);
                            break;
                        }
                    }
                    if v_next.is_some() {
                        for vt in vts.iter_mut().filter(|vt| vt.is_hull) {
                            vt.visited = false;
                        }
                    }
                    if let (true, Some(nx)) = (in_last, v_next) {
                        let q = vts[nx].pos;
                        let d = ((q.x - last.x) as i128).pow(2) + ((q.y - last.y) as i128).pow(2);
                        if d < last_dst {
                            last_dst = d;
                        } else {
                            out.push(Seg::new(vts[v].pos, q).nearest_point(last));
                            append_v = false;
                            break;
                        }
                    }
                }
            }
            VType::Inside => {}
        }

        vts[v].visited = true;
        v_prev = Some(v);
        v = v_next?;
    }

    if append_v {
        out.push(vts[v].pos);
    }
    out.dedup();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hull::make_clockwise;

    fn square(x0: i64, y0: i64, x1: i64, y1: i64) -> Vec<Point> {
        make_clockwise(vec![Point { x: x0, y: y0 }, Point { x: x1, y: y0 }, Point { x: x1, y: y1 }, Point { x: x0, y: y1 }])
    }

    /// D3 repro from docs/parity/CODE-COMPARE-router.md: the exit crossing
    /// used to be dropped, so the path went straight through the hull.
    #[test]
    fn walkaround_does_not_cut_through_hull_d3_repro() {
        let pt = |x, y| Point { x, y };
        let line = [pt(403, 5139), pt(9992, 5639)];
        let hull = make_clockwise(vec![pt(5157, 5508), pt(4849, 5386), pt(4717, 5082), pt(4839, 4774), pt(6502, 4054), pt(6810, 4176), pt(6942, 4480), pt(6820, 4788)]);
        for cw in [true, false] {
            let path = walkaround(&line, &hull, cw).unwrap();
            assert!(path.len() > 3, "cw={cw} {path:?}");
        }
    }

    /// D3: random chords against random convex octagons must never produce
    /// a path whose segments run through the hull interior.
    #[test]
    fn walkaround_random_chords_never_enter_convex_hull() {
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut rnd = |m: i64| -> i64 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as i64) % m
        };
        let mut bad = 0;
        for _ in 0..3000 {
            let (cx, cy, r) = (5000 + rnd(1000), 5000 + rnd(1000), 300 + rnd(1500));
            let k = 0.4142f64;
            let ro = (r as f64 * k) as i64;
            let hull = make_clockwise(vec![
                Point { x: cx + ro, y: cy - r }, Point { x: cx + r, y: cy - ro }, Point { x: cx + r, y: cy + ro }, Point { x: cx + ro, y: cy + r },
                Point { x: cx - ro, y: cy + r }, Point { x: cx - r, y: cy + ro }, Point { x: cx - r, y: cy - ro }, Point { x: cx - ro, y: cy - r },
            ]);
            let line = [Point { x: rnd(3000), y: rnd(10000) }, Point { x: 7000 + rnd(3000), y: rnd(10000) }];
            for cw in [true, false] {
                let Some(path) = walkaround(&line, &hull, cw) else { continue };
                for w in path.windows(2) {
                    for f in [0.25f64, 0.5, 0.75] {
                        let (x, y) = (w[0].x as f64 + (w[1].x - w[0].x) as f64 * f, w[0].y as f64 + (w[1].y - w[0].y) as f64 * f);
                        let depth = (0..hull.len()).map(|i| {
                            let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
                            let (ex, ey) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
                            let cr = ex * (y - a.y as f64) - ey * (x - a.x as f64);
                            cr / (ex * ex + ey * ey).sqrt()
                        });
                        // hull is clockwise (y up): interior has all-negative or all-positive cross; use sign-agnostic min of |.| when same sign
                        let d: Vec<f64> = depth.collect();
                        if (d.iter().all(|v| *v < -3.0)) || (d.iter().all(|v| *v > 3.0)) {
                            bad += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(bad, 0, "walkaround paths entered the hull");
    }

    #[test]
    fn walks_around_a_square_both_ways() {
        let hull = square(400, -100, 600, 100);
        let line = [Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }];
        let a = walkaround(&line, &hull, true).unwrap();
        let b = walkaround(&line, &hull, false).unwrap();
        assert_eq!(a.first(), Some(&line[0]));
        assert_eq!(a.last(), Some(&line[1]));
        assert_eq!(b.last(), Some(&line[1]));
        // One goes over the top, the other under the bottom.
        let ys_a: Vec<i64> = a.iter().map(|p| p.y).collect();
        let ys_b: Vec<i64> = b.iter().map(|p| p.y).collect();
        assert!((ys_a.contains(&100) && ys_b.contains(&-100)) || (ys_a.contains(&-100) && ys_b.contains(&100)), "{a:?} / {b:?}");
    }

    #[test]
    fn a_line_that_misses_the_hull_is_unchanged() {
        let hull = square(400, 500, 600, 700);
        let line = [Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }];
        assert_eq!(walkaround(&line, &hull, true).unwrap(), line.to_vec());
    }

    #[test]
    fn walks_both_ways_around_a_roundrect_pad_hull() {
        let shape = eda_drc::kimath::Shape::RoundRect { x0: 2475 - 975, y0: -1905 - 300, x1: 2475 + 975, y1: -1905 + 300, r: 150 };
        let hull = crate::hull::primitive_hull(&shape, 200, 200);
        let line = [Point { x: -2475, y: -1905 }, Point { x: 8665, y: -1905 }, Point { x: 12475, y: 1905 }];
        assert_eq!(hull_intersection(&hull, &line).len(), 2);
        for cw in [true, false] {
            let p = walkaround(&line, &hull, cw).expect("both windings clear one pad");
            assert_eq!(p.first(), Some(&line[0]));
            assert_eq!(p.last(), Some(&line[2]));
            assert!(p.iter().all(|q| !inside(&hull, *q) || on_edge(&hull, *q)), "never enters the hull: {p:?}");
        }
    }

    #[test]
    fn starting_inside_the_hull_fails() {
        let hull = square(-100, -100, 100, 100);
        let line = [Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }];
        assert!(walkaround(&line, &hull, true).is_none());
    }

    #[test]
    fn ending_inside_the_hull_stops_at_the_nearest_projection() {
        let hull = square(400, -100, 600, 100);
        let line = [Point { x: 0, y: 0 }, Point { x: 500, y: 0 }];
        let p = walkaround(&line, &hull, true).unwrap();
        assert_eq!(p.first(), Some(&line[0]));
        assert!(p.len() >= 2);
    }
}
