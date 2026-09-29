//! Tidying a routed board, as FreeRouting's post-route optimizer does: each
//! via and trace in turn, by position, has its connections ripped up and
//! routed again, and the board keeps the new route where it leaves fewer
//! connections open, fewer vias or less copper weighted by width and
//! clearance, and goes back to the old route where not. Pass after pass,
//! alternately with the preferred directions and without, until a pass
//! improves the board too little. Ported from FreeRouting's `BatchOptRoute`,
//! single-threaded and on the board itself (`clone_board` false).
//!
//! The steps are public so the parity tests can hold each to FreeRouting's;
//! [`optimize_board`] runs them all.

use crate::geometry::FloatPoint;
use crate::model::{FixedState, ItemKind};
use crate::routing::trace::StopConnection;
use crate::routing::RoutingBoard;

use super::batch::{autoroute_item_params, pass_items, remove_tails_params, RouteParams, RouteResult};

/// `BatchOptRoute.MAX_AUTOROUTE_PASSES`: the passes an item's ripped
/// connections get to be routed again.
pub const MAX_AUTOROUTE_PASSES: i32 = 6;

/// `BatchOptRoute.ADDITIONAL_RIPUP_COST_FACTOR_AT_START`.
const ADDITIONAL_RIPUP_COST_FACTOR_AT_START: i32 = 10;

/// FreeRouting's default `optimization_improvement_threshold`: a pass
/// improving the board less than this is the last.
pub const IMPROVEMENT_THRESHOLD: f32 = 0.00001;

/// The vias and traces to optimize, by position, read afresh each time as
/// the board changes under them: `BatchOptRoute.ReadSortedRouteItems`.
#[derive(Debug, Clone)]
pub struct SortedRouteItems {
    min_item_coor: FloatPoint,
    min_item_layer: i32,
}

impl Default for SortedRouteItems {
    fn default() -> Self {
        SortedRouteItems { min_item_coor: FloatPoint::new(i32::MIN as f64, i32::MIN as f64), min_item_layer: -1 }
    }
}

/// Whether `(p, layer)` comes after `(q, q_layer)`: by x, then y, then layer.
fn after(p: FloatPoint, layer: i32, q: FloatPoint, q_layer: i32) -> bool {
    p.x > q.x || p.x == q.x && (p.y > q.y || p.y == q.y && layer > q_layer)
}

impl SortedRouteItems {
    /// The next via or trace after the last one given, in x, then y, then
    /// layer; a trace by its end further along, and not one ending at a via
    /// that is itself to come. Vias first where they tie with traces.
    pub fn next(&mut self, rb: &RoutingBoard) -> Option<usize> {
        let mut result = None;
        let (mut curr_min_coor, mut curr_min_layer) = (FloatPoint::new(i32::MAX as f64, i32::MAX as f64), i32::MAX);
        let order = rb.items_in_order();
        for &i in &order {
            let it = rb.item(i);
            if let ItemKind::Via { center, .. } = it.kind {
                if it.is_user_fixed() {
                    continue;
                }
                let c = FloatPoint::new(center.x as f64, center.y as f64);
                if after(c, it.first_layer, self.min_item_coor, self.min_item_layer) && after(curr_min_coor, curr_min_layer, c, it.first_layer) {
                    curr_min_coor = c;
                    curr_min_layer = it.first_layer;
                    result = Some(i);
                }
            }
        }
        for &i in &order {
            let it = rb.item(i);
            let ItemKind::Trace { layer, .. } = it.kind else { continue };
            if it.is_shove_fixed(&rb.board.rules) {
                continue;
            }
            let first = FloatPoint::from_point(&rb.first_corner(i));
            let last = FloatPoint::from_point(&rb.last_corner(i));
            let compare = if first.x < last.x || first.x == last.x && first.y < last.y { last } else { first };
            if after(compare, layer, self.min_item_coor, self.min_item_layer) && after(curr_min_coor, curr_min_layer, compare, layer) {
                let to_via = rb.normal_contacts(i).iter().any(|&c| matches!(rb.item(c).kind, ItemKind::Via { .. }) && !rb.item(c).is_user_fixed());
                if !to_via {
                    curr_min_coor = compare;
                    curr_min_layer = layer;
                    result = Some(i);
                }
            }
        }
        self.min_item_coor = curr_min_coor;
        self.min_item_layer = curr_min_layer;
        result
    }
}

