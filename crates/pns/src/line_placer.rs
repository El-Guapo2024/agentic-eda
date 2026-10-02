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
//! 1. **No `MOUSE_TRAIL_TRACER`.** KiCad infers posture automatically from
//!    the *area* swept between two candidate paths and the actual recorded
//!    mouse trail -- a heuristic that needs a dense stream of intermediate
//!    mouse positions to mean anything. This port keeps the posture
//!    *state* (`direction`, a [`Direction45`]) and the explicit toggle
//!    (`flip_posture`, the `/` key), continues it from the last fixed
//!    segment the way KiCad does too, but does not try to guess a better
//!    posture from cursor movement alone.
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
//! Fixed runs are deliberately **not** inserted into the `Node` passed to
//! `preview()`/`fix()` -- same-net items never collide with each other
//! (`Node::all_colliding`'s own net check), so a same-net tail can never
//! block its own head regardless of whether it's "in" the node. This
//! sidesteps needing a mutable per-session working node during the
//! interactive phase entirely: only `finish()`'s output ever needs to be
//! committed anywhere.

/// `LINE_PLACER::rhWalkOnly`/`rhShoveOnly`'s head effort: merge segments,
/// plus smart pads when enabled (KiCad also requires 45-degree corner mode,
/// the only mode this router places in).
fn head_effort(settings: &RoutingSettings) -> u32 {
    optimizer::effort::MERGE_SEGMENTS | if settings.smart_pads { optimizer::effort::SMART_PADS } else { 0 }
}

use crate::direction45::{CornerMode, Direction45};
use crate::item::{Item, ItemId, Net};
use crate::layer::LayerRange;
use crate::line::Line;
use crate::node::Node;
use crate::settings::{Mode, RoutingSettings};
use crate::shove::{DisplacedLine, DisplacedVia};
use crate::{optimizer, walkaround};
use eda_drc::kimath::Shape;
use eda_model::ir::{Point, Um};
use eda_model::BoardRules;
use std::collections::HashMap;

