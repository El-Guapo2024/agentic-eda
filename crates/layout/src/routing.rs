//! Phase 5: obstacle-aware orthogonal channel routing between layers.
//!
//! Each edge's chain (real port -> dummy bend points -> real port) is
//! rendered as a sequence of H/V-only segments, snapped to the grid, that:
//! - start/end at the pin's actual stub tip ([`crate::graph::Node::stub_tip`]),
//!   not the box boundary;
//! - never cross the interior of any symbol box.
//!
//! Strategy (deliberately simple, not a general router): the vertical space
//! between two adjacent layers is a "channel" guaranteed clear of every box
//! in either layer (see `coords::assign_coords`'s `effective_spacing`,
//! built from the *tallest* node in the whole graph plus `node_spacing`).
//! For a single layer-to-layer hop we pick one horizontal track inside that
//! channel (staggered per edge index to spread edges apart) and connect
//! each endpoint to it:
//! - a port whose stub already points into the channel (Bottom-side for the
//!   low-layer end, Top-side for the high-layer end, or any Left/Right-side
//!   port, which stays clear of its own box in x for the whole vertical
//!   run) just runs straight from its stub tip to the track;
//! - a port whose stub points the "wrong" way (Top-side needing the channel
//!   below it, or Bottom-side needing the channel above it) first sidesteps
//!   past its own box's left edge (by one grid step, always less than
//!   `node_spacing` so it can never reach a neighboring box), then runs
//!   vertically past the box to the track.
//!
//! Dummy bend points (zero-size, and always placed with at least
//! `node_spacing` clearance from every real box in their layer, per
//! `coords::assign_coords`'s overlap resolution) need no escape at all:
//! each hop just uses the dummy's `center_x` directly as the track's x on
//! that side, so multi-layer edges become a sequence of clean
//! channel-to-channel vertical passes through the dummy's row.
//!
//! Row assignment: each inter-layer channel's height is sized (in
//! `coords::assign_coords`, via `dummy::channel_demand`) to fit one
//! distinct horizontal track row per hop routed through it, and each hop
//! is assigned its row deterministically as that edge's position in the
//! channel's sorted edge-index list — so two hops through the same channel
//! never land on the same row, and different-net wires never run
//! collinear-overlapping through a channel. Known limitation: row demand
//! only counts hops that actually cross that particular layer gap (each
//! chain contributes at most one hop per gap); it does not otherwise
//! special-case a dummy bend point that merely passes through a channel's
//! row without terminating there — it still consumes exactly one row like
//! any other hop, since the channel-sizing math is per-hop, not
//! per-terminating-endpoint.

use crate::coords::Coords;
use crate::dummy::ExtGraph;
use crate::graph::{LayoutGraph, Point, Side, STUB_LEN};

fn snap(v: i64, grid: i64) -> i64 {
    if grid <= 0 {
        return v;
    }
    let half = grid / 2;
    let q = if v >= 0 { (v + half).div_euclid(grid) } else { -((-v + half).div_euclid(grid)) };
    q * grid
}

/// Picks a deterministic horizontal track inside `[channel_lo, channel_hi]`
/// for `edge_index`'s hop through this channel: `row` is that edge's
/// position in `dummy::channel_demand()`'s sorted, deduped list of edges
/// using this channel, so every distinct hop in the channel gets a
/// genuinely distinct row (no modulo wrap / reuse) as long as
/// `coords::assign_coords` sized the channel to fit `row_count` rows —
/// which it does (see its per-gap `demand_spacing`).
fn pick_track_y(channel_lo: i64, channel_hi: i64, row: usize, grid: i64) -> i64 {
    let grid = grid.max(1);
    let y = channel_lo + (row as i64) * grid;
    snap(y, grid).min(channel_hi.max(channel_lo)).max(channel_lo)
}

fn push_dedup(poly: &mut Vec<Point>, p: Point) {
    if poly.last() != Some(&p) {
        poly.push(p);
    }
}