/// The traces not fixed by the user, their lengths weighted by half width
/// plus clearance, those fixed against pushing at half: what the optimizer
/// shortens. `BatchOptRoute.calc_weighted_trace_length`.
pub fn weighted_trace_length(rb: &RoutingBoard) -> f64 {
    let mut result = 0.0;
    for i in rb.items_in_order() {
        let it = rb.item(i);
        let ItemKind::Trace { layer, half_width, polyline } = &it.kind else { continue };
        if it.fixed != FixedState::Unfixed && it.fixed != FixedState::ShoveFixed {
            continue;
        }
        let mut weighted = polyline.length_approx() * (*half_width + rb.clearance_value(it.clearance_class, 1, *layer)) as f64;
        if it.fixed == FixedState::ShoveFixed {
            // Fewer violations with pin exit directions.
            weighted /= 2.0;
        }
        result += weighted;
    }
    result
}

/// The traces' summed length. `BasicBoard.cumulative_trace_length`.
pub fn cumulative_trace_length(rb: &RoutingBoard) -> f64 {
    let mut result = 0.0;
    for i in rb.items_in_order() {
        if let ItemKind::Trace { polyline, .. } = &rb.item(i).kind {
            result += polyline.length_approx();
        }
    }
    result
}

/// The vias on the board.
pub fn via_count(rb: &RoutingBoard) -> usize {
    rb.items_in_order().into_iter().filter(|&i| matches!(rb.item(i).kind, ItemKind::Via { .. })).count()
}

/// The connections still missing: per net, its connectable items in so
/// many connected sets less one. `RatsNest.incomplete_count`.
pub fn incomplete_count(rb: &RoutingBoard) -> usize {
    let mut by_net: std::collections::BTreeMap<i32, Vec<usize>> = std::collections::BTreeMap::new();
    for i in rb.items_in_order() {
        let it = rb.item(i);
        if it.is_connectable_kind() {
            for &n in &it.nets {
                by_net.entry(n).or_default().push(i);
            }
        }
    }
    let mut result = 0;
    for (net, items) in by_net {
        let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let mut sets = 0;
        for &i in &items {
            if seen.insert(i) {
                sets += 1;
                seen.extend(rb.connected_set(i, net));
            }
        }
        result += sets.max(1) - 1;
    }
    result
}

/// The board's user unit from its own: `CoordinateTransform.board_to_user`.
fn board_to_user(rb: &RoutingBoard, value: f64) -> f64 {
    let (scale, micrometres) = rb.board.user_unit;
    value * scale * micrometres / micrometres
}

/// The optimizer between items and passes.
#[derive(Debug, Clone)]
pub struct Optimizer {
    /// In the first passes ripping up costs more, for speed.
    pub use_increased_ripup_costs: bool,
    /// The weighted trace length to beat.
    pub min_cumulative_trace_length_before: f64,
}

/// One pass: what it started from, where its reading has got to, and how
/// much it improved the board.
#[derive(Debug, Clone)]
pub struct OptPass {
    pub pass_no: i32,
    pub with_preferred_directions: bool,
    via_count_before: usize,
    trace_length_before: f64,
    pub items: SortedRouteItems,
    /// As of the last item improved; 0 if none was.
    pub route_improved: f32,
}

/// An item's connections ripped up, to be routed again.
#[derive(Debug, Clone)]
pub struct Attempt {
    pub item: usize,
    /// The ripped connections' items, by number descending.
    pub ripped: Vec<usize>,
    incomplete_count_before: usize,
    via_count_before: usize,
    /// How its connections are routed again.
    pub params: RouteParams,
}

/// How an attempt came out. `ItemRouteResult`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemResult {
    pub improved: bool,
    pub incomplete_count_before: usize,
    pub incomplete_count_after: usize,
    pub via_count_before: usize,
    pub via_count_after: usize,
    pub trace_length_before: f64,
    pub trace_length_after: f64,
}

