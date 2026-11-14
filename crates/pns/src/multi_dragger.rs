//! Port of `PNS::MULTI_DRAGGER` (`pcbnew/router/pns_multi_dragger.{h,cpp}` at 8303b2ad): `D` on several selected tracks
//! (`ROUTER_TOOL::InlineDrag` with a selection of tracks, `ROUTER::StartDragging( aP, ITEM_SET )` of more than one `SEGMENT_T | ARC_T`). KiCad
//! calls it "Very trival version for demonstration purposes": every line a selected track is part of is dragged with the one under the cursor
//! (the "primary" line), each kept at the distance it had from it.
//!
//! ## `Start` ([`MultiDragger::start`])
//!
//! - One line per selected track (`m_world->AssembleLine( litem )`), a track whose line is already there adding to its `originalLeaders`.
//! - Each line is a **corner** candidate when one of its ends is an end of a selected segment (`aPrimitives.FindVertex`), the nearer such end
//!   when both are (`cornerIsLast`), and **strict** when the cursor is within half the line's width of it.
//! - It is a **segment** candidate (`midSeg`, `leaderSegIndex`) for each of its segments that is selected, the last of them winning, and strict
//!   when the cursor is within half the width of that segment and it was not already a strict corner.
//! - A strict corner anywhere makes the drag `DM_CORNER`, else a strict segment `DM_SEGMENT`, else the nearer of the best corner and the best
//!   segment by `cornerDistance` / `leaderSegDistance`, whose line is the primary.
//! - In `DM_CORNER` every line is turned so that its corner is its last point (`Reverse()`), and a corner that is not a trivial end (a joint
//!   with one segment, `IsTrivialEndpoint`) makes the drag `DM_SEGMENT` (the lines stay turned, `leaderSegIndex` is not: KiCad's).
//!
//! ## `Drag` ([`MultiDragger::drag`])
//!
//! `tryPosture( 0..2 )` until one works: the primary line is dragged to the cursor -- its last corner (`DragCorner`, with the snap threshold
//! "Smooth dragged segments" gives) or its leader segment (`DragSegment`) -- and every other line by the same, projected onto the
//! perpendicular of the primary's last (or leader) segment at its own distance from it (`lastPreDrag.LineDistance( .., true )`,
//! `perp.Resize( dist )`): a corner drag only of a line whose last segment runs at an obtuse, right or straight angle to the primary's, ending
//! in the primary's last direction where it can (`DragCorner( .., primaryLastSegDir )`); a segment drag only of a line whose leader segment is
//! parallel to the primary's (`ANG_HALF_FULL | ANG_STRAIGHT`). Variant 1 drops the last point of every line first, variant 2 takes the
//! primary's last segment before the drag as the guide; a corner posture works when every line ends in the primary's last direction.
//! Then the mode:
//! - Highlight collisions (`multidragMarkObstacles`): each line is cut back where it would run into a line before it (`clipToOtherLine`, a
//!   binary search to 100 nm), and the drag is always good (`m_dragStatus = true`: a release commits, collisions and all);
//! - Walk around (`multidragWalkaround`): the lines, in the order of `dragDist`, are walked one by one round what they run into (and the lines
//!   walked before them) in a world without the dragged lines (`WP_SHORTEST`, the walkaround iteration limit, three times the length), then in
//!   the reverse order; the attempt that works and lengthens the lines least is the one taken;
//! - Shove (`multidragShove`): the lines are the heads of the gesture's one `SHOVE` (`m_shove`, over `m_preShoveNode`, the world without the
//!   dragged lines), added as `AddHeads( l.draggedLine, SHP_SHOVE | SHP_DONT_OPTIMIZE )`: no `SHP_DONT_LOCK_ENDPOINTS`, so `SHOVE::Run` locks the
//!   joints at each head's ends (`LockJoint`), and its springback stack carries what the positions before pushed (`reduceSpringback`,
//!   `pushSpringback`: [`crate::springback`]); a line the posture left out is put back as it was.
//!
//! ## `FixRoute`
//!
//! `if( !m_dragStatus && !Settings().AllowDRCViolations() ) return false;` then `CommitRouting( CurrentNode() )`: what the last `Drag` left
//! (`m_lastNode`) -- a Ctrl+click changes nothing (`aForceCommit` is not read), and a refused click ends the drag (`InlineDrag`). The leader
//! segments of that solution (`restoreLeaderSegments`: each line's last segment in `DM_CORNER`, `findNewLeaderSegment`'s in `DM_SEGMENT`; the
//! selected segments where it did not resolve) are what `InlineDrag` selects afterwards ([`MultiDragger::leader_segments`]).
//!
//! The springback frames are [`World`]s against the world without the dragged lines, rebuilt into a node for each shove ([`crate::springback`]'s
//! decision); each keeps the joints its run locked (`LockJoint` of the heads' ends) and those of the frames below it, as KiCad's frames do. The mode
//! is read live (`Settings().Mode()`), but only a drag started in Shove has a `SHOVE`. A position that is not resolved commits, with "Allow DRC
//! violations", what KiCad's `m_lastNode` then is ([`Unresolved`]): nothing, or the shove's frame without the dragged lines.
//!
//! **Differs:**
//! - A position that cannot be resolved is shown as the dragged lines flagged colliding over what the frames had pushed; KiCad shows the world
//!   as it was (Walk around) or the shove's world without the dragged lines (Shove).
//! - A line with arcs: `leaderSegIndex` is the index of the segment in the line's points; KiCad's is the index of the link, which differs from
//!   it after an arc.

use crate::direction45::{AngleType, Direction45};
use crate::item::{Item, ItemId};
use crate::line::Line;
use crate::node::{AssembleOpts, Node};
use crate::seg_ops::{self, Seg2};
use crate::settings::{Mode, RoutingSettings};
use crate::shove::{DisplacedLine, DisplacedVia};
use crate::springback::{HeadSet, Springback, World};
use crate::walkaround::{WalkPolicy, WalkStatus, Walker};
use eda_drc::kimath::Shape;
use eda_model::ir::{Nm, Point};
use eda_model::BoardRules;
use std::collections::HashSet;

/// `m_dragMode`: `DM_CORNER` or `DM_SEGMENT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiDragMode {
    Corner,
    Segment,
}

/// `MULTI_DRAGGER::MDRAG_LINE`.
#[derive(Debug, Clone)]
pub struct MDragLine {
    /// `originalLeaders`: the selected tracks of the line.
    pub original_leaders: Vec<ItemId>,
    is_strict: bool,
    is_mid_seg: bool,
    is_corner: bool,
    is_draggable: bool,
    /// `leaderSegIndex`: -1 for none.
    leader_seg_index: isize,
    corner_is_last: bool,
    /// `originalLine`: the line as the world has it (turned in `DM_CORNER`).
    pub original_line: Line,
    pre_drag_line: Line,
    /// `draggedLine`.
    pub dragged_line: Line,
    drag_ok: bool,
    /// `isPrimaryLine`: the line the cursor is on.
    pub is_primary_line: bool,
    mid_seg: Seg2,
    drag_dist: i64,
    corner_distance: i64,
    leader_seg_distance: i64,
    /// `mdragIndex`: the line's place in `m_mdragLines`.
    pub mdrag_index: usize,
}

/// `PNS::MULTI_DRAGGER`: see the module doc.
#[derive(Debug, Clone)]
pub struct MultiDragger {
    mode: MultiDragMode,
    lines: Vec<MDragLine>,
    /// `m_dragStartPoint`.
    start_point: Point,
    /// `m_origDraggedItems`.
    orig_dragged: Vec<ItemId>,
    /// The last position the drag was given.
    last: Option<Point>,
    /// The gesture's one `SHOVE` (`m_shove`), made for a drag in Shove mode: its world (`m_preShoveNode`) and its springback stack.
    shove: Option<MultiShove>,
    /// `m_shove` exists: the drag started in Shove (`Start`). A drag started in another mode and switched to Shove resolves nothing in Shove
    /// (`multidragShove`: `if( !m_shove ) return false;`).
    shove_made: bool,
    /// What the last `Drag` left (`m_lastNode`, `m_dragStatus`, `m_leaderSegments`): what `FixRoute` commits.
    last_drag: Option<MultiDrag>,
}

