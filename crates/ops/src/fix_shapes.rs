//! `ConnectBoardShapes` (`pcbnew/fix_board_shape.cpp` at KiCad 8303b2ad): close the small gaps between the ends of lines, arcs and curves.
//!
//! "Trim/extend open shapes or add segments to make vertices of shapes coincide" is what Import Graphics' "Fix discontinuities" says and what
//! Heal Shapes and Cleanup Graphics do with a selection. The shapes are walked end to end: from a shape, the nearest end point of another
//! shape within the tolerance is the next one, and the two are brought together (`connectPair`): two lines meet where their lines cross
//! when they cross or are more than 45 degrees apart, a line takes the end point of an arc or a curve, two arcs or two curves both move
//! to the middle of the gap.
//!
//! The k-d tree KiCad searches (the two nearest end points of all, as they were before anything moved) is a scan here, which returns the same
//! two points. Only lines, arcs and curves have end points to join; a circle, a rectangle or a polygon is not part of a chain (KiCad adds
//! their start and end points to the tree as well, so a circle's centre near a line's end stops a walk there).

use eda_model::ir::{Point, Shape, Nm};

type P = (i64, i64);

fn pt(p: Point) -> P {
    (p.x.raw_shim(), p.y.raw_shim())
}

fn point(p: P) -> Point {
    Point { x: Nm::from_raw_shim(p.0), y: Nm::from_raw_shim(p.1) }
}

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn sq(a: P, b: P) -> i128 {
    let d = sub(a, b);
    d.0 as i128 * d.0 as i128 + d.1 as i128 * d.1 as i128
}

/// `std::min_element` over four squared distances: the first of the smallest.
fn argmin4(d: [i128; 4]) -> usize {
    let mut best = 0;
    for (i, v) in d.iter().enumerate().skip(1) {
        if *v < d[best] {
            best = i;
        }
    }
    best
}

fn cross(a: P, b: P) -> i128 {
    a.0 as i128 * b.1 as i128 - a.1 as i128 * b.0 as i128
}

/// `rescale( num, value, den )` = `num * value / den`, truncating toward zero.
fn rescale(num: i128, value: i128, den: i128) -> i64 {
    (num * value / den) as i64
}

/// `EDA_ANGLE( VECTOR2D )` in degrees (the negative x axis is -180, the zero vector 0).
fn angle_of(v: P) -> f64 {
    let (x, y) = v;
    if x == 0 && y == 0 {
        return 0.0;
    }
    if y == 0 {
        return if x >= 0 { 0.0 } else { -180.0 };
    }
    if x == 0 {
        return if y >= 0 { 90.0 } else { -90.0 };
    }
    (y as f64).atan2(x as f64).to_degrees()
}

fn normalize180(a: f64) -> f64 {
    let mut v = a;
    while v <= -180.0 {
        v += 360.0;
    }
    while v > 180.0 {
        v -= 360.0;
    }
    v
}

/// `SEG::Angle`: the absolute angle between the two lines, 0 to 180 degrees.
fn seg_angle(s: (P, P), o: (P, P)) -> f64 {
    let this = normalize180(angle_of(sub(s.0, s.1)));
    let other = normalize180(angle_of(sub(o.0, o.1)));
    normalize180(this - other).abs()
}

/// `SEG::Intersect( aSeg, aIgnoreEndpoints = false, aLines )`: where the two segments (or, with `lines`, the two infinite lines) cross.
fn seg_intersect(s: (P, P), o: (P, P), lines: bool) -> Option<P> {
    if !lines {
        if s.0 .0.max(s.1 .0) < o.0 .0.min(o.1 .0) || o.0 .0.max(o.1 .0) < s.0 .0.min(s.1 .0) {
            return None;
        }
        if s.0 .1.max(s.1 .1) < o.0 .1.min(o.1 .1) || o.0 .1.max(o.1 .1) < s.0 .1.min(s.1 .1) {
            return None;
        }
    }
    let dir1 = sub(s.1, s.0);
    let dir2 = sub(o.1, o.0);
    let offset = sub(o.0, s.0);
    let determinant = cross(dir2, dir1);
    if determinant == 0 {
        if cross(dir1, offset) != 0 {
            return None; // parallel, not collinear
        }
        if lines {
            if o.0 == o.1 {
                return Some(o.0);
            }
            if s.0 == s.1 {
                return Some(s.0);
            }
            return Some(((s.0 .0 + o.0 .0) / 2, (s.0 .1 + o.0 .1) / 2));
        }
        return collinear_overlap(s, o, dir1.0.abs() >= dir1.1.abs());
    }
    let param2 = cross(dir2, offset);
    let param1 = cross(dir1, offset);
    if !lines {
        if determinant > 0 {
            if param1 < 0 || param1 > determinant || param2 < 0 || param2 > determinant {
                return None;
            }
        } else if param1 > 0 || param1 < determinant || param2 > 0 || param2 < determinant {
            return None;
        }
    }
    Some((o.0 .0 + rescale(param1, dir2.0 as i128, determinant), o.0 .1 + rescale(param1, dir2.1 as i128, determinant)))
}