/// `BatchOptRoute.contains_only_unfixed_traces`.
fn only_unfixed_traces(rb: &RoutingBoard, items: &[usize]) -> bool {
    items.iter().all(|&i| !rb.item(i).is_user_fixed() && matches!(rb.item(i).kind, ItemKind::Trace { .. }))
}

impl Default for Optimizer {
    fn default() -> Self {
        Optimizer { use_increased_ripup_costs: true, min_cumulative_trace_length_before: 0.0 }
    }
}

impl Optimizer {
    /// Start pass `pass_no`: odd passes route with the preferred directions,
    /// even ones without. The start of `opt_route_pass`.
    pub fn begin_pass(&mut self, rb: &RoutingBoard, pass_no: i32) -> OptPass {
        self.min_cumulative_trace_length_before = weighted_trace_length(rb);
        OptPass {
            pass_no,
            with_preferred_directions: pass_no % 2 != 0,
            via_count_before: via_count(rb),
            trace_length_before: board_to_user(rb, cumulative_trace_length(rb)),
            items: SortedRouteItems::default(),
            route_improved: 0.0,
        }
    }

    /// Rip up the item's connections -- and, for a trace, those of the
    /// traces it forks from where only such traces meet -- after a
    /// snapshot, the net's traces joined up again. `None` where one of them
    /// is fixed by the user, and nothing changes. The start of
    /// `opt_route_item`.
    pub fn begin_item(&mut self, rb: &mut RoutingBoard, item: usize, pass: &OptPass) -> Option<Attempt> {
        let incomplete_count_before = incomplete_count(rb);
        let via_count_before = via_count(rb);
        let mut ripped_items = vec![item];
        if matches!(rb.item(item).kind, ItemKind::Trace { .. }) {
            for contacts in [rb.start_contacts(item), rb.end_contacts(item)] {
                if only_unfixed_traces(rb, &contacts) {
                    ripped_items.extend(contacts);
                }
            }
        }
        rb.sort_items(&mut ripped_items);
        let mut ripped: Vec<usize> = ripped_items.iter().flat_map(|&i| rb.connection_items(i, StopConnection::None)).collect();
        rb.sort_items(&mut ripped);
        if ripped.iter().any(|&i| rb.item(i).is_user_fixed()) {
            return None;
        }
        rb.generate_snapshot();
        rb.remove_items(&ripped, false);
        for net in rb.item(item).nets.clone() {
            combine_traces(rb, net);
        }
        let mut start_ripup_costs = rb.board.settings.start_ripup_costs;
        if self.use_increased_ripup_costs {
            start_ripup_costs *= ADDITIONAL_RIPUP_COST_FACTOR_AT_START;
        }
        if matches!(rb.item(item).kind, ItemKind::Trace { .. }) {
            // Less for traces seems to work better.
            start_ripup_costs = (0.6 * start_ripup_costs as f64).round() as i32;
        }
        let trace_costs = if pass.with_preferred_directions {
            rb.board.settings.trace_costs.clone()
        } else {
            rb.board.settings.preferred_direction_costs.iter().map(|&c| (c, c)).collect()
        };
        let params = RouteParams { start_ripup_costs, trace_costs, remove_unconnected_vias: true, search_margin: 0 };
        Some(Attempt { item, ripped, incomplete_count_before, via_count_before, params })
    }

