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
fn merge_step(pts: &[eda_model::ir::Point], step: usize, node: &Node, layer: i32, width: Um, net: &crate::item::Net, rules: &BoardRules, exclude: &[ItemId]) -> Option<Vec<eda_model::ir::Point>> {
    if pts.len() < step + 3 {
        return None;
    }
    let seg_count = pts.len() - 1;
    let cost_orig = corner_cost(pts);
    for n in 0..=(seg_count - step - 1) {
        let (p_a, p_b) = (pts[n], pts[n + step + 1]);
        let mut best: Option<(Vec<eda_model::ir::Point>, i64)> = None;
        for start_diagonal in [false, true] {
            let bypass = Direction45::Undefined.build_initial_trace(p_a, p_b, start_diagonal, CornerMode::Mitered45);
            let mut candidate = pts[..n].to_vec();
            candidate.extend(bypass);
            candidate.extend(pts[(n + step + 2)..].to_vec());
            if collides(node, &candidate, layer, width, net, rules, exclude) {
                continue;
            }
            let cost = corner_cost(&candidate);
            if cost < cost_orig && best.as_ref().map(|(_, c)| cost < *c).unwrap_or(true) {
                best = Some((candidate, cost));
            }
        }
        if let Some((candidate, _)) = best {
            return Some(candidate);
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
fn merge_full(mut pts: Vec<eda_model::ir::Point>, node: &Node, layer: i32, width: Um, net: &crate::item::Net, rules: &BoardRules, exclude: &[ItemId]) -> Vec<eda_model::ir::Point> {
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
        match merge_step(&pts, step, node, layer, width, net, rules, exclude) {
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
    let mut out = line.clone();
    out.clear_links();
    out.pts = merge_full(out.pts, node, out.layer, out.width, &out.net, rules, exclude);
    out.pts = merge_obtuse(out.pts, node, out.layer, out.width, &out.net, rules, exclude);
    out.simplify();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::net_of;
    use eda_model::ir::Point;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
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

    #[test]
    fn never_introduces_a_collision() {
        use crate::item::{Item, Solid};
        use crate::layer::LayerRange;
        let mut node = Node::new();
        // A pad directly in the path of the straight-line shortcut between
        // the line's endpoints, but clear of the detour the line already
        // takes around it.
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 1500, y: 0 }, shape: Shape::Circle { c: Point { x: 1500, y: 0 }, r: 400 }, source: "U1.1".into() }));
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 20_000, y: 0 }, shape: Shape::Circle { c: Point { x: 20_000, y: 0 }, r: 400 }, source: "U1.1".into() }));
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
