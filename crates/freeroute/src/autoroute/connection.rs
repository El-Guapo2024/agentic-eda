//! The connection a routed item belongs to: the traces and vias from it
//! to the next fork or item the autorouter may not rip up, both ways. What
//! ripping an item up costs goes by how far its connection detours.
//! Ported from FreeRouting's `autoroute.Connection`.

use crate::geometry::{FloatPoint, Point};
use crate::model::ItemKind;
use crate::routing::RoutingBoard;

/// `Connection.DETOUR_ADD`.
const DETOUR_ADD: f64 = 100.0;
/// `Connection.DETOUR_ITEM_COST`.
const DETOUR_ITEM_COST: f64 = 0.1;

/// A routing connection ending at the next fork or terminal item.
#[derive(Debug, Clone)]
pub struct Connection {
    /// Where it ends, `None` where it ends in empty space.
    pub start_point: Option<Point>,
    pub start_layer: i32,
    pub end_point: Option<Point>,
    pub end_layer: i32,
    /// Its items, by number descending.
    pub items: Vec<usize>,
}

impl Connection {
    /// The connection of a routable item; `None` for any other.
    /// `Connection.get`, less the caching in the items, which
    /// [`Engine::connection`](super::engine::Engine::connection) does.
    pub fn of(rb: &RoutingBoard, item: usize) -> Option<Connection> {
        if !rb.item(item).is_routable() {
            return None;
        }
        let mut items = vec![item];
        let (mut start_point, mut start_layer, mut end_point, mut end_layer) = (None, 0, None, 0);
        for first in rb.normal_contacts(item) {
            let Some(mut prev_point) = rb.normal_contact_point(item, first) else {
                // No unique contact point.
                continue;
            };
            let mut prev_layer = rb.first_common_layer(item, first);
            // From a trace, only one contact where it meets the next item;
            // pins and vias belong to several connections.
            let mut fork_found = matches!(rb.item(item).kind, ItemKind::Trace { .. }) && rb.trace_contacts_at(item, &prev_point).len() != 1;
            let mut curr = first;
            // Along the contacts to the next fork or item not routable.
            loop {
                if !rb.item(curr).is_routable() || fork_found {
                    match &start_point {
                        None => {
                            start_point = Some(prev_point);
                            start_layer = prev_layer;
                        }
                        Some(start) if !prev_point.java_equals(start) => {
                            end_point = Some(prev_point);
                            end_layer = prev_layer;
                        }
                        Some(_) => {}
                    }
                    break;
                }
                if !items.contains(&curr) {
                    items.push(curr);
                }
                // The contacts but the one where it was entered: one
                // leads on, more are a fork, none a stub.
                let (mut next_point, mut next_layer, mut next) = (None, -1, None);
                for c in rb.normal_contacts(curr) {
                    let layer = rb.first_common_layer(curr, c);
                    if layer < 0 {
                        continue;
                    }
                    let Some(point) = rb.normal_contact_point(curr, c) else {
                        fork_found = true;
                        break;
                    };
                    if prev_layer != layer || !prev_point.java_equals(&point) {
                        next_point = Some(point);
                        next_layer = layer;
                        if next.is_some() {
                            // A second way on: the point is the second's,
                            // as the Java sets it before it looks.
                            fork_found = true;
                            break;
                        }
                        next = Some(c);
                    }
                }
                let Some(n) = next else { break };
                curr = n;
                prev_point = next_point.expect("set with next");
                prev_layer = next_layer;
            }
        }
        rb.sort_items(&mut items);
        Some(Connection { start_point, start_layer, end_point, end_layer, items })
    }

    /// The summed length of its traces, in its item order.
    /// `Connection.trace_length`.
    pub fn trace_length(&self, rb: &RoutingBoard) -> f64 {
        let mut result = 0.0;
        for &i in &self.items {
            if let ItemKind::Trace { polyline, .. } = &rb.item(i).kind {
                result += polyline.length_approx();
            }
        }
        result
    }

    /// How much longer it runs than the straight line between its ends,
    /// with a charge per item. `Connection.get_detour`.
    pub fn detour(&self, rb: &RoutingBoard) -> f64 {
        let (Some(start), Some(end)) = (&self.start_point, &self.end_point) else {
            return i32::MAX as f64;
        };
        let min_trace_length = FloatPoint::from_point(start).distance(&FloatPoint::from_point(end));
        (self.trace_length(rb) + DETOUR_ADD) / (min_trace_length + DETOUR_ADD) + DETOUR_ITEM_COST * (self.items.len() as f64 - 1.0)
    }
}