/// One endpoint's path from its "far" point (stub tip, or its own point for
/// a dummy) to a point at `track_y` on the shared channel track — in that
/// order (far -> channel).
#[allow(clippy::too_many_arguments)]
fn endpoint_approach(
    g: &LayoutGraph,
    ext: &ExtGraph,
    coords: &Coords,
    ext_id: usize,
    port_hint: Option<usize>,
    node_top_left: &[Point],
    dir_down: bool,
    track_y: i64,
    grid: i64,
    row: usize,
) -> Vec<Point> {
    let en = &ext.ext_nodes[ext_id];
    match en.real_node {
        None => {
            // Dummy: zero size, always clear of every box in its own layer
            // by at least node_spacing (see coords::assign_coords), so a
            // straight vertical pass through its row is always safe.
            vec![Point { x: coords.center_x[ext_id], y: track_y }]
        }
        Some(node_id) => {
            let node = &g.nodes[node_id];
            let port_idx = port_hint.unwrap_or(0);
            let top_left = node_top_left[node_id];
            let stub = node.stub_tip(top_left, port_idx);
            let side = node.ports[port_idx].side;
            // Aligned: the stub already points toward the target channel,
            // so a single vertical run at the stub's own x gets there
            // without crossing anything (Left/Right stubs are always at
            // least STUB_LEN clear of their own box in x, and further than
            // that from any neighbor, since node_spacing > 2*STUB_LEN).
            let aligned = match side {
                Side::Bottom => dir_down,
                Side::Top => !dir_down,
                Side::Left | Side::Right => true,
            };
            if aligned {
                // Same-side lane offset: a Left/Right stub's constant
                // coordinate is the box edge, the same for *every* port on
                // that side (unlike Top/Bottom, whose offset already lives
                // in the varying coordinate) — so if this node has more
                // than one port on `side`, two of them heading into
                // different channels would otherwise hug the identical
                // line and collide collinearly (this is exactly what the
                // >=3-pin power/ground threshold change exposed on the LDO
                // fixture: U1's two North power pins, VIN and VOUT, both
                // now get real wires into different channels). Nudge the
                // box-hugging leg out by this port's rank among same-side
                // ports; nodes with only one port per side (the common
                // case, and every synthetic test graph in this crate) are
                // untouched, so this can't newly collide two *different*
                // nodes' escape lines the way a node-independent lane
                // would. The two hops still meet cleanly at the shared
                // horizontal track (`route_edges` bridges any x gap
                // between a chain's two endpoint approaches there), so
                // connectivity is unaffected.
                let same_side_rank = node.ports.iter().take(port_idx).filter(|p| p.side == side).count();
                let same_side_count = node.ports.iter().filter(|p| p.side == side).count();
                if same_side_count <= 1 || !matches!(side, Side::Left | Side::Right) {
                    vec![stub, Point { x: stub.x, y: track_y }]
                } else {
                    let lane = grid.max(1) * (same_side_rank as i64 + 1);
                    let exit_x = if side == Side::Left { stub.x - lane } else { stub.x + lane };
                    vec![stub, Point { x: exit_x, y: stub.y }, Point { x: exit_x, y: track_y }]
                }
            } else {
                // Misaligned (Top-side port needing the channel below, or
                // Bottom-side needing the channel above): sidestep past the
                // box's left edge, then run vertically past the box to the
                // channel. The sidestep distance is `(row + 1)` grid steps
                // — `row` being this edge's unique row index within the
                // channel (see `route_edges`) — so two different edges that
                // share a box (e.g. two nets both landing on the same part)
                // get distinct exit lines and never run collinear on top of
                // each other, the same way distinct channel rows keep their
                // horizontal spans apart.
                let margin = grid.max(1) * (row as i64 + 1);
                let exit_x = top_left.x - margin;
                vec![stub, Point { x: exit_x, y: stub.y }, Point { x: exit_x, y: track_y }]
            }
        }
    }
}

/// Path between two ports on the *same* box (a star net whose hub and leaf
/// are both pins of one part): walks the box's perimeter, expanded by one
/// stub length on every side, from one stub tip to the other. Every segment
/// runs either along one expanded-rim line or between two adjacent
/// corners of that expanded rectangle, so it can never cross the box
/// interior regardless of which two sides the ports sit on.
fn same_node_path(node: &crate::graph::Node, top_left: Point, port_a: usize, port_b: usize) -> Vec<Point> {
    let a_stub = node.stub_tip(top_left, port_a);
    let b_stub = node.stub_tip(top_left, port_b);
    let side_a = node.ports[port_a].side;
    let side_b = node.ports[port_b].side;

    let margin = STUB_LEN;
    let (left, right) = (top_left.x - margin, top_left.x + node.width + margin);
    let (top, bottom) = (top_left.y - margin, top_left.y + node.height + margin);
    let tl = Point { x: left, y: top };
    let tr = Point { x: right, y: top };
    let br = Point { x: right, y: bottom };
    let bl = Point { x: left, y: bottom };
    // Clockwise order Top,Right,Bottom,Left; corner reached leaving each side.
    let corner_after = [tr, br, bl, tl];
    let side_idx = |s: Side| match s {
        Side::Top => 0,
        Side::Right => 1,
        Side::Bottom => 2,
        Side::Left => 3,
    };
    let rim = |s: Side, p: Point| -> Point {
        match s {
            Side::Top => Point { x: p.x, y: top },
            Side::Bottom => Point { x: p.x, y: bottom },
            Side::Left => Point { x: left, y: p.y },
            Side::Right => Point { x: right, y: p.y },
        }
    };

    let mut path = vec![a_stub, rim(side_a, a_stub)];
    let mut idx = side_idx(side_a);
    let idx_b = side_idx(side_b);
    while idx != idx_b {
        path.push(corner_after[idx]);
        idx = (idx + 1) % 4;
    }
    path.push(rim(side_b, b_stub));
    path.push(b_stub);
    path
}

