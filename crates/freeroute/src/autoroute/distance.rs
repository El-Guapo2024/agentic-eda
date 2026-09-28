//! A lower bound on the cost still to go: from a point on a layer to the
//! nearest destination, allowing for vias and each layer's trace costs.
//! It orders the maze search's queue. Ported from FreeRouting's
//! `DestinationDistance`, operation for operation, as its sums break ties.

use crate::geometry::{FloatPoint, IntBox, CRIT};

/// The Java's empty box, the critical bound inverted.
const EMPTY: IntBox = IntBox::new(CRIT, CRIT, -CRIT, -CRIT);

#[derive(Debug, Clone)]
pub struct DestinationDistance {
    trace_costs: Vec<(f64, f64)>,
    layer_count: usize,
    active_layer_count: usize,
    min_component_side_trace_cost: f64,
    max_component_side_trace_cost: f64,
    min_solder_side_trace_cost: f64,
    max_solder_side_trace_cost: f64,
    max_inner_side_trace_cost: f64,
    min_component_inner_trace_cost: f64,
    min_solder_inner_trace_cost: f64,
    min_component_solder_inner_trace_cost: f64,
    min_normal_via_cost: f64,
    min_cheap_via_cost: f64,
    component_side_box: IntBox,
    solder_side_box: IntBox,
    inner_side_box: IntBox,
    box_is_empty: bool,
    component_side_box_is_empty: bool,
    solder_side_box_is_empty: bool,
    inner_side_box_is_empty: bool,
}

impl DestinationDistance {
    pub fn new(trace_costs: &[(f64, f64)], layer_active: &[bool], min_normal_via_cost: f64, min_cheap_via_cost: f64) -> Self {
        let layer_count = layer_active.len();
        let active_layer_count = layer_active.iter().filter(|a| **a).count();
        let (mut min_c, mut max_c, mut min_s, mut max_s) = (0.0, 0.0, 0.0, 0.0);
        if layer_active[0] {
            let (h, v) = trace_costs[0];
            (min_c, max_c) = if h < v { (h, v) } else { (v, h) };
        }
        if layer_active[layer_count - 1] {
            let (h, v) = trace_costs[layer_count - 1];
            (min_s, max_s) = if h < v { (h, v) } else { (v, h) };
        }
        let mut max_inner = f64::min(max_c, max_s);
        for ind in 1..layer_count.saturating_sub(1) {
            if !layer_active[ind] {
                continue;
            }
            let (h, v) = trace_costs[ind];
            max_inner = max_inner.min(h.max(v));
        }
        let min_component_inner = f64::min(min_c, max_inner);
        let min_solder_inner = f64::min(min_s, max_inner);
        DestinationDistance {
            trace_costs: trace_costs.to_vec(),
            layer_count,
            active_layer_count,
            min_component_side_trace_cost: min_c,
            max_component_side_trace_cost: max_c,
            min_solder_side_trace_cost: min_s,
            max_solder_side_trace_cost: max_s,
            max_inner_side_trace_cost: max_inner,
            min_component_inner_trace_cost: min_component_inner,
            min_solder_inner_trace_cost: min_solder_inner,
            min_component_solder_inner_trace_cost: f64::min(min_component_inner, min_solder_inner),
            min_normal_via_cost,
            min_cheap_via_cost,
            component_side_box: EMPTY,
            solder_side_box: EMPTY,
            inner_side_box: EMPTY,
            box_is_empty: true,
            component_side_box_is_empty: true,
            solder_side_box_is_empty: true,
            inner_side_box_is_empty: true,
        }
    }

    /// Add a destination: `b` on `layer`. `DestinationDistance.join`.
    pub fn join(&mut self, b: &IntBox, layer: i32) {
        if layer == 0 {
            self.component_side_box = self.component_side_box.union(b);
            self.component_side_box_is_empty = false;
        } else if layer == self.layer_count as i32 - 1 {
            self.solder_side_box = self.solder_side_box.union(b);
            self.solder_side_box_is_empty = false;
        } else {
            self.inner_side_box = self.inner_side_box.union(b);
            self.inner_side_box_is_empty = false;
        }
        self.box_is_empty = false;
    }

    /// The bound from `p` on `layer`. `calculate(FloatPoint, int)`.
    pub fn calculate_point(&self, p: &FloatPoint, layer: i32) -> f64 {
        self.calculate(&p.bounding_box(), layer)
    }

