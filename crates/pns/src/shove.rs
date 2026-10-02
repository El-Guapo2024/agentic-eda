//! Port of `PNS::SHOVE` (`pcbnew/router/pns_shove.{h,cpp}`): instead of
//! stopping at (MarkObstacles) or routing around (Walkaround) an obstacle,
//! push it -- and transitively, anything *it* then collides with -- out of
//! the way.
//!
//! Scoped down significantly from upstream (see `PARITY.md` for the full
//! list); the headline simplifications:
//!
//! - **No springback stack.** KiCad keeps a history of previously-shoved
//!   branch nodes so an interactive drag that wiggles the cursor back and
//!   forth can cheaply "un-shove" instead of recomputing from scratch every
//!   tick. This port's frontend drives the backend one HTTP request per
//!   mouse sample, not a continuous tick stream -- there is no "previous
//!   tick's branch" to incrementally reuse, so every call just branches
//!   fresh from the committed world and recomputes.
//! - **A flat worklist with a hard iteration cap, not KiCad's
//!   forward/reverse rank bookkeeping.** KiCad assigns each shoved item a
//!   rank this run so a line that gets pushed back into something that
//!   already pushed *it* this run is forced to route around instead of
//!   re-pushing (avoiding A-pushes-B-pushes-A ping-pong). This port
//!   instead just caps total iterations
//!   (`ROUTING_SETTINGS::ShoveIterationLimit`) and fails (falls back to
//!   walkaround, exactly like upstream's own `SH_INCOMPLETE` ->
//!   `rhWalkOnly` path) if the worklist doesn't drain in time -- simpler,
//!   and failure has the identical observable outcome upstream's
//!   iteration-limit path produces anyway.
//! - **Via push is the direct one-shot MTV displacement** (move the via
//!   directly away from the pusher by the clearance violation plus a
//!   margin, once, no re-verification) -- the same approach KiCad's own
//!   `onCollidingVia`/`pushOrShoveVia` uses for its *own* via push
//!   (confirmed by the task's SHOVE research spec: `VIA::PushoutForce`'s
//!   iterative search is used for via lead-in/dragging, not by SHOVE
//!   itself).
//! - Each obstacle *line* is pushed exactly as `ShoveObstacleLine` /
//!   `shoveLineToHullSet` do: one hull per pusher segment, 4 traversal-
//!   order x winding attempts validated by `checkShoveDirection`, endpoint,
//!   self-intersection and pusher-collision checks, and 3 tries with the
//!   hulls grown 1 µm each (the last may snap via-less ends onto a hull).
//!   The hull walk itself is still our simplified `walk_around_hull`, not a
//!   full `LINE::Walkaround` port.
//!
//! Unlike `walkaround`/`optimizer` (pure functions over a borrowed
//! [`Node`]), this module needs to *mutate* a node -- shoving genuinely
//! changes what's on the board. It branches its own scratch copy
//! internally and returns only the diff: the resolved head, plus every
//! other line/via that had to move. [`crate::line_placer::LinePlacer`]
//! never holds this scratch node itself (see that module's own doc
//! comment on why it can stay stateless) -- it just re-runs shove fresh
//! from the real world plus its own already-fixed runs on every call, and
//! only remembers the *accepted* fix/finish's displaced set for the final
//! commit.

use crate::item::{Item, ItemId, Net, Via};
use crate::line::Line;
use crate::node::Node;
use crate::settings::RoutingSettings;
use crate::walkaround;
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;
use std::collections::HashMap;

/// An existing track this shove displaced -- `source_track` (the original
/// `Track::id`) is `None` only for a line this *same* routing session
/// already placed and then shoved again before committing, which can't
/// happen in the current `LinePlacer` flow (a session's own runs are
/// same-net with the head, so shove never targets them) but is kept
/// `Option` rather than widened to avoid a false guarantee.
#[derive(Debug, Clone)]
pub struct DisplacedLine {
    pub source_track: Option<String>,
    pub line: Line,
}

#[derive(Debug, Clone)]
pub struct DisplacedVia {
    pub source_via: String,
    pub pos: Point,
}

#[derive(Debug, Clone)]
pub struct ShoveOutcome {
    pub head: Vec<Point>,
    pub displaced_lines: Vec<DisplacedLine>,
    pub displaced_vias: Vec<DisplacedVia>,
}