/// `MULTI_DRAGGER::m_shove` and `m_preShoveNode` (`Start` in Shove mode): the world without every dragged line, and the one `SHOVE`'s springback
/// stack (`m_nodeStack`), whose frames are kept as [`World`]s against that world, as the line placer's and the pair placer's are.
#[derive(Debug, Clone)]
struct MultiShove {
    pre_shove: Node,
    springback: Springback,
}

/// What `MULTI_DRAGGER::Drag` makes of one position.
#[derive(Debug, Clone, Default)]
pub struct MultiDrag {
    /// The lines dragged (`completed`), each with its place in [`MultiDragger::lines`]: walked round or shoved where the mode did so.
    pub lines: Vec<(usize, Line)>,
    /// What the shove pushed aside (Shove mode).
    pub displaced_lines: Vec<DisplacedLine>,
    pub displaced_vias: Vec<DisplacedVia>,
    /// `m_dragStatus`: a release here commits.
    pub status: bool,
    /// The dragged lines run into something of the world (what `markViolations` shows), or the mode could not resolve the position.
    pub colliding: bool,
    /// `m_leaderSegments` after `restoreLeaderSegments( completed )`: for each line dragged, its segment that stands for its leader in the new
    /// shape -- the last one in `DM_CORNER`, the one `findNewLeaderSegment` finds in `DM_SEGMENT`. Empty where the mode could not resolve the
    /// position (KiCad's then still are `m_origDraggedItems`: [`MultiDragger::leader_segments`]).
    pub leaders: Vec<(i32, Seg2)>,
    /// What `m_lastNode` is where the position was not resolved (`status` false), which is what `FixRoute` commits when "Allow DRC violations"
    /// lets it: `None` for a resolved position.
    pub unresolved: Option<Unresolved>,
}

/// `m_lastNode` after a `Drag` that did not resolve its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved {
    /// `multidragWalkaround` with no attempt that worked, or `multidragShove` with no `SHOVE`: `m_lastNode` was deleted and is null, so
    /// `CurrentNode()` is `m_world` -- the board as it is.
    World,
    /// `multidragShove` whose `Run` failed: `m_lastNode = m_shove->CurrentNode()->Branch()`, the springback frame the shove started from with no
    /// dragged line in it ("Re-add any m_mdragLines that were removed from m_preShoveNode during Start() but are not part of aCompletedLines": only
    /// those come back), so the lines the posture took are gone from it.
    ShoveFrame,
}

/// What committing a multi-track drag replaces and adds (`ROUTER::CommitRouting( m_lastNode )`).
#[derive(Debug, Clone, Default)]
pub struct MultiCommit {
    pub remove_track_ids: Vec<String>,
    pub remove_via_ids: Vec<String>,
    pub tracks: Vec<Line>,
    /// The board track each of `tracks` replaces, by the same index.
    pub sources: Vec<Option<String>>,
    /// A via the shove moved: its board id and where it went.
    pub vias: Vec<(String, Point)>,
}

/// `NODE::AssembleLine( litem )` with its defaults.
fn assemble(node: &Node, id: ItemId) -> Option<Line> {
    node.assemble_line_with(id, AssembleOpts { follow_locked_segments: false, allow_width_mismatch: true, stop_at_locked_joints: false })
}

/// `VECTOR2I::EuclideanNorm()`: the rounded length (`KiROUND( |x| * M_SQRT2 )` on the diagonals).
fn norm(dx: i64, dy: i64) -> i64 {
    if dx.abs() == dy.abs() {
        return (dx.abs() as f64 * std::f64::consts::SQRT_2).round() as i64;
    }
    if dx == 0 {
        return dy.abs();
    }
    if dy == 0 {
        return dx.abs();
    }
    (dx as f64).hypot(dy as f64).round() as i64
}

/// `sign( v )`: -1, 0 or 1.
fn sign(v: i128) -> i64 {
    (v > 0) as i64 - (v < 0) as i64
}

/// `VECTOR2I::Resize( aNewLength )` (integral), on KiCad's `rescale` of the squares.
fn resize(v: (i64, i64), len: i64) -> (i64, i64) {
    let (x, y) = v;
    if x == 0 && y == 0 {
        return (0, 0);
    }
    let (nx, ny) = if x.abs() == y.abs() {
        let n = len.abs() as f64 * std::f64::consts::FRAC_1_SQRT_2;
        (n, n)
    } else {
        let (x_sq, y_sq) = (x as i128 * x as i128, y as i128 * y as i128);
        let l_sq = x_sq + y_sq;
        let new_sq = len as i128 * len as i128;
        ((seg_ops::rescale(new_sq, x_sq, l_sq) as f64).sqrt(), (seg_ops::rescale(new_sq, y_sq, l_sq) as f64).sqrt())
    };
    let r = |n: f64| n.round() as i64;
    let s = len.signum();
    ((if x < 0 { -r(nx) } else { r(nx) }) * s, (if y < 0 { -r(ny) } else { r(ny) }) * s)
}

/// `SHAPE_LINE_CHAIN::CSegment( i )`: a negative index counts from the end.
fn cseg(line: &Line, i: isize) -> Seg2 {
    let n = line.segment_count() as isize;
    let i = if i < 0 { i + n } else { i };
    let i = i.clamp(0, (n - 1).max(0)) as usize;
    (line.pts[i], line.pts[(i + 1).min(line.pts.len() - 1)])
}

/// `SHAPE_LINE_CHAIN::CPoint( i )`: a negative index counts from the end.
fn cpoint(line: &Line, i: isize) -> Point {
    let n = line.point_count() as isize;
    let i = if i < 0 { i + n } else { i };
    line.pts[i.clamp(0, n - 1) as usize]
}

/// A segment index as `LINE::DragSegment` takes it: a negative one counts from the end.
fn seg_index(line: &Line, i: isize) -> usize {
    if i < 0 {
        (line.segment_count() as isize + i).max(0) as usize
    } else {
        i as usize
    }
}

fn dir_of(s: Seg2) -> Direction45 {
    Direction45::from_seg(s.0, s.1)
}

fn add(p: Point, v: (i64, i64)) -> Point {
    Point { x: p.x + Nm::new(v.0), y: p.y + Nm::new(v.1) }
}

/// `SEG::Distance( aP )`: `isqrt( SquaredDistance( aP ) )`.
fn seg_distance(s: Seg2, p: Point) -> i64 {
    eda_drc::kimath::isqrt(eda_drc::kimath::Seg::new(s.0, s.1).sq_distance_to_point(p))
}

/// `SHAPE_LINE_CHAIN::Length()` of a line of segments: the sum of each one's rounded length.
fn chain_length(pts: &[Point]) -> i64 {
    pts.windows(2).map(|w| seg_ops::length(w[0], w[1]).get()).sum()
}

/// `SHAPE_LINE_CHAIN::PointAlong( aPathLength )`.
fn point_along(pts: &[Point], len: i64) -> Point {
    if len == 0 {
        return pts[0];
    }
    let mut total = 0i64;
    for w in pts.windows(2) {
        let l = seg_ops::length(w[0], w[1]).get();
        if total + l >= len {
            let d = ((w[1].x - w[0].x).get(), (w[1].y - w[0].y).get());
            return add(w[0], resize(d, len - total));
        }
        total += l;
    }
    *pts.last().unwrap()
}

/// `SHAPE_LINE_CHAIN::Split( aP )` (not exact): `p` made a vertex of the chain, the index of it returned; `None` (-1) when it is not on it.
fn split(pts: &mut Vec<Point>, p: Point) -> Option<usize> {
    let found = pts.iter().position(|&q| q == p);
    let mut ii: Option<usize> = None;
    let mut min_dist = 2i64;
    for s in 0..pts.len().saturating_sub(1) {
        let seg = (pts[s], pts[s + 1]);
        let dist = seg_distance(seg, p);
        // "make sure we are not producing a 'slightly concave' primitive"
        if dist < min_dist && seg.0 != p && seg.1 != p {
            min_dist = dist;
            if found.is_none() || found.is_some_and(|f| s < f) {
                ii = Some(s);
            }
        }
    }
    let ii = ii.or(found)?;
    // "Don't create duplicate points"
    if pts[ii] == p {
        return Some(ii);
    }
    pts.insert(ii + 1, p);
    Some(ii + 1)
}

