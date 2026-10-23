//! Port of `PNS::LINE_PLACER` (`pcbnew/router/pns_line_placer.{h,cpp}`) --
//! the interactive single-track routing state machine: a fixed "tail" of
//! already-placed runs, a volatile "head" from the last fixed point to the
//! cursor, posture (45-degree direction) tracking, via placement, and
//! end-of-route snapping.
//!
//! ## Why this is simpler than KiCad's version
//!
//! This project's frontend drives the backend over HTTP, one request per
//! mouse sample -- not a continuous in-process tick stream. Two
//! consequences follow directly, both documented in `PARITY.md`:
//!
//! 1. **The mouse trail is the cursor positions the studio asks about.**
//!    KiCad infers the posture from the *area* swept between two candidate
//!    paths and the path the mouse took (`MOUSE_TRAIL_TRACER`, ported in
//!    [`crate::mouse_trail`]). Here every `preview` is a `Move`: the cursor
//!    it is asked about is a point of the trail, as the studio sends one
//!    request per mouse sample, and the posture of the head is what the
//!    tracer says it is (`GetPosture`), unless "Use mouse path to set
//!    track posture" is off or `/` has flipped it (`flip_posture`). A fix
//!    starts a new trail from the last fixed point, continuing in the
//!    direction of the last segment, as `FixRoute` does.
//! 2. **No live "tail" mutation between fixes (`reduceTail`/`mergeHead`/
//!    `handlePullback`).** Those exist to stop a *continuously re-walked*
//!    tail from accumulating zig-zags across thousands of small mouse
//!    deltas. Here, the head is recomputed from scratch from the last
//!    fixed point on every `preview()` call -- there is no accumulated
//!    tail state to need periodic straightening. `handleSelfIntersections`'
//!    core correctness property (a head that loops back over its own
//!    fixed tail must retract, not overlap) doesn't need separate handling
//!    for the same reason: the head is always freshly built from the last
//!    fixed point, so it can't accumulate a stale self-crossing tail to
//!    begin with.
//!
//! Each fixed "run" is single-layer (`crate::line::Line`); a via placed
//! mid-route (`switch_layer`) ends the current run and starts a new one on
//! the new layer, mirroring the existing single-segment router's own
//! `dropViaAndSwitchLayer` (`web/studio/src/components/canvas/routing.ts`).
//!
//! **Springback** (`SHOVE`'s `m_nodeStack`, [`crate::springback`]): in Shove mode every `Move` starts its shove from the newest frame the
//! head still presses on (`reduceSpringback`) and pushes the world it leaves (`pushSpringback`), so a pushed track stays pushed, is pushed
//! on from where it was left, and springs back as the head draws back. A click pushes a locked frame with the fixed run in it (a track of
//! that world, which a later leg's shove may push, as KiCad's: the preview and the commit draw it as the newest frame has it,
//! [`LinePlacer::placed_runs`]), Backspace rewinds the stack to
//! the frame the undone run was fixed in, and what the commit replaces is what the newest frame has moved against the session's world
//! ([`LinePlacer::displaced_tracks`]). The `Node` passed to `preview()`/`fix()` is that world, and must be the same for the whole session: the
//! frames are diffs against it. In the other modes the head is resolved in the world the last click left (`m_currentNode` is
//! `m_lastNode` there); the fixed runs in it are the head's own net, which it never collides with.

/// `LINE_PLACER::rhWalkOnly`/`rhShoveOnly`'s head effort: `OE_LOW` merges
/// nothing, `OE_MEDIUM`/`OE_FULL` merge segments; smart pads on top when
/// enabled and the corner mode is a 45-degree one.
///
/// "Smart Pads is incompatible with 90-degree mode for now" and with a posture the person forced with `/`
/// (`!m_mouseTrailTracer.IsManuallyForced()`).
fn head_effort(settings: &RoutingSettings, manually_forced: bool) -> u32 {
    let merge = match settings.optimizer_effort {
        crate::settings::OptEffort::Low => 0,
        crate::settings::OptEffort::Medium | crate::settings::OptEffort::Full => optimizer::effort::MERGE_SEGMENTS,
    };
    // `cornerMode == DIRECTION_45::MITERED_45 || cornerMode == DIRECTION_45::ROUNDED_45`
    merge | if settings.smart_pads && !settings.corner_mode.is_90() && !manually_forced { optimizer::effort::SMART_PADS } else { 0 }
}

use crate::direction45::Direction45;
use crate::mouse_trail::MouseTrailTracer;
use crate::item::{Item, ItemId, Net};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::Node;
use crate::settings::{Mode, RoutingSettings};
use crate::shove::{DisplacedLine, DisplacedVia};
use crate::springback::{HeadSet, Springback, World};
use crate::{optimizer, walkaround};
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

/// `ANCHOR_SNAP_UM`-equivalent: how close the cursor must be to a same-net
/// pad/via/track-end to snap onto it and offer to finish the route there.
/// Matches the existing single-segment router's own snap radius
/// (`web/studio/src/components/canvas/routing.ts`'s `findRouteAnchor`
/// caller), so the two tools feel consistent.
pub const SNAP_UM: Um = 500;

/// How far off a point may be and still be the point: the studio's own snapping (`PcbSnap.routePoint`) has put the cursor exactly on a
/// pad, a via, a track end or a track's centre line, and the rounding to whole micrometres moves it at most this much.
pub const EXACT_UM: Um = 2;

#[derive(Debug, Clone)]
pub struct Preview {
    /// Already-fixed runs from earlier in this session, unchanged.
    pub runs: Vec<Line>,
    /// The live, not-yet-fixed head: last fixed point (or the route's
    /// origin) to the cursor, already walked/shoved/mark-obstacled per the
    /// active [`Mode`].
    pub head: Line,
    /// Whether `head` (or the pending via) currently collides with
    /// anything -- only meaningful in `Mode::MarkObstacles`, where
    /// collisions are reported rather than resolved.
    pub colliding: bool,
    /// A via pending at the head's end, if `placing_via` is set.
    pub via: Option<(Point, Um, Um)>,
    /// A same-net anchor near the cursor the route would snap onto and
    /// finish at, if committed now (`Move()`'s end-item snapping).
    pub snapped_end: Option<Point>,
    /// `snapped_end` is in the middle of this same-net track (`SplitAdjacentSegments( m_lastNode, aEndItem, splitPoint )`): the route
    /// ends on it, and finishing splits it there.
    pub snapped_segment: Option<ItemId>,
    /// Other tracks/vias `Mode::Shove` would displace if this preview were
    /// accepted right now -- empty in every other mode. Transient, exactly
    /// like `head`: nothing here is real until `fix`/`finish` absorbs it.
    pub displaced_lines: Vec<DisplacedLine>,
    pub displaced_vias: Vec<DisplacedVia>,
}

/// [`LinePlacer::build_head`]'s result: the resolved point list, whether
/// it (or the pending via) still collides, and -- `Mode::Shove` only --
/// whatever else had to move to make room for it.
struct HeadResult {
    pts: Vec<Point>,
    /// The arcs of the head (a rounded corner: `CORNER_MODE`'s `ROUNDED_45` / `ROUNDED_90`), as [`Line::arcs`].
    arcs: Vec<crate::arcs::ArcPts>,
    colliding: bool,
    displaced_lines: Vec<DisplacedLine>,
    displaced_vias: Vec<DisplacedVia>,
    /// The world the head was resolved in, against the session's: the springback frame the shove pushed, or the one it started from
    /// (`m_currentNode` after `rhShoveOnly` / `rhWalkOnly`). What a click fixes the head in.
    world: World,
    /// `Mode::Shove` with a via pending: the head was shoved together with its via (`rhShoveOnly`: `newHead.AppendVia( makeVia( ... ) )`
    /// before `m_shove->AddHeads( newHead )`), and this is where the via ended -- the head's last point. `None`: the via, if there is one,
    /// is placed on the finished head (`place_via`).
    via: Option<Point>,
}

impl HeadResult {
    /// A head resolved without a shove, in `world` (whose moved tracks and vias are what the preview shows displaced).
    fn plain(pts: Vec<Point>, colliding: bool, node: &Node, world: World) -> Self {
        HeadResult { pts, arcs: Vec::new(), colliding, displaced_lines: world.displaced_lines(node), displaced_vias: world.displaced_vias(), world, via: None }
    }