fn ext_layer_of_real(ext: &ExtGraph, node_id: usize) -> Option<usize> {
    ext.ext_nodes.iter().find(|n| n.real_node == Some(node_id)).map(|n| n.layer)
}

/// True if the axis-aligned segment `a`->`b` passes through the *interior*
/// of the box `[top_left, top_left+(w,h)]` (touching an edge is fine — a
/// stub tip legitimately sits on the box's own boundary line extended).
/// Diagonal segments never occur in this router's output, but if one ever
/// did we'd rather over-block (return true) than silently draw through a
/// box.
fn seg_intersects_box(a: Point, b: Point, top_left: Point, w: i64, h: i64) -> bool {
    let (x0, x1) = (top_left.x, top_left.x + w);
    let (y0, y1) = (top_left.y, top_left.y + h);
    if a.y == b.y {
        let y = a.y;
        if y <= y0 || y >= y1 {
            return false;
        }
        let (xa, xb) = (a.x.min(b.x), a.x.max(b.x));
        xa < x1 && xb > x0
    } else if a.x == b.x {
        let x = a.x;
        if x <= x0 || x >= x1 {
            return false;
        }
        let (ya, yb) = (a.y.min(b.y), a.y.max(b.y));
        ya < y1 && yb > y0
    } else {
        true
    }
}

/// A path is clear if none of its segments cross the interior of *any* box
/// — including the two endpoint nodes' own boxes: a stub tip sits exactly
/// on its own box's boundary (extended outward), so `seg_intersects_box`
/// never flags the segment leaving it in the correct direction, but an
/// L-bend chosen the "wrong" way can still curl back through that same
/// box, which must be rejected just as readily as cutting through anyone
/// else's.
fn path_clear(pts: &[Point], boxes: &[(usize, Point, i64, i64)]) -> bool {
    for w in pts.windows(2) {
        for &(_, tl, bw, bh) in boxes {
            if seg_intersects_box(w[0], w[1], tl, bw, bh) {
                return false;
            }
        }
    }
    true
}

/// Workstream 4: when a chain is a single direct hop between two *real*
/// nodes (no dummy bend points), try a short, obstacle-free orthogonal
/// path between their stub tips before falling back to the general
/// channel-row machinery — straight line first, then each of the two
/// possible single-bend L-shapes, in that fixed order (deterministic, no
/// row/track staggering needed since there's nothing to stagger against).
/// Returns `None` when no such path (at most 3 segments — a straight line
/// or one L bend needs at most 2) is obstacle-free, so the caller can fall
/// back to the channel router.
fn try_direct_path(a: Point, b: Point, boxes: &[(usize, Point, i64, i64)]) -> Option<Vec<Point>> {
    let candidates: [Vec<Point>; 3] =
        [vec![a, b], vec![a, Point { x: a.x, y: b.y }, b], vec![a, Point { x: b.x, y: a.y }, b]];
    for cand in candidates {
        // Drop a degenerate middle point (already-aligned case duplicating
        // an endpoint) before the clearance check.
        let mut pts = Vec::with_capacity(cand.len());
        for p in cand {
            if pts.last() != Some(&p) {
                pts.push(p);
            }
        }
        if pts.len() < 2 {
            continue;
        }
        // Every segment in a candidate must be axis-aligned.
        if !pts.windows(2).all(|w| w[0].x == w[1].x || w[0].y == w[1].y) {
            continue;
        }
        if path_clear(&pts, boxes) {
            return Some(pts);
        }
    }
    None
}

/// Drops redundant collinear intermediate points (A-B-C where B lies
/// exactly on the segment A-C) from an already-orthogonal polyline.
fn drop_redundant_collinear(poly: Vec<Point>) -> Vec<Point> {
    if poly.len() < 3 {
        return poly;
    }
    let mut out: Vec<Point> = Vec::with_capacity(poly.len());
    out.push(poly[0]);
    for i in 1..poly.len() - 1 {
        let (a, b, c) = (out[out.len() - 1], poly[i], poly[i + 1]);
        let collinear_h = a.y == b.y && b.y == c.y && (b.x - a.x).signum() == (c.x - b.x).signum();
        let collinear_v = a.x == b.x && b.x == c.x && (b.y - a.y).signum() == (c.y - b.y).signum();
        // A zero-length leading run (b == a) is also redundant.
        if b == a || ((collinear_h || collinear_v) && (b.x - a.x, b.y - a.y) != (0, 0)) {
            continue;
        }
        out.push(b);
    }
    let last = poly[poly.len() - 1];
    if out.last() != Some(&last) {
        out.push(last);
    }
    out
}

