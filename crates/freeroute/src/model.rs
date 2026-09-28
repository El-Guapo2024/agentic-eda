//! The board as the router reads it: layers, rules, the autorouter's
//! settings, and every item with its raw shapes, in the board's own order.
//! Ported from the data side of FreeRouting's `RoutingBoard`, `Item` and its
//! subclasses, `BoardRules` and `AutorouteSettings` -- only what routing
//! reads.
//!
//! Items keep the board's order, which is also the order FreeRouting
//! inserts them into its search trees, and their FreeRouting numbers,
//! which break ties throughout the router.

use std::collections::BTreeMap;

use crate::board::{AreaShape, PadShape};
use crate::geometry::{Direction, IntBox, IntPoint, Line, Polyline};
use crate::rules::ClearanceMatrix;

/// How far an item may be changed. FreeRouting's `FixedState`, in its
/// order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FixedState {
    Unfixed,
    ShoveFixed,
    UserFixed,
    SystemFixed,
}

impl FixedState {
    /// From `FixedState.ordinal()`.
    pub fn from_ordinal(n: u32) -> Option<FixedState> {
        [FixedState::Unfixed, FixedState::ShoveFixed, FixedState::UserFixed, FixedState::SystemFixed].get(n as usize).copied()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layer {
    pub name: String,
    pub is_signal: bool,
}

/// A pad or via stack: the layers it spans and, per board layer, the
/// widest extent of its shape there. Only what routing reads of
/// FreeRouting's `Padstack`.
#[derive(Debug, Clone, PartialEq)]
pub struct Padstack {
    pub no: usize,
    pub from_layer: i32,
    pub to_layer: i32,
    /// `get_shape(layer).max_width()`, `None` where the stack has no shape.
    pub max_width: Vec<Option<f64>>,
    /// `get_shape(layer)` per board layer, about the origin; `None` where
    /// the stack has no shape.
    pub shapes: Vec<Option<PadShape>>,
}

/// A via the router may place. FreeRouting's `ViaInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViaInfo {
    /// Its padstack's number.
    pub padstack: usize,
    pub clearance_class: i32,
    pub attach_smd_allowed: bool,
}

/// Routing rules shared by a group of nets. FreeRouting's `NetClass`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetClass {
    pub trace_clearance_class: i32,
    /// Index into [`Rules::via_rules`].
    pub via_rule: usize,
    pub active_layers: Vec<bool>,
    pub trace_half_width: Vec<i64>,
    /// Its traces may not be pushed aside.
    pub shove_fixed: bool,
    /// Its traces are pulled tight. `NetClass.get_pull_tight`.
    pub pull_tight: bool,
    /// Cycles through pours are left alone.
    /// `NetClass.get_ignore_cycles_with_areas`.
    pub ignore_cycles_with_areas: bool,
    /// The autorouter leaves its nets alone. `NetClass.is_ignored_by_autorouter`.
    pub ignored_by_autorouter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Net {
    pub no: i32,
    /// Index into [`Rules::net_classes`].
    pub class: usize,
    pub contains_plane: bool,
}

/// The rules routing reads. FreeRouting's `BoardRules`, and the board
/// library's padstacks.
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    pub clearance: ClearanceMatrix,
    /// `ClearanceMatrix.max_value(class, layer)`: the largest value ever
    /// set in the class's row, which the Java keeps as it goes -- it never
    /// shrinks and is taken before rounding, so it cannot be recomputed from
    /// the matrix.
    pub max_clearance: Vec<Vec<i64>>,
    pub padstacks: Vec<Padstack>,
    pub via_infos: Vec<ViaInfo>,
    /// Each rule's vias, as indices into `via_infos`, in preference order.
    pub via_rules: Vec<Vec<usize>>,
    pub net_classes: Vec<NetClass>,
    pub nets: BTreeMap<i32, Net>,
    pub min_trace_half_width: i64,
    pub default_via_diameter: f64,
    /// `BoardRules.get_pin_edge_to_turn_dist`: how far from a pin's edge a
    /// trace may first turn; negative for no exit restriction.
    pub pin_edge_to_turn_dist: f64,
    /// `BasicBoard.get_max_trace_half_width` when the board was read.
    pub board_max_trace_half_width: i64,
    /// `BoardRules.get_max_trace_half_width`: the widest trace any net
    /// class allows, 100 at least.
    pub max_trace_half_width: i64,
    /// The interactive settings' `trace_pull_tight_accuracy`.
    pub pull_tight_accuracy: i32,
}

impl Rules {
    pub fn net(&self, no: i32) -> Option<&Net> {
        self.nets.get(&no)
    }

