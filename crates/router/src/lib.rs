//! `eda-router` — a native grid-maze autorouter (v1), architecture-inspired
//! by FreeRouting but implemented from scratch against our own IR: no Java,
//! no DSN round-trip. Grid A* per net with rip-up & reroute on contention.
//!
//! See `route()` for the entry point and the module docs on `pads`, `grid`,
//! and `astar` for the pieces.

pub mod astar;
pub mod grid;
pub mod pads;

use eda_model::ir::{Design, Point, RoutingSection, Track, Via};
use eda_model::{CheckResult, ConstraintModel};
use grid::{Grid, Occ};
use std::collections::{HashMap, HashSet, VecDeque};

/// Routing rules are the model's board rules; re-exported under the name
/// the router API has always used.
pub use eda_model::BoardRules as RouteRules;

/// A "REF.PIN"-addressed pad and its net (or a synthetic private net for
/// unassigned pins, so they still act as physical obstacles).
struct PadInfo {
    pt: Point,
    size: (i64, i64),
    net: String,
    /// Copper layers this pad exists on.
    layers: Vec<u8>,
}

/// Grid cells whose centre lies strictly inside the pad's copper, so a
/// track ending on any of them is guaranteed to touch the pad. Computed
/// from the real rectangle: a pad centre is rarely grid-aligned.
fn pad_interior_cells(grid: &Grid, pad: &PadInfo, rules: &RouteRules) -> Vec<(i64, i64)> {
    let (cx, cy) = grid.to_cell(pad.pt);
    let hx = (pad.size.0 / 2 + rules.grid) / rules.grid;
    let hy = (pad.size.1 / 2 + rules.grid) / rules.grid;
    let (x0, x1) = (pad.pt.x - pad.size.0 / 2, pad.pt.x + pad.size.0 / 2);
    let (y0, y1) = (pad.pt.y - pad.size.1 / 2, pad.pt.y + pad.size.1 / 2);
    let mut out = vec![(cx, cy)];
    for dx in -hx..=hx {
        for dy in -hy..=hy {
            let p = grid.to_point(cx + dx, cy + dy);
            if p.x > x0 && p.x < x1 && p.y > y0 && p.y < y1 && (dx, dy) != (0, 0) {
                out.push((cx + dx, cy + dy));
            }
        }
    }
    out
}

/// One routing step: connect `a_pin` to the net's already-connected
/// copper (pads of earlier steps plus every track/via routed so far);
/// `b_pin` is the nearest earlier pin, used for the heuristic and for
/// rip-up region selection. Steps come in Prim (nearest-first) order, so
/// the net grows as a spanning tree instead of a star out of one pad.
struct Edge {
    a_pin: String,
    b_pin: String,
}

/// Grid cell + physical layer of a pad, as an A* endpoint.
type CellLayer = (i64, i64, u8);
/// `route_net`'s failure payload: the (start, goal) of the edge that
/// couldn't be routed, for rip-up victim selection.
type EdgeFailure = (CellLayer, CellLayer);

pub fn route(
    design: &Design,
    model: &ConstraintModel,
    rules: &RouteRules,
    seed: u64,
) -> Result<Design, Vec<CheckResult>> {
    match route_partial(design, model, rules, seed) {
        (Some(d), fails) if fails.is_empty() => Ok(d),
        (_, fails) => Err(fails),
    }
}

