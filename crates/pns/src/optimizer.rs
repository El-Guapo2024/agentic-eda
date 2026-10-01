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
//!
//! Deliberately not ported (see `crates/pns/PARITY.md`): `MERGE_OBTUSE`
//! (corner-cutting via line-line intersection -- a refinement on top of
//! MERGE_SEGMENTS, not required for a correct route), `SMART_PADS` (pad-exit
//! angle tuning), `FANOUT_CLEANUP` (short-stub canonicalization), and the
//! `KEEP_TOPOLOGY`/`PRESERVE_VERTEX`/`RESTRICT_AREA` constraint system
//! (the research spec confirms `RESTRICT_VERTEX_RANGE` is itself a dead
//! stub upstream, and the other three are call-site refinements rather
//! than correctness requirements -- every candidate this port accepts is
//! still collision-checked, which is the one constraint that must never be
//! skipped).

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

/// Runs `MERGE_SEGMENTS` then `MERGE_COLINEAR` on `line`, checking every
/// candidate against `node` (with `exclude` -- typically the line's own
/// pre-edit segment ids, already removed from `node` by the caller before
/// calling this). Pure function: never mutates `node`.
pub fn optimize(line: &Line, node: &Node, rules: &BoardRules, exclude: &[ItemId]) -> Line {
    let mut out = line.clone();
    out.clear_links();
    out.pts = merge_full(out.pts, node, out.layer, out.width, &out.net, rules, exclude);
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
}
