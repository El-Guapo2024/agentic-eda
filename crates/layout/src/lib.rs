//! eda-layout: a Sugiyama-style layered schematic layout core.
//!
//! Generic over a small internal graph ([`graph::LayoutGraph`]) — nodes with
//! size + ports, directed edges between ports. Not wired to
//! `eda-model::ConstraintModel` yet; that mapping (parts/nets -> nodes/edges,
//! cluster grouping) is future work, see the crate-level docs in the repo
//! task notes.
//!
//! Pipeline: cycle breaking (`cycle`) -> layering (`layering`) -> crossing
//! minimization (`crossing`, over a dummy-node-augmented graph from
//! `dummy`) -> coordinate assignment (`coords`) -> orthogonal routing
//! (`routing`). Everything is deterministic: no HashMap iteration order is
//! ever used for anything that affects output; BTreeMap/sorted Vec only.

pub mod coords;
pub mod crossing;
pub mod cycle;
pub mod dummy;
pub mod graph;
pub mod layering;
pub mod routing;
pub mod maze;

pub use graph::{EdgeEndpoint, LayoutGraph, Node, Point, Port, Side};

use std::collections::BTreeMap;

/// KiCad half-grid: 1.27mm in micrometers.
pub const DEFAULT_GRID: i64 = 1_270;