    /// [`Self::plain`] for a head that may hold arcs.
    fn plain_line(line: Line, colliding: bool, node: &Node, world: World) -> Self {
        HeadResult { arcs: line.arcs, ..Self::plain(line.pts, colliding, node, world) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixOutcome {
    /// Committed. `real_end` is `true` if this click reached a same-net
    /// anchor and finished the whole connection -- the caller should call
    /// [`LinePlacer::finish`] next rather than continue routing.
    Fixed { real_end: bool },
    /// Refused: the head (or pending via) still collides and the active
    /// mode doesn't allow committing a violation.
    Blocked,
}

pub struct LinePlacer {
    pub net: Net,
    pub width: Um,
    pub origin: Point,
    pub direction: Direction45,
    /// The posture the session started with (`m_initial_direction`): what
    /// Backspace goes back to once every run has been undone.
    initial_direction: Direction45,
    pub manually_forced: bool,
    /// `m_mouseTrailTracer`: the path the cursor took since the route (or the last fixed leg) started.
    trail: RefCell<MouseTrailTracer>,
    /// The cursor of the last `Move` and the tracer as it was before it: asking again about the same cursor (a click at the place the cursor is) starts
    /// from that state, so it gives the posture the person saw.
    last_move: RefCell<Option<(Point, MouseTrailTracer)>>,
    /// The direction of the first segment of the head last shown (`m_currentTrace.CSegment( 0 )`), which `/` flips from.
    last_head_dir: Cell<Direction45>,
    pub placing_via: bool,
    pub via_diameter: Um,
    pub via_drill: Um,
    /// The layer the route goes on to after the pending via (`m_sizes.GetLayerBottom()`, what `makeVia` spans to): the via collides on
    /// every layer between the current one and this. `None`: the via is taken as being on the current layer alone.
    pub via_to_layer: Option<i32>,
    pub runs: Vec<Line>,
    pub current_layer: i32,
    pub idle: bool,
    pub placement_correct: bool,
    /// Accumulated across every accepted (`fix`/`finish`-absorbed) shove
    /// this session, keyed by source id -- see [`Self::displaced_tracks`].
    // ordered, so the commit a session produces does not depend on hash order
    displaced_tracks: BTreeMap<String, Vec<Line>>,
    displaced_vias: BTreeMap<String, Point>,
    /// The segments the route cannot end on, even where the cursor is on them: the one it started in the middle of
    /// (`pickSingleItem( mousePos, net, layer, false, { m_startItem } )`), and its two halves.
    pub exclude_end: Vec<ItemId>,
    /// Where the route ended, in the middle of a same-net track: that track is split there when the route is committed.
    pub end_split: Option<(ItemId, Point)>,
    /// `m_shove`'s springback stack (`m_nodeStack`): the worlds the head's shoves left, newest last, and a locked one per click.
    springback: RefCell<Springback>,
    /// The world of the last `Move` (`m_currentNode`): the frame its shove pushed or started from.
    last_world: RefCell<World>,
    /// `m_fixedTail`'s stages, one per run: the length of the springback stack when the run was fixed -- its newest frame is the
    /// stage's `commit` node, which Backspace rewinds the stack to (`RewindSpringbackTo( st.commit )`).
    stages: Vec<usize>,
}

impl LinePlacer {
    /// `LINE_PLACER::Start`. `start_item`, if given, seeds the initial
    /// posture: continuing an existing segment keeps its direction
    /// (`DIRECTION_45(that segment, oriented away from aP)`); anything
    /// else (a pad, free space) leaves the generic default `N` KiCad's own
    /// `InitialDirection()` falls back to when "start diagonal" is off.
    pub fn start(node: &Node, p: Point, start_item: Option<ItemId>, net: Net, layer: i32, width: Um) -> Self {
        let direction = match start_item.and_then(|id| node.get(id)) {
            Some(Item::Segment(s)) if s.a == p => Direction45::from_seg(s.b, s.a),
            Some(Item::Segment(s)) if s.b == p => Direction45::from_seg(s.a, s.b),
            _ => Direction45::N,
        };
        // `LINE_PLACER::Start`: `m_mouseTrailTracer.Clear(); AddTrailPoint( aP ); SetTolerance( m_head.Width() ); SetDefaultDirections( m_initial_direction, UNDEFINED )`
        let mut trail = MouseTrailTracer::new();
        trail.clear();
        trail.add_trail_point(p);
        trail.set_tolerance(width);
        trail.set_default_directions(direction, Direction45::Undefined);
        LinePlacer { net, width, origin: p, direction, initial_direction: direction, manually_forced: false, trail: RefCell::new(trail), last_move: RefCell::new(None), last_head_dir: Cell::new(Direction45::Undefined), placing_via: false, via_diameter: 0, via_drill: 0, via_to_layer: None, runs: Vec::new(), current_layer: layer, idle: false, placement_correct: false, displaced_tracks: BTreeMap::new(), displaced_vias: BTreeMap::new(), exclude_end: Vec::new(), end_split: None, springback: RefCell::new(Springback::new()), last_world: RefCell::new(World::default()), stages: Vec::new() }
    }

    pub fn fixed_start(&self) -> Point {
        self.runs.last().and_then(|r| r.last()).unwrap_or(self.origin)
    }

    /// The items `build_head`'s collision tests ignore: none. A route that starts in the middle of a track
    /// (`SplitAdjacentSegments`, [`crate::router::Router::start`]) needs nothing here -- the two halves are on the
    /// route's own net, which never collides with it. Kept as its own method (rather than inlined) because
    /// that is where an exclusion set would plug in.
    fn exclude(&self) -> Vec<ItemId> {
        Vec::new()
    }

    /// `LINE_PLACER::buildInitialLine` + `routeHead`: the raw 45-degree
    /// candidate from the last fixed point to `p`, then resolved per the
    /// active routing mode. In `Mode::Shove` it reduces the springback
    /// stack, shoves from the newest frame left and pushes the world the
    /// shove leaves (`SHOVE::Run`); the result carries that world, and what
    /// it has moved, for [`Self::preview`]/[`Self::fix`].
    fn build_head(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> HeadResult {
        let start = self.fixed_start();
        // the world the last `Move` left (`m_currentNode`): in Shove mode the newest springback frame, in the others the world the last click
        // left (those modes never shove, so their `m_currentNode` is `m_lastNode` of the last `FixRoute`)
        let current = if settings.mode == Mode::Shove { self.springback.borrow().top().0 } else { self.springback.borrow().locked_top() };
        if start == p {
            return HeadResult::plain(vec![start], false, node, current);
        }
        // `buildInitialLine`: in Shove mode a pending via is pushed off the pads under it before the head is built to it (`v.PushoutForce( ..,
        // collMask = SOLID_T, .. )`, the head then ends at `aP + force`); the other modes place theirs on the finished head (`place_via`).
        let p = if self.placing_via && settings.mode == Mode::Shove { self.place_via(node, rules, settings, p, self.via_to_layer.unwrap_or(self.current_layer)) } else { p };
        // `buildInitialLine`: `if( GetFreeAngleMode() && Mode() == RM_MarkObstacles ) l = SHAPE_LINE_CHAIN( { m_p_start, aP } )`, else the trace in the
        // corner mode (`guessedDir.BuildInitialTrace( m_p_start, aP, false, cornerMode )`): a rounded corner is an arc of the head
        let mut raw = Line::new(self.net.clone(), self.current_layer, self.width);
        if settings.free_angle() {
            raw.pts = vec![start, p];
        } else {
            let trace = self.posture(settings, p).build_initial_line(start, p, false, settings.corner_mode);
            (raw.pts, raw.arcs) = (trace.pts, trace.arcs);
        }
        let exclude = self.exclude();
        let iteration_limit = settings.walkaround_iteration_limit.max(0) as u32;
        let effort = head_effort(settings, self.manually_forced);
        let optimize = |line: &Line, world: &Node| optimizer::optimize_constrained(line, world, rules, &exclude, effort, &optimizer::Constraints { corner_mode: settings.corner_mode, ..Default::default() });

        let mut current = current;
        if settings.mode == Mode::Shove {
            // `rhShoveOnly`: first walk the head around the pads alone
            // (`rhWalkBase( aP, walkSolids, ITEM::SOLID_T, RM_Shove )`), then
            // shove tracks and vias out of its way. A pad the head cannot be
            // walked around, a locked item, or the iteration limit falls back
            // to a full walkaround for this call, exactly like upstream.
            if let Some(walked) = walkaround::walk_base_line(node, rules, &raw, crate::node::kind_mask::SOLID, iteration_limit, settings.walkaround_hug_length_threshold, &exclude, settings.corner_mode) {
                // `if( m_placingVia && viaOk ) newHead.AppendVia( makeVia( newHead.CLastPoint() ) )`: the via is part of the head, so the shove
                // moves what is in the way of the via as well as what is in the way of the track (`SHOVE::Run`: `headVia->SetRank( 100000 )`,
                // `m_currentNode->Add( std::move( headVia ) )`)
                let head_via = if self.placing_via { walked.last().map(|end| self.make_via(end)) } else { None };
                // `SHOVE::Run`: "Pop NODEs containing previous shoves which are no longer necessary" (`reduceSpringback( headSet )`), then
                // `m_currentNode = parent->Branch()`: the shove starts from the world the newest frame the head still presses on left
                let head_set = HeadSet { pts: &walked.pts, net: &self.net, layer: self.current_layer, width: self.width, via: head_via.as_ref() };
                let (parent, prior_area) = self.springback.borrow_mut().reduce(node, &head_set, rules);
                let mut parent_node = parent.apply(node);
                // `SHOVE::Run`: "m_currentNode->LockJoint( head.CPoint( 0 ), &head, true ); if( !head.EndsWithVia() ) m_currentNode->LockJoint(
                // head.CLastPoint(), &head, true )" -- a line of the head's net the shove pushes (the route's own tail) ends there
                parent_node.lock_joint(start, &self.net);
                if let (None, Some(&end)) = (&head_via, walked.pts.last()) {
                    parent_node.lock_joint(end, &self.net);
                }
                let outcome = match &head_via {
                    Some(via) => crate::shove::shove_heads_after(&parent_node, vec![crate::shove::ShoveHead { line: walked.clone(), via: Some(via.clone()) }], rules, settings, prior_area),
                    None => crate::shove::shove_line_after(&parent_node, &walked, rules, settings, prior_area),
                };
                if let Some(outcome) = outcome {
                    // the head as the shove left it, with the arcs of it the shove did not cut into
                    let line = Line { arcs: walked.arcs.clone(), ..Line::from_points(self.net.clone(), self.current_layer, self.width, outcome.head.clone()) };
                    // Like `OPTIMIZER::Optimize( &aNewHead, effort, m_currentNode )`
                    // after the shove: against the world the shove left, so a
                    // shortcut is only taken where the pushed items now allow it.
                    let optimized = optimize(&line, &outcome.world);
                    let via = head_via.as_ref().and_then(|_| optimized.pts.last().copied());
                    // `pushSpringback( m_currentNode, m_affectedArea )`: the world this shove left is the newest frame
                    let mut world = parent;
                    world.merge_shove(&outcome.displaced_lines, &outcome.displaced_vias);
                    self.springback.borrow_mut().push(node, world.clone(), outcome.area);
                    return HeadResult { pts: optimized.pts, arcs: optimized.arcs, colliding: false, displaced_lines: world.displaced_lines(node), displaced_vias: world.displaced_vias(), world, via };
                }
                // the shove failed: "m_currentNode = parent", and `rhWalkOnly` walks the head in it
                current = parent;
            }
        }

        // `rhWalkOnly` / `rhMarkObstacles` resolve the head in `m_currentNode`: the session's world as the springback frame has it (its
        // moved tracks and vias; the fixed runs in it are the head's own net, which it never collides with)
        let moved;
        let world_node = if current.tracks.is_empty() && current.vias.is_empty() {
            node
        } else {
            moved = current.apply(node);
            &moved
        };
        match settings.mode {
            Mode::MarkObstacles => {
                let colliding = raw.pts.windows(2).any(|w| {
                    let shape = Shape::Stadium { a: w[0], b: w[1], r: self.width / 2 };
                    world_node.first_colliding(&shape, &self.net, LayerRange::single(self.current_layer), rules, &exclude).is_some()
                });
                HeadResult::plain_line(raw, colliding, node, current)
            }
            Mode::Walkaround | Mode::Shove => {
                // `rhWalkOnly`: `rhWalkBase( aP, walkFull, ITEM::ANY_T, RM_Walkaround )`, refused if the walk still
                // collides with anything, then the head optimized per the effort setting.
                let walked = walkaround::walk_base_line(world_node, rules, &raw, crate::node::kind_mask::ANY, iteration_limit, settings.walkaround_hug_length_threshold, &exclude, settings.corner_mode)
                    .filter(|path| !walkaround::path_collides(world_node, rules, &self.net, self.current_layer, self.width, &path.pts, &exclude));
                match walked {
                    Some(path) => HeadResult::plain_line(optimize(&path, world_node), false, node, current),
                    None => HeadResult::plain_line(raw, true, node, current), // ST_STUCK: show the direct line, flagged violating
                }
            }
        }
    }

    /// `m_mouseTrailTracer.GetPosture( aP )` (`buildInitialLine`): the direction the head's elbow is built from. With "Use mouse path to set track
    /// posture" off the tracer only keeps the posture the route started with or `/` flipped it to (`SetMouseDisabled`).
    fn posture(&self, settings: &RoutingSettings, p: Point) -> Direction45 {
        let mut trail = self.trail.borrow_mut();
        trail.set_mouse_disabled(!settings.auto_posture);
        trail.get_posture(p)
    }

    /// The start of a `Move` to `p`: the tracer goes back to how it was before the last `Move` when that was to the same cursor, and is remembered
    /// otherwise.
    fn begin_move(&self, p: Point) {
        let mut last = self.last_move.borrow_mut();
        match &*last {
            Some((at, before)) if *at == p => *self.trail.borrow_mut() = before.clone(),
            _ => *last = Some((p, self.trail.borrow().clone())),
        }
    }

    /// `m_mouseTrailTracer.Clear(); SetTolerance( m_head.Width() ); AddTrailPoint( start ); SetDefaultDirections( direction, UNDEFINED )`: a new trail
    /// from the point a fix (or a via) leaves the route at.
    fn restart_trail(&self, start: Point, direction: Direction45) {
        let mut t = self.trail.borrow_mut();
        t.clear();
        t.set_tolerance(self.width);
        t.add_trail_point(start);
        t.set_default_directions(direction, Direction45::Undefined);
        *self.last_move.borrow_mut() = None;
    }

    /// `makeVia( pos )`: a via of the pending size on the layers from the current one to the one the route goes on to.
    fn make_via(&self, pos: Point) -> crate::item::Via {
        crate::item::Via { net: self.net.clone(), layers: LayerRange::new(self.current_layer, self.via_to_layer.unwrap_or(self.current_layer)), pos, diameter: self.via_diameter, drill: self.via_drill, source_via: None, locked: false, is_virtual: false }
    }

    /// Via placement for the pending via at the head's end (`LINE_PLACER::buildInitialLine`'s via part): put it at `at` and,
    /// where it sits on something, push it out by `VIA::PushoutForce` along the lead from where this leg started -- on pads
    /// and everything else in `Mode::Walkaround`, on pads alone in `Mode::Shove` (the tracks and vias it meets are the shove's
    /// to move), not at all in `Mode::MarkObstacles` -- at most `ROUTING_SETTINGS::ViaForcePropIterationLimit` steps. A via
    /// that does not come free stays where the cursor put it, and the caller's own collision check marks it.
    fn place_via(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, at: Point, to_layer: i32) -> Point {
        let mask = match settings.mode {
            Mode::MarkObstacles => return at,
            Mode::Walkaround => crate::node::kind_mask::ANY,
            Mode::Shove => crate::node::kind_mask::SOLID,
        };
        let via = crate::item::Via { net: self.net.clone(), layers: LayerRange::new(self.current_layer, to_layer), pos: at, diameter: self.via_diameter, drill: self.via_drill, source_via: None, locked: false, is_virtual: false };
        let start = self.fixed_start();
        let lead = (at.x - start.x, at.y - start.y);
        match crate::shove::via_pushout_force(node, rules, &via, lead, mask, settings.via_force_prop_iteration_limit, &[]) {
            Some(force) => Point { x: at.x + force.0 as Um, y: at.y + force.1 as Um },
            None => at,
        }
    }

    /// `LINE_PLACER::Move`: the live preview, including end-of-route
    /// snapping onto a same-net anchor near `p`. What it changes is the
    /// mouse trail and, in `Mode::Shove`, the springback stack (a frame for
    /// the world this shove left); nothing is committed until
    /// [`Self::fix`]/[`Self::finish`] accepts it (see the module doc comment).
    pub fn preview(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> Preview {
        self.begin_move(p);
        let (mut snapped_end, snapped_segment) = self.snap_end(node, settings, p);
        let target = snapped_end.unwrap_or(p);
        let head_result = self.build_head(node, rules, settings, target);
        *self.last_world.borrow_mut() = head_result.world.clone();
        let mut colliding = head_result.colliding;
        let shoved_via = head_result.via;
        let mut head = Line::from_points(self.net.clone(), self.current_layer, self.width, head_result.pts);
        head.arcs = head_result.arcs;
        head.simplify();
        // `Move`: a route that ends on a track it runs along (its last segment collinear with the track's and overlapping it) ends
        // where it first touches the track, not at the cursor further along it
        if let (Some(seg), true) = (snapped_segment, head.segment_count() > 0) {
            if let Some(Item::Segment(target_seg)) = node.get(seg) {
                let n = head.pts.len();
                let (last_seg, track) = ((head.pts[n - 2], head.pts[n - 1]), (target_seg.a, target_seg.b));
                if crate::seg_ops::collinear(track, last_seg) && crate::seg_ops::overlaps(track, last_seg) {
                    let split_point = eda_drc::kimath::Seg::new(track.0, track.1).nearest_point(last_seg.0);
                    head.pts[n - 1] = split_point;
                    head.simplify();
                    snapped_end = Some(split_point);
                }
            }
        }
        let via = if self.placing_via {
            let via_pos = head.last().unwrap_or(target);
            if shoved_via.is_some() {
                // shoved together with the head: what was in its way has been moved, so the via is where the head ends and collides with nothing
                Some((via_pos, self.via_diameter, self.via_drill))
            } else {
                let to_layer = self.via_to_layer.unwrap_or(self.current_layer);
                let placed = self.place_via(node, rules, settings, via_pos, to_layer);
                let shape = Shape::Circle { c: placed, r: self.via_diameter / 2 };
                if node.first_colliding(&shape, &self.net, LayerRange::new(self.current_layer, to_layer), rules, &[]).is_some() {
                    colliding = true;
                }
                Some((placed, self.via_diameter, self.via_drill))
            }
        } else {
            None
        };
        // `Move`: "m_mouseTrailTracer.AddTrailPoint( aP )" once the head is routed to it
        self.trail.borrow_mut().add_trail_point(target);
        if let Some(w) = head.pts.windows(2).find(|w| w[0] != w[1]) {
            self.last_head_dir.set(Direction45::from_seg(w[0], w[1]));
        }
        Preview { runs: head_result.world.placed_runs(&self.runs), head, colliding, via, snapped_end, snapped_segment, displaced_lines: head_result.displaced_lines, displaced_vias: head_result.displaced_vias }
    }

    /// Where a cursor at `p` ends the route, if it does (`TOOL_BASE::updateEndItem`, `pickSingleItem` for the net, `snapToItem`): on
    /// the same-net pad, via or track end it is on or near; or, with "snap to tracks" (the magnetic setting), in the middle of the
    /// same-net track it is on -- the studio's own snapping has put it exactly on the centre line -- which is then split there. The
    /// track the route started in the middle of is not an end. `(the point, the track when it is in the middle of one)`.
    fn snap_end(&self, node: &Node, settings: &RoutingSettings, p: Point) -> (Option<Point>, Option<ItemId>) {
        let layers = LayerRange::single(self.current_layer);
        if let Some((_, a)) = node.nearest_anchor(p, layers, EXACT_UM, Some(&self.net)) {
            return (Some(a), None);
        }
        if settings.snap_to_tracks {
            if let Some(id) = node.segment_at(p, self.current_layer, Some(&self.net), Some(EXACT_UM), &self.exclude_end) {
                match node.get(id) {
                    Some(Item::Segment(s)) => {
                        let seg = eda_drc::kimath::Seg::new(s.a, s.b);
                        let on = if seg.sq_distance_to_point(p) == 0 { p } else { seg.nearest_point(p) };
                        return (Some(on), Some(id));
                    }
                    // `snapToItem` for `ARC_T`: the point of the arc nearest the cursor (`AlignToArc`)
                    Some(Item::Arc(a)) => {
                        let on = if a.arc.distance_to(p) < 0.5 { p } else { a.arc.nearest_point(p) };
                        return (Some(on), Some(id));
                    }
                    _ => {}
                }
            }
        }
        match node.nearest_anchor(p, layers, SNAP_UM, Some(&self.net)) {
            Some((_, a)) => (Some(a), None),
            None => (None, None),
        }
    }

    /// `LINE_PLACER::FixRoute`'s own `!Settings().AllowDRCViolations()`
    /// guard: in every mode except `MarkObstacles` (whose whole point is
    /// "let the user route through a violation and fix it later"), a
    /// still-colliding head must never be committed.
    fn may_commit_despite_collision(settings: &RoutingSettings) -> bool {
        settings.allow_drc_violations()
    }

    /// What the commit replaces and moves is what the world of the last `Move` has moved against the session's world (the springback
    /// frame the head was resolved in: every shove since the route began that is still in it, the earlier legs' included).
    fn take_displacement(&mut self, world: &World) {
        self.displaced_tracks = world.moved_tracks();
        self.displaced_vias = world.vias.clone();
    }

    /// The fixed runs as the world of the last `Move` has them (a later leg's shove may have pushed the route's own tail, as KiCad's does:
    /// [`World::placed_runs`]): what the preview draws and the commit writes. [`Self::runs`] are the runs as they were fixed, whose ends -- the
    /// points each leg started from, locked joints for every shove -- are where they were.
    pub fn placed_runs(&self) -> Vec<Line> {
        self.last_world.borrow().placed_runs(&self.runs)
    }

    /// `FixRoute`: `m_shove->AddLockedSpringbackNode( m_lastNode )` -- the world of the last `Move` with the head just fixed in it (and the
    /// via it ends in) is a frame that is never popped.
    fn lock_fixed(&mut self, node: &Node, run: Option<&Line>, via: Option<crate::item::Via>) {
        let world = self.last_world.borrow().with_fixed(run, via);
        self.take_displacement(&world);
        self.springback.borrow_mut().add_locked(node, world.clone());
        *self.last_world.borrow_mut() = world;
    }

    /// `LINE_PLACER::FixRoute` for an intermediate click: commit the
    /// current head (per `preview`'s own geometry, so the two can never
    /// disagree) as a fixed run, advance posture from its last segment,
    /// and reset for the next leg.
    pub fn fix(&mut self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> FixOutcome {
        let preview = self.preview(node, rules, settings, p);
        if preview.colliding && !Self::may_commit_despite_collision(settings) {
            return FixOutcome::Blocked;
        }
        if preview.head.point_count() < 2 {
            // Zero movement: only a legitimate "finish" if it's onto a
            // real anchor, otherwise there is nothing to fix.
            return if preview.snapped_end.is_some() { FixOutcome::Fixed { real_end: true } } else { FixOutcome::Blocked };
        }
        let real_end = preview.snapped_end.is_some();
        if real_end {
            self.record_end_split(&preview);
        }
        // `FixRoute`: with "Fix all segments on click" off, a click that does not end the route fixes every segment but
        // the last, which stays free and follows the cursor; the next leg starts where the last one began
        // (`m_currentStart = p_pre_last`) and continues in the direction of the one before it (`lastDirSeg = CSegment( -2 )`).
        // "Rollback doesn't work properly if fix-all isn't enabled and we are placing arcs, so if we are, act as though we are in fix-all mode."
        let keep_last_free = !settings.fix_all_segments && preview.head.arc_count() == 0 && !real_end && !self.placing_via && preview.head.segment_count() > 1;
        let mut head = preview.head.clone();
        if keep_last_free {
            head.pts.pop();
        }
        self.direction = head.pts.windows(2).rev().find(|w| w[0] != w[1]).map(|w| Direction45::from_seg(w[0], w[1])).unwrap_or(self.direction);
        // the stage's `commit` is the newest frame now (`m_fixedTail.AddStage( .., m_currentNode )`)
        self.stages.push(self.springback.borrow().len());
        self.lock_fixed(node, Some(&head), None);
        self.runs.push(head);
        self.placement_correct = true;
        if !real_end {
            // `FixRoute`: "m_mouseTrailTracer.Clear(); SetTolerance( m_head.Width() ); AddTrailPoint( m_currentStart ); SetDefaultDirections( lastSegDir, UNDEFINED )"
            self.restart_trail(self.fixed_start(), self.direction);
        }
        FixOutcome::Fixed { real_end }
    }

    /// A route that ends in the middle of a track splits it where it ends (`Move`: `SplitAdjacentSegments( m_lastNode, aEndItem,
    /// splitPoint )`, when the head `reachesEnd`): remember where, for the commit.
    fn record_end_split(&mut self, preview: &Preview) {
        if let (Some(seg), Some(end)) = (preview.snapped_segment, preview.snapped_end) {
            if preview.head.last() == Some(end) {
                self.end_split = Some((seg, end));
            }
        }
    }

    /// `LINE_PLACER::ToggleViaPlacement` + the layer-switch half of the
    /// existing single-segment router's `dropViaAndSwitchLayer`: fix the
    /// current head ending in a via, then continue routing from the via's
    /// position on `new_layer`.
    #[allow(clippy::too_many_arguments)] // the query context (node/rules/settings/p) plus the via's own (layer, diameter, drill) -- splitting either group into its own type would just move the count, not reduce it.
    pub fn switch_layer(&mut self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point, new_layer: i32, diameter: Um, drill: Um) {
        self.via_to_layer = Some(new_layer);
        self.begin_move(p);
        let head_result = self.build_head(node, rules, settings, p);
        let shoved_via = head_result.via;
        *self.last_world.borrow_mut() = head_result.world;
        let mut head = Line::from_points(self.net.clone(), self.current_layer, self.width, head_result.pts);
        head.arcs = head_result.arcs;
        head.simplify();
        let via_pos = match shoved_via {
            // shoved together with the head: the via is where the head ends
            Some(_) => head.last().unwrap_or(p),
            None => self.place_via(node, rules, settings, head.last().unwrap_or(p), new_layer),
        };
        let via = crate::item::Via { layers: LayerRange::new(self.current_layer, new_layer), ..self.make_via(via_pos) };
        let via = crate::item::Via { diameter, drill, ..via };
        if head.point_count() >= 2 {
            self.stages.push(self.springback.borrow().len());
            self.lock_fixed(node, Some(&head), Some(via));
            self.runs.push(head);
        } else {
            self.lock_fixed(node, None, Some(via));
        }
        self.current_layer = new_layer;
        self.via_diameter = diameter;
        self.via_drill = drill;
        self.origin = via_pos; // the next run (if `runs` was empty) starts here
        self.placing_via = false;
        self.via_to_layer = None;
        self.placement_correct = true;
        // `FixRoute` after a via: "lastSegDir = pl.EndsWithVia() ? DIRECTION_45::UNDEFINED : d_last"
        self.restart_trail(via_pos, Direction45::Undefined);
    }

    /// `LINE_PLACER::UnfixRoute` (Backspace): drop the last fixed run,
    /// restoring posture to whatever the new last run ended with (or the
    /// route's initial direction if none remain). A no-op once there is
    /// nothing left to undo, mirroring `FIXED_TAIL::PopStage`'s refusal to
    /// shrink below the route's own origin.
    pub fn undo_last_segment(&mut self) -> bool {
        let Some(popped) = self.runs.pop() else { return false };
        // `UnfixRoute`: `m_shove->RewindSpringbackTo( st.commit )` -- the frames from the one the run was fixed in are gone, and with them
        // what its shoves had moved
        if let Some(len) = self.stages.pop() {
            self.springback.borrow_mut().rewind_to(len);
        }
        let world = self.springback.borrow().top().0;
        self.take_displacement(&world);
        *self.last_world.borrow_mut() = world;
        self.direction = self
            .runs
            .last()
            .and_then(|r| r.pts.windows(2).rev().find(|w| w[0] != w[1]))
            .map(|w| Direction45::from_seg(w[0], w[1]))
            .unwrap_or(self.initial_direction);
        let _ = popped;
        // `UnfixRoute`: "m_mouseTrailTracer.Clear(); SetDefaultDirections( m_initial_direction, m_direction ); AddTrailPoint( m_p_start )"
        {
            let mut t = self.trail.borrow_mut();
            t.clear();
            t.set_default_directions(self.initial_direction, self.direction);
            t.add_trail_point(self.fixed_start());
        }
        *self.last_move.borrow_mut() = None;
        self.manually_forced = false;
        true
    }

    /// `LINE_PLACER::FlipPosture` (the `/` key): rotate the posture by one
    /// 45-degree octant and lock it (no further implicit changes -- there
    /// is no automatic posture heuristic to lock against in this port, see
    /// the module doc comment, but the flag is kept for parity/clarity).
    pub fn flip_posture(&mut self) {
        // "In order to fix issue 12369 get the current line placer first direction and copy it to the mouse trail tracer, as the current placer may have
        // changed the route."
        let mut t = self.trail.borrow_mut();
        let first = self.last_head_dir.get();
        if !t.is_manually_forced() && first != Direction45::Undefined {
            t.set_default_directions(first, Direction45::Undefined);
        }
        t.flip_posture();
        self.direction = t.direction();
        self.manually_forced = true;
        // the next `Move` starts from the flipped tracer, not from how it was before the last one
        *self.last_move.borrow_mut() = None;
    }

    pub fn toggle_via(&mut self, enabled: bool, diameter: Um, drill: Um) {
        self.placing_via = enabled;
        self.via_diameter = diameter;
        self.via_drill = drill;
        if !enabled {
            self.via_to_layer = None;
        }
    }

    /// `LINE_PLACER::FixRoute` with `aForceFinish`/reaching a real end:
    /// commit the final head (subject to the same collision guard as
    /// [`Self::fix`]) regardless of whether it lands exactly on a same-net
    /// anchor, and return every run this session produced, ready for the
    /// caller to turn into `Track`/`Via` IR (`crate::router`). `None` if
    /// the final head still collides and the mode refuses to commit it --
    /// the session is left unmodified (idle/placement state untouched) so
    /// the caller can keep routing instead.
    pub fn finish(&mut self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> Option<Vec<Line>> {
        let preview = self.preview(node, rules, settings, p);
        if preview.colliding && !Self::may_commit_despite_collision(settings) {
            return None;
        }
        let world = self.last_world.borrow().clone();
        self.take_displacement(&world);
        let real_end = preview.snapped_end.is_some();
        if real_end {
            self.record_end_split(&preview);
        }
        if preview.head.point_count() >= 2 {
            self.runs.push(preview.head);
        }
        self.idle = true;
        self.placement_correct = !self.runs.is_empty();
        if settings.remove_loops && real_end {
            self.remove_loops(node);
        }
        Some(self.placed_runs())
    }

    /// `LINE_PLACER::removeLoops`, adapted to this port's "one commit per
    /// whole finished connection" granularity (`crate::router`'s own doc
    /// comment on why a multi-run, via-switching session still becomes one
    /// undo step): once the connection reaches a real end (lands on a
    /// same-net anchor), check whether its own two endpoints -- the
    /// route's true start (the very first point of its first run) and the
    /// anchor it just landed on -- are *also* already joined by some
    /// other, pre-existing same-net path (`Node::find_lines_between_joints`).
    /// If so, that other path is now a redundant parallel connection
    /// between the same two electrical points and gets deleted, same as
    /// upstream -- unless any of its segments are locked (upstream's own
    /// "don't remove locked tracks" rule). This session's own runs are
    /// never inserted into `node` while routing (see the module doc
    /// comment), so every match `find_lines_between_joints` returns here
    /// is necessarily a *different*, already-committed line, never the one
    /// just placed.
    ///
    /// Deliberately coarser than upstream, which re-checks per internal
    /// segment of the new line (each one can itself be a `JOINT` if the
    /// new route happened to touch an intermediate branch point) -- this
    /// checks only the whole connection's two outer endpoints, matching
    /// the "whole finished line is the unit" adaptation the rest of this
    /// crate's commit model already makes. The common case (a route
    /// finished directly between two points already connected by one
    /// redundant pre-existing path) behaves identically; a new route that
    /// also happens to graze a third, unrelated branch point partway
    /// through is not specially handled.
    fn remove_loops(&mut self, node: &Node) {
        let (Some(start), Some(end)) = (self.runs.first().and_then(|r| r.first()), self.runs.last().and_then(|r| r.last())) else { return };
        if start == end {
            return;
        }
        for line in node.find_lines_between_joints(start, end, &self.net) {
            let has_locked_segment = line.segment_ids.iter().any(|id| node.get(*id).is_some_and(|it| it.is_track() && it.is_locked()));
            if has_locked_segment {
                continue;
            }
            let mut track_ids: Vec<String> = line.segment_ids.iter().filter_map(|id| node.get(*id).and_then(|it| it.source_track_id()).map(str::to_string)).collect();
            track_ids.sort_unstable();
            track_ids.dedup();
            for track_id in track_ids {
                // An empty replacement (`point_count() < 2`) tells the
                // eventual caller (`crate::router::Router::build_commit`)
                // "remove this track, nothing replaces it" -- the same
                // convention a fully-retracted shove already relies on.
                self.displaced_tracks.insert(track_id, Vec::new());
            }
        }
    }

    /// Every other track this session's shove (if any) displaced, by
    /// source `Track::id`, keyed so the latest position for a given track
    /// across however many fix/finish calls touched it wins -- the
    /// caller's final commit must remove and re-add each of these
    /// alongside this session's own new runs, in the same undo step
    /// (`crate::router`/`Cmd::CommitRoute`).
    pub fn displaced_tracks(&self) -> impl Iterator<Item = (&str, &[Line])> {
        self.displaced_tracks.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    /// Every via this session's shove moved, by source `Via::id`.
    pub fn displaced_vias(&self) -> impl Iterator<Item = (&str, Point)> {
        self.displaced_vias.iter().map(|(k, &v)| (k.as_str(), v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{net_of, Solid};
    use crate::node::Node;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200\nclearance: 200\nvia_drill: 300\nvia_diameter: 600\n").unwrap()
    }

    #[test]
    fn straight_route_commits_on_fix_and_finish() {
        let node = Node::new();
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        let pv = placer.preview(&node, &rules, &settings, Point { x: 1000, y: 0 });
        assert!(!pv.colliding);
        assert_eq!(pv.head.last(), Some(Point { x: 1000, y: 0 }));
        assert_eq!(placer.fix(&node, &rules, &settings, Point { x: 1000, y: 0 }), FixOutcome::Fixed { real_end: false });
        assert_eq!(placer.runs.len(), 1);
        let finished = placer.finish(&node, &rules, &settings, Point { x: 2000, y: 0 }).expect("collision-free finish must succeed");
        assert!(placer.idle);
        assert!(!finished.is_empty());
        let total_len: f64 = finished.iter().map(|l| l.length()).sum();
        assert!(total_len > 0.0);
    }

    #[test]
    fn walks_around_an_obstacle_between_fixed_points() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 2500, y: 0 }, shape: Shape::Circle { c: Point { x: 2500, y: 0 }, r: 500 }, source: "U1.1".into(), edge: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        let placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        let pv = placer.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert!(!pv.colliding, "walkaround must clear the obstacle");
        assert!(pv.head.point_count() > 2, "must actually detour, not run straight through the pad");
    }

    /// D8: MarkObstacles refuses a colliding commit unless `can_violate_drc`.
    #[test]
    fn mark_obstacles_commits_collision_only_when_can_violate_drc() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 1000, y: 0 }, shape: Shape::Circle { c: Point { x: 1000, y: 0 }, r: 500 }, source: "U1.1".into(), edge: false }));
        let rules = rules();
        let mut settings = RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() };
        assert!(!settings.can_violate_drc);
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        assert_eq!(placer.fix(&node, &rules, &settings, Point { x: 2000, y: 0 }), FixOutcome::Blocked);
        assert!(placer.finish(&node, &rules, &settings, Point { x: 2000, y: 0 }).is_none());
        settings.can_violate_drc = true;
        assert!(matches!(placer.fix(&node, &rules, &settings, Point { x: 2000, y: 0 }), FixOutcome::Fixed { .. }));
    }

    /// `GetFixAllSegments()` off: a click fixes every segment but the last, which stays free (`FixRoute`'s `lastV`).
    #[test]
    fn without_fix_all_segments_a_click_leaves_the_last_segment_free() {
        let node = Node::new();
        let rules = rules();
        let target = Point { x: 3000, y: 1000 };
        let mut all = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        let head = placer.preview(&node, &rules, &all, target).head.pts;
        assert_eq!(head.len(), 3, "an elbow of two legs: {head:?}");
        assert_eq!(placer.fix(&node, &rules, &all, target), FixOutcome::Fixed { real_end: false });
        assert_eq!(placer.runs[0].pts, head, "by default the whole head is fixed");
        assert_eq!(placer.fixed_start(), target);

        all.fix_all_segments = false;
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        assert_eq!(placer.fix(&node, &rules, &all, target), FixOutcome::Fixed { real_end: false });
        assert_eq!(placer.runs[0].pts, head[..2].to_vec(), "only the first leg is fixed");
        assert_eq!(placer.fixed_start(), head[1], "and the next one starts where the free one did");
        assert_eq!(placer.direction, Direction45::from_seg(head[0], head[1]), "continuing in the direction of the leg before it");
        // A click that ends the route fixes the lot, whatever the setting says.
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        let finished = placer.finish(&node, &rules, &all, target).unwrap();
        assert_eq!(finished[0].pts, head);
    }

    /// `WalkaroundHugLengthThreshold()`: a head whose walkaround is more than twice the threshold times the direct length
    /// hugs the obstacle instead of chasing the cursor round it.
    #[test]
    fn a_long_detour_hugs_the_obstacle_unless_the_hug_threshold_allows_it() {
        let mut node = Node::new();
        // a 20 mm wall of GND between the start and the cursor, 4 mm apart: the way round is 5 times as long
        node.add(Item::Segment(crate::item::Segment { net: net_of("GND"), layer: 0, a: Point { x: 2000, y: -10_000 }, b: Point { x: 2000, y: 10_000 }, width: 200, source_track: None, locked: false }));
        let rules = rules();
        let cursor = Point { x: 4000, y: 0 };
        let placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);

        let hugging = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        let pv = placer.preview(&node, &rules, &hugging, cursor);
        assert!(!pv.colliding);
        let end = pv.head.last().unwrap();
        assert_ne!(end, cursor, "the head does not go the 20 mm round to the cursor: {:?}", pv.head.pts);
        assert!(end.x < 2000 && pv.head.length() < 6000.0, "it stops at the wall's clearance, on the near side: {:?}", pv.head.pts);

        let patient = RoutingSettings { walkaround_hug_length_threshold: 3.0, ..hugging.clone() };
        let pv = placer.preview(&node, &rules, &patient, cursor);
        assert!(!pv.colliding);
        assert_eq!(pv.head.last(), Some(cursor), "with a threshold of 3 the 21 mm detour is taken: {:?}", pv.head.pts);
        assert!(pv.head.length() > 20_000.0);
    }

    /// `ViaForcePropIterationLimit()`: how many steps `VIA::PushoutForce` has to get the via off a pad.
    #[test]
    fn a_via_placed_on_a_pad_is_pushed_off_within_the_via_force_iteration_limit() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 0, y: 0 }, shape: Shape::Circle { c: Point { x: 0, y: 0 }, r: 500 }, source: "U1.1".into(), edge: false }));
        let rules = rules();
        let mut placer = LinePlacer::start(&node, Point { x: 3000, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.toggle_via(true, 600, 300);
        let on_the_pad = Point { x: 200, y: 300 };
        let settings = RoutingSettings::default();
        let placed = placer.place_via(&node, &rules, &settings, on_the_pad, 1);
        let centre_distance = ((placed.x as f64).powi(2) + (placed.y as f64).powi(2)).sqrt();
        assert!(centre_distance >= 1000.0 - 2.0, "the 600 um via is {centre_distance} um from the 500 um pad's centre: {placed:?}");
        // four steps of a quarter of the via are not enough to leave this pad
        let tight = RoutingSettings { via_force_prop_iteration_limit: 4, ..settings.clone() };
        assert_eq!(placer.place_via(&node, &rules, &tight, on_the_pad, 1), on_the_pad, "out of steps: the via stays where the cursor put it");
        // Highlight collisions never moves a via.
        let marking = RoutingSettings { mode: Mode::MarkObstacles, ..settings };
        assert_eq!(placer.place_via(&node, &rules, &marking, on_the_pad, 1), on_the_pad);
    }

    #[test]
    fn undo_last_segment_restores_previous_state() {
        let node = Node::new();
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.fix(&node, &rules, &settings, Point { x: 1000, y: 0 });
        placer.fix(&node, &rules, &settings, Point { x: 1000, y: 1000 }); // (0,0) stayed fixed-start for this leg; the first fix moved it to (1000,0)
        assert_eq!(placer.runs.len(), 2);
        assert!(placer.undo_last_segment());
        assert_eq!(placer.runs.len(), 1);
        assert_eq!(placer.fixed_start(), Point { x: 1000, y: 0 });
        assert!(placer.undo_last_segment());
        assert_eq!(placer.runs.len(), 0);
        assert!(!placer.undo_last_segment(), "nothing left to undo");
    }

    #[test]
    fn snaps_onto_a_same_net_pad_and_reports_real_end() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: 3000, y: 0 }, shape: Shape::Circle { c: Point { x: 3000, y: 0 }, r: 400 }, source: "U2.1".into(), edge: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        let outcome = placer.fix(&node, &rules, &settings, Point { x: 3010, y: 10 });
        assert_eq!(outcome, FixOutcome::Fixed { real_end: true }, "a click near a same-net pad must finish the route");
        assert_eq!(placer.runs[0].last(), Some(Point { x: 3000, y: 0 }), "the fixed point must snap exactly onto the pad centre");
    }

    #[test]
    fn via_and_layer_switch_starts_a_new_run_on_the_new_layer() {
        let node = Node::new();
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.toggle_via(true, 600, 300);
        placer.switch_layer(&node, &rules, &settings, Point { x: 1000, y: 0 }, 1, 600, 300);
        assert_eq!(placer.current_layer, 1);
        assert_eq!(placer.runs.len(), 1);
        assert_eq!(placer.runs[0].layer, 0);
        assert_eq!(placer.origin, Point { x: 1000, y: 0 });
        let pv = placer.preview(&node, &rules, &settings, Point { x: 2000, y: 0 });
        assert_eq!(pv.head.layer, 1);
    }

    #[test]
    fn shove_mode_pushes_a_crossing_track_and_records_it_for_commit() {
        use crate::item::Segment;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: -2000 }, b: Point { x: 2500, y: 2000 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);

        let pv = placer.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert!(!pv.colliding, "shove must resolve the crossing, not just report it");
        assert_eq!(pv.head.pts, vec![Point { x: 0, y: 0 }, Point { x: 5000, y: 0 }], "shove moves the obstacle, not the pusher");
        assert_eq!(pv.displaced_lines.len(), 1);
        assert!(placer.displaced_tracks().next().is_none(), "preview alone must not persist anything");

        let outcome = placer.finish(&node, &rules, &settings, Point { x: 5000, y: 0 }).expect("collision-free shoved finish must succeed");
        assert_eq!(outcome.len(), 1);
        let displaced: Vec<_> = placer.displaced_tracks().collect();
        assert_eq!(displaced.len(), 1);
        assert_eq!(displaced[0].0, "trkA");
    }

    /// `rhShoveOnly`: with a pad on the straight line AND a track to push, Shove mode walks the head around the
    /// pad first and then pushes the track -- it used to give up at the pad and fall back to Walkaround, which
    /// leaves the track where it is and routes around that too.
    #[test]
    fn shove_mode_walks_around_a_pad_and_still_pushes_a_track() {
        use crate::item::{Segment, Solid};
        use crate::layer::LayerRange;
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 2000, y: 0 }, shape: Shape::Circle { c: Point { x: 2000, y: 0 }, r: 400 }, source: "U1.1".into(), edge: false }));
        node.add(Item::Segment(Segment { net: net_of("PWR"), layer: 0, a: Point { x: 4300, y: -2500 }, b: Point { x: 4300, y: -300 }, width: 200, source_track: Some(("trkP".into(), 0)), locked: false }));
        let rules = rules();
        let placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        let shove = placer.preview(&node, &rules, &RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() }, Point { x: 8000, y: 0 });
        assert!(!shove.colliding);
        assert!(shove.head.point_count() > 2, "the head had to go around the pad: {:?}", shove.head.pts);
        assert!(shove.head.pts.iter().any(|p| p.y.abs() > 500), "{:?}", shove.head.pts);
        assert_eq!(shove.head.last(), Some(Point { x: 8000, y: 0 }));
        // contrast: Walkaround routes around both and moves neither
        let walk = placer.preview(&node, &rules, &RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() }, Point { x: 8000, y: 0 });
        assert!(walk.displaced_lines.is_empty());
        assert!(walk.head.length() >= shove.head.length() - 1.0, "pushing the track needs no more detour than walking around it");
    }

    /// `rhShoveOnly`: `newHead.AppendVia( makeVia( newHead.CLastPoint() ) )` before `m_shove->AddHeads( newHead, SHP_SHOVE )` -- a head that ends in a
    /// via is shoved together with it, so what the via sits on (on every layer it spans) is pushed aside as what the track meets is. The via used to be
    /// placed on the finished head and only checked, so a track under it was reported as a collision the shove would not resolve.
    #[test]
    fn a_head_that_ends_in_a_via_shoves_what_the_via_meets_on_the_other_layer() {
        use crate::item::Segment;
        let mut node = Node::new();
        // GND on the other layer, 50 um inside the via's clearance ring and clear of the head's own line
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 1, a: Point { x: 4450, y: -3000 }, b: Point { x: 4450, y: 3000 }, width: 200, source_track: Some(("trkG".into(), 0)), locked: false }));
        let rules = rules();
        let shove = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.toggle_via(true, 600, 300);
        placer.via_to_layer = Some(1);
        let pv = placer.preview(&node, &rules, &shove, Point { x: 4000, y: 0 });
        assert!(!pv.colliding, "{pv:?}");
        assert_eq!(pv.head.pts, vec![Point { x: 0, y: 0 }, Point { x: 4000, y: 0 }]);
        assert_eq!(pv.via, Some((Point { x: 4000, y: 0 }, 600, 300)));
        assert_eq!(pv.displaced_lines.len(), 1, "the track under the via is pushed aside: {:?}", pv.displaced_lines);
        // the track bulges round the via: abreast of it, it is the via's edge (4300) + the clearance (200) + the track's half width (100) away
        let bulge: Vec<_> = pv.displaced_lines[0].line.pts.iter().filter(|q| q.y.abs() <= 300).collect();
        assert!(!bulge.is_empty() && bulge.iter().all(|q| q.x >= 4600 - 1), "{:?}", pv.displaced_lines[0].line.pts);
        // Highlight collisions only reports it: the same via collides with the track on the other layer
        let mark = RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() };
        let pv = placer.preview(&node, &rules, &mark, Point { x: 4000, y: 0 });
        assert!(pv.colliding && pv.displaced_lines.is_empty());
        // and Walk around pushes the via off the track rather than the track off the via
        let walk = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        let pv = placer.preview(&node, &rules, &walk, Point { x: 4000, y: 0 });
        assert!(!pv.colliding && pv.displaced_lines.is_empty() && pv.via.is_some_and(|(at, _, _)| at != Point { x: 4000, y: 0 }), "{pv:?}");
        // and a shove that cannot move the track (locked) leaves the via flagged, as the walk-around fallback finds it
        let mut locked = Node::new();
        locked.add(Item::Segment(Segment { net: net_of("GND"), layer: 1, a: Point { x: 4450, y: -3000 }, b: Point { x: 4450, y: 3000 }, width: 200, source_track: Some(("trkG".into(), 0)), locked: true }));
        // (a new route: a placer's springback frames are of the world it routes in)
        let mut placer = LinePlacer::start(&locked, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.toggle_via(true, 600, 300);
        placer.via_to_layer = Some(1);
        let pv = placer.preview(&locked, &rules, &shove, Point { x: 4000, y: 0 });
        assert!(pv.displaced_lines.is_empty() && pv.colliding, "a locked track is not shoved, and the via on it is flagged: {pv:?}");
    }

    /// The click that places the via (`switch_layer`) takes the shoved track along into the commit, and the next run starts on the via.
    #[test]
    fn placing_the_via_of_a_shoved_head_records_what_it_pushed() {
        use crate::item::Segment;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 1, a: Point { x: 4450, y: -3000 }, b: Point { x: 4450, y: 3000 }, width: 200, source_track: Some(("trkG".into(), 0)), locked: false }));
        let rules = rules();
        let shove = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.toggle_via(true, 600, 300);
        placer.switch_layer(&node, &rules, &shove, Point { x: 4000, y: 0 }, 1, 600, 300);
        assert_eq!(placer.current_layer, 1);
        assert_eq!(placer.runs.len(), 1);
        assert_eq!(placer.runs[0].pts, vec![Point { x: 0, y: 0 }, Point { x: 4000, y: 0 }]);
        assert_eq!(placer.origin, Point { x: 4000, y: 0 });
        let displaced: Vec<_> = placer.displaced_tracks().collect();
        assert_eq!(displaced.len(), 1);
        assert_eq!(displaced[0].0, "trkG");
        assert!(displaced[0].1.iter().all(|l| l.pts.iter().filter(|q| q.y.abs() <= 300).all(|q| q.x >= 4600 - 1)), "{displaced:?}");
        assert!(!placer.placing_via && placer.via_to_layer.is_none());
    }

    /// The board a session would commit: the session's world with the tracks and vias it moved, and its runs.
    fn committed_world(node: &Node, placer: &LinePlacer) -> Node {
        let world = World {
            tracks: placer.displaced_tracks().map(|(t, l)| (t.to_string(), l.to_vec())).collect(),
            vias: placer.displaced_vias().map(|(v, p)| (v.to_string(), p)).collect(),
            fixed: placer.placed_runs(),
            fixed_vias: Vec::new(),
        };
        world.apply(node)
    }

    /// Every pair of items of different nets in `node` closer than the rules allow.
    fn violations(node: &Node, rules: &BoardRules) -> Vec<(ItemId, ItemId)> {
        let mut out = Vec::new();
        for (id, item) in node.iter() {
            for o in node.all_colliding(&item.shape(item.layers().start()), item.net(), item.layers(), rules, &[id]) {
                out.push((id.min(o.id), id.max(o.id)));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn crossing_track() -> Node {
        use crate::item::Segment;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 2500, y: -2000 }, b: Point { x: 2500, y: 2000 }, width: 200, source_track: Some(("trkA".into(), 0)), locked: false }));
        node
    }

    /// `SHOVE::reduceSpringback` / `pushSpringback`: a track the head pushed stays pushed as long as the head presses on it, is pushed on from
    /// where it was left as the head goes further, and springs back to where it was once the head has drawn back off it.
    #[test]
    fn a_pushed_track_is_pushed_on_from_where_it_was_left_and_springs_back_when_the_head_draws_back() {
        let (node, rules) = (crossing_track(), rules());
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        // the first move (KiCad's first frame: the head a few pixels long) touches nothing
        assert!(placer.preview(&node, &rules, &settings, Point { x: 500, y: 0 }).displaced_lines.is_empty());
        let reach = |pv: &Preview| pv.displaced_lines.iter().flat_map(|d| d.line.pts.iter()).map(|q| q.x).max().unwrap_or(2500);
        let pushed = placer.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert_eq!(pushed.displaced_lines.len(), 1);
        assert!(reach(&pushed) >= 5000 + 100 + 200 + 100 - 1, "{:?}", pushed.displaced_lines);
        let further = placer.preview(&node, &rules, &settings, Point { x: 6000, y: 0 });
        assert!(reach(&further) >= 6000 + 400 - 1, "{:?}", further.displaced_lines);
        assert_eq!(placer.springback.borrow().len(), 3, "a frame per move that shoved");
        // drawn back off the track: every frame it no longer presses on is popped, down to the first
        let back = placer.preview(&node, &rules, &settings, Point { x: 1500, y: 0 });
        assert!(back.displaced_lines.is_empty(), "the track springs back: {:?}", back.displaced_lines);
        assert_eq!(placer.springback.borrow().len(), 2, "the first frame and the one this move pushed");
    }

    /// `FixRoute`: `AddLockedSpringbackNode( m_lastNode )` -- the world a click leaves (the track the leg pushed, and the leg itself) is where
    /// the next leg is routed and shoves from, so the next leg pushes the track on from there and never back across the fixed leg; and
    /// `UnfixRoute`: `RewindSpringbackTo( st.commit )` -- Backspace takes the push back with the leg.
    #[test]
    fn the_next_leg_shoves_from_the_world_the_click_left_and_backspace_takes_the_push_back() {
        let (node, rules) = (crossing_track(), rules());
        let settings = RoutingSettings { mode: Mode::Shove, fix_all_segments: true, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.preview(&node, &rules, &settings, Point { x: 500, y: 0 });
        placer.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert_eq!(placer.fix(&node, &rules, &settings, Point { x: 5000, y: 0 }), FixOutcome::Fixed { real_end: false });
        assert_eq!(placer.displaced_tracks().count(), 1, "the click keeps the push");
        // a leg going north from the click runs into the track where the first leg left it (bulged round x = 5000)
        let mut path = vec![Point { x: 5000, y: -500 }, Point { x: 5000, y: -1500 }, Point { x: 5000, y: -3000 }];
        let last = path.pop().unwrap();
        for p in path {
            placer.preview(&node, &rules, &settings, p);
        }
        let pv = placer.preview(&node, &rules, &settings, last);
        assert!(!pv.colliding);
        assert_eq!(pv.displaced_lines.len(), 1, "the track, as both legs have pushed it: {:?}", pv.displaced_lines);
        placer.finish(&node, &rules, &settings, last).expect("finishes");
        let board = committed_world(&node, &placer);
        assert!(violations(&board, &rules).is_empty(), "the commit is clean: {:?}", violations(&board, &rules));

        // Backspace after the first click: the track is back where it was
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.preview(&node, &rules, &settings, Point { x: 500, y: 0 });
        placer.fix(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert_eq!(placer.displaced_tracks().count(), 1);
        assert!(placer.undo_last_segment());
        assert_eq!(placer.displaced_tracks().count(), 0, "the leg and its push are gone");
        assert!(placer.preview(&node, &rules, &settings, Point { x: 1500, y: 0 }).displaced_lines.is_empty());
    }

    /// `UnfixRoute`: `RewindSpringbackTo( st.commit )`. When no shove of the undone leg succeeded, its stage's `commit` is the locked frame the
    /// click before left (`m_currentNode` was the parent `Run` fell back to), and KiCad erases that frame too: the earlier leg is gone from the
    /// world while it is still fixed, and `CommitPlacement` (`RewindToLastLockedNode`) then commits a frame without it. Here the earlier leg's
    /// frame stays.
    #[test]
    fn backspace_after_a_leg_whose_shoves_all_failed_keeps_the_leg_before() {
        let (node, rules) = (Node::new(), rules());
        let settings = RoutingSettings { mode: Mode::Shove, fix_all_segments: true, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.preview(&node, &rules, &settings, Point { x: 500, y: 0 });
        placer.preview(&node, &rules, &settings, Point { x: 5000, y: 0 });
        assert_eq!(placer.fix(&node, &rules, &settings, Point { x: 5000, y: 0 }), FixOutcome::Fixed { real_end: false });
        // the second leg: every shove fails (the iteration limit), so the head is walked in the first click's world
        let failing = RoutingSettings { shove_iteration_limit: 1, ..settings.clone() };
        let len = placer.springback.borrow().len();
        placer.preview(&node, &rules, &failing, Point { x: 5000, y: -1000 });
        assert_eq!(placer.springback.borrow().len(), len, "no shove succeeded, no frame was pushed");
        assert_eq!(placer.fix(&node, &rules, &failing, Point { x: 5000, y: -1000 }), FixOutcome::Fixed { real_end: false });
        assert!(placer.undo_last_segment());
        let world = placer.springback.borrow().locked_top();
        assert_eq!(world.fixed.len(), 1, "the first leg is still in the world");
        assert_eq!(world.fixed[0].pts, placer.runs[0].pts);
    }

    /// `FixRoute` adds the fixed segments to the world as ordinary tracks (`m_lastNode->Add( segment )`), so a later leg's shove that pushes
    /// another net's track into the route's own tail pushes the tail on; the ends of each run stay (`LockJoint` of each head's start). Here:
    /// three legs (east, north, west), then a fourth heads south onto GND's track between it and the first leg: GND is pushed into the first
    /// leg, which bulges away, the head reaches the cursor, the preview draws the pushed run and the commit is clean. (With the runs locked, the
    /// review found, the head stopped 150 um short of the cursor.)
    #[test]
    fn a_later_leg_pushes_the_routes_own_tail_as_kicads_shove_does() {
        use crate::item::Segment;
        let rules = rules();
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 3000, y: -700 }, b: Point { x: 7000, y: -700 }, width: 200, source_track: Some(("B".into(), 0)), locked: false }));
        let settings = RoutingSettings { mode: Mode::Shove, fix_all_segments: true, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        for (moves, fix) in [(vec![(500, 0), (5000, 0)], (10000, 0)), (vec![(10000, -1000)], (10000, -3000)), (vec![(9000, -3000)], (5000, -3000))] {
            for m in moves {
                placer.preview(&node, &rules, &settings, Point { x: m.0, y: m.1 });
            }
            assert_eq!(placer.fix(&node, &rules, &settings, Point { x: fix.0, y: fix.1 }), FixOutcome::Fixed { real_end: false });
        }
        let mut last = None;
        for y in [-2500, -2000, -1500, -1200, -1000, -900, -800, -700, -650] {
            last = Some(placer.preview(&node, &rules, &settings, Point { x: 5000, y }));
        }
        let pv = last.unwrap();
        assert!(!pv.colliding);
        assert_eq!(pv.head.last(), Some(Point { x: 5000, y: -650 }), "the head reaches the cursor: {:?}", pv.head.pts);
        assert_eq!(pv.displaced_lines.len(), 1, "GND's track, not the route's own run: {:?}", pv.displaced_lines);
        let first = &pv.runs[0];
        assert_eq!((first.first(), first.last()), (Some(Point { x: 0, y: 0 }), Some(Point { x: 10000, y: 0 })), "its ends stay: {:?}", first.pts);
        assert!(first.pts.iter().any(|q| q.y > 0), "the first leg bulges away from the push: {:?}", first.pts);
        assert_eq!(pv.runs.len(), 3);
        placer.finish(&node, &rules, &settings, Point { x: 5000, y: -650 }).expect("finishes");
        assert!(placer.placed_runs()[0].pts.iter().any(|q| q.y > 0), "and the commit writes it pushed");
        let board = committed_world(&node, &placer);
        assert!(violations(&board, &rules).is_empty(), "the commit is clean: {:?}", violations(&board, &rules));
    }

    /// `CORNER_MODE` (`buildInitialLine`: `BuildInitialTrace( m_p_start, aP, false, cornerMode )`): arc 45 makes the diagonal leg a 45-degree arc
    /// tangent to the straight one, arc 90 the corner a quarter arc, 90 two axis legs; a click fixes the arc as an arc, and the head keeps it through
    /// the walk and the optimizer, in every mode.
    #[test]
    fn the_corner_mode_shapes_the_head_and_a_rounded_corner_is_an_arc_of_it() {
        use crate::direction45::CornerMode;
        let (node, rules) = (Node::new(), rules());
        let to = Point { x: 6000, y: 2000 };
        for mode in [Mode::MarkObstacles, Mode::Walkaround, Mode::Shove] {
            for corner in [CornerMode::Mitered45, CornerMode::Rounded45, CornerMode::Mitered90, CornerMode::Rounded90] {
                let settings = RoutingSettings { mode, corner_mode: corner, auto_posture: false, ..RoutingSettings::default() };
                let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
                placer.direction = Direction45::E;
                placer.restart_trail(Point { x: 0, y: 0 }, Direction45::E);
                let pv = placer.preview(&node, &rules, &settings, to);
                assert!(!pv.colliding);
                assert_eq!((pv.head.first(), pv.head.last()), (Some(Point { x: 0, y: 0 }), Some(to)), "{mode:?} {corner:?}");
                let arcs = pv.head.arc_runs();
                match corner {
                    CornerMode::Mitered45 => assert_eq!(pv.head.pts, vec![Point { x: 0, y: 0 }, Point { x: 4000, y: 0 }, to], "{mode:?}"),
                    CornerMode::Mitered90 => assert_eq!(pv.head.pts, vec![Point { x: 0, y: 0 }, Point { x: 6000, y: 0 }, to], "{mode:?}"),
                    CornerMode::Rounded45 | CornerMode::Rounded90 => {
                        assert_eq!(arcs.len(), 1, "{mode:?} {corner:?}: {:?}", pv.head);
                        let arc = pv.head.arcs[arcs[0].arc];
                        let sweep = crate::arcs::ArcPts::central_angle(&arc).abs();
                        let want = if corner == CornerMode::Rounded45 { 45.0 } else { 90.0 };
                        assert!((sweep - want).abs() < 0.5, "{mode:?} {corner:?}: a {sweep} degree arc");
                        // it leaves the straight leg along it (tangent) and ends at the cursor
                        assert_eq!(pv.head.pts[0].y, 0);
                        assert_eq!(pv.head.last(), Some(to));
                    }
                }
                assert!(matches!(placer.fix(&node, &rules, &settings, to), FixOutcome::Fixed { .. }));
                assert_eq!(placer.runs[0].arc_count(), usize::from(matches!(corner, CornerMode::Rounded45 | CornerMode::Rounded90)), "{mode:?} {corner:?}: the fixed run keeps its arc");
            }
        }
    }

    /// `rhShoveOnly` with a rounded head: the head and its arc push the track in their way, and the head is still the arc it was.
    #[test]
    fn a_rounded_head_shoves_what_its_arc_meets() {
        use crate::direction45::CornerMode;
        use crate::item::Segment;
        let mut node = Node::new();
        // across the arc of the corner (the arc of radius ~4.8 mm from (1172, 0) to (6000, 2000))
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: 4500, y: -2000 }, b: Point { x: 4500, y: 4000 }, width: 200, source_track: Some(("g".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, corner_mode: CornerMode::Rounded45, auto_posture: false, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.direction = Direction45::E;
        placer.restart_trail(Point { x: 0, y: 0 }, Direction45::E);
        let pv = placer.preview(&node, &rules, &settings, Point { x: 6000, y: 2000 });
        assert!(!pv.colliding);
        assert_eq!(pv.head.arc_count(), 1, "{:?}", pv.head);
        assert_eq!(pv.displaced_lines.len(), 1, "the track is pushed");
        placer.finish(&node, &rules, &settings, Point { x: 6000, y: 2000 }).expect("finishes");
        let board = committed_world(&node, &placer);
        assert!(violations(&board, &rules).is_empty(), "{:?}", violations(&board, &rules));
    }

    /// `buildInitialLine`: a via that lands on a pad is pushed off it before the head is built, and the head ends at the via.
    #[test]
    fn a_head_via_on_a_pad_is_pushed_off_it_and_the_head_ends_at_the_via() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 4000, y: 100 }, shape: Shape::Circle { c: Point { x: 4000, y: 100 }, r: 400 }, source: "U1.1".into(), edge: false }));
        let rules = rules();
        let shove = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        placer.toggle_via(true, 600, 300);
        let pv = placer.preview(&node, &rules, &shove, Point { x: 4000, y: 0 });
        assert!(!pv.colliding, "{pv:?}");
        let (at, d, _) = pv.via.expect("a via");
        assert_eq!(pv.head.last(), Some(at), "the head ends at the via");
        let gap = (((at.x - 4000) as f64).powi(2) + ((at.y - 100) as f64).powi(2)).sqrt() - 400.0 - d as f64 / 2.0;
        assert!(gap >= 200.0 - 2.0, "the via clears the pad by the clearance: {gap} um at {at:?}");
    }

    /// `MOUSE_TRAIL_TRACER` (`buildInitialLine`: `guessedDir = m_mouseTrailTracer.GetPosture( aP )`): the head's elbow follows the path the mouse took --
    /// along a diagonal and then straight gives a head that starts diagonal -- and with "Use mouse path to set track posture" off it is the posture the route
    /// started with.
    #[test]
    fn the_mouse_path_sets_the_posture_of_the_head_unless_the_setting_is_off() {
        let (node, rules) = (Node::new(), rules());
        let on = RoutingSettings::default();
        assert!(on.auto_posture, "KiCad's default");
        let off = RoutingSettings { auto_posture: false, ..on.clone() };
        let first_direction = |settings: &RoutingSettings, up_first: bool| {
            let placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
            let mut head = Vec::new();
            for i in 1..=12 {
                head = placer.preview(&node, &rules, settings, if up_first { Point { x: 500 * i, y: -500 * i } } else { Point { x: 500 * i, y: 0 } }).head.pts;
            }
            for i in 1..=12 {
                head = placer.preview(&node, &rules, settings, if up_first { Point { x: 6000 + 500 * i, y: -6000 } } else { Point { x: 6000 + 500 * i, y: -500 * i } }).head.pts;
            }
            assert_eq!(head.len(), 3, "{head:?}");
            Direction45::from_seg(head[0], head[1])
        };
        assert!(first_direction(&on, true).is_diagonal(), "the mouse went up the diagonal first, so does the head");
        assert!(!first_direction(&on, false).is_diagonal(), "and straight first");
        // off, the posture is the initial one (straight first, `N` is not diagonal) whatever the mouse did
        assert!(!first_direction(&off, true).is_diagonal());
        assert!(!first_direction(&off, false).is_diagonal());
    }

    /// `FlipPosture` (`/`): the posture flips from the direction of the head shown and stays flipped whatever the mouse does; a fix starts a new trail that
    /// continues in the direction of the last segment (`FixRoute`).
    #[test]
    fn a_flip_stays_and_a_fix_continues_in_the_direction_of_the_last_segment() {
        let (node, rules) = (Node::new(), rules());
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        let shown = placer.preview(&node, &rules, &settings, Point { x: 6000, y: -2000 }).head.pts;
        assert_eq!(Direction45::from_seg(shown[0], shown[1]), Direction45::E, "straight first at the start");
        placer.flip_posture();
        assert!(placer.manually_forced);
        let flipped = placer.preview(&node, &rules, &settings, Point { x: 6000, y: -2000 }).head.pts;
        assert!(Direction45::from_seg(flipped[0], flipped[1]).is_diagonal(), "E flipped is the next octant: diagonal first, {flipped:?}");
        // however the mouse moves, the flip holds
        for i in 1..=10 {
            let moved = placer.preview(&node, &rules, &settings, Point { x: 6000 + 500 * i, y: -2000 - 700 * i }).head.pts;
            assert!(Direction45::from_seg(moved[0], moved[1]).is_diagonal(), "{moved:?}");
        }
        // a fix: the next leg starts in the direction the last one ended in, and the flip is forgotten (`m_mouseTrailTracer.Clear()`)
        let at = Point { x: 6000, y: -2000 };
        assert!(matches!(placer.fix(&node, &rules, &settings, at), FixOutcome::Fixed { real_end: false }));
        let last = placer.runs[0].pts.windows(2).rev().find(|w| w[0] != w[1]).map(|w| Direction45::from_seg(w[0], w[1])).unwrap();
        let next = placer.preview(&node, &rules, &settings, Point { x: 12_000, y: -5000 }).head.pts;
        assert_eq!(Direction45::from_seg(next[0], next[1]), last, "continues in the direction of the last segment: {next:?} after {:?}", placer.runs[0].pts);
    }

    /// `OE_LOW` merges nothing in the head; the default merges segments.
    #[test]
    fn head_effort_follows_the_optimizer_setting() {
        use crate::settings::OptEffort;
        let low = RoutingSettings { optimizer_effort: OptEffort::Low, smart_pads: false, ..RoutingSettings::default() };
        assert_eq!(head_effort(&low, false), 0);
        let medium = RoutingSettings { optimizer_effort: OptEffort::Medium, smart_pads: true, ..RoutingSettings::default() };
        assert_eq!(head_effort(&medium, false), optimizer::effort::MERGE_SEGMENTS | optimizer::effort::SMART_PADS);
        assert_eq!(head_effort(&medium, true), optimizer::effort::MERGE_SEGMENTS, "a posture forced with `/` leaves the pad connection alone");
    }

    #[test]
    fn finishing_a_route_onto_an_already_connected_same_net_anchor_removes_the_redundant_old_path() {
        use crate::item::Segment;
        let mut node = Node::new();
        // A pre-existing direct connection between (0,0) and (3000,0),
        // same net as the new route below -- once the new route also joins
        // those same two points, this one becomes a redundant parallel
        // path and should be deleted (`RoutingSettings::remove_loops`
        // defaults `true`, matching upstream).
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 3000, y: 0 }, width: 200, source_track: Some(("trkOld".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings::default();
        assert!(settings.remove_loops, "sanity: the default this test exercises");
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);

        let outcome = placer.finish(&node, &rules, &settings, Point { x: 3000, y: 0 }).expect("must finish onto the old track's own endpoint");
        assert_eq!(outcome.len(), 1, "the new connection itself is still placed");

        let displaced: Vec<_> = placer.displaced_tracks().collect();
        assert_eq!(displaced.len(), 1);
        assert_eq!(displaced[0].0, "trkOld");
        assert!(displaced[0].1.iter().all(|l| l.point_count() < 2), "no replacement geometry -- the old track is simply removed, not moved");
    }

    #[test]
    fn remove_loops_leaves_a_locked_redundant_path_alone() {
        use crate::item::Segment;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 3000, y: 0 }, width: 200, source_track: Some(("trkOld".into(), 0)), locked: true }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);

        placer.finish(&node, &rules, &settings, Point { x: 3000, y: 0 }).expect("must finish");
        assert!(placer.displaced_tracks().next().is_none(), "a locked redundant path must never be removed, matching upstream");
    }

    #[test]
    fn remove_loops_is_a_no_op_when_the_setting_is_off() {
        use crate::item::Segment;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: 0, y: 0 }, b: Point { x: 3000, y: 0 }, width: 200, source_track: Some(("trkOld".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { remove_loops: false, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);

        placer.finish(&node, &rules, &settings, Point { x: 3000, y: 0 }).expect("must finish");
        assert!(placer.displaced_tracks().next().is_none(), "remove_loops: false must leave the redundant path in place");
    }
}