/// `ANCHOR_SNAP_UM`-equivalent: how close the cursor must be to a same-net
/// pad/via/track-end to snap onto it and offer to finish the route there.
/// Matches the existing single-segment router's own snap radius
/// (`web/studio/src/components/canvas/routing.ts`'s `findRouteAnchor`
/// caller), so the two tools feel consistent.
pub const SNAP_UM: Um = 500;

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
    colliding: bool,
    displaced_lines: Vec<DisplacedLine>,
    displaced_vias: Vec<DisplacedVia>,
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
    pub manually_forced: bool,
    pub placing_via: bool,
    pub via_diameter: Um,
    pub via_drill: Um,
    pub runs: Vec<Line>,
    pub current_layer: i32,
    pub idle: bool,
    pub placement_correct: bool,
    /// Accumulated across every accepted (`fix`/`finish`-absorbed) shove
    /// this session, keyed by source id -- see [`Self::displaced_tracks`].
    displaced_tracks: HashMap<String, Line>,
    displaced_vias: HashMap<String, Point>,
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
        LinePlacer { net, width, origin: p, direction, manually_forced: false, placing_via: false, via_diameter: 0, via_drill: 0, runs: Vec::new(), current_layer: layer, idle: false, placement_correct: false, displaced_tracks: HashMap::new(), displaced_vias: HashMap::new() }
    }

    pub fn fixed_start(&self) -> Point {
        self.runs.last().and_then(|r| r.last()).unwrap_or(self.origin)
    }

    /// `ITEM::AnchorCount`-driven ids this session's own fixed runs may
    /// have already been assembled from, when a run started life as (part
    /// of) an existing track the user clicked into -- never the case for
    /// freshly built runs, so always empty in this port today. Kept as its
    /// own method (rather than inlined) because `build_head`'s exclusion
    /// set is exactly where that would plug in if start-on-existing-track
    /// splitting is added later.
    fn exclude(&self) -> Vec<ItemId> {
        Vec::new()
    }

    /// `LINE_PLACER::buildInitialLine` + `routeHead`: the raw 45-degree
    /// candidate from the last fixed point to `p`, then resolved per the
    /// active routing mode. Pure -- never mutates `self`, including in
    /// `Mode::Shove`: a successful shove's displaced items are reported
    /// back in the result for the caller ([`Self::preview`]/[`Self::fix`])
    /// to decide whether and when to actually keep them.
    fn build_head(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> HeadResult {
        let start = self.fixed_start();
        if start == p {
            return HeadResult { pts: vec![start], colliding: false, displaced_lines: Vec::new(), displaced_vias: Vec::new() };
        }
        let raw = self.direction.build_initial_trace(start, p, false, CornerMode::Mitered45);
        let exclude = self.exclude();

        if settings.mode == Mode::Shove {
            // `rhShoveOnly`: try shove first; a failed shove (locked item,
            // a pad in the way, iteration limit) falls back to walkaround
            // for this call, exactly like upstream.
            if let Some(outcome) = crate::shove::shove_line(node, &raw, &self.net, self.current_layer, self.width, rules, settings) {
                let line = Line::from_points(self.net.clone(), self.current_layer, self.width, outcome.head);
                // Optimizing against the *original* node is deliberately
                // conservative: it doesn't know about this call's own
                // displaced items, so it will never propose a shortcut that
                // only looks clear because something was just pushed out of
                // its way (that would re-introduce the very collision shove
                // just resolved, since nothing here tracks the displaced
                // items' NEW positions as obstacles the optimizer must also
                // avoid). Safe, at the cost of occasionally leaving a
                // slightly less-optimized head than upstream would.
                let optimized = optimizer::optimize_with(&line, node, rules, &exclude, head_effort(settings));
                return HeadResult { pts: optimized.pts, colliding: false, displaced_lines: outcome.displaced_lines, displaced_vias: outcome.displaced_vias };
            }
        }

        match settings.mode {
            Mode::MarkObstacles => {
                let colliding = raw.windows(2).any(|w| {
                    let shape = Shape::Stadium { a: w[0], b: w[1], r: self.width / 2 };
                    node.first_colliding(&shape, &self.net, LayerRange::single(self.current_layer), rules, &exclude).is_some()
                });
                HeadResult { pts: raw, colliding, displaced_lines: Vec::new(), displaced_vias: Vec::new() }
            }
            Mode::Walkaround | Mode::Shove => {
                let wr = walkaround::route(&raw, node, &self.net, self.current_layer, self.width, rules, &exclude, settings.walkaround_iteration_limit as u32);
                match wr.best() {
                    Some(path) => {
                        let line = Line::from_points(self.net.clone(), self.current_layer, self.width, path.clone());
                        let optimized = optimizer::optimize_with(&line, node, rules, &exclude, head_effort(settings));
                        HeadResult { pts: optimized.pts, colliding: false, displaced_lines: Vec::new(), displaced_vias: Vec::new() }
                    }
                    None => HeadResult { pts: raw, colliding: true, displaced_lines: Vec::new(), displaced_vias: Vec::new() }, // ST_STUCK: show the direct line, flagged violating
                }
            }
        }
    }

    /// Via placement for the pending via at the head's end: tries the
    /// exact cursor position first, then a small ring of nearby offsets
    /// (a simplified stand-in for `VIA::PushoutForce`'s iterative search --
    /// see `PARITY.md`), on the *new* layer's span.
    fn place_via(&self, node: &Node, rules: &BoardRules, at: Point, to_layer: i32) -> Point {
        let layers = LayerRange::new(self.current_layer, to_layer);
        let fits = |p: Point| {
            let shape = Shape::Circle { c: p, r: self.via_diameter / 2 };
            node.first_colliding(&shape, &self.net, layers, rules, &[]).is_none()
        };
        if fits(at) {
            return at;
        }
        let step = (self.via_diameter / 2).max(100);
        for ring in 1..=8 {
            let r = step * ring as Um;
            for k in 0..8 {
                let theta = std::f64::consts::PI * 2.0 * k as f64 / 8.0;
                let candidate = Point { x: at.x + (r as f64 * theta.cos()).round() as Um, y: at.y + (r as f64 * theta.sin()).round() as Um };
                if fits(candidate) {
                    return candidate;
                }
            }
        }
        at // give up: leave it at the cursor, marked colliding by the caller's own check
    }

    /// `LINE_PLACER::Move`: the live preview, including end-of-route
    /// snapping onto a same-net anchor near `p`. Never mutates `self` --
    /// not even in `Mode::Shove`, where the shove this computes is always
    /// disposable until [`Self::fix`]/[`Self::finish`] actually accepts it
    /// (see the module doc comment).
    pub fn preview(&self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point) -> Preview {
        let snapped_end = node.nearest_anchor(p, LayerRange::single(self.current_layer), SNAP_UM, Some(&self.net)).map(|(_, a)| a);
        let target = snapped_end.unwrap_or(p);
        let head_result = self.build_head(node, rules, settings, target);
        let mut colliding = head_result.colliding;
        let mut head = Line::from_points(self.net.clone(), self.current_layer, self.width, head_result.pts);
        head.simplify();
        let via = if self.placing_via {
            let via_pos = head.last().unwrap_or(target);
            let placed = self.place_via(node, rules, via_pos, self.current_layer);
            let shape = Shape::Circle { c: placed, r: self.via_diameter / 2 };
            if node.first_colliding(&shape, &self.net, LayerRange::single(self.current_layer), rules, &[]).is_some() {
                colliding = true;
            }
            Some((placed, self.via_diameter, self.via_drill))
        } else {
            None
        };
        Preview { runs: self.runs.clone(), head, colliding, via, snapped_end, displaced_lines: head_result.displaced_lines, displaced_vias: head_result.displaced_vias }
    }

    /// `LINE_PLACER::FixRoute`'s own `!Settings().AllowDRCViolations()`
    /// guard: in every mode except `MarkObstacles` (whose whole point is
    /// "let the user route through a violation and fix it later"), a
    /// still-colliding head must never be committed.
    fn may_commit_despite_collision(settings: &RoutingSettings) -> bool {
        settings.allow_drc_violations()
    }

    /// Merge a just-accepted preview's shoved items into this session's
    /// running displacement set -- keyed by source id, so a later call that
    /// re-shoves (or un-shoves, by no longer touching) the same track only
    /// ever contributes its most recent position to the final commit.
    fn absorb_displacement(&mut self, preview: &Preview) {
        for d in &preview.displaced_lines {
            if let Some(id) = &d.source_track {
                self.displaced_tracks.insert(id.clone(), d.line.clone());
            }
        }
        for d in &preview.displaced_vias {
            self.displaced_vias.insert(d.source_via.clone(), d.pos);
        }
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
        self.direction = preview.head.pts.windows(2).rev().find(|w| w[0] != w[1]).map(|w| Direction45::from_seg(w[0], w[1])).unwrap_or(self.direction);
        self.absorb_displacement(&preview);
        self.runs.push(preview.head);
        self.placement_correct = true;
        FixOutcome::Fixed { real_end }
    }

    /// `LINE_PLACER::ToggleViaPlacement` + the layer-switch half of the
    /// existing single-segment router's `dropViaAndSwitchLayer`: fix the
    /// current head ending in a via, then continue routing from the via's
    /// position on `new_layer`.
    #[allow(clippy::too_many_arguments)] // the query context (node/rules/settings/p) plus the via's own (layer, diameter, drill) -- splitting either group into its own type would just move the count, not reduce it.
    pub fn switch_layer(&mut self, node: &Node, rules: &BoardRules, settings: &RoutingSettings, p: Point, new_layer: i32, diameter: Um, drill: Um) {
        let head_result = self.build_head(node, rules, settings, p);
        let mut head = Line::from_points(self.net.clone(), self.current_layer, self.width, head_result.pts);
        head.simplify();
        for d in &head_result.displaced_lines {
            if let Some(id) = &d.source_track {
                self.displaced_tracks.insert(id.clone(), d.line.clone());
            }
        }
        for d in &head_result.displaced_vias {
            self.displaced_vias.insert(d.source_via.clone(), d.pos);
        }
        let via_pos = self.place_via(node, rules, head.last().unwrap_or(p), new_layer);
        if head.point_count() >= 2 {
            self.runs.push(head);
        }
        self.current_layer = new_layer;
        self.via_diameter = diameter;
        self.via_drill = drill;
        self.origin = via_pos; // the next run (if `runs` was empty) starts here
        self.placing_via = false;
        self.placement_correct = true;
    }

    /// `LINE_PLACER::UnfixRoute` (Backspace): drop the last fixed run,
    /// restoring posture to whatever the new last run ended with (or the
    /// route's initial direction if none remain). A no-op once there is
    /// nothing left to undo, mirroring `FIXED_TAIL::PopStage`'s refusal to
    /// shrink below the route's own origin.
    pub fn undo_last_segment(&mut self) -> bool {
        let Some(popped) = self.runs.pop() else { return false };
        self.direction = self
            .runs
            .last()
            .and_then(|r| r.pts.windows(2).rev().find(|w| w[0] != w[1]))
            .map(|w| Direction45::from_seg(w[0], w[1]))
            .unwrap_or(self.direction);
        let _ = popped;
        true
    }

    /// `LINE_PLACER::FlipPosture` (the `/` key): rotate the posture by one
    /// 45-degree octant and lock it (no further implicit changes -- there
    /// is no automatic posture heuristic to lock against in this port, see
    /// the module doc comment, but the flag is kept for parity/clarity).
    pub fn flip_posture(&mut self) {
        self.direction = self.direction.right();
        self.manually_forced = true;
    }

    pub fn toggle_via(&mut self, enabled: bool, diameter: Um, drill: Um) {
        self.placing_via = enabled;
        self.via_diameter = diameter;
        self.via_drill = drill;
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
        self.absorb_displacement(&preview);
        let real_end = preview.snapped_end.is_some();
        if preview.head.point_count() >= 2 {
            self.runs.push(preview.head);
        }
        self.idle = true;
        self.placement_correct = !self.runs.is_empty();
        if settings.remove_loops && real_end {
            self.remove_loops(node);
        }
        Some(self.runs.clone())
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
            let has_locked_segment = line.segment_ids.iter().any(|id| matches!(node.get(*id), Some(Item::Segment(s)) if s.locked));
            if has_locked_segment {
                continue;
            }
            let mut track_ids: Vec<String> = line.segment_ids.iter().filter_map(|id| match node.get(*id) { Some(Item::Segment(s)) => s.source_track.as_ref().map(|(t, _)| t.clone()), _ => None }).collect();
            track_ids.sort_unstable();
            track_ids.dedup();
            for track_id in track_ids {
                // An empty replacement (`point_count() < 2`) tells the
                // eventual caller (`crate::router::Router::build_commit`)
                // "remove this track, nothing replaces it" -- the same
                // convention a fully-retracted shove already relies on.
                self.displaced_tracks.insert(track_id, Line::new(self.net.clone(), self.current_layer, 0));
            }
        }
    }

    /// Every other track this session's shove (if any) displaced, by
    /// source `Track::id`, keyed so the latest position for a given track
    /// across however many fix/finish calls touched it wins -- the
    /// caller's final commit must remove and re-add each of these
    /// alongside this session's own new runs, in the same undo step
    /// (`crate::router`/`Cmd::CommitRoute`).
    pub fn displaced_tracks(&self) -> impl Iterator<Item = (&str, &Line)> {
        self.displaced_tracks.iter().map(|(k, v)| (k.as_str(), v))
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 2500, y: 0 }, shape: Shape::Circle { c: Point { x: 2500, y: 0 }, r: 500 }, source: "U1.1".into() }));
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
        node.add(Item::Solid(Solid { net: net_of("GND"), layers: LayerRange::new(0, 1), pos: Point { x: 1000, y: 0 }, shape: Shape::Circle { c: Point { x: 1000, y: 0 }, r: 500 }, source: "U1.1".into() }));
        let rules = rules();
        let mut settings = RoutingSettings { mode: Mode::MarkObstacles, ..RoutingSettings::default() };
        assert!(!settings.can_violate_drc);
        let mut placer = LinePlacer::start(&node, Point { x: 0, y: 0 }, None, net_of("SIG"), 0, 200);
        assert_eq!(placer.fix(&node, &rules, &settings, Point { x: 2000, y: 0 }), FixOutcome::Blocked);
        assert!(placer.finish(&node, &rules, &settings, Point { x: 2000, y: 0 }).is_none());
        settings.can_violate_drc = true;
        assert!(matches!(placer.fix(&node, &rules, &settings, Point { x: 2000, y: 0 }), FixOutcome::Fixed { .. }));
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
        node.add(Item::Solid(Solid { net: net_of("SIG"), layers: LayerRange::new(0, 1), pos: Point { x: 3000, y: 0 }, shape: Shape::Circle { c: Point { x: 3000, y: 0 }, r: 400 }, source: "U2.1".into() }));
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
        assert!(displaced[0].1.point_count() < 2, "no replacement geometry -- the old track is simply removed, not moved");
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
