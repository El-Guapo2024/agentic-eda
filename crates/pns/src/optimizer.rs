//! Port of `PNS::OPTIMIZER` (`pcbnew/router/pns_optimizer.{h,cpp}`), scoped
//! to the two passes every plain interactive route and post-shove cleanup
//! actually uses (per the task-research spec this was ported from):
//!
//! - **`MERGE_SEGMENTS`** (`mergeFull`/`mergeStep` below): repeatedly looks
//!   for a shortcut between two non-adjacent segments that (a) doesn't
//!   collide and (b) strictly lowers the line's total corner cost, biggest
//!   shortcut-span first, shrinking the span on failure. This is the pass
//!   that turns a jagged shoved/walked line back into a clean minimal-
//!   corner path.
//! - **`MERGE_COLINEAR`**: dropping a vertex between two exactly-collinear
//!   segments -- already implemented as [`crate::line::Line::simplify`]
//!   (same single left-to-right sweep KiCad's own `mergeColinear` does, see
//!   that method's doc comment), reused here rather than duplicated.
//! - **`MERGE_OBTUSE`** (`merge_obtuse` below): a *different* shortcut
//!   strategy from `MERGE_SEGMENTS`, run right after it (same order as
//!   upstream's own `Optimize()` driver: `mergeFull` -> `mergeObtuse` ->
//!   `mergeColinear`). Where `MERGE_SEGMENTS` hunts for *any* collision-
//!   free, lower-total-cost 45-degree bypass between two points on the
//!   line (at most a 2-segment replacement, from
//!   [`crate::direction45::Direction45::build_initial_trace`]),
//!   `MERGE_OBTUSE` only ever *extends two existing obtuse segments'
//!   already-established directions* to their natural meeting point --
//!   geometrically narrower, but able to collapse an arbitrarily long
//!   "staircase" of small obtuse zigzags (common after a walkaround) into
//!   a single new corner in one step, and without `MERGE_SEGMENTS`'s
//!   "total line cost" comparison ever getting in the way (the research
//!   spec's own confirmation that both passes are part of the plain
//!   interactive route's default effort level, not redundant with each
//!   other).
//!
//! Deliberately not ported (see `crates/pns/PARITY.md`): `SMART_PADS`
//! (pad-exit angle tuning), `FANOUT_CLEANUP` (short-stub canonicalization),
//! and the `KEEP_TOPOLOGY`/`PRESERVE_VERTEX`/`RESTRICT_AREA` constraint
//! system (the research spec confirms `RESTRICT_VERTEX_RANGE` is itself a
//! dead stub upstream, and the other three are call-site refinements
//! rather than correctness requirements -- every candidate this port
//! accepts is still collision-checked, which is the one constraint that
//! must never be skipped).

use crate::direction45::{AngleType, CornerMode, Direction45};
use crate::item::ItemId;
use crate::line::Line;
use crate::node::Node;
use eda_drc::kimath::Shape;
use eda_model::BoardRules;

/// `COST_ESTIMATOR::CornerCost` table (`pns_optimizer.cpp`).
fn corner_cost_of(angle: AngleType) -> i64 {
    match angle {
        AngleType::Straight => 5,
        AngleType::Obtuse => 10,
        AngleType::Right => 30,
        AngleType::Acute => 50,
        AngleType::HalfFull => 60,
        AngleType::Undefined => 100,
    }
}

fn corner_cost(pts: &[eda_model::ir::Point]) -> i64 {
    if pts.len() < 3 {
        return 0;
    }
    let dirs: Vec<Direction45> = pts.windows(2).map(|w| Direction45::from_seg(w[0], w[1])).collect();
    dirs.windows(2).map(|w| corner_cost_of(w[0].angle_to(&w[1]))).sum()
}

fn collides(node: &Node, pts: &[eda_model::ir::Point], layer: i32, width: Um, net: &crate::item::Net, rules: &BoardRules, exclude: &[ItemId]) -> bool {
    use crate::layer::LayerRange;
    pts.windows(2).any(|w| {
        let shape = Shape::Stadium { a: w[0], b: w[1], r: width / 2 };
        node.first_colliding(&shape, net, LayerRange::single(layer), rules, exclude).is_some()
    })
}

use eda_model::ir::Um;

/// One `mergeStep`: try every start position at shortcut span `step`
/// (segments `n` and `n+step`), accept the first collision-free candidate
/// that strictly lowers corner cost. Returns the new point list on success.
#[allow(clippy::too_many_arguments)] // mirrors the collision query's own (node, net, layer, width, rules, exclude) parameter set, threaded straight through.
fn merge_step(pts: &[eda_model::ir::Point], step: usize, node: &Node, layer: i32, width: Um, net: &crate::item::Net, rules: &BoardRules, exclude: &[ItemId], area: Option<Area>) -> Option<Vec<eda_model::ir::Point>> {
    if pts.len() < step + 3 {
        return None;
    }
    let seg_count = pts.len() - 1;
    let cost_orig = corner_cost(pts);
    for n in 0..=(seg_count - step - 1) {
        let (p_a, p_b) = (pts[n], pts[n + step + 1]);
        // KiCad `mergeStep`: cost[i] = INT_MAX unless the bypass is valid;
        // each valid candidate is `Simplify2`d before `CornerCost`.
        let mut path: [Option<Vec<eda_model::ir::Point>>; 2] = [None, None];
        let mut cost = [i64::MAX; 2];
        for (i, start_diagonal) in [false, true].into_iter().enumerate() {
            let bypass = Direction45::Undefined.build_initial_trace(p_a, p_b, start_diagonal, CornerMode::Mitered45);
            // `checkColliding( aLine, bypass )`: only the replacement is tested -- the
            // rest of the line is what it already was (and may already violate
            // something, which must not block every merge).
            if collides(node, &bypass, layer, width, net, rules, exclude) {
                continue;
            }
            // `checkConstraints( n, n + step + 1, .. )`
            if let Some(a) = area {
                if !area_allows(a, n, n + step + 1, pts, &bypass) {
                    continue;
                }
            }
            let mut candidate = pts[..n].to_vec();
            candidate.extend(bypass);
            candidate.extend(pts[(n + step + 2)..].to_vec());
            let mut l = Line::from_points(net.clone(), layer, width, candidate);
            l.simplify(); // `Simplify2`
            cost[i] = corner_cost(&l.pts);
            path[i] = Some(l.pts);
        }
        // `cost[0] < cost_orig && cost[0] < cost[1]`, else `cost[1] < cost_orig`.
        if cost[0] < cost_orig && cost[0] < cost[1] {
            return path[0].take();
        } else if cost[1] < cost_orig {
            return path[1].take();
        }
    }
    None
}