/// `LINE::Collide( &aRef, aNode, layer, &ctx )` of two lines: do they come closer than the rules' clearance between them (`GetClearance`, with
/// the epsilon) on a layer they share? Two lines of one net never collide.
fn lines_collide(node: &Node, rules: &BoardRules, a: &Line, b: &Line) -> bool {
    if a.layer != b.layer || crate::item::same_net(&a.net, &b.net) {
        return false;
    }
    let clearance = node.clearance_between(rules, crate::node::Side::track(&a.net, a.layer), crate::node::Side::track(&b.net, b.layer), true);
    a.segs().any(|(p, q)| {
        let sa = Shape::Stadium { a: p, b: q, r: a.width / 2 };
        b.segs().any(|(u, v)| sa.collides(&Shape::Stadium { a: u, b: v, r: b.width / 2 }, clearance).is_some())
    })
}

/// `clipToOtherLine( aNode, aRef, aClipped )`: `clipped` cut back (a binary search on its length, down to `clipLengthThreshold` = 100 nm) to
/// the longest start of it that does not collide with `reference`; true if it was cut. A line that collides from its start is left with no
/// points (`tightest` is never set).
fn clip_to_other_line(node: &Node, rules: &BoardRules, reference: &Line, clipped: &mut Line) -> bool {
    const CLIP_LENGTH_THRESHOLD: i64 = 100;
    let mut did_clip = false;
    let mut cur_l = chain_length(&clipped.pts);
    let mut step = cur_l / 2 - 1;
    let mut tightest: Vec<Point> = Vec::new();
    let mut l = clipped.clone();
    while step > CLIP_LENGTH_THRESHOLD {
        let mut sl_tmp = clipped.pts.clone();
        let pclip = point_along(&sl_tmp, cur_l);
        let idx = split(&mut sl_tmp, pclip);
        // `sl_tmp.Slice( 0, idx )`
        sl_tmp.truncate(idx.map_or(0, |i| i + 1));
        l.pts = sl_tmp.clone();
        if lines_collide(node, rules, &l, reference) {
            did_clip = true;
            cur_l -= step;
            step /= 2;
        } else {
            tightest = sl_tmp;
            if did_clip {
                cur_l += step;
                step /= 2;
            } else {
                break;
            }
        }
    }
    clipped.pts = tightest;
    did_clip
}

impl MultiDragger {
    /// `MULTI_DRAGGER::Start( aP, aPrimitives )`: `items` are the selected tracks' segments and arcs, `at` the point the drag starts from (the
    /// cursor snapped onto the nearest of them), `mode` the router's mode then (`if( Settings().Mode() == RM_Shove )`: only a drag started in Shove
    /// has a `SHOVE`, `m_shove`). `None` when there is nothing to drag.
    pub fn start(node: &Node, at: Point, items: &[ItemId], start_mode: Mode) -> Option<MultiDragger> {
        if items.is_empty() {
            return None;
        }
        let primitives: Vec<&Item> = items.iter().filter_map(|id| node.get(*id)).collect();
        let mut lines: Vec<MDragLine> = Vec::new();
        for &id in items {
            if !node.get(id).is_some_and(Item::is_track) {
                continue;
            }
            // "we can possibly have multiple SEGMENTs in aPrimitives that belong to the same line"
            if let Some(l) = lines.iter_mut().find(|l| l.original_line.segment_ids.contains(&id)) {
                l.original_leaders.push(id);
                continue;
            }
            let Some(original_line) = assemble(node, id) else { continue };
            let index = lines.len();
            lines.push(MDragLine {
                original_leaders: vec![id],
                is_strict: false,
                is_mid_seg: false,
                is_corner: false,
                is_draggable: true,
                leader_seg_index: -1,
                corner_is_last: false,
                pre_drag_line: original_line.clone(),
                dragged_line: original_line.clone(),
                original_line,
                drag_ok: false,
                is_primary_line: false,
                mid_seg: (at, at),
                drag_dist: 0,
                corner_distance: 0,
                leader_seg_distance: 0,
                mdrag_index: index,
            });
        }
        if lines.is_empty() {
            return None;
        }
        // `aPrimitives.FindVertex( v )`: a selected segment with an end at `v`
        let find_vertex = |v: Point| primitives.iter().any(|it| matches!(it, Item::Segment(s) if s.a == v || s.b == v));
        let mut any_strict_corners = false;
        let mut any_strict_mid_segs = false;
        for l in lines.iter_mut() {
            let thr = (l.original_line.width / 2).get();
            let (orig_first, orig_last) = (l.original_line.pts[0], *l.original_line.pts.last().unwrap());
            let dist_first = norm((orig_first.x - at.x).get(), (orig_first.y - at.y).get());
            let dist_last = norm((orig_last.x - at.x).get(), (orig_last.y - at.y).get());
            l.corner_distance = dist_first.min(dist_last);
            let (ilast, ifirst) = (find_vertex(orig_last), find_vertex(orig_first));
            let take_first = if ilast && ifirst { dist_first < dist_last } else { !ilast && ifirst };
            if ifirst || ilast {
                l.is_corner = true;
                if take_first {
                    l.corner_is_last = false;
                    l.leader_seg_index = 0;
                    l.corner_distance = dist_first;
                    if dist_first <= thr {
                        l.is_strict = true;
                        l.corner_distance = 0;
                    }
                } else {
                    l.corner_is_last = true;
                    l.leader_seg_index = l.original_line.segment_count() as isize - 1;
                    l.corner_distance = dist_last;
                    if dist_last <= thr {
                        l.is_strict = true;
                        l.corner_distance = 0;
                    }
                }
            }
            // each selected segment of the line (`dyn_cast<SEGMENT*>( links[lidx] )`, an arc is not one), the last one winning
            for lidx in 0..l.original_line.segment_ids.len() {
                let lid = l.original_line.segment_ids[lidx];
                let Some(Item::Segment(lseg)) = node.get(lid) else { continue };
                if !items.contains(&lid) {
                    continue;
                }
                let d = seg_distance((lseg.a, lseg.b), at);
                l.mid_seg = (lseg.a, lseg.b);
                l.is_mid_seg = true;
                l.leader_seg_index = lidx as isize;
                l.leader_seg_distance = d + thr;
                if d < thr && !l.is_strict {
                    l.is_corner = false;
                    l.is_strict = true;
                    l.leader_seg_distance = 0;
                }
            }
            if l.is_strict {
                any_strict_corners |= l.is_corner;
                any_strict_mid_segs |= !l.is_corner;
            }
        }
        let mut mode = if any_strict_corners {
            MultiDragMode::Corner
        } else if any_strict_mid_segs {
            MultiDragMode::Segment
        } else {
            // the nearest corner and the nearest segment, the first of equals (`<`)
            let best_corner = (0..lines.len()).min_by_key(|&i| (lines[i].corner_distance, i))?;
            let best_seg = (0..lines.len()).min_by_key(|&i| (lines[i].leader_seg_distance, i))?;
            if lines[best_corner].corner_distance < lines[best_seg].leader_seg_distance {
                lines[best_corner].is_primary_line = true;
                MultiDragMode::Corner
            } else {
                lines[best_seg].is_primary_line = true;
                MultiDragMode::Segment
            }
        };
        if mode == MultiDragMode::Corner {
            for l in lines.iter_mut() {
                // "make sure the corner to drag is the last one"
                if !l.corner_is_last {
                    l.original_line.reverse();
                    l.corner_is_last = true;
                }
                // "and if it's connected (non-trivial fanout), disregard it": `JOINT::IsTrivialEndpoint`
                let last = *l.original_line.pts.last().unwrap();
                let trivial = node.joint_at(last, &l.original_line.net).is_some_and(|j| j.links.len() == 1 && matches!(node.get(j.links[0]), Some(Item::Segment(_))));
                if !trivial {
                    mode = MultiDragMode::Segment;
                }
            }
        }
        if any_strict_corners || any_strict_mid_segs {
            if let Some(l) = lines.iter_mut().find(|l| l.is_strict) {
                l.is_primary_line = true;
            }
        }
        Some(MultiDragger { mode, lines, start_point: at, orig_dragged: items.to_vec(), last: None, shove: None, shove_made: start_mode == Mode::Shove, last_drag: None })
    }

    /// `m_dragMode`.
    pub fn mode(&self) -> MultiDragMode {
        self.mode
    }

