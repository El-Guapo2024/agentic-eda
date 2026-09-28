//! How one connection is to be routed: trace widths, via choices and
//! costs, and the limits of ripping up and pushing aside. Ported from
//! FreeRouting's `AutorouteControl`, with the settings its batch
//! autorouter gives every connection.

use crate::model::Board;

/// A via's reach: the layers it spans. `AutorouteControl.ViaMask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViaMask {
    pub from_layer: i32,
    pub to_layer: i32,
    pub attach_smd_allowed: bool,
}

/// `AutorouteControl`.
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub net_no: i32,
    pub layer_count: usize,
    /// Per layer, the cost factors for horizontal and vertical steps.
    pub trace_costs: Vec<(f64, f64)>,
    pub layer_active: Vec<bool>,
    pub trace_half_width: Vec<i64>,
    /// The half width plus the trace's own share of its clearance: how wide
    /// a trace is in the clearance-compensated tree.
    pub compensated_trace_half_width: Vec<i64>,
    pub via_radius: Vec<f64>,
    /// Extra cost of a via from one layer to another, all 0 in batch
    /// routing: `add_via_costs[from].to_layer[to]`.
    pub add_via_costs: Vec<Vec<i32>>,
    pub trace_clearance_class: i32,
    pub via_clearance_class: i32,
    /// Index into the board's via rules.
    pub via_rule: usize,
    pub via_masks: Vec<ViaMask>,
    pub max_via_radius: f64,
    pub min_normal_via_cost: f64,
    pub min_cheap_via_cost: f64,
    pub vias_allowed: bool,
    pub attach_smd_allowed: bool,
    pub with_neckdown: bool,
    pub is_fanout: bool,
    pub remove_unconnected_vias: bool,
    pub ripup_allowed: bool,
    pub ripup_costs: i32,
    pub ripup_pass_no: i32,
    pub via_lower_bound: i32,
    pub via_upper_bound: i32,
    pub max_shove_trace_recursion_depth: i32,
    pub max_shove_via_recursion_depth: i32,
    pub max_spring_over_recursion_depth: i32,
    pub pull_tight_accuracy: i32,
}

impl Control {
    /// The control for routing net `net_no` with via costs `via_costs`:
    /// `AutorouteControl(board, net, settings, via_costs, trace_costs)`.
    pub fn new(board: &Board, net_no: i32, via_costs: i32, trace_costs: Vec<(f64, f64)>) -> Control {
        let s = &board.settings;
        let rules = &board.rules;
        let layer_count = board.layer_count();
        let mut layer_active = s.layer_active.clone();
        let net_class = rules.net_class(net_no);
        let (trace_clearance_class, via_rule) = match net_class {
            Some(c) => (c.trace_clearance_class, c.via_rule),
            None => (1, 0),
        };
        // BoardRules.get_trace_half_width(net, layer): the net's class, net
        // 1's for the null net.
        let width_class = rules.net_class(if net_no > 0 { net_no } else { 1 });
        let mut trace_half_width = vec![0; layer_count];
        let mut compensated_trace_half_width = vec![0; layer_count];
        for i in 0..layer_count {
            trace_half_width[i] = width_class.map_or(0, |c| c.trace_half_width[i]);
            compensated_trace_half_width[i] = trace_half_width[i] + rules.clearance.compensation(trace_clearance_class, i as i32);
            if net_class.is_some_and(|c| !c.active_layers[i]) {
                layer_active[i] = false;
            }
        }
        let vias = &rules.via_rules[via_rule];
        let via_clearance_class = vias.first().map_or(1, |&v| rules.via_infos[v].clearance_class);
        let mut attach_smd_allowed = false;
        let mut via_radius = vec![0.0f64; layer_count];
        let mut via_masks = Vec::with_capacity(vias.len());
        for &v in vias {
            let info = &rules.via_infos[v];
            if info.attach_smd_allowed {
                attach_smd_allowed = true;
            }
            let padstack = rules.padstack(info.padstack).expect("via padstack");
            for j in padstack.from_layer..=padstack.to_layer {
                let radius = padstack.max_width.get(j as usize).copied().flatten().map_or(0.0, |w| 0.5 * w);
                via_radius[j as usize] = via_radius[j as usize].max(radius);
            }
            via_masks.push(ViaMask { from_layer: padstack.from_layer, to_layer: padstack.to_layer, attach_smd_allowed: info.attach_smd_allowed });
        }
        let mut max_via_radius = 0.0f64;
        for j in 0..layer_count {
            via_radius[j] = via_radius[j].max(trace_half_width[j] as f64);
            max_via_radius = max_via_radius.max(via_radius[j]);
        }
        let min_normal_via_cost = via_costs as f64 * max_via_radius.max(1.0);
        Control {
            net_no,
            layer_count,
            trace_costs,
            layer_active,
            trace_half_width,
            compensated_trace_half_width,
            via_radius,
            add_via_costs: vec![vec![0; layer_count]; layer_count],
            trace_clearance_class,
            via_clearance_class,
            via_rule,
            via_masks,
            max_via_radius,
            min_normal_via_cost,
            min_cheap_via_cost: 0.8 * min_normal_via_cost,
            vias_allowed: s.vias_allowed,
            attach_smd_allowed,
            with_neckdown: s.automatic_neckdown,
            is_fanout: false,
            remove_unconnected_vias: true,
            ripup_allowed: false,
            ripup_costs: 1000,
            ripup_pass_no: 1,
            via_lower_bound: 0,
            via_upper_bound: layer_count as i32,
            max_shove_trace_recursion_depth: 20,
            max_shove_via_recursion_depth: 5,
            max_spring_over_recursion_depth: 5,
            pull_tight_accuracy: 500,
        }
    }

    /// The control the batch autorouter builds for a connection of
    /// `net_no` in ripup pass `pass_no`: plane via costs for a net with a
    /// plane, ripup allowed at the start costs times the pass.
    /// `BatchAutorouter.autoroute_item`.
    pub fn for_batch(board: &Board, net_no: i32, pass_no: i32) -> Control {
        let s = &board.settings;
        let contains_plane = board.rules.net(net_no).is_some_and(|n| n.contains_plane);
        let via_costs = if contains_plane { s.plane_via_costs } else { s.via_costs };
        let mut c = Control::new(board, net_no, via_costs, s.trace_costs.clone());
        c.ripup_allowed = true;
        c.ripup_costs = s.start_ripup_costs * pass_no;
        c.remove_unconnected_vias = !s.with_fanout;
        c
    }
}