/// Like [`route`], but on failure also returns whatever was routed (for
/// review/rendering). `None` design means a precondition failed before
/// routing started. `route` is the gated contract; use this for diagnostics.
pub fn route_partial(
    design: &Design,
    model: &ConstraintModel,
    rules: &RouteRules,
    seed: u64,
) -> (Option<Design>, Vec<CheckResult>) {
    let placement = match &design.placement {
        Some(p) => p,
        None => {
            return (None, vec![CheckResult::fail(
                "route_precondition",
                "design",
                "placement section is required before routing",
            )])
        }
    };
    if placement.outline.len() < 3 {
        return (None, vec![CheckResult::fail(
            "route_precondition",
            "design.placement.outline",
            "board outline needs at least 3 points",
        )]);
    }

    // --- 1. pad geometry -----------------------------------------------
    let num_layers = rules.layers.len().max(1);
    let mut pads: HashMap<String, PadInfo> = HashMap::new();
    let mut precondition: Vec<CheckResult> = Vec::new();
    for fp in &placement.footprints {
        let Some(part) = model.part(&fp.id) else {
            precondition.push(CheckResult::fail("route_precondition", &fp.id, "placed footprint has no part in the model"));
            continue;
        };
        // SMD pads exist only on their side's outer copper layer; through
        // hole pads exist on every layer. The via cost in A* is what makes
        // hopping layers around a same-side obstacle a deliberate choice.
        let side_layer = match fp.side {
            eda_model::ir::Side::Top => 0u8,
            eda_model::ir::Side::Bottom => (num_layers - 1) as u8,
        };
        let Some(geoms) = pads::pad_positions(model, part, fp) else {
            precondition.push(CheckResult::fail(
                "route_precondition",
                &fp.id,
                format!(
                    "no footprint geometry for part (footprint={:?}, package={:?}); define it in `footprints` or use a built-in package name",
                    part.footprint, part.package
                ),
            ));
            continue;
        };
        for g in geoms {
            let refpin = format!("{}.{}", fp.id, g.number);
            let net = model
                .nets
                .iter()
                .find(|n| n.pins.iter().any(|p| p == &refpin))
                .map(|n| n.name.clone())
                .unwrap_or_else(|| format!("__unassigned__{refpin}"));
            let layers = if g.through_hole { (0..num_layers as u8).collect() } else { vec![side_layer] };
            pads.insert(refpin, PadInfo { pt: g.center, size: g.size, net, layers });
        }
    }
    if !precondition.is_empty() {
        return (None, precondition);
    }

    // --- 2. grid & obstacle map -----------------------------------------
    let mut grid = Grid::with_widths(placement.outline.clone(), rules.grid, rules.clearance, rules.track_width, rules.via_diameter, num_layers);
    for pad in pads.values() {
        let (cx, cy) = grid.to_cell(pad.pt);
        // Rasterise the pad area: every cell whose own square touches the
        // pad rectangle, so the pad edge never lies more than half a cell
        // past an occupied cell centre (the invariant `Grid` clearance
        // arithmetic relies on).
        let hx = (pad.size.0 / 2 + rules.grid / 2) / rules.grid;
        let hy = (pad.size.1 / 2 + rules.grid / 2) / rules.grid;
        for &l in &pad.layers {
            for dx in -hx..=hx {
                for dy in -hy..=hy {
                    grid.set(cx + dx, cy + dy, l, &pad.net, Occ::Pad);
                }
            }
        }
    }

    // --- 3. net -> star edges --------------------------------------------
    let mut edges_by_net: HashMap<String, Vec<Edge>> = HashMap::new();
    let mut airline_by_net: HashMap<String, i64> = HashMap::new();
    let mut skipped: Vec<CheckResult> = Vec::new();

    for net in &model.nets {
        let mut resolved: Vec<&String> = Vec::new();
        for p in &net.pins {
            if pads.contains_key(p) {
                resolved.push(p);
            } else {
                skipped.push(CheckResult::fail(
                    "route_pin_missing",
                    format!("{}/{}", net.name, p),
                    "pin has no resolvable pad (unknown footprint/part)",
                ));
            }
        }
        if resolved.len() < 2 {
            continue;
        }
        // Prim's MST over pad centres (Manhattan), deterministic: ties go
        // to the lexically smaller pin ref.
        let mut es = Vec::new();
        let mut total = 0i64;
        let mut in_tree: Vec<&String> = vec![resolved[0]];
        let mut rest: Vec<&String> = resolved[1..].to_vec();
        while !rest.is_empty() {
            let mut best: Option<(i64, usize, &String)> = None;
            for (ri, r) in rest.iter().enumerate() {
                for t in &in_tree {
                    let (a, b) = (&pads[*r].pt, &pads[*t].pt);
                    let d = (a.x - b.x).abs() + (a.y - b.y).abs();
                    let better = match best {
                        None => true,
                        Some((bd, bri, bt)) => d < bd || (d == bd && (r.as_str(), t.as_str()) < (rest[bri].as_str(), bt.as_str())),
                    };
                    if better {
                        best = Some((d, ri, t));
                    }
                }
            }
            let (d, ri, t) = best.unwrap();
            let r = rest.remove(ri);
            total += d;
            es.push(Edge { a_pin: r.clone(), b_pin: t.clone() });
            in_tree.push(r);
        }
        airline_by_net.insert(net.name.clone(), total);
        edges_by_net.insert(net.name.clone(), es);
    }

    // --- 4. deterministic net ordering: shortest airline first, seed
    // shuffles only within ties -------------------------------------------
    let mut nets: Vec<String> = edges_by_net.keys().cloned().collect();
    nets.sort_by(|a, b| (airline_by_net[a], a).cmp(&(airline_by_net[b], b)));
    shuffle_ties(&mut nets, &airline_by_net, seed);

    // --- 5. route with rip-up & reroute -----------------------------------
    let mut queue: VecDeque<String> = nets.into();
    let mut ripup_rounds: HashMap<String, u32> = HashMap::new();
    let mut routed_order: Vec<String> = Vec::new();
    let mut tracks: HashMap<String, Vec<Track>> = HashMap::new();
    let mut vias: HashMap<String, Vec<Via>> = HashMap::new();
    let mut fails: Vec<CheckResult> = skipped;
    let mut permanently_failed: HashSet<String> = HashSet::new();

    let iter_budget = 10 * queue.len().max(1) + 50;
    let mut iters = 0usize;

    while let Some(net) = queue.pop_front() {
        iters += 1;
        if iters > iter_budget {
            fails.push(CheckResult::fail("route_net_unrouted", &net, "router iteration budget exceeded"));
            continue;
        }
        if permanently_failed.contains(&net) {
            continue;
        }

        match route_net(&net, &edges_by_net[&net], &pads, &mut grid, rules) {
            Ok((net_tracks, net_vias)) => {
                tracks.insert(net.clone(), net_tracks);
                vias.insert(net.clone(), net_vias);
                routed_order.retain(|n| n != &net);
                routed_order.push(net.clone());
            }
            Err(failure_region) => {
                let rounds = *ripup_rounds.get(&net).unwrap_or(&0);
                let margin = 4;
                let (fa, fb) = failure_region;
                let victims: Vec<String> = grid
                    .nets_in_region((fa.0, fa.1), (fb.0, fb.1), margin)
                    .into_iter()
                    .filter(|n| n != &net && routed_order.contains(n))
                    .collect();
                // Most-recently-routed first, cap at 3.
                let mut victims: Vec<String> = victims;
                victims.sort_by_key(|n| std::cmp::Reverse(routed_order.iter().position(|r| r == n).unwrap_or(0)));
                victims.truncate(3);

                if rounds < 2 && !victims.is_empty() {
                    ripup_rounds.insert(net.clone(), rounds + 1);
                    for v in &victims {
                        grid.clear_net(v);
                        tracks.remove(v);
                        vias.remove(v);
                        routed_order.retain(|n| n != v);
                        queue.push_back(v.clone());
                    }
                    // Retry this net after its blockers are cleared.
                    queue.push_front(net.clone());
                } else {
                    permanently_failed.insert(net.clone());
                    fails.push(CheckResult::fail(
                        "route_net_unrouted",
                        &net,
                        "grid maze router could not find a path after rip-up budget was exhausted",
                    ));
                }
            }
        }
    }

    // --- 6. assemble output design ----------------------------------------
    let mut out = design.clone();
    let mut all_tracks: Vec<Track> = tracks.into_values().flatten().collect();
    let mut all_vias: Vec<Via> = vias.into_values().flatten().collect();
    all_tracks.sort_by(|a, b| (&a.net, &a.layer, a.pts.first()).cmp(&(&b.net, &b.layer, b.pts.first())));
    all_vias.sort_by(|a, b| (&a.net, a.at).cmp(&(&b.net, b.at)));
    out.routing = Some(RoutingSection { tracks: all_tracks, vias: all_vias, zones: vec![] });

    // Contract: `route` fails loudly on any unrouted net; the partial
    // design is only exposed through `route_partial` for diagnostics.
    (Some(out), fails)
}