/// `SEG::checkCollinearOverlap`: do two collinear segments overlap on the axis they mostly run along, and where (the middle of the overlap)?
fn collinear_overlap(s: (P, P), o: (P, P), use_x: bool) -> Option<P> {
    let (i, j) = if use_x { (0, 1) } else { (1, 0) };
    let get = |p: P, k: usize| if k == 0 { p.0 } else { p.1 };
    let (s1s, s1e, s2s, s2e) = (get(s.0, i), get(s.1, i), get(o.0, i), get(o.1, i));
    let (c1s, c1e) = (get(s.0, j), get(s.1, j));
    let (s1min, s1max, s2min, s2max) = (s1s.min(s1e), s1s.max(s1e), s2s.min(s2e), s2s.max(s2e));
    if !(s1max >= s2min && s2max >= s1min) {
        return None;
    }
    let (ostart, oend) = (s1min.max(s2min), s1max.min(s2max));
    let proj = (ostart + oend) / 2;
    let other = if s1e != s1s { c1s + rescale((proj - s1s) as i128, (c1e - c1s) as i128, (s1e - s1s) as i128) } else { c1s };
    Some(if use_x { (proj, other) } else { (other, proj) })
}

/// The shape being walked: only its end points and what moves with them.
#[derive(Clone, Copy)]
struct Piece {
    /// Index into the caller's list.
    at: usize,
}