fn source_track_of(node: &Node, line: &Line) -> Option<String> {
    line.segment_ids.iter().find_map(|id| match node.get(*id) {
        Some(Item::Segment(s)) => s.source_track.as_ref().map(|(t, _)| t.clone()),
        _ => None,
    })
}

fn is_locked(node: &Node, line: &Line) -> bool {
    line.segment_ids.iter().any(|id| matches!(node.get(*id), Some(Item::Segment(s)) if s.locked))
}

/// `c_ENDPOINT_ON_HULL_THRESHOLD` (1000 nm) in this IR's µm.
const ENDPOINT_ON_HULL_THRESHOLD: f64 = 1.0;
/// `cHullFailureExpansionFactor` (1000 nm) in µm.
const HULL_FAILURE_EXPANSION: Um = 1;

/// `SHAPE_LINE_CHAIN::POINT_INSIDE_TRACKER` over the closed polygon
/// `obstacle + reverse(shoved)`: is `p` inside the area swept by the move?
fn point_inside_swept(p: Point, obstacle: &[Point], shoved: &[Point]) -> bool {
    let poly: Vec<Point> = obstacle.iter().copied().chain(shoved.iter().rev().copied()).collect();
    crate::walkaround::point_in_polygon(&poly, p)
}

/// `SHOVE::checkShoveDirection`: the obstacle must move *away* from the
/// pusher, i.e. the pusher's start point is not inside the area between
/// the obstacle's old and new shape.
fn check_shove_direction(pusher: &Line, obstacle: &[Point], shoved: &[Point]) -> bool {
    let Some(cp) = pusher.first() else { return true };
    !point_inside_swept(cp, obstacle, shoved)
}

/// `SHAPE_LINE_CHAIN::SelfIntersecting`: any two non-adjacent segments cross.
fn self_intersecting(pts: &[Point]) -> bool {
    let n = pts.len();
    for i in 0..n.saturating_sub(1) {
        for j in (i + 2)..n.saturating_sub(1) {
            let (a, b) = (eda_drc::kimath::Seg::new(pts[i], pts[i + 1]), eda_drc::kimath::Seg::new(pts[j], pts[j + 1]));
            if a.intersect(&b).is_some() {
                // Closed chains touch at the shared endpoint; this chain is open.
                return true;
            }
        }
    }
    false
}

/// `LINE::Collide( &aCurLine, ... )`: any segment pair within clearance.
fn lines_collide(a: &[Point], aw: Um, b: &Line, clearance: Um) -> bool {
    let need = clearance + aw / 2 + b.width / 2;
    let need_sq = (need as i128) * (need as i128);
    a.windows(2).any(|s| b.segs().any(|(c, d)| eda_drc::kimath::Seg::new(s[0], s[1]).sq_distance_to_seg(&eda_drc::kimath::Seg::new(c, d)) < need_sq))
}

/// `SHAPE_LINE_CHAIN::NearestPoint` on a closed hull.
fn hull_nearest(hull: &[Point], p: Point) -> Point {
    (0..hull.len())
        .map(|i| eda_drc::kimath::Seg::new(hull[i], hull[(i + 1) % hull.len()]).nearest_point(p))
        .min_by_key(|q| (q.x - p.x).pow(2) + (q.y - p.y).pow(2))
        .unwrap_or(p)
}