    pub fn net_class(&self, net: i32) -> Option<&NetClass> {
        self.net(net).map(|n| &self.net_classes[n.class])
    }

    /// `ClearanceMatrix.max_value(class, layer)`, class and layer clamped
    /// into range as the Java does.
    pub fn max_clearance(&self, class: i32, layer: i32) -> i64 {
        if self.max_clearance.is_empty() {
            return 0;
        }
        let c = (class.max(0) as usize).min(self.max_clearance.len() - 1);
        let row = &self.max_clearance[c];
        if row.is_empty() {
            return 0;
        }
        row[(layer.max(0) as usize).min(row.len() - 1)]
    }

    /// `ClearanceMatrix.max_value(layer)`: the largest value ever set on
    /// the layer, in any class's row.
    pub fn max_clearance_on_layer(&self, layer: i32) -> i64 {
        let mut result = 0;
        for row in &self.max_clearance {
            if row.is_empty() {
                continue;
            }
            result = result.max(row[(layer.max(0) as usize).min(row.len() - 1)]);
        }
        result
    }

    pub fn padstack(&self, no: usize) -> Option<&Padstack> {
        self.padstacks.iter().find(|p| p.no == no)
    }
}

/// What the batch autorouter is told to do. FreeRouting's
/// `AutorouteSettings`, the parts routing reads.
#[derive(Debug, Clone, PartialEq)]
pub struct AutorouteSettings {
    pub vias_allowed: bool,
    pub via_costs: i32,
    pub plane_via_costs: i32,
    pub start_ripup_costs: i32,
    pub with_fanout: bool,
    pub automatic_neckdown: bool,
    pub layer_active: Vec<bool>,
    /// `get_trace_cost_arr()`: per layer, the cost factors for horizontal
    /// and vertical trace segments.
    pub trace_costs: Vec<(f64, f64)>,
    /// `get_preferred_direction_trace_costs(layer)`.
    pub preferred_direction_costs: Vec<f64>,
}

/// What kind of keepout or pour an area is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaKind {
    /// `ObstacleArea`: a keepout for traces and vias.
    Keepout,
    /// `ViaObstacleArea`: a keepout for vias only.
    ViaKeepout,
    /// `ComponentObstacleArea`: a keepout for component placement only.
    ComponentKeepout,
    /// `ConductionArea`: copper, a pour or plane.
    Conduction { is_obstacle: bool },
}

/// A direction a trace may leave a pin in, and how far from the centre
/// the pad's edge lies that way. `Pin.TraceExitRestriction`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExitRestriction {
    pub direction: Direction,
    pub min_length: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ItemKind {
    /// A component pin: one pad per layer of its stack, `None` where the
    /// stack has no shape on that layer; per layer too, the half width a
    /// trace may neck down to at it, the widest extent of its pad
    /// (`Pin.get_max_width`), and the directions a trace may leave it in
    /// (none: any).
    Pin { center: IntPoint, pads: Vec<Option<PadShape>>, neckdown: Vec<i64>, max_width: Vec<f64>, exits: Vec<Vec<ExitRestriction>> },
    /// A via: its stack's pads moved to its centre, one per layer of the
    /// stack; whether it may overlap SMD pins of its net.
    Via { center: IntPoint, padstack: usize, pads: Vec<Option<PadShape>>, attach_allowed: bool },
    Trace { layer: i32, half_width: i64, polyline: Polyline },
    Area { kind: AreaKind, layer: i32, shape: AreaShape },
    /// The board outline; with its keepout outside when that was generated.
    Outline { half_width: i64, shapes: Vec<Vec<Line>>, keepout_outside: bool },
    /// A component's outline, which enters no search tree.
    ComponentOutline,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// FreeRouting's item number.
    pub id: u32,
    pub kind: ItemKind,
    pub first_layer: i32,
    pub last_layer: i32,
    pub clearance_class: i32,
    pub fixed: FixedState,
    /// The component it belongs to, 0 for none.
    pub component: i32,
    pub nets: Vec<i32>,
}

impl Item {
    /// `Item.contains_net`: never for a net number of 0 or less.
    pub fn contains_net(&self, net: i32) -> bool {
        net > 0 && self.nets.contains(&net)
    }

    /// `Item.shares_net_no`.
    pub fn shares_net_no(&self, nets: &[i32]) -> bool {
        self.nets.iter().any(|n| nets.contains(n))
    }

    /// `Item.shares_net`.
    pub fn shares_net(&self, other: &Item) -> bool {
        self.shares_net_no(&other.nets)
    }

