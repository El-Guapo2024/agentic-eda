//! The batch autorouter's pass: which items it routes, in what order, and
//! routing one of them -- search, locate, rip up what the path runs
//! through, insert, tidy. Ported from FreeRouting's `BatchAutorouter`
//! (`autoroute_pass`'s item list and `autoroute_item`),
//! `AutorouteEngine.autoroute_connection`, the connected sets of `Item`,
//! and `RoutingBoard.remove_trace_tails`.

use crate::model::{AreaKind, ItemKind};
use crate::routing::insert::insert_found_connection;
use crate::routing::pull_tight::PullTight;
use crate::routing::trace::StopConnection;
use crate::routing::RoutingBoard;

use super::control::Control;
use super::engine::Engine;
use super::locate::{locate, Located};
use super::maze::MazeSearch;

/// How routing an item came out. `AutorouteEngine.AutorouteResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteResult {
    AlreadyConnected,
    Routed,
    NotRouted,
    InsertError,
}

impl RouteResult {
    /// The Java enum's name.
    pub fn name(&self) -> &'static str {
        match self {
            RouteResult::AlreadyConnected => "ALREADY_CONNECTED",
            RouteResult::Routed => "ROUTED",
            RouteResult::NotRouted => "NOT_ROUTED",
            RouteResult::InsertError => "INSERT_ERROR",
        }
    }
}

impl RoutingBoard {
    /// Pins, vias, traces and pours: FreeRouting's `Connectable` items.
    fn is_connectable(&self, item: usize) -> bool {
        self.board.items[item].is_connectable_kind()
    }

    /// The connectable items of `net` on the board, in its order.
    /// `BasicBoard.get_connectable_items`.
    pub fn connectable_items(&self, net: i32) -> Vec<usize> {
        self.items_in_order().into_iter().filter(|&i| self.is_connectable(i) && self.board.items[i].contains_net(net)).collect()
    }

    /// `BasicBoard.connectable_item_count`.
    pub fn connectable_item_count(&self, net: i32) -> usize {
        self.connectable_items(net).len()
    }

    /// The connectable items of `net` reached from the item through the
    /// contacts of each, by number descending; empty if the item is not of
    /// that net. `Item.get_connected_set(net, false)`.
    pub fn connected_set(&self, item: usize, net: i32) -> Vec<usize> {
        if net > 0 && !self.board.items[item].contains_net(net) {
            return Vec::new();
        }
        let mut result = vec![item];
        let mut stack = vec![item];
        // Depth first, as the Java recurses; the result is a set, so the
        // order of visiting does not show.
        while let Some(at) = stack.pop() {
            for c in self.normal_contacts(at) {
                if net > 0 && !self.board.items[c].contains_net(net) {
                    continue;
                }
                if !result.contains(&c) {
                    result.push(c);
                    stack.push(c);
                }
            }
        }
        self.sort_items(&mut result);
        result
    }

    /// The connectable items of `net` not in the item's connected set, by
    /// number descending. `Item.get_unconnected_set`.
    pub fn unconnected_set(&self, item: usize, net: i32) -> Vec<usize> {
        if net > 0 && !self.board.items[item].contains_net(net) {
            return Vec::new();
        }
        let connected = self.connected_set(item, net);
        let mut result: Vec<usize> = if net > 0 {
            self.connectable_items(net)
        } else {
            self.board.items[item].nets.iter().flat_map(|&n| self.connectable_items(n)).collect()
        };
        result.retain(|i| !connected.contains(i));
        self.sort_items(&mut result);
        result
    }

    /// Whether one of the item's nets is left to the user.
    /// `Item.has_ignored_nets`.
    pub fn has_ignored_nets(&self, item: usize) -> bool {
        self.board.items[item].nets.iter().any(|&n| self.board.rules.net_class(n).is_some_and(|c| c.ignored_by_autorouter))
    }

