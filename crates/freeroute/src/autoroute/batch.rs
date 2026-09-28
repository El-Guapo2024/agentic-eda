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
use super::locate::locate;
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
    fn is_tail(&self, item: usize) -> bool {
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

/// Route `item` on `net` in pass `pass_no`: search a path from what it is
/// not yet connected to towards what it is, turn it into traces, rip up
/// what it runs through, insert it, and tidy the changed area. The result,
/// and the items ripped up. `BatchAutorouter.autoroute_item` with
/// `AutorouteEngine.autoroute_connection`.
pub fn autoroute_item(rb: &mut RoutingBoard, item: usize, net: i32, pass_no: i32) -> (RouteResult, Vec<usize>) {
    let contains_plane = rb.board.rules.net(net).is_some_and(|n| n.contains_plane);
    let ctrl = Control::for_batch(&rb.board, net, pass_no);
    let unconnected = rb.unconnected_set(item, net);
    if unconnected.is_empty() {
        return (RouteResult::AlreadyConnected, Vec::new());
    }
    let connected = rb.connected_set(item, net);
    if contains_plane && connected.iter().any(|&c| matches!(rb.board.items[c].kind, ItemKind::Area { kind: AreaKind::Conduction { .. }, .. })) {
        return (RouteResult::AlreadyConnected, Vec::new());
    }
    let (start, dest) = if contains_plane { (connected, unconnected) } else { (unconnected, connected) };
    let located = {
        let mut engine = Engine::new(rb, net, ctrl.trace_clearance_class);
        let Some(mut maze) = MazeSearch::new(&mut engine, &ctrl, &start, &dest) else {
            return (RouteResult::NotRouted, Vec::new());
        };
        let Some(found) = maze.find_connection() else {
            return (RouteResult::NotRouted, Vec::new());
        };
        locate(maze.engine, &ctrl, &found)
    };
    let Some(located) = located else {
        return (RouteResult::NotRouted, Vec::new());
    };
    if !ctrl.layer_active[located.start_layer as usize] || !ctrl.layer_active[located.target_layer as usize] {
        return (RouteResult::NotRouted, Vec::new());
    }
    // Ripping up is not ported: the search stops before it would enter an
    // item's room, so nothing is ripped here.
    let ripped: Vec<usize> = Vec::new();
    if !insert_found_connection(rb, &located, &ctrl) {
        return (RouteResult::InsertError, ripped);
    }
    let mut algo = PullTight::new(&[], None, rb.board.rules.pull_tight_accuracy, None, 0);
    algo.opt_changed_area(rb, Some(ctrl.trace_costs.as_slice()));
    rb.changed_area = None;
    (RouteResult::Routed, ripped)
}
