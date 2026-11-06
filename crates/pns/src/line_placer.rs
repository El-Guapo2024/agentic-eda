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
//! 2. **The leg's tail is KiCad's (UI-LEFT R6).** In Walk around and Shove (`FollowMouse()`) the leg is a settled tail and a head routed on
//!    from its end, kept from one `Move` to the next ([`TailState`]): `routeStep`'s `reduceTail`, `handleSelfIntersections`, `handlePullback`,
//!    `optimizeTailHeadTransition` (its `FANOUT_CLEANUP` straightens a short fan-out from a pad) and `mergeHead` are ported
//!    ([`LinePlacer::build_head`]), and the walk and the shove are of the whole leg, split back into head and tail (`splitHeadTail`). A repeated
//!    request for the same cursor starts from the state before the last one, as the tracer does. **Differs:** a head with an arc (the rounded
//!    corner modes) never goes into the tail, so there the head is the leg from its start.
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

/// **INACTIVE since UI-LEFT R7:** the fixed 0.5 mm the route's end once snapped within. The grab radius is KiCad's, the grid
/// ([`RoutingSettings::grab_radius`], `pickSingleItem`'s `maxSlopRadius`), measured to the copper ([`Node::pick_single_item`]); this is its default.
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
    last_move: RefCell<Option<(Point, MouseTrailTracer, TailState)>>,
    /// `m_tail`, `m_head`, `m_direction` and `m_initial_direction` of the leg being routed: what `routeStep` keeps from one `Move` to the next in
    /// Walk around and Shove ([`TailState`]).
    tail_state: RefCell<TailState>,
    /// `FIXED_TAIL::STAGE::pts[0].direction` of each stage ([`Self::stages`]): `m_direction` when the run was fixed, which Backspace gives back.
    stage_dirs: Vec<Direction45>,
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
    /// What [`Self::set_layer`] may switch from (R6-R3): `m_startItem` -- `None` for nothing (empty space, or after a fix), `Some(Some(layers))` for a via or a pad
    /// (`SOLID`) and its layers, `Some(None)` for a track -- and `m_chainedPlacement` (a fix that did not end in a via: the route goes on from that segment).
    start_layers: Option<Option<LayerRange>>,
    chained: bool,
    /// `m_startItem->Kind() == ITEM::SEGMENT_T`: the route started on a track segment (not an arc), so a size change before anything is placed
    /// leaves its width alone (`UpdateSizes`).
    pub start_on_segment: bool,
}

impl LinePlacer {
    /// `LINE_PLACER::Start`: the posture starts at `InitialDirection()`, `N` ("start diagonal" off) whatever the route starts on -- KiCad
    /// computes the direction of a segment it starts from (`lastSegDir`, `initialDir`) but hands the tracer `m_initial_direction`
    /// (`SetDefaultDirections( m_initial_direction, UNDEFINED )`). `start_item` is kept for the callers' shape.
    pub fn start(node: &Node, p: Point, start_item: Option<ItemId>, net: Net, layer: i32, width: Nm) -> Self {
        // `m_initial_direction = Settings().InitialDirection()`: `N` (`NE` with "start diagonal", which the studio does not set). The start
        // segment's direction KiCad's `Start` works out (`lastSegDir`, `initialDir`) is never used (findings p5 F2-12).
        // `m_startItem = aStartItem`: what `SetLayer` may switch from -- a via or a pad on the layer asked, or nothing
        let start_layers = start_item.and_then(|id| node.get(id)).map(|it| matches!(it, crate::item::Item::Via(_) | crate::item::Item::Solid(_)).then(|| it.layers()));
        let direction = Direction45::N;
        let start_on_segment = start_item.and_then(|id| node.get(id)).is_some_and(|it| matches!(it, crate::item::Item::Segment(_)));
        // `LINE_PLACER::Start`: `m_mouseTrailTracer.Clear(); AddTrailPoint( aP ); SetTolerance( m_head.Width() ); SetDefaultDirections( m_initial_direction, UNDEFINED )`
        let mut trail = MouseTrailTracer::new();
        trail.clear();
        trail.add_trail_point(p);
        trail.set_tolerance(width);
        trail.set_default_directions(direction, Direction45::Undefined);
        LinePlacer { net, width, origin: p, direction, initial_direction: direction, manually_forced: false, trail: RefCell::new(trail), last_move: RefCell::new(None), tail_state: RefCell::new(TailState::new(direction)), stage_dirs: Vec::new(), last_head_dir: Cell::new(Direction45::Undefined), placing_via: false, via_diameter: Nm::ZERO, via_drill: Nm::ZERO, via_to_layer: None, via_span: None, runs: Vec::new(), fixed_vias: Vec::new(), current_layer: layer, idle: false, placement_correct: false, displaced_tracks: BTreeMap::new(), displaced_vias: BTreeMap::new(), exclude_end: Vec::new(), end_split: None, springback: RefCell::new(Springback::new()), last_world: RefCell::new(World::default()), last_good: RefCell::new(None), stages: Vec::new(), start_layers, chained: false, start_on_segment }
    }

    /// `FixRoute`'s `setInitialDirection( d_last )` -- "if( m_tail.SegmentCount() == 0 ) m_direction = aDirection;" -- then
    /// `m_fixedTail.AddStage( .., m_direction, .. )`: the direction Backspace gives back to the leg the stage starts.
    fn push_stage_direction(&mut self, d_last: Direction45) {
        let st = self.tail_state.borrow();
        self.stage_dirs.push(if st.tail.len() < 2 { d_last } else { st.direction });
    }