/// Intersection point of the two *infinite* lines through `(a1,a2)` and
/// `(b1,b2)` -- `SEG::IntersectLines`. Needed here (unlike
/// `eda_drc::kimath::Seg::intersect`, which only ever reports a crossing
/// within both segments' own finite extent -- the right tool for a real
/// collision test, the wrong one here) because `merge_obtuse`'s two
/// segments are deliberately non-adjacent, with a gap between them: the
/// point where they'd meet if each were extended is exactly what a
/// straightened corner needs. Same cross-product derivation as
/// `Seg::intersect` (see that function's own derivation comment), just
/// without the bounding-box/parameter-range checks that make it a
/// *segment* intersection instead of a *line* one. `None` only for
/// (near-)parallel lines, which can't have a unique meeting point.
///
/// `pub(crate)`: `diff_pair.rs`'s own polyline-offset construction (its own
/// doc comment) needs exactly this same "where would these two lines
/// meet" query to re-join consecutive offset segments at their own
/// mitered corner, and reuses this rather than duplicating it.
pub(crate) fn intersect_lines(a1: eda_model::ir::Point, a2: eda_model::ir::Point, b1: eda_model::ir::Point, b2: eda_model::ir::Point) -> Option<eda_model::ir::Point> {
    let (dir1x, dir1y) = ((a2.x - a1.x) as i128, (a2.y - a1.y) as i128);
    let (dir2x, dir2y) = ((b2.x - b1.x) as i128, (b2.y - b1.y) as i128);
    let det = dir2x * dir1y - dir2y * dir1x; // dir2.Cross(dir1)
    if det == 0 {
        return None;
    }
    let (offx, offy) = ((b1.x - a1.x) as i128, (b1.y - a1.y) as i128);
    let t = dir2x * offy - dir2y * offx; // dir2.Cross(offset); point = a1 + (t/det)*dir1
    let px = a1.x as i128 + (dir1x * t) / det;
    let py = a1.y as i128 + (dir1y * t) / det;
    Some(eda_model::ir::Point { x: px as Um, y: py as Um })
}

/// `OPTIMIZER::mergeObtuse`: see this module's own doc comment for how
/// this differs from [`merge_full`]. `pts.len() < 3` is a safe no-op
/// (matches upstream's own `step < 0` early return -- `PointCount() - 3`
/// would already be negative there). Mirrors upstream's exact loop shape:
/// start at the widest span (`PointCount() - 3`, i.e. "first and last
/// segment"), shrink by 1 on a span that finds nothing, finalize once the
/// span drops below 2 (comparing two *adjacent* segments isn't a gap this
/// pass is for -- `MERGE_COLINEAR` already owns that case).
#[allow(clippy::too_many_arguments)] // same query-context parameter set every other pass in this module threads through unchanged.
fn merge_obtuse(pts: Vec<eda_model::ir::Point>, node: &Node, layer: i32, width: Um, net: &crate::item::Net, rules: &BoardRules, exclude: &[ItemId]) -> Vec<eda_model::ir::Point> {
    if pts.len() < 3 {
        return pts;
    }
    let mut current = pts;
    let mut step = current.len().saturating_sub(3);
    loop {
        let n_segs = current.len() - 1;
        let max_step = n_segs.saturating_sub(2);
        step = step.min(max_step);
        if step < 2 {
            return current;
        }
        let mut found = false;
        for n in 0..(n_segs - step) {
            let (s1a, s1b) = (current[n], current[n + 1]);
            let (s2a, s2b) = (current[n + step], current[n + step + 1]);
            if Direction45::from_seg(s1a, s1b).angle_to(&Direction45::from_seg(s2a, s2b)) != AngleType::Obtuse {
                continue;
            }
            let Some(ip) = intersect_lines(s1a, s1b, s2a, s2b) else { continue };
            // The new corners this would create must *still* be obtuse --
            // i.e. a genuine straightening, not swapping one kind of
            // zigzag for a sharper one.
            if Direction45::from_seg(s1a, ip).angle_to(&Direction45::from_seg(ip, s2b)) != AngleType::Obtuse {
                continue;
            }
            let mut candidate = current[..=n].to_vec();
            candidate.push(ip);
            candidate.extend_from_slice(&current[(n + step + 1)..]);
            if collides(node, &candidate, layer, width, net, rules, exclude) {
                continue;
            }
            current = candidate;
            found = true;
            break; // retry at the same (still-wide) span from scratch
        }
        if !found {
            if step <= 2 {
                return current;
            }
            step -= 1;
        }
    }
}

/// `mergeFull`: the outer shrinking-span loop around [`merge_step`].
#[allow(clippy::too_many_arguments)] // same query-context parameter set every other pass in this module threads through unchanged.
fn merge_full(pts: Vec<eda_model::ir::Point>, node: &Node, layer: i32, width: Um, net: &crate::item::Net, rules: &BoardRules, exclude: &[ItemId], area: Option<Area>) -> Vec<eda_model::ir::Point> {
    if pts.len() < 3 {
        return pts;
    }
    // KiCad `mergeFull`: `line.Simplify2()` before the search.
    let mut simplified = Line::from_points(net.clone(), layer, width, pts);
    simplified.simplify();
    let mut pts = simplified.pts;
    if pts.len() < 3 {
        return pts;
    }
    let mut step = pts.len().saturating_sub(2); // segCount - 1
    loop {
        let max_step = pts.len().saturating_sub(3); // segCount - 2, as a span index
        step = step.min(max_step);
        if step < 1 || pts.len() < 3 {
            break;
        }
        match merge_step(&pts, step, node, layer, width, net, rules, exclude, area) {
            Some(new_pts) => pts = new_pts, // success: retry at the same (large) span
            None => step -= 1,              // failure: shrink the search radius
        }
    }
    pts
}

/// Runs `MERGE_SEGMENTS`, then `MERGE_OBTUSE`, then `MERGE_COLINEAR` on
/// `line` (upstream's own pass order), checking every candidate against
/// `node` (with `exclude` -- typically the line's own pre-edit segment
/// ids, already removed from `node` by the caller before calling this).
/// Pure function: never mutates `node`.
pub fn optimize(line: &Line, node: &Node, rules: &BoardRules, exclude: &[ItemId]) -> Line {
    optimize_with(line, node, rules, exclude, effort::MERGE_SEGMENTS | effort::MERGE_OBTUSE)
}