    /// `m_mdragLines`.
    pub fn lines(&self) -> &[MDragLine] {
        &self.lines
    }

    /// The selected tracks the drag started from (`m_origDraggedItems`).
    pub fn dragged_items(&self) -> &[ItemId] {
        &self.orig_dragged
    }

    /// `m_dragStartPoint`.
    pub fn start_point(&self) -> Point {
        self.start_point
    }

    /// `tryPosture( aVariant )`: the lines dragged to `at` in posture `variant` (see the module doc), and whether the posture works. `completed`
    /// is filled either way (`Drag` goes on with what the last posture tried left in it).
    fn try_posture(&self, variant: u8, at: Point, settings: &RoutingSettings, completed: &mut Vec<MDragLine>, guide: &mut Option<Seg2>) -> bool {
        let mut lines = self.lines.clone();
        for l in lines.iter_mut() {
            l.drag_ok = false;
            l.pre_drag_line = l.original_line.clone();
        }
        let Some(primary) = lines.iter().position(|l| l.is_primary_line) else { return false };
        let mut primary_dragged = lines[primary].original_line.clone();
        primary_dragged.clear_links();
        let mut primary_pre_drag = lines[primary].original_line.clone();
        if variant == 1 && primary_pre_drag.point_count() > 2 {
            primary_pre_drag.pts.pop();
            primary_dragged.pts.pop();
            for l in lines.iter_mut() {
                l.pre_drag_line.pts.pop();
            }
        }
        completed.clear();
        let snap = if settings.smooth_dragged_segments { primary_dragged.width / 4 } else { Nm::ZERO };
        let last_pre_drag: Seg2;
        let mut primary_dir = Direction45::Undefined;
        let perp: (i64, i64);
        let mut primary_last_seg_dir = Direction45::Undefined;
        if self.mode == MultiDragMode::Corner {
            if primary_pre_drag.segment_count() == 0 {
                return false;
            }
            last_pre_drag = cseg(&primary_pre_drag, -1);
            primary_dir = dir_of(last_pre_drag);
            let at_index = primary_dragged.point_count().saturating_sub(1);
            primary_dragged.drag_corner(at, at_index, false, snap);
            if primary_dragged.segment_count() == 0 {
                return false;
            }
            let mut last_prim_drag = cseg(&primary_dragged, -1);
            if variant == 2 {
                last_prim_drag = last_pre_drag;
            }
            let last_seg = cseg(&primary_dragged, -1);
            if dir_of(last_seg) != primary_dir && seg_ops::length(last_seg.0, last_seg.1) < primary_dragged.width {
                last_prim_drag = last_pre_drag;
            }
            // `( lastPrimDrag.B - lastPrimDrag.A ).Perpendicular()`
            perp = (-(last_prim_drag.1.y - last_prim_drag.0.y).get(), (last_prim_drag.1.x - last_prim_drag.0.x).get());
            primary_last_seg_dir = dir_of(last_prim_drag);
        } else {
            if primary_dragged.segment_count() == 0 {
                return false;
            }
            let leader = lines[primary].leader_seg_index;
            last_pre_drag = cseg(&primary_dragged, leader);
            let index = seg_index(&primary_dragged, leader);
            primary_dragged.drag_segment45(at, index, snap);
            let mid = lines[primary].mid_seg;
            perp = (-(mid.1.y - mid.0.y).get(), (mid.1.x - mid.0.x).get());
            // `m_guide = SEG( aP, aP + perp )`: what `findNewLeaderSegment` looks for each line's new leader on
            *guide = Some((at, add(at, perp)));
        }
        for l in lines.iter_mut() {
            if l.is_draggable {
                l.drag_ok = false;
                // "reject nulls"
                if l.pre_drag_line.segment_count() >= 1 {
                    if self.mode == MultiDragMode::Corner {
                        let parallel_dir = dir_of(cseg(&l.pre_drag_line, -1));
                        let lead_angle = primary_dir.angle_to(&parallel_dir);
                        if matches!(lead_angle, AngleType::Obtuse | AngleType::Right | AngleType::Straight) {
                            // the distance of the line's last point from the primary's line, projected onto the perpendicular
                            let dist = seg_ops::line_distance_signed(last_pre_drag, *l.pre_drag_line.pts.last().unwrap()).get();
                            let projected = add(at, resize(perp, dist));
                            let mut parallel_dragged = l.pre_drag_line.clone();
                            parallel_dragged.clear_links();
                            let index = parallel_dragged.point_count() - 1;
                            parallel_dragged.drag_corner45_preferring(projected, index, Nm::ZERO, primary_last_seg_dir);
                            l.drag_ok = true;
                            if !l.is_primary_line {
                                l.dragged_line = parallel_dragged;
                                completed.push(l.clone());
                            }
                        }
                    } else {
                        let sdrag = l.mid_seg;
                        let ang = dir_of(last_pre_drag).angle_to(&dir_of(sdrag));
                        // `ang & ( DIRECTION_45::ANG_HALF_FULL | DIRECTION_45::ANG_STRAIGHT )`
                        if matches!(ang, AngleType::HalfFull | AngleType::Straight) {
                            let dist = seg_ops::line_distance_signed(last_pre_drag, cpoint(&l.pre_drag_line, l.leader_seg_index)).get();
                            let projected = add(at, resize(perp, dist));
                            let sperp = (at, add(at, resize(perp, 10_000_000)));
                            let start_proj = seg_ops::line_project(sperp, self.start_point);
                            let v = ((projected.x - start_proj.x).get(), (projected.y - start_proj.y).get());
                            l.drag_dist = norm(v.0, v.1) * sign(v.0 as i128 * perp.0 as i128 + v.1 as i128 * perp.1 as i128);
                            l.drag_ok = true;
                            if !l.is_primary_line {
                                let mut dragged = l.pre_drag_line.clone();
                                dragged.clear_links();
                                let index = seg_index(&dragged, l.leader_seg_index);
                                dragged.drag_segment45(projected, index, snap);
                                l.dragged_line = dragged;
                                completed.push(l.clone());
                            }
                        }
                    }
                }
            }
            if l.is_primary_line {
                l.dragged_line = primary_dragged.clone();
                l.drag_ok = true;
                completed.push(l.clone());
            }
        }
        if self.mode == MultiDragMode::Segment {
            return true;
        }
        for l in completed.iter() {
            if !l.drag_ok && variant < 2 {
                return false;
            }
            if l.is_primary_line {
                continue;
            }
            if l.dragged_line.segment_count() == 0 || dir_of(cseg(&l.dragged_line, -1)) != primary_last_seg_dir {
                return false;
            }
        }
        true
    }

    /// `MULTI_DRAGGER::Drag( aP )`: see the module doc. `settings.mode` is the mode the drag runs in. What it leaves is what a release
    /// commits ([`Self::fix_route`]); in Shove mode the gesture's springback stack goes on from what the positions before it pushed.
    pub fn drag(&mut self, world: &Node, rules: &BoardRules, settings: &RoutingSettings, to: Point) -> MultiDrag {
        self.last = Some(to);
        let mut completed: Vec<MDragLine> = Vec::new();
        let mut guide: Option<Seg2> = None;
        for variant in 0..3 {
            if self.try_posture(variant, to, settings, &mut completed, &mut guide) {
                break;
            }
        }
        let mut drag = match settings.mode {
            Mode::Walkaround => self.multidrag_walkaround(world, rules, settings, completed.clone()),
            Mode::Shove => self.multidrag_shove(world, rules, settings, completed.clone()),
            Mode::MarkObstacles => self.multidrag_mark_obstacles(world, rules, completed.clone()),
        };
        // `restoreLeaderSegments( aCompletedLines )`, which each mode runs once it has resolved the position
        if drag.status || settings.mode == Mode::MarkObstacles {
            drag.leaders = self.restore_leader_segments(&completed, &drag, guide);
        }
        self.last_drag = Some(drag.clone());
        drag
    }