/// `SHOVE::shoveLineToHullSet`: re-walk `obstacle` around every hull in
/// turn, trying the 4 combinations of hull traversal order x winding, and
/// accept the first result that keeps both endpoints, moves away from the
/// pusher, doesn't self-intersect and clears the pusher.
fn shove_line_to_hull_set(pusher: &Line, obstacle: &Line, hulls: &[Vec<Point>], clearance: Um, adjust_start: bool, adjust_end: bool) -> Option<Vec<Point>> {
    for attempt in 0..4 {
        let invert = attempt >= 2;
        let clockwise = attempt % 2 == 1;
        let order: Vec<usize> = if invert { (0..hulls.len()).rev().collect() } else { (0..hulls.len()).collect() };
        let mut obs = obstacle.pts.clone();

        if (adjust_start || adjust_end) && obs.len() >= 2 {
            let min_dist_p = |pref: Point| -> Option<(f64, Point)> {
                let mut best: Option<(f64, Point)> = None;
                for &i in &order {
                    let hull = &hulls[i];
                    let p = hull_nearest(hull, pref);
                    let d = if crate::walkaround::point_in_polygon(hull, pref) { 0.0 } else { (((p.x - pref.x) as f64).powi(2) + ((p.y - pref.y) as f64).powi(2)).sqrt() };
                    if d < ENDPOINT_ON_HULL_THRESHOLD && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, p));
                    }
                }
                best
            };
            let p0 = min_dist_p(obs[0]);
            let p1 = min_dist_p(*obs.last().unwrap());
            if let (Some((_, p)), true) = (p1, adjust_end) {
                obs.push(p);
            }
            if let (Some((_, p)), true) = (p0, adjust_start) {
                obs.insert(0, p);
            }
        }

        let mut path = obs.clone();
        let mut fail = false;
        for &i in &order {
            match walkaround::walk_around_hull(&path, &hulls[i], clockwise) {
                Some(p) => {
                    let mut l = Line::from_points(obstacle.net.clone(), obstacle.layer, obstacle.width, p);
                    l.simplify();
                    path = l.pts;
                }
                None => {
                    fail = true;
                    break;
                }
            }
        }
        if fail || path.len() < 2 {
            continue;
        }
        if path.first() != obs.first() || path.last() != obs.last() {
            continue;
        }
        if !check_shove_direction(pusher, &obs, &path) {
            continue;
        }
        if self_intersecting(&path) {
            continue;
        }
        if lines_collide(&path, obstacle.width, pusher, clearance) {
            continue;
        }
        return Some(path);
    }
    None
}

/// `JOINT::Via()`: a via is linked at this joint.
fn has_via(node: &Node, p: Point, net: &Net) -> bool {
    node.joint_at(p, net).is_some_and(|j| j.links.iter().any(|&id| matches!(node.get(id), Some(Item::Via(_)))))
}

/// `SHOVE::ShoveObstacleLine`: one hull per pusher segment at clearance +
/// the obstacle's width, three tries with the hulls grown by 1 µm each
/// time; the third try may also snap the obstacle's free (via-less) ends
/// onto a hull.
fn push_line(node: &Node, pusher: &Line, obstacle: &Line, rules: &BoardRules) -> Option<Vec<Point>> {
    let clearance = Node::clearance(rules, &pusher.net, &obstacle.net);
    let voe_start = obstacle.via_at_start.is_some() || obstacle.first().is_some_and(|p| has_via(node, p, &obstacle.net));
    let voe_end = obstacle.via_at_end.is_some() || obstacle.last().is_some_and(|p| has_via(node, p, &obstacle.net));
    let mut extra = 0;
    for attempt in 0..3 {
        let hulls: Vec<Vec<Point>> = pusher
            .segs()
            .map(|(a, b)| crate::hull::segment_hull(a, b, pusher.width, clearance + extra + crate::hull::HULL_ROUNDING_GUARD, obstacle.width))
            .filter(|h| h.len() >= 3)
            .collect();
        let (adj_start, adj_end) = (attempt >= 2 && !voe_start, attempt >= 2 && !voe_end);
        if let Some(p) = shove_line_to_hull_set(pusher, obstacle, &hulls, clearance, adj_start, adj_end) {
            return Some(p);
        }
        extra += HULL_FAILURE_EXPANSION;
    }
    None
}