/// Attempts to route every star edge of one net. On success returns the new
/// tracks/vias for the net. On failure returns the start/goal grid cells of
/// the edge that failed, for rip-up victim selection.
fn route_net(
    net: &str,
    edges: &[Edge],
    pads: &HashMap<String, PadInfo>,
    grid: &mut Grid,
    rules: &RouteRules,
) -> Result<(Vec<Track>, Vec<Via>), EdgeFailure> {
    let mut tracks = Vec::new();
    let mut vias = Vec::new();

    // Pins connected so far (their pad cells are valid goals).
    let mut connected: Vec<&String> = Vec::new();
    if let Some(first) = edges.first() {
        connected.push(&first.b_pin);
    }
    for e in edges {
        let pa = &pads[&e.a_pin];
        let pb = &pads[&e.b_pin];
        let (ax, ay) = grid.to_cell(pa.pt);
        let (bx, by) = grid.to_cell(pb.pt);
        let start = (ax, ay, pa.layers[0]);
        let goal = (bx, by, pb.layers[0]);

        // Goal set: routed copper of this net plus the pads of connected pins.
        let mut goals: HashSet<(i64, i64, u8)> = grid.routed_cells_of(net).into_iter().collect();
        let mut h_targets: Vec<(i64, i64)> = Vec::new();
        for c in &connected {
            let pc = &pads[*c];
            let (cx, cy) = grid.to_cell(pc.pt);
            for &l in &pc.layers {
                for (gx, gy) in pad_interior_cells(grid, pc, rules) {
                    goals.insert((gx, gy, l));
                }
            }
            h_targets.push((cx, cy));
        }
        if h_targets.is_empty() {
            h_targets.push((bx, by));
        }
        // Never terminate inside the start pad itself.
        let shx = (pa.size.0 / 2 + rules.grid / 2) / rules.grid;
        let shy = (pa.size.1 / 2 + rules.grid / 2) / rules.grid;
        for &l in &pa.layers {
            for dx in -shx..=shx {
                for dy in -shy..=shy {
                    goals.remove(&(ax + dx, ay + dy, l));
                }
            }
        }

        match astar::route_to_any(grid, net, start, &goals, &h_targets) {
            Some(path) => {
                connected.push(&e.a_pin);
                // Mark cells so later edges of the same star net see this
                // one as routed (same net, so still passable for them).
                for (i, &(cx, cy, l)) in path.iter().enumerate() {
                    let is_via = (i > 0 && path[i - 1].2 != l) || (i + 1 < path.len() && path[i + 1].2 != l);
                    grid.set(cx, cy, l, net, if is_via { Occ::Via } else { Occ::Track });
                }
                // Which connected pin (if any) does the path end on?
                let end = path.last().copied().unwrap_or(start);
                let end_pin: Option<&String> = connected.iter().copied().find(|c| {
                    let pc = &pads[*c];
                    pc.layers.contains(&end.2) && pad_interior_cells(grid, pc, rules).contains(&(end.0, end.1))
                });
                let (edge_tracks, edge_vias) = path_to_geometry(net, &e.a_pin, end_pin.map(|s| s.as_str()).unwrap_or(""), &path, grid, rules);
                tracks.extend(edge_tracks);
                vias.extend(edge_vias);
            }
            None => {
                if std::env::var_os("EDA_ROUTE_DEBUG").is_some() {
                    eprintln!("route: net {net} pin {} -> nearest {} failed; start {:?} goals {} targets {:?}", e.a_pin, e.b_pin, start, goals.len(), h_targets);
                    for l in 0..grid.num_layers as u8 {
                        eprintln!("layer {l} around start (passable-as-track: P):\n{}", grid.dump_around(start.0, start.1, l, net, 8));
                        let mut row = String::new();
                        for dy in -8..=8i64 {
                            for dx in -8..=8i64 {
                                row.push(if grid.passable(start.0 + dx, start.1 + dy, l, net) { 'P' } else { '.' });
                            }
                            row.push('\n');
                        }
                        eprintln!("{row}");
                    }
                }
                return Err((start, goal));
            }
        }
    }
    Ok((tracks, vias))
}