    /// `restoreLeaderSegments( aCompletedLines )`: for each line dragged (`dragOK`), in `DM_CORNER` the last segment of its new shape
    /// (`draggedLine.GetLink( -1 )`), in `DM_SEGMENT` the one `findNewLeaderSegment` finds -- the segment the guide (`m_guide`, the
    /// perpendicular through the cursor) crosses (`IntersectLines`, then `Contains`) that runs the way the line's leader ran before
    /// (`DIRECTION_45`, or the opposite way). The new shape is the one the mode left (`drag.lines`).
    fn restore_leader_segments(&self, completed: &[MDragLine], drag: &MultiDrag, guide: Option<Seg2>) -> Vec<(i32, Seg2)> {
        let mut out = Vec::new();
        for l in completed.iter().filter(|l| l.drag_ok) {
            let Some(line) = drag.lines.iter().find(|(i, _)| *i == l.mdrag_index).map(|(_, line)| line) else { continue };
            if line.segment_count() == 0 {
                continue;
            }
            match self.mode {
                MultiDragMode::Corner => out.push((line.layer, cseg(line, -1))),
                MultiDragMode::Segment => {
                    let (Some(guide), true) = (guide, l.pre_drag_line.segment_count() > 0) else { continue };
                    let orig_leader = cseg(&l.pre_drag_line, l.leader_seg_index);
                    let orig_dir = dir_of(orig_leader);
                    let found = (0..line.segment_count() as isize).map(|i| cseg(line, i)).find(|&cur| {
                        let cur_dir = dir_of(cur);
                        seg_ops::intersect_lines(cur, guide).is_some_and(|ip| seg_ops::contains(cur, ip)) && (cur_dir == orig_dir || cur_dir == orig_dir.opposite())
                    });
                    out.extend(found.map(|seg| (line.layer, seg)));
                }
            }
        }
        out
    }

    /// `ROUTER::GetLastCommittedLeaderSegments()` after the last `Drag`: the leader segments of its solution, or -- where it could not be
    /// resolved (`restoreLeaderSegments` not run) -- the selected tracks' segments as they are (`m_leaderSegments = m_origDraggedItems.CItems()`
    /// in `tryPosture`). `ROUTER_TOOL::InlineDrag` selects their tracks once the drag is over. Each with its layer.
    pub fn leader_segments(&self, world: &Node) -> Vec<(i32, Seg2)> {
        match &self.last_drag {
            Some(d) if !d.leaders.is_empty() => d.leaders.clone(),
            _ => self.orig_dragged.iter().filter_map(|id| world.get(*id).and_then(|it| it.track_ends().map(|e| (it.layers().start(), e)))).collect(),
        }
    }

    /// The world without the lines of `completed` (`preWalkNode->Remove( l.originalLine )`).
    fn without(world: &Node, lines: &[MDragLine]) -> Node {
        let mut node = world.branch();
        for l in lines {
            node.remove_line_segments(&l.original_line);
        }
        node
    }

    /// Does any of `lines` run into something of `node` (`markViolations`: the dragged items themselves are not marked)?
    fn any_colliding(node: &Node, rules: &BoardRules, lines: &[(usize, Line)]) -> bool {
        lines.iter().any(|(_, l)| {
            l.segs().any(|(a, b)| node.first_colliding(&Shape::Stadium { a, b, r: l.width / 2 }, &l.net, crate::layer::LayerRange::single(l.layer), rules, &[]).is_some())
        })
    }

    /// `multidragMarkObstacles`: see the module doc.
    fn multidrag_mark_obstacles(&self, world: &Node, rules: &BoardRules, mut completed: Vec<MDragLine>) -> MultiDrag {
        let node = Self::without(world, &completed);
        for l1 in 0..completed.len() {
            for l2 in (l1 + 1)..completed.len() {
                let reference = completed[l1].dragged_line.clone();
                let mut clipped = completed[l2].dragged_line.clone();
                if clip_to_other_line(&node, rules, &reference, &mut clipped) {
                    completed[l2].dragged_line = clipped;
                }
            }
        }
        let lines: Vec<(usize, Line)> = completed.into_iter().map(|l| (l.mdrag_index, l.dragged_line)).collect();
        let colliding = Self::any_colliding(&node, rules, &lines);
        MultiDrag { lines, status: true, colliding, ..MultiDrag::default() }
    }

    /// `MULTI_DRAGGER::tryWalkaround`: `WP_SHORTEST`, not solids only, the walkaround iteration limit, three times the line's length.
    fn try_walkaround(node: &Node, rules: &BoardRules, settings: &RoutingSettings, line: &Line) -> Option<Vec<Point>> {
        let mut walker = Walker::new(node, rules);
        walker.corner_mode = settings.corner_mode;
        walker.arcs = line.arcs.clone();
        walker.iteration_limit = settings.walkaround_iteration_limit.max(0) as u32;
        walker.length_limit_on = true;
        walker.length_expansion_factor = 3.0;
        walker.set_allowed_policies(&[WalkPolicy::Shortest]);
        let outcome = walker.route(&line.net, line.layer, line.width.max(Nm::new(1)), &line.pts);
        (outcome.status_of(WalkPolicy::Shortest) == WalkStatus::Done).then(|| outcome.line_of(WalkPolicy::Shortest).to_vec())
    }

    /// `multidragWalkaround`: see the module doc.
    fn multidrag_walkaround(&self, world: &Node, rules: &BoardRules, settings: &RoutingSettings, mut completed: Vec<MDragLine>) -> MultiDrag {
        completed.sort_by_key(|l| l.drag_dist);
        let pre_walk = Self::without(world, &completed);
        // `WALK_STATE`: the lines walked in this order, and how much longer they got
        let mut best: Option<(i64, Vec<(usize, Line)>)> = None;
        let mut first_ok: Option<bool> = None;
        for attempt in 0..2 {
            let mut node = pre_walk.branch();
            let mut total = 0i64;
            let mut walked: Vec<(usize, Line)> = Vec::new();
            let mut fail = false;
            for lidx in 0..completed.len() {
                let l = &completed[if attempt == 1 { completed.len() - 1 - lidx } else { lidx }];
                match Self::try_walkaround(&node, rules, settings, &l.dragged_line) {
                    Some(pts) => {
                        let walk = Line { pts, segment_ids: Vec::new(), ..l.dragged_line.clone() };
                        node.add_line(&walk, None, false);
                        total += chain_length(&walk.pts) - chain_length(&l.dragged_line.pts);
                        walked.push((l.mdrag_index, walk));
                    }
                    None => {
                        fail = true;
                        break;
                    }
                }
            }
            if attempt == 0 {
                first_ok = Some(!fail);
            }
            if fail {
                continue;
            }
            // both work: the first if it lengthens the lines less ("walkState[0].totalLength < walkState[1].totalLength"), else the second
            match &best {
                Some((t0, _)) if first_ok == Some(true) && *t0 < total => {}
                _ => best = Some((total, walked)),
            }
        }
        match best {
            Some((_, mut lines)) => {
                lines.sort_by_key(|(i, _)| completed.iter().position(|l| l.mdrag_index == *i));
                MultiDrag { lines, status: true, ..MultiDrag::default() }
            }
            None => MultiDrag { lines: completed.into_iter().map(|l| (l.mdrag_index, l.dragged_line)).collect(), status: false, colliding: true, unresolved: Some(Unresolved::World), ..MultiDrag::default() },
        }
    }

