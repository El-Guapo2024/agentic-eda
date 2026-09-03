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
}

impl Default for LayoutOptions {
    fn default() -> Self {
        Self {
            grid: DEFAULT_GRID,
            layer_spacing: 10 * DEFAULT_GRID, // 12.7mm
            node_spacing: 3 * DEFAULT_GRID,   // 3.81mm
            crossing_sweeps: 4,
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
    let ext = dummy::build(&gt, &node_layer);
    let order = crossing::minimize_crossings(&ext, opts.crossing_sweeps);
    let channel_demand = dummy::channel_demand(&ext);
    let coords = coords::assign_coords(&gt, &ext, &order, opts, &channel_demand);

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

    let edge_polylines_t = routing::route_edges(&gt, &ext, &coords, &node_top_left_t, opts.grid, &channel_demand);

    let mut positions = BTreeMap::new();
    for (id, p) in node_top_left_t.into_iter().enumerate() {
        positions.insert(id, swap_xy(p));
    }

    let edge_polylines = edge_polylines_t.into_iter().map(|poly| poly.into_iter().map(swap_xy).collect()).collect();

    LayoutResult { positions, edge_polylines }
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