/// `OPTIMIZER` effort flags (`pns_optimizer.h`).
pub mod effort {
    pub const MERGE_SEGMENTS: u32 = 0x01;
    pub const SMART_PADS: u32 = 0x02;
    pub const MERGE_OBTUSE: u32 = 0x04;
    pub const FANOUT_CLEANUP: u32 = 0x08;
    pub const MERGE_COLINEAR: u32 = 0x80;
}

/// `OPTIMIZER::Optimize( const LINE*, LINE*, LINE* )`'s pass order, for
/// the passes this crate ports: merge segments, merge obtuse, merge
/// collinear, smart pads, fanout cleanup.
pub fn optimize_with(line: &Line, node: &Node, rules: &BoardRules, exclude: &[ItemId], flags: u32) -> Line {
    optimize_in_area(line, node, rules, exclude, flags, None)
}

/// `OPTIMIZER::SetRestrictArea( area, false )` + `RESTRICT_AREA`: `x0, y0, x1, y1`.
pub type Area = (Um, Um, Um, Um);

/// `AREA_CONSTRAINT::Check` (`pns_optimizer.cpp:~210`), non-strict: a shortcut
/// replacing vertices `v1..=v2` of `path` with `replacement` is allowed when
/// both ends lie in the area, or when one end is outside but the replacement
/// leaves the outside leg's heading alone (so the part of the line the shove
/// never touched keeps its shape).
fn area_allows(area: Area, v1: usize, v2: usize, path: &[eda_model::ir::Point], replacement: &[eda_model::ir::Point]) -> bool {
    let inside = |p: eda_model::ir::Point| p.x >= area.0 && p.x <= area.2 && p.y >= area.1 && p.y <= area.3;
    let parallel = |a: (eda_model::ir::Point, eda_model::ir::Point), b: (eda_model::ir::Point, eda_model::ir::Point)| (a.1.x - a.0.x) as i128 * (b.1.y - b.0.y) as i128 - (a.1.y - a.0.y) as i128 * (b.1.x - b.0.x) as i128 == 0;
    let (p1_in, p2_in) = (inside(path[v1]), inside(path[v2]));
    if p1_in && p2_in {
        return true;
    }
    if v1 < path.len() - 1 && !p1_in && p2_in && inside(path[v1 + 1]) {
        return parallel((replacement[0], replacement[1]), (path[v1], path[v1 + 1]));
    }
    if p1_in && !p2_in && v2 >= 1 && inside(path[v2 - 1]) {
        let n = replacement.len();
        return parallel((replacement[n - 2], replacement[n - 1]), (path[v2 - 1], path[v2]));
    }
    false
}

/// [`optimize_with`], with `RESTRICT_AREA` when `area` is given.
pub fn optimize_in_area(line: &Line, node: &Node, rules: &BoardRules, exclude: &[ItemId], flags: u32, area: Option<Area>) -> Line {
    let mut out = line.clone();
    out.clear_links();
    if flags & effort::MERGE_SEGMENTS != 0 {
        out.pts = merge_full(out.pts, node, out.layer, out.width, &out.net, rules, exclude, area);
    }
    if flags & effort::MERGE_OBTUSE != 0 {
        out.pts = merge_obtuse(out.pts, node, out.layer, out.width, &out.net, rules, exclude);
    }
    if flags & effort::MERGE_COLINEAR != 0 {
        merge_colinear(&mut out.pts);
    }
    if flags & effort::SMART_PADS != 0 {
        run_smart_pads(&mut out, node, rules, exclude);
    }
    if flags & effort::FANOUT_CLEANUP != 0 {
        fanout_cleanup(&mut out, node, rules, exclude);
    }
    out.simplify();
    out
}

/// `OPTIMIZER::mergeColinear`: drop the shared vertex of two collinear,
/// non-degenerate consecutive segments.
fn merge_colinear(pts: &mut Vec<eda_model::ir::Point>) {
    let mut i = 0;
    while i + 2 < pts.len() {
        let (a, b, c) = (pts[i], pts[i + 1], pts[i + 2]);
        let degenerate = a == b || b == c;
        let cross = (b.x - a.x) as i128 * (c.y - a.y) as i128 - (b.y - a.y) as i128 * (c.x - a.x) as i128;
        if !degenerate && cross == 0 {
            pts.remove(i + 1);
        } else {
            i += 1;
        }
    }
}

/// `DIRECTION_45::ANG_ACUTE | ANG_RIGHT | ANG_HALF_FULL | ANG_UNDEFINED`.
fn forbidden(a: AngleType) -> bool {
    matches!(a, AngleType::Acute | AngleType::Right | AngleType::HalfFull | AngleType::Undefined)
}

/// `LINE::CountCorners( forbidden )`.
fn count_forbidden_corners(pts: &[eda_model::ir::Point]) -> usize {
    let dirs: Vec<Direction45> = pts.windows(2).filter(|w| w[0] != w[1]).map(|w| Direction45::from_seg(w[0], w[1])).collect();
    dirs.windows(2).filter(|w| forbidden(w[0].angle_to(&w[1]))).count()
}

fn chain_len(pts: &[eda_model::ir::Point]) -> i64 {
    pts.windows(2).map(|w| (((w[1].x - w[0].x) as f64).hypot((w[1].y - w[0].y) as f64)) as i64).sum()
}

/// `SHAPE_LINE_CHAIN::Append( chain )`: skip a duplicate joining point.
fn append(v: &mut Vec<eda_model::ir::Point>, more: &[eda_model::ir::Point]) {
    for &p in more {
        if v.last() != Some(&p) {
            v.push(p);
        }
    }
}