/// `ConnectBoardShapes( aShapeList, aChainingEpsilon )` over the shapes of `shapes` that are lines, arcs or curves. Moves their end
/// points in place; nothing is added or removed. `epsilon` is in micrometres (the design's unit).
pub(crate) fn connect_board_shapes(shapes: &mut [Shape], epsilon: Nm) {
    // The pieces and every end point of them as they were before anything moved (the k-d tree is built once).
    let pieces: Vec<Piece> = shapes.iter().enumerate().filter(|(_, s)| matches!(s, Shape::Segment { .. } | Shape::Arc { .. } | Shape::Bezier { .. })).map(|(at, _)| Piece { at }).collect();
    if pieces.is_empty() {
        return;
    }
    let ends_of = |s: &Shape| -> (P, P) {
        match s {
            Shape::Segment { start, end, .. } | Shape::Arc { start, end, .. } | Shape::Bezier { start, end, .. } => (pt(*start), pt(*end)),
            _ => unreachable!(),
        }
    };
    let endpoints: Vec<(P, usize)> = pieces.iter().enumerate().flat_map(|(pi, p)| {
        let (a, b) = ends_of(&shapes[p.at]);
        [(a, pi), (b, pi)]
    }).collect();
    let mut skip = vec![false; pieces.len()];
    let eps_sq = epsilon.raw_shim() as i128 * epsilon.raw_shim() as i128;

    // `findNext`: of the two nearest end points to `p`, the closest valid one of another piece that is within the epsilon.
    let find_next = |curr: usize, p: P, skip: &[bool]| -> Option<usize> {
        let mut order: Vec<(i128, usize)> = endpoints.iter().enumerate().map(|(i, (q, _))| (sq(p, *q), i)).collect();
        order.sort();
        let mut closest = None;
        let mut closest_sq = eps_sq;
        for (d, i) in order.into_iter().take(2) {
            let cand = endpoints[i].1;
            if cand == curr || skip[cand] {
                continue;
            }
            if d < closest_sq {
                closest_sq = d;
                closest = Some(cand);
            }
        }
        closest
    };

    let ends = |shapes: &[Shape], pi: usize| ends_of(&shapes[pieces[pi].at]);
    let closer_to_first = |r: P, a: P, b: P| sq(r, a) < sq(r, b);
    let min_distance_sq = |r: P, a: P, b: P| sq(r, a).min(sq(r, b));

    let mut candidates: Vec<usize> = (0..pieces.len()).collect();
    while let Some(&graphic) = candidates.first() {
        let walk_from = |shapes: &mut [Shape], skip: &mut Vec<bool>, candidates: &mut Vec<usize>, start: usize, start_pt: P| {
            let mut curr = start;
            let mut prev_pt = start_pt;
            loop {
                let Some(next) = find_next(curr, prev_pt, skip) else { break };
                connect_pair(shapes, pieces[curr].at, pieces[next].at);
                let (s, e) = ends(shapes, next);
                prev_pt = if closer_to_first(prev_pt, s, e) { e } else { s };
                curr = next;
                skip[curr] = true;
                candidates.retain(|c| *c != curr);
            }
        };
        let (pt_start, pt_end) = ends(shapes, graphic);
        let gr_at_end = find_next(graphic, pt_end, &skip);
        let gr_at_start = find_next(graphic, pt_start, &skip);
        let begin_from_end = match (gr_at_end, gr_at_start) {
            (Some(e), Some(s)) => {
                let (es, ee) = ends(shapes, e);
                let (ss, se) = ends(shapes, s);
                min_distance_sq(pt_end, es, ee) <= min_distance_sq(pt_start, ss, se)
            }
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => true,
        };
        if begin_from_end {
            let e = ends(shapes, graphic).1;
            walk_from(shapes, &mut skip, &mut candidates, graphic, e);
            let s = ends(shapes, graphic).0;
            walk_from(shapes, &mut skip, &mut candidates, graphic, s);
        } else {
            let s = ends(shapes, graphic).0;
            walk_from(shapes, &mut skip, &mut candidates, graphic, s);
            let e = ends(shapes, graphic).1;
            walk_from(shapes, &mut skip, &mut candidates, graphic, e);
        }
        candidates.retain(|c| *c != graphic);
    }
}

fn set_start(s: &mut Shape, p: P) {
    match s {
        Shape::Segment { start, .. } | Shape::Arc { start, .. } | Shape::Bezier { start, .. } => *start = point(p),
        _ => {}
    }
}

fn set_end(s: &mut Shape, p: P) {
    match s {
        Shape::Segment { end, .. } | Shape::Arc { end, .. } | Shape::Bezier { end, .. } => *end = point(p),
        _ => {}
    }
}

/// Move an end of a curve to `to`; its control point moves by the same step.
fn move_bezier_end(s: &mut Shape, at_start: bool, to: P) {
    if let Shape::Bezier { start, c1, c2, end, .. } = s {
        if at_start {
            let d = sub(to, pt(*start));
            *start = point(to);
            *c1 = Point { x: c1.x + Nm::from_raw_shim(d.0), y: c1.y + Nm::from_raw_shim(d.1) };
        } else {
            let d = sub(to, pt(*end));
            *end = point(to);
            *c2 = Point { x: c2.x + Nm::from_raw_shim(d.0), y: c2.y + Nm::from_raw_shim(d.1) };
        }
    }
}