fn path_to_geometry(
    net: &str,
    a_pin: &str,
    b_pin: &str,
    path: &[(i64, i64, u8)],
    grid: &Grid,
    rules: &RouteRules,
) -> (Vec<Track>, Vec<Via>) {
    let mut tracks = Vec::new();
    let mut vias = Vec::new();

    // Split into same-layer runs, inserting a via at each layer change.
    let mut run_start = 0usize;
    for i in 1..path.len() {
        if path[i].2 != path[i - 1].2 {
            let span = SegmentSpan { path, start: run_start, end: i - 1, a_pin, b_pin };
            tracks.push(segment_track(net, &span, grid, rules));
            let (vx, vy, from_l) = path[i - 1];
            let to_l = path[i].2;
            let at = grid.to_point(vx, vy);
            vias.push(Via {
                net: net.to_string(),
                at,
                drill: rules.via_drill,
                diameter: rules.via_diameter,
                from_layer: rules.layers[from_l as usize].clone(),
                to_layer: rules.layers[to_l as usize].clone(),
            });
            run_start = i;
        }
    }
    let span = SegmentSpan { path, start: run_start, end: path.len() - 1, a_pin, b_pin };
    tracks.push(segment_track(net, &span, grid, rules));
    (tracks, vias)
}

/// One same-layer run of a path, plus the pin refs to attach at its ends
/// (only set when that end is the very start/end of the whole path).
struct SegmentSpan<'a> {
    path: &'a [(i64, i64, u8)],
    start: usize,
    end: usize,
    a_pin: &'a str,
    b_pin: &'a str,
}

