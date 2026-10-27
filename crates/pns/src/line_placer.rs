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
use eda_model::ir::{Point, Nm};
use eda_model::BoardRules;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

/// `ANCHOR_SNAP_NM`-equivalent: how close the cursor must be to a same-net
/// pad/via/track-end to snap onto it and offer to finish the route there.
/// Matches the existing single-segment router's own snap radius
/// (`web/studio/src/components/canvas/routing.ts`'s `findRouteAnchor`
/// caller), so the two tools feel consistent.
pub const SNAP_NM: Nm = Nm::new(500_000);

/// How far off a point may be and still be the point: the studio's own snapping (`PcbSnap.routePoint`) has put the cursor exactly on a
/// pad, a via, a track end or a track's centre line, and the rounding of the browser's coordinates moves it at most this much (the 2 um
/// the studio's micrometre coordinates once allowed).
pub const EXACT_NM: Nm = Nm::new(2_000);

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
    pub via: Option<(Point, Nm, Nm)>,
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
#[derive(Clone)]
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
    /// With a via pending, where the head's walk or shove left it -- the head's last point: pushed off what it sat on before the head was
    /// walked to it (`rhWalkBase` / `buildInitialLine`), or shoved together with the head (`rhShoveOnly`: `newHead.AppendVia( makeVia( ... ) )`
    /// before `m_shove->AddHeads( newHead )`). `None`: the via, if there is one, is placed on the finished head (`place_via`; Highlight
    /// collisions, or a walk that got stuck).
    via: Option<Point>,
    /// `via` was a head of the shove: what was in its way has been moved, so it collides with nothing.
    via_shoved: bool,
    /// Not routed to this cursor: `routeHead` failed and this is the head of the last `Move` that routed (or an empty one).
    kept: bool,
}