    /// Weigh the attempt and keep it or undo it. The end of
    /// `opt_route_item`.
    pub fn finish_item(&mut self, rb: &mut RoutingBoard, attempt: &Attempt, pass: &mut OptPass) -> ItemResult {
        let result = ItemResult {
            improved: false,
            incomplete_count_before: attempt.incomplete_count_before,
            incomplete_count_after: incomplete_count(rb),
            via_count_before: attempt.via_count_before,
            via_count_after: via_count(rb),
            trace_length_before: self.min_cumulative_trace_length_before,
            trace_length_after: weighted_trace_length(rb),
        };
        let improved = if result.incomplete_count_after != result.incomplete_count_before {
            result.incomplete_count_after < result.incomplete_count_before
        } else if result.via_count_after != result.via_count_before {
            result.via_count_after < result.via_count_before
        } else {
            result.trace_length_after < result.trace_length_before
        };
        if improved {
            if result.incomplete_count_after < result.incomplete_count_before
                || result.incomplete_count_after == result.incomplete_count_before && result.via_count_after < result.via_count_before
            {
                self.min_cumulative_trace_length_before = result.trace_length_after;
            } else {
                // Only shorter: catch a length that grew elsewhere, say
                // where acid traps went.
                self.min_cumulative_trace_length_before = self.min_cumulative_trace_length_before.min(result.trace_length_after);
            }
            rb.pop_snapshot();
            let (via_after, length_after) = (via_count(rb), board_to_user(rb, cumulative_trace_length(rb)));
            pass.route_improved = if pass.via_count_before != 0 && pass.trace_length_before != 0.0 {
                // The Java divides the via counts as integers.
                (1.0 - (((via_after / pass.via_count_before) as f64 + length_after / pass.trace_length_before) / 2.0)) as f32
            } else {
                0.0
            };
        } else {
            rb.undo();
        }
        ItemResult { improved, ..result }
    }

    /// The pass's improvement, the ripup costs lowered for the passes after
    /// the first that improves nothing: -1 to go on then whatever. The end
    /// of `opt_route_pass`.
    pub fn end_pass(&mut self, pass: &OptPass) -> f32 {
        if self.use_increased_ripup_costs && pass.route_improved == 0.0 {
            self.use_increased_ripup_costs = false;
            return -1.0;
        }
        pass.route_improved
    }
}

/// Join the net's traces end to end wherever they meet alone, until none
/// do. `BasicBoard.combine_traces`.
pub fn combine_traces(rb: &mut RoutingBoard, net: i32) -> bool {
    let mut result = false;
    loop {
        let mut changed = false;
        for i in rb.items_in_order() {
            if rb.is_on_board(i) && rb.item(i).contains_net(net) && matches!(rb.item(i).kind, ItemKind::Trace { .. }) && rb.combine(i) {
                changed = true;
                result = true;
                break;
            }
        }
        if !changed {
            return result;
        }
    }
}

/// Route the ripped connections again: up to [`MAX_AUTOROUTE_PASSES`]
/// passes, each ending with the tails removed, and the tails once more.
/// `BatchAutorouter.autoroute_passes_for_optimizing_item`.
pub fn route_attempt(rb: &mut RoutingBoard, attempt: &Attempt) {
    let mut pass_no = 1;
    let mut still_unrouted = true;
    while still_unrouted && pass_no <= MAX_AUTOROUTE_PASSES {
        let items = pass_items(rb);
        if items.is_empty() {
            still_unrouted = false;
        } else {
            for item in items {
                for net in rb.item(item).nets.clone() {
                    let _: RouteResult = autoroute_item_params(rb, item, net, pass_no, &attempt.params).result;
                }
            }
            remove_tails_params(rb, &attempt.params);
        }
        pass_no += 1;
    }
    remove_tails_params(rb, &attempt.params);
}

/// What the optimizer did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OptSummary {
    pub passes: i32,
    /// Items whose connections were routed again, and those kept.
    pub attempts: usize,
    pub improved: usize,
}

/// Optimize the board: passes until one improves it less than
/// [`IMPROVEMENT_THRESHOLD`], or `max_passes` have run.
/// `BatchOptRoute.optimize_board`.
pub fn optimize_board(rb: &mut RoutingBoard, max_passes: i32) -> OptSummary {
    let mut opt = Optimizer::default();
    let mut summary = OptSummary::default();
    let mut route_improved: f64 = -1.0;
    let mut pass_no = 0;
    while (route_improved >= IMPROVEMENT_THRESHOLD as f64 || route_improved < 0.0) && pass_no < max_passes {
        pass_no += 1;
        let mut pass = opt.begin_pass(rb, pass_no);
        while let Some(item) = pass.items.next(rb) {
            let Some(attempt) = opt.begin_item(rb, item, &pass) else { continue };
            route_attempt(rb, &attempt);
            summary.attempts += 1;
            if opt.finish_item(rb, &attempt, &mut pass).improved {
                summary.improved += 1;
            }
        }
        route_improved = opt.end_pass(&pass) as f64;
    }
    summary.passes = pass_no;
    summary
}