/// Builds one orthogonal, box-avoiding polyline per edge, in `g.edges` order.
///
/// `channel_demand` (from `dummy::channel_demand`, one entry per inter-layer
/// channel, each a sorted/deduped list of the edge indices using it) is
/// used to look up each hop's unique row index within its channel.
pub fn route_edges(
    g: &LayoutGraph,
    ext: &ExtGraph,
    coords: &Coords,
    node_top_left: &[Point],
    grid: i64,
    channel_demand: &[Vec<usize>],
) -> Vec<Vec<Point>> {
    let mut result: Vec<Vec<Point>> = vec![Vec::new(); g.edges.len()];
    let tallest = g.nodes.iter().map(|n| n.height).max().unwrap_or(0);
    let boxes: Vec<(usize, Point, i64, i64)> =
        g.nodes.iter().enumerate().map(|(id, n)| (id, node_top_left[id], n.width, n.height)).collect();

    for chain in &ext.chains {
        let edge = &g.edges[chain.edge_index];

        if edge.from.node == edge.to.node {
            // Both endpoints are pins of the same box (e.g. a star net hub
            // and leaf on one part): no inter-layer channel involved at
            // all, just walk the box's perimeter.
            let top_left = node_top_left[edge.from.node];
            result[chain.edge_index] = same_node_path(&g.nodes[edge.from.node], top_left, edge.from.port, edge.to.port);
            continue;
        }

        // Resolve which original endpoint (from/to) maps to the low end of
        // the chain, so we grab the right port index.
        let from_layer = ext_layer_of_real(ext, edge.from.node).unwrap_or(0);
        let to_layer = ext_layer_of_real(ext, edge.to.node).unwrap_or(0);
        let (low_port, high_port) = if from_layer <= to_layer { (edge.from.port, edge.to.port) } else { (edge.to.port, edge.from.port) };

        let n = chain.nodes.len();
        if n < 2 {
            // Degenerate (self-loop-ish) chain: nothing to route between.
            let ext_id = chain.nodes[0];
            let p = match ext.ext_nodes[ext_id].real_node {
                Some(node_id) => g.nodes[node_id].stub_tip(node_top_left[node_id], low_port),
                None => Point { x: coords.center_x[ext_id], y: coords.layer_y[ext.ext_nodes[ext_id].layer] },
            };
            result[chain.edge_index] = vec![p];
            continue;
        }

        // Direct-path shortcut (workstream 4): a single real-to-real hop
        // whose endpoints are already reachable by a short (<=2-segment)
        // obstacle-free orthogonal path skips the channel-row machinery
        // entirely — most useful for 2-pin nets (including the short
        // power/ground drops workstream 2 asks for) where the full
        // per-edge channel detour is needless.
        if n == 2 {
            if let (Some(a_node), Some(b_node)) = (ext.ext_nodes[chain.nodes[0]].real_node, ext.ext_nodes[chain.nodes[1]].real_node) {
                let a_stub = g.nodes[a_node].stub_tip(node_top_left[a_node], low_port);
                let b_stub = g.nodes[b_node].stub_tip(node_top_left[b_node], high_port);
                if let Some(direct) = try_direct_path(a_stub, b_stub, &boxes) {
                    result[chain.edge_index] = direct;
                    continue;
                }
            }
        }

        let mut poly: Vec<Point> = Vec::new();
        for i in 0..n - 1 {
            let a_id = chain.nodes[i];
            let b_id = chain.nodes[i + 1];
            let a_layer = ext.ext_nodes[a_id].layer;
            let b_layer = ext.ext_nodes[b_id].layer;
            debug_assert_eq!(b_layer, a_layer + 1, "chain hops must connect adjacent layers");

            let channel_lo = coords.layer_y[a_layer] + tallest + STUB_LEN;
            let channel_hi = coords.layer_y[b_layer] - STUB_LEN;
            let row = channel_demand
                .get(a_layer)
                .and_then(|rows| rows.binary_search(&chain.edge_index).ok())
                .unwrap_or(0);
            let track_y = pick_track_y(channel_lo, channel_hi, row, grid);

            let a_port = if i == 0 { Some(low_port) } else { None };
            let b_port = if i + 1 == n - 1 { Some(high_port) } else { None };

            let a_pts = endpoint_approach(g, ext, coords, a_id, a_port, node_top_left, true, track_y, grid, row);
            let mut b_pts = endpoint_approach(g, ext, coords, b_id, b_port, node_top_left, false, track_y, grid, row);
            b_pts.reverse();

            for p in a_pts {
                push_dedup(&mut poly, p);
            }
            for p in b_pts {
                push_dedup(&mut poly, p);
            }
        }
        result[chain.edge_index] = poly;
    }

    result.into_iter().map(drop_redundant_collinear).collect()
}