/// `OPTIMIZER::computeBreakouts` for a pad: `rectBreakouts` for a plain
/// rectangle (and an oval, via `ApproximateSegmentAsRect`),
/// `circleBreakouts` for a circle, `customBreakouts` for a polygon. A
/// rounded rectangle reaches KiCad's router as a compound shape, which
/// has no breakouts -- so none here either.
fn breakouts(width: Um, item: &crate::item::Item) -> Vec<Vec<eda_model::ir::Point>> {
    use eda_model::ir::Point;
    let crate::item::Item::Solid(solid) = item else { return Vec::new() };
    let rect = |x0: Um, y0: Um, x1: Um, y1: Um| -> Vec<Vec<Point>> {
        let (sx, sy) = (x1 - x0, y1 - y0);
        let c = Point { x: x0 + sx / 2, y: y0 + sy / 2 };
        let add = |p: Point, d: (Um, Um)| Point { x: p.x + d.0, y: p.y + d.1 };
        let d_off = (if sx > sy { (sx - sy) / 2 } else { 0 }, if sx < sy { (sy - sx) / 2 } else { 0 });
        let neg = |d: (Um, Um)| (-d.0, -d.1);
        let (dv, dh) = ((0, sy / 2 + width), (sx / 2 + width, 0));
        let mut out = vec![vec![c, add(c, dh)], vec![c, add(c, neg(dh))], vec![c, add(c, dv)], vec![c, add(c, neg(dv))]];
        let l = width + sx.min(sy) / 2;
        let (cp, cm) = (add(c, d_off), add(c, neg(d_off)));
        if sx >= sy {
            out.push(vec![c, cp, add(cp, (l, l))]);
            out.push(vec![c, cp, add(cp, (l, -l))]);
            out.push(vec![c, cm, add(cm, (-l, l))]);
            out.push(vec![c, cm, add(cm, (-l, -l))]);
        } else {
            out.push(vec![c, cp, add(cp, (l, l))]);
            out.push(vec![c, cm, add(cm, (l, -l))]);
            out.push(vec![c, cp, add(cp, (-l, l))]);
            out.push(vec![c, cm, add(cm, (-l, -l))]);
        }
        for b in &mut out {
            b.dedup();
        }
        out
    };
    match solid.shape {
        Shape::Rect { x0, y0, x1, y1 } => rect(x0, y0, x1, y1),
        Shape::Stadium { a, b, r } => {
            // `ApproximateSegmentAsRect`.
            let (p0, p1) = (Point { x: a.x - r, y: a.y - r }, Point { x: b.x + r, y: b.y + r });
            rect(p0.x.min(p1.x), p0.y.min(p1.y), p0.x.max(p1.x), p0.y.max(p1.y))
        }
        Shape::Circle { c, r } => (0..8)
            .map(|i| {
                let a = i as f64 * std::f64::consts::FRAC_PI_4;
                let l = r as f64 * std::f64::consts::SQRT_2;
                vec![c, Point { x: c.x + (l * a.cos()).round() as Um, y: c.y + (l * a.sin()).round() as Um }]
            })
            .collect(),
        Shape::Polygon { ref pts } => {
            let (x0, x1) = (pts.iter().map(|p| p.x).min().unwrap_or(0), pts.iter().map(|p| p.x).max().unwrap_or(0));
            let (y0, y1) = (pts.iter().map(|p| p.y).min().unwrap_or(0), pts.iter().map(|p| p.y).max().unwrap_or(0));
            let p0 = solid.pos;
            let length = (x1 - x0).max(y1 - y0) / 2 + 1;
            let n = pts.len();
            (0..8)
                .filter_map(|i| {
                    let a = i as f64 * std::f64::consts::FRAC_PI_4;
                    let v0 = Point { x: p0.x + (length as f64 * a.cos()).round() as Um, y: p0.y + (length as f64 * a.sin()).round() as Um };
                    let ray = eda_drc::kimath::Seg::new(p0, v0);
                    (0..n).find_map(|k| ray.intersect(&eda_drc::kimath::Seg::new(pts[k], pts[(k + 1) % n]))).map(|hit| vec![p0, hit])
                })
                .collect()
        }
        _ => Vec::new(),
    }
}

/// `OPTIMIZER::findPadOrVia`: the pad or via of this net at `p` on `layer`.
fn find_pad_or_via<'a>(node: &'a Node, layer: i32, net: &crate::item::Net, p: eda_model::ir::Point) -> Option<&'a crate::item::Item> {
    let j = node.joint_at(p, net)?;
    j.links.iter().filter_map(|&id| node.get(id)).find(|it| matches!(it, crate::item::Item::Solid(_) | crate::item::Item::Via(_)) && it.layers().overlaps(&crate::layer::LayerRange::single(layer)))
}

/// `OPTIMIZER::smartPadsSingle`: try leaving the pad along each breakout,
/// joined by a 45-degree trace to one of the line's first few vertices, and
/// keep the cheapest collision-free variant without a forbidden corner
/// (ties: the longer breakout). Returns the vertex index used, or `None`.
fn smart_pads_single(line: &mut Line, pad: &crate::item::Item, at_end: bool, end_vertex: usize, node: &Node, rules: &BoardRules, exclude: &[ItemId]) -> Option<usize> {
    use crate::layer::LayerRange;
    // "don't do optimization on vias".
    let crate::item::Item::Solid(solid) = pad else { return None };
    let bks = breakouts(line.width, pad);
    let mut pts = line.pts.clone();
    if at_end {
        pts.reverse();
    }
    let p_end = end_vertex.min(3.min(pts.len().saturating_sub(1)));
    let mut variants: Vec<(usize, i64, Vec<eda_model::ir::Point>)> = Vec::new();
    for p in 1..=p_end {
        let seg = Shape::Stadium { a: pts[0], b: pts[p], r: line.width / 2 };
        if solid.shape.collides(&seg, 0).is_none() {
            continue;
        }
        for b in &bks {
            for diag in [true, false] {
                let Some(&bl) = b.last() else { continue };
                let connect = Direction45::Undefined.build_initial_trace(bl, pts[p], diag, CornerMode::Mitered45);
                if connect.len() < 2 || b.len() < 2 {
                    continue;
                }
                let d_bk = Direction45::from_seg(b[b.len() - 2], bl);
                if forbidden(d_bk.angle_to(&Direction45::from_seg(connect[0], connect[1]))) {
                    continue;
                }
                if chain_len(b) > chain_len(&pts) {
                    continue;
                }
                let mut v = b.clone();
                append(&mut v, &connect);
                append(&mut v, &pts[(p + 1)..]);
                if count_forbidden_corners(&v) == 0 {
                    if at_end {
                        v.reverse();
                    }
                    let mut tmp = Line::from_points(line.net.clone(), line.layer, line.width, v);
                    tmp.simplify();
                    variants.push((p, chain_len(b), tmp.pts));
                }
            }
        }
    }
    let mut min_cost = corner_cost(&line.pts);
    let mut max_len = 0i64;
    let mut best: Option<(usize, Vec<eda_model::ir::Point>)> = None;
    for (p, len, v) in variants {
        let cost = corner_cost(&v);
        let clear = !v.windows(2).any(|w| {
            let shape = Shape::Stadium { a: w[0], b: w[1], r: line.width / 2 };
            node.first_colliding(&shape, &line.net, LayerRange::single(line.layer), rules, exclude).is_some()
        });
        if clear && (cost < min_cost || (cost == min_cost && len > max_len)) {
            if cost <= min_cost {
                max_len = max_len.max(len);
            }
            min_cost = min_cost.min(cost);
            best = Some((p, v));
        }
    }
    let (p, v) = best?;
    line.pts = v;
    Some(p)
}