fn segment_track(net: &str, span: &SegmentSpan, grid: &Grid, rules: &RouteRules) -> Track {
    let SegmentSpan { path, start, end, a_pin, b_pin } = *span;
    let mut pts: Vec<Point> = Vec::new();
    for &(cx, cy, _) in &path[start..=end] {
        let p = grid.to_point(cx, cy);
        if let (Some(&prev2), Some(&prev1)) = (pts.get(pts.len().wrapping_sub(2)), pts.last()) {
            // Collinear merge: drop prev1 if prev2->prev1->p is a straight line.
            if is_collinear(prev2, prev1, p) {
                pts.pop();
            }
        }
        pts.push(p);
    }
    let layer = rules.layers[path[start].2 as usize].clone();
    let mut pins = Vec::new();
    if start == 0 {
        pins.push(a_pin.to_string());
    }
    if end == path.len() - 1 && !b_pin.is_empty() {
        pins.push(b_pin.to_string());
    }
    Track { net: net.to_string(), pins, layer, width: rules.track_width, pts }
}

fn is_collinear(a: Point, b: Point, c: Point) -> bool {
    (b.x - a.x) as i128 * (c.y - a.y) as i128 == (b.y - a.y) as i128 * (c.x - a.x) as i128
}

/// splitmix64, used only to permute equal-airline-length net groups.
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn shuffle_ties(nets: &mut [String], airline: &HashMap<String, i64>, seed: u64) {
    let mut i = 0;
    while i < nets.len() {
        let mut j = i + 1;
        while j < nets.len() && airline[&nets[j]] == airline[&nets[i]] {
            j += 1;
        }
        if j - i > 1 {
            let group = &mut nets[i..j];
            // Deterministic Fisher-Yates keyed off seed + group content.
            let mut state = seed ^ splitmix64(i as u64);
            for k in (1..group.len()).rev() {
                state = splitmix64(state);
                let swap_with = (state as usize) % (k + 1);
                group.swap(k, swap_with);
            }
        }
        i = j;
    }
}