impl HeadResult {
    /// A head resolved without a shove, in `world` (whose moved tracks and vias are what the preview shows displaced).
    fn plain(pts: Vec<Point>, colliding: bool, node: &Node, world: World) -> Self {
        HeadResult { pts, arcs: Vec::new(), colliding, displaced_lines: world.displaced_lines(node), displaced_vias: world.displaced_vias(), world, via: None, via_shoved: false, kept: false }
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
    pub width: Nm,
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
    pub via_diameter: Nm,
    pub via_drill: Nm,
    /// The layer the route goes on to after the pending via (`m_sizes.GetLayerBottom()`, what `makeVia` spans to): the via collides on
    /// every layer between the current one and this. `None`: the via is taken as being on the current layer alone.
    pub via_to_layer: Option<i32>,
    /// The layers the pending via itself spans, when they are not "from the current layer to [`Self::via_to_layer`]" (`LINE_PLACER::makeVia`:
    /// a `VIATYPE::THROUGH` via spans `F_Cu` .. `B_Cu`, a blind/buried or micro via the sizes' layer pair, `m_sizes.GetLayerTop()` ..
    /// `GetLayerBottom()`, which need not include the layer the route is on -- `ROUTER_TOOL::handleLayerSwitch` puts the pair there). `None`:
    /// from the current layer to `via_to_layer`, what `V` (`layerToggle`) has always placed.
    pub via_span: Option<LayerRange>,
    pub runs: Vec<Line>,
    /// The vias the session's fixes placed (`FixRoute`: "if( pl.EndsWithVia() ) { .. m_lastNode->Add( std::move( newVia ) ); }"), in order,
    /// each with the number of [`Self::runs`] there were once it was placed: Backspace takes a via away with the run it ended (or, for one
    /// placed with no segment before it, with the run before it). One `finish` places, when a via is armed, is the last.
    pub fixed_vias: Vec<(crate::item::Via, usize)>,
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
    /// `m_head` of the last `Move` that routed (`routeStep`: `LINE prevHead( m_head )`): what a `Move` whose `routeHead` fails shows and a
    /// click then fixes. `None` once a leg is fixed (the new leg's head is empty).
    last_good: RefCell<Option<HeadResult>>,
    /// `m_fixedTail`'s stages, one per run: the length of the springback stack when the run was fixed -- its newest frame is the
    /// stage's `commit` node, which Backspace rewinds the stack to (`RewindSpringbackTo( st.commit )`).
    stages: Vec<usize>,
}

impl LinePlacer {
    /// `LINE_PLACER::Start`: the posture starts at `InitialDirection()`, `N` ("start diagonal" off) whatever the route starts on -- KiCad
    /// computes the direction of a segment it starts from (`lastSegDir`, `initialDir`) but hands the tracer `m_initial_direction`
    /// (`SetDefaultDirections( m_initial_direction, UNDEFINED )`). `start_item` is kept for the callers' shape.
    pub fn start(node: &Node, p: Point, start_item: Option<ItemId>, net: Net, layer: i32, width: Nm) -> Self {
        // `m_initial_direction = Settings().InitialDirection()`: `N` (`NE` with "start diagonal", which the studio does not set). The start
        // segment's direction KiCad's `Start` works out (`lastSegDir`, `initialDir`) is never used (findings p5 F2-12).
        let _ = (node, start_item);
        let direction = Direction45::N;
        // `LINE_PLACER::Start`: `m_mouseTrailTracer.Clear(); AddTrailPoint( aP ); SetTolerance( m_head.Width() ); SetDefaultDirections( m_initial_direction, UNDEFINED )`
        let mut trail = MouseTrailTracer::new();
        trail.clear();
        trail.add_trail_point(p);
        trail.set_tolerance(width);
        trail.set_default_directions(direction, Direction45::Undefined);
        LinePlacer { net, width, origin: p, direction, initial_direction: direction, manually_forced: false, trail: RefCell::new(trail), last_move: RefCell::new(None), last_head_dir: Cell::new(Direction45::Undefined), placing_via: false, via_diameter: Nm::ZERO, via_drill: Nm::ZERO, via_to_layer: None, via_span: None, runs: Vec::new(), fixed_vias: Vec::new(), current_layer: layer, idle: false, placement_correct: false, displaced_tracks: BTreeMap::new(), displaced_vias: BTreeMap::new(), exclude_end: Vec::new(), end_split: None, springback: RefCell::new(Springback::new()), last_world: RefCell::new(World::default()), last_good: RefCell::new(None), stages: Vec::new() }
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
    /// `routeStep`: `routeHead`, and when it fails (the walk is stuck, or in Shove mode the walk round the pads is: `rhShoveOnly`'s `if( !
    /// rhWalkBase( aP, walkSolids, ITEM::SOLID_T, RM_Shove, viaOk ) ) return false;`) the head and tail as they were (`m_tail = std::move(
    /// prevTail ); m_head = std::move( prevHead );`), or, with no head yet, a zero-length one at the start ("so that the user gets some
    /// feedback that routing is happening"). In Walk around and Shove the head is never shown colliding: it stays at the last position
    /// that worked, and a click fixes that (findings p5 F2-4, F2-5).
    fn build_head(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> HeadResult {
        match self.route_head(node, rules, settings, p) {
            Some(r) => {
                *self.last_good.borrow_mut() = Some(r.clone());
                r
            }
            None => {
                let kept = self.last_good.borrow().clone().unwrap_or_else(|| {
                    let current = if settings.mode == Mode::Shove { self.springback.borrow().top().0 } else { self.springback.borrow().locked_top() };
                    HeadResult::plain(vec![self.fixed_start()], false, node, current)
                });
                HeadResult { kept: true, ..kept }
            }
        }
    }

    /// `routeHead`: `None` when it fails.
    fn route_head(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> Option<HeadResult> {
        let start = self.fixed_start();
        // the world the last `Move` left (`m_currentNode`): in Shove mode the newest springback frame, in the others the world the last click
        // left (those modes never shove, so their `m_currentNode` is `m_lastNode` of the last `FixRoute`)
        let current = if settings.mode == Mode::Shove { self.springback.borrow().top().0 } else { self.springback.borrow().locked_top() };
        if start == p {
            return Some(HeadResult::plain(vec![start], false, node, current));
        }
        // `buildInitialLine`: `if( GetFreeAngleMode() && Mode() == RM_MarkObstacles ) l = SHAPE_LINE_CHAIN( { m_p_start, aP } )`, else the trace in the
        // corner mode (`guessedDir.BuildInitialTrace( m_p_start, aP, false, cornerMode )`): a rounded corner is an arc of the head
        let build_raw = |to: Point| {
            let mut raw = Line::new(self.net.clone(), self.current_layer, self.width);
            if settings.free_angle() {
                raw.pts = vec![start, to];
            } else {
                let trace = self.posture(settings, to).build_initial_line(start, to, false, settings.corner_mode);
                (raw.pts, raw.arcs) = (trace.pts, trace.arcs);
            }
            raw
        };
        let raw = build_raw(p);
        let exclude = self.exclude();
        let iteration_limit = settings.walkaround_iteration_limit.max(0) as u32;
        // `rhWalkBase`: with a via pending, `buildInitialLine` pushes the via off what it sits on first (`v.PushoutForce( m_currentNode, lead,
        // force, collMask, iterLimit )`, `collMask` every kind in Walk around, the pads alone in Shove's pad walk) and builds the head to the
        // pushed position (`BuildInitialTrace( m_p_start, aP + force, .. )`); the head is walked there, and a second round (`while( round < 2
        // && m_placingVia )`) pushes the via again from where the walk ended and walks to that. The via goes at the walked head's end
        // (`v.SetPos( walkFull.CLastPoint() ); walkFull.AppendVia( v )`). Without a via, one round.
        let walk_base = |world: &Node, mask: u8| -> Option<Line> {
            if !self.placing_via {
                return walkaround::walk_base_line(world, rules, &raw, mask, iteration_limit, settings.walkaround_hug_length_threshold, &exclude, settings.corner_mode);
            }
            let to_layer = self.via_to_layer.unwrap_or(self.current_layer);
            let (mut walk_p, mut walked) = (p, None);
            for _round in 0..2 {
                let target = self.push_via(world, rules, settings, walk_p, to_layer, mask);
                let l1 = build_raw(target);
                let w = walkaround::walk_base_line(world, rules, &l1, mask, iteration_limit, settings.walkaround_hug_length_threshold, &exclude, settings.corner_mode)?;
                walk_p = w.last().unwrap_or(target);
                walked = Some(w);
            }
            walked
        };
        // `!m_mouseTrailTracer.IsManuallyForced()`: the tracer's own flag, which `Clear()` resets on every `FixRoute` -- one `/` keeps Smart Pads off
        // for the leg it was pressed in, not for the rest of the route (findings p5 F2-13)
        let effort = head_effort(settings, self.trail.borrow().is_manually_forced());
        let optimize = |line: &Line, world: &Node| optimizer::optimize_constrained(line, world, rules, &exclude, effort, &optimizer::Constraints { corner_mode: settings.corner_mode, ..Default::default() });

        let mut current = current;
        if settings.mode == Mode::Shove {
            // `rhShoveOnly`: first walk the head around the pads alone
            // (`rhWalkBase( aP, walkSolids, ITEM::SOLID_T, RM_Shove )`), then
            // shove tracks and vias out of its way. A pad the head cannot be
            // walked around, a locked item, or the iteration limit falls back
            // to a full walkaround for this call, exactly like upstream.
            if let Some(walked) = walk_base(node, crate::node::kind_mask::SOLID) {
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
                    return Some(HeadResult { pts: optimized.pts, arcs: optimized.arcs, colliding: false, displaced_lines: world.displaced_lines(node), displaced_vias: world.displaced_vias(), world, via, via_shoved: true, kept: false });
                }
                // the shove failed: "m_currentNode = parent", and `rhWalkOnly` walks the head in it
                current = parent;
            } else {
                // `rhShoveOnly`: "if( ! rhWalkBase( aP, walkSolids, ITEM::SOLID_T, RM_Shove, viaOk ) ) return false;" -- no fall-back to a
                // walk round the tracks; `routeStep` keeps the head it had
                return None;
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
                Some(HeadResult::plain_line(raw, colliding, node, current))
            }
            Mode::Walkaround | Mode::Shove => {
                // `rhWalkOnly`: `rhWalkBase( aP, walkFull, ITEM::ANY_T, RM_Walkaround )`, refused if the walk still
                // collides with anything, then the head optimized per the effort setting.
                let walked = walk_base(world_node, crate::node::kind_mask::ANY).filter(|path| !walkaround::path_collides(world_node, rules, &self.net, self.current_layer, self.width, &path.pts, &exclude));
                // `rhWalkOnly` fails when the walk does (ST_STUCK) or still collides
                walked.map(|path| {
                    let optimized = optimize(&path, world_node);
                    let via = if self.placing_via { optimized.last() } else { None };
                    HeadResult { via, ..HeadResult::plain_line(optimized, false, node, current) }
                })
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

    /// The layers of the pending via: [`Self::via_span`], else from the current layer to `to_layer`.
    fn via_layers(&self, to_layer: i32) -> LayerRange {
        self.via_span.unwrap_or_else(|| LayerRange::new(self.current_layer, to_layer))
    }

    /// `makeVia( pos )`: a via of the pending size on the layers from the current one to the one the route goes on to (or the span [`Self::via_span`] names).
    fn make_via(&self, pos: Point) -> crate::item::Via {
        crate::item::Via { net: self.net.clone(), layers: self.via_layers(self.via_to_layer.unwrap_or(self.current_layer)), pos, diameter: self.via_diameter, drill: self.via_drill, source_via: None, locked: false, is_virtual: false }
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
        self.push_via(node, rules, settings, at, to_layer, mask)
    }

    /// `buildInitialLine`'s `v.PushoutForce( m_currentNode, lead, force, collMask, iterLimit )` for the pending via at `at`, the lead from where
    /// this leg started: where it comes free of the items of `mask`, or `at` when it does not.
    fn push_via(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, at: Point, to_layer: i32, mask: u8) -> Point {
        if settings.mode == Mode::MarkObstacles {
            return at;
        }
        let via = crate::item::Via { net: self.net.clone(), layers: self.via_layers(to_layer), pos: at, diameter: self.via_diameter, drill: self.via_drill, source_via: None, locked: false, is_virtual: false };
        let start = self.fixed_start();
        let lead = (at.x - start.x, at.y - start.y);
        match crate::shove::via_pushout_force(node, rules, &via, lead, mask, settings.via_force_prop_iteration_limit, &[]) {
            Some(force) => Point { x: at.x + force.0, y: at.y + force.1 },
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
        let (snapped_end, snapped_segment) = self.snap_end(node, settings, p);
        let target = snapped_end.unwrap_or(p);
        let head_result = self.build_head(node, rules, settings, target);
        *self.last_world.borrow_mut() = head_result.world.clone();
        let mut colliding = head_result.colliding;
        let (resolved_via, via_shoved) = (head_result.via, head_result.via_shoved);
        let mut head = Line::from_points(self.net.clone(), self.current_layer, self.width, head_result.pts);
        head.arcs = head_result.arcs;
        head.simplify();
        // a head kept from an earlier `Move` does not reach the end the cursor is on
        let (snapped_end, snapped_segment) = if head_result.kept && snapped_end.is_some() && head.last() != snapped_end { (None, None) } else { (snapped_end, snapped_segment) };
        let mut snapped_end = snapped_end;
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
            if via_shoved {
                // shoved together with the head: what was in its way has been moved, so the via is where the head ends and collides with nothing
                Some((head.last().unwrap_or(target), self.via_diameter, self.via_drill))
            } else {
                // pushed off before the head was walked to it (the head ends there), or placed on the head's end now
                let to_layer = self.via_to_layer.unwrap_or(self.current_layer);
                let placed = resolved_via.unwrap_or_else(|| self.place_via(node, rules, settings, head.last().unwrap_or(target), to_layer));
                let shape = Shape::Circle { c: placed, r: self.via_diameter / 2 };
                if node.first_colliding(&shape, &self.net, self.via_layers(to_layer), rules, &[]).is_some() {
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
        if let Some((_, a)) = node.nearest_anchor(p, layers, EXACT_NM, Some(&self.net)) {
            return (Some(a), None);
        }
        if settings.snap_to_tracks {
            if let Some(id) = node.segment_at(p, self.current_layer, Some(&self.net), Some(EXACT_NM), &self.exclude_end) {
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
        match node.nearest_anchor(p, layers, SNAP_NM, Some(&self.net)) {
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
        *self.last_good.borrow_mut() = None;
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
    pub fn switch_layer(&mut self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point, new_layer: i32, diameter: Nm, drill: Nm) {
        self.via_to_layer = Some(new_layer);
        self.begin_move(p);
        let head_result = self.build_head(node, rules, settings, p);
        let resolved_via = head_result.via;
        *self.last_world.borrow_mut() = head_result.world;
        let mut head = Line::from_points(self.net.clone(), self.current_layer, self.width, head_result.pts);
        head.arcs = head_result.arcs;
        head.simplify();
        let via_pos = match resolved_via {
            // pushed off, or shoved together with the head: the via is where the head ends
            Some(_) => head.last().unwrap_or(p),
            None => self.place_via(node, rules, settings, head.last().unwrap_or(p), new_layer),
        };
        let via = crate::item::Via { layers: self.via_layers(new_layer), ..self.make_via(via_pos) };
        let via = crate::item::Via { diameter, drill, ..via };
        if head.point_count() >= 2 {
            self.stages.push(self.springback.borrow().len());
            self.lock_fixed(node, Some(&head), Some(via.clone()));
            self.runs.push(head);
        } else {
            self.lock_fixed(node, None, Some(via.clone()));
        }
        self.fixed_vias.push((via, self.runs.len()));
        *self.last_good.borrow_mut() = None;
        self.current_layer = new_layer;
        self.via_diameter = diameter;
        self.via_drill = drill;
        self.origin = via_pos; // the next run (if `runs` was empty) starts here
        self.placing_via = false;
        self.via_to_layer = None;
        self.via_span = None;
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
        // the via that run ended in goes with it, and the route is back on the layer the run was on (`m_currentLayer = st.pts[0].layer`)
        let left = self.runs.len();
        let ended_in_via = self.fixed_vias.iter().rev().find(|(v, runs)| *runs > left && popped.last() == Some(v.pos)).map(|(v, _)| v.clone());
        let layer_after = self.current_layer;
        self.fixed_vias.retain(|(_, runs)| *runs <= left);
        *self.last_good.borrow_mut() = None;
        self.current_layer = popped.layer;
        // `m_placingVia = st.pts[0].placingVia` (what `FixRoute`'s `AddStage` recorded): a run that ended in a via is taken back with the via
        // armed again, to the layer it went on to
        match ended_in_via {
            Some(v) => {
                self.placing_via = true;
                self.via_to_layer = Some(layer_after);
                self.via_span = Some(v.layers);
                self.via_diameter = v.diameter;
                self.via_drill = v.drill;
            }
            None => {
                self.placing_via = false;
                self.via_to_layer = None;
                self.via_span = None;
            }
        }
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

    pub fn toggle_via(&mut self, enabled: bool, diameter: Nm, drill: Nm) {
        self.placing_via = enabled;
        self.via_diameter = diameter;
        self.via_drill = drill;
        if !enabled {
            self.via_to_layer = None;
            self.via_span = None;
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
        // `FixRoute`: a head that ends in a via commits the via, with segments before it or none ("Nothing to commit if we have an empty
        // line" -- unless `pl.EndsWithVia()`)
        if let (true, Some((at, diameter, drill))) = (self.placing_via, preview.via) {
            let via = crate::item::Via { layers: self.via_layers(self.via_to_layer.unwrap_or(self.current_layer)), diameter, drill, ..self.make_via(at) };
            self.fixed_vias.push((via, self.runs.len()));
            self.placing_via = false;
        }
        self.idle = true;
        self.placement_correct = !self.runs.is_empty();
        if settings.remove_loops && real_end {
            self.remove_loops(node);
        }
        Some(self.placed_runs())
    }

    /// `LINE_PLACER::CommitPlacement` without a last `FixRoute` (Esc, or a finish that was refused: `router_tool.cpp::performRouting` ends
    /// every way out of its loop with `m_router->CommitRouting()`): what the clicks fixed -- the runs, the vias, and the shoves the clicks
    /// locked (`m_shove->RewindToLastLockedNode(); m_lastNode = m_shove->CurrentNode()`), not the head's. Ends the placement.
    pub fn commit_fixed(&mut self) -> Vec<Line> {
        let world = self.springback.borrow().locked_top();
        self.take_displacement(&world);
        *self.last_world.borrow_mut() = world;
        self.idle = true;
        self.placing_via = false;
        self.placed_runs()
    }

    /// `LINE_PLACER::removeLoops( m_lastNode, current )`, which `Move` runs once the head reaches a same-net end item (here: when the route is
    /// finished on one): the route is added to the world (`aNode->Add( aLatest, true )`), and for each of its segments the line it is part of
    /// there -- through the tracks it continues, `ourLine = aNode->AssembleLine( seg )` -- gives two joints (`FindLineEnds`; the route's own
    /// two ends if that line closes on itself); every other line of the net between those two joints (`FindLinesBetweenJoints`) that is not
    /// that line is redundant, and its tracks are removed (`toErase`). (findings p5 F2-7: only the route's own two end points were tried.)
    /// **Differs:** the IR keeps a track as a polyline, so a track is removed only when every segment of it is redundant; and a line with a
    /// locked segment stays ("don't remove locked tracks", as before).
    fn remove_loops(&mut self, node: &Node) {
        let runs = self.placed_runs();
        let (Some(route_start), Some(route_end)) = (runs.first().and_then(|r| r.first()), runs.last().and_then(|r| r.last())) else { return };
        if route_start == route_end {
            return;
        }
        // the world the last `Move` left (what the shoves moved), with the route in it
        let moved = World { fixed: Vec::new(), fixed_vias: Vec::new(), ..self.last_world.borrow().clone() };
        let mut w = moved.apply(node);
        let mut ours: Vec<ItemId> = Vec::new();
        for run in runs.iter().filter(|r| r.point_count() >= 2) {
            ours.extend(w.add_line(run, None, false));
        }
        for (v, _) in &self.fixed_vias {
            w.add(Item::Via(v.clone()));
        }
        let mut erase: std::collections::BTreeSet<ItemId> = std::collections::BTreeSet::new();
        for &seg in &ours {
            let Some(our_line) = w.assemble_line(seg) else { continue };
            let (mut a, mut b) = (our_line.first(), our_line.last());
            if a == b {
                (a, b) = (Some(route_start), Some(route_end));
            }
            let (Some(a), Some(b)) = (a, b) else { continue };
            for line in w.find_lines_between_joints(a, b, &self.net) {
                if line.segment_ids.contains(&seg) || line.segment_count() == 0 {
                    continue;
                }
                if line.segment_ids.iter().any(|id| w.get(*id).is_some_and(|it| it.is_track() && it.is_locked())) {
                    continue;
                }
                erase.extend(line.segment_ids.iter().copied());
            }
        }
        // the IR tracks all of whose segments are redundant
        let mut by_track: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (id, it) in w.iter() {
            if let Some(t) = it.source_track_id() {
                let e = by_track.entry(t.to_string()).or_insert((0, 0));
                e.0 += 1;
                if erase.contains(&id) {
                    e.1 += 1;
                }
            }
        }
        for (track_id, (total, erased)) in by_track {
            if erased > 0 && erased == total {
                // An empty replacement (`point_count() < 2`) tells the eventual caller (`crate::router::Router::build_commit`) "remove this
                // track, nothing replaces it" -- the same convention a fully-retracted shove already relies on.
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
    use eda_model::units::mm;
    use super::*;
    use crate::item::{net_of, Solid};
    use crate::node::Node;

    fn rules() -> BoardRules {
        serde_yaml::from_str("track_width: 200000\nclearance: 200000\nvia_drill: 300000\nvia_diameter: 600000\n").unwrap()
    }

    #[test]
    fn straight_route_commits_on_fix_and_finish() {
        let node = Node::new();
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        let pv = placer.preview(&node, &rules, &settings, Point { x: mm(1.0), y: Nm::ZERO });
        assert!(!pv.colliding);
        assert_eq!(pv.head.last(), Some(Point { x: mm(1.0), y: Nm::ZERO }));
        assert_eq!(placer.fix(&node, &rules, &settings, Point { x: mm(1.0), y: Nm::ZERO }), FixOutcome::Fixed { real_end: false });
        assert_eq!(placer.runs.len(), 1);
        let finished = placer.finish(&node, &rules, &settings, Point { x: mm(2.0), y: Nm::ZERO }).expect("collision-free finish must succeed");
        assert!(placer.idle);
        assert!(!finished.is_empty());
        let total_len: f64 = finished.iter().map(|l| l.length()).sum();
        assert!(total_len > 0.0);
    }

    #[test]
    fn walks_around_an_obstacle_between_fixed_points() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(2.5), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(2.5), y: Nm::ZERO }, r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        let placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        let pv = placer.preview(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
        assert!(!pv.colliding, "walkaround must clear the obstacle");
        assert!(pv.head.point_count() > 2, "must actually detour, not run straight through the pad");
    }

    /// D8: MarkObstacles refuses a colliding commit unless `can_violate_drc`.
    #[test]
    fn mark_obstacles_commits_collision_only_when_can_violate_drc() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(1.0), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(1.0), y: Nm::ZERO }, r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let mut settings = RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() };
        assert!(!settings.can_violate_drc);
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        assert_eq!(placer.fix(&node, &rules, &settings, Point { x: mm(2.0), y: Nm::ZERO }), FixOutcome::Blocked);
        assert!(placer.finish(&node, &rules, &settings, Point { x: mm(2.0), y: Nm::ZERO }).is_none());
        settings.can_violate_drc = true;
        assert!(matches!(placer.fix(&node, &rules, &settings, Point { x: mm(2.0), y: Nm::ZERO }), FixOutcome::Fixed { .. }));
    }

    /// `GetFixAllSegments()` off: a click fixes every segment but the last, which stays free (`FixRoute`'s `lastV`).
    #[test]
    fn without_fix_all_segments_a_click_leaves_the_last_segment_free() {
        let node = Node::new();
        let rules = rules();
        let target = Point { x: mm(3.0), y: mm(1.0) };
        let mut all = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        let head = placer.preview(&node, &rules, &all, target).head.pts;
        assert_eq!(head.len(), 3, "an elbow of two legs: {head:?}");
        assert_eq!(placer.fix(&node, &rules, &all, target), FixOutcome::Fixed { real_end: false });
        assert_eq!(placer.runs[0].pts, head, "by default the whole head is fixed");
        assert_eq!(placer.fixed_start(), target);

        all.fix_all_segments = false;
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        assert_eq!(placer.fix(&node, &rules, &all, target), FixOutcome::Fixed { real_end: false });
        assert_eq!(placer.runs[0].pts, head[..2].to_vec(), "only the first leg is fixed");
        assert_eq!(placer.fixed_start(), head[1], "and the next one starts where the free one did");
        assert_eq!(placer.direction, Direction45::from_seg(head[0], head[1]), "continuing in the direction of the leg before it");
        // A click that ends the route fixes the lot, whatever the setting says.
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        let finished = placer.finish(&node, &rules, &all, target).unwrap();
        assert_eq!(finished[0].pts, head);
    }

    /// `WalkaroundHugLengthThreshold()`: a head whose walkaround is more than twice the threshold times the direct length
    /// hugs the obstacle instead of chasing the cursor round it.
    #[test]
    fn a_long_detour_hugs_the_obstacle_unless_the_hug_threshold_allows_it() {
        let mut node = Node::new();
        // a 20 mm wall of GND between the start and the cursor, 4 mm apart: the way round is 5 times as long
        node.add(Item::Segment(crate::item::Segment { net: net_of("GND"), layer: 0, a: Point { x: mm(2.0), y: mm(-10.0) }, b: Point { x: mm(2.0), y: mm(10.0) }, width: mm(0.2), source_track: None, locked: false }));
        let rules = rules();
        let cursor = Point { x: mm(4.0), y: Nm::ZERO };
        let placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));

        let hugging = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        let pv = placer.preview(&node, &rules, &hugging, cursor);
        assert!(!pv.colliding);
        let end = pv.head.last().unwrap();
        assert_ne!(end, cursor, "the head does not go the 20 mm round to the cursor: {:?}", pv.head.pts);
        assert!(end.x < mm(2.0) && pv.head.length() < 6_000_000.0, "it stops at the wall's clearance, on the near side: {:?}", pv.head.pts);

        let patient = RoutingSettings { walkaround_hug_length_threshold: 3.0, ..hugging.clone() };
        let pv = placer.preview(&node, &rules, &patient, cursor);
        assert!(!pv.colliding);
        assert_eq!(pv.head.last(), Some(cursor), "with a threshold of 3 the 21 mm detour is taken: {:?}", pv.head.pts);
        assert!(pv.head.length() > 20_000_000.0);
    }

    /// `ViaForcePropIterationLimit()`: how many steps `VIA::PushoutForce` has to get the via off a pad.
    #[test]
    fn a_via_placed_on_a_pad_is_pushed_off_within_the_via_force_iteration_limit() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: Nm::ZERO, y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: Nm::ZERO, y: Nm::ZERO }, r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let mut placer = LinePlacer::start(&node, Point { x: mm(3.0), y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.toggle_via(true, mm(0.6), mm(0.3));
        let on_the_pad = Point { x: mm(0.2), y: mm(0.3) };
        let settings = RoutingSettings::default();
        let placed = placer.place_via(&node, &rules, &settings, on_the_pad, 1);
        let centre_distance = ((placed.x.as_f64()).powi(2) + (placed.y.as_f64()).powi(2)).sqrt();
        assert!(centre_distance >= 1_000_000.0 - 500.0, "the 600 um via is {centre_distance} um from the 500 um pad's centre: {placed:?}");
        // one step of the whole MTV leaves the pad (KiCad's quarter-diameter limit is a no-op, findings p5 F1-3), and the next
        // iteration finds it free -- two iterations; with one, the loop ends at the limit and the push counts as failed
        let two = RoutingSettings { via_force_prop_iteration_limit: 2, ..settings.clone() };
        let placed = placer.place_via(&node, &rules, &two, on_the_pad, 1);
        assert!(((placed.x.as_f64()).powi(2) + (placed.y.as_f64()).powi(2)).sqrt() >= 1_000_000.0 - 500.0, "{placed:?}");
        let tight = RoutingSettings { via_force_prop_iteration_limit: 1, ..settings.clone() };
        assert_eq!(placer.place_via(&node, &rules, &tight, on_the_pad, 1), on_the_pad, "out of steps: the via stays where the cursor put it");
        // Highlight collisions never moves a via.
        let marking = RoutingSettings { mode: Mode::MarkObstacles, ..settings };
        assert_eq!(placer.place_via(&node, &rules, &marking, on_the_pad, 1), on_the_pad);
    }

    /// findings p5 F2-4 / F2-5: a `Move` whose `routeHead` fails keeps the head of the last one that routed (`routeStep`: `m_head = std::move(
    /// prevHead )`), not colliding, and a click fixes that head; with no head routed yet it is a zero-length one, which a click cannot fix.
    /// A walk fails when the start itself collides (every clipped prefix does); here the world grows a pad over the start between two moves.
    #[test]
    fn a_failed_route_head_keeps_the_last_good_head() {
        let free = Node::new();
        let mut blocked = Node::new();
        blocked.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(0.3), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(0.3), y: Nm::ZERO }, r: mm(0.1) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        for mode in [Mode::Walkaround, Mode::Shove] {
            let settings = RoutingSettings { mode, ..RoutingSettings::default() };
            // nothing routed yet: the zero-length head, where the old port showed the raw line in violation (and in Shove mode walked round the tracks)
            let mut placer = LinePlacer::start(&blocked, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
            let pv = placer.preview(&blocked, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
            assert!(!pv.colliding && pv.head.point_count() < 2, "{mode:?}: {pv:?}");
            assert_eq!(placer.fix(&blocked, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO }), FixOutcome::Blocked);
            // a head that routed is kept
            let mut placer = LinePlacer::start(&free, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
            let a = Point { x: mm(2.0), y: mm(1.0) };
            assert_eq!(placer.preview(&free, &rules, &settings, a).head.last(), Some(a));
            let pv = placer.preview(&blocked, &rules, &settings, Point { x: mm(5.0), y: mm(-3.0) });
            assert!(!pv.colliding && pv.head.last() == Some(a), "{mode:?}: the head stays where it last worked: {pv:?}");
            assert_eq!(placer.fix(&blocked, &rules, &settings, Point { x: mm(5.0), y: mm(-3.0) }), FixOutcome::Fixed { real_end: false });
            assert_eq!(placer.runs.last().and_then(|r| r.last()), Some(a), "{mode:?}: the click fixes the head it shows");
        }
    }

    #[test]
    fn undo_last_segment_restores_previous_state() {
        let node = Node::new();
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.fix(&node, &rules, &settings, Point { x: mm(1.0), y: Nm::ZERO });
        placer.fix(&node, &rules, &settings, Point { x: mm(1.0), y: mm(1.0) }); // (0,0) stayed fixed-start for this leg; the first fix moved it to (1000,0)
        assert_eq!(placer.runs.len(), 2);
        assert!(placer.undo_last_segment());
        assert_eq!(placer.runs.len(), 1);
        assert_eq!(placer.fixed_start(), Point { x: mm(1.0), y: Nm::ZERO });
        assert!(placer.undo_last_segment());
        assert_eq!(placer.runs.len(), 0);
        assert!(!placer.undo_last_segment(), "nothing left to undo");
    }

    #[test]
    fn snaps_onto_a_same_net_pad_and_reports_real_end() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: mm(3.0), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(3.0), y: Nm::ZERO }, r: mm(0.4) }, source: "U2.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        let outcome = placer.fix(&node, &rules, &settings, Point { x: mm(3.01), y: mm(0.01) });
        assert_eq!(outcome, FixOutcome::Fixed { real_end: true }, "a click near a same-net pad must finish the route");
        assert_eq!(placer.runs[0].last(), Some(Point { x: mm(3.0), y: Nm::ZERO }), "the fixed point must snap exactly onto the pad centre");
    }

    #[test]
    fn via_and_layer_switch_starts_a_new_run_on_the_new_layer() {
        let node = Node::new();
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.toggle_via(true, mm(0.6), mm(0.3));
        placer.switch_layer(&node, &rules, &settings, Point { x: mm(1.0), y: Nm::ZERO }, 1, mm(0.6), mm(0.3));
        assert_eq!(placer.current_layer, 1);
        assert_eq!(placer.runs.len(), 1);
        assert_eq!(placer.runs[0].layer, 0);
        assert_eq!(placer.origin, Point { x: mm(1.0), y: Nm::ZERO });
        let pv = placer.preview(&node, &rules, &settings, Point { x: mm(2.0), y: Nm::ZERO });
        assert_eq!(pv.head.layer, 1);
    }

    #[test]
    fn shove_mode_pushes_a_crossing_track_and_records_it_for_commit() {
        use crate::item::Segment;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: mm(2.5), y: mm(-2.0) }, b: Point { x: mm(2.5), y: mm(2.0) }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));

        let pv = placer.preview(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
        assert!(!pv.colliding, "shove must resolve the crossing, not just report it");
        assert_eq!(pv.head.pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(5.0), y: Nm::ZERO }], "shove moves the obstacle, not the pusher");
        assert_eq!(pv.displaced_lines.len(), 1);
        assert!(placer.displaced_tracks().next().is_none(), "preview alone must not persist anything");

        let outcome = placer.finish(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO }).expect("collision-free shoved finish must succeed");
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(2.0), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(2.0), y: Nm::ZERO }, r: mm(0.4) }, source: "U1.1".into(), edge: false, rule: 0 }));
        node.add(Item::Segment(Segment { net: net_of("PWR"), layer: 0, a: Point { x: mm(4.3), y: mm(-2.5) }, b: Point { x: mm(4.3), y: mm(-0.3) }, width: mm(0.2), source_track: Some(("trkP".into(), 0)), locked: false }));
        let rules = rules();
        let placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        let shove = placer.preview(&node, &rules, &RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() }, Point { x: mm(8.0), y: Nm::ZERO });
        assert!(!shove.colliding);
        assert!(shove.head.point_count() > 2, "the head had to go around the pad: {:?}", shove.head.pts);
        assert!(shove.head.pts.iter().any(|p| p.y.abs() > mm(0.5)), "{:?}", shove.head.pts);
        assert_eq!(shove.head.last(), Some(Point { x: mm(8.0), y: Nm::ZERO }));
        // contrast: Walkaround routes around both and moves neither
        let walk = placer.preview(&node, &rules, &RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() }, Point { x: mm(8.0), y: Nm::ZERO });
        assert!(walk.displaced_lines.is_empty());
        assert!(walk.head.length() >= shove.head.length() - 1000.0, "pushing the track needs no more detour than walking around it");
    }

    /// `rhShoveOnly`: `newHead.AppendVia( makeVia( newHead.CLastPoint() ) )` before `m_shove->AddHeads( newHead, SHP_SHOVE )` -- a head that ends in a
    /// via is shoved together with it, so what the via sits on (on every layer it spans) is pushed aside as what the track meets is. The via used to be
    /// placed on the finished head and only checked, so a track under it was reported as a collision the shove would not resolve.
    #[test]
    fn a_head_that_ends_in_a_via_shoves_what_the_via_meets_on_the_other_layer() {
        use crate::item::Segment;
        let mut node = Node::new();
        // GND on the other layer, 50 um inside the via's clearance ring and clear of the head's own line
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 1, a: Point { x: mm(4.45), y: mm(-3.0) }, b: Point { x: mm(4.45), y: mm(3.0) }, width: mm(0.2), source_track: Some(("trkG".into(), 0)), locked: false }));
        let rules = rules();
        let shove = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.toggle_via(true, mm(0.6), mm(0.3));
        placer.via_to_layer = Some(1);
        let pv = placer.preview(&node, &rules, &shove, Point { x: mm(4.0), y: Nm::ZERO });
        assert!(!pv.colliding, "{pv:?}");
        assert_eq!(pv.head.pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(4.0), y: Nm::ZERO }]);
        assert_eq!(pv.via, Some((Point { x: mm(4.0), y: Nm::ZERO }, mm(0.6), mm(0.3))));
        assert_eq!(pv.displaced_lines.len(), 1, "the track under the via is pushed aside: {:?}", pv.displaced_lines);
        // the track bulges round the via: abreast of it, it is the via's edge (4300) + the clearance (200) + the track's half width (100) away
        let bulge: Vec<_> = pv.displaced_lines[0].line.pts.iter().filter(|q| q.y.abs() <= mm(0.3)).collect();
        assert!(!bulge.is_empty() && bulge.iter().all(|q| q.x >= mm(4.6) - Nm::new(1)), "{:?}", pv.displaced_lines[0].line.pts);
        // Highlight collisions only reports it: the same via collides with the track on the other layer
        let mark = RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() };
        let pv = placer.preview(&node, &rules, &mark, Point { x: mm(4.0), y: Nm::ZERO });
        assert!(pv.colliding && pv.displaced_lines.is_empty());
        // and Walk around pushes the via off the track rather than the track off the via
        let walk = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        let pv = placer.preview(&node, &rules, &walk, Point { x: mm(4.0), y: Nm::ZERO });
        assert!(!pv.colliding && pv.displaced_lines.is_empty() && pv.via.is_some_and(|(at, _, _)| at != Point { x: mm(4.0), y: Nm::ZERO }), "{pv:?}");
        // and a shove that cannot move the track (locked) falls back to the walk (`rhShoveOnly` -> `rhWalkOnly`), which pushes the via off the
        // track as Walk around does (`buildInitialLine( .., RM_Walkaround )`: `collMask = ITEM::ANY_T`); the head ends at the pushed via
        let mut locked = Node::new();
        locked.add(Item::Segment(Segment { net: net_of("GND"), layer: 1, a: Point { x: mm(4.45), y: mm(-3.0) }, b: Point { x: mm(4.45), y: mm(3.0) }, width: mm(0.2), source_track: Some(("trkG".into(), 0)), locked: true }));
        // (a new route: a placer's springback frames are of the world it routes in)
        let mut placer = LinePlacer::start(&locked, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.toggle_via(true, mm(0.6), mm(0.3));
        placer.via_to_layer = Some(1);
        let pv = placer.preview(&locked, &rules, &shove, Point { x: mm(4.0), y: Nm::ZERO });
        assert!(pv.displaced_lines.is_empty() && !pv.colliding, "a locked track is not shoved, the via is pushed off it: {pv:?}");
        let (at, _, _) = pv.via.unwrap();
        assert!(at.x <= mm(4.45) - mm(0.1) - mm(0.2) - mm(0.3) && pv.head.last() == Some(at), "{pv:?}");
    }

    /// The click that places the via (`switch_layer`) takes the shoved track along into the commit, and the next run starts on the via.
    #[test]
    fn placing_the_via_of_a_shoved_head_records_what_it_pushed() {
        use crate::item::Segment;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 1, a: Point { x: mm(4.45), y: mm(-3.0) }, b: Point { x: mm(4.45), y: mm(3.0) }, width: mm(0.2), source_track: Some(("trkG".into(), 0)), locked: false }));
        let rules = rules();
        let shove = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.toggle_via(true, mm(0.6), mm(0.3));
        placer.switch_layer(&node, &rules, &shove, Point { x: mm(4.0), y: Nm::ZERO }, 1, mm(0.6), mm(0.3));
        assert_eq!(placer.current_layer, 1);
        assert_eq!(placer.runs.len(), 1);
        assert_eq!(placer.runs[0].pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(4.0), y: Nm::ZERO }]);
        assert_eq!(placer.origin, Point { x: mm(4.0), y: Nm::ZERO });
        let displaced: Vec<_> = placer.displaced_tracks().collect();
        assert_eq!(displaced.len(), 1);
        assert_eq!(displaced[0].0, "trkG");
        assert!(displaced[0].1.iter().all(|l| l.pts.iter().filter(|q| q.y.abs() <= mm(0.3)).all(|q| q.x >= mm(4.6) - Nm::new(1))), "{displaced:?}");
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
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: mm(2.5), y: mm(-2.0) }, b: Point { x: mm(2.5), y: mm(2.0) }, width: mm(0.2), source_track: Some(("trkA".into(), 0)), locked: false }));
        node
    }

    /// `SHOVE::reduceSpringback` / `pushSpringback`: a track the head pushed stays pushed as long as the head presses on it, is pushed on from
    /// where it was left as the head goes further, and springs back to where it was once the head has drawn back off it.
    #[test]
    fn a_pushed_track_is_pushed_on_from_where_it_was_left_and_springs_back_when_the_head_draws_back() {
        let (node, rules) = (crossing_track(), rules());
        let settings = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        // the first move (KiCad's first frame: the head a few pixels long) touches nothing
        assert!(placer.preview(&node, &rules, &settings, Point { x: mm(0.5), y: Nm::ZERO }).displaced_lines.is_empty());
        let reach = |pv: &Preview| pv.displaced_lines.iter().flat_map(|d| d.line.pts.iter()).map(|q| q.x).max().unwrap_or(mm(2.5));
        let pushed = placer.preview(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
        assert_eq!(pushed.displaced_lines.len(), 1);
        assert!(reach(&pushed) >= mm(5.0) + mm(0.1) + mm(0.2) + mm(0.1) - Nm::new(1), "{:?}", pushed.displaced_lines);
        let further = placer.preview(&node, &rules, &settings, Point { x: mm(6.0), y: Nm::ZERO });
        assert!(reach(&further) >= mm(6.0) + mm(0.4) - Nm::new(1), "{:?}", further.displaced_lines);
        assert_eq!(placer.springback.borrow().len(), 3, "a frame per move that shoved");
        // drawn back off the track: every frame it no longer presses on is popped, down to the first
        let back = placer.preview(&node, &rules, &settings, Point { x: mm(1.5), y: Nm::ZERO });
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
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.preview(&node, &rules, &settings, Point { x: mm(0.5), y: Nm::ZERO });
        placer.preview(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
        assert_eq!(placer.fix(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO }), FixOutcome::Fixed { real_end: false });
        assert_eq!(placer.displaced_tracks().count(), 1, "the click keeps the push");
        // a leg going north from the click runs into the track where the first leg left it (bulged round x = 5000)
        let mut path = vec![Point { x: mm(5.0), y: mm(-0.5) }, Point { x: mm(5.0), y: mm(-1.5) }, Point { x: mm(5.0), y: mm(-3.0) }];
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
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.preview(&node, &rules, &settings, Point { x: mm(0.5), y: Nm::ZERO });
        placer.fix(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
        assert_eq!(placer.displaced_tracks().count(), 1);
        assert!(placer.undo_last_segment());
        assert_eq!(placer.displaced_tracks().count(), 0, "the leg and its push are gone");
        assert!(placer.preview(&node, &rules, &settings, Point { x: mm(1.5), y: Nm::ZERO }).displaced_lines.is_empty());
    }

    /// `UnfixRoute`: `RewindSpringbackTo( st.commit )`. When no shove of the undone leg succeeded, its stage's `commit` is the locked frame the
    /// click before left (`m_currentNode` was the parent `Run` fell back to), and KiCad erases that frame too: the earlier leg is gone from the
    /// world while it is still fixed, and `CommitPlacement` (`RewindToLastLockedNode`) then commits a frame without it. Here the earlier leg's
    /// frame stays.
    #[test]
    fn backspace_after_a_leg_whose_shoves_all_failed_keeps_the_leg_before() {
        let (node, rules) = (Node::new(), rules());
        let settings = RoutingSettings { mode: Mode::Shove, fix_all_segments: true, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.preview(&node, &rules, &settings, Point { x: mm(0.5), y: Nm::ZERO });
        placer.preview(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO });
        assert_eq!(placer.fix(&node, &rules, &settings, Point { x: mm(5.0), y: Nm::ZERO }), FixOutcome::Fixed { real_end: false });
        // the second leg: every shove fails (the iteration limit), so the head is walked in the first click's world
        let failing = RoutingSettings { shove_iteration_limit: 1, ..settings.clone() };
        let len = placer.springback.borrow().len();
        placer.preview(&node, &rules, &failing, Point { x: mm(5.0), y: mm(-1.0) });
        assert_eq!(placer.springback.borrow().len(), len, "no shove succeeded, no frame was pushed");
        assert_eq!(placer.fix(&node, &rules, &failing, Point { x: mm(5.0), y: mm(-1.0) }), FixOutcome::Fixed { real_end: false });
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
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: mm(3.0), y: mm(-0.7) }, b: Point { x: mm(7.0), y: mm(-0.7) }, width: mm(0.2), source_track: Some(("B".into(), 0)), locked: false }));
        let settings = RoutingSettings { mode: Mode::Shove, fix_all_segments: true, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        let at = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        for (moves, fix) in [(vec![at(0.5, 0.0), at(5.0, 0.0)], at(10.0, 0.0)), (vec![at(10.0, -1.0)], at(10.0, -3.0)), (vec![at(9.0, -3.0)], at(5.0, -3.0))] {
            for m in moves {
                placer.preview(&node, &rules, &settings, m);
            }
            assert_eq!(placer.fix(&node, &rules, &settings, fix), FixOutcome::Fixed { real_end: false });
        }
        let mut last = None;
        for y in [-2.5, -2.0, -1.5, -1.2, -1.0, -0.9, -0.8, -0.7, -0.65] {
            last = Some(placer.preview(&node, &rules, &settings, at(5.0, y)));
        }
        let pv = last.unwrap();
        assert!(!pv.colliding);
        assert_eq!(pv.head.last(), Some(Point { x: mm(5.0), y: mm(-0.65) }), "the head reaches the cursor: {:?}", pv.head.pts);
        assert_eq!(pv.displaced_lines.len(), 1, "GND's track, not the route's own run: {:?}", pv.displaced_lines);
        let first = &pv.runs[0];
        assert_eq!((first.first(), first.last()), (Some(Point { x: Nm::ZERO, y: Nm::ZERO }), Some(Point { x: mm(10.0), y: Nm::ZERO })), "its ends stay: {:?}", first.pts);
        assert!(first.pts.iter().any(|q| q.y > Nm::ZERO), "the first leg bulges away from the push: {:?}", first.pts);
        assert_eq!(pv.runs.len(), 3);
        placer.finish(&node, &rules, &settings, Point { x: mm(5.0), y: mm(-0.65) }).expect("finishes");
        assert!(placer.placed_runs()[0].pts.iter().any(|q| q.y > Nm::ZERO), "and the commit writes it pushed");
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
        let to = Point { x: mm(6.0), y: mm(2.0) };
        for mode in [Mode::MarkObstacles, Mode::Walkaround, Mode::Shove] {
            for corner in [CornerMode::Mitered45, CornerMode::Rounded45, CornerMode::Mitered90, CornerMode::Rounded90] {
                let settings = RoutingSettings { mode, corner_mode: corner, auto_posture: false, ..RoutingSettings::default() };
                let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
                placer.direction = Direction45::E;
                placer.restart_trail(Point { x: Nm::ZERO, y: Nm::ZERO }, Direction45::E);
                let pv = placer.preview(&node, &rules, &settings, to);
                assert!(!pv.colliding);
                assert_eq!((pv.head.first(), pv.head.last()), (Some(Point { x: Nm::ZERO, y: Nm::ZERO }), Some(to)), "{mode:?} {corner:?}");
                let arcs = pv.head.arc_runs();
                match corner {
                    CornerMode::Mitered45 => assert_eq!(pv.head.pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(4.0), y: Nm::ZERO }, to], "{mode:?}"),
                    CornerMode::Mitered90 => assert_eq!(pv.head.pts, vec![Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(6.0), y: Nm::ZERO }, to], "{mode:?}"),
                    CornerMode::Rounded45 | CornerMode::Rounded90 => {
                        assert_eq!(arcs.len(), 1, "{mode:?} {corner:?}: {:?}", pv.head);
                        let arc = pv.head.arcs[arcs[0].arc];
                        let sweep = crate::arcs::ArcPts::central_angle(&arc).abs();
                        let want = if corner == CornerMode::Rounded45 { 45.0 } else { 90.0 };
                        assert!((sweep - want).abs() < 0.5, "{mode:?} {corner:?}: a {sweep} degree arc");
                        // it leaves the straight leg along it (tangent) and ends at the cursor
                        assert_eq!(pv.head.pts[0].y, Nm::ZERO);
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
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 0, a: Point { x: mm(4.5), y: mm(-2.0) }, b: Point { x: mm(4.5), y: mm(4.0) }, width: mm(0.2), source_track: Some(("g".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { mode: Mode::Shove, corner_mode: CornerMode::Rounded45, auto_posture: false, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.direction = Direction45::E;
        placer.restart_trail(Point { x: Nm::ZERO, y: Nm::ZERO }, Direction45::E);
        let pv = placer.preview(&node, &rules, &settings, Point { x: mm(6.0), y: mm(2.0) });
        assert!(!pv.colliding);
        assert_eq!(pv.head.arc_count(), 1, "{:?}", pv.head);
        assert_eq!(pv.displaced_lines.len(), 1, "the track is pushed");
        placer.finish(&node, &rules, &settings, Point { x: mm(6.0), y: mm(2.0) }).expect("finishes");
        let board = committed_world(&node, &placer);
        assert!(violations(&board, &rules).is_empty(), "{:?}", violations(&board, &rules));
    }

    /// `buildInitialLine`: a via that lands on a pad is pushed off it before the head is built, and the head ends at the via.
    #[test]
    fn a_head_via_on_a_pad_is_pushed_off_it_and_the_head_ends_at_the_via() {
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(4.0), y: mm(0.1) }, shape: Shape::Circle { c: Point { x: mm(4.0), y: mm(0.1) }, r: mm(0.4) }, source: "U1.1".into(), edge: false, rule: 0 }));
        let rules = rules();
        let shove = RoutingSettings { mode: Mode::Shove, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.toggle_via(true, mm(0.6), mm(0.3));
        let pv = placer.preview(&node, &rules, &shove, Point { x: mm(4.0), y: Nm::ZERO });
        assert!(!pv.colliding, "{pv:?}");
        let (at, d, _) = pv.via.expect("a via");
        assert_eq!(pv.head.last(), Some(at), "the head ends at the via");
        let gap = (((at.x - mm(4.0)).as_f64()).powi(2) + ((at.y - mm(0.1)).as_f64()).powi(2)).sqrt() - 400_000.0 - d.as_f64() / 2.0;
        assert!(gap >= 200_000.0 - 500.0, "the via clears the pad by the clearance: {gap} um at {at:?}");
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
            let placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
            let mut head = Vec::new();
            for i in 1..=12 {
                head = placer.preview(&node, &rules, settings, if up_first { Point { x: mm(0.5) * i, y: mm(-0.5) * i } } else { Point { x: mm(0.5) * i, y: Nm::ZERO } }).head.pts;
            }
            for i in 1..=12 {
                head = placer.preview(&node, &rules, settings, if up_first { Point { x: mm(6.0) + mm(0.5) * i, y: mm(-6.0) } } else { Point { x: mm(6.0) + mm(0.5) * i, y: mm(-0.5) * i } }).head.pts;
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

    /// findings p5 F2-12: a route from the end of a diagonal track starts straight (`InitialDirection()` is `N`), not along the track.
    #[test]
    fn a_route_from_a_diagonal_track_end_starts_straight() {
        use crate::item::Segment;
        let mut node = Node::new();
        let seg = node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(1.0), y: mm(1.0) }, width: mm(0.2), source_track: None, locked: false }));
        let settings = RoutingSettings { auto_posture: false, ..RoutingSettings::default() };
        let placer = LinePlacer::start(&node, Point { x: mm(1.0), y: mm(1.0) }, Some(seg), net_of("SIG"), 0, mm(0.2));
        let pts = placer.preview(&node, &rules(), &settings, Point { x: mm(3.0), y: mm(2.0) }).head.pts;
        assert!(!Direction45::from_seg(pts[0], pts[1]).is_diagonal(), "straight first: {pts:?}");
    }

    /// findings p5 F2-13: Smart Pads are off for the leg a `/` was pressed in (`IsManuallyForced()`), and on again after the click that fixes it
    /// (`FixRoute`: `m_mouseTrailTracer.Clear()`).
    #[test]
    fn a_flip_turns_smart_pads_off_for_its_leg_only() {
        let (node, rules) = (Node::new(), rules());
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        placer.preview(&node, &rules, &settings, Point { x: mm(3.0), y: Nm::ZERO });
        placer.flip_posture();
        assert!(placer.trail.borrow().is_manually_forced());
        assert_eq!(head_effort(&settings, placer.trail.borrow().is_manually_forced()) & optimizer::effort::SMART_PADS, 0);
        placer.fix(&node, &rules, &settings, Point { x: mm(3.0), y: Nm::ZERO });
        assert!(!placer.trail.borrow().is_manually_forced(), "the fix starts a new trail");
        assert_ne!(head_effort(&settings, placer.trail.borrow().is_manually_forced()) & optimizer::effort::SMART_PADS, 0);
    }

    /// `FlipPosture` (`/`): the posture flips from the direction of the head shown and stays flipped whatever the mouse does; a fix starts a new trail that
    /// continues in the direction of the last segment (`FixRoute`).
    #[test]
    fn a_flip_stays_and_a_fix_continues_in_the_direction_of_the_last_segment() {
        let (node, rules) = (Node::new(), rules());
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        let shown = placer.preview(&node, &rules, &settings, Point { x: mm(6.0), y: mm(-2.0) }).head.pts;
        assert_eq!(Direction45::from_seg(shown[0], shown[1]), Direction45::E, "straight first at the start");
        placer.flip_posture();
        assert!(placer.manually_forced);
        let flipped = placer.preview(&node, &rules, &settings, Point { x: mm(6.0), y: mm(-2.0) }).head.pts;
        assert!(Direction45::from_seg(flipped[0], flipped[1]).is_diagonal(), "E flipped is the next octant: diagonal first, {flipped:?}");
        // however the mouse moves, the flip holds
        for i in 1..=10 {
            let moved = placer.preview(&node, &rules, &settings, Point { x: mm(6.0) + mm(0.5) * i, y: mm(-2.0) - mm(0.7) * i }).head.pts;
            assert!(Direction45::from_seg(moved[0], moved[1]).is_diagonal(), "{moved:?}");
        }
        // a fix: the next leg starts in the direction the last one ended in, and the flip is forgotten (`m_mouseTrailTracer.Clear()`)
        let at = Point { x: mm(6.0), y: mm(-2.0) };
        assert!(matches!(placer.fix(&node, &rules, &settings, at), FixOutcome::Fixed { real_end: false }));
        let last = placer.runs[0].pts.windows(2).rev().find(|w| w[0] != w[1]).map(|w| Direction45::from_seg(w[0], w[1])).unwrap();
        let next = placer.preview(&node, &rules, &settings, Point { x: mm(12.0), y: mm(-5.0) }).head.pts;
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

    /// Two SIG pads, A at (0, 0) and B at (3000, 0), joined by an old detour above them, and the tracks given.
    fn two_pads_and_a_detour(extra: &[(Point, Point, &str)]) -> Node {
        use crate::item::Segment;
        let mut node = Node::new();
        for at in [Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: mm(3.0), y: Nm::ZERO }] {
            node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 0), pos: at, shape: Shape::Circle { c: at, r: mm(0.3) }, source: "P".into(), edge: false, rule: 0 }));
        }
        let detour = [Point { x: Nm::ZERO, y: Nm::ZERO }, Point { x: Nm::ZERO, y: mm(2.0) }, Point { x: mm(3.0), y: mm(2.0) }, Point { x: mm(3.0), y: Nm::ZERO }];
        for (k, w) in detour.windows(2).enumerate() {
            node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: w[0], b: w[1], width: mm(0.2), source_track: Some(("trkOld".into(), k)), locked: false }));
        }
        for (k, &(a, b, track)) in extra.iter().enumerate() {
            node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a, b, width: mm(0.2), source_track: Some((track.into(), k)), locked: false }));
        }
        node
    }

    /// `removeLoops`: a route from pad A to pad B, which an old track already joins, makes the old track redundant, and it goes.
    #[test]
    fn finishing_a_route_onto_an_already_connected_same_net_anchor_removes_the_redundant_old_path() {
        let node = two_pads_and_a_detour(&[]);
        let rules = rules();
        let settings = RoutingSettings::default();
        assert!(settings.remove_loops, "sanity: the default this test exercises");
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));
        let outcome = placer.finish(&node, &rules, &settings, Point { x: mm(3.0), y: Nm::ZERO }).expect("must finish onto pad B");
        assert_eq!(outcome.len(), 1, "the new connection itself is still placed");
        let displaced: Vec<_> = placer.displaced_tracks().collect();
        assert_eq!(displaced.len(), 1);
        assert_eq!(displaced[0].0, "trkOld");
        assert!(displaced[0].1.iter().all(|l| l.point_count() < 2), "no replacement geometry -- the old track is simply removed, not moved");
    }

    /// findings p5 F2-7: the loop's ends are those of the line the route is part of, through the track it continues (`ourLine = AssembleLine(
    /// seg )`): a route from the free end X of a stub out of pad A to pad B closes a loop A..B with the old detour, which goes; the stub stays.
    #[test]
    fn remove_loops_follows_the_track_the_route_continues() {
        let x = Point { x: mm(1.0), y: Nm::ZERO };
        let node = two_pads_and_a_detour(&[(Point { x: Nm::ZERO, y: Nm::ZERO }, x, "trkStub")]);
        let mut placer = LinePlacer::start(&node, x, None, net_of("SIG"), 0, mm(0.2));
        placer.finish(&node, &rules(), &RoutingSettings::default(), Point { x: mm(3.0), y: Nm::ZERO }).expect("must finish onto pad B");
        let displaced: Vec<&str> = placer.displaced_tracks().map(|(t, _)| t).collect();
        assert_eq!(displaced, vec!["trkOld"], "the old A-B path goes, the stub the route continues stays");
    }

    #[test]
    fn remove_loops_leaves_a_locked_redundant_path_alone() {
        use crate::item::Segment;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(3.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkOld".into(), 0)), locked: true }));
        let rules = rules();
        let settings = RoutingSettings::default();
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));

        placer.finish(&node, &rules, &settings, Point { x: mm(3.0), y: Nm::ZERO }).expect("must finish");
        assert!(placer.displaced_tracks().next().is_none(), "a locked redundant path must never be removed, matching upstream");
    }

    #[test]
    fn remove_loops_is_a_no_op_when_the_setting_is_off() {
        use crate::item::Segment;
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("SIG"), layer: 0, a: Point { x: Nm::ZERO, y: Nm::ZERO }, b: Point { x: mm(3.0), y: Nm::ZERO }, width: mm(0.2), source_track: Some(("trkOld".into(), 0)), locked: false }));
        let rules = rules();
        let settings = RoutingSettings { remove_loops: false, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, Point { x: Nm::ZERO, y: Nm::ZERO }, None, net_of("SIG"), 0, mm(0.2));

        placer.finish(&node, &rules, &settings, Point { x: mm(3.0), y: Nm::ZERO }).expect("must finish");
        assert!(placer.displaced_tracks().next().is_none(), "remove_loops: false must leave the redundant path in place");
    }
}