#[derive(Debug, Clone)]
pub struct LayoutOptions {
    /// All output coordinates snap to a multiple of this (um).
    pub grid: i64,
    /// Nominal distance between layer baselines (um); the effective spacing
    /// used is at least this, and at least tall-enough to avoid vertical
    /// overlap given the actual node heights in the graph.
    pub layer_spacing: i64,
    /// Minimum gap between adjacent node boxes within a layer (um).
    pub node_spacing: i64,
    /// Barycenter sweep count for crossing minimization.
    pub crossing_sweeps: usize,
    /// Height (um) of the reserved refdes text slot immediately *above*
    /// every symbol box on screen, and of the value text slot immediately
    /// *below* it. The router treats box+slots as one solid obstacle, so
    /// no other net's wire is ever routed where a label will be drawn.
    /// (Screen "above"/"below" is the within-layer packing axis; see the
    /// `layout` doc comment for the transpose.) `eda-engine` sets these
    /// from `geometry::SLOT_ABOVE_UM`/`SLOT_BELOW_UM`, the same constants
    /// `eda-render` and `eda-gates` place the text with.
    pub label_slot_above: i64,
    pub label_slot_below: i64,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            grid: DEFAULT_GRID,
            layer_spacing: 10 * DEFAULT_GRID, // 12.7mm
            node_spacing: 3 * DEFAULT_GRID,   // 3.81mm
            crossing_sweeps: 4,
            label_slot_above: 0,
            label_slot_below: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LayoutResult {
    /// Grid-snapped top-left corner per node, keyed by NodeId for
    /// determinism (no HashMap).
    pub positions: BTreeMap<graph::NodeId, Point>,
    /// One orthogonal polyline per edge, parallel to `LayoutGraph::edges`.
    pub edge_polylines: Vec<Vec<Point>>,
}

/// Horizontal-flow boundary transpose (workstream 1).
///
/// The internal Sugiyama pipeline (`cycle` -> `layering` -> `dummy` ->
/// `crossing` -> `coords` -> `routing`) is left completely untouched: it
/// always stacks layers as rows along its own y axis and orders nodes
/// within a layer along its own x axis, using `Node::width` for
/// within-layer packing and the tallest `Node::height` for layer-gap
/// sizing, with `Side::Bottom`/`Side::Top` treated as the "points into the
/// inter-layer channel" sides in `routing::endpoint_approach`.
///
/// We want the opposite on screen: layers as *columns* (x) so signal flow
/// reads left-to-right, with `Side::Left`/`Side::Right` (where
/// `geometry::build_ports` already puts the signal-in/signal-out split)
/// as the inter-layer sides, while `Side::Top`/`Side::Bottom` (power/
/// ground) stay perpendicular to flow regardless.
///
/// Rather than reworking the axis conventions through five files, we
/// reflect across the diagonal at the boundary: build a transposed copy of
/// the input graph (`width`<->`height` swapped per node, `Top`<->`Left`
/// and `Bottom`<->`Right` swapped per port, offsets unchanged — this is
/// exactly the point reflection `(x, y) -> (y, x)` applied to every port
/// location and stub direction, so it's self-inverse), run the untouched
/// pipeline on that, then reflect every output point (node top-lefts, edge
/// polyline points) back with the same `(x, y) -> (y, x)` swap. The result:
/// what the algorithm laid out as rows-of-power/ground-flow becomes
/// columns of left-to-right signal flow, with power pins still on the
/// physical north side and ground still south (their `Side` labels were
/// never touched — only the synthetic graph handed to the solver was
/// reflected, and reflecting twice is the identity on which physical side
/// a `Port`'s `Side` enum refers to, since callers always read `Side` off
/// their own original, untransposed `Node`/`Part` definitions, not off
/// this internal copy).
pub fn layout(g: &LayoutGraph, opts: &LayoutOptions) -> LayoutResult {
    if g.nodes.is_empty() {
        return LayoutResult { positions: BTreeMap::new(), edge_polylines: Vec::new() };
    }

    let gt = transpose_graph(g);

    let reversed = cycle::break_cycles(&gt);
    let node_layer = layering::assign_layers(&gt, &reversed);
    // A layer with more than `MAX_NODES_PER_LAYER` real nodes reads as one
    // giant column in the rendered schematic (`schematic_column_overflow`);
    // split it into several narrower, side-by-side sub-columns instead —
    // safe to do purely by renumbering layers, since no edge ever connects
    // two nodes in the same layer (see `split_overflowing_layers`'s doc).
    let base_layer = layering::split_overflowing_layers(&gt, &reversed, &node_layer, layering::MAX_NODES_PER_LAYER);
    // Sheet shape: layers become screen *columns* and a layer's nodes stack
    // along screen y, so a graph with few layers but many nodes per layer
    // draws as a tall ribbon that no sheet fits (`schematic_sheet_aspect`).
    // When the node-count imbalance is worse than the gate's own limit, try
    // progressively smaller sub-column caps and keep the most balanced
    // split; a graph that is already in shape is left exactly as it was.
    let node_layer = rebalance_layers(&gt, &reversed, &node_layer, base_layer);
    let ext = dummy::build(&gt, &node_layer);
    let order = crossing::minimize_crossings(&ext, opts.crossing_sweeps, &gt, opts);
    let channel_demand = dummy::channel_demand(&ext, &gt);
    let coords = coords::assign_coords(&gt, &ext, &order, opts, &channel_demand);

    if std::env::var_os("EDA_LAYOUT_DEBUG").is_some() {
        for (l, layer) in order.iter().enumerate() {
            let row: Vec<String> = layer.iter().map(|&id| format!("{}{}@{}w{}", if ext.ext_nodes[id].real_node.is_some() { "N" } else { "d" }, ext.ext_nodes[id].real_node.map(|r| r.to_string()).unwrap_or_default(), coords.center_x[id], ext.ext_nodes[id].width)).collect();
            eprintln!("layer {l} y={} : {}", coords.layer_y[l], row.join("  "));
        }
        let mut dup = std::collections::HashMap::new();
        for layer in order.iter() { for &id in layer { *dup.entry(id).or_insert(0) += 1; } }
        let total: usize = order.iter().map(|l| l.len()).sum();
        eprintln!("order: {} entries, {} ext nodes, dups={}", total, ext.ext_nodes.len(), dup.values().filter(|&&c| c > 1).count());
    }
    let real_ext = coords::real_ext_lookup(&ext);
    let mut node_top_left_t = vec![Point { x: 0, y: 0 }; gt.node_count()];
    for (node_id, &ext_id) in &real_ext {
        let node = &gt.nodes[*node_id];
        let layer = ext.ext_nodes[ext_id].layer;
        let cx = coords.center_x[ext_id];
        let left = cx - node.width / 2;
        let top = coords.layer_y[layer];
        node_top_left_t[*node_id] = Point { x: snap(left, opts.grid), y: top };
    }

    // Box + reserved-label-slot obstacles, in transposed space: the slots
    // extend along the transposed x axis (= screen y), so a node's solid
    // extent grows by `label_slot_above` on the low side and
    // `label_slot_below` on the high side of transposed x.
    let slot_boxes = slot_obstacles(&gt, &node_top_left_t, opts);
    let mut edge_polylines_t = routing::route_edges(&gt, &ext, &coords, &node_top_left_t, opts.grid, &channel_demand, &slot_boxes);
    // Wire-aware repair: re-route any edge left collinear on another
    // net's wire (see `maze`).
    maze::repair(&gt, &node_top_left_t, &mut edge_polylines_t, opts.grid, &slot_boxes);

    let mut positions = BTreeMap::new();
    for (id, p) in node_top_left_t.into_iter().enumerate() {
        positions.insert(id, swap_xy(p));
    }

    let edge_polylines = edge_polylines_t.into_iter().map(|poly| poly.into_iter().map(swap_xy).collect()).collect();

    LayoutResult { positions, edge_polylines }
}

/// Per-real-node solid rectangle = symbol box grown by its reserved refdes
/// and value label slots, in transposed coordinates. Returned as
/// `(node_id, top_left, width, height)`, the shape `routing`/`maze` use.
pub(crate) fn slot_obstacles(gt: &LayoutGraph, node_top_left_t: &[Point], opts: &LayoutOptions) -> Vec<(usize, Point, i64, i64)> {
    let (above, below) = (opts.label_slot_above.max(0), opts.label_slot_below.max(0));
    gt.nodes
        .iter()
        .enumerate()
        .map(|(id, n)| {
            let tl = node_top_left_t[id];
            (id, Point { x: tl.x - above, y: tl.y }, n.width + above + below, n.height)
        })
        .collect()
}

/// Ratio between the widest layer's node count and the number of layers —
/// a cheap proxy for the drawn sheet's height/width ratio, since layers are
/// screen columns and a layer's nodes stack along screen y.
fn layer_imbalance(layer: &[usize]) -> f64 {
    if layer.is_empty() {
        return 1.0;
    }
    let layers = layer.iter().max().copied().unwrap_or(0) + 1;
    let mut counts = vec![0usize; layers];
    for &l in layer {
        counts[l] += 1;
    }
    let widest = counts.iter().copied().max().unwrap_or(1).max(1) as f64;
    let l = layers as f64;
    if widest >= l {
        widest / l
    } else {
        l / widest
    }
}

/// See the call site in [`layout`]. Returns `base` unchanged unless a
/// smaller sub-column cap yields a strictly more balanced layering.
fn rebalance_layers(gt: &LayoutGraph, reversed: &[bool], original: &[usize], base: Vec<usize>) -> Vec<usize> {
    const LIMIT: f64 = 2.5;
    let mut best_score = layer_imbalance(&base);
    if best_score <= LIMIT {
        return base;
    }
    let mut best = base;
    for cap in (2..layering::MAX_NODES_PER_LAYER).rev() {
        let cand = layering::split_overflowing_layers(gt, reversed, original, cap);
        let score = layer_imbalance(&cand);
        if score < best_score {
            best_score = score;
            best = cand;
        }
    }
    best
}

fn swap_xy(p: Point) -> Point {
    Point { x: p.y, y: p.x }
}

fn transpose_side(s: Side) -> Side {
    match s {
        Side::Top => Side::Left,
        Side::Bottom => Side::Right,
        Side::Left => Side::Top,
        Side::Right => Side::Bottom,
    }
}

/// Builds the reflected copy of `g` fed to the (untouched) internal
/// pipeline — see the `layout` doc comment for why this transpose is
/// correct and self-inverse.
fn transpose_graph(g: &LayoutGraph) -> LayoutGraph {
    let nodes = g
        .nodes
        .iter()
        .map(|n| {
            let ports = n.ports.iter().map(|p| graph::Port { side: transpose_side(p.side), offset: p.offset }).collect();
            graph::Node { id: n.id, width: n.height, height: n.width, ports }
        })
        .collect();
    LayoutGraph { nodes, edges: g.edges.clone() }
}

fn snap(v: i64, grid: i64) -> i64 {
    if grid <= 0 {
        return v;
    }
    let half = grid / 2;
    let q = if v >= 0 { (v + half).div_euclid(grid) } else { -((-v + half).div_euclid(grid)) };
    q * grid
}