/// `SHOVE::Run`/`ShoveLine`: push whatever `raw` (the candidate head path)
/// collides with out of the way, transitively, until nothing left on the
/// worklist collides or the iteration limit
/// (`ROUTING_SETTINGS::ShoveIterationLimit`) is spent. `None` means the
/// shove failed outright (hit a locked item, a pad, or ran out of
/// iterations) -- the caller should fall back to walkaround, same as
/// upstream's `rhShoveOnly`.
pub fn shove_line(node: &Node, raw: &[Point], net: &Net, layer: i32, width: Um, rules: &BoardRules, settings: &RoutingSettings) -> Option<ShoveOutcome> {
    if raw.len() < 2 {
        return Some(ShoveOutcome { head: raw.to_vec(), displaced_lines: Vec::new(), displaced_vias: Vec::new() });
    }
    let mut work = node.branch();
    let pusher_template = Line::from_points(net.clone(), layer, width, raw.to_vec());
    let pusher_ids = work.add_line(&pusher_template, None, false);
    if pusher_ids.is_empty() {
        return Some(ShoveOutcome { head: raw.to_vec(), displaced_lines: Vec::new(), displaced_vias: Vec::new() });
    }

    let mut stack: Vec<ItemId> = pusher_ids.clone();
    let mut displaced_lines: HashMap<String, Line> = HashMap::new();
    let mut displaced_vias: HashMap<String, Point> = HashMap::new();
    let mut iterations: i32 = 0;

    while let Some(seg_id) = stack.pop() {
        iterations += 1;
        if iterations > settings.shove_iteration_limit {
            return None;
        }
        if !work.contains(seg_id) {
            continue; // superseded by an earlier iteration's remove+re-add
        }
        let Some(line) = work.assemble_line(seg_id) else { continue };
        let obstacles = work.line_colliding(&line, rules, &line.segment_ids);
        let Some((_, obstacle)) = obstacles.into_iter().min_by(|a, b| a.1.actual.cmp(&b.1.actual)) else {
            continue; // this line is settled; nothing left to resolve for it
        };

        match work.get(obstacle.id) {
            Some(Item::Solid(_)) => return None, // can't shove a pad -- walkaround's job
            Some(Item::Via(v)) if v.locked => return None,
            Some(Item::Via(v)) => {
                let via = v.clone();
                let push_len = (obstacle.clearance_needed - obstacle.actual).max(1) + via.diameter / 2;
                let (mut dx, mut dy) = ((via.pos.x - obstacle.pos.x) as f64, (via.pos.y - obstacle.pos.y) as f64);
                let mut mag = (dx * dx + dy * dy).sqrt();
                if mag < 1.0 {
                    // The via sits exactly on the pusher's centreline (its
                    // nearest-point and the via's own centre coincide), so
                    // "away from the collision point" is undefined -- push
                    // perpendicular to the pusher's own first segment
                    // instead, same as KiCad falls back to a line-normal
                    // direction when a lead/trail vector degenerates.
                    let (sx, sy) = line.segs().next().map(|(a, b)| ((b.x - a.x) as f64, (b.y - a.y) as f64)).unwrap_or((1.0, 0.0));
                    (dx, dy) = (-sy, sx);
                    mag = (dx * dx + dy * dy).sqrt().max(1.0);
                }
                let new_pos = Point { x: via.pos.x + (dx / mag * push_len as f64).round() as Um, y: via.pos.y + (dy / mag * push_len as f64).round() as Um };

                let attached: Vec<ItemId> = work.joint_at(via.pos, &via.net).map(|j| j.links.clone()).unwrap_or_default().into_iter().filter(|&id| matches!(work.get(id), Some(Item::Segment(_)))).collect();
                work.remove(obstacle.id);
                work.add(Item::Via(Via { pos: new_pos, ..via.clone() }));
                if let Some(sv) = &via.source_via {
                    displaced_vias.insert(sv.clone(), new_pos);
                }
                for seg in attached {
                    if !work.contains(seg) {
                        continue;
                    }
                    let Some(mut l2) = work.assemble_line(seg) else { continue };
                    if is_locked(&work, &l2) {
                        return None;
                    }
                    let src = source_track_of(&work, &l2);
                    if l2.first() == Some(via.pos) {
                        l2.pts[0] = new_pos;
                    }
                    if l2.last() == Some(via.pos) {
                        let last = l2.pts.len() - 1;
                        l2.pts[last] = new_pos;
                    }
                    work.remove_line_segments(&l2);
                    let new_ids = work.add_line(&l2, src.clone().map(|t| (t, 0)), false);
                    if let Some(t) = src {
                        displaced_lines.insert(t, l2);
                    }
                    stack.extend(new_ids);
                }
                stack.push(seg_id); // the pusher itself may still be close to the via's new spot
            }
            Some(Item::Segment(_)) => {
                let obstacle_line = work.assemble_line(obstacle.id)?;
                if is_locked(&work, &obstacle_line) {
                    return None;
                }
                let new_pts = push_line(&work, &line, &obstacle_line, rules)?;
                let src = source_track_of(&work, &obstacle_line);
                let mut new_line = obstacle_line.clone();
                new_line.pts = new_pts;
                new_line.clear_links();
                work.remove_line_segments(&obstacle_line);
                let new_ids = work.add_line(&new_line, src.clone().map(|t| (t, 0)), false);
                if let Some(t) = src {
                    displaced_lines.insert(t, new_line);
                }
                stack.push(seg_id);
                stack.extend(new_ids);
            }
            _ => return None,
        }
    }

    let head = pusher_ids.iter().find_map(|&id| work.assemble_line(id)).map(|l| l.pts).unwrap_or_else(|| raw.to_vec());
    Some(ShoveOutcome {
        head,
        displaced_lines: displaced_lines.into_iter().map(|(source_track, line)| DisplacedLine { source_track: Some(source_track), line }).collect(),
        displaced_vias: displaced_vias.into_iter().map(|(source_via, pos)| DisplacedVia { source_via, pos }).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eda_drc::kimath::Shape;
    use crate::item::{net_of, Segment};
    use crate::layer::LayerRange;
    use eda_model::ir::Point;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
    }

    #[test]
    fn pushes_a_crossing_track_out_of_the_way() {
        let mut node = Node::new();
        // An existing GND track running vertically through where the new
        // SIG route needs to go horizontally.
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: -2000 }, b: Point { x: 2500, y: 2000 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let raw = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        let outcome = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules, &settings).expect("a single crossing track must be shovable");
        assert_eq!(outcome.head, raw, "the pusher itself goes straight through; only the obstacle moves");
        assert_eq!(outcome.displaced_lines.len(), 1);
        assert_eq!(outcome.displaced_lines[0].source_track.as_deref(), Some("trkA"));
        // The displaced track must actually clear the new route's own
        // geometry (the pusher's path at its own width), not just not be
        // exactly where it started.
        let moved = &outcome.displaced_lines[0].line;
        for pusher_leg in raw.windows(2) {
            let pusher_shape = Shape::Stadium { a: pusher_leg[0], b: pusher_leg[1], r: 100 };
            for moved_leg in moved.segs() {
                let moved_shape = Shape::Stadium { a: moved_leg.0, b: moved_leg.1, r: 100 };
                assert!(pusher_shape.collides(&moved_shape, rules.clearance_of("SIG")).is_none(), "shoved track still too close to the new route");
            }
        }
        // Endpoints of the shoved track must be preserved (shove never
        // disconnects anything it moves).
        assert_eq!(moved.first(), Some(Point { x: 2500, y: -2000 }));
        assert_eq!(moved.last(), Some(Point { x: 2500, y: 2000 }));
    }

    #[test]
    fn shove_direction_rejects_moving_toward_the_pusher() {
        // Obstacle along y=0; pusher starts above it at y=500. Shoving the
        // obstacle up past the pusher's start puts that start inside the
        // swept area -- the wrong way.
        let pusher = Line::from_points(None, 0, 100, vec![Point { x: 500, y: 500 }, Point { x: 500, y: -500 }]);
        let obs = vec![Point { x: 0, y: 0 }, Point { x: 1000, y: 0 }];
        let up = vec![Point { x: 0, y: 0 }, Point { x: 0, y: 800 }, Point { x: 1000, y: 800 }, Point { x: 1000, y: 0 }];
        let down = vec![Point { x: 0, y: 0 }, Point { x: 0, y: -800 }, Point { x: 1000, y: -800 }, Point { x: 1000, y: 0 }];
        assert!(!check_shove_direction(&pusher, &obs, &up));
        assert!(check_shove_direction(&pusher, &obs, &down));
    }

    #[test]
    fn refuses_to_shove_a_locked_track() {
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: -2000 }, b: Point { x: 2500, y: 2000 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: true }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let raw = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        assert!(shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules, &settings).is_none());
    }

    #[test]
    fn pushes_a_stitching_via_aside() {
        let mut node = Node::new();
        node.add(Item::Via(Via { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 2500, y: 0 }, diameter: 600, drill: 300, source_via: Some("viaA".into()), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let raw = vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }];
        let outcome = shove_line(&node, &raw, &net_of("SIG"), 0, 200, &rules, &settings).expect("a lone via must be shovable");
        assert_eq!(outcome.displaced_vias.len(), 1);
        assert_eq!(outcome.displaced_vias[0].source_via, "viaA");
        assert_ne!(outcome.displaced_vias[0].pos, Point { x: 2500, y: 0 }, "the via must actually move");
    }
}
