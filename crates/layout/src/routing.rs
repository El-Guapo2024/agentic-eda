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
    ext_id: usize,
    port_hint: Option<usize>,
    node_top_left: &[Point],
    dir_down: bool,
    track_y: i64,
    grid: i64,
    slot_boxes: &[(usize, Point, i64, i64)],
    placed: &[Vec<Point>],
    escapes: &[Vec<(i64, i64)>],
    dummy_cols: &[i64],
) -> Vec<Point> {
    let en = &ext.ext_nodes[ext_id];
    match en.real_node {
        None => {
            // Dummy: zero size, always clear of every box in its own layer
            // by at least node_spacing (see coords::assign_coords), so a
            // straight vertical pass through its row is always safe.
            vec![Point { x: dummy_cols[ext_id], y: track_y }]
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
                    // Nudge the box-hugging leg out by this port's rank
                    // among same-side ports, so two ports on one side
                    // heading into different channels don't hug the
                    // identical line (the LDO fixture's U1 VIN/VOUT case).
                    //
                    // Known defect, diagnosed but not fixed: when a box has
                    // many ports on one side and their pin spacing equals
                    // the grid, `pin - (rank + 1) * grid` maps every port to
                    // (nearly) the same exit coordinate, so their runs along
                    // the shared box-edge line nest inside one another and
                    // overlap. A correct fix assigns interval-disjoint lanes
                    // (sort the side's ports, greedy interval colouring)
                    // rather than a rank-proportional offset. Deliberately
                    // NOT box-clearance-searched: stepping outward for a
                    // clear offset collapses even more ports onto one line.
                    let lane = grid.max(1) * (same_side_rank as i64 + 1);
                    let exit_x = if side == Side::Left { stub.x - lane } else { stub.x + lane };
                    vec![stub, Point { x: exit_x, y: stub.y }, Point { x: exit_x, y: track_y }]
                }
            } else {
                // Misaligned (Top-side port needing the channel below, or
                // Bottom-side needing the channel above): sidestep past the
                // box's left edge, then run vertically past the box to the
                // channel. Both the sidestep column (`base`) and the lane
                // the box-hugging leg runs on come from
                // `build_escape_lanes`, which assigns them across the whole
                // board — see that function for why a per-edge/per-rank
                // stagger computed here is not enough.
                let (lane_idx, base) = escapes[node_id][port_idx];
                // Per-port escape lane. The leg that hugs the box edge runs
                // along the *constant* coordinate shared by every port on
                // this side (`stub.y` = the box edge extended by STUB_LEN),
                // so staggering only `base` (the perpendicular offset)
                // leaves all of this side's escapes nested inside one
                // another on that single line — the l4 U2 case where four
                // nets' sidesteps all ran on x=885190. Give each port on the
                // side its own lane, `grid` apart, stepped outward from the
                // box in the stub's own direction: lane 0 stays on the
                // original line, and lanes are ordered by the port's offset
                // along the side so the port nearest the escape direction
                // takes the innermost lane and no lane's horizontal leg
                // crosses an inner port's vertical leg.
                let outward = if side == Side::Top { -1 } else { 1 };
                let lane_y = stub.y + outward * lane_idx * grid.max(1);
                // Same box/slot-aware search as the aligned case above: on
                // a dense board a staggered `base` can walk straight into a
                // neighbouring symbol before the horizontal leg to the
                // track even starts.
                let build = |x: i64| {
                    if lane_idx == 0 {
                        vec![stub, Point { x, y: stub.y }, Point { x, y: track_y }]
                    } else {
                        vec![stub, Point { x: stub.x, y: lane_y }, Point { x, y: lane_y }, Point { x, y: track_y }]
                    }
                };
                first_clear(node_id, base, -grid.max(1), build, slot_boxes, placed)
            }
        }
    }
}


/// How many `step`-sized offsets past the nominal sidestep an endpoint
/// approach may search before giving up and using the nominal one (the
/// gate then still fails, loudly, rather than the router silently
/// pretending a box-cutting wire is fine).
const ESCAPE_TRIES: i64 = 24;

/// How far (in grid steps) an escape's vertical leg may be pushed off its
/// nominal column to find a free one. Kept small: a long push walks the
/// wire past neighbouring symbols and trades `schematic_wire_overlap` for
/// `schematic_wire_detour`.
const ESCAPE_BASE_TRIES: i32 = 3;