    /// Whether a trace ends at nothing, or a via connects on one layer
    /// only. `is_tail`, per kind.
    pub(crate) fn is_tail(&self, item: usize) -> bool {
        match &self.board.items[item].kind {
            ItemKind::Trace { .. } => self.start_contacts(item).is_empty() || self.end_contacts(item).is_empty(),
            ItemKind::Via { .. } => {
                let contacts = self.normal_contacts(item);
                if contacts.len() <= 1 {
                    return true;
                }
                let first = &self.board.items[contacts[0]];
                contacts[1..].iter().all(|&c| self.board.items[c].first_layer == first.first_layer && self.board.items[c].last_layer == first.last_layer)
            }
            _ => false,
        }
    }

    /// Remove the traces and vias of `net` (every net if not positive)
    /// that lead nowhere, with the connections they end: true if any went.
    /// `RoutingBoard.remove_trace_tails`.
    pub fn remove_trace_tails(&mut self, net: i32, stop: StopConnection) -> bool {
        let mut stubs = Vec::new();
        for i in self.items_in_order() {
            let item = &self.board.items[i];
            if !item.is_routable() || item.nets.len() != 1 || (net > 0 && item.nets[0] != net) {
                continue;
            }
            if self.is_tail(i) {
                if matches!(item.kind, ItemKind::Via { .. }) {
                    match stop {
                        StopConnection::Via => continue,
                        StopConnection::FanoutVia => unimplemented!("stopping at fanout vias (Item.is_fanout_via) is not ported yet"),
                        StopConnection::None => {}
                    }
                }
                stubs.push(i);
            }
        }
        let mut connections = Vec::new();
        for &i in &stubs {
            if self.normal_contacts(i).len() == 1 {
                connections.extend(self.connection_items(i, stop));
            } else {
                connections.push(i);
            }
        }
        self.sort_items(&mut connections);
        if connections.is_empty() {
            return false;
        }
        self.remove_items(&connections, false);
        self.combine_traces(net);
        true
    }
}

/// The items the pass routes, in its order: each connectable item that is
/// not a trace or via, for each of its nets its connection still missing
/// something, once per such net. `BatchAutorouter.autoroute_pass`.
pub fn pass_items(rb: &RoutingBoard) -> Vec<usize> {
    let mut list = Vec::new();
    let mut handled: Vec<usize> = Vec::new();
    for i in rb.items_in_order() {
        let item = &rb.board.items[i];
        if !item.is_connectable_kind() || item.is_routable() || handled.contains(&i) {
            continue;
        }
        for &net in &item.nets {
            let connected = rb.connected_set(i, net);
            for &c in &connected {
                if rb.board.items[c].nets.len() <= 1 && !handled.contains(&c) {
                    handled.push(c);
                }
            }
            if connected.len() < rb.connectable_item_count(net) && !rb.has_ignored_nets(i) {
                list.push(i);
            }
        }
    }
    list
}

/// What routing one item did.
#[derive(Debug, Clone)]
pub struct Routed {
    pub result: RouteResult,
    /// The items the path ran through, ripped up.
    pub ripped: Vec<usize>,
    /// The search's start and destination items, where it searched.
    pub start: Vec<usize>,
    pub dest: Vec<usize>,
    /// The connection found, as traces.
    pub located: Option<Located>,
    /// The autoroute tree's leaf count and layout hash when the search
    /// started: see [`crate::routing::AutorouteTree::fingerprint`].
    pub tree: Option<(usize, u64)>,
}

/// Route `item` on `net` in pass `pass_no`: search a path from what it is
/// not yet connected to towards what it is, turn it into traces, rip up
/// what it runs through, insert it, and tidy the changed area.
/// `BatchAutorouter.autoroute_item` with `AutorouteEngine.autoroute_connection`.
pub fn autoroute_item(rb: &mut RoutingBoard, item: usize, net: i32, pass_no: i32) -> Routed {
    autoroute_item_with(rb, item, net, pass_no, 0)
}