/// `connectPair`: bring the nearest pair of end points of two shapes together.
fn connect_pair(shapes: &mut [Shape], a: usize, b: usize) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum K {
        Seg,
        Arc,
        Bez,
    }
    let kind = |s: &Shape| match s {
        Shape::Segment { .. } => K::Seg,
        Shape::Arc { .. } => K::Arc,
        _ => K::Bez,
    };
    let ends = |s: &Shape| -> (P, P) {
        match s {
            Shape::Segment { start, end, .. } | Shape::Arc { start, end, .. } | Shape::Bezier { start, end, .. } => (pt(*start), pt(*end)),
            _ => unreachable!(),
        }
    };
    let (k0, k1) = (kind(&shapes[a]), kind(&shapes[b]));
    let (e0, e1) = (ends(&shapes[a]), ends(&shapes[b]));
    match (k0, k1) {
        (K::Seg, K::Seg) => {
            let d = [sq(e0.0, e1.0), sq(e0.0, e1.1), sq(e0.1, e1.0), sq(e0.1, e1.1)];
            let idx = argmin4(d);
            let (i0, i1) = (idx / 2, idx % 2);
            if seg_intersect(e0, e1, false).is_some() || seg_angle(e0, e1) > 45.0 {
                if let Some(inter) = seg_intersect(e0, e1, true) {
                    if i0 == 0 { set_start(&mut shapes[a], inter) } else { set_end(&mut shapes[a], inter) }
                    if i1 == 0 { set_start(&mut shapes[b], inter) } else { set_end(&mut shapes[b], inter) }
                    return true;
                }
            }
            false
        }
        (K::Seg, K::Arc) | (K::Arc, K::Seg) | (K::Seg, K::Bez) | (K::Bez, K::Seg) => {
            // The line takes the nearer end point of the arc or curve.
            let (seg_i, other_i) = if k0 == K::Seg { (a, b) } else { (b, a) };
            let seg_pts = ends(&shapes[seg_i]);
            let other_pts = ends(&shapes[other_i]);
            let d = [sq(seg_pts.0, other_pts.0), sq(seg_pts.0, other_pts.1), sq(seg_pts.1, other_pts.0), sq(seg_pts.1, other_pts.1)];
            match argmin4(d) {
                0 => set_start(&mut shapes[seg_i], other_pts.0),
                1 => set_start(&mut shapes[seg_i], other_pts.1),
                2 => set_end(&mut shapes[seg_i], other_pts.0),
                _ => set_end(&mut shapes[seg_i], other_pts.1),
            }
            true
        }
        (K::Arc, K::Arc) => {
            let d = [sq(e0.0, e1.0), sq(e0.0, e1.1), sq(e0.1, e1.0), sq(e0.1, e1.1)];
            let idx = argmin4(d);
            let (i0, i1) = (idx / 2, idx % 2);
            let p0 = if i0 == 0 { e0.0 } else { e0.1 };
            let p1 = if i1 == 0 { e1.0 } else { e1.1 };
            let middle = ((p0.0 + p1.0) / 2, (p0.1 + p1.1) / 2);
            if i0 == 0 { set_start(&mut shapes[a], middle) } else { set_end(&mut shapes[a], middle) }
            if i1 == 0 { set_start(&mut shapes[b], middle) } else { set_end(&mut shapes[b], middle) }
            true
        }
        (K::Bez, K::Arc) | (K::Arc, K::Bez) => {
            // The curve takes the nearer end point of the arc, its control point following.
            let (bez_i, arc_i) = if k0 == K::Bez { (a, b) } else { (b, a) };
            let bez_pts = ends(&shapes[bez_i]);
            let arc_pts = ends(&shapes[arc_i]);
            let d = [sq(bez_pts.0, arc_pts.0), sq(bez_pts.0, arc_pts.1), sq(bez_pts.1, arc_pts.0), sq(bez_pts.1, arc_pts.1)];
            let idx = argmin4(d);
            let target = if idx % 2 == 0 { arc_pts.0 } else { arc_pts.1 };
            move_bezier_end(&mut shapes[bez_i], idx < 2, target);
            true
        }
        (K::Bez, K::Bez) => {
            let d = [sq(e0.0, e1.0), sq(e0.0, e1.1), sq(e0.1, e1.0), sq(e0.1, e1.1)];
            let idx = argmin4(d);
            let (i0, i1) = (idx / 2, idx % 2);
            let p0 = if i0 == 0 { e0.0 } else { e0.1 };
            let p1 = if i1 == 0 { e1.0 } else { e1.1 };
            let middle = ((p0.0 + p1.0) / 2, (p0.1 + p1.1) / 2);
            move_bezier_end(&mut shapes[a], i0 == 0, middle);
            move_bezier_end(&mut shapes[b], i1 == 0, middle);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(a: (i64, i64), b: (i64, i64)) -> Shape {
        Shape::Segment { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: Nm::from_raw_shim(100_000), filled: false, start: Point { x: Nm::from_raw_shim(a.0), y: Nm::from_raw_shim(a.1) }, end: Point { x: Nm::from_raw_shim(b.0), y: Nm::from_raw_shim(b.1) } }
    }

    fn ends_of(s: &Shape) -> ((i64, i64), (i64, i64)) {
        match s {
            Shape::Segment { start, end, .. } => ((start.x.raw_shim(), start.y.raw_shim()), (end.x.raw_shim(), end.y.raw_shim())),
            _ => panic!(),
        }
    }

    #[test]
    fn two_lines_with_a_small_gap_meet_where_their_lines_cross() {
        // A horizontal line stopping 0.3 mm short of a vertical one's line.
        let mut v = vec![seg((0, 0), (9_700_000, 0)), seg((10_000_000, 100_000), (10_000_000, 5_000_000))];
        connect_pair(&mut v, 0, 1);
        assert_eq!(ends_of(&v[0]).1, (10_000_000, 0));
        assert_eq!(ends_of(&v[1]).0, (10_000_000, 0));
    }

    #[test]
    fn a_walk_closes_every_small_gap_of_a_square_and_leaves_big_ones() {
        // A square of 10 mm with 0.2 mm gaps at three corners and a 5 mm one at the fourth.
        let mut v = vec![seg((0, 0), (9_800_000, 0)), seg((10_000_000, 200_000), (10_000_000, 9_800_000)), seg((9_800_000, 10_000_000), (200_000, 10_000_000)), seg((0, 5_000_000), (0, 0))];
        // (the last side starts at its middle: its end at the origin is the first line's start)
        connect_board_shapes(&mut v, Nm::from_raw_shim(1_000_000));
        let corners: Vec<_> = v.iter().map(ends_of).collect();
        assert_eq!(corners[0].1, (10_000_000, 0), "{corners:?}");
        assert_eq!(corners[1].0, (10_000_000, 0));
        assert_eq!(corners[1].1, (10_000_000, 10_000_000));
        assert_eq!(corners[2].0, (10_000_000, 10_000_000));
        // The fourth corner (0, 10 mm) is 5 mm from the last line's start: not joined.
        assert_ne!(corners[2].1, corners[3].0);
    }

    #[test]
    fn a_gap_wider_than_the_tolerance_is_left_alone() {
        let mut v = vec![seg((0, 0), (9_000_000, 0)), seg((10_000_000, 100_000), (10_000_000, 5_000_000))];
        let before = v.clone();
        connect_board_shapes(&mut v, Nm::from_raw_shim(500_000));
        assert_eq!(v, before);
    }

    #[test]
    fn a_line_takes_the_end_of_the_arc_it_nearly_touches() {
        let arc = Shape::Arc { id: String::new(), layer: "Edge.Cuts".into(), stroke_width: Nm::from_raw_shim(100_000), filled: false, start: Point { x: Nm::from_raw_shim(10_000_000), y: Nm::ZERO }, mid: Point { x: Nm::from_raw_shim(12_000_000), y: Nm::from_raw_shim(2_000_000) }, end: Point { x: Nm::from_raw_shim(10_000_000), y: Nm::from_raw_shim(4_000_000) } };
        let mut v = vec![seg((0, 0), (9_900_000, 100_000)), arc];
        connect_board_shapes(&mut v, Nm::from_raw_shim(1_000_000));
        assert_eq!(ends_of(&v[0]).1, (10_000_000, 0));
    }

    #[test]
    fn the_line_crossing_helpers_follow_seg_intersect() {
        assert_eq!(seg_intersect(((0, 0), (10_000, 10_000)), ((0, 10_000), (10_000, 0)), false), Some((5000, 5000)));
        assert_eq!(seg_intersect(((0, 0), (10_000, 0)), ((0, 5000), (10_000, 5000)), true), None);
        assert!((seg_angle(((0, 0), (10_000, 0)), ((0, 0), (0, 10_000))) - 90.0).abs() < 1e-9);
        assert_eq!(seg_intersect(((0, 0), (4000, 0)), ((6000, 3000), (6000, 9000)), true), Some((6000, 0)));
        assert_eq!(seg_intersect(((0, 0), (4000, 0)), ((6000, 3000), (6000, 9000)), false), None);
    }
}