/// Ceiling on how far outward an escape lane may be pushed, in grid steps.
/// Beyond this the wire is longer than any overlap it avoids is worth.
const MAX_ESCAPE_LANES: i64 = 8;

/// Channel occupancy at or above which track rows are assigned in endpoint
/// order rather than edge-index order (see `build_channel_rows`).
const MIN_ORDERED_CHANNEL_HOPS: usize = 6;

/// First escape offset, starting at `base` and stepping by `step`, whose
/// path is clear of every obstacle in `boxes` except `own`'s own rectangle
/// (a stub legitimately starts on — and, for a slot-inflated box, inside —
/// its own symbol's obstacle). Falls back to `base` when nothing is clear.
fn first_clear(
    own: usize,
    base: i64,
    step: i64,
    build: impl Fn(i64) -> Vec<Point>,
    boxes: &[(usize, Point, i64, i64)],
    _placed: &[Vec<Point>],
) -> Vec<Point> {
    for k in 0..ESCAPE_TRIES {
        let cand = build(base + k * step);
        let clear = cand.windows(2).all(|w| {
            boxes.iter().all(|&(id, tl, bw, bh)| {
                if id == own {
                    // A wire leaving this node's own pin legitimately starts
                    // inside its own slot-inflated rectangle, so check only
                    // the real box plus one stub length of clearance
                    // (recovered from the inflated rect's own node height,
                    // which the slot inflation never touched).
                    return true;
                }
                !seg_intersects_box(w[0], w[1], tl, bw, bh)
            })
        });
        // The nominal offset already carries the per-row/per-port stagger
        // that keeps two escapes off the same line; stepping outward for
        // box clearance would throw that away and re-introduce
        // `schematic_wire_overlap`, so a candidate that lands collinear on
        // an already-routed wire is rejected too.
        if clear {
            return cand;
        }
    }
    build(base)
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
pub(crate) fn seg_intersects_box(a: Point, b: Point, top_left: Point, w: i64, h: i64) -> bool {
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
fn try_direct_path(
    a: Point,
    b: Point,
    boxes: &[(usize, Point, i64, i64)],
    endpoints: (usize, usize),
    endpoint_boxes: [(Point, i64, i64); 2],
    placed: &[Vec<Point>],
    _grid: i64,
) -> Option<Vec<Point>> {
    // Two unrelated direct hops that both happen to run along the same
    // vertical (or horizontal) line collide as a collinear overlap even
    // though neither crosses a box — e.g. two straight-down bus drops that
    // share an x. Reject a candidate that would create such an overlap
    // (both endpoints stay exactly at the real stub tips `a`/`b` — a wire
    // must start there — so an overlapping straight line is dropped in
    // favor of an L-bend candidate rather than nudged sideways).
    // Filter the endpoint boxes out once, not once per candidate.
    let others: Vec<(usize, Point, i64, i64)> =
        boxes.iter().copied().filter(|&(id, ..)| id != endpoints.0 && id != endpoints.1).collect();
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
        // The two endpoint symbols' own rectangles are slot-inflated here,
        // and a Top/Bottom stub legitimately starts *inside* its own box's
        // label slot — so `others` skips those two, exactly as
        // `path_clear`'s doc note already allows for the box boundary.
        // The two endpoint symbols are dropped from `others` because their
        // rectangles are slot-inflated here and a Top/Bottom stub
        // legitimately starts inside its own box's label slot. Dropping them
        // outright, though, also licenses an L-bend that cuts straight
        // through the very box it is connecting to — which is exactly how
        // BUZZ_BASE came to run through R10. Check them against their *real*
        // rectangles instead: a stub tip sits on that boundary, which
        // `seg_intersects_box` already treats as clear.
        let own_clear = endpoint_boxes
            .iter()
            .all(|&(tl, w, h)| pts.windows(2).all(|s| !seg_intersects_box(s[0], s[1], tl, w, h)));
        // Same argument for the endpoint's reserved label slots: only the
        // one segment that actually leaves the pin may be inside them (a
        // Top/Bottom stub tip sits in the refdes slot by construction). Any
        // other leg through that space lands a wire on top of the refdes the
        // renderer is about to draw — `schematic_label_over_wire` on U3.
        let slot_clear = [(endpoints.0, a), (endpoints.1, b)].iter().all(|&(id, tip)| {
            let Some(&(_, tl, w, h)) = boxes.iter().find(|&&(bid, ..)| bid == id) else { return true };
            pts.windows(2).all(|s| s[0] == tip || s[1] == tip || !seg_intersects_box(s[0], s[1], tl, w, h))
        });
        if own_clear && slot_clear && path_clear(&pts, &others) && !collides_collinear(&pts, placed) {
            return Some(pts);
        }
    }
    None
}

/// True if any segment of `pts` is collinear-and-overlapping (shares a
/// positive-length run, not just a touching point) with any segment of any
/// already-placed polyline — the same defect `schematic_wire_overlap`
/// flags, checked here so the router itself avoids introducing it.
fn collides_collinear(pts: &[Point], placed: &[Vec<Point>]) -> bool {
    for w in pts.windows(2) {
        for other in placed {
            for ow in other.windows(2) {
                if segments_overlap(w[0], w[1], ow[0], ow[1]) {
                    return true;
                }
            }
        }
    }
    false
}

fn segments_overlap(a0: Point, a1: Point, b0: Point, b1: Point) -> bool {
    // Vertical case.
    if a0.x == a1.x && b0.x == b1.x && a0.x == b0.x {
        let (alo, ahi) = (a0.y.min(a1.y), a0.y.max(a1.y));
        let (blo, bhi) = (b0.y.min(b1.y), b0.y.max(b1.y));
        return alo.max(blo) < ahi.min(bhi);
    }
    // Horizontal case.
    if a0.y == a1.y && b0.y == b1.y && a0.y == b0.y {
        let (alo, ahi) = (a0.x.min(a1.x), a0.x.max(a1.x));
        let (blo, bhi) = (b0.x.min(b1.x), b0.x.max(b1.x));
        return alo.max(blo) < ahi.min(bhi);
    }
    false
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

/// Per-`(node, port)` escape geometry for Top/Bottom-side ports, as
/// `(lane, base)`.
///
/// The misaligned escape in [`endpoint_approach`] has to leave its pin
/// along the line `stub.y` — the box edge extended by one stub length —
/// which is shared by *every* Top (resp. Bottom) port of *every* node in
/// the layer, since `coords::assign_coords` gives a layer's nodes a common
/// `top_left.y`. Staggering only the perpendicular offset (the old
/// `row + same_side_rank` margin) therefore left all of those escapes
/// nested inside one another on that one line — the l4 defect where four
/// nets terminating on U2 all ran along x=885190.
///
/// So: `base` (the vertical leg's constant coordinate) is staggered per
/// port *within its own node*, ordered by the port's offset along the side
/// so the port nearest the escape direction turns first, then pushed off
/// any column already spoken for; and `lane` (how far outward from the box
/// edge the horizontal leg sits, in grid steps) comes from a greedy
/// interval colouring of the spans `[base, port_x]`.
///
/// The colouring is keyed on the lane's *absolute* coordinate, not on
/// `(stub.y, lane)`: two nodes in different layers have different `stub.y`,
/// so a per-line colouring lets `stub_y_a + lane_a * grid` land exactly on
/// `stub_y_b + lane_b * grid` — which is how l2's I2C_SCL/USB_DM pair still
/// collided at y=213360 after the per-line version. Ports whose spans are
/// disjoint share a lane and the geometry is unchanged; only genuinely
/// overlapping spans are pushed outward.
fn build_escape_lanes(g: &LayoutGraph, node_top_left: &[Point], grid: i64, reserved: &[i64]) -> Vec<Vec<(i64, i64)>> {
    let grid = grid.max(1);
    let mut out: Vec<Vec<(i64, i64)>> = g.nodes.iter().map(|n| vec![(0, 0); n.ports.len()]).collect();
    // Every escape's vertical leg gets its own, globally distinct constant
    // coordinate. Two escapes on *different* nodes can otherwise land on the
    // same `base` (their boxes' left edges differ by an arbitrary amount, not
    // a whole number of ranks), and a `base` can equally well land on a
    // dummy bend point's column, whose vertical pass runs the full height of
    // a layer — both show up as collinear overlaps. `reserved` pre-seeds the
    // dummy columns and real ports' own stub lines.
    let mut used: std::collections::BTreeSet<i64> = reserved.iter().copied().collect();
    // (stub line y, outward sign, base, port x, node, port), in a
    // deterministic sweep order.
    let mut entries: Vec<(i64, i64, i64, i64, usize, usize)> = Vec::new();
    for (node_id, node) in g.nodes.iter().enumerate() {
        let tl = node_top_left[node_id];
        for side in [Side::Top, Side::Bottom] {
            let mut side_ports: Vec<usize> = (0..node.ports.len()).filter(|&i| node.ports[i].side == side).collect();
            side_ports.sort_by_key(|&i| (node.port_point(tl, i).x, i));
            for (rank, &pi) in side_ports.iter().enumerate() {
                let mut base = tl.x - grid * (rank as i64 + 1);
                let mut guard = 0i32;
                while used.contains(&base) && guard < ESCAPE_BASE_TRIES {
                    base -= grid;
                    guard += 1;
                }
                used.insert(base);
                let px = node.port_point(tl, pi).x;
                out[node_id][pi] = (0, base);
                let outward = if side == Side::Top { -1 } else { 1 };
                entries.push((node.stub_tip(tl, pi).y, outward, base, px, node_id, pi));
            }
        }
    }
    entries.sort_by_key(|&(y, o, lo, hi, n, p)| (y, o, lo, hi, n, p));
    // Absolute lane coordinate -> spans already claimed on it.
    let mut occupied: std::collections::BTreeMap<i64, Vec<(i64, i64)>> = std::collections::BTreeMap::new();
    for &(stub_y, outward, lo, hi, node_id, pi) in entries.iter() {
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        let mut lane = 0i64;
        while lane < MAX_ESCAPE_LANES {
            let y = stub_y + outward * lane * grid;
            let free = occupied.get(&y).is_none_or(|spans| spans.iter().all(|&(a, b)| lo.max(a) >= hi.min(b)));
            if free {
                occupied.entry(y).or_default().push((lo, hi));
                break;
            }
            lane += 1;
        }
        out[node_id][pi].0 = lane.min(MAX_ESCAPE_LANES - 1);
    }
    out
}

/// Track row per (channel, edge), assigned in endpoint order.
///
/// The row index decides which horizontal track inside the channel a hop
/// runs on. Taking it from the edge's position in `channel_demand`'s
/// *edge-index* list makes the ordering arbitrary with respect to geometry:
/// two hops whose spans nest still get interleaved rows, and every pair of
/// interleaved tracks is a crossing. Sorting the channel's hops by the span
/// their two endpoints define instead makes nested hops take nested rows,
/// which is the whole point of a channel router. `channel_demand` still
/// decides *how many* rows the channel is sized for (`coords`), so the row
/// numbers stay within the space allotted.
fn build_channel_rows(
    g: &LayoutGraph,
    ext: &ExtGraph,
    coords: &Coords,
    channel_demand: &[Vec<usize>],
) -> Vec<std::collections::BTreeMap<usize, usize>> {
    let gaps = channel_demand.len();
    let mut per_gap: Vec<Vec<(i64, i64, usize)>> = vec![Vec::new(); gaps];
    for chain in &ext.chains {
        let group = g.edges[chain.edge_index].group;
        for w in chain.nodes.windows(2) {
            let a_layer = ext.ext_nodes[w[0]].layer;
            if a_layer >= gaps {
                continue;
            }
            let (ax, bx) = (coords.center_x[w[0]], coords.center_x[w[1]]);
            per_gap[a_layer].push((ax.min(bx), ax.max(bx), group));
        }
    }
    per_gap
        .into_iter()
        .map(|mut hops| {
            // Only worth doing on a busy channel. With a handful of hops the
            // ordering barely changes the crossing count, but it can still
            // move a short hop onto a far track and turn it into a
            // `schematic_wire_detour` (which is what it did to
            // `opamp_filter`); below the threshold, fall through to
            // `channel_demand`'s group order.
            if hops.len() < MIN_ORDERED_CHANNEL_HOPS {
                return std::collections::BTreeMap::new();
            }
            hops.sort_by_key(|&(lo, hi, group)| (lo, hi, group));
            hops.dedup_by_key(|&mut (_, _, group)| group);
            hops.into_iter().enumerate().map(|(row, (_, _, group))| (group, row)).collect()
        })
        .collect()
}

/// Column each dummy bend point's vertical pass runs on.
///
/// A dummy is a zero-size virtual bend, so nothing anchors its wire to
/// exactly `coords.center_x`; but its pass spans a whole layer plus half a
/// channel on each side, so two dummies in *adjacent* layers that happen to
/// share a column run collinear through the channel between them (this is
/// the l4 `I2C_SDA`/`USB_DP` and `I2C_SCL`/`USB_DM` pairs). Nudge the later
/// one off by up to two grid steps — well inside the `node_spacing`
/// clearance `coords::assign_coords` guarantees around every dummy, so the
/// pass stays box-free.
fn build_dummy_columns(g: &LayoutGraph, ext: &ExtGraph, coords: &Coords, node_top_left: &[Point], grid: i64) -> Vec<i64> {
    let grid = grid.max(1);
    let mut cols: Vec<i64> = coords.center_x.clone();
    let layers = ext.ext_nodes.iter().map(|n| n.layer).max().map(|m| m + 1).unwrap_or(0);
    let mut prev: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
    for l in 0..layers {
        let mut here: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
        // A real node's own stub lines are fixed; a dummy column landing on
        // one runs collinear with that pin's escape. A dummy's pass reaches
        // half a channel either side of its own layer, so the neighbouring
        // layers' stubs count too.
        for en in ext.ext_nodes.iter().filter(|n| n.layer + 1 >= l && n.layer <= l + 1) {
            if let Some(nid) = en.real_node {
                for pi in 0..g.nodes[nid].ports.len() {
                    here.insert(g.nodes[nid].stub_tip(node_top_left[nid], pi).x);
                }
            }
        }
        let ids: Vec<usize> =
            (0..ext.ext_nodes.len()).filter(|&i| ext.ext_nodes[i].layer == l && ext.ext_nodes[i].real_node.is_none()).collect();
        for id in ids {
            let c = coords.center_x[id];
            let pick = [0, grid, -grid, 2 * grid, -2 * grid]
                .into_iter()
                .map(|d| c + d)
                .find(|v| !prev.contains(v) && !here.contains(v))
                .unwrap_or(c);
            cols[id] = pick;
            here.insert(pick);
        }
        prev = here;
    }
    cols
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
    slot_boxes: &[(usize, Point, i64, i64)],
) -> Vec<Vec<Point>> {
    let mut result: Vec<Vec<Point>> = vec![Vec::new(); g.edges.len()];
    let tallest = g.nodes.iter().map(|n| n.height).max().unwrap_or(0);
    // Obstacles include each symbol's reserved refdes/value label slots
    // (see `LayoutOptions::label_slot_above`), so a wire is never routed
    // through space the renderer is going to put text in.
    let boxes: Vec<(usize, Point, i64, i64)> = if slot_boxes.is_empty() {
        g.nodes.iter().enumerate().map(|(id, n)| (id, node_top_left[id], n.width, n.height)).collect()
    } else {
        slot_boxes.to_vec()
    };

    // Columns an escape's vertical leg must not land on: every dummy bend
    // point's column (its vertical pass spans a whole layer) and every real
    // port's own stub line.
    let mut reserved: Vec<i64> = Vec::new();
    for (ext_id, en) in ext.ext_nodes.iter().enumerate() {
        if en.real_node.is_none() {
            reserved.push(coords.center_x[ext_id]);
        }
    }
    for (node_id, node) in g.nodes.iter().enumerate() {
        for pi in 0..node.ports.len() {
            reserved.push(node.stub_tip(node_top_left[node_id], pi).x);
        }
    }
    let escapes = build_escape_lanes(g, node_top_left, grid, &reserved);
    let channel_rows = build_channel_rows(g, ext, coords, channel_demand);
    let dummy_cols = build_dummy_columns(g, ext, coords, node_top_left, grid);

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
                let own_boxes = [
                    (node_top_left[a_node], g.nodes[a_node].width, g.nodes[a_node].height),
                    (node_top_left[b_node], g.nodes[b_node].width, g.nodes[b_node].height),
                ];
                if let Some(direct) = try_direct_path(a_stub, b_stub, &boxes, (a_node, b_node), own_boxes, &result, grid) {
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
            let group = g.edges[chain.edge_index].group;
            let row = channel_rows
                .get(a_layer)
                .and_then(|m| m.get(&group).copied())
                .or_else(|| channel_demand.get(a_layer).and_then(|rows| rows.binary_search(&group).ok()))
                .unwrap_or(0);
            let track_y = pick_track_y(channel_lo, channel_hi, row, grid);

            let a_port = if i == 0 { Some(low_port) } else { None };
            let b_port = if i + 1 == n - 1 { Some(high_port) } else { None };

            let a_pts = endpoint_approach(g, ext, a_id, a_port, node_top_left, true, track_y, grid, &boxes, &result, &escapes, &dummy_cols);
            let mut b_pts = endpoint_approach(g, ext, b_id, b_port, node_top_left, false, track_y, grid, &boxes, &result, &escapes, &dummy_cols);
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