    /// `multidragShove`: see the module doc. The gesture's one `SHOVE` (`m_shove`, made at the first position): each position pops the frames
    /// of the springback stack none of the heads collides with any more (`SHOVE::Run`: `reduceSpringback( headSet )`), shoves from the frame
    /// left (`m_currentNode = parent->Branch()`) with each head's ends locked (`LockJoint`: the heads' policy, `SHP_SHOVE | SHP_DONT_OPTIMIZE`,
    /// has no `SHP_DONT_LOCK_ENDPOINTS`), and pushes the world it leaves as the newest frame (`pushSpringback`). So a track the bus pushed stays
    /// pushed while the bus presses on it and springs back, frame by frame, once the bus draws back.
    fn multidrag_shove(&mut self, world: &Node, rules: &BoardRules, settings: &RoutingSettings, mut completed: Vec<MDragLine>) -> MultiDrag {
        completed.sort_by_key(|l| l.drag_dist);
        // `if( !m_shove ) return false;`: `m_lastNode` deleted above, the world as it is
        if !self.shove_made {
            return MultiDrag { lines: completed.into_iter().map(|l| (l.mdrag_index, l.dragged_line)).collect(), status: false, colliding: true, unresolved: Some(Unresolved::World), ..MultiDrag::default() };
        }
        if self.shove.is_none() {
            // `m_preShoveNode`: the world without every dragged line (`Start`), not only the completed ones
            self.shove = Some(MultiShove { pre_shove: Self::without(world, &self.lines), springback: Springback::new() });
        }
        let shove = self.shove.as_mut().expect("made above");
        let heads: Vec<crate::shove::ShoveHead> = completed
            .iter()
            .map(|l| {
                let mut line = l.dragged_line.clone();
                line.clear_links();
                crate::shove::ShoveHead { line, via: None }
            })
            .collect();
        let unresolved = |completed: Vec<MDragLine>, parent: &World, base: &Node| MultiDrag {
            lines: completed.into_iter().map(|l| (l.mdrag_index, l.dragged_line)).collect(),
            displaced_lines: parent.displaced_lines(base),
            displaced_vias: parent.displaced_vias(),
            status: false,
            colliding: true,
            leaders: Vec::new(),
            unresolved: Some(Unresolved::ShoveFrame),
        };
        // "empty head? nothing to shove...": `SH_INCOMPLETE`
        if heads.iter().any(|h| h.line.segment_count() == 0) {
            let (top, _) = shove.springback.top();
            return unresolved(completed, &top, &shove.pre_shove);
        }
        // `reduceSpringback( headSet )`: the heads asked about together
        let head_sets: Vec<HeadSet> = heads.iter().map(|h| HeadSet { pts: &h.line.pts, net: &h.line.net, layer: h.line.layer, width: h.line.width, via: None }).collect();
        let (parent, prior_area) = shove.springback.reduce_all(&shove.pre_shove, &head_sets, rules);
        drop(head_sets);
        let node = parent.apply(&shove.pre_shove);
        let Some(outcome) = crate::shove::shove_heads_locked_after(&node, heads, rules, settings, prior_area) else {
            // "bring back previous state": `m_currentNode = parent`, nothing pushed
            return unresolved(completed, &parent, &shove.pre_shove);
        };
        let mut frame = parent;
        frame.merge_shove(&outcome.displaced_lines, &outcome.displaced_vias);
        // `LockJoint` of each head's ends (`AddHeads( .., SHP_SHOVE | SHP_DONT_OPTIMIZE )`: no `SHP_DONT_LOCK_ENDPOINTS`), kept with the frame
        for l in &completed {
            frame.lock_head_ends(&l.dragged_line.pts, &l.dragged_line.net, false);
        }
        shove.springback.push(&shove.pre_shove, frame.clone(), outcome.area);
        let lines: Vec<(usize, Line)> = completed
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let pts = outcome.heads.get(i).cloned().unwrap_or_else(|| l.dragged_line.pts.clone());
                (l.mdrag_index, Line { pts, segment_ids: Vec::new(), ..l.dragged_line.clone() })
            })
            .collect();
        MultiDrag { lines, displaced_lines: frame.displaced_lines(&shove.pre_shove), displaced_vias: frame.displaced_vias(), status: true, colliding: false, leaders: Vec::new(), unresolved: None }
    }

    /// The last position the drag was given, if any.
    pub fn last_point(&self) -> Option<Point> {
        self.last
    }

    /// `MULTI_DRAGGER::FixRoute( aForceCommit )`: what the last `Drag` left (`CommitRouting( CurrentNode() )`, `m_lastNode`), committed when it
    /// resolved its position (`m_dragStatus`) or violations are allowed (`AllowDRCViolations()`, read now); `None` otherwise, and before any `Drag`.
    /// `aForceCommit` is not read. A position that was not resolved commits KiCad's `m_lastNode` of it ([`Unresolved`]): nothing (`m_world`), or
    /// the shove's frame, where the dragged lines are not -- their tracks go.
    pub fn fix_route(&self, world: &Node, settings: &RoutingSettings) -> Option<MultiCommit> {
        let drag = self.last_drag.as_ref()?;
        if !drag.status && !settings.allow_drc_violations() {
            return None;
        }
        match drag.unresolved {
            Some(Unresolved::World) => Some(MultiCommit::default()),
            Some(Unresolved::ShoveFrame) => Some(self.commit_with(world, drag, false)),
            None => Some(self.commit_of(world, drag)),
        }
    }

    /// What committing `drag` replaces and adds: the IR tracks of each dragged line (what they kept of themselves beyond the line handed back),
    /// the new lines (each from the track the line was assembled from: `pl.SetSourceItem( aSeg->GetSourceItem() )`), and what a shove moved.
    pub fn commit_of(&self, world: &Node, drag: &MultiDrag) -> MultiCommit {
        self.commit_with(world, drag, true)
    }

    /// [`Self::commit_of`], the dragged lines written (`with_lines`) or taken away with nothing in their place (a shove frame without them).
    fn commit_with(&self, world: &Node, drag: &MultiDrag, with_lines: bool) -> MultiCommit {
        let mut commit = MultiCommit::default();
        let mut covered: HashSet<ItemId> = HashSet::new();
        let mut tracks: Vec<String> = Vec::new();
        for (index, _) in &drag.lines {
            let Some(l) = self.lines.iter().find(|l| l.mdrag_index == *index) else { continue };
            covered.extend(l.original_line.segment_ids.iter().copied());
            for id in &l.original_line.segment_ids {
                if let Some(t) = world.get(*id).and_then(|it| it.source_track_id()) {
                    if !tracks.iter().any(|x| x == t) {
                        tracks.push(t.to_string());
                    }
                }
            }
        }
        let shoved: HashSet<&String> = drag.displaced_lines.iter().filter_map(|d| d.source_track.as_ref()).collect();
        for t in &tracks {
            commit.remove_track_ids.push(t.clone());
            if shoved.contains(t) {
                continue;
            }
            for run in world.uncovered_track_runs(t, &covered) {
                commit.tracks.push(run);
                commit.sources.push(Some(t.clone()));
            }
        }
        for (index, line) in drag.lines.iter().filter(|_| with_lines) {
            if line.point_count() < 2 {
                continue;
            }
            let source = self.lines.iter().find(|l| l.mdrag_index == *index).and_then(|l| l.original_leaders.first()).and_then(|id| world.get(*id)).and_then(|it| it.source_track_id()).map(str::to_string);
            commit.tracks.push(Line { segment_ids: Vec::new(), ..line.clone() });
            commit.sources.push(source);
        }
        for d in &drag.displaced_lines {
            if let Some(id) = &d.source_track {
                if !commit.remove_track_ids.contains(id) {
                    commit.remove_track_ids.push(id.clone());
                }
                // a track the frames took away with nothing in its place is one empty line (`World::displaced_lines`)
                if d.line.point_count() >= 2 {
                    commit.tracks.push(d.line.clone());
                    commit.sources.push(Some(id.clone()));
                }
            }
        }
        for v in &drag.displaced_vias {
            commit.remove_via_ids.push(v.source_via.clone());
            commit.vias.push((v.source_via.clone(), v.pos));
        }
        commit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{net_of, Segment, Solid};
    use crate::layer::LayerRange;
    use eda_model::units::mm;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap()
    }

    fn p(x: f64, y: f64) -> Point {
        Point { x: mm(x), y: mm(y) }
    }

    fn pn(x: i64, y: i64) -> Point {
        Point { x: Nm::new(x), y: Nm::new(y) }
    }

    fn track(node: &mut Node, net: &str, track: &str, pts: &[Point]) -> Vec<ItemId> {
        pts.windows(2).enumerate().map(|(i, w)| node.add(Item::Segment(Segment { net: net_of(net), layer: 0, a: w[0], b: w[1], width: mm(0.2), source_track: Some((track.into(), i)), locked: false }))).collect()
    }

    /// Three parallel 10 mm tracks A, B, C 1 mm apart (y = 0, 1, 2), from x = 0 to x = 10, free at both ends; all of them selected.
    fn bus() -> (Node, Vec<ItemId>) {
        let mut node = Node::new();
        let mut items = Vec::new();
        for (k, net) in ["A", "B", "C"].iter().enumerate() {
            items.extend(track(&mut node, net, &format!("t{net}"), &[p(0.0, k as f64), p(10.0, k as f64)]));
        }
        (node, items)
    }

    fn settings(mode: Mode) -> RoutingSettings {
        RoutingSettings { mode, ..RoutingSettings::default() }
    }

    /// `DM_SEGMENT`: the cursor on the middle of A's segment (a strict leader segment) slides it 2 mm up; B and C, parallel to it, slide by the
    /// same, each at its own distance from A along A's perpendicular (`LineDistance( .., true )`, `perp.Resize( dist )`), and each is KiCad's
    /// `dragSegment45` of a line of one segment: 45-degree legs from its fixed ends.
    #[test]
    fn a_segment_drag_slides_every_selected_parallel_line() {
        let (node, items) = bus();
        let mut d = MultiDragger::start(&node, p(5.0, 0.0), &items, Mode::Shove).expect("starts");
        assert_eq!(d.mode(), MultiDragMode::Segment);
        assert_eq!(d.lines().len(), 3);
        assert!(d.lines()[0].is_primary_line && !d.lines()[1].is_primary_line);
        for mode in [Mode::Walkaround, Mode::MarkObstacles, Mode::Shove] {
            let drag = d.drag(&node, &rules(), &settings(mode), p(5.0, -2.0));
            assert!(drag.status && !drag.colliding, "{mode:?}: {drag:?}");
            let line = |i: usize| drag.lines.iter().find(|(k, _)| *k == i).expect("dragged").1.pts.clone();
            assert_eq!(line(0), vec![p(0.0, 0.0), p(2.0, -2.0), p(8.0, -2.0), p(10.0, 0.0)], "{mode:?}");
            assert_eq!(line(1), vec![p(0.0, 1.0), p(2.0, -1.0), p(8.0, -1.0), p(10.0, 1.0)], "{mode:?}");
            assert_eq!(line(2), vec![p(0.0, 2.0), p(2.0, 0.0), p(8.0, 0.0), p(10.0, 2.0)], "{mode:?}");
        }
        let commit = d.fix_route(&node, &settings(Mode::Walkaround)).expect("commits");
        assert_eq!(commit.remove_track_ids, vec!["tA", "tB", "tC"]);
        assert_eq!(commit.tracks.len(), 3);
        assert_eq!(commit.sources, vec![Some("tA".to_string()), Some("tB".to_string()), Some("tC".to_string())]);
    }

    /// `DM_CORNER`: the cursor on A's free end (a strict corner, a trivial end) drags it to (12, -2) -- `BuildInitialTrace` from A's other end,
    /// straight then diagonal -- and B's and C's ends go to the cursor offset along the perpendicular of A's last leg (a diagonal: `Resize` of
    /// (2, 2) mm to 1 and 2 mm), each ending in A's last direction.
    #[test]
    fn a_corner_drag_takes_every_parallel_end_along() {
        let (node, items) = bus();
        let mut d = MultiDragger::start(&node, p(10.0, 0.0), &items, Mode::Shove).expect("starts");
        assert_eq!(d.mode(), MultiDragMode::Corner);
        for mode in [Mode::Walkaround, Mode::MarkObstacles, Mode::Shove] {
            let drag = d.drag(&node, &rules(), &settings(mode), p(12.0, -2.0));
            assert!(drag.status && !drag.colliding, "{mode:?}: {drag:?}");
            let line = |i: usize| drag.lines.iter().find(|(k, _)| *k == i).expect("dragged").1.pts.clone();
            assert_eq!(line(0), vec![p(0.0, 0.0), p(10.0, 0.0), p(12.0, -2.0)], "{mode:?}");
            assert_eq!(line(1), vec![p(0.0, 1.0), pn(10_414_214, 1_000_000), pn(12_707_107, -1_292_893)], "{mode:?}");
            assert_eq!(line(2), vec![p(0.0, 2.0), pn(10_828_428, 2_000_000), pn(13_414_214, -585_786)], "{mode:?}");
        }
    }

    /// A corner that is not a trivial end (a pad there) makes the drag `DM_SEGMENT` (`IsTrivialEndpoint`), and a selected line that is not
    /// parallel to the primary's leader segment is left where it is (`ang & ( ANG_HALF_FULL | ANG_STRAIGHT )`): it is not dragged, and the commit
    /// keeps its track.
    #[test]
    fn a_connected_corner_falls_back_to_a_segment_drag_and_a_crossing_line_stays() {
        let (mut node, mut items) = bus();
        node.add(Item::Solid(Solid { net: net_of("A"), layers: LayerRange::single(0), pos: p(10.0, 0.0), shape: Shape::Circle { c: p(10.0, 0.0), r: mm(0.3) }, source: "J1.1".into(), edge: false, rule: 0, free_pad: false, anchor_points: Vec::new() }));
        let d = MultiDragger::start(&node, p(10.0, 0.0), &items, Mode::Shove).expect("starts");
        assert_eq!(d.mode(), MultiDragMode::Segment, "the corner is on a pad");
        // a vertical line D, selected too
        items.extend(track(&mut node, "D", "tD", &[p(20.0, -5.0), p(20.0, 5.0)]));
        let mut d = MultiDragger::start(&node, p(5.0, 1.0), &items, Mode::Shove).expect("starts");
        assert_eq!(d.mode(), MultiDragMode::Segment);
        let drag = d.drag(&node, &rules(), &settings(Mode::Walkaround), p(5.0, -1.0));
        assert!(drag.status);
        assert!(drag.lines.iter().all(|(_, l)| l.net != net_of("D")), "D is not dragged: {drag:?}");
        let commit = d.fix_route(&node, &settings(Mode::Walkaround)).expect("commits");
        assert!(!commit.remove_track_ids.contains(&"tD".to_string()));
    }

    /// Shove mode: the bus slid up into a GND track pushes it aside (`multidragShove`: the lines are the shove's heads); the commit replaces it.
    /// Walk around mode: the lines walk round it instead, and the GND track stays.
    #[test]
    fn the_bus_shoves_or_walks_round_a_track_in_its_way() {
        let (mut node, items) = bus();
        track(&mut node, "GND", "tG", &[p(4.0, -2.2), p(6.0, -2.2)]);
        let rules = rules();
        let mut d = MultiDragger::start(&node, p(5.0, 0.0), &items, Mode::Shove).expect("starts");
        let shoved = d.drag(&node, &rules, &settings(Mode::Shove), p(5.0, -2.0));
        assert!(shoved.status, "{shoved:?}");
        assert!(shoved.displaced_lines.iter().any(|l| l.source_track.as_deref() == Some("tG")), "the GND track is pushed: {shoved:?}");
        let commit = d.fix_route(&node, &settings(Mode::Shove)).expect("commits");
        assert!(commit.remove_track_ids.contains(&"tG".to_string()));
        let walked = d.drag(&node, &rules, &settings(Mode::Walkaround), p(5.0, -2.0));
        assert!(walked.status && walked.displaced_lines.is_empty(), "{walked:?}");
        let a = &walked.lines.iter().find(|(k, _)| *k == 0).unwrap().1;
        assert_ne!(a.pts, vec![p(0.0, 0.0), p(2.0, -2.0), p(8.0, -2.0), p(10.0, 0.0)], "A goes round the GND track");
        // the walked lines keep clear of the GND track
        let mut world = node.branch();
        for l in d.lines() {
            world.remove_line_segments(&l.original_line);
        }
        assert!(!MultiDragger::any_colliding(&world, &rules, &walked.lines), "{walked:?}");
    }

    /// UI-LEFT-2 RR1: Shove keeps one `SHOVE` for the gesture (`m_shove`), whose springback stack carries what the positions before pushed. The
    /// bus slid up into the GND track pushes it, and a frame per position goes on the stack; pressing on, the GND track is pushed on from where
    /// the frame left it; drawn back clear, the frames the bus no longer touches are popped (`reduceSpringback`: never the bottom one) and the GND
    /// track springs back. The release commits what the last position left (`m_lastNode`), not a shove made again.
    #[test]
    fn the_bus_shove_springs_back_frame_by_frame_and_the_release_commits_the_last_position() {
        let (mut node, items) = bus();
        track(&mut node, "GND", "tG", &[p(4.0, -2.2), p(6.0, -2.2)]);
        let rules = rules();
        let shove = settings(Mode::Shove);
        let mut d = MultiDragger::start(&node, p(5.0, 0.0), &items, Mode::Shove).expect("starts");
        let depth = |d: &MultiDragger| d.shove.as_ref().map_or(0, |s| s.springback.len());
        let gnd_y = |drag: &MultiDrag| drag.displaced_lines.iter().filter(|l| l.source_track.as_deref() == Some("tG")).flat_map(|l| l.line.pts.iter().map(|q| q.y)).min();
        let first = d.drag(&node, &rules, &shove, p(5.0, -0.5));
        assert!(first.status && first.displaced_lines.is_empty(), "{first:?}");
        assert_eq!(depth(&d), 1, "the first position's frame");
        let pushed = d.drag(&node, &rules, &shove, p(5.0, -2.0));
        assert!(pushed.status, "{pushed:?}");
        let y1 = gnd_y(&pushed).expect("the GND track is pushed");
        assert_eq!(depth(&d), 2);
        let further = d.drag(&node, &rules, &shove, p(5.0, -2.4));
        assert!(further.status, "{further:?}");
        assert!(gnd_y(&further).expect("still pushed") < y1, "pushed on from the frame: {further:?}");
        assert_eq!(depth(&d), 3, "the bus still presses on the frame's GND track, so the frame stays");
        let back = d.drag(&node, &rules, &shove, p(5.0, -0.5));
        assert!(back.status && back.displaced_lines.is_empty(), "the GND track springs back: {back:?}");
        assert_eq!(depth(&d), 2, "both frames the bus no longer touches are popped, the bottom one stays, the new one is pushed");
        let commit = d.fix_route(&node, &shove).expect("commits");
        assert!(!commit.remove_track_ids.contains(&"tG".to_string()), "the GND track as the last position left it: {commit:?}");
        // the leaders (`restoreLeaderSegments`): each line's segment the guide through the cursor crosses, parallel to its leader before
        let leaders = d.leader_segments(&node);
        assert_eq!(leaders.len(), 3, "{leaders:?}");
        assert!(leaders.iter().any(|(_, s)| *s == (p(0.5, -0.5), p(9.5, -0.5))), "A's slid segment: {leaders:?}");
    }

    /// Before any position a release commits nothing (`FixRoute` of no `m_lastNode`), and where the mode could not resolve the position the
    /// leaders are the selected tracks' segments as they are (`m_leaderSegments = m_origDraggedItems` in `tryPosture`).
    #[test]
    fn nothing_is_committed_before_a_drag_and_an_unresolved_one_leads_with_the_selected_segments() {
        let (node, items) = bus();
        let d = MultiDragger::start(&node, p(5.0, 0.0), &items, Mode::Shove).expect("starts");
        assert!(d.fix_route(&node, &settings(Mode::Shove)).is_none());
        let leaders = d.leader_segments(&node);
        assert_eq!(leaders, vec![(0, (p(0.0, 0.0), p(10.0, 0.0))), (0, (p(0.0, 1.0), p(10.0, 1.0))), (0, (p(0.0, 2.0), p(10.0, 2.0)))]);
    }

    /// `MULTI_DRAGGER::Drag` reads the mode live (`switch( Settings().Mode() )`), but only a drag started in Shove has a `SHOVE` (`Start`): switched
    /// to Shove later, `multidragShove` resolves nothing ("if( !m_shove ) return false;", `m_lastNode` null: the world as it is). And where a position
    /// is not resolved, a click with "Allow DRC violations" commits KiCad's `m_lastNode` of it: nothing at all (the walk found no way, or no shove),
    /// or -- a shove that failed -- the frame it started from, which holds none of the dragged lines, so their tracks go.
    #[test]
    fn a_drag_started_in_walk_around_never_shoves_and_an_unresolved_position_commits_kicads_last_node() {
        let (mut node, items) = bus();
        track(&mut node, "GND", "tG", &[p(4.0, -2.2), p(6.0, -2.2)]);
        let rules = rules();
        let allow = RoutingSettings { mode: Mode::MarkObstacles, can_violate_drc: true, ..RoutingSettings::default() };
        let mut d = MultiDragger::start(&node, p(5.0, 0.0), &items, Mode::Walkaround).expect("starts");
        let drag = d.drag(&node, &rules, &settings(Mode::Shove), p(5.0, -2.0));
        assert!(!drag.status && drag.unresolved == Some(Unresolved::World), "{drag:?}");
        assert!(d.fix_route(&node, &settings(Mode::Shove)).is_none(), "refused");
        let commit = d.fix_route(&node, &allow).expect("allowed");
        assert!(commit.remove_track_ids.is_empty() && commit.tracks.is_empty(), "m_world: nothing changes: {commit:?}");
        // a shove that failed: the frame it started from, without the dragged lines
        let mut d = MultiDragger::start(&node, p(5.0, 0.0), &items, Mode::Shove).expect("starts");
        d.last_drag = Some(MultiDrag { lines: d.lines.iter().map(|l| (l.mdrag_index, l.dragged_line.clone())).collect(), status: false, colliding: true, unresolved: Some(Unresolved::ShoveFrame), ..MultiDrag::default() });
        let commit = d.fix_route(&node, &allow).expect("allowed");
        let mut removed = commit.remove_track_ids.clone();
        removed.sort();
        assert_eq!(removed, vec!["tA", "tB", "tC"]);
        assert!(commit.tracks.is_empty(), "the dragged lines are not in the frame: {commit:?}");
    }

    /// The joints a shove locks are the frame's (`LockJoint` in the node `pushSpringback` keeps; `NODE::Branch` copies them): each head's ends,
    /// carried by every frame after.
    #[test]
    fn the_bus_shoves_frames_keep_the_joints_it_locked() {
        let (mut node, items) = bus();
        track(&mut node, "GND", "tG", &[p(4.0, -2.2), p(6.0, -2.2)]);
        let rules = rules();
        let mut d = MultiDragger::start(&node, p(5.0, 0.0), &items, Mode::Shove).expect("starts");
        assert!(d.drag(&node, &rules, &settings(Mode::Shove), p(5.0, -2.0)).status);
        assert!(d.drag(&node, &rules, &settings(Mode::Shove), p(5.0, -2.5)).status);
        let shove = d.shove.as_ref().expect("the gesture's shove");
        let (top, _) = shove.springback.top();
        for end in [p(0.0, 0.0), p(10.0, 0.0), p(0.0, 2.0), p(10.0, 2.0)] {
            assert!(top.locked.iter().any(|(q, _)| *q == end), "{end:?} is locked in the frame: {:?}", top.locked);
            assert!(top.apply(&shove.pre_shove).joint_at(end, &top.locked.iter().find(|(q, _)| *q == end).unwrap().1).is_some_and(|j| j.locked));
        }
    }

    /// Highlight collisions: a release always commits (`multidragMarkObstacles` returns true), collisions and all -- a Ctrl+click is not needed.
    #[test]
    fn highlight_collisions_commits_whatever_it_runs_into() {
        let (mut node, items) = bus();
        track(&mut node, "GND", "tG", &[p(4.0, -2.2), p(6.0, -2.2)]);
        let mut d = MultiDragger::start(&node, p(5.0, 0.0), &items, Mode::Shove).expect("starts");
        let drag = d.drag(&node, &rules(), &settings(Mode::MarkObstacles), p(5.0, -2.0));
        assert!(drag.status && drag.colliding, "{drag:?}");
        assert!(d.fix_route(&node, &settings(Mode::MarkObstacles)).is_some());
    }

    /// `clipToOtherLine`: a line that runs into one dragged before it is cut back to the longest start of it that keeps clear of it (to 100 nm).
    #[test]
    fn a_line_is_clipped_where_it_runs_into_another() {
        let (node, _) = bus();
        let reference = Line::from_points(net_of("A"), 0, mm(0.2), vec![p(5.0, -5.0), p(5.0, 5.0)]);
        let mut clipped = Line::from_points(net_of("B"), 0, mm(0.2), vec![p(0.0, 0.0), p(10.0, 0.0)]);
        assert!(clip_to_other_line(&node, &rules(), &reference, &mut clipped));
        let end = *clipped.pts.last().unwrap();
        assert_eq!(clipped.pts.len(), 2);
        // it ends at the clearance (0.2 mm, less the 0.5 um epsilon) plus the two half widths (0.1 mm each) from the reference, to the
        // search's 100 nm and a little
        let gap = (mm(5.0) - end.x).get();
        assert!((399_400..=399_900).contains(&gap), "{gap}: {clipped:?}");
    }
}