/// [`autoroute_item`] with the search keeping `search_margin` more room
/// round the trace than it needs, the trace itself going in as wide as
/// ever: FreeRouting's own search can take a gap with no room to spare
/// that its insertion then refuses, and in batch it takes the same gap
/// every pass.
pub fn autoroute_item_with(rb: &mut RoutingBoard, item: usize, net: i32, pass_no: i32, search_margin: i64) -> Routed {
    let mut out = Routed { result: RouteResult::NotRouted, ripped: Vec::new(), start: Vec::new(), dest: Vec::new(), located: None, tree: None };
    // autoroute_pass marks what changes from here on, ripping up included;
    // an area a failed connection left is kept for the next.
    rb.start_marking_changed_area();
    let contains_plane = rb.board.rules.net(net).is_some_and(|n| n.contains_plane);
    // FreeRouting throws making the control, which autoroute_pass catches.
    let Some(mut ctrl) = Control::try_for_batch(&rb.board, net, pass_no) else {
        return out;
    };
    for w in &mut ctrl.compensated_trace_half_width {
        *w += search_margin;
    }
    let unconnected = rb.unconnected_set(item, net);
    if unconnected.is_empty() {
        out.result = RouteResult::AlreadyConnected;
        return out;
    }
    let connected = rb.connected_set(item, net);
    if contains_plane && connected.iter().any(|&c| matches!(rb.board.items[c].kind, ItemKind::Area { kind: AreaKind::Conduction { .. }, .. })) {
        out.result = RouteResult::AlreadyConnected;
        return out;
    }
    let (start, dest) = if contains_plane { (connected, unconnected) } else { (unconnected, connected) };
    out.start = start.clone();
    out.dest = dest.clone();
    let located = {
        // FreeRouting throws making the tree: autoroute_pass catches it.
        let Some(mut engine) = Engine::try_new(rb, net, ctrl.trace_clearance_class) else {
            return out;
        };
        if rb.fingerprint_trees {
            let (leaves, hash, _) = engine.tree_fingerprint(false);
            out.tree = Some((leaves, hash));
        }
        let Some(mut maze) = MazeSearch::new(&mut engine, &ctrl, &start, &dest) else {
            return out;
        };
        let Some(found) = maze.find_connection() else {
            return out;
        };
        locate(maze.engine, &ctrl, &found)
    };
    let Some(located) = located else {
        return out;
    };
    out.located = Some(located.clone());
    out.ripped = located.ripped.clone();
    if !ctrl.layer_active[located.start_layer as usize] || !ctrl.layer_active[located.target_layer as usize] {
        return out;
    }
    // Rip up the connections of the items the path runs through, then the
    // tails that leaves on their nets.
    let stop = if ctrl.remove_unconnected_vias { StopConnection::None } else { StopConnection::FanoutVia };
    let mut ripped_connections = Vec::new();
    let mut changed_nets: Vec<i32> = Vec::new();
    for &r in &located.ripped {
        ripped_connections.extend(rb.connection_items(r, stop));
        changed_nets.extend(&rb.item(r).nets);
    }
    rb.sort_items(&mut ripped_connections);
    changed_nets.sort_unstable();
    changed_nets.dedup();
    rb.remove_items(&ripped_connections, false);
    for net in changed_nets {
        rb.remove_trace_tails(net, stop);
    }
    if !insert_found_connection(rb, &located, &ctrl) {
        out.result = RouteResult::InsertError;
        return out;
    }
    let mut algo = PullTight::new(&[], None, rb.board.rules.pull_tight_accuracy, None, 0);
    algo.opt_changed_area(rb, Some(ctrl.trace_costs.as_slice()));
    rb.changed_area = None;
    out.result = RouteResult::Routed;
    out
}

