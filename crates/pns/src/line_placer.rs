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

/// `LINE_PLACER::rhWalkOnly`/`rhShoveOnly`'s head effort: `OE_LOW` merges
/// nothing, `OE_MEDIUM`/`OE_FULL` merge segments; smart pads on top when
/// enabled (KiCad also requires 45-degree corner mode, the only mode this
/// router places in).
fn head_effort(settings: &RoutingSettings) -> u32 {
    let merge = match settings.optimizer_effort {
        crate::settings::OptEffort::Low => 0,
        crate::settings::OptEffort::Medium | crate::settings::OptEffort::Full => optimizer::effort::MERGE_SEGMENTS,
    };
    merge | if settings.smart_pads { optimizer::effort::SMART_PADS } else { 0 }
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
use std::collections::{BTreeMap, HashMap};

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
    /// The posture the session started with (`m_initial_direction`): what
    /// Backspace goes back to once every run has been undone.
    initial_direction: Direction45,
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
    // ordered, so the commit a session produces does not depend on hash order
    displaced_tracks: BTreeMap<String, Vec<Line>>,
    displaced_vias: BTreeMap<String, Point>,
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
        LinePlacer { net, width, origin: p, direction, initial_direction: direction, manually_forced: false, placing_via: false, via_diameter: 0, via_drill: 0, runs: Vec::new(), current_layer: layer, idle: false, placement_correct: false, displaced_tracks: BTreeMap::new(), displaced_vias: BTreeMap::new() }
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
        // `buildInitialLine`: `if( GetFreeAngleMode() && Mode() == RM_MarkObstacles ) l = SHAPE_LINE_CHAIN( { m_p_start, aP } )`
        let raw = if settings.free_angle() { vec![start, p] } else { self.direction.build_initial_trace(start, p, false, CornerMode::Mitered45) };
        let exclude = self.exclude();

        if settings.mode == Mode::Shove {
            // `rhShoveOnly`: first walk the head around the pads alone
            // (`rhWalkBase( aP, walkSolids, ITEM::SOLID_T, RM_Shove )`), then
            // shove tracks and vias out of its way. A pad the head cannot be
            // walked around, a locked item, or the iteration limit falls back
            // to a full walkaround for this call, exactly like upstream.
            let iteration_limit = settings.walkaround_iteration_limit.max(0) as u32;
            if let Some(walked) = walkaround::walk_base(node, rules, &self.net, self.current_layer, self.width, &raw, crate::node::kind_mask::SOLID, iteration_limit, settings.walkaround_hug_length_threshold, &exclude) {
                if let Some(outcome) = crate::shove::shove_line(node, &walked, &self.net, self.current_layer, self.width, rules, settings) {
                    let line = Line::from_points(self.net.clone(), self.current_layer, self.width, outcome.head.clone());
                    // Like `OPTIMIZER::Optimize( &aNewHead, effort, m_currentNode )`
                    // after the shove: against the world the shove left, so a
                    // shortcut is only taken where the pushed items now allow it.
                    let optimized = optimizer::optimize_with(&line, &outcome.world, rules, &exclude, head_effort(settings));
                    return HeadResult { pts: optimized.pts, colliding: false, displaced_lines: outcome.displaced_lines, displaced_vias: outcome.displaced_vias };
                }
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
                // `rhWalkOnly`: `rhWalkBase( aP, walkFull, ITEM::ANY_T, RM_Walkaround )`, refused if the walk still
                // collides with anything, then the head optimized per the effort setting.
                let iteration_limit = settings.walkaround_iteration_limit.max(0) as u32;
                let walked = walkaround::walk_base(node, rules, &self.net, self.current_layer, self.width, &raw, crate::node::kind_mask::ANY, iteration_limit, settings.walkaround_hug_length_threshold, &exclude)
                    .filter(|path| !walkaround::path_collides(node, rules, &self.net, self.current_layer, self.width, path, &exclude));
                match walked {
                    Some(path) => {
                        let line = Line::from_points(self.net.clone(), self.current_layer, self.width, path);
                        let optimized = optimizer::optimize_with(&line, node, rules, &exclude, head_effort(settings));
                        HeadResult { pts: optimized.pts, colliding: false, displaced_lines: Vec::new(), displaced_vias: Vec::new() }
                    }
                    None => HeadResult { pts: raw, colliding: true, displaced_lines: Vec::new(), displaced_vias: Vec::new() }, // ST_STUCK: show the direct line, flagged violating
                }
            }
        }
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
        let via = crate::item::Via { net: self.net.clone(), layers: LayerRange::new(self.current_layer, to_layer), pos: at, diameter: self.via_diameter, drill: self.via_drill, source_via: None, locked: false };
        let start = self.fixed_start();
        let lead = (at.x - start.x, at.y - start.y);
        match crate::shove::via_pushout_force(node, rules, &via, lead, mask, settings.via_force_prop_iteration_limit, &[]) {
            Some(force) => Point { x: at.x + force.0 as Um, y: at.y + force.1 as Um },
            None => at,
        }
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
            let placed = self.place_via(node, rules, settings, via_pos, self.current_layer);
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
        self.absorb(&preview.displaced_lines, &preview.displaced_vias);
    }

    /// The replacement of a track is whatever the latest shove that touched it
    /// left: all of that shove's lines for the track (a track cut by a junction
    /// has several, a line on several imported tracks leaves the others with
    /// none) take the place of what an earlier one said.
    fn absorb(&mut self, lines: &[DisplacedLine], vias: &[DisplacedVia]) {
        let mut fresh: HashMap<&str, Vec<Line>> = HashMap::new();
        for d in lines {
            if let Some(id) = &d.source_track {
                let e = fresh.entry(id.as_str()).or_default();
                if d.line.point_count() >= 2 {
                    e.push(d.line.clone());
                }
            }
        }
        for (id, ls) in fresh {
            self.displaced_tracks.insert(id.to_string(), ls);
        }
        for d in vias {
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
        // `FixRoute`: with "Fix all segments on click" off, a click that does not end the route fixes every segment but
        // the last, which stays free and follows the cursor; the next leg starts where the last one began
        // (`m_currentStart = p_pre_last`) and continues in the direction of the one before it (`lastDirSeg = CSegment( -2 )`).
        let keep_last_free = !settings.fix_all_segments && !real_end && !self.placing_via && preview.head.segment_count() > 1;
        let mut head = preview.head.clone();
        if keep_last_free {
            head.pts.pop();
        }
        self.direction = head.pts.windows(2).rev().find(|w| w[0] != w[1]).map(|w| Direction45::from_seg(w[0], w[1])).unwrap_or(self.direction);
        self.absorb_displacement(&preview);
        self.runs.push(head);
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
        self.absorb(&head_result.displaced_lines, &head_result.displaced_vias);
        let via_pos = self.place_via(node, rules, settings, head.last().unwrap_or(p), new_layer);
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
            .unwrap_or(self.initial_direction);
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

    /// `OE_LOW` merges nothing in the head; the default merges segments.
    #[test]
    fn head_effort_follows_the_optimizer_setting() {
        use crate::settings::OptEffort;
        let low = RoutingSettings { optimizer_effort: OptEffort::Low, smart_pads: false, ..RoutingSettings::default() };
        assert_eq!(head_effort(&low), 0);
        let medium = RoutingSettings { optimizer_effort: OptEffort::Medium, smart_pads: true, ..RoutingSettings::default() };
        assert_eq!(head_effort(&medium), optimizer::effort::MERGE_SEGMENTS | optimizer::effort::SMART_PADS);
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