/// `OPTIMIZER::runSmartPads`.
fn run_smart_pads(line: &mut Line, node: &Node, rules: &BoardRules, exclude: &[ItemId]) {
    if line.pts.len() < 3 {
        return;
    }
    let (p_start, p_end) = (line.pts[0], *line.pts.last().unwrap());
    let start_pad = find_pad_or_via(node, line.layer, &line.net, p_start).cloned();
    let end_pad = find_pad_or_via(node, line.layer, &line.net, p_end).cloned();
    let mut vtx = None;
    if let Some(sp) = &start_pad {
        vtx = smart_pads_single(line, sp, false, 3, node, rules, exclude);
    }
    if let Some(ep) = &end_pad {
        let n = line.pts.len() - 1;
        smart_pads_single(line, ep, true, vtx.map_or(n, |v| n.saturating_sub(v)), node, rules, exclude);
    }
    line.simplify();
}

/// `OPTIMIZER::fanoutCleanup`: a short (< 10 x width) line from a pad/via
/// to a pad/via (or ending on a via) is replaced by the direct 45-degree
/// trace when that is clear.
fn fanout_cleanup(line: &mut Line, node: &Node, rules: &BoardRules, exclude: &[ItemId]) -> bool {
    if line.pts.len() < 3 {
        return false;
    }
    let (p_start, p_end) = (line.pts[0], *line.pts.last().unwrap());
    if find_pad_or_via(node, line.layer, &line.net, p_start).is_none() {
        return false;
    }
    let end_match = find_pad_or_via(node, line.layer, &line.net, p_end).is_some() || line.via_at_end.is_some();
    if !(end_match && chain_len(&line.pts) < line.width * 10) {
        return false;
    }
    for diag in [false, true] {
        let l2 = Direction45::Undefined.build_initial_trace(p_start, p_end, diag, CornerMode::Mitered45);
        if !collides(node, &l2, line.layer, line.width, &line.net, rules, exclude) {
            line.pts = l2;
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------------------------------------------------
// differential pairs: `OPTIMIZER::Optimize( DIFF_PAIR* )`
// ---------------------------------------------------------------------------------------------------------------------

/// `findCoupledVertices`: the segments of `coupled` that run beside the segment `orig_seg` (of the other line) at the pair's gap, at
/// `vertex`.
fn find_coupled_vertices(vertex: eda_model::ir::Point, orig_seg: crate::seg_ops::Seg2, coupled: &[eda_model::ir::Point], pair: &crate::diff_pair::DiffPair) -> Vec<usize> {
    use crate::diff_pair::{approx_parallel, line_project, vnorm, vsub};
    let mut out = Vec::new();
    for (i, s) in coupled.windows(2).enumerate() {
        let seg = (s[0], s[1]);
        let proj_over_coupled = line_project(seg, vertex);
        if approx_parallel(seg, orig_seg, 1) {
            let dist = vnorm(vsub(proj_over_coupled, vertex)) - pair.width;
            if pair.gap_constraint.matches(dist) {
                out.push(i);
            }
        }
    }
    out
}

/// The answers `NODE::CheckColliding` has given for the segments (and vias) the pair optimizer has asked about so far. The world does
/// not change while a pair is optimized and the candidates it tries share nearly all their segments, so each distinct segment is
/// asked once.
struct HitCache<'a> {
    node: &'a Node,
    rules: &'a BoardRules,
    memo: std::cell::RefCell<std::collections::HashMap<(i64, i64, i64, i64, Um, i32, crate::item::Net), bool, std::hash::BuildHasherDefault<Fnv>>>,
}

/// FNV-1a: the memo is hit by the thousand per preview, and its keys are small and not adversarial.
#[derive(Default)]
struct Fnv(u64);

impl std::hash::Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut h = if self.0 == 0 { 0xcbf2_9ce4_8422_2325 } else { self.0 };
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        self.0 = h;
    }
}

impl<'a> HitCache<'a> {
    fn new(node: &'a Node, rules: &'a BoardRules) -> Self {
        HitCache { node, rules, memo: std::cell::RefCell::new(std::collections::HashMap::default()) }
    }

    /// Whether one segment of a line collides with the world.
    fn segment_hits(&self, a: eda_model::ir::Point, b: eda_model::ir::Point, l: &crate::diff_pair::PairLine) -> bool {
        use crate::node::kind_mask;
        let key = (a.x, a.y, b.x, b.y, l.width, l.layer, l.net.clone());
        let cached = self.memo.borrow().get(&key).copied();
        cached.unwrap_or_else(|| {
            let one = [a, b];
            let hit = crate::diff_pair::PairLine { pts: &one, via: None, ..*l }.hits_node(self.node, self.rules, kind_mask::ANY);
            self.memo.borrow_mut().insert(key, hit);
            hit
        })
    }

    fn via_hits(&self, l: &crate::diff_pair::PairLine) -> bool {
        l.via.is_some_and(|v| crate::diff_pair::PairLine { pts: &[], via: Some(v), ..*l }.hits_node(self.node, self.rules, crate::node::kind_mask::ANY))
    }

    /// Which segments of a line collide with the world, to be reused for the candidates that change only part of it.
    fn chain_hits(&self, l: &crate::diff_pair::PairLine) -> ChainHits {
        let mut prefix = vec![0usize];
        for w in l.pts.windows(2) {
            prefix.push(prefix.last().unwrap() + usize::from(self.segment_hits(w[0], w[1], l)));
        }
        ChainHits { pts: l.pts.to_vec(), prefix }
    }

    /// `NODE::CheckColliding( &line )` (any kind of item): some segment of the line, or its via, is within clearance of the world. `base`
    /// is the line this one is a variation of: the segments both share, at the start and at the end, are not asked about again.
    fn hits_with_base(&self, base: &ChainHits, l: &crate::diff_pair::PairLine) -> bool {
        let (b, c) = (&base.pts, l.pts);
        let lp = b.iter().zip(c.iter()).take_while(|(x, y)| x == y).count();
        let ls = b.iter().rev().zip(c.iter().rev()).take(b.len().min(c.len()) - lp.min(b.len().min(c.len()))).take_while(|(x, y)| x == y).count();
        // segments with both ends in the shared start are the base's own, and so are those with both ends in the shared end
        if lp >= 2 && base.prefix[lp - 1] > 0 {
            return true;
        }
        if ls >= 2 && base.prefix[b.len() - 1] - base.prefix[b.len() - ls] > 0 {
            return true;
        }
        let first_new = lp.saturating_sub(1);
        let end_new = if ls >= 2 { c.len() - ls } else { c.len().saturating_sub(1) };
        if (first_new..end_new).any(|i| self.segment_hits(c[i], c[i + 1], l)) {
            return true;
        }
        self.via_hits(l)
    }
}