    /// `LINE_PLACER::HasPlacedAnything`: `m_placementCorrect || m_fixedTail.StageCount() > 1`.
    pub fn has_placed_anything(&self) -> bool {
        self.placement_correct || self.stages.len() > 1
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

    /// `LINE_PLACER::routeStep`: in Walk around and Shove (`Settings().FollowMouse()`, which is off in Highlight collisions) the leg is a settled
    /// tail and a head routed on from its end, kept from one `Move` to the next ([`TailState`]); each step first shortens the tail where a straight
    /// run from one of its segments reaches the cursor (`reduceTail`), routes the head (`routeHead`), cuts the tail back where the new head crosses
    /// it (`handleSelfIntersections`) or turns back on it at a right or acute angle (`handlePullback`) -- routing again after either -- and at the
    /// end straightens a fan-out from a pad (`optimizeTailHeadTransition`'s `FANOUT_CLEANUP`), merges the tail's last segments with the head's
    /// first (its `MERGE_SEGMENTS`) or else moves a head of three segments or more with no acute or reversed corner into the tail (`mergeHead`). The
    /// trace shown, fixed and committed is the tail and the head (`Trace()`). In Highlight collisions the tail stays empty and the head is the
    /// elbow from the leg's start.
    ///
    /// When `routeHead` fails (the walk is stuck, or in Shove mode the walk round the pads is: `rhShoveOnly`'s `if( ! rhWalkBase( aP, walkSolids,
    /// ITEM::SOLID_T, RM_Shove, viaOk ) ) return false;`) the head and tail are as they were (`m_tail = std::move( prevTail ); m_head = std::move(
    /// prevHead );`), with no tail a zero-length one at the start ("so that the user gets some feedback that routing is happening"), and the head
    /// shown is the one of the last `Move` that routed. In Walk around and Shove the head is never shown colliding: it stays at the last position
    /// that worked, and a click fixes that (findings p5 F2-4, F2-5).
    ///
    /// **Differs:** a head with an arc in it (a rounded corner mode) is never moved into the tail (KiCad's merges handle arcs); so in the rounded
    /// modes the tail stays empty and the head is the leg from its start, as before this port.
    fn build_head(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> HeadResult {
        let follow_mouse = settings.mode != Mode::MarkObstacles;
        let mut st = self.tail_state.borrow().clone();
        let (mut n_iter, mut go_back, mut fail) = (1, false, false);
        let mut routed: Option<HeadResult> = None;
        let mut i = 0;
        while i < n_iter {
            let prev = st.clone();
            if !go_back && follow_mouse {
                self.reduce_tail(&mut st, node, rules, settings, p);
            }
            go_back = false;
            match self.route_head(node, rules, settings, p, &st) {
                Some((r, head, tail)) => {
                    st.head = head;
                    st.tail = tail;
                    routed = Some(r);
                }
                None => {
                    st = prev;
                    // `if( m_tail.PointCount() == 0 ) { m_tail.Line().Append( m_p_start ); m_tail.Line().Append( m_p_start, true ); }`
                    if st.tail.is_empty() {
                        let p_start = self.fixed_start();
                        st.tail = vec![p_start, p_start];
                    }
                    fail = true;
                    break;
                }
            }
            if handle_self_intersections(&mut st) {
                n_iter += 1;
                go_back = true;
            }
            if !go_back && handle_pullback(&mut st) {
                n_iter += 1;
                st.head.clear();
                go_back = true;
            }
            i += 1;
        }
        let arcs = routed.as_ref().is_some_and(|r| !r.arcs.is_empty());
        if !fail && follow_mouse && !arcs {
            let world = routed.as_ref().map(|r| r.world.clone()).unwrap_or_default();
            if !self.optimize_tail_head_transition(&mut st, node, rules, settings, &world) {
                merge_head(&mut st);
            }
        }
        *self.tail_state.borrow_mut() = st.clone();
        match routed.filter(|_| !fail) {
            Some(mut r) => {
                // `Trace()`: the tail and the head; a head with an arc has no tail (above), and is the line as routed
                if !st.tail.is_empty() || r.arcs.is_empty() {
                    r.pts = trace_of(&st);
                    r.arcs.clear();
                }
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

    /// The world the last `Move` left (`m_currentNode`): in Shove mode the newest springback frame, in the others the world the last click left
    /// (those modes never shove, so their `m_currentNode` is `m_lastNode` of the last `FixRoute`).
    fn current_world(&self, settings: &RoutingSettings) -> World {
        if settings.mode == Mode::Shove {
            self.springback.borrow().top().0
        } else {
            self.springback.borrow().locked_top()
        }
    }

    /// `LINE_PLACER::reduceTail( aEnd )`: from the tail's last segment back to its first, a straight-first trace from the segment's start to the
    /// cursor (`dir.BuildInitialTrace( s.A, aEnd )`, `MITERED_45`); the walk back stops at the first that collides with anything of the world
    /// (`m_currentNode->CheckColliding( &tmp, ITEM::ANY_T )`), and of those that leave in the segment's own direction the earliest is taken: the tail
    /// is cut at that segment's start, the head cleared, the direction the segment's. Not for a tail of fewer than two segments or with no head.
    fn reduce_tail(&self, st: &mut TailState, node: &Node, rules: &BoardRules, settings: &RoutingSettings, end: Point) -> bool {
        let n = st.tail.len().saturating_sub(1);
        if st.head.len() < 2 || n < 2 {
            return false;
        }
        let current = self.current_world(settings);
        let moved;
        let world = if current.tracks.is_empty() && current.vias.is_empty() {
            node
        } else {
            moved = current.apply(node);
            &moved
        };
        let mut reduce: Option<(usize, Direction45)> = None;
        for i in (0..n).rev() {
            let (a, b) = (st.tail[i], st.tail[i + 1]);
            let dir = Direction45::from_seg(a, b);
            let replacement = dir.build_initial_trace(a, end, false, crate::direction45::CornerMode::Mitered45);
            if replacement.len() < 2 {
                continue;
            }
            if walkaround::path_collides(world, rules, &self.net, self.current_layer, self.width, &replacement, &self.exclude()) {
                break;
            }
            if Direction45::from_seg(replacement[0], replacement[1]) == dir {
                reduce = Some((i, dir));
            }
        }
        if let Some((i, dir)) = reduce {
            st.direction = dir;
            st.tail.truncate(i + 1);
            st.head.clear();
            return true;
        }
        if st.tail.len() < 2 {
            st.direction = st.initial;
        }
        false
    }

    /// `LINE_PLACER::optimizeTailHeadTransition`: the trace from a pad or via, short (under ten widths) and ending on a pad or via or in the head's
    /// via, made the direct trace (`OPTIMIZER::Optimize( &linetmp, OPTIMIZER::FANOUT_CLEANUP, m_currentNode )`, not with the posture forced by
    /// `/`): it becomes the head, the tail goes. Else, with three or more segments in the tail, its last three (or fewer) with the head's first two
    /// merged by `MERGE_SEGMENTS`: when that shortens them, they replace the tail's last points and the head is cleared (the head's further points
    /// too, as KiCad's `head.Slice( 0, end )` takes no more), the direction that of the merged line's last segment. `world` is `m_currentNode`.
    fn optimize_tail_head_transition(&self, st: &mut TailState, node: &Node, rules: &BoardRules, settings: &RoutingSettings, world: &World) -> bool {
        let exclude = self.exclude();
        let moved;
        let world_node = if world.tracks.is_empty() && world.vias.is_empty() && world.fixed.is_empty() && world.fixed_vias.is_empty() {
            node
        } else {
            moved = world.apply(node);
            &moved
        };
        let trace = Line::from_points(self.net.clone(), self.current_layer, self.width, trace_of(st));
        if !self.trail.borrow().is_manually_forced() {
            if let Some(t) = optimizer::fanout_cleanup_trace(&trace, self.placing_via, world_node, rules, &exclude, settings.corner_mode) {
                if t.pts.len() < 2 || !t.arcs.is_empty() {
                    return false;
                }
                st.direction = Direction45::from_seg(t.pts[0], t.pts[1]);
                st.head = t.pts;
                st.tail.clear();
                return true;
            }
        }
        // `tail.ShapeCount() < 3`
        if st.tail.len() < 4 {
            return false;
        }
        // `int threshold = std::min( tail.PointCount(), tailLookbackSegments + 1 )` with `tailLookbackSegments = 3`
        let threshold = st.tail.len().min(4);
        let mut opt_line: Vec<Point> = st.tail[st.tail.len() - threshold..].to_vec();
        // `head.Slice( 0, end )` with `end = std::min( 2, head.PointCount() - 1 )`
        if !st.head.is_empty() {
            let end = 2.min(st.head.len() - 1);
            append_points(&mut opt_line, &st.head[..=end]);
        }
        let new_head = Line::from_points(self.net.clone(), self.current_layer, self.width, opt_line);
        let merged = optimizer::optimize_constrained(&new_head, world_node, rules, &exclude, optimizer::effort::MERGE_SEGMENTS, &optimizer::Constraints { corner_mode: settings.corner_mode, ..Default::default() });
        // `mergeFull` returns `current_path.SegmentCount() < segs_pre`
        if merged.segment_count() < new_head.segment_count() && merged.pts.len() >= 2 {
            st.head.clear();
            let keep = st.tail.len() - threshold;
            st.tail.truncate(keep);
            append_points(&mut st.tail, &merged.pts);
            simplify_points(&mut st.tail);
            let n = merged.pts.len();
            st.direction = Direction45::from_seg(merged.pts[n - 2], merged.pts[n - 1]);
            return true;
        }
        false
    }

    /// `routeHead` (`rhMarkObstacles`, `rhWalkOnly`, `rhShoveOnly`) from the tail's end: the head and tail it leaves (`splitHeadTail` of the walked or
    /// shoved line against the old tail, the head then optimized), with the result whose points are the whole line. `None` when it fails.
    fn route_head(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point, st: &TailState) -> Option<(HeadResult, Vec<Point>, Vec<Point>)> {
        // `updatePStart( m_tail )`: the tail's end, else where the leg starts
        let start = st.tail.last().copied().unwrap_or_else(|| self.fixed_start());
        let current = self.current_world(settings);
        if start == p {
            // `buildInitialLine`: "if( m_p_start == aP ) l.Clear()" -- no head
            let mut r = HeadResult::plain(vec![start], false, node, current);
            if !st.tail.is_empty() {
                r.pts = st.tail.clone();
            }
            return Some((r, Vec::new(), st.tail.clone()));
        }
        // `buildInitialLine`: `if( GetFreeAngleMode() && Mode() == RM_MarkObstacles ) l = SHAPE_LINE_CHAIN( { m_p_start, aP } )`, else the trace in the
        // corner mode (`guessedDir.BuildInitialTrace( m_p_start, aP, false, cornerMode )`): a rounded corner is an arc of the head. With a tail the
        // trace leaves in the placer's direction (`m_direction.BuildInitialTrace`), not the posture (which is asked all the same: `GetPosture( aP )`).
        let build_l1 = |to: Point| {
            let mut raw = Line::new(self.net.clone(), self.current_layer, self.width);
            if settings.free_angle() {
                raw.pts = vec![start, to];
            } else {
                let posture = self.posture(settings, to);
                let dir = if st.tail.is_empty() { posture } else { st.direction };
                let trace = dir.build_initial_line(start, to, false, settings.corner_mode);
                (raw.pts, raw.arcs) = (trace.pts, trace.arcs);
            }
            raw
        };
        // `rhWalkBase`: `LINE initTrack( m_tail ); initTrack.Line().Append( l1.CLine() ); initTrack.Line().Simplify();` -- the walk is of the whole leg
        let with_tail = |l1: Line| -> Line {
            if st.tail.is_empty() {
                return l1;
            }
            let mut full = Line::from_points(self.net.clone(), self.current_layer, self.width, st.tail.clone());
            append_points(&mut full.pts, &l1.pts);
            full.simplify();
            full
        };
        let raw = build_l1(p);
        let exclude = self.exclude();
        let iteration_limit = settings.walkaround_iteration_limit.max(0) as u32;
        // `rhWalkBase`: with a via pending, `buildInitialLine` pushes the via off what it sits on first (`v.PushoutForce( m_currentNode, lead,
        // force, collMask, iterLimit )`, `collMask` every kind in Walk around, the pads alone in Shove's pad walk) and builds the head to the
        // pushed position (`BuildInitialTrace( m_p_start, aP + force, .. )`); the head is walked there, and a second round (`while( round < 2
        // && m_placingVia )`) pushes the via again from where the walk ended and walks to that. The via goes at the walked head's end
        // (`v.SetPos( walkFull.CLastPoint() ); walkFull.AppendVia( v )`). Without a via, one round.
        let walk_base = |world: &Node, mask: u8| -> Option<Line> {
            if !self.placing_via {
                return walkaround::walk_base_line(world, rules, &with_tail(raw.clone()), mask, iteration_limit, settings.walkaround_hug_length_threshold, &exclude, settings.corner_mode);
            }
            let to_layer = self.via_to_layer.unwrap_or(self.current_layer);
            let (mut walk_p, mut walked) = (p, None);
            for _round in 0..2 {
                let target = self.push_via(world, rules, settings, walk_p, to_layer, mask);
                let l1 = with_tail(build_l1(target));
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
        // `splitHeadTail( newHead, m_tail, aNewHead, aNewTail )`, then `OPTIMIZER::Optimize( &aNewHead, effort, m_currentNode )`: the whole line (a
        // line with no tail is all head, as before there was a tail) and the new head and tail
        let split_and_optimize = |line: Line, world: &Node| -> (Line, Vec<Point>, Vec<Point>) {
            if st.tail.len() < 2 || line.arc_count() > 0 {
                let optimized = optimize(&line, world);
                let head = optimized.pts.clone();
                return (optimized, head, Vec::new());
            }
            let (head_pts, tail_pts) = split_head_tail(&line.pts, &st.tail);
            let head = optimize(&Line::from_points(self.net.clone(), self.current_layer, self.width, head_pts), world);
            let mut full = Line::from_points(self.net.clone(), self.current_layer, self.width, tail_pts.clone());
            append_points(&mut full.pts, &head.pts);
            (full, head.pts, tail_pts)
        };

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
                if let Some(&first) = walked.pts.first() {
                    parent_node.lock_joint(first, &self.net);
                }
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
                    let (optimized, head, tail) = split_and_optimize(line, &outcome.world);
                    let via = head_via.as_ref().and_then(|_| optimized.pts.last().copied());
                    // `pushSpringback( m_currentNode, m_affectedArea )`: the world this shove left is the newest frame
                    let mut world = parent;
                    world.merge_shove(&outcome.displaced_lines, &outcome.displaced_vias);
                    self.springback.borrow_mut().push(node, world.clone(), outcome.area);
                    let r = HeadResult { pts: optimized.pts, arcs: optimized.arcs, colliding: false, displaced_lines: world.displaced_lines(node), displaced_vias: world.displaced_vias(), world, via, via_shoved: true, kept: false };
                    return Some((r, head, tail));
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
                // `rhMarkObstacles`: "If the head is in colliding state, snap to the hull of the first obstacle. This way, one can route tracks as
                // tightly as possible without enabling the shove/walk mode that certain users find too intrusive."
                let raw = match self.mark_obstacles_snap(world_node, rules, settings, &raw, p) {
                    Some(nearest) => build_l1(nearest),
                    None => raw,
                };
                let colliding = raw.pts.windows(2).any(|w| {
                    let shape = Shape::Stadium { a: w[0], b: w[1], r: self.width / 2 };
                    world_node.first_colliding(&shape, &self.net, LayerRange::single(self.current_layer), rules, &exclude).is_some()
                });
                // `aNewHead = m_head; aNewTail = m_tail;`
                let head = raw.pts.clone();
                let mut r = HeadResult::plain_line(raw, colliding, node, current);
                if !st.tail.is_empty() {
                    let mut full = st.tail.clone();
                    append_points(&mut full, &head);
                    r.pts = full;
                    r.arcs.clear();
                }
                Some((r, head, st.tail.clone()))
            }
            Mode::Walkaround | Mode::Shove => {
                // `rhWalkOnly`: `rhWalkBase( aP, walkFull, ITEM::ANY_T, RM_Walkaround )`, refused if the walk still
                // collides with anything, then the head optimized per the effort setting.
                let walked = walk_base(world_node, crate::node::kind_mask::ANY).filter(|path| !walkaround::path_collides(world_node, rules, &self.net, self.current_layer, self.width, &path.pts, &exclude));
                // `rhWalkOnly` fails when the walk does (ST_STUCK) or still collides
                walked.map(|path| {
                    let (optimized, head, tail) = split_and_optimize(path, world_node);
                    let via = if self.placing_via { optimized.last() } else { None };
                    (HeadResult { via, ..HeadResult::plain_line(optimized, false, node, current) }, head, tail)
                })
            }
        }
    }

    /// `LINE_PLACER::rhMarkObstacles`' hull snap: the obstacle the head `raw` meets first (`m_currentNode->NearestObstacle( &m_head )`, the
    /// pending via counted, `simplifyHull` in a 90-degree corner mode), the hull it is walked round at the head's width (`GetClearance( obs->m_item,
    /// &m_head, false )`, `HullCache( obs->m_item, clearance, m_head.Width(), m_head.Layer() )`) and the point of it nearest the cursor `p` --
    /// `hull.NearestPoint( aP )`, or in `MITERED_90` / `ROUNDED_90` `hull.BBox().NearestPoint( aP )`. When that point is within half the head's width
    /// of the cursor (`( nearest - aP ).EuclideanNorm() < m_head.Width() / 2`), the head is built to it instead (`buildInitialLine( nearest, .. )`):
    /// the caller does that. `None`: no obstacle, or the cursor is not that close to its hull. **Differs:** for a pad or via whose hole reaches
    /// further than its copper the hull is the hole's ([`Node::obstacle_hull`]), where KiCad's nearest obstacle may be the `HOLE` item itself.
    fn mark_obstacles_snap(&self, world: &Node, rules: &BoardRules, settings: &RoutingSettings, raw: &Line, p: Point) -> Option<Point> {
        // `buildInitialLine( aP, m_head, RM_MarkObstacles )`: a pending via is appended at the cursor
        let via = self.placing_via.then(|| self.make_via(p));
        let opts = crate::node::QueryOpts { box_hull: settings.corner_mode.is_90(), ..Default::default() };
        let hit = world.nearest_obstacle(raw, via.as_ref(), rules, &self.exclude(), opts)?;
        let item = world.get(hit.id)?;
        let head = crate::rule_resolver::Side::of_line(raw);
        let hull = world.obstacle_hull(rules, item, head, Nm::ZERO, self.width, self.current_layer, false);
        if hull.is_empty() {
            return None;
        }
        let nearest = if settings.corner_mode.is_90() { bbox_nearest_point(&hull, p) } else { chain_nearest_point(&hull, p) };
        (crate::kigeom::norm_i(nearest.x - p.x, nearest.y - p.y) < self.width / 2).then_some(nearest)
    }

    /// `m_mouseTrailTracer.GetPosture( aP )` (`buildInitialLine`): the direction the head's elbow is built from. With "Use mouse path to set track
    /// posture" off the tracer only keeps the posture the route started with or `/` flipped it to (`SetMouseDisabled`).
    fn posture(&self, settings: &RoutingSettings, p: Point) -> Direction45 {
        let mut trail = self.trail.borrow_mut();
        trail.set_mouse_disabled(!settings.auto_posture);
        trail.get_posture(p)
    }

    /// The start of a `Move` to `p`: the tracer and the leg's tail and head ([`TailState`]) go back to how they were before the last `Move` when that
    /// was to the same cursor (a click on the cursor shows what was shown), and are remembered otherwise.
    fn begin_move(&self, p: Point) {
        let mut last = self.last_move.borrow_mut();
        match &*last {
            Some((at, before, tail)) if *at == p => {
                *self.trail.borrow_mut() = before.clone();
                *self.tail_state.borrow_mut() = tail.clone();
            }
            _ => *last = Some((p, self.trail.borrow().clone(), self.tail_state.borrow().clone())),
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
                    // `m_head.Line().SetPoint( m_head.PointCount() - 1, splitPoint )`
                    if let Some(last) = self.tail_state.borrow_mut().head.last_mut() {
                        *last = split_point;
                    }
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
        // with nothing exactly there, what `pickSingleItem( mousePos, net, layer, false, { m_startItem } )` finds within the grab radius -- the grid
        // (`maxSlopRadius`, UI-LEFT R7) -- by its copper, not its anchor; then `snapToItem` (a track's middle only with snap to tracks, `checkSnap`)
        let layer = if self.placing_via { None } else { Some(self.current_layer) };
        let Some(id) = node.pick_single_item(p, Some(&self.net), layer, self.current_layer, settings.grab_radius, &self.exclude_end) else { return (None, None) };
        match node.snap_to_item(id, p) {
            Some((at, None)) => (Some(at), None),
            Some((at, Some(track))) if settings.snap_to_tracks => (Some(at), Some(track)),
            _ => (None, None),
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
        self.displaced_vias = world.moved_vias();
    }

    /// The fixed runs as the world of the last `Move` has them (a later leg's shove may have pushed the route's own tail, as KiCad's does:
    /// [`World::placed_runs`]): what the preview draws and the commit writes. [`Self::runs`] are the runs as they were fixed, whose ends -- the
    /// points each leg started from, locked joints for every shove -- are where they were.
    pub fn placed_runs(&self) -> Vec<Line> {
        self.last_world.borrow().placed_runs(&self.runs)
    }

    /// The vias the clicks placed ([`Self::fixed_vias`], in order) as the world of the last `Move` has them: a later leg's shove may have pushed one
    /// (`FixRoute` adds it to `m_lastNode` as an ordinary via; [`World::placed_vias`]). What the commit writes.
    pub fn placed_vias(&self) -> Vec<crate::item::Via> {
        let vias: Vec<crate::item::Via> = self.fixed_vias.iter().map(|(v, _)| v.clone()).collect();
        self.last_world.borrow().placed_vias(&vias)
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
        // the stage's `commit` is the newest frame now (`m_fixedTail.AddStage( .., m_direction, m_currentNode )`)
        self.stages.push(self.springback.borrow().len());
        self.push_stage_direction(self.direction);
        self.lock_fixed(node, Some(&head), None);
        self.runs.push(head);
        *self.last_good.borrow_mut() = None;
        self.placement_correct = true;
        // `setInitialDirection( d_last )`, then "m_direction = m_initial_direction; m_head.Line().Clear(); m_tail.Line().Clear();"
        self.tail_state.borrow_mut().reset(self.direction);
        if !real_end {
            // `FixRoute`: "m_startItem = nullptr; .. m_chainedPlacement = !pl.EndsWithVia();"
            self.start_layers = None;
            self.chained = true;
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
            // `SEG lastDirSeg = ( !fixAll && l.SegmentCount() > 1 ) ? l.CSegment( -2 ) : l.CSegment( -1 )`, `setInitialDirection( d_last )`
            let segs: Vec<(Point, Point)> = head.segs().filter(|(a, b)| a != b).collect();
            let last = if !settings.fix_all_segments && segs.len() > 1 { segs[segs.len() - 2] } else { segs[segs.len() - 1] };
            let d_last = Direction45::from_seg(last.0, last.1);
            self.stages.push(self.springback.borrow().len());
            self.push_stage_direction(d_last);
            self.tail_state.borrow_mut().reset(d_last);
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
        // `FixRoute` of a line that ends in a via: "m_startItem = nullptr; .. m_chainedPlacement = !pl.EndsWithVia();"
        self.start_layers = None;
        self.chained = false;
        // `FixRoute` after a via: "lastSegDir = pl.EndsWithVia() ? DIRECTION_45::UNDEFINED : d_last"
        self.restart_trail(via_pos, Direction45::Undefined);
    }

    /// `LINE_PLACER::SetLayer` (R6-R3): the layer keys while routing (`ROUTER_TOOL::onLayerCommand` -> `handleLayerSwitch( aEvent, false )` ->
    /// `ROUTER::SwitchLayer`) move the head to `layer` when nothing holds it to the one it is on -- the route started in empty space or after a via, or on a via
    /// or pad that is on `layer` too -- and not once a fix chained it to a segment (`m_chainedPlacement`) or when it started on a track. The head starts again
    /// from where the leg starts, in the route's first direction (`m_p_start = m_currentStart; m_direction = m_initial_direction; m_mouseTrailTracer.Clear()`).
    /// False: the caller arms a via to `layer` instead (`handleLayerSwitch` goes on to the via).
    pub fn set_layer(&mut self, layer: i32) -> bool {
        if self.idle {
            self.current_layer = layer;
            return true;
        }
        if self.chained {
            return false;
        }
        match self.start_layers {
            None => {}
            Some(Some(layers)) if layers.overlaps_layer(layer) => {}
            Some(_) => return false,
        }
        self.current_layer = layer;
        self.direction = self.initial_direction;
        // `m_direction = m_initial_direction; .. m_head.Line().Clear(); m_tail.Line().Clear();`
        {
            let mut st = self.tail_state.borrow_mut();
            let initial = st.initial;
            st.reset(initial);
        }
        *self.last_good.borrow_mut() = None;
        *self.last_move.borrow_mut() = None;
        self.restart_trail(self.fixed_start(), self.initial_direction);
        true
    }

    /// `LINE_PLACER::UnfixRoute` (Backspace): drop the last fixed run,
    /// restoring posture to whatever the new last run ended with (or the
    /// route's initial direction if none remain). A no-op once there is
    /// nothing left to undo, mirroring `FIXED_TAIL::PopStage`'s refusal to
    /// shrink below the route's own origin.
    pub fn undo_last_segment(&mut self) -> bool {
        let Some(popped) = self.runs.pop() else { return false };
        // `UnfixRoute`: "m_startItem = nullptr;" (`m_chainedPlacement` is left as it was)
        self.start_layers = None;
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
        // `UnfixRoute`: "m_head.Line().Clear(); m_tail.Line().Clear(); .. m_direction = st.pts[0].direction;" (`m_initial_direction` is kept)
        {
            let dir = self.stage_dirs.pop();
            let mut st = self.tail_state.borrow_mut();
            let initial = st.initial;
            st.reset(initial);
            if let Some(d) = dir {
                st.direction = d;
            }
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
        for v in self.placed_vias() {
            w.add(Item::Via(v));
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

/// What `LINE_PLACER` keeps of the leg being routed from one `Move` to the next (Walk around and Shove): the settled `m_tail` from the leg's start,
/// the `m_head` routed on from its end, the direction the head leaves the tail in (`m_direction`) and the direction the leg started in
/// (`m_initial_direction`: `InitialDirection()`, then each click's last segment, `setInitialDirection( d_last )`). Points only: a head with an arc
/// never becomes a tail ([`LinePlacer::build_head`]).
#[derive(Debug, Clone)]
pub(crate) struct TailState {
    tail: Vec<Point>,
    head: Vec<Point>,
    direction: Direction45,
    initial: Direction45,
}

impl TailState {
    fn new(initial: Direction45) -> Self {
        TailState { tail: Vec::new(), head: Vec::new(), direction: initial, initial }
    }

    /// `FixRoute`, `SetLayer`, a via: "m_head.Line().Clear(); m_tail.Line().Clear(); .. m_direction = m_initial_direction;".
    fn reset(&mut self, initial: Direction45) {
        *self = TailState::new(initial);
    }
}

/// `LINE_PLACER::Trace()`: the tail and the head, simplified when there are more than two points ("if we have a zero-length seg as the only part
/// of the trace, we don't want it to be removed at this stage").
fn trace_of(st: &TailState) -> Vec<Point> {
    let mut l = st.tail.clone();
    append_points(&mut l, &st.head);
    if l.len() > 2 {
        simplify_points(&mut l);
    }
    l
}

/// `SHAPE_LINE_CHAIN::Append( chain )`: the points of `more`, the first left out when it repeats the last.
fn append_points(v: &mut Vec<Point>, more: &[Point]) {
    for (i, &p) in more.iter().enumerate() {
        if i == 0 && v.last() == Some(&p) {
            continue;
        }
        v.push(p);
    }
}

/// `SHAPE_LINE_CHAIN::Simplify()` of plain points ([`Line::simplify`]).
fn simplify_points(v: &mut Vec<Point>) {
    let mut l = Line::from_points(None, 0, Nm::ZERO, std::mem::take(v));
    l.simplify();
    *v = l.pts;
}

/// `SHAPE_LINE_CHAIN::Find( aP )`: the index of the point itself.
fn find_point(chain: &[Point], p: Point) -> Option<usize> {
    chain.iter().position(|q| *q == p)
}

/// `LINE_PLACER::splitHeadTail( aNewLine, aOldTail, aNewHead, aNewTail )`: the routed line `line` (the old tail and the new head together) cut
/// where it leaves the old tail -- at the first point of the old tail it no longer has (`l2.CLine().Find( aOldTail.CPoint( i ) ) < 0`), the old
/// tail's end first split into it when it lies on one of its segments (`PointOnEdge`, `Split`). `(head, tail)`; with an old tail of fewer than two
/// points the whole line is the head.
fn split_head_tail(line: &[Point], old_tail: &[Point]) -> (Vec<Point>, Vec<Point>) {
    let mut l2 = line.to_vec();
    let n = l2.len();
    if !(n > 1 && old_tail.len() > 1) {
        return (l2, Vec::new());
    }
    let last = *old_tail.last().unwrap();
    // `if( l2.CLine().PointOnEdge( aOldTail.CLastPoint() ) ) l2.Line().Split( aOldTail.CLastPoint() );`
    if let Some(i) = (0..l2.len() - 1).find(|&i| {
        let (a, b) = (l2[i], l2[i + 1]);
        a == last || b == last || eda_drc::kimath::Seg::new(a, b).sq_distance_to_point(last) <= 1
    }) {
        if !l2.contains(&last) {
            l2.insert(i + 1, last);
        }
    }
    let mut i = 0;
    let mut found = false;
    while i < old_tail.len() {
        if find_point(&l2, old_tail[i]).is_none() {
            found = true;
            break;
        }
        i += 1;
    }
    if !found {
        i -= 1;
    }
    if i >= l2.len() {
        i = l2.len() - 1;
    }
    let tail = if i == 0 { Vec::new() } else { l2[..=i].to_vec() };
    (l2[i..].to_vec(), tail)
}

/// `SHAPE_LINE_CHAIN::Intersect( aChain, aIp )` of two open chains, `ours` against `theirs` (KiCad 8303b2ad's: `theirs`' segments taken in the order
/// of their least x, a collinear pair by `SEG::Contains` of each end, any other by `SEG::Intersect`): each crossing's point and `index_our`, the
/// segment of `ours` it is on (the next one at that segment's end).
fn chain_intersections(ours: &[Point], theirs: &[Point]) -> Vec<(Point, usize)> {
    use eda_drc::kimath::Seg;
    let cross = |o: Point, a: Point, b: Point| (a.x - o.x).area(b.y - o.y) - (a.y - o.y).area(b.x - o.x);
    let contains = |a: Point, b: Point, p: Point| Seg::new(a, b).sq_distance_to_point(p) <= 3;
    let mut sorted: Vec<usize> = (0..theirs.len().saturating_sub(1)).collect();
    sorted.sort_by_key(|&j| theirs[j].x.min(theirs[j + 1].x));
    let mut out = Vec::new();
    for s1 in 0..ours.len().saturating_sub(1) {
        let (a1, b1) = (ours[s1], ours[s1 + 1]);
        for &j in &sorted {
            let (a2, b2) = (theirs[j], theirs[j + 1]);
            if cross(a1, b1, a2).abs() <= 1 && cross(a1, b1, b2).abs() <= 1 {
                if contains(a1, b1, a2) {
                    out.push((a2, s1));
                }
                if contains(a1, b1, b2) {
                    out.push((b2, s1));
                }
                if contains(a2, b2, a1) {
                    out.push((a1, s1));
                }
                if contains(a2, b2, b1) {
                    out.push((b1, s1 + 1));
                }
            } else if let Some(p) = Seg::new(a1, b1).intersect(&Seg::new(a2, b2)) {
                out.push((p, if p == b1 { s1 + 1 } else { s1 }));
            }
        }
    }
    out
}

/// `LINE_PLACER::handleSelfIntersections`: a head that starts where the tail does replaces it ("completely new head trace? chop off the tail");
/// one that crosses the tail cuts it back -- at the crossing nearest the tail's start that is neither the head's start nor the tail's end: before
/// its segment, the direction that segment's, or, on the tail's first two segments, the tail and head both cleared ("just start routing from the
/// beginning"). True when the tail was cut (`routeStep` routes again).
fn handle_self_intersections(st: &mut TailState) -> bool {
    if st.tail.len() < 2 || st.head.len() < 2 {
        return false;
    }
    if st.tail[0] == st.head[0] {
        st.direction = st.initial;
        st.tail.clear();
        return true;
    }
    let ips = chain_intersections(&st.tail, &st.head);
    if ips.is_empty() {
        return false;
    }
    let (mut n, mut ipoint) = (usize::MAX, Point { x: Nm::ZERO, y: Nm::ZERO });
    for &(p, index_our) in &ips {
        if index_our < n {
            n = index_our;
            ipoint = p;
        }
    }
    if ipoint == st.head[0] || Some(&ipoint) == st.tail.last() {
        return false;
    }
    if n < 2 {
        st.direction = st.initial;
        st.tail.clear();
        st.head.clear();
        return true;
    }
    // `const SEG last = tail.CSegment( n - 1 ); m_direction = DIRECTION_45( last ); tail.Remove( n, -1 );`
    st.direction = Direction45::from_seg(st.tail[n - 1], st.tail[n]);
    st.tail.truncate(n);
    true
}

/// `LINE_PLACER::handlePullback`: a tail of one point goes; a head that leaves the tail at a right or acute angle takes the tail's last point back
/// ("hoping that the next iteration will result with a head trace that starts with a segment following our current direction"), the direction
/// the tail's last segment's (the leg's first once the tail has none). True when the tail changed (`routeStep` routes again).
fn handle_pullback(st: &mut TailState) -> bool {
    if st.head.len() < 2 {
        return false;
    }
    let n = st.tail.len();
    if n == 0 {
        return false;
    }
    if n == 1 {
        st.tail.clear();
        return true;
    }
    let first_head = Direction45::from_seg(st.head[0], st.head[1]);
    let last_tail = Direction45::from_seg(st.tail[n - 2], st.tail[n - 1]);
    let angle = first_head.angle_to(&last_tail);
    if matches!(angle, crate::direction45::AngleType::Right | crate::direction45::AngleType::Acute) {
        st.direction = last_tail;
        st.tail.pop();
        if st.tail.len() < 2 {
            st.direction = st.initial;
        }
        return true;
    }
    false
}

/// `LINE_PLACER::mergeHead`: a head of three segments or more, joined to the tail's end, with no acute, reversed or undefined corner of its own
/// or where it leaves the tail (`ANG_ACUTE | ANG_HALF_FULL | ANG_UNDEFINED`), becomes part of the tail; the direction is the tail's last segment's.
fn merge_head(st: &mut TailState) -> bool {
    use crate::direction45::AngleType;
    let forbidden = |a: AngleType| matches!(a, AngleType::Acute | AngleType::HalfFull | AngleType::Undefined);
    simplify_points(&mut st.head);
    simplify_points(&mut st.tail);
    let n_head = st.head.len().saturating_sub(1);
    let n_tail = st.tail.len().saturating_sub(1);
    if n_head < 3 {
        return false;
    }
    if n_tail > 0 && st.head.first() != st.tail.last() {
        return false;
    }
    // `m_head.CountCorners( ForbiddenAngles ) != 0`
    if st.head.windows(3).any(|w| forbidden(Direction45::from_seg(w[0], w[1]).angle_to(&Direction45::from_seg(w[1], w[2])))) {
        return false;
    }
    if n_tail > 0 {
        let dir_head = Direction45::from_seg(st.head[0], st.head[1]);
        let k = st.tail.len();
        let dir_tail = Direction45::from_seg(st.tail[k - 2], st.tail[k - 1]);
        if forbidden(dir_head.angle_to(&dir_tail)) {
            return false;
        }
    }
    let head = std::mem::take(&mut st.head);
    append_points(&mut st.tail, &head);
    simplify_points(&mut st.tail);
    let k = st.tail.len();
    if k >= 2 {
        st.direction = Direction45::from_seg(st.tail[k - 2], st.tail[k - 1]);
    }
    true
}

/// `SHAPE_LINE_CHAIN::NearestPoint( aP )` of a closed chain (a hull): the segment nearest `p` by `SEG::Distance` (the first of equals, the closing
/// segment counted), then `SEG::NearestPoint( aP )` on it.
fn chain_nearest_point(chain: &[Point], p: Point) -> Point {
    let n = chain.len();
    let seg = |i: usize| crate::kigeom::Seg::new(chain[i], chain[(i + 1) % n]);
    let mut nearest = 0;
    let mut min_d = Nm::MAX;
    for i in 0..n {
        let d = seg(i).distance(p);
        if d < min_d {
            min_d = d;
            nearest = i;
        }
    }
    seg(nearest).nearest_point(p)
}

/// `hull.BBox().NearestPoint( aP )`: `p` clamped into the box of the chain's points (`BOX2::NearestPoint`; a point inside the box is itself).
fn bbox_nearest_point(chain: &[Point], p: Point) -> Point {
    let (x0, x1) = (chain.iter().map(|q| q.x).min().unwrap_or(p.x), chain.iter().map(|q| q.x).max().unwrap_or(p.x));
    let (y0, y1) = (chain.iter().map(|q| q.y).min().unwrap_or(p.y), chain.iter().map(|q| q.y).max().unwrap_or(p.y));
    Point { x: p.x.clamp(x0, x1), y: p.y.clamp(y0, y1) }
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(2.5), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(2.5), y: Nm::ZERO }, r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(1.0), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(1.0), y: Nm::ZERO }, r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: Nm::ZERO, y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: Nm::ZERO, y: Nm::ZERO }, r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
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
        blocked.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(0.3), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(0.3), y: Nm::ZERO }, r: mm(0.1) }, source: "U1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
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
        node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: mm(3.0), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(3.0), y: Nm::ZERO }, r: mm(0.4) }, source: "U2.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(2.0), y: Nm::ZERO }, shape: Shape::Circle { c: Point { x: mm(2.0), y: Nm::ZERO }, r: mm(0.4) }, source: "U1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
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

    /// `FixRoute` adds the via a click ends in to the world as an ordinary via (`m_lastNode->Add( Clone( pl.Via() ) )`, not locked). A later leg's
    /// shove that pushes another net's track into it meets it at a locked joint -- the legs on either side of it start and end there (`LockJoint`)
    /// -- and `pushOrShoveVia` gives up (`if( jt->IsLocked() ) return SH_INCOMPLETE`): the shove fails and the head falls back to the walk
    /// (`rhShoveOnly` -> `rhWalkOnly`), GND is not wrapped round the via. (With the via locked, as it was here, the shove walked GND round it:
    /// `SH_TRY_WALK`.) Four legs: east on F.Cu to a via, then on B.Cu east, south, and back west under GND, pushing it north onto the via.
    #[test]
    fn a_later_legs_shove_stops_at_an_earlier_clicks_via() {
        use crate::item::Segment;
        let rules = rules();
        let at = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let mut node = Node::new();
        node.add(Item::Segment(Segment { net: net_of("GND"), layer: 1, a: at(2.0, 0.8), b: at(7.0, 0.8), width: mm(0.2), source_track: Some(("G".into(), 0)), locked: false }));
        let settings = RoutingSettings { mode: Mode::Shove, fix_all_segments: true, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, at(0.0, 0.0), None, net_of("SIG"), 0, mm(0.2));
        placer.preview(&node, &rules, &settings, at(0.5, 0.0));
        placer.preview(&node, &rules, &settings, at(5.0, 0.0));
        placer.toggle_via(true, mm(0.6), mm(0.3));
        placer.switch_layer(&node, &rules, &settings, at(5.0, 0.0), 1, mm(0.6), mm(0.3));
        for (moves, fix) in [(vec![at(6.0, 0.0)], at(8.0, 0.0)), (vec![at(8.0, 0.5)], at(8.0, 0.95))] {
            for m in moves {
                placer.preview(&node, &rules, &settings, m);
            }
            assert_eq!(placer.fix(&node, &rules, &settings, fix), FixOutcome::Fixed { real_end: false });
        }
        let frame = placer.springback.borrow().locked_top().apply(&node);
        let via = frame.iter().find_map(|(_, it)| match it {
            Item::Via(v) => Some(v.clone()),
            _ => None,
        });
        let via = via.expect("the click's via is in the world");
        assert!(!via.locked && via.source_via.as_deref() == Some(crate::springback::fixed_via_tag(0).as_str()), "an ordinary via of the world: {via:?}");
        let near_via = |pv: &Preview| pv.displaced_lines.iter().flat_map(|d| d.line.segs()).any(|(a, b)| crate::kigeom::Seg::new(a, b).distance(at(5.0, 0.0)) < mm(0.7));
        let mut last = None;
        for x in [7.5, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0] {
            last = Some(placer.preview(&node, &rules, &settings, at(x, 0.95)));
        }
        let pv = last.unwrap();
        assert!(!near_via(&pv), "GND is not shoved onto the via, nor walked round it: {:?}", pv.displaced_lines);
        assert_eq!(placer.placed_vias().len(), 1);
        assert_eq!(placer.placed_vias()[0].pos, at(5.0, 0.0), "the via stays where it was placed");
    }

    fn ts(tail: &[(f64, f64)], head: &[(f64, f64)]) -> TailState {
        let at = |&(x, y): &(f64, f64)| Point { x: mm(x), y: mm(y) };
        TailState { tail: tail.iter().map(at).collect(), head: head.iter().map(at).collect(), direction: Direction45::E, initial: Direction45::N }
    }

    /// The tail's own steps, held to `pns_line_placer.cpp`: `mergeHead` takes a head of three segments with no acute corner into the tail (one of
    /// two, or one that turns back, stays the head); `handlePullback` takes the tail's last point back when the head leaves it at a right angle;
    /// `handleSelfIntersections` cuts the tail before the segment the head crosses (and clears both when it is one of the first two); and
    /// `splitHeadTail` keeps as tail what the routed line still has of the old one.
    #[test]
    fn the_tail_is_merged_pulled_back_cut_and_split_as_kicads() {
        let p = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let mut st = ts(&[], &[(0.0, 0.0), (1.0, 0.0), (2.0, 1.0), (4.0, 1.0)]);
        assert!(merge_head(&mut st));
        assert_eq!((st.tail.len(), st.head.len(), st.direction), (4, 0, Direction45::E));
        let mut two = ts(&[], &[(0.0, 0.0), (1.0, 0.0), (2.0, 1.0)]);
        assert!(!merge_head(&mut two), "two segments stay the head");
        let mut back = ts(&[], &[(0.0, 0.0), (2.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
        assert!(!merge_head(&mut back), "a reversal (ANG_HALF_FULL) is forbidden");
        // the head leaves the tail's eastward end going south: a right angle, the tail's last point goes
        let mut pb = ts(&[(0.0, 0.0), (1.0, 0.0), (3.0, 0.0)], &[(3.0, 0.0), (3.0, 2.0)]);
        assert!(handle_pullback(&mut pb));
        assert_eq!(pb.tail, vec![p(0.0, 0.0), p(1.0, 0.0)]);
        assert_eq!(pb.direction, Direction45::E);
        let mut on = ts(&[(0.0, 0.0), (1.0, 0.0), (3.0, 0.0)], &[(3.0, 0.0), (4.0, 1.0)]);
        assert!(!handle_pullback(&mut on), "an obtuse turn is no pullback");
        // a head that comes back across the tail's third segment cuts the tail before it
        let mut si = ts(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0), (4.0, 0.0)], &[(4.0, 0.0), (4.0, 1.0), (2.5, 1.0), (2.5, -1.0)]);
        assert!(handle_self_intersections(&mut si));
        assert_eq!(si.tail, vec![p(0.0, 0.0), p(1.0, 0.0)], "points 0..n-1 of the tail kept, n the crossed segment");
        let mut early = ts(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)], &[(3.0, 0.0), (3.0, 1.0), (0.5, 1.0), (0.5, -1.0)]);
        assert!(handle_self_intersections(&mut early));
        assert!(early.tail.is_empty() && early.head.is_empty() && early.direction == Direction45::N, "on the first two segments: start again");
        // `splitHeadTail`: the walked line has the old tail's first two points, not its third
        let (head, tail) = split_head_tail(&[p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.5), p(4.0, 0.5)], &[p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0)]);
        assert_eq!(tail, vec![p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.5)]);
        assert_eq!(head, vec![p(2.0, 0.5), p(4.0, 0.5)]);
    }

    /// `optimizeTailHeadTransition`'s `FANOUT_CLEANUP`: a trace from a pad to a pad shorter than ten widths is the direct straight-first trace
    /// when that is clear, whatever posture the mouse gave it (here diagonal first, from a cursor that went diagonally); not when `/` forced the
    /// posture (`!m_mouseTrailTracer.IsManuallyForced()`), and not from empty space.
    #[test]
    fn a_short_fan_out_from_a_pad_is_straightened() {
        let rules = rules();
        let at = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let pads = |with_start: bool| {
            let mut node = Node::new();
            let mut pad = |c: Point| node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: c, shape: Shape::Circle { c, r: mm(0.15) }, source: format!("U1.{}", c.x.get()), edge: false, rule: 0, anchor_points: Vec::new() }));
            if with_start {
                pad(at(0.0, 0.0));
            }
            pad(at(1.2, 0.4));
            node
        };
        let settings = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        let route = |node: &Node, flip: bool| {
            let mut placer = LinePlacer::start(node, at(0.0, 0.0), None, net_of("SIG"), 0, mm(0.2));
            for q in [at(0.1, 0.1), at(0.2, 0.2), at(0.3, 0.3), at(0.4, 0.4)] {
                placer.preview(node, &rules, &settings, q);
            }
            if flip {
                placer.flip_posture();
            }
            placer.preview(node, &rules, &settings, at(1.2, 0.4)).head.pts
        };
        let straight = Direction45::Undefined.build_initial_trace(at(0.0, 0.0), at(1.2, 0.4), false, crate::direction45::CornerMode::Mitered45);
        let posture = route(&pads(false), false);
        assert_ne!(posture, straight, "the mouse made it diagonal first: {posture:?}");
        assert_eq!(route(&pads(true), false), straight, "from the pad: straightened");
        assert_ne!(route(&pads(true), true), straight, "a posture forced by / is kept");
    }

    /// `routeStep` in Walk around: a head walked round a pad has three segments or more and becomes the tail (`mergeHead`), and the cursor's next
    /// moves are routed on from the tail (`splitHeadTail` keeps what the walk still has of it); the trace reaches the cursor, collides with nothing,
    /// starts at the leg's start, and the click fixes it and clears the tail.
    #[test]
    fn a_walked_head_becomes_the_tail_and_a_click_fixes_the_trace() {
        let rules = rules();
        let at = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: at(3.0, 0.0), shape: Shape::Circle { c: at(3.0, 0.0), r: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
        let settings = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        let mut placer = LinePlacer::start(&node, at(0.0, 0.0), None, net_of("SIG"), 0, mm(0.2));
        let mut pv = None;
        for x in [0.5, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 6.5] {
            pv = Some(placer.preview(&node, &rules, &settings, at(x, 0.0)));
            assert!(!placer.tail_state.borrow().tail.is_empty() || x < 4.0, "the walk round the pad went into the tail by x = {x}");
        }
        let pv = pv.unwrap();
        assert!(!pv.colliding && pv.head.first() == Some(at(0.0, 0.0)) && pv.head.last() == Some(at(6.5, 0.0)), "{:?}", pv.head.pts);
        assert!(!walkaround::path_collides(&node, &rules, &net_of("SIG"), 0, mm(0.2), &pv.head.pts, &[]));
        assert_eq!(placer.fix(&node, &rules, &settings, at(6.5, 0.0)), FixOutcome::Fixed { real_end: false });
        assert_eq!(placer.runs[0].pts.first(), Some(&at(0.0, 0.0)));
        assert!(placer.tail_state.borrow().tail.is_empty() && placer.tail_state.borrow().head.is_empty());
    }

    /// `reduceTail`: from the tail's last segment back, the earliest whose straight-first trace to the cursor leaves in its own direction and is
    /// clear: the tail is cut at its start, the head cleared, the direction its own. A trace that collides stops the walk back.
    #[test]
    fn reduce_tail_takes_the_tail_back_to_the_earliest_segment_that_reaches_the_cursor() {
        let rules = rules();
        let at = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let settings = RoutingSettings { mode: Mode::Walkaround, ..RoutingSettings::default() };
        let node = Node::new();
        let placer = LinePlacer::start(&node, at(0.0, 0.0), None, net_of("SIG"), 0, mm(0.2));
        let mut st = ts(&[(0.0, 0.0), (1.0, 0.0), (2.0, 1.0), (3.0, 1.0)], &[(3.0, 1.0), (4.0, 1.0)]);
        assert!(placer.reduce_tail(&mut st, &node, &rules, &settings, at(5.0, 0.0)));
        assert_eq!((st.tail.clone(), st.head.len(), st.direction), (vec![at(0.0, 0.0)], 0, Direction45::E));
        // a pad on the straight run from the start: the walk back stops at the segment whose trace meets it, and the last clear one is taken
        let mut blocked = Node::new();
        blocked.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: at(1.5, 0.3), shape: Shape::Circle { c: at(1.5, 0.3), r: mm(0.2) }, source: "U1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
        let mut st = ts(&[(0.0, 0.0), (1.0, 0.0), (2.0, 1.0), (3.0, 1.0)], &[(3.0, 1.0), (4.0, 1.0)]);
        assert!(placer.reduce_tail(&mut st, &blocked, &rules, &settings, at(5.0, 0.0)));
        assert_eq!(st.tail, vec![at(0.0, 0.0), at(1.0, 0.0), at(2.0, 1.0)]);
    }

    /// `rhMarkObstacles`' hull snap: in Highlight collisions a cursor within half the head's width of the hull of the first obstacle the head meets
    /// (`HullCache( obs, clearance, m_head.Width() )`) ends the head on the hull (`buildInitialLine( nearest, .. )`), so it routes tightly and does not
    /// collide; one further inside is left where it is, colliding.
    #[test]
    fn highlight_collisions_snaps_the_head_onto_the_hull_of_the_first_obstacle() {
        let rules = rules();
        let at = |x: f64, y: f64| Point { x: mm(x), y: mm(y) };
        let mut node = Node::new();
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: at(5.0, 0.0), shape: Shape::Rect { x0: mm(4.5), y0: mm(-0.5), x1: mm(5.5), y1: mm(0.5) }, source: "U1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
        let settings = RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() };
        let placer = LinePlacer::start(&node, at(0.0, 0.0), None, net_of("SIG"), 0, mm(0.2));
        // the hull: the pad grown by the clearance and half the width, its left side at 4.5 - 0.2 - 0.1 = 4.2 mm
        let pv = placer.preview(&node, &rules, &settings, at(4.25, 0.0));
        assert_eq!(pv.head.last(), Some(at(4.2, 0.0)), "{:?}", pv.head.pts);
        assert!(!pv.colliding);
        // outside the hull the head collides with nothing, so there is no obstacle to snap to ("If the head is in colliding state")
        assert_eq!(placer.preview(&node, &rules, &settings, at(4.15, 0.0)).head.last(), Some(at(4.15, 0.0)));
        // 150 um inside: no snap, the head collides
        let pv = placer.preview(&node, &rules, &settings, at(4.35, 0.0));
        assert_eq!(pv.head.last(), Some(at(4.35, 0.0)));
        assert!(pv.colliding);
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: mm(4.0), y: mm(0.1) }, shape: Shape::Circle { c: Point { x: mm(4.0), y: mm(0.1) }, r: mm(0.4) }, source: "U1.1".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
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
            node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 0), pos: at, shape: Shape::Circle { c: at, r: mm(0.3) }, source: "P".into(), edge: false, rule: 0, anchor_points: Vec::new() }));
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