/// The clean-up ending a pass: every net's tails removed, and the changed
/// area pulled tight. `BatchAutorouter.remove_tails`, as `autoroute_pass`
/// ends with it.
pub fn remove_pass_tails(rb: &mut RoutingBoard) {
    let stop = if rb.board.settings.with_fanout { StopConnection::FanoutVia } else { StopConnection::None };
    let trace_costs = rb.board.settings.trace_costs.clone();
    rb.start_marking_changed_area();
    rb.remove_trace_tails(-1, stop);
    let mut algo = PullTight::new(&[], None, rb.board.rules.pull_tight_accuracy, None, 0);
    algo.opt_changed_area(rb, Some(trace_costs.as_slice()));
    rb.changed_area = None;
}

/// What one pass of [`autoroute_passes`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassSummary {
    pub pass_no: i32,
    /// The items it set out to route, once per unconnected net.
    pub items: usize,
    /// Connections routed or found connected already.
    pub routed: usize,
    /// Connections not routed.
    pub not_routed: usize,
    /// Of those, the ones whose path was found but would not go in.
    pub insert_errors: usize,
}

/// A hash of the traces and vias on the board, the same for the same
/// copper however it came about. What `BasicBoard.get_hash` is for in the
/// passes: telling a board seen before.
pub fn routing_hash(rb: &RoutingBoard) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut pieces: Vec<u64> = (0..rb.board.items.len())
        .filter(|&i| rb.is_on_board(i))
        .filter_map(|i| {
            let item = rb.item(i);
            let mut h = std::collections::hash_map::DefaultHasher::new();
            item.nets.hash(&mut h);
            match &item.kind {
                ItemKind::Trace { layer, half_width, polyline } => {
                    (0, layer, half_width).hash(&mut h);
                    for l in &polyline.lines {
                        (l.a.x, l.a.y, l.b.x, l.b.y).hash(&mut h);
                    }
                }
                ItemKind::Via { center, padstack, .. } => (1, center.x, center.y, padstack).hash(&mut h),
                _ => return None,
            }
            Some(h.finish())
        })
        .collect();
    pieces.sort_unstable();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    pieces.hash(&mut h);
    h.finish()
}

/// Route the board pass after pass, from `start_pass_no`, until a pass
/// finds nothing left to route, the board comes back to a state a pass
/// started from before, or `max_passes` have run: each routes its items
/// with ripup costs growing with the pass number, then removes the tails
/// and tidies. `BatchAutorouter.autoroute_passes`, less its stop for boards
/// that stop improving, and with no time limits.
pub fn autoroute_passes(rb: &mut RoutingBoard, start_pass_no: i32, max_passes: i32) -> Vec<PassSummary> {
    autoroute_passes_with(rb, start_pass_no, max_passes, 0)
}

/// [`autoroute_passes`] with each search keeping `search_margin` to spare:
/// see [`autoroute_item_with`].
pub fn autoroute_passes_with(rb: &mut RoutingBoard, start_pass_no: i32, max_passes: i32, search_margin: i64) -> Vec<PassSummary> {
    let mut summaries = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pass_no in start_pass_no..start_pass_no + max_passes {
        let items = pass_items(rb);
        // A board seen before would only go round again.
        if items.is_empty() || !seen.insert(routing_hash(rb)) {
            break;
        }
        let mut summary = PassSummary { pass_no, items: items.len(), routed: 0, not_routed: 0, insert_errors: 0 };
        for &item in &items {
            // The nets as the item has them now; the item may have left the
            // board, and is routed still, as the Java routes the object.
            let nets = rb.item(item).nets.clone();
            for net in nets {
                match autoroute_item_with(rb, item, net, pass_no, search_margin).result {
                    RouteResult::Routed | RouteResult::AlreadyConnected => summary.routed += 1,
                    RouteResult::NotRouted => summary.not_routed += 1,
                    RouteResult::InsertError => {
                        summary.not_routed += 1;
                        summary.insert_errors += 1;
                    }
                }
            }
        }
        remove_pass_tails(rb);
        summaries.push(summary);
    }
    summaries
}