/// [`HitCache::chain_hits`]: the vertices of a line, and how many of its segments collide among the first `i` of them.
struct ChainHits {
    pts: Vec<eda_model::ir::Point>,
    prefix: Vec<usize>,
}

/// `verifyDpBypass`: the new reference line and the new coupled one keep the clearance from each other and from the world. `base_ref`
/// and `base_coupled` are the lines the candidates are variations of.
fn verify_dp_bypass(cache: &HitCache, base: (&ChainHits, &ChainHits), pair: &crate::diff_pair::DiffPair, ref_is_p: bool, new_ref: &[eda_model::ir::Point], new_coupled: &[eda_model::ir::Point]) -> bool {
    use crate::diff_pair::PairLine;
    let (ref_base, coupled_base) = if ref_is_p { (pair.p_pair_line(), pair.n_pair_line()) } else { (pair.n_pair_line(), pair.p_pair_line()) };
    let ref_line = PairLine { pts: new_ref, ..ref_base };
    let coupled_line = PairLine { pts: new_coupled, ..coupled_base };
    !(cache.hits_with_base(base.0, &ref_line) || cache.hits_with_base(base.1, &coupled_line) || ref_line.hits_line(&coupled_line, cache.rules))
}

/// `coupledBypass`: a new route for the coupled line that follows the bypass the reference line took -- from a vertex of the coupled line
/// beside the bypass's start to another vertex of it -- picking the one that keeps the most coupled length and that is clear.
///
/// KiCad walks the candidates in order and keeps a clear one only when it is longer than the best so far; so the winner is the clear
/// candidate with the most coupled length, the first of them in that order on a tie. That is also the first clear one when they are tried
/// longest first, which is how it is found here: clearing a candidate costs a walk over both lines, working out its length does not.
fn coupled_bypass(cache: &HitCache, base: (&ChainHits, &ChainHits), pair: &crate::diff_pair::DiffPair, ref_is_p: bool, new_ref: &[eda_model::ir::Point], ref_bypass: &[eda_model::ir::Point], coupled: &[eda_model::ir::Point]) -> Option<Vec<eda_model::ir::Point>> {
    let starts = find_coupled_vertices(ref_bypass[0], (ref_bypass[0], ref_bypass[1]), coupled, pair);
    let dir = Direction45::from_seg(ref_bypass[0], ref_bypass[1]);
    // (coupled length, position in KiCad's order, the coupled line with the bypass in it)
    let mut candidates: Vec<(i64, usize, Vec<eda_model::ir::Point>)> = Vec::new();
    for &si in &starts {
        for j in 1..coupled.len().saturating_sub(1) {
            if si.abs_diff(j) <= 1 {
                continue;
            }
            let vs = coupled[si];
            // `dir.BuildInitialTrace( vs, aCoupled.CPoint(j), dir.IsDiagonal() )`: a defined direction decides the posture itself
            let bypass = dir.build_initial_trace(vs, coupled[j], dir.is_diagonal(), CornerMode::Mitered45);
            let coupled_length = pair.coupled_length_of(new_ref, &bypass);
            let mut new_coupled = coupled.to_vec();
            if si < j {
                crate::line::replace_points(&mut new_coupled, si, j, bypass);
            } else {
                let mut rev = bypass;
                rev.reverse();
                crate::line::replace_points(&mut new_coupled, j, si, rev);
            }
            candidates.push((coupled_length, candidates.len(), new_coupled));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    candidates.into_iter().find(|(_, _, new_coupled)| verify_dp_bypass(cache, base, pair, ref_is_p, new_ref, new_coupled)).map(|(_, _, c)| c)
}

/// `OPTIMIZER::mergeDpStep`: a shortcut between segments `n` and `n + step` of one line of the pair (an obtuse corner pair), with the other
/// line taking the matching shortcut if it can; the first that gives up no more than a tenth of the coupled length wins.
fn merge_dp_step(cache: &HitCache, pair: &mut crate::diff_pair::DiffPair, try_p: bool, step: usize) -> bool {
    let (current_path, coupled_path) = if try_p { (pair.p.clone(), pair.n.clone()) } else { (pair.n.clone(), pair.p.clone()) };
    let (ref_line, coupled_line) = if try_p { (pair.p_pair_line(), pair.n_pair_line()) } else { (pair.n_pair_line(), pair.p_pair_line()) };
    let base = (cache.chain_hits(&ref_line), cache.chain_hits(&coupled_line));
    let n_segs = current_path.len() as i64 - 1 - 1;
    // `CoupledLength( aRef, aCoupled )` is a sum over every segment of the reference with every segment of the coupled line. Each
    // candidate replaces a stretch of the reference, so its coupled length is the total less the replaced segments' share plus the
    // bypass's: the shares are worked out once here, not for every candidate (a walk round a pad cluster leaves tens of segments).
    let share = |a: crate::seg_ops::Seg2| coupled_path.windows(2).map(|w| pair.coupled_length_of_segs(a, (w[0], w[1]))).sum::<i64>();
    let mut prefix = vec![0i64];
    for w in current_path.windows(2) {
        prefix.push(prefix.last().unwrap() + share((w[0], w[1])));
    }
    let clen_pre = *prefix.last().unwrap();
    let budget = clen_pre / 10; // "fixme: come up with something more intelligent here..."
    let mut n = 1usize;
    while (n as i64) < n_segs - step as i64 {
        let (s1, s2) = ((current_path[n], current_path[n + 1]), (current_path[n + step], current_path[n + step + 1]));
        let (dir1, dir2) = (Direction45::from_seg(s1.0, s1.1), Direction45::from_seg(s2.0, s2.1));
        if dir1.angle_to(&dir2) == AngleType::Obtuse {
            let bypass = Direction45::Undefined.build_initial_trace(s1.0, s2.1, dir1.is_diagonal(), CornerMode::Mitered45);
            let mut new_ref = current_path.clone();
            crate::line::replace_points(&mut new_ref, n, n + step, bypass.clone());
            // segments 0..n and n+step+1.. of the reference are as they were; the bypass is new
            let coupled_uni = prefix[n] + (clen_pre - prefix[n + step + 1]) + bypass.windows(2).map(|w| share((w[0], w[1]))).sum::<i64>();
            if cfg!(test) {
                assert_eq!(coupled_uni, pair.coupled_length_of(&new_ref, &coupled_path), "the coupled length of a candidate is the total less what it replaced plus what it adds");
            }
            let delta_uni = coupled_uni - clen_pre + budget;
            if let Some(new_coup) = coupled_bypass(cache, (&base.0, &base.1), pair, try_p, &new_ref, &bypass, &coupled_path) {
                let delta_coupled = pair.coupled_length_of(&new_ref, &new_coup) - clen_pre + budget;
                if delta_coupled >= 0 {
                    let (mut r, mut c) = (Line::from_points(None, 0, 0, new_ref), Line::from_points(None, 0, 0, new_coup));
                    r.simplify(); // `Simplify2`
                    c.simplify();
                    pair.set_shape(r.pts, c.pts, !try_p);
                    return true;
                }
            } else if delta_uni >= 0 && verify_dp_bypass(cache, (&base.0, &base.1), pair, try_p, &new_ref, &coupled_path) {
                let (mut r, mut c) = (Line::from_points(None, 0, 0, new_ref), Line::from_points(None, 0, 0, coupled_path.clone()));
                r.simplify();
                c.simplify();
                pair.set_shape(r.pts, c.pts, !try_p);
                return true;
            }
        }
        n += 1;
    }
    false
}

/// `OPTIMIZER::mergeDpSegments` / `Optimize( DIFF_PAIR* )`: shorten the pair's lines by the shortcuts [`merge_dp_step`] finds, the longest
/// span first, as long as the pair stays clear. `node` is the world the pair is checked against (neither line is in it).
pub fn optimize_pair(node: &Node, rules: &BoardRules, pair: &mut crate::diff_pair::DiffPair) {
    let cache = HitCache::new(node, rules);
    let seg_count = |pts: &[eda_model::ir::Point]| pts.len() as i64 - 1;
    let (mut step_p, mut step_n) = (seg_count(&pair.p) - 2, seg_count(&pair.n) - 2);
    for _ in 0..10_000 {
        let (max_step_p, max_step_n) = (seg_count(&pair.p) - 2, seg_count(&pair.n) - 2);
        step_p = step_p.min(max_step_p);
        step_n = step_n.min(max_step_n);
        if step_p < 1 && step_n < 1 {
            break;
        }
        let (mut found_p, mut found_n) = (false, false);
        if step_p > 1 {
            found_p = merge_dp_step(&cache, pair, true, step_p as usize);
        }
        if step_n > 1 {
            found_n = merge_dp_step(&cache, pair, false, step_n as usize);
        }
        if !found_n && !found_p {
            step_n -= 1;
            step_p -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::net_of;
    use eda_model::ir::Point;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
    }

    /// D11: mergeFull Simplify2s its input, so collinear points are gone
    /// from the result even when nothing else can be merged.
    #[test]
    fn merge_full_simplifies_collinear_points() {
        let node = Node::new();
        let pt = |x, y| Point { x, y };
        let out = merge_full(vec![pt(0, 0), pt(500, 0), pt(1000, 0), pt(1000, 1000)], &node, 0, 200, &net_of("SIG"), &rules(), &[], None);
        assert_eq!(out, vec![pt(0, 0), pt(1000, 0), pt(1000, 1000)]);
    }

    #[test]
    fn smart_pads_leaves_a_wide_pad_along_its_long_axis() {
        use crate::item::{Item, Solid};
        use crate::layer::LayerRange;
        let net = net_of("SIG");
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net.clone(), layers: LayerRange::new(0, 0), pos: Point { x: 0, y: 0 }, shape: Shape::Rect { x0: -1000, y0: -250, x1: 1000, y1: 250 }, source: "U1.1".into(), edge: false }));
        // Drawn leaving the pad centre straight down, then across.
        let line = Line::from_points(net, 0, 200, vec![Point { x: 0, y: 0 }, Point { x: 0, y: 3000 }, Point { x: 5000, y: 3000 }, Point { x: 8000, y: 3000 }]);
        let out = optimize_with(&line, &node, &rules(), &[], effort::SMART_PADS);
        assert_eq!(out.pts[0], Point { x: 0, y: 0 });
        // KiCad prefers the longer breakout on a tie: out along +x (the long axis).
        assert_eq!(out.pts[1].y, 0, "first leg runs along the pad's long axis: {:?}", out.pts);
        assert!(out.pts[1].x > 0);
    }

    #[test]
    fn merge_colinear_drops_a_midpoint() {
        let mut v = vec![Point { x: 0, y: 0 }, Point { x: 500, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1000, y: 500 }];
        merge_colinear(&mut v);
        assert_eq!(v.len(), 3);
    }

    #[test]
    fn rect_pad_has_eight_breakouts_and_circle_eight() {
        use crate::item::{Item, Solid};
        use crate::layer::LayerRange;
        let rect = Item::Solid(Solid { net: None, layers: LayerRange::new(0, 0), pos: Point { x: 0, y: 0 }, shape: Shape::Rect { x0: -500, y0: -250, x1: 500, y1: 250 }, source: "U1.1".into(), edge: false });
        assert_eq!(breakouts(200, &rect).len(), 8);
        let circ = Item::Solid(Solid { net: None, layers: LayerRange::new(0, 0), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 300 }, source: "U1.2".into(), edge: false });
        assert_eq!(breakouts(200, &circ).len(), 8);
    }

    #[test]
    fn merges_a_needless_zigzag_into_a_straight_run() {
        let node = Node::new();
        let rules = rules();
        // A jagged path that could be a single straight line with no
        // obstacles in the way -- optimizer should collapse it.
        let line = Line::from_points(net_of("SIG"), 0, 200, vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1000, y: 500 }, Point { x: 2000, y: 500 }, Point { x: 2000, y: 1000 }, Point { x: 3000, y: 1000 }]);
        let optimized = optimize(&line, &node, &rules, &[]);
        assert!(optimized.segment_count() <= line.segment_count(), "optimizer must never add complexity");
        assert!(corner_cost(&optimized.pts) <= corner_cost(&line.pts));
    }

    /// `RESTRICT_AREA` (`AREA_CONSTRAINT`): a shortcut whose ends are outside the area the shove touched
    /// is not taken; the same line with a big enough area is merged.
    #[test]
    fn a_restricted_area_keeps_the_optimizer_off_the_rest_of_the_line() {
        let node = Node::new();
        let rules = rules();
        let zigzag = vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 1500, y: 500 }, Point { x: 2500, y: 500 }, Point { x: 3000, y: 1000 }, Point { x: 4000, y: 1000 }];
        let line = Line::from_points(net_of("SIG"), 0, 200, zigzag);
        let free = optimize_in_area(&line, &node, &rules, &[], effort::MERGE_SEGMENTS, None);
        assert!(free.pts.len() < line.pts.len(), "unrestricted, it merges: {:?}", free.pts);
        let whole = optimize_in_area(&line, &node, &rules, &[], effort::MERGE_SEGMENTS, Some((-100, -100, 4100, 1100)));
        assert_eq!(whole.pts, free.pts, "an area around the whole line changes nothing");
        let elsewhere = optimize_in_area(&line, &node, &rules, &[], effort::MERGE_SEGMENTS, Some((10_000, 10_000, 11_000, 11_000)));
        assert_eq!(elsewhere.pts, line.pts, "an area the line never enters keeps it as it is");
    }

    /// `checkColliding( aLine, bypass )`: only the bypass is tested, so a line that already violates
    /// something elsewhere can still be shortened where it is clear.
    #[test]
    fn a_violation_elsewhere_on_the_line_does_not_block_a_merge() {
        use crate::item::{Item, Solid};
        use crate::layer::LayerRange;
        let mut node = Node::new();
        // a pad the line's first leg already runs through
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 300 }, source: "U1.1".into(), edge: false }));
        let rules = rules();
        let line = Line::from_points(net_of("SIG"), 0, 200, vec![Point { x: 0, y: 0 }, Point { x: 3000, y: 0 }, Point { x: 3500, y: 500 }, Point { x: 5000, y: 500 }, Point { x: 5500, y: 1000 }, Point { x: 8000, y: 1000 }]);
        let out = optimize_in_area(&line, &node, &rules, &[], effort::MERGE_SEGMENTS, None);
        assert!(out.pts.len() < line.pts.len(), "{:?}", out.pts);
        assert_eq!(out.pts.first(), line.pts.first());
        assert_eq!(out.pts.last(), line.pts.last());
    }

    #[test]
    fn never_introduces_a_collision() {
        use crate::item::{Item, Solid};
        use crate::layer::LayerRange;
        let mut node = Node::new();
        // A pad directly in the path of the straight-line shortcut between
        // the line's endpoints, but clear of the detour the line already
        // takes around it.
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 1500, y: 0 }, shape: Shape::Circle { c: Point { x: 1500, y: 0 }, r: 400 }, source: "U1.1".into(), edge: false }));
        let rules = rules();
        let line = Line::from_points(net_of("SIG"), 0, 200, vec![Point { x: 0, y: 0 }, Point { x: 0, y: 1000 }, Point { x: 3000, y: 1000 }, Point { x: 3000, y: 0 }]);
        assert!(!collides(&node, &line.pts, 0, 200, &net_of("SIG"), &rules, &[]), "test setup: the original detour must itself be collision-free");
        let optimized = optimize(&line, &node, &rules, &[]);
        assert!(!collides(&node, &optimized.pts, 0, 200, &net_of("SIG"), &rules, &[]));
    }

    #[test]
    fn intersect_lines_finds_the_extended_meeting_point_of_two_non_adjacent_segments() {
        // Same two segments `merge_obtuse_collapses_a_two_step_obtuse_zigzag`
        // below relies on -- verified independently here so a failure in
        // that test points at the right cause.
        let p0 = Point { x: 0, y: 0 };
        let p1 = Point { x: 1000, y: 0 };
        let p2 = Point { x: 3000, y: -500 };
        let p3 = Point { x: 4000, y: 500 };
        assert_eq!(intersect_lines(p0, p1, p2, p3), Some(Point { x: 3500, y: 0 }));
        // Parallel lines never meet.
        assert_eq!(intersect_lines(Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }, Point { x: 0, y: 500 }, Point { x: 1000, y: 500 }), None);
    }

    #[test]
    fn merge_obtuse_collapses_a_two_step_obtuse_zigzag() {
        let node = Node::new();
        let rules = rules();
        let net = net_of("SIG");
        // Two segments an octant apart (E, then NE two segments later) whose
        // extended lines meet cleanly at (3500, 0) without creating a worse
        // corner than either original -- MERGE_SEGMENTS' own 2-segment
        // bypass isn't what's being tested here, only the obtuse-extension
        // shortcut (see this module's header comment on why both passes
        // earn their keep).
        let p0 = Point { x: 0, y: 0 };
        let p1 = Point { x: 1000, y: 0 }; // seg0 (p0->p1): E
        let p2 = Point { x: 3000, y: -500 };
        let p3 = Point { x: 4000, y: 500 }; // seg2 (p2->p3): NE
        let p4 = Point { x: 5000, y: 500 };
        let pts = vec![p0, p1, p2, p3, p4];
        let result = merge_obtuse(pts.clone(), &node, 0, 200, &net, &rules, &[]);
        assert!(result.len() < pts.len(), "the two in-between points must collapse into one new corner");
        assert_eq!(result.first(), Some(&p0), "the line's own start must survive untouched");
        assert_eq!(result.last(), Some(&p4), "the line's own end must survive untouched");
        assert!(result.contains(&Point { x: 3500, y: 0 }), "the new corner must be the two segments' own extended meeting point");
    }

    #[test]
    fn merge_obtuse_never_introduces_a_collision() {
        use crate::item::{Item, Solid};
        use crate::layer::LayerRange;
        let mut node = Node::new();
        // The same zigzag as `merge_obtuse_collapses_a_two_step_obtuse_
        // zigzag`, scaled up 10x (direction/obtuse-classification is
        // scale-invariant, only the *margins* needed here change) so a
        // pad can sit far enough from the original detour's own closest
        // approach to it to be genuinely clear, while still sitting
        // exactly on the (0,0)-(35000,0) shortcut the merge would
        // otherwise produce.
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 20_000, y: 0 }, shape: Shape::Circle { c: Point { x: 20_000, y: 0 }, r: 400 }, source: "U1.1".into(), edge: false }));
        let rules = rules();
        let net = net_of("SIG");
        let pts = vec![Point { x: 0, y: 0 }, Point { x: 10_000, y: 0 }, Point { x: 30_000, y: -5_000 }, Point { x: 40_000, y: 5_000 }, Point { x: 50_000, y: 5_000 }];
        assert!(!collides(&node, &pts, 0, 200, &net, &rules, &[]), "test setup: the original zigzag must itself be collision-free");
        let result = merge_obtuse(pts.clone(), &node, 0, 200, &net, &rules, &[]);
        // The only candidate this geometry offers collides, so the honest
        // outcome is "leave it as it was" -- same as upstream's own
        // "found_anything == false at every span" fallback.
        assert_eq!(result, pts, "the blocked merge must be refused outright, not silently applied anyway");
        assert!(!collides(&node, &result, 0, 200, &net, &rules, &[]));
    }
}