    /// The bound from `b` on `layer`. `calculate(IntBox, int)`.
    pub fn calculate(&self, b: &IntBox, layer: i32) -> f64 {
        if self.box_is_empty {
            return i32::MAX as f64;
        }
        let delta = |d: &IntBox| -> (f64, f64) {
            let dx = if b.ll.x > d.ur.x {
                (b.ll.x - d.ur.x) as f64
            } else if b.ur.x < d.ll.x {
                (d.ll.x - b.ur.x) as f64
            } else {
                0.0
            };
            let dy = if b.ll.y > d.ur.y {
                (b.ll.y - d.ur.y) as f64
            } else if b.ur.y < d.ll.y {
                (d.ll.y - b.ur.y) as f64
            } else {
                0.0
            };
            // (max, min) of the two, as the Java orders them.
            if dx > dy {
                (dx, dy)
            } else {
                (dy, dx)
            }
        };
        let (c_max, c_min) = delta(&self.component_side_box);
        let (s_max, s_min) = delta(&self.solder_side_box);
        let (i_max, i_min) = delta(&self.inner_side_box);
        let via = self.min_normal_via_cost;
        let min_c = self.min_component_side_trace_cost;
        let min_s = self.min_solder_side_trace_cost;
        let mut result = i32::MAX as f64;
        if layer == 0 {
            if !self.component_side_box_is_empty {
                let (h, v) = self.trace_costs[0];
                result = b.weighted_distance(&self.component_side_box, h, v);
            }
            if self.active_layer_count <= 1 {
                return result;
            }
            let tmp = if min_s < min_c { min_s * s_max + min_c * s_min + via } else { min_c * s_max + min_s * s_min + via };
            result = result.min(tmp);
            result = result.min(c_max + c_min * self.min_component_inner_trace_cost + 2.0 * via);
            if self.active_layer_count == 2 {
                return result;
            }
            result = result.min(i_max + i_min * self.min_component_inner_trace_cost + via);
            result = result.min(s_max + self.min_component_solder_inner_trace_cost * s_min + 2.0 * via);
            result = result.min(c_max + c_min + 2.0 * via);
            if self.active_layer_count == 3 {
                return result;
            }
            result = result.min(i_max + i_min + 2.0 * via);
            result = result.min(s_max + s_min + 3.0 * via);
            return result;
        }
        if layer == self.layer_count as i32 - 1 {
            if !self.solder_side_box_is_empty {
                let (h, v) = self.trace_costs[layer as usize];
                result = b.weighted_distance(&self.solder_side_box, h, v);
            }
            let tmp = if min_c < min_s { min_c * c_max + min_s * c_min + via } else { min_s * c_max + min_c * c_min + via };
            result = result.min(tmp);
            result = result.min(s_max + s_min * self.min_solder_inner_trace_cost + 2.0 * via);
            if self.active_layer_count <= 2 {
                return result;
            }
            result = result.min(i_min * self.min_solder_inner_trace_cost + i_max + via);
            result = result.min(c_max + self.min_component_solder_inner_trace_cost * c_min + 2.0 * via);
            result = result.min(s_max + s_min + 2.0 * via);
            if self.active_layer_count == 3 {
                return result;
            }
            result = result.min(i_max + i_min + 2.0 * via);
            result = result.min(c_max + c_min + 3.0 * via);
            return result;
        }
        if !self.inner_side_box_is_empty {
            let (h, v) = self.trace_costs[layer as usize];
            result = b.weighted_distance(&self.inner_side_box, h, v);
        }
        result = result.min(i_max + i_min + via);
        result = result.min(c_max + c_min * self.min_component_inner_trace_cost + via);
        result = result.min(s_max + s_min * self.min_solder_inner_trace_cost + via);
        result = result.min(c_max + c_min + 2.0 * via);
        result = result.min(s_max + s_min + 2.0 * via);
        result
    }

    /// `calculate` with the cheap via cost. `calculate_cheap_distance`.
    pub fn calculate_cheap(&self, b: &IntBox, layer: i32) -> f64 {
        let mut cheap = self.clone();
        cheap.min_normal_via_cost = self.min_cheap_via_cost;
        cheap.calculate(b, layer)
    }

    pub fn max_component_side_trace_cost(&self) -> f64 {
        self.max_component_side_trace_cost
    }

    pub fn max_solder_side_trace_cost(&self) -> f64 {
        self.max_solder_side_trace_cost
    }

    pub fn max_inner_side_trace_cost(&self) -> f64 {
        self.max_inner_side_trace_cost
    }
}
