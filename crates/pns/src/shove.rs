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
//! - Each obstacle *line* is pushed with a single hull-hug attempt per
//!   pusher segment (chained in sequence, same as upstream's per-segment
//!   hull loop in `ShoveObstacleLine`), not upstream's 3-retry clearance-
//!   expansion x 4-winding-order search. A push that doesn't clear in one
//!   attempt fails this call (falls back to walkaround), rather than
//!   retrying with gradually relaxed geometry.
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
use eda_drc::kimath::Shape;
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

/// Push `obstacle` (a full assembled line) out of the way of `pusher`'s
/// geometry: walk it around one hull per pusher segment, chained in
/// sequence (`ShoveObstacleLine`'s per-segment hull loop). `None` if any
/// segment's hull-hug fails in both windings.
fn push_line(pusher: &Line, obstacle: &Line, rules: &BoardRules) -> Option<Vec<Point>> {
    let mut pts = obstacle.pts.clone();
    let clearance = Node::clearance(rules, &pusher.net, &obstacle.net);
    for (a, b) in pusher.segs() {
        let hull = crate::hull::hull_of(&Shape::Stadium { a, b, r: pusher.width / 2 }, clearance, obstacle.width);
        if hull.len() < 3 {
            continue;
        }
        let forward = walkaround::walk_around_hull(&pts, &hull, true);
        let backward = walkaround::walk_around_hull(&pts, &hull, false);
        pts = match (forward, backward) {
            (Some(f), Some(b)) if path_len(&b) < path_len(&f) => b,
            (Some(f), Some(_)) => f,
            (Some(f), None) => f,
            (None, Some(b)) => b,
            (None, None) => return None,
        };
    }
    Some(pts)
}

fn path_len(pts: &[Point]) -> f64 {
    pts.windows(2).map(|w| (((w[1].x - w[0].x) as f64).powi(2) + ((w[1].y - w[0].y) as f64).powi(2)).sqrt()).sum()
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
                let new_pts = push_line(&line, &obstacle_line, rules)?;
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