    /// Whether a trace of `net` must keep clear of this item.
    /// `is_trace_obstacle`, per kind.
    pub fn is_trace_obstacle(&self, net: i32) -> bool {
        match &self.kind {
            ItemKind::Area { kind: AreaKind::ViaKeepout | AreaKind::ComponentKeepout, .. } => false,
            ItemKind::Area { kind: AreaKind::Conduction { is_obstacle }, .. } => *is_obstacle && !self.contains_net(net),
            _ => !self.contains_net(net),
        }
    }

    /// `Item.is_obstacle(int)`: items of other nets.
    pub fn is_obstacle(&self, net: i32) -> bool {
        !self.contains_net(net)
    }

    /// An unfixed trace or via with a net, which the autorouter may rip up
    /// and reroute. `is_routable`.
    pub fn is_routable(&self) -> bool {
        matches!(self.kind, ItemKind::Trace { .. } | ItemKind::Via { .. }) && !self.is_user_fixed() && !self.nets.is_empty()
    }

    /// Whether a via of `net` may go through it. `is_drillable`, per kind.
    pub fn is_drillable(&self, net: i32) -> bool {
        match &self.kind {
            ItemKind::Trace { .. } => self.contains_net(net),
            ItemKind::Area { kind: AreaKind::Conduction { is_obstacle }, .. } => !is_obstacle || self.contains_net(net),
            _ => false,
        }
    }

    /// `Item.is_user_fixed`.
    pub fn is_user_fixed(&self) -> bool {
        self.fixed >= FixedState::UserFixed
    }

    /// Whether the push algorithms may not move it: by its fixed state, or,
    /// for a trace, by its net's class. `is_shove_fixed`, per kind.
    pub fn is_shove_fixed(&self, rules: &Rules) -> bool {
        if self.fixed >= FixedState::ShoveFixed {
            return true;
        }
        if let ItemKind::Trace { .. } = self.kind {
            // Nets.is_normal_net_no: a real net, not the null net.
            return self.nets.iter().any(|&n| n > 0 && rules.net_class(n).is_some_and(|c| c.shove_fixed));
        }
        false
    }

    /// A pin a via may go through: one on a single layer.
    /// `Pin.drill_allowed`.
    pub fn drill_allowed(&self) -> bool {
        matches!(self.kind, ItemKind::Pin { .. }) && self.first_layer == self.last_layer
    }

    /// A pin, via, trace or pour -- something a trace can connect to -- with
    /// a net. `Item.is_connectable`.
    pub fn is_connectable(&self) -> bool {
        self.is_connectable_kind() && !self.nets.is_empty()
    }

    /// FreeRouting's `Connectable` items.
    pub fn is_connectable_kind(&self) -> bool {
        matches!(
            self.kind,
            ItemKind::Pin { .. } | ItemKind::Via { .. } | ItemKind::Trace { .. } | ItemKind::Area { kind: AreaKind::Conduction { .. }, .. }
        )
    }

    /// The layer shape `index` lies on. `shape_layer`, per kind; the
    /// outline's shapes are its edges, repeated layer by layer.
    pub fn shape_layer(&self, index: usize, layer_count: usize, shape_count: usize) -> i32 {
        match &self.kind {
            ItemKind::Pin { .. } | ItemKind::Via { .. } => {
                let index = (index as i32).min(self.last_layer - self.first_layer);
                self.first_layer + index
            }
            ItemKind::Trace { layer, .. } | ItemKind::Area { layer, .. } => *layer,
            ItemKind::Outline { .. } => {
                if shape_count == 0 {
                    return 0;
                }
                let r = (index * layer_count / shape_count) as i32;
                r.clamp(0, layer_count as i32 - 1)
            }
            ItemKind::ComponentOutline => self.first_layer,
        }
    }

    /// The point a pin or via connects at. `DrillItem.get_center`.
    pub fn center(&self) -> Option<IntPoint> {
        match &self.kind {
            ItemKind::Pin { center, .. } | ItemKind::Via { center, .. } => Some(*center),
            _ => None,
        }
    }
}

/// The board as the router reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    /// `RoutingBoard.bounding_box`.
    pub bounds: IntBox,
    pub layers: Vec<Layer>,
    pub rules: Rules,
    pub settings: AutorouteSettings,
    /// In the board's order.
    pub items: Vec<Item>,
    /// Whether the board came from a CAD system, which narrows area tree
    /// shapes (see [`crate::board::area_section_width`]).
    pub host_cad: bool,
    /// The widest an area's tree shape may be, as the Java worked it out.
    pub area_section: f64,
    /// The last item number drawn, `ItemIdNoGenerator.max_generated_no`.
    pub id_max: u32,
}

impl Board {
    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    pub fn item(&self, id: u32) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }
}
